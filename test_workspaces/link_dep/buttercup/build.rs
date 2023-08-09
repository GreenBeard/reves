fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    println!("cargo:fox=red");
}
