mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

const COMPOUND_PLACES: &str =
    include_str!("../Examples/features/memory/shared_compound_places.jet");
const COMPOUND_EXPECTED: &str =
    include_str!("../Examples/features/expected/memory/shared_compound_places.out");

#[test]
fn shared_plain_access_repl_uses_the_same_value_surface() {
    let output = jet::REPL::run_transcript(
        &[
            "struct Counter { value: Int }",
            "cell := shared Counter{ value: 0 }",
            "cell.value += 1",
            "cell.value",
        ],
        None,
    );
    assert!(
        output.contains("1 : Int"),
        "REPL shared access drifted: {output}"
    );
    assert!(
        !output.contains("E1116"),
        "REPL used the retired closure form: {output}"
    );
}

#[test]
fn shared_compound_places_match_o0_o1_o2_and_interpreter() {
    use std::{fs, process::Command};

    let scratch = common::Scratch::new("shared-compound-places");
    fs::write(scratch.join("run.jet"), COMPOUND_PLACES).unwrap();
    for (mode, args, built) in [
        ("O0", vec!["run", "run.jet"], false),
        ("O1", vec!["build", "run.jet"], true),
        ("O2", vec!["build", "--release", "run.jet"], true),
        ("interpreter", vec!["run", "--interpret", "run.jet"], false),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .args(args)
            .current_dir(&scratch.path)
            .env("JET_STORE_DIR", scratch.join("cache"))
            .env("JETPACK_ROOT", scratch.join("jetpack"))
            .env("JETPACK_ENV", "1")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{mode}: {}", String::from_utf8_lossy(&output.stderr));
        let output = if built {
            Command::new(scratch.join(".jet/build/run"))
                .current_dir(&scratch.path)
                .output()
                .unwrap()
        } else {
            output
        };
        assert!(output.status.success(), "{mode}: {output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stdout), COMPOUND_EXPECTED, "{mode}");
    }
}

#[test]
fn shared_compound_places_match_web() {
    tir_support::assert_awaited_web_tier(
        "shared-compound-places-web",
        COMPOUND_PLACES,
        COMPOUND_EXPECTED,
    );
}

#[test]
fn shared_map_entry_compound_updates_keep_the_total_update_diagnostic() {
    // S17 rejects partial indexed RMW. A Shared projection must neither bypass
    // E0164 nor silently store only the RHS of +=/-=.
    for operator in ["+=", "-="] {
        for transaction in [false, true] {
            let update = format!("store.items[\"a\"] {operator} 1");
            let update = if transaction {
                format!("#Transact {{ {update} }}")
            } else {
                update
            };
            let source = format!(
                "struct Store {{ items: [String: Int] }}\nfn run() {{\n\
                 store :: shared Store{{items: [String: Int]{{}}}}\n{update}\n}}"
            );
            let diagnostics = tir_support::compile_source("shared-index-rmw", &source)
                .expect_err("indexed RMW must remain rejected");
            assert!(
                diagnostics.iter().any(|diagnostic| diagnostic.code == "E0164"),
                "{operator}, transaction={transaction}: {diagnostics:?}"
            );
        }
    }
}
