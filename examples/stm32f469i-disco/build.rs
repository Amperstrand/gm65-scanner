use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=memory.x");

    // Provide empty defmt.x fallback when defmt feature is OFF.
    // When defmt IS enabled, the defmt crate generates the real defmt.x
    // in its own OUT_DIR (with _defmt_timestamp PROVIDE, .defmt sections, etc.)
    // and our build.rs must NOT shadow it.
    if env::var("CARGO_FEATURE_DEFMT").is_err() {
        File::create(out.join("defmt.x"))
            .unwrap()
            .write_all(b"/* empty fallback - defmt feature is not enabled */\n")
            .unwrap();
        println!("cargo:rerun-if-changed=build.rs");
    }
}
