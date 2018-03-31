extern crate fuse3_sys;
extern crate libc;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::default::Default;

use fuse3_sys::fuse_operations;

fn main() {
  let args = std::env::args_os().collect::<Vec<OsString>>();
  let (mountpoint, source) = match &args[..] {
    &[_, ref mp, ref src] => (mp, src),
    _ => panic!("we need a mountpoint AND a source lol"),
  };
  if !Path::new(mountpoint).is_dir() {
    panic!("no mount dir bro");
  }
  if !Path::new(source).is_dir() {
    panic!("no source dir broskimo");
  }
  let options = ["-o", "ro", "-o", "fsname=myfs"]
    .iter()
    .map(|o| o.as_ref())
    .collect::<Vec<&OsStr>>();
  let hello_oper: fuse_operations = fuse_operations {
    ..Default::default()
  };
  // fuse::mount(
  //   MyFS {
  //     source_path: PathBuf::from(source),
  //   },
  //   &mountpoint,
  //   &options,
  // ).unwrap();
  println!("Hello, world!");
}
