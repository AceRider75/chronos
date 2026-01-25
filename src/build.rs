fn main() {
    println!("cargo:rustc-link-search=native=target/asm");
    println!("cargo:rustc-link-lib=static=switch");
    println!("cargo:rerun-if-changed=src/asm/switch.s");
}