extern crate fuse_sys;
extern crate libc;

use std::default::Default;
use std::env;
use std::ffi::{self, CStr, CString, OsStr, OsString};
use std::fs::{self, File};
use std::iter::{IntoIterator, Iterator};
use std::io::{self, Read, Seek, SeekFrom};
use std::mem;
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::str;

use libc::{EACCES, EINVAL, EIO, EISDIR, ENOENT, EREMOTEIO};

use fuse_sys::{fuse_conn_info, fuse_file_info, fuse_fill_dir_t,
               fuse_operations, mode_t, off_t};

enum FusionFSError {
  ErrnoError(c_int),
}

type FusionFSResult<T> = Result<T, FusionFSError>;

impl From<str::Utf8Error> for FusionFSError {
  fn from(error: str::Utf8Error) -> Self {
    FusionFSError::ErrnoError(EINVAL)
  }
}

impl From<io::Error> for FusionFSError {
  fn from(error: io::Error) -> Self {
    FusionFSError::ErrnoError(EREMOTEIO)
  }
}

impl From<ffi::NulError> for FusionFSError {
  fn from(error: ffi::NulError) -> Self {
    FusionFSError::ErrnoError(EIO)
  }
}

// fuse_main() is implemented a C preprocessor macro, so we redefine it here as
// a rust function, copying over the relevant part of the source.
/**
 * Main function of FUSE.
 *
 * This is for the lazy.  This is all that has to be called from the
 * main() function.
 *
 * This function does the following:
 *   - parses command line options (-d -s and -h)
 *   - passes relevant mount options to the fuse_mount()
 *   - installs signal handlers for INT, HUP, TERM and PIPE
 *   - registers an exit handler to unmount the filesystem on program exit
 *   - creates a fuse handle
 *   - registers the operations
 *   - calls either the single-threaded or the multi-threaded event loop
 *
 * Note: this is currently implemented as a macro.
 *
 * @param argc the argument counter passed to the main() function
 * @param argv the argument vector passed to the main() function
 * @param op the file system operation
 * @param user_data user data supplied in the context during the init() method
 * @return 0 on success, nonzero on failure
 */
/*
  int fuse_main(int argc, char *argv[], const struct fuse_operations *op,
  void *user_data);
 */
/*
#define fuse_main(argc, argv, op, user_data)				\
        fuse_main_real(argc, argv, op, sizeof(*(op)), user_data)
*/
unsafe fn fuse_main_wrapper(
  argc: c_int,
  argv: *mut *mut c_char,
  op: *const fuse_sys::fuse_operations,
) -> c_int {
  fuse_sys::fuse_main_real(
    argc,
    argv,
    op,
    mem::size_of_val(&*op),
    ptr::null_mut(),
  )
}

unsafe fn from_c_string<'a>(
  s: *const c_char,
) -> Result<&'a str, str::Utf8Error> {
  CStr::from_ptr(s).to_str()
}

unsafe fn into_c_string_vec(args: Vec<&str>) -> Vec<*mut c_char> {
  args
    .into_iter()
    .map(|x| CString::new(x).unwrap().into_raw())
    .collect::<Vec<*mut c_char>>()
}

struct MyFS {
  src_dir: *const PathBuf,
}

static mut MY_FS: MyFS = MyFS {
  src_dir: ptr::null_mut(),
};

unsafe fn zero_stat_buf<'a>(
  stat_ptr: *mut fuse_sys::stat,
) -> &'a mut fuse_sys::stat {
  ptr::write_bytes(stat_ptr, 0, 1);
  &mut *stat_ptr
}

unsafe fn get_rel_path(
  relpath: *const c_char,
) -> Result<PathBuf, FusionFSError> {
  let rel_path_with_slash = from_c_string(relpath)?;

  if !rel_path_with_slash.starts_with("/") {
    return Err(FusionFSError::ErrnoError(ENOENT));
  }

  let rel_path = &rel_path_with_slash[1..];

  let src_dir = &*MY_FS.src_dir;
  Ok(src_dir.join(rel_path))
}

