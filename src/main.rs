extern crate fuse3_sys;
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

use fuse3_sys::{fuse_config, fuse_conn_info, fuse_file_info, fuse_fill_dir_t,
                fuse_operations, fuse_readdir_flags, mode_t, off_t};

unsafe fn our_fuse_main(
  argc: c_int,
  argv: *mut *mut c_char,
  op: *const fuse3_sys::fuse_operations,
) -> c_int {
  fuse3_sys::fuse_main_real(
    argc,
    argv,
    op,
    mem::size_of::<fuse3_sys::fuse_operations>(),
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

unsafe extern "C" fn hello_init(
  conn: *mut fuse_conn_info,
  cfg: *mut fuse_config,
) -> *mut c_void {
  (*cfg).kernel_cache = 1 as c_int;
  ptr::null_mut()
}

unsafe extern "C" fn hello_getattr(
  path_c_str: *const c_char,
  stbuf_ptr: *mut fuse3_sys::stat,
  fi_ptr: *mut fuse_file_info,
) -> c_int {
  libc::memset(
    stbuf_ptr as *mut libc::c_void,
    0,
    mem::size_of::<fuse3_sys::stat>(),
  );
  let stbuf = &mut *stbuf_ptr;

  let path = from_c_string(path_c_str);
  if path == "/" {
    stbuf.st_mode = fuse3_sys::S_IFDIR | 0o755;
    stbuf.st_nlink = 2;
    0
  } else if path == format!("/{}", MY_FS.filename) {
    stbuf.st_mode = fuse3_sys::S_IFREG | 0o444;
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
  flags: fuse_readdir_flags,
) -> c_int {
  let path = CStr::from_ptr(path_c_str).to_str().unwrap();
  if path != "/" {
    return -ENOENT;
  }

  let filler_fn = filler_ptr.unwrap();

  let entries = vec![".", "..", MY_FS.filename];

  for s in entries {
    let cur_str = CString::new(s).unwrap();
    filler_fn(buf, cur_str.as_ptr(), ptr::null_mut(), 0, 0);
  }

  0
}

unsafe extern "C" fn hello_open(
  path_c_str: *const c_char,
  fi_ptr: *mut fuse_file_info,
) -> c_int {
  let path = from_c_string(path_c_str);

  if &path[1..] != MY_FS.filename {
    return -ENOENT;
  }

  let fi = *fi_ptr;
  let access: u32 = (fi.flags as u32) & fuse3_sys::O_ACCMODE;
  if access != fuse3_sys::O_RDONLY {
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
  let fuse_args: Vec<&str> = match &args[..] {
    &[exe, "--help"] => vec![exe, "--help"],
    &[exe, mp, src] => {
      if !Path::new(mp).is_dir() {
        panic!("no mount dir bro");
      }
      if !Path::new(src).is_dir() {
        panic!("no source dir broskimo");
      }
      vec![exe, "-o", "ro", "-o", "fsname=myfs", "-d", mp]
    }
    _ => panic!("we need a mountpoint AND a source lol"),
  };

  unsafe {
    MY_FS.filename = "hello.txt";
    MY_FS.content = "asdf\n";

    let hello_oper: fuse_operations = fuse_operations {
      init: Some(hello_init),
      getattr: Some(hello_getattr),
      readdir: Some(hello_readdir),
      open: Some(hello_open),
      read: Some(hello_read),
      ..Default::default()
    };

    let mut c_fuse_args = into_c_string_vec(fuse_args);

    our_fuse_main(
      *&c_fuse_args.len() as c_int,
      *&c_fuse_args.as_mut_ptr(),
      &hello_oper,
    );

    for opt in c_fuse_args {
      let _ = CString::from_raw(opt);
    }
  }
}
