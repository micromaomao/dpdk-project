use std::env;
use std::path;

fn main() {
  let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

  cbindgen::Builder::new()
    .with_crate(crate_dir)
    .generate()
    .expect("Unable to generate bindings")
    .write_to_file(path::PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("bindings.h"));
}
