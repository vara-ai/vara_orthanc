extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=resources/OrthancCPlugin_1_11_3.h");
    let bindings = bindgen::builder()
        .header("resources/OrthancCPlugin_1_11_3.h")
        .allowlist_type("Orthanc.*")
        .allowlist_type("_Orthanc.*")
        .allowlist_var("Orthanc.*")
        .allowlist_function("Orthanc.*")
        .generate()
        .expect("Unable to generate bindings for OrthancCPlugin.h");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Unable to write bindings.rs to path.");
}
