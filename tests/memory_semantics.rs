mod common;
#[path = "tir_support/mod.rs"]
mod tir_support;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};


fn jet() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(jet())
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .env(
            "JET_MEMORY_LEDGER",
            root.join(".jet/memory/ledger-v1.jsonl"),
        )
        .output()
        .unwrap()
}
fn assert_cli_snapshot(name: &str, actual: &[u8]) {
    let expected = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/cli")
            .join(name),
    )
    .unwrap();
    assert_eq!(actual, expected, "CLI snapshot mismatch: {name}");
}


fn ledger(root: &Path, rows: &[&str]) {
    let path = root.join(".jet/memory/ledger-v1.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, rows.join("\n") + "\n").unwrap();
}

fn row(kind: &str, code: &str, repairs: &[&str]) -> String {
    let repairs = repairs
        .iter()
        .map(|repair| format!("\"{repair}\""))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":\"jet.memory.ledger\",\"version\":1,\"kind\":\"{kind}\",\"code\":\"{code}\",\"source\":\"main.jet\",\"span_start\":0,\"span_end\":6,\"byte_spans\":true,\"scope\":\"run\",\"provenance\":\"fixture\",\"detail\":\"observed\",\"expected\":\"borrow\",\"repairs\":[{repairs}]}}"
    )
}

#[test]
fn memo_computed_and_view_copy_examples_agree_across_execution_tiers() {
    tir_support::assert_example_cli_tiers_agree(
        "memory/memoize",
        include_str!("../examples/features/expected/memory/memoize.out"),
    );
    tir_support::assert_example_cli_tiers_agree(
        "memory/computed_field",
        include_str!("../examples/features/expected/memory/computed_field.out"),
    );
    tir_support::assert_example_cli_tiers_agree(
        "memory/copy_verb",
        include_str!("../examples/features/expected/memory/copy_verb.out"),
    );
}

#[test]
fn computed_field_nested_value_blocks_use_and_invalidate_sibling_dependencies() {
    let src = r#"
struct Score {
    enabled: Bool
    base: Int
    bonus: Int
    #Memo
    total: Int => if enabled -> {
        subtotal :: base + bonus
        subtotal
    } else -> base
}

fn run() {
    score := Score.{enabled: true, base: 10, bonus: 2}
    print(score.total)
    score.bonus = 7
    print(score.total)
}
"#;
    tir_support::assert_tiers_agree("computed_field_nested_value", src, "12\n17\n");
}

