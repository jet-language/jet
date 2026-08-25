//! Integration guard for the one `--json` machine-output envelope.
//!
//! The unit reader tests construct valid rows. This test executes the CLI
//! doors, so a command that quietly returns a parallel JSON object cannot pass
//! by using the reader only in synthetic examples.

mod common;

use common::Scratch;
use jet_foundation::ExitCodes;
use jet_foundation::MachineOutput::{read_machine_output, MachineRecord};
use jet_foundation::JSON::{json_get, json_str, parse};
use std::fs;
use std::path::Path;
use std::process::Command;

#[allow(dead_code)] // Keep the metadata able to name stderr doors when one exists.
#[derive(Clone, Copy)]
enum MachineStream {
    Stdout,
    Stderr,
}

struct MachineDoor<'a> {
    label: &'a str,
    args: &'a [&'a str],
    action: Option<&'a str>,
    expected_status: i32,
    expected_stream: MachineStream,
}

fn assert_machine_stream(label: &str, stream: &str, bytes: &[u8], action: Option<&str>) {
    assert!(
        !bytes.iter().all(u8::is_ascii_whitespace),
        "{label} expected machine output on {stream}"
    );
    let text = String::from_utf8(bytes.to_vec())
        .unwrap_or_else(|error| panic!("{label} {stream} is not UTF-8 machine output: {error}"));
    let records = read_machine_output(&text).unwrap_or_else(|error| {
        panic!("{label} {stream} is not jet.report/v1 output: {error}\n{text}")
    });
    assert_eq!(
        records.len(),
        1,
        "{label} {stream} must contain exactly one machine record:\n{text}"
    );
    let value = parse(&text)
        .unwrap_or_else(|error| panic!("{label} {stream} is not one JSON object: {error}\n{text}"));
    match action {
        Some(expected) => {
            assert_eq!(records, [MachineRecord::Status]);
            let actual = json_get(&value, "action")
                .and_then(json_str)
                .unwrap_or_else(|| panic!("{label} {stream} has no action:\n{text}"));
            assert_eq!(actual, expected, "{label} emitted the wrong action");
        }
        None => assert_eq!(records, [MachineRecord::Report]),
    }
}

fn assert_machine_door(root: &Path, door: &MachineDoor<'_>) {
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(door.args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| panic!("{} did not start: {error}", door.label));
    assert_eq!(
        output.status.code(),
        Some(door.expected_status),
        "{} returned an unexpected exit status",
        door.label
    );
    let (expected, other, expected_name, other_name) = match door.expected_stream {
        MachineStream::Stdout => (&output.stdout, &output.stderr, "stdout", "stderr"),
        MachineStream::Stderr => (&output.stderr, &output.stdout, "stderr", "stdout"),
    };
    assert_machine_stream(door.label, expected_name, expected, door.action);
    assert!(
        other.iter().all(u8::is_ascii_whitespace),
        "{} emitted unexpected output on {}:\n{}",
        door.label,
        other_name,
        String::from_utf8_lossy(other)
    );
}

