//! Card #2276: a rustc rejection of generated Rust is an ICE, not a user
//! diagnostic. The debug-only self-test seam makes real rustc reject the
//! private input while the shared generated source remains attachable.

mod common;

#[path = "cli_parts/support.rs"]
mod cli_support;

use std::fs;
use std::process::Command;

#[test]
fn generated_rust_rejection_is_a_branded_ice_and_preserves_backend_evidence() {
    let project = common::Scratch::new("ice-generated-rust-rejection");
    let source = format!(
        "fn run() {{\n    print(\"rejection-{}\")\n}}\n",
        std::process::id()
    );
    fs::write(project.join("main.jet"), &source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["build", "--profile=debug", "--verbose", "main.jet"])
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
        stderr.contains("internal compiler error:"),
        "backend rejection must remain an internal compiler error:\n{stderr}"
    );
    assert!(
        stderr.contains(project.join("build/main.rs").to_string_lossy().as_ref()),
        "report must identify the preserved generated source:\n{stderr}"
    );
    assert!(
        stderr.contains("JET_RUSTC_REJECTION_SENTINEL"),
        "report must retain the first backend error:\n{stderr}"
    );

    assert!(project.join("build/main.rs").is_file());
    let log_path = project.join("build/main.rustc.log");
    assert!(
        stderr.contains(log_path.to_string_lossy().as_ref()),
        "report must identify the complete backend log:\n{stderr}"
    );
    let log = fs::read_to_string(&log_path).expect("backend stderr must survive the ICE");
    assert!(
        log.contains("JET_RUSTC_REJECTION_SENTINEL"),
        "persisted backend stderr lost the first rejection:\n{log}"
    );
    assert!(
        log.contains("JET_RUSTC_REJECTION_SECOND_SENTINEL"),
        "persisted backend stderr lost the second rejection:\n{log}"
    );

    let output_text = format!("{}{}", String::from_utf8_lossy(&output.stdout), stderr);
    assert!(
        output_text.contains("[build] rustc error:\nerror: JET_RUSTC_REJECTION_SENTINEL"),
        "verbose build must echo the first backend error:\n{output_text}"
    );
    assert!(
        !output_text.contains("JET_RUSTC_REJECTION_SECOND_SENTINEL"),
        "verbose output must exclude later backend errors:\n{output_text}"
    );
    assert!(
        !output_text.contains("warning:"),
        "verbose output must exclude later backend warnings:\n{output_text}"
    );

    let banner = &stderr[stderr.find("internal compiler error:").unwrap()..];
    assert!(
        !banner.contains("JET_RUSTC_REJECTION_SECOND_SENTINEL"),
        "ICE banner must exclude later backend errors:\n{banner}"
    );
    assert!(
        !banner.contains("warning:"),
        "ICE banner must exclude later backend warnings:\n{banner}"
    );
    cli_support::check_snapshot(
        "generated_rust_rejection.txt",
        &normalize_backend_locations(banner, &project.path),
    );
    cli_support::check_snapshot(
        "generated_rust_rejection.rustc.txt",
        &normalize_backend_error_excerpts(&log, &project.path),
    );
}


fn normalize_backend_error_excerpts(text: &str, project: &std::path::Path) -> String {
    let mut excerpts = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let header = line.trim_start();
        if header.starts_with("error:") || header.starts_with("error[") {
            if let Some(block) = current.take() {
                if block.contains("JET_RUSTC_REJECTION_") {
                    excerpts.push(block.trim_end().to_string());
                }
            }
            current = Some(line.to_string());
        } else if header.starts_with("warning:") {
            if let Some(block) = current.take() {
                if block.contains("JET_RUSTC_REJECTION_") {
                    excerpts.push(block.trim_end().to_string());
                }
            }
        } else if let Some(block) = current.as_mut() {
            block.push('\n');
            block.push_str(line);
        }
    }
    if let Some(block) = current {
        if block.contains("JET_RUSTC_REJECTION_") {
            excerpts.push(block.trim_end().to_string());
        }
    }
    normalize_backend_locations(&excerpts.join("\n\n"), project)
}

fn normalize_backend_locations(text: &str, project: &std::path::Path) -> String {
    let text = text.replace(project.to_string_lossy().as_ref(), "<project>");
    let mut normalized = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("--> ") {
            normalized.push_str("  --> <generated>:<line>:<column>");
        } else if let Some((number, source)) = line.split_once('|') {
            if number.trim().parse::<usize>().is_ok() {
                normalized.push_str("<line> |");
                normalized.push_str(source);
            } else if number.trim().is_empty() {
                normalized.push_str("       |");
                normalized.push_str(source);
            } else {
                normalized.push_str(line);
            }
        } else {
            normalized.push_str(line);
        }
        normalized.push('\n');
    }
    normalized
}

