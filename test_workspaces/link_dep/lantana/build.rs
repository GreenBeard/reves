use std::ffi::OsString;

fn read_var(var: &str) -> Option<OsString> {
    println!("cargo:rerun-if-env-changed={}", var);
    std::env::var_os(var)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    assert!(read_var("DEP_MALLOW_FOX").unwrap() == "red");
    println!("cargo:panda=white-and-black");
}
