const _FUCHSIA_COLOR: &str = fuchsia::COLOR;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}
