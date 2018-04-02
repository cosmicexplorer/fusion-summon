extern crate fuse_sys;
extern crate libc;

use std::default::Default;
use std::env;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fs::{self, File};
use std::iter::{IntoIterator, Iterator};
use std::io::{Read, Seek, SeekFrom};
use std::mem;
use std::os::raw::{c_char, c_int, c_void};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::str;

use libc::{EACCES, EINVAL, EISDIR, ENOENT};

use fuse_sys::{fuse_conn_info, fuse_file_info, fuse_fill_dir_t,
               fuse_operations, mode_t, off_t};

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

unsafe fn from_c_string<'a>(s: *const c_char) -> &'a str {
  CStr::from_ptr(s).to_str().unwrap()
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

unsafe extern "C" fn hello_getattr(
  path_c_str: *const c_char,
  stbuf_ptr: *mut fuse_sys::stat,
) -> c_int {
  let stbuf = zero_stat_buf(stbuf_ptr);

  let rel_path_with_slash = from_c_string(path_c_str);

  if !rel_path_with_slash.starts_with("/") {
    return -ENOENT;
  }

  let rel_path = &rel_path_with_slash[1..];

  let resulting_path = (&*MY_FS.src_dir).join(rel_path);

  if resulting_path.is_dir() {
    let dir_data = fs::metadata(resulting_path).unwrap();
    stbuf.st_mode = dir_data.mode() as mode_t;
    stbuf.st_nlink = dir_data.nlink();
    0
  } else if resulting_path.is_file() {
    let file_data = fs::metadata(resulting_path).unwrap();
    stbuf.st_mode = file_data.mode() as mode_t;
    stbuf.st_nlink = file_data.nlink();
    stbuf.st_size = file_data.size() as off_t;
    0
  } else {
    -ENOENT
  }
}

unsafe extern "C" fn hello_readdir(
  path_c_str: *const c_char,
  buf: *mut c_void,
  filler_ptr: fuse_fill_dir_t,
  offset: off_t,
  fi: *mut fuse_file_info,
) -> c_int {
  let rel_path_with_slash = from_c_string(path_c_str);

  if !rel_path_with_slash.starts_with("/") {
    return -ENOENT;
  }

  let rel_path = &rel_path_with_slash[1..];

  let resulting_path = (&*MY_FS.src_dir).join(rel_path);

  if !resulting_path.is_dir() {
    return -ENOENT;
  }

  let filler_fn = filler_ptr.unwrap();

  let source_readdir = fs::read_dir(resulting_path).unwrap();

  let mut source_paths = source_readdir
    .map(|dir_result| {
      // FIXME: too much ownership nonsense here
      let entry_path: &PathBuf = &dir_result.unwrap().path();
      let fname: &OsStr = entry_path.file_name().unwrap();
      fname.to_os_string().into_string().unwrap()
    })
    .collect::<Vec<String>>();

  source_paths.push(String::from(".."));
  source_paths.push(String::from("."));

  for s in source_paths {
    let c_str = CString::new(s.as_str()).unwrap();
    filler_fn(buf, c_str.as_ptr(), ptr::null_mut(), 0);
  }

  0
}

unsafe extern "C" fn hello_open(
  path_c_str: *const c_char,
  fi_ptr: *mut fuse_file_info,
) -> c_int {
  let rel_path_with_slash = from_c_string(path_c_str);

  if !rel_path_with_slash.starts_with("/") {
    return -ENOENT;
  }

  let rel_path = &rel_path_with_slash[1..];

  let resulting_path = (&*MY_FS.src_dir).join(rel_path);

  if resulting_path.is_dir() {
    return -EISDIR;
  }

  if !resulting_path.is_file() {
    return -ENOENT;
  }

  let fi = *fi_ptr;
  let access: u32 = (fi.flags as u32) & fuse_sys::O_ACCMODE;
  if access != fuse_sys::O_RDONLY {
    return -EACCES;
  }

  0
}

unsafe extern "C" fn hello_read(
  path_c_str: *const c_char,
  buf: *mut c_char,
  size: usize,
  offset: off_t,
  fi: *mut fuse_file_info,
) -> c_int {
  let rel_path_with_slash = from_c_string(path_c_str);

  if !rel_path_with_slash.starts_with("/") {
    return -ENOENT;
  }

  let rel_path = &rel_path_with_slash[1..];

  let resulting_path = (&*MY_FS.src_dir).join(rel_path);

  if resulting_path.is_dir() {
    return -EISDIR;
  }

  if !resulting_path.is_file() {
    return -ENOENT;
  }

  if offset < 0 {
    return -EINVAL;
  }

  let off: usize = offset as usize;

  let mut target_file = File::open(resulting_path).unwrap();
  target_file.seek(SeekFrom::Start(off as u64));

  let dest_slice = slice::from_raw_parts_mut(buf as *mut u8, size);

  target_file.read(dest_slice).unwrap() as c_int
}

fn main() {
  let args_string = env::args().collect::<Vec<String>>();
  let args = args_string
    .iter()
    .map(|s| s.as_str())
    .collect::<Vec<&str>>();
  if args.len() != 3 {
    panic!("we need a mountpoint AND a source lol");
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