fn getattr_dir(
  pb: PathBuf,
  stat: &mut fuse_sys::stat,
) -> Result<c_int, FusionFSError> {
  let dir_data = fs::metadata(pb)?;
  stat.st_mode = dir_data.mode() as mode_t;
  stat.st_nlink = dir_data.nlink();
  Ok(0)
}

fn getattr_file(
  pb: PathBuf,
  stat: &mut fuse_sys::stat
) -> Result<c_int, FusionFSError> {
  let file_data = fs::metadata(pb)?;
  stat.st_mode = file_data.mode() as mode_t;
  stat.st_nlink = file_data.nlink();
  stat.st_size = file_data.size() as off_t;
  Ok(0)
}

fn do_getattr(
  pb: PathBuf,
  mut stbuf: &mut fuse_sys::stat,
) -> Result<c_int, FusionFSError> {
  if pb.is_dir() {
    getattr_dir(pb, &mut stbuf)
  } else if pb.is_file() {
    getattr_file(pb, &mut stbuf)
  } else {
    Err(FusionFSError::ErrnoError(ENOENT))
  }
}

unsafe extern "C" fn hello_getattr(
  path_c_str: *const c_char,
  stbuf_ptr: *mut fuse_sys::stat,
) -> c_int {
  let mut stbuf = zero_stat_buf(stbuf_ptr);

  let resulting_path = match get_rel_path(path_c_str) {
    Ok(res_path) => res_path,
    Err(FusionFSError::ErrnoError(ec)) => return -ec,
  };

  match do_getattr(resulting_path, &mut stbuf) {
    Ok(rc) => rc,
    Err(FusionFSError::ErrnoError(ec)) => return -ec,
  }
}

fn get_source_readdir(pb: PathBuf) -> FusionFSResult<Vec<String>> {
  if !pb.is_dir() {
    return Err(FusionFSError::ErrnoError(EINVAL));
  }

  let source_readdir = fs::read_dir(pb)?;

  let mut source_paths = source_readdir
    .map(|dir_result| {
      // FIXME: too much ownership nonsense here, no proper error handling
      let entry_path: &PathBuf = &dir_result.unwrap().path();
      let fname: &OsStr = entry_path.file_name().unwrap();
      fname.to_os_string().into_string().unwrap()
    })
    .collect::<Vec<String>>();

  source_paths.push(String::from(".."));
  source_paths.push(String::from("."));

  Ok(source_paths)
}

unsafe fn fill_dir(
  buf: &mut c_void,
  paths: Vec<String>,
  filler_ptr: fuse_fill_dir_t,
) -> FusionFSResult<c_int> {
  let filler_fn = match filler_ptr {
    None => return Err(FusionFSError::ErrnoError(EINVAL)),
    Some(f) => f,
  };

  for p in paths {
    let c_str = CString::new(p.as_str())?;
    // TODO: check the bindings for docs on the return value of this function
    filler_fn(buf, c_str.as_ptr(), ptr::null_mut(), 0);
  }

  Ok(0)
}

unsafe extern "C" fn hello_readdir(
  path_c_str: *const c_char,
  buf: *mut c_void,
  filler_ptr: fuse_fill_dir_t,
  offset: off_t,
  fi: *mut fuse_file_info,
) -> c_int {
  let resulting_path = match get_rel_path(path_c_str) {
    Ok(res) => res,
    Err(FusionFSError::ErrnoError(ec)) => return -ec,
  };

  let source_paths = match get_source_readdir(resulting_path) {
    Ok(path_vec) => path_vec,
    Err(FusionFSError::ErrnoError(ec)) => return -ec,
  };

  match fill_dir(&mut *buf, source_paths, filler_ptr) {
    Ok(rc) => rc,
    Err(FusionFSError::ErrnoError(ec)) => return -ec,
  }
}

