//! The `jetos` binary — direct front door for the JetOS system tools.

#![allow(non_snake_case)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some(jet::Syntax::ENGINE_PROTOCOL_FLAG) {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    std::process::exit(jet::Jetpack::run(args));
}
