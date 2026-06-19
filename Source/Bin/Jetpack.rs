//! The `jetpack` binary — Jet's package manager (Phase 1, D-JPK1/9).
//!
//! Independent from the `jet` binary: this entry point delegates straight into
//! `jet::jetpack`. Later, `jet` commands may wrap these, but that plumbing is
//! deliberately not built here.

// Source files/modules use PascalCase names (owner decision).
#![allow(non_snake_case)]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(jet::Jetpack::run(args));
}
