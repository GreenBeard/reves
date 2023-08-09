use std::ffi::OsString;

fn read_var(var: &str) -> Option<OsString> {
    println!("cargo:rerun-if-env-changed={}", var);
    std::env::var_os(var)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    if read_var("CARGO_CFG_FOOBAR").is_some() {
        println!("cargo:fox=red");
    }
}
