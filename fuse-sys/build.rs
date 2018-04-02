extern crate bindgen;
extern crate pkg_config;

use std::path::{Path, PathBuf};

fn main() {
  let (fuse_lib_name, platform_clang_args) = if cfg!(target_os = "macos") {
    ("osxfuse", vec!["-D_DARWIN_USE_64_BIT_INODE"])
  } else {
    ("fuse", vec![])
  };

  let fuse_dep = pkg_config::Config::new()
    .atleast_version("2.9.7")
    .probe(fuse_lib_name)
    .unwrap();

  let include_dir_args = &fuse_dep
    .include_paths
    .iter()
    .map(|p| format!("-I{}", p.to_str().unwrap()))
    .collect::<Vec<String>>();

  let fuse_bindings = bindgen::Builder::default()
    .clang_arg("-D_FILE_OFFSET_BITS=64")
    .clang_args(platform_clang_args)
    .clang_args(include_dir_args)
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
