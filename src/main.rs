extern crate fuse_sys;
extern crate libc;

use std::default::Default;
use std::env;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::iter::{IntoIterator, Iterator};
use std::mem;
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::str;

use libc::{EACCES, EINVAL, ENOENT};

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

struct MyFS<'a> {
  filename: &'a str,
  content: &'a str,
}

static mut MY_FS: MyFS = MyFS {
  filename: "",
  content: "",
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
  let path = from_c_string(path_c_str);

  if path == "/" {
    stbuf.st_mode = (fuse_sys::S_IFDIR | 0o755) as mode_t;
    stbuf.st_nlink = 2;
    0
  } else if path == format!("/{}", MY_FS.filename) {
    stbuf.st_mode = (fuse_sys::S_IFREG | 0o444) as mode_t;
    stbuf.st_nlink = 1;
    stbuf.st_size = MY_FS.content.len() as off_t;
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
  let path = CStr::from_ptr(path_c_str).to_str().unwrap();
  if path != "/" {
    return -ENOENT;
  }

  let filler_fn = filler_ptr.unwrap();

  let entries = [".", "..", MY_FS.filename];

  for &s in entries.iter() {
    let cur_str = CString::new(s).unwrap();
    filler_fn(buf, cur_str.as_ptr(), ptr::null_mut(), 0);
  }

  0
}

unsafe extern "C" fn hello_open(
  path_c_str: *const c_char,
  fi_ptr: *mut fuse_file_info,
) -> c_int {
  let path = from_c_string(path_c_str);

  if path != format!("/{}", MY_FS.filename) {
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
  let path = from_c_string(path_c_str);

  if &path[1..] != MY_FS.filename {
    return -ENOENT;
  }

  let len = MY_FS.content.len();

  if offset < 0 {
    return -EINVAL;
  }

  let off: usize = offset as usize;

  if off >= len {
    0
  } else {
    let adj_size = if (off + size) > len {
      len - off
    } else {
      size
    };

    let src_vec = &MY_FS
      .content
      .as_bytes()
      .iter()
      .map(|&b| b as c_char)
      .collect::<Vec<c_char>>();
    let src_slice = &src_vec.as_slice();

    let dest_slice = slice::from_raw_parts_mut(buf, adj_size);

    dest_slice.copy_from_slice(&src_slice[off..(off + adj_size)]);

    adj_size as c_int
  }
}

fn main() {
  let args_string = env::args().collect::<Vec<String>>();
  let args = args_string
    .iter()
    .map(|s| s.as_str())
    .collect::<Vec<&str>>();
  let fuse_args: Vec<&str> = if args.len() == 2 && args[1] == "--help" {
    let exe = args[0];
    vec![exe, "--help"]
  } else if args.len() == 3 {
    let exe = args[0];
    let mp = args[1];
    let src = args[2];
    if !Path::new(mp).is_dir() {
      panic!("no mount dir bro");
    }
    if !Path::new(src).is_dir() {
      panic!("no source dir broskimo");
    }
    vec![exe, "-o", "ro", "-o", "fsname=myfs", "-d", mp]
  } else {
    panic!("we need a mountpoint AND a source lol")
  };

  unsafe {
    MY_FS.filename = "hello.txt";
    MY_FS.content = "asdf\n";

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
