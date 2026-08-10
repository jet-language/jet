//! D-ONCE-LAW1=A: Core-call AOT rows have one home and one projection.

mod common;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn read(path: &str) -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("read {path}: {error}"))
}

fn quoted(text: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut rest = text;
    while let Some((_, after_open)) = rest.split_once('"') {
        let Some((value, after_close)) = after_open.split_once('"') else {
            break;
        };
        values.push(value);
        rest = after_close;
    }
    values
}

fn arm_pairs(source: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut pattern = String::new();
    for line in source.lines() {
        if pattern.is_empty() && !line.trim_start().starts_with("(\"") {
            continue;
        }
        pattern.push_str(line);
        if !line.contains("=>") {
            continue;
        }
        let tuple = pattern
            .split_once("=>")
            .map(|(head, _)| head)
            .unwrap_or(&pattern)
            .trim_start()
            .trim_start_matches('(')
            .split_once(')')
            .map(|(tuple, _)| tuple)
            .unwrap_or_default();
        if let Some((modules, members)) = tuple.split_once(',') {
            for module in quoted(modules) {
                for member in quoted(members) {
                    pairs.push((module.to_string(), member.to_string()));
                }
            }
        }
        pattern.clear();
    }
    pairs
}

#[test]
fn aot_projection_is_complete_both_directions() {
    assert_eq!(
        arm_pairs("(\"core.a\" | \"core.b\", \"x\" | \"y\") => value"),
        [
            ("core.a".to_string(), "x".to_string()),
            ("core.a".to_string(), "y".to_string()),
            ("core.b".to_string(), "x".to_string()),
            ("core.b".to_string(), "y".to_string()),
        ]
    );
    let mut keys = HashSet::new();
    for row in jet::Syntax::AOT_CORE_CALLS {
        assert!(
            keys.insert((row.module.to_string(), row.member.to_string())),
            "duplicate Core call row: {}.{}",
            row.module,
            row.member
        );
        assert_eq!(jet::Syntax::aot_core_call(row.module, row.member), Some(row));
    }
    assert!(keys.len() > 500, "Core call table lost rows: {}", keys.len());

    let emit = read("crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs");
    assert!(
        emit.contains("crate::Syntax::aot_core_call(module, method)?"),
        "AOT emit no longer projects from the foundation Core-call table"
    );
    assert!(
        !emit.contains("const PLAIN_CORE_CALLS"),
        "AOT emit regained a hand-kept Core-call list"
    );
    let bespoke = emit
        .find("// #1635: every arm below stays bespoke")
        .expect("bespoke Core-call emission is named");
    let shadowed: Vec<String> = arm_pairs(&emit[bespoke..])
        .into_iter()
        .filter(|pair| keys.contains(pair))
        .map(|(module, member)| format!("{module}.{member}"))
        .collect();
    assert!(
        shadowed.is_empty(),
        "bespoke AOT arms repeat foundation rows:\n{}",
        shadowed.join("\n")
    );
}

#[test]
fn core_call_truth_names_the_foundation_home() {
    let row = jet_foundation::Registry::row("AotCoreCalls")
        .expect("AotCoreCalls truth is registered");
    assert_eq!(
        row.home,
        Some("crates/jet-foundation/src/Syntax/core_calls.rs")
    );
    assert_eq!(
        row.guard.map(|guard| guard.test),
        Some("aot_projection_is_complete_both_directions")
    );
}
