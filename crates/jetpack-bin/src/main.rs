//! The `jetpack` binary — Jet's package manager (Phase 1, D-JPK1/9).
//!
//! Independent from the `jet` binary: `jet` execs this binary by name for
//! every engine verb (D-JPK-DISPATCH1=B) instead of linking it in-process —
//! git/kubectl-style dispatch. This entry point is the whole engine-side
//! contract: answer the `--engine-protocol` handshake, else run the verb.
//!
//! Card #367 / D-PRODUCT-SPLIT1=C: binary ownership lives in the dedicated
//! `crates/jetpack-bin` package over the `jetpack` engine library. The split
//! keeps host-only Wasmtime outside compiler-linked code. The whole
//! workspace ships as one coordinated release (every member crate's
//! `Cargo.toml` version moves together, all currently `"1.0.0"`), so
//! `CARGO_PKG_VERSION` here still matches the `jet` binary's — if that
//! lockstep convention ever changes, this handshake needs a real shared
//! version source instead.

// Source files/modules use PascalCase names (owner decision).
#![allow(non_snake_case)]

#[path = "../../jet-pkg-model/src/Prelude/CompilerExtension.rs"]
mod CompilerExtensionHost;

fn main() {
    jetpack::Codegen::TIR::install_comptime_bridge();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str)
        == Some(jet_pkg_model::CompilerExtension::HOST_SUBCOMMAND)
    {
        std::process::exit(run_compiler_extension_host(&args[1..]));
    }
    if let Some(code) = jetpack::CLI::ProfileDispatch::dispatch_current_process() {
        std::process::exit(code);
    }
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

/// Hidden, bounded compiler-extension host protocol. Snapshot bytes arrive on
/// stdin; one validated response is written to stdout. Every failure is plain
/// Jet-owned stderr plus nonzero status. Wasmtime runs only in this process.
fn run_compiler_extension_host(args: &[String]) -> i32 {
    use jet_pkg_model::CompilerExtension::{
        analyze_with_host, TypedSnapshot, MAX_SNAPSHOT_BYTES,
    };
    use std::io::{Read, Write};

    let [wasm_path] = args else {
        eprintln!("compiler-extension host requires exactly one component path");
        return 2;
    };
    let mut request = Vec::new();
    if let Err(e) = std::io::stdin()
        .take(MAX_SNAPSHOT_BYTES.saturating_add(1) as u64)
        .read_to_end(&mut request)
    {
        eprintln!("couldn't read compiler-extension snapshot: {e}");
        return 2;
    }
    if request.len() > MAX_SNAPSHOT_BYTES {
        eprintln!(
            "compiler-extension snapshot exceeds IPC limit ({MAX_SNAPSHOT_BYTES} bytes)"
        );
        return 2;
    }
    let snapshot = match TypedSnapshot::decode(&request) {
        Ok(snapshot) => snapshot,
        Err(e) => {
            eprintln!("{}", e.message);
            return 2;
        }
    };
    let response = match analyze_with_host(
        wasm_path,
        &snapshot,
        CompilerExtensionHost::jet_compiler_extension_load,
        CompilerExtensionHost::jet_compiler_extension_analyze,
        CompilerExtensionHost::jet_compiler_extension_close,
    ) {
        Ok(response) => response,
        Err(e) => {
            eprintln!("{}", e.message);
            return 2;
        }
    };
    let response = match response.encode() {
        Ok(response) => response,
        Err(e) => {
            eprintln!("{}", e.message);
            return 2;
        }
    };
    if response.len() > snapshot.limits.max_response_bytes {
        eprintln!(
            "compiler-extension response exceeds IPC limit ({} > {})",
            response.len(),
            snapshot.limits.max_response_bytes
        );
        return 2;
    }
    if let Err(e) = std::io::stdout().write_all(&response) {
        eprintln!("couldn't write compiler-extension response: {e}");
        return 2;
    }
    0
}
