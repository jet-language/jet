//! Card #2276: a rustc rejection of generated Rust is an ICE, not a user
//! diagnostic. The debug-only self-test seam makes real rustc reject the
//! private input while the shared generated source remains attachable.

mod common;

use std::fs;
use std::process::Command;

#[test]
fn generated_rust_rejection_is_a_branded_ice_and_keeps_source() {
    let project = common::Scratch::new("ice-generated-rust-rejection");
    let source = format!(
        "fn run() {{\n    print(\"rejection-{}\")\n}}\n",
        std::process::id()
    );
    fs::write(project.join("main.jet"), &source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", "--profile=debug", "main.jet"])
        .current_dir(&project.path)
        .env("JET_ICE_RUSTC_REJECTION_SELF_TEST", "1")
        .env("JET_PROVE_FRESH_TEST", "1")
        .env("NO_COLOR", "1")
        .output()
        .expect("jet build should start");

    assert_eq!(
        output.status.code(),
        Some(101),
        "generated-Rust rejection must exit as an ICE:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("internal compiler error: the generated Rust did not compile."),
        "missing branded ICE report:\n{stderr}"
    );
    assert!(
        stderr.contains("This is a bug in jet, NOT in your program. Please report it,")
            && stderr.contains("attaching your source file and the generated file below."),
        "missing branded report body:\n{stderr}"
    );
    assert!(
        stderr.contains("generated: build/main.rs"),
        "report must identify the preserved generated source:\n{stderr}"
    );
    assert!(
        !stderr.contains("JET_RUSTC_REJECTION_SENTINEL")
            && !stderr.contains("--- rustc said ---")
            && !stderr.contains("compile_error!")
            && !stderr.contains("error[E"),
        "raw rustc rejection leaked to user stderr:\n{stderr}"
    );

    let generated = fs::read_to_string(project.join("build/main.rs"))
        .expect("generated Rust must survive an ICE for the report");
    assert!(
        generated.contains("fn main()") && generated.contains("rejection-"),
        "preserved generated Rust is incomplete:\n{generated}"
    );
}
