use std::ffi::OsString;

fn read_var(var: &str) -> Option<OsString> {
    println!("cargo:rerun-if-env-changed={}", var);
    std::env::var_os(var)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let value = read_var("DEP_PIMPERNEL_PANDA");
    if read_var("CARGO_CFG_FOOBAR").is_some() {
        assert!(value.unwrap() == "white-and-black");
        println!("cargo:elephant=gray");
    }
}
