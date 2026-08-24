//! Integration guard for the one `--json` machine-output envelope.
//!
//! The unit reader tests construct valid rows. This test executes the CLI
//! doors, so a command that quietly returns a parallel JSON object cannot pass
//! by using the reader only in synthetic examples.

mod common;

use common::Scratch;
use jet_foundation::MachineOutput::read_machine_output;
use std::fs;
use std::path::Path;
use std::process::Command;

fn assert_machine_stream(label: &str, stream: &str, bytes: &[u8]) -> bool {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return false;
    }
    let text = String::from_utf8(bytes.to_vec()).unwrap_or_else(|error| {
        panic!("{label} {stream} is not UTF-8 machine output: {error}")
    });
    read_machine_output(&text).unwrap_or_else(|error| {
        panic!("{label} {stream} is not jet.report/v1 output: {error}\n{text}")
    });
    true
}

fn assert_machine_door(root: &Path, label: &str, args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("{label} did not start: {error}"));
    let stdout = assert_machine_stream(label, "stdout", &output.stdout);
    let stderr = assert_machine_stream(label, "stderr", &output.stderr);
    assert!(stdout || stderr, "{label} emitted no machine output");
}

#[test]
fn every_json_report_door_uses_the_one_machine_envelope() {
    let scratch = Scratch::new("machine-output-envelope");
    fs::write(
        scratch.join("run.jet"),
        "fn square(n: Int) Int -> n * n\nfn run() { square(2) }\n",
    )
    .unwrap();
    fs::write(scratch.join("env.jet"), "module env.dev { }\n").unwrap();
    fs::write(
        scratch.join("before.jet"),
        "fn square(n: Int) Int -> n * n\nfn run() { square(2) }\n",
    )
    .unwrap();
    fs::write(
        scratch.join("after.jet"),
        "fn square(n: Int) Int -> n * n + 1\nfn run() { square(2) }\n",
    )
    .unwrap();

    // Keep these as real argv cases. A synthetic render_status_json loop does
    // not catch a command that emits a third envelope at its own door.
    let doors: &[(&str, &[&str])] = &[
        ("check", &["check", "run.jet", "--json"]),
        ("abilities-json", &["build", "run.jet", "--abilities-json"]),
        ("fmt", &["fmt", "--check", "run.jet", "--json"]),
        ("budget", &["budget", "check", "--json"]),
        (
            "compiler-always-json",
            &["inspect", "compiler", "lex", "run.jet"],
        ),
        ("compiler", &["inspect", "compiler", "lex", "run.jet", "--json"]),
        ("compiler-parse", &["inspect", "compiler", "parse", "run.jet", "--json"]),
        ("compiler-check", &["inspect", "compiler", "check", "run.jet", "--json"]),
        (
            "compiler-source-map",
            &["inspect", "compiler", "source-map", "run.jet", "--json"],
        ),
        ("reserved", &["inspect", "reserved", "--json"]),
        ("facts", &["inspect", "facts", "--json"]),
        (
            "digest",
            &["inspect", "digest", "--list-topics", "--json"],
        ),
        ("env", &["inspect", "env", "env.jet", "--json"]),
        ("semindex", &["inspect", "semindex", "run.jet", "--json"]),
        ("dossier", &["inspect", "dossier", "run.jet", "run", "--json"]),
        ("dossier-ffi", &["inspect", "dossier", "ffi", "--json"]),
        (
            "expand",
            &["inspect", "expand", "--facts", "inline", "run.jet", "--json"],
        ),
        (
            "expand-memory",
            &["inspect", "expand", "--facts", "memory", "run.jet", "--json"],
        ),
        (
            "expand-web",
            &["inspect", "expand", "--facts", "web", "run.jet", "--json"],
        ),
        (
            "expand-effects",
            &["inspect", "expand", "--facts", "effects", "run.jet", "--json"],
        ),
        (
            "expand-layout",
            &["inspect", "expand", "--facts", "layout", "run.jet", "--json"],
        ),
        (
            "expand-derive",
            &["inspect", "expand", "--facts", "derive", "run.jet", "--json"],
        ),
        (
            "expand-templates",
            &[
                "inspect",
                "expand",
                "--facts",
                "templates",
                "run.jet",
                "--json",
            ],
        ),
        (
            "expand-callable-signature",
            &[
                "inspect",
                "expand",
                "--facts",
                "callable-signature",
                "run.jet",
                "--json",
            ],
        ),
        ("guarantees", &["inspect", "guarantees", "run.jet", "--json"]),
        ("gates", &["inspect", "gates", "run.jet", "--json"]),
        ("authority", &["inspect", "authority", "run.jet", "--json"]),
        ("structure", &["inspect", "structure", "run.jet", "--json"]),
        ("unsafe", &["inspect", "unsafe", "run.jet", "--json"]),
        ("live", &["inspect", "live", "0", "--json"]),
        (
            "impact",
            &["inspect", "impact", "run.jet", "square", "--json"],
        ),
        ("graph", &["inspect", "graph", "run.jet", "--json"]),
        (
            "query-build",
            &["inspect", "query", "build", "run.jet", "--json"],
        ),
        (
            "structural-diff",
            &["diff", "--structural", "before.jet", "after.jet", "--json"],
        ),
        ("find", &["find", "square", "run.jet", "--json"]),
        ("fill", &["fill", "run.jet", "--json"]),
        ("eval", &["eval", "1 + 2", "--json"]),
        ("status", &["status", "run.jet", "--json"]),
        ("audit-memory", &["audit", "memory", "--json"]),
        ("audit-copies", &["audit", "copies", "run.jet", "--json"]),
        ("gc-report", &["gc", "report", "--json"]),
        ("remote-list", &["remote", "list", "--json"]),
        ("hangar-generations", &["hangar", "generations", "--json"]),
    ];

    for (label, args) in doors {
        assert_machine_door(&scratch.path, label, args);
    }

    let trace = scratch.join("fixture.jettrace");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".jet/perf/1787269028-687eeec2.jettrace"),
        &trace,
    )
    .unwrap();
    let trace = trace.to_string_lossy().into_owned();
    for (label, action) in [("perf-view", "view"), ("perf-export", "export")] {
        let args = ["perf", action, trace.as_str(), "--json"];
        assert_machine_door(&scratch.path, label, &args);
    }
}
