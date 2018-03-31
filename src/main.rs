extern crate fuse3_sys;

use std::default::Default;
use std::ffi::{CStr, CString, OsStr, OsString};
use std::iter::{IntoIterator, Iterator};
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::ptr;

use fuse3_sys::fuse_operations;

unsafe fn our_fuse_main(
  argc: c_int,
  argv: *mut *mut c_char,
  op: *const fuse3_sys::fuse_operations,
) -> c_int {
  fuse3_sys::fuse_main_real(
    argc,
    argv,
    op,
    std::mem::size_of::<fuse3_sys::fuse_operations>(),
    std::ptr::null_mut(),
  )
}

fn to_c_strings(args: Vec<&str>) -> Vec<*mut c_char> {
  args
    .iter()
    .map(|x| CString::new(x.bytes().collect::<Vec<u8>>()).unwrap())
    .collect::<Vec<CString>>()
    .into_iter()
    .map(|x| x.into_raw())
    .collect::<Vec<*mut c_char>>()
}

fn main() {
  let args_string = std::env::args().collect::<Vec<String>>();
  let args = args_string
    .iter()
    .map(|s| s.as_str())
    .collect::<Vec<&str>>();
  let mut fuse_args: Vec<&str> = match &args[..] {
    &[exe, "--help"] => vec![exe, "--help"],
    &[ref exe, ref mp, ref src] => {
      if !Path::new(mp).is_dir() {
        panic!("no mount dir bro");
      }
      if !Path::new(src).is_dir() {
        panic!("no source dir broskimo");
      }
      vec![exe, "-o", "ro", "-o", "fsname=myfs", mp]
    }
    _ => panic!("we need a mountpoint AND a source lol"),
  };
  let mut c_fuse_args = to_c_strings(fuse_args);

  let hello_oper: fuse_operations = fuse_operations {
    ..Default::default()
  };

  unsafe {
    our_fuse_main(
      *&c_fuse_args.len() as c_int,
      *&c_fuse_args.as_mut_ptr(),
      &hello_oper,
    );

    for opt in c_fuse_args {
      let _ = CString::from_raw(opt);
    }
  }

  // fuse::mount(
  //   MyFS {
  //     source_path: PathBuf::from(source),
  //   },
  //   &mountpoint,
  //   &options,
  // ).unwrap();
}
