fn main() {
    println!("cargo:rustc-check-cfg=cfg(jet_release)");
}
