//! D-FACT-GATE1=A: one read model, stable projections, and compile-time cost.

mod common;

use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn jet() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_jet"))
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(jet())
        .current_dir(root)
        .args(args)
        .output()
        .expect("run jet")
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    String::from_utf8(output.stdout.clone()).expect("utf8 stdout")
}

fn write_gates(root: &Path) {
    fs::write(
        root.join("gates.jet"),
        r#"fn run() {
    #Unsafe("test unsafe reason") {
        print("unsafe row")
    }
    #Impure("test impure reason") {}
    #Nondeterministic("test nondeterministic reason") {}
}
"#,
    )
    .unwrap();
}

fn write_source_gate_kinds(root: &Path) {
    fs::write(
        root.join("source-gates.jet"),
        r#"#UnitFamily(Length, base: meter) {
    meter
    thirdish(scale: 2/3)
}
tag Input { deny: [IO] }
#Scrub(Input) fn scrub(raw: #Input String) => String { return ~raw }

fn run() {
    #Grant(caps: IO) {}
    maybe().drop("intentional result discard")
    task.detach()
    approx(1)
    wrapping(1 + 2)
    Thirdish.from_meter_rounded(1meter, .NearestEven, digits: 0).drop("rounded conversion gate")
}
"#,
    )
    .unwrap();
}

fn write_tier_fixture(root: &Path) {
    fs::write(
        root.join("tier.jet"),
        "@answer :: 40 + 2\n\nfn run() {\n    print(\"{@answer}\")\n}\n",
    )
    .unwrap();
}

fn have_tool(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn assert_tier_output(output: Output) {
    let text = stdout(&output);
    assert!(text.contains("42"), "expected tier output 42, got {text}");
}

#[test]
fn full_and_filtered_views_keep_one_provenance_ledger() {
    let scratch = common::Scratch::new("gate-views");
    write_gates(&scratch.path);

    let human = stdout(&run(&scratch.path, &["inspect", "gates", "gates.jet"]));
    assert!(human.contains("gates:"), "{human}");
    assert!(human.contains("unsafe:"), "{human}");
    assert!(human.contains("impure:"), "{human}");
    assert!(human.contains("nondeterministic:"), "{human}");

    let json = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--json", "gates.jet"],
    ));
    assert!(json.starts_with("{\"schema_version\":1,\"entries\":["), "{json}");
    assert!(json.contains("\"provenance\":["), "{json}");
    assert!(json.contains("test unsafe reason"), "{json}");

    let kind = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--kind", "impure", "gates.jet"],
    ));
    assert!(kind.contains("impure: 1"), "{kind}");
    assert!(!kind.contains("unsafe:"), "{kind}");

    let scope = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--scope", "block", "gates.jet"],
    ));
    assert!(scope.contains("impure:"), "{scope}");
    assert!(!scope.contains("unsafe:"), "{scope}");

    let authority = stdout(&run(
        &scratch.path,
        &["inspect", "authority", "--json", "gates.jet"],
    ));
    assert!(authority.contains("\"kind\":\"unsafe\""), "{authority}");
    assert!(!authority.contains("precision_demotion"), "{authority}");

    let unsafe_view = stdout(&run(
        &scratch.path,
        &["inspect", "unsafe", "--json", "gates.jet"],
    ));
    assert!(unsafe_view.contains("\"gates\":["), "{unsafe_view}");
    assert!(!unsafe_view.contains("test impure reason"), "{unsafe_view}");
}

#[test]
fn source_gate_kinds_keep_their_written_reasons() {
    let scratch = common::Scratch::new("gate-source-kinds");
    write_source_gate_kinds(&scratch.path);

    let json = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--json", "source-gates.jet"],
    ));
    for kind in [
        "dependency_grant",
        "taint_scrub",
        "duty_drop",
        "precision_demotion",
    ] {
        assert!(json.contains(&format!("\"kind\":\"{kind}\"")), "{kind}: {json}");
    }
    assert!(json.contains("intentional result discard"), "{json}");
    assert!(json.contains("#Scrub(Input)"), "{json}");
    assert!(json.contains("\"subject\":\"from_meter_rounded\""), "{json}");
    assert!(json.contains("source-gates.jet:"), "{json}");
}

#[test]
fn range_knowledge_gate_has_three_tier_example_parity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = "examples/features/types/range_types.jet";
    let release = stdout(&run(root, &["run", "--release", example]));
    let default = stdout(&run(root, &["run", example]));
    let interpret = stdout(&run(root, &["run", "--interpret", example]));
    let expected = "7\n3\n10\ntrue\n7\n15\n";
    assert_eq!(release, expected);
    assert_eq!(default, expected);
    assert_eq!(interpret, expected);
}

