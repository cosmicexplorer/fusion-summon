extern crate bindgen;
extern crate pkg_config;

#[cfg(target_os = "macos")]
static FUSE_LIB_NAME: &str = "osxfuse";

#[cfg(not(target_os = "macos"))]
static FUSE_LIB_NAME: &str = "fuse";

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
  let fuse_dep = pkg_config::Config::new()
    .atleast_version("2.9.7")
    .probe(FUSE_LIB_NAME)
    .unwrap();

  let include_dir_args = &fuse_dep
    .include_paths
    .iter()
    .map(|p| format!("-I{}", p.to_str().unwrap()));

  let fuse_bindings = bindgen::Builder::default()
    .clang_arg("-D_FILE_OFFSET_BITS=64")
    .clang_args(include_dir_args.clone())
    .derive_debug(true)
    .derive_default(true)
    .rustfmt_bindings(true)
    .header("include/fuse-wrapper.h")
    .generate()
    .unwrap();

  fuse_bindings
    .write_to_file("include/fuse_bindings.rs")
    .unwrap();

  for lp in &fuse_dep.link_paths {
    println!(
      "cargo:rust-link-search=native={}",
      lp.to_str().unwrap()
    );
  }

  for ll in &fuse_dep.libs {
    println!("cargo:rustc-link-lib=dylib={}", ll)
  }
}