#[test]
fn exercised_memory_ledger_reports_empty_gc_sentry_and_combined_runs() {
    let root = common::unique_tmp("jet_memory_audit");
    std::fs::create_dir_all(&root).unwrap();

    ledger(&root, &[]);
    let empty = run(&root, &["audit", "memory", "--json"]);
    assert!(empty.status.success(), "{}", String::from_utf8_lossy(&empty.stderr));
    assert_cli_snapshot("memory_audit_empty.txt", &empty.stdout);
    let empty_json = String::from_utf8_lossy(&empty.stdout);
    assert!(empty_json.contains("\"coverage\":\"exercised runs only\""));
    assert!(empty_json.contains("\"witnesses\":0"));

    let gc = row("gc", "gc", &["own the value directly"]);
    ledger(&root, &[&gc]);
    let gc_only = run(&root, &["audit", "memory", "--json"]);
    let gc_json = String::from_utf8_lossy(&gc_only.stdout);
    assert!(gc_only.status.success(), "{}", String::from_utf8_lossy(&gc_only.stderr));
    assert_cli_snapshot("memory_audit_gc.txt", &gc_only.stdout);
    assert!(gc_json.contains("\"kind\":\"gc\""));
    assert!(gc_json.contains("\"witnesses\":1"));

    let sentry = row("sentry", "R0802", &["move the raw access"]);
    ledger(&root, &[&sentry]);
    let sentry_only = run(&root, &["audit", "memory", "--json"]);
    let sentry_json = String::from_utf8_lossy(&sentry_only.stdout);
    assert!(sentry_only.status.success(), "{}", String::from_utf8_lossy(&sentry_only.stderr));
    assert_cli_snapshot("memory_audit_sentry.txt", &sentry_only.stdout);
    assert!(sentry_json.contains("\"kind\":\"sentry\""));
    assert!(sentry_json.contains("\"witnesses\":1"));

    ledger(&root, &[&gc, &sentry]);
    let combined = run(&root, &["audit", "memory", "--json"]);
    let combined_json = String::from_utf8_lossy(&combined.stdout);
    assert!(combined.status.success(), "{}", String::from_utf8_lossy(&combined.stderr));
    assert_cli_snapshot("memory_audit_combined.txt", &combined.stdout);
    assert!(combined_json.contains("\"kind\":\"gc\""));
    assert!(combined_json.contains("\"kind\":\"sentry\""));
    assert!(combined_json.contains("\"witnesses\":2"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn memory_fix_applies_one_exact_repair_and_names_ambiguous_options() {
    let root = common::unique_tmp("jet_memory_fix");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.jet"), "borrow").unwrap();

    let exact = row("gc", "gc", &["owned"]);
    ledger(&root, &[&exact]);
    let fixed = run(&root, &["fix", "memory"]);
    assert!(fixed.status.success(), "{}", String::from_utf8_lossy(&fixed.stderr));
    assert_cli_snapshot("memory_fix_exact.txt", &fixed.stdout);
    assert_eq!(std::fs::read_to_string(root.join("main.jet")).unwrap(), "owned");

    std::fs::write(root.join("main.jet"), "borrow").unwrap();
    let ambiguous = row("gc", "gc", &["owned", "#Policy(gc) borrow"]);
    ledger(&root, &[&ambiguous]);
    let unchanged = run(&root, &["fix", "memory"]);
    assert!(unchanged.status.success(), "{}", String::from_utf8_lossy(&unchanged.stderr));
    assert_cli_snapshot("memory_fix_ambiguous.txt", &unchanged.stdout);
    let stdout = String::from_utf8_lossy(&unchanged.stdout);
    assert!(stdout.contains("options: main.jet:0..6 observed"));
    assert!(stdout.contains("  - owned"));
    assert!(stdout.contains("  - #Policy(gc) borrow"));
    assert_eq!(std::fs::read_to_string(root.join("main.jet")).unwrap(), "borrow");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn missing_memory_ledger_is_stable_e2112() {
    let root = common::unique_tmp("jet_memory_missing");
    std::fs::create_dir_all(&root).unwrap();
    let output = run(&root, &["audit", "memory"]);
    assert_eq!(output.status.code(), Some(1));
    let actual = String::from_utf8_lossy(&output.stderr)
        .replace(root.to_str().unwrap(), "WORKSPACE");
    assert_eq!(actual, include_str!("cli/memory_ledger_missing_e2112.txt"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn gc_promotions_append_to_the_configured_cross_run_memory_ledger() {
    let root = common::unique_tmp("jet_memory_gc_feed");
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("gc.jet");
    std::fs::write(
        &source,
        r#"#Policy(gc)
#[!Equatable, !Comparable]
enum Link {
    End(Int)
    Next(Link)
}

fn promoted_cycle() => Link {
    first := Link.Next(Link.End(1))
    second := Link.Next(first)
    first = Link.Next(second)
    return first
}

fn run() {
    cycle :: promoted_cycle()
    print(cycle)
}
"#,
    )
    .unwrap();
    let source = source.to_str().unwrap();

    let first = run(&root, &["run", source]);
    assert!(
        first.status.success(),
        "first GC run failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        "Next(Next(Next(End(1))))\n"
    );
    let ledger_path = root.join(".jet/memory/ledger-v1.jsonl");
    let first_ledger = std::fs::read_to_string(&ledger_path).unwrap();
    let first_rows = first_ledger.lines().count();
    assert!(first_rows > 0, "the exercised GC run wrote no witnesses");
    assert!(first_ledger.lines().all(|row| {
        row.contains("\"kind\":\"gc\"")
            && row.contains("\"provenance\":\"")
    }));

    let second = run(&root, &["run", source]);
    assert!(
        second.status.success(),
        "second GC run failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&second.stdout),
        "Next(Next(Next(End(1))))\n"
    );
    let persisted = std::fs::read_to_string(&ledger_path).unwrap();
    assert_eq!(persisted.lines().count(), first_rows * 2);

    let audit = run(&root, &["audit", "memory", "--json"]);
    assert!(
        audit.status.success(),
        "memory audit failed: {}",
        String::from_utf8_lossy(&audit.stderr)
    );
    let audit = String::from_utf8_lossy(&audit.stdout);
    assert!(audit.contains("\"coverage\":\"exercised runs only\""));
    assert!(audit.contains(&format!("\"witnesses\":{}", first_rows * 2)));
    let _ = std::fs::remove_dir_all(root);
}
