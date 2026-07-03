//! The `jetpack` binary — Jet's package manager (Phase 1, D-JPK1/9).
//!
//! Independent from the `jet` binary: `jet` execs this binary by name for
//! every engine verb (D-JPK-DISPATCH1=B) instead of linking it in-process —
//! git/kubectl-style dispatch. This entry point is the whole engine-side
//! contract: answer the `--engine-protocol` handshake, else run the verb.

// Source files/modules use PascalCase names (owner decision).
#![allow(non_snake_case)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // D-JPK-DISPATCH1=B (A1): `jet` queries this before exec-ing any real
    // verb, to catch a `jet`/`jetpack` version mismatch as E1227 instead of
    // an engine that mysteriously doesn't understand a verb `jet` sent it.
    // `CARGO_PKG_VERSION` here is the root `jet` package's version — the same
    // package `Source/main.rs` (the `jet` binary) is built from — so the two
    // binaries can only disagree by being copies from different toolchain
    // installs, which is exactly the skew this handshake catches. Hidden:
    // never listed in `jetpack help` or completions.
    if args.first().map(String::as_str) == Some(jet::Syntax::ENGINE_PROTOCOL_FLAG) {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    std::process::exit(jet::Jetpack::run(args));
}
