//! The `jetos` binary — direct front door for JetOS system tools.

#![allow(non_snake_case)]

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    args.insert(0, "os".to_string());
    std::process::exit(jetpack::run(args));
}