#[test]
fn every_json_report_door_uses_the_one_machine_envelope() {
    let scratch = Scratch::new("machine-output-envelope");
    fs::write(
        scratch.join("run.jet"),
        "fn square(n: Int) Int -> n * n\n\nfn run() { square(2) }\n",
    )
    .unwrap();
    fs::write(scratch.join("env.jet"), "module env.dev { }\n").unwrap();
    fs::write(
        scratch.join("before.jet"),
        "fn square(n: Int) Int -> n * n\n\nfn run() { square(2) }\n",
    )
    .unwrap();
    fs::write(
        scratch.join("after.jet"),
        "fn square(n: Int) Int -> n * n + 1\n\nfn run() { square(2) }\n",
    )
    .unwrap();

    // Keep these as real argv cases. A synthetic render_status_json loop does
    // not catch a command that emits a third envelope at its own door.
    let doors: &[MachineDoor<'_>] = &[
        MachineDoor {
            label: "check",
            args: &["check", "run.jet", "--json"],
            action: Some("check"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "build-effects",
            args: &["build", "run.jet", "--json"],
            action: Some("build.effects"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "fmt",
            args: &["fmt", "--check", "run.jet", "--json"],
            action: Some("fmt"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "budget",
            args: &["budget", "check", "--json"],
            action: Some("check"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "compiler-always-json",
            args: &["inspect", "compiler", "lex", "run.jet"],
            action: Some("inspect.compiler.lex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "compiler",
            args: &["inspect", "compiler", "lex", "run.jet", "--json"],
            action: Some("inspect.compiler.lex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "compiler-parse",
            args: &["inspect", "compiler", "parse", "run.jet", "--json"],
            action: Some("inspect.compiler.parse"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "compiler-check",
            args: &["inspect", "compiler", "check", "run.jet", "--json"],
            action: Some("inspect.compiler.check"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "compiler-source-map",
            args: &["inspect", "compiler", "source-map", "run.jet", "--json"],
            action: Some("inspect.compiler.source_map"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "reserved",
            args: &["inspect", "reserved", "--json"],
            action: Some("inspect.reserved"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "facts",
            args: &["inspect", "facts", "--json"],
            action: Some("inspect.facts"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "digest",
            args: &["inspect", "digest", "--list-topics", "--json"],
            action: Some("inspect.digest"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "env",
            args: &["inspect", "env", "env.jet", "--json"],
            action: Some("inspect.env"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "semindex",
            args: &["inspect", "semindex", "run.jet", "--json"],
            action: Some("inspect.semindex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "dossier",
            args: &["inspect", "dossier", "run.jet", "run", "--json"],
            action: Some("inspect.dossier"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "dossier-ffi",
            args: &["inspect", "dossier", "ffi", "--json"],
            action: Some("inspect.ffi"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "expand",
            args: &[
                "inspect", "expand", "--facts", "inline", "run.jet", "--json",
            ],
            action: Some("inspect.semindex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "expand-memory",
            args: &[
                "inspect", "expand", "--facts", "memory", "run.jet", "--json",
            ],
            action: Some("inspect.semindex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "expand-web",
            args: &["inspect", "expand", "--facts", "web", "run.jet", "--json"],
            action: Some("inspect.semindex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "expand-effects",
            args: &[
                "inspect", "expand", "--facts", "effects", "run.jet", "--json",
            ],
            action: Some("inspect.semindex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "expand-layout",
            args: &[
                "inspect", "expand", "--facts", "layout", "run.jet", "--json",
            ],
            action: Some("inspect.semindex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "expand-derive",
            args: &[
                "inspect", "expand", "--facts", "derive", "run.jet", "--json",
            ],
            action: Some("inspect.semindex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "expand-templates",
            args: &[
                "inspect",
                "expand",
                "--facts",
                "templates",
                "run.jet",
                "--json",
            ],
            action: Some("inspect.semindex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "expand-callable-signature",
            args: &[
                "inspect",
                "expand",
                "--facts",
                "callable-signature",
                "run.jet",
                "--json",
            ],
            action: Some("inspect.semindex"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "guarantees",
            args: &["inspect", "guarantees", "run.jet", "--json"],
            action: Some("inspect.guarantees"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "gates",
            args: &["inspect", "gates", "run.jet", "--json"],
            action: Some("inspect.gates"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "authority",
            args: &["inspect", "authority", "run.jet", "--json"],
            action: Some("inspect.gates"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "structure",
            args: &["inspect", "structure", "run.jet", "--json"],
            action: Some("inspect.structure"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "unsafe",
            args: &["inspect", "unsafe", "run.jet", "--json"],
            action: Some("inspect.unsafe"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "live",
            args: &["inspect", "live", "0", "--json"],
            action: None,
            expected_status: ExitCodes::USER_ERROR,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "impact",
            args: &["inspect", "impact", "run.jet", "square", "--json"],
            action: Some("inspect.impact"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "graph",
            args: &["inspect", "graph", "run.jet", "--json"],
            action: Some("inspect.build"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "query-build",
            args: &["inspect", "query", "build", "run.jet", "--json"],
            action: Some("inspect.build"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "structural-diff",
            args: &["diff", "--structural", "before.jet", "after.jet", "--json"],
            action: Some("diff.structural"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "find",
            args: &["find", "square", "run.jet", "--json"],
            action: Some("find"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "fill",
            args: &["fill", "run.jet", "--json"],
            action: Some("fill"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "eval",
            args: &["eval", "1 + 2", "--json"],
            action: Some("eval"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "status",
            args: &["status", "run.jet", "--json"],
            action: Some("status"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        // No ledger or trace exists in this scratch fixture, so these two doors
        // are error cases here, like `live`. They still have to answer with one
        // jet.report/v1 diagnostic on stdout, which is what this test proves.
        MachineDoor {
            label: "audit-memory",
            args: &["audit", "memory", "--json"],
            action: None,
            expected_status: ExitCodes::USER_ERROR,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "audit-copies",
            args: &["audit", "copies", "run.jet", "--json"],
            action: Some("audit.copies"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "gc-report",
            args: &["gc", "report", "--json"],
            action: None,
            expected_status: ExitCodes::USER_ERROR,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "remote-list",
            args: &["remote", "list", "--json"],
            action: Some("remote.list"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
        MachineDoor {
            label: "hangar-generations",
            args: &["hangar", "generations", "--json"],
            action: Some("generations"),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        },
    ];

    for door in doors {
        assert_machine_door(&scratch.path, door);
    }

    let trace = scratch.join("fixture.jettrace");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".jet/perf/1787269028-687eeec2.jettrace"),
        &trace,
    )
    .unwrap();
    let trace = trace.to_string_lossy().into_owned();
    for (label, action, expected_action) in [
        ("perf-view", "view", "perf.view"),
        ("perf-export", "export", "perf.export"),
    ] {
        let args = ["perf", action, trace.as_str(), "--json"];
        let door = MachineDoor {
            label,
            args: &args,
            action: Some(expected_action),
            expected_status: ExitCodes::OK,
            expected_stream: MachineStream::Stdout,
        };
        assert_machine_door(&scratch.path, &door);
    }
}