fn do_open(pb: PathBuf, fi: &mut fuse_file_info) -> FusionFSResult<c_int> {
  if pb.is_dir() {
    return Err(FusionFSError::ErrnoError(EISDIR));
  }
  if !pb.is_file() {
    return Err(FusionFSError::ErrnoError(ENOENT));
  }

  let access: u32 = (fi.flags as u32) & fuse_sys::O_ACCMODE;
  if access == fuse_sys::O_RDONLY {
    Ok(0)
  } else {
    Err(FusionFSError::ErrnoError(EACCES))
  }
}

unsafe extern "C" fn hello_open(
  path_c_str: *const c_char,
  fi_ptr: *mut fuse_file_info,
) -> c_int {
  let resulting_path = match get_rel_path(path_c_str) {
    Ok(res) => res,
    Err(FusionFSError::ErrnoError(ec)) => return -ec,
  };

  match do_open(resulting_path, &mut *fi_ptr) {
    Ok(rc) => rc,
    Err(FusionFSError::ErrnoError(ec)) => return -ec,
  }
}

unsafe fn do_read(
  pb: PathBuf,
  buf: *mut c_char,
  size: usize,
  offset: off_t,
) -> FusionFSResult<c_int> {
  if pb.is_dir() {
    return Err(FusionFSError::ErrnoError(EISDIR));
  }
  if !pb.is_file() {
    return Err(FusionFSError::ErrnoError(ENOENT));
  }

  if offset < 0 {
    return Err(FusionFSError::ErrnoError(EINVAL));
  }

  let mut target_file = File::open(pb)?;
  target_file.seek(SeekFrom::Start(offset as u64));

  let dest_slice = slice::from_raw_parts_mut(buf as *mut u8, size);

  let bytes_read = target_file.read(dest_slice)?;

  Ok(bytes_read as c_int)
}

unsafe extern "C" fn hello_read(
  path_c_str: *const c_char,
  buf: *mut c_char,
  size: usize,
  offset: off_t,
  fi: *mut fuse_file_info,
) -> c_int {
  let resulting_path = match get_rel_path(path_c_str) {
    Ok(res) => res,
    Err(FusionFSError::ErrnoError(ec)) => return -ec,
  };

  match do_read(resulting_path, buf, size, offset) {
    Ok(rc) => rc,
    Err(FusionFSError::ErrnoError(ec)) => return -ec,
  }
}

fn main() {
  let args_string = env::args().collect::<Vec<String>>();
  let args = args_string
    .iter()
    .map(|s| s.as_str())
    .collect::<Vec<&str>>();
  if args.len() != 3 {
    panic!(
      "we need a mountpoint AND a source lol (args: {:?})",
      args
    );
  }
  let exe = args[0];
  let mp = args[1];
  let src = args[2];
  if !Path::new(mp).is_dir() {
    panic!("no mount dir bro (was: {:?})", mp);
  }
  let src_dir = PathBuf::from(src);
  if !src_dir.is_dir() {
    panic!("no source dir broskimo (was: {:?})", src);
  }

  let fuse_args: Vec<&str> =
    vec![exe, "-o", "ro", "-o", "fsname=myfs", "-d", mp];

  unsafe {
    MY_FS.src_dir = &src_dir;

    let hello_oper: fuse_operations = fuse_operations {
      getattr: Some(hello_getattr),
      readdir: Some(hello_readdir),
      open: Some(hello_open),
      read: Some(hello_read),
      ..Default::default()
    };

    let mut c_fuse_args = into_c_string_vec(fuse_args);

    fuse_main_wrapper(
      *&c_fuse_args.len() as c_int,
      *&c_fuse_args.as_mut_ptr(),
      &hello_oper,
    );

    for opt in c_fuse_args {
      let _ = CString::from_raw(opt);
    }
  }
}
