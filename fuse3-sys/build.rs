extern crate bindgen;
extern crate metadeps;

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
  let deps = metadeps::probe().unwrap();
  let fuse3_dep = deps.get("fuse3").unwrap();

  let include_dir_args = &fuse3_dep
    .include_paths
    .iter()
    .map(|p| format!("-I{}", p.to_str().unwrap()));

  let fuse3_bindings = bindgen::Builder::default()
    .clang_args(include_dir_args.clone())
    .derive_debug(true)
    .derive_default(true)
    .rustfmt_bindings(true)
    .header("include/fuse-wrapper.h")
    .generate()
    .unwrap();

  fuse3_bindings
    .write_to_file("include/fuse3_bindings.rs")
    .unwrap();

  for lp in &fuse3_dep.link_paths {
    println!(
      "cargo:rust-link-search=native={}",
      lp.to_str().unwrap()
    );
  }

  for ll in &fuse3_dep.libs {
    println!("cargo:rustc-link-lib=dylib={}", ll)
  }
}
