fn main() {
    let _command = std::process::Command::new("nix-instantiate");
    panic!("Cargo executed a build script despite package.build = false");
}