#[test]
fn ledger_json_and_generated_rust_are_stable_across_two_builds() {
    let scratch = common::Scratch::new("gate-stability");
    fs::write(scratch.join("plain.jet"), "fn run() { print(\"stable\") }\n").unwrap();

    let rust_a = stdout(&run(&scratch.path, &["emit", "--rust", "plain.jet"]));
    stdout(&run(&scratch.path, &["build", "plain.jet"]));
    let ledger_a = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--json", "plain.jet"],
    ));
    let rust_after_ledger = stdout(&run(&scratch.path, &["emit", "--rust", "plain.jet"]));
    stdout(&run(&scratch.path, &["build", "plain.jet"]));
    let rust_b = stdout(&run(&scratch.path, &["emit", "--rust", "plain.jet"]));
    let ledger_b = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--json", "plain.jet"],
    ));

    assert_eq!(rust_a, rust_b, "ledger inspection changed generated Rust");
    assert_eq!(rust_a, rust_after_ledger, "ledger inspection changed generated Rust");
    assert_eq!(ledger_a, ledger_b, "ledger JSON changed between builds");
}

#[test]
fn heavy_numeric_kind_is_summarized_after_security_rows() {
    let scratch = common::Scratch::new("gate-heavy");
    let mut source = String::from("fn run() {\n");
    for index in 0..20 {
        source.push_str(&format!("    value{index} :: wrapping(1 + 2)\n"));
    }
    source.push_str("}\n");
    fs::write(scratch.join("heavy.jet"), source).unwrap();

    let human = stdout(&run(
        &scratch.path,
        &[
            "inspect",
            "gates",
            "--gate",
            "unsafe=allow",
            "heavy.jet",
        ],
    ));
    let security = human.find("build_flag:").expect(&human);
    let numeric = human.find("precision_demotion: 20 entries").expect(&human);
    assert!(security < numeric, "security rows must precede numeric summary: {human}");
}

#[test]
fn i9_parser_tier_keeps_the_gate_source() {
    let scratch = common::Scratch::new("gate-tier-parser");
    write_source_gate_kinds(&scratch.path);
    let parsed = stdout(&run(
        &scratch.path,
        &["inspect", "compiler", "parse", "source-gates.jet"],
    ));
    assert!(parsed.contains("\"operation\":\"parse\""), "{parsed}");
    assert!(parsed.contains("#Scrub"), "{parsed}");
}

#[test]
fn i9_sema_tier_reads_the_same_gate_ledger() {
    let scratch = common::Scratch::new("gate-tier-sema");
    write_source_gate_kinds(&scratch.path);
    let ledger = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--json", "source-gates.jet"],
    ));
    assert!(ledger.contains("\"schema_version\":1"), "{ledger}");
    assert!(ledger.contains("\"kind\":\"dependency_grant\""), "{ledger}");
    assert!(ledger.contains("\"kind\":\"precision_demotion\""), "{ledger}");
}

#[test]
fn i9_tir_tier_keeps_the_fixture_behavior() {
    let scratch = common::Scratch::new("gate-tier-tir");
    write_tier_fixture(&scratch.path);
    assert_tier_output(run(&scratch.path, &["run", "--interpret", "tier.jet"]));
}

#[test]
fn i9_aot_tier_keeps_the_fixture_buildable() {
    if !common::have_rustc() {
        eprintln!("note: skipping gate AOT tier proof (need rustc)");
        return;
    }
    let scratch = common::Scratch::new("gate-tier-aot");
    write_tier_fixture(&scratch.path);
    let _ = stdout(&run(&scratch.path, &["build", "tier.jet"]));
}

#[test]
fn i9_jit_and_dev_tiers_keep_the_fixture_behavior() {
    let scratch = common::Scratch::new("gate-tier-jit-dev");
    write_tier_fixture(&scratch.path);
    assert_tier_output(run(&scratch.path, &["run", "tier.jet"]));
    assert_tier_output(run(
        &scratch.path,
        &["dev", "tier.jet", "--watch=off"],
    ));
}

#[test]
fn i9_comptime_tier_keeps_the_compile_time_value() {
    let scratch = common::Scratch::new("gate-tier-comptime");
    write_tier_fixture(&scratch.path);
    let rust = stdout(&run(&scratch.path, &["emit", "--rust", "tier.jet"]));
    assert!(rust.contains("42"), "comptime value was not emitted: {rust}");
}

#[test]
fn i9_repl_tier_keeps_the_fixture_behavior() {
    let scratch = common::Scratch::new("gate-tier-repl");
    let mut child = Command::new(jet())
        .current_dir(&scratch.path)
        .args(["repl"])
        .env("XDG_STATE_HOME", scratch.path.join("state"))
        .env_remove("JET_REPL_HISTORY")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start REPL");
    child
        .stdin
        .as_mut()
        .expect("REPL stdin")
        .write_all(b"40 + 2\n:quit\n")
        .expect("write REPL input");
    assert_tier_output(child.wait_with_output().expect("finish REPL"));
}

#[test]
fn i9_web_tier_keeps_the_fixture_buildable() {
    if !common::have_rustc() || !have_tool("node") {
        eprintln!("note: skipping gate web tier proof (need rustc and node)");
        return;
    }
    let scratch = common::Scratch::new("gate-tier-web");
    fs::write(
        scratch.join("web.jet"),
        "#Target(Web)\nfn run() { print(\"tier-parity\") }\n",
    )
    .unwrap();
    let _ = stdout(&run(
        &scratch.path,
        &["build", "--target=web", "web.jet"],
    ));
}
