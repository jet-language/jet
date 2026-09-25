//! Tier parity for typed `core.testing.histories` failures.

mod common;
mod tir_support;

const INVALID_SEED_SOURCE: &str = r#"
use core.testing as api

fn reference_size(commands: [DataTree]) -> DataTree {
    DataTree.Int(commands.len())
}

fn candidate_size(commands: [DataTree]) -> DataTree {
    DataTree.Int(commands.len())
}

fn observe(value: DataTree) -> DataTree { ~value }

fn run() !Err {
    comparison :: api.histories<DataTree>(
        seed: -1,
        cases: 1,
        strategy: None,
        model: reference_size,
        actual: candidate_size,
        observe: observe,
    )
    print(comparison.status)
}
"#;
const INTEGER_COMMAND_SOURCE: &str = r#"
use core.testing as api

enum HistoryStep {
    Step(Int)
}

fn reference_size(commands: [HistoryStep]) -> DataTree {
    DataTree.Int(commands.len())
}

fn candidate_size(commands: [HistoryStep]) -> DataTree {
    DataTree.Int(commands.len())
}

fn observe(value: DataTree) -> DataTree { ~value }

fn run() !Err {
    comparison :: api.histories<HistoryStep>(
        seed: 1,
        cases: 1,
        strategy: None,
        model: reference_size,
        actual: candidate_size,
        observe: observe,
    )
    print(comparison.status)
}
"#;

const CALLBACK_FAILURE_SOURCE: &str = r#"
use core.testing as api

fn failing_model(commands: [DataTree]) DataTree !Err -> {
    return Err("history callback marker")
}

fn candidate_size(commands: [DataTree]) DataTree {
    DataTree.Int(commands.len())
}

fn observe(value: DataTree) -> DataTree { ~value }

fn run() !Err {
    comparison :: api.histories<DataTree>(
        seed: 1,
        cases: 1,
        strategy: None,
        model: failing_model,
        actual: candidate_size,
        observe: observe,
    )
    print(comparison.status)
    print(comparison.reason)
}
"#;

#[test]
fn core_testing_histories_error_is_stable_across_i9_tiers() {
    let expected_error = "history seed is invalid";
    let mut tiers = vec![
        (
            "default run",
            tir_support::jit_run("core_testing_histories_invalid_seed", INVALID_SEED_SOURCE),
        ),
        (
            "forced interpreter",
            tir_support::interpreter_run("core_testing_histories_invalid_seed", INVALID_SEED_SOURCE),
        ),
    ];
    if tir_support::have_rustc() {
        tiers.push((
            "AOT",
            tir_support::build_and_run_full(
                "jet_core_testing_histories_invalid_seed",
                "core_testing_histories_invalid_seed",
                INVALID_SEED_SOURCE,
            ),
        ));
    }

    let baseline = &tiers[0].1;
    for (tier, result) in &tiers {
        assert_eq!(result, baseline, "{tier} disagreed on the history seed error");
        assert_eq!(result.0, 1, "{tier} must report the invalid history seed");
        assert!(result.1.is_empty(), "{tier} wrote stdout: {:?}", result.1);
        assert!(
            result.2.contains(expected_error),
            "{tier} lost the exact seed error:\n{}",
            result.2
        );
    }
}

#[test]
fn core_testing_histories_generated_int_commands_are_checked_across_i9_tiers() {
    let mut tiers = vec![
        (
            "default run",
            tir_support::jit_run("core_testing_histories_int_command", INTEGER_COMMAND_SOURCE),
        ),
        (
            "forced interpreter",
            tir_support::interpreter_run("core_testing_histories_int_command", INTEGER_COMMAND_SOURCE),
        ),
    ];
    if tir_support::have_rustc() {
        tiers.push((
            "AOT",
            tir_support::build_and_run_full(
                "jet_core_testing_histories_int_command",
                "core_testing_histories_int_command",
                INTEGER_COMMAND_SOURCE,
            ),
        ));
    }

    let baseline = &tiers[0].1;
    for (tier, result) in &tiers {
        assert_eq!(result, baseline, "{tier} disagreed on generated integer history");
        assert_eq!(result.0, 0, "{tier} failed:\n{}", result.2);
        assert_eq!(result.1, "matched\n", "{tier} changed history output");
        assert!(result.2.is_empty(), "{tier} wrote stderr: {:?}", result.2);
    }
}

#[test]
fn core_testing_histories_callback_failure_becomes_unavailable_across_i9_tiers() {
    let mut tiers = vec![
        (
            "default run",
            tir_support::jit_run("core_testing_histories_callback_failure", CALLBACK_FAILURE_SOURCE),
        ),
        (
            "forced interpreter",
            tir_support::interpreter_run("core_testing_histories_callback_failure", CALLBACK_FAILURE_SOURCE),
        ),
    ];
    if tir_support::have_rustc() {
        tiers.push((
            "AOT",
            tir_support::build_and_run_full(
                "jet_core_testing_histories_callback_failure",
                "core_testing_histories_callback_failure",
                CALLBACK_FAILURE_SOURCE,
            ),
        ));
    }

    let baseline = &tiers[0].1;
    for (tier, result) in &tiers {
        assert_eq!(result, baseline, "{tier} disagreed on callback failure");
        assert_eq!(result.0, 0, "{tier} failed:\n{}", result.2);
        assert!(
            result.1.starts_with("unavailable\n"),
            "{tier} did not preserve the unavailable comparison:\n{}",
            result.1
        );
        assert!(
            result.1.contains("history callback marker"),
            "{tier} lost the typed callback failure:\n{}",
            result.1
        );
        assert!(result.2.is_empty(), "{tier} wrote stderr: {:?}", result.2);
    }
}
