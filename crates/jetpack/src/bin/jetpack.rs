//! The `jetpack` binary — Jet's package manager (Phase 1, D-JPK1/9).
//!
//! Independent from the `jet` binary: `jet` execs this binary by name for
//! every engine verb (D-JPK-DISPATCH1=B) instead of linking it in-process —
//! git/kubectl-style dispatch. This entry point is the whole engine-side
//! contract: answer the `--engine-protocol` handshake, else run the verb.
//!
//! Card #367 / D-PRODUCT-SPLIT1=C: binary ownership lives in `crates/jetpack`
//! itself (was a thin root-package shim over `jet::Jetpack::run`). The whole
//! workspace ships as one coordinated release (every member crate's
//! `Cargo.toml` version moves together, all currently `"1.0.0"`), so
//! `CARGO_PKG_VERSION` here still matches the `jet` binary's — if that
//! lockstep convention ever changes, this handshake needs a real shared
//! version source instead.

// Source files/modules use PascalCase names (owner decision).
#![allow(non_snake_case)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // D-JPK-DISPATCH1=B (A1): `jet` queries this before exec-ing any real
    // verb, to catch a `jet`/`jetpack` version mismatch as E1227 instead of
    // an engine that mysteriously doesn't understand a verb `jet` sent it.
    // Hidden: never listed in `jetpack help` or completions.
    if args.first().map(String::as_str) == Some(jetpack::Syntax::ENGINE_PROTOCOL_FLAG) {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    std::process::exit(jetpack::run(args));
}
