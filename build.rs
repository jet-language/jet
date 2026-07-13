fn main() {
    let target = std::env::var("TARGET").expect("Cargo always provides TARGET to build scripts");
    println!("cargo:rustc-env=JET_BUILD_TARGET={target}");
}
