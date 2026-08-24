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
#Scrub(Input) fn scrub(raw: #Input String) String -> { ~raw }
#MustUse fn discardable() Int -> { 1 }

fn run() {
    #Abilities(caps: IO) {}
    discardable().drop("intentional result discard")
    detached :: task 42
    detached.detach()
    approx(1)
    wrapping(1 + 2)
    Thirdish.from_meter_rounded(1meter, .NearestEven, digits: 0).drop("rounded conversion gate")
}
"#,
    )
    .unwrap();
}

fn write_tier_fixture(root: &Path) {
    fs::write(root.join("tier.jet"), knowledge_tier_source()).unwrap();
}

fn knowledge_tier_source() -> &'static str {
    include_str!("fixtures/knowledge_tier.jet")
}

fn knowledge_tier_web_source() -> &'static str {
    include_str!("fixtures/knowledge_tier_web.jet")
}

// D-TYPE2-NUM1 / card #1550 / c9: the number grid keeps one executable
// meaning across every compiler-facing and hosted tier. Reuse the ratified
// range example and its golden output; this matrix adds the missing per-tier
// proof without creating a second numeric example or spelling.
const NUMBER_GRID_EXAMPLE: &str = include_str!("../examples/features/types/range_types.jet");
const NUMBER_GRID_EXPECTED: &str =
    include_str!("../examples/features/expected/types/range_types.out");
const NUMBER_GRID_WEB_SOURCE: &str = r#"#Target(Web)
fn set_brightness(level: Int(0..100)) Int(0..100) -> level

fn run() {
    print(set_brightness(42))
    print(Int(0..100).from_int(3))
}
"#;

fn have_tool(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn have_wasm_target() -> bool {
    Command::new("rustc")
        .args([
            "--print",
            "target-libdir",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn checked_number_grid_bundle(tag: &str) -> (common::Scratch, jet::AST::ProgramBundle) {
    let scratch = common::Scratch::new(tag);
    let entry = scratch.join("range_types.jet");
    fs::write(&entry, NUMBER_GRID_EXAMPLE).unwrap();
    let shown = entry.to_string_lossy().into_owned();
    let mut bundle = jet::Loader::load_entry(&shown).expect("number-grid example must load");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics.is_empty(),
        "number-grid sema failed: {diagnostics:#?}"
    );
    (scratch, bundle)
}

fn assert_inline_int_range(ty: &jet::AST::Type, lo: i64, hi: i64) {
    match ty {
        jet::AST::Type::InlineRange {
            base,
            lo: actual_lo,
            hi: actual_hi,
        } => {
            assert_eq!(base.as_ref(), &jet::AST::Type::Int);
            assert_eq!((*actual_lo, *actual_hi), (lo, hi));
        }
        other => panic!("expected Int({lo}..{hi}), got {other:?}"),
    }
}

fn assert_tier_output(output: Output) {
    let text = stdout(&output);
    assert_tier_text(&text);
}

fn assert_tier_text(text: &str) {
    for expected in [
        "42",
        "comptime_range=9",
        "comptime_exactness=true",
        "comptime_unit=2.0",
        "comptime_state=state",
        "comptime_classification=world",
        "exactness=true",
        "unit=2.0",
        "state=state",
        "classification=world",
        "range=9",
    ] {
        assert!(text.contains(expected), "expected {expected:?}, got {text}");
    }
}

fn assert_knowledge_ledger(json: &str) {
    assert!(
        json.starts_with("{\"schema\":\"jet.report/v1\"")
            && json.contains("\"gates\":{\"entries\":["),
        "{json}"
    );
    for subject in [
        "approx",
        "wrapping",
        "from_half_rounded",
        ".raw",
        "#Transition(_, Pending)",
        "#Transition(Pending, Confirmed)",
        "#Scrub(Input)",
    ] {
        assert!(
            json.contains(&format!("\"subject\":\"{subject}\"")),
            "missing {subject} in {json}"
        );
    }
    assert_eq!(
        json.matches("\"kind\":\"state_transition\"").count(),
        2,
        "{json}"
    );
    assert_eq!(
        json.matches("\"kind\":\"taint_scrub\"").count(),
        1,
        "{json}"
    );
    assert_eq!(
        json.matches("\"kind\":\"precision_demotion\"").count(),
        5,
        "{json}"
    );
    let scrub = json.find("\"kind\":\"taint_scrub\"").expect("taint row");
    let state = json
        .find("\"kind\":\"state_transition\"")
        .expect("state row");
    let precision = json
        .find("\"kind\":\"precision_demotion\"")
        .expect("precision row");
    assert!(
        scrub < state && state < precision,
        "knowledge rows drifted: {json}"
    );
    assert!(json.contains("\"span\":{\"start\":"), "{json}");
    assert!(
        json.contains("\"reason\":\"#Transition(Pending, Confirmed)\""),
        "{json}"
    );
    assert!(json.contains("tier.jet:"), "{json}");
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
    assert!(
        json.starts_with("{\"schema_version\":1,\"entries\":["),
        "{json}"
    );
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
fn authority_ledger_mirrors_manifest_and_lock_block_shape() {
    let scratch = common::Scratch::new("authority-ledger");
    fs::write(
        scratch.join("package.jet"),
        r#"name: "ledger"
version: "0.1.0"
authority: .{
    holds: { allow: [IO], deny: [Exec] },
    grants: { "image-codec": [FS.Read] },
    trust: { default: prompt, ci: { prompt: deny }, services: { stripe: allow } },
    providers: { nix: { registry: "nixpkgs", deny: ["openssl-1.0"] } },
}
"#,
    )
    .unwrap();
    fs::write(scratch.join("run.jet"), "fn run() { print(\"ledger\") }\n").unwrap();
    fs::create_dir_all(scratch.join(".jet")).unwrap();
    fs::write(
        scratch.join(".jet/lock"),
        r#"version = 1

[root]
dependencies = []
authority = .{ holds: { allow: [IO], deny: [Exec] }, grants: { "image-codec": [FS.Read] }, trust: { default: prompt, ci: { prompt: deny }, services: { stripe: allow } }, providers: { nix: { registry: "nixpkgs", deny: ["openssl-1.0"] } } }
"#,
    )
    .unwrap();

    let json = stdout(&run(
        &scratch.path,
        &["inspect", "authority", "--json", "run.jet"],
    ));
    for subject in [
        "authority.holds.allow",
        "authority.holds.deny",
        "image-codec",
        "authority.trust.default",
        "authority.trust.ci",
        "authority.trust.services.stripe",
        "authority.providers.nix",
    ] {
        assert!(
            json.contains(&format!("\"subject\":\"{subject}\"")),
            "{subject}: {json}"
        );
    }
    assert!(
        json.contains(".jet/lock"),
        "lock authority provenance missing: {json}"
    );
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
        assert!(
            json.contains(&format!("\"kind\":\"{kind}\"")),
            "{kind}: {json}"
        );
    }
    for subject in ["approx", "wrapping", "from_meter_rounded"] {
        let marker = format!("\"subject\":\"{subject}\"");
        let subject_at = json.find(&marker).expect("precision gate subject");
        let entry_at = json[..subject_at]
            .rfind("{\"kind\":\"")
            .expect("precision gate entry");
        assert!(
            json[entry_at..subject_at].starts_with("{\"kind\":\"precision_demotion\""),
            "precision gate {subject} did not use exact ledger kind: {json}"
        );
    }
    assert!(json.contains("intentional result discard"), "{json}");
    assert!(json.contains("#Scrub(Input)"), "{json}");
    assert!(
        json.contains("\"subject\":\"from_meter_rounded\""),
        "{json}"
    );
    assert!(json.contains("source-gates.jet:"), "{json}");
}

#[test]
fn range_knowledge_gate_has_three_tier_example_parity() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = "examples/features/types/range_types.jet";
    let release = stdout(&run(root, &["run", "--release", example]));
    let default = stdout(&run(root, &["run", example]));
    let interpret = stdout(&run(root, &["run", "--interpret", example]));
    let expected =
        fs::read_to_string(root.join("examples/features/expected/types/range_types.out"))
            .expect("range_types golden output");
    assert_eq!(release, expected);
    assert_eq!(default, expected);
    assert_eq!(interpret, expected);
}

#[test]
fn ledger_json_and_generated_rust_are_stable_across_two_builds() {
    let scratch = common::Scratch::new("gate-stability");
    fs::write(
        scratch.join("plain.jet"),
        "fn run() { print(\"stable\") }\n",
    )
    .unwrap();

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
    assert_eq!(
        rust_a, rust_after_ledger,
        "ledger inspection changed generated Rust"
    );
    assert_eq!(ledger_a, ledger_b, "ledger JSON changed between builds");
}

#[test]
fn structure_inspection_is_read_only_and_structure_facts_erase_before_runtime() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = "examples/features/tooling/structure_plane.jet";

    let rust_before = stdout(&run(root, &["emit", "--rust", example]));
    let run_before = stdout(&run(root, &["run", example]));
    let release_before = stdout(&run(root, &["run", "--release", example]));

    let structure = stdout(&run(root, &["inspect", "structure", example]));
    let structure_json = stdout(&run(root, &["inspect", "structure", "--json", example]));
    for fact in ["import-edge", "lifecycle", "liveness"] {
        assert!(structure.contains(fact), "missing {fact}: {structure}");
        assert!(
            structure_json.contains(fact),
            "missing {fact}: {structure_json}"
        );
    }
    assert!(structure.contains("provenance"), "{structure}");
    assert!(
        structure_json.contains("\"provenance\""),
        "{structure_json}"
    );

    let rust_after = stdout(&run(root, &["emit", "--rust", example]));
    let run_after = stdout(&run(root, &["run", example]));
    let release_after = stdout(&run(root, &["run", "--release", example]));

    assert_eq!(
        rust_before, rust_after,
        "structure inspection changed AOT Rust"
    );
    assert_eq!(
        run_before, run_after,
        "structure inspection changed JIT output"
    );
    assert_eq!(
        release_before, release_after,
        "structure inspection changed AOT output"
    );
    for erased in [
        "Structure.Liveness",
        "Structure.Lifecycle",
        "Structure.ImportEdge",
        "policy allow",
        "manifest rule edit",
    ] {
        assert!(
            !rust_before.contains(erased),
            "structure policy leaked into Rust: {erased}"
        );
    }
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
        &["inspect", "gates", "--gate", "unsafe=allow", "heavy.jet"],
    ));
    let security = human.find("build_flag:").expect(&human);
    let numeric = human.find("precision_demotion: 20 entries").expect(&human);
    assert!(
        security < numeric,
        "security rows must precede numeric summary: {human}"
    );
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
    assert!(parsed.contains("wrapping"), "{parsed}");
    assert!(parsed.contains("from_meter_rounded"), "{parsed}");
}

#[test]
fn i9_parser_tier_keeps_all_knowledge_plane_gate_sources() {
    let scratch = common::Scratch::new("gate-tier-parser-knowledge");
    write_tier_fixture(&scratch.path);
    let parsed = stdout(&run(
        &scratch.path,
        &["inspect", "compiler", "parse", "tier.jet"],
    ));
    assert!(parsed.contains("\"operation\":\"parse\""), "{parsed}");
    for gate in [
        "approx",
        "wrapping",
        "from_half_rounded",
        "#Transition",
        "#Scrub",
    ] {
        assert!(parsed.contains(gate), "missing {gate} in {parsed}");
    }
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
    assert!(
        ledger.contains("\"kind\":\"precision_demotion\""),
        "{ledger}"
    );
    assert!(ledger.contains("\"subject\":\"approx\""), "{ledger}");
    assert!(ledger.contains("\"subject\":\"wrapping\""), "{ledger}");
    assert!(
        ledger.contains("\"subject\":\"from_meter_rounded\""),
        "{ledger}"
    );
}

#[test]
fn i9_sema_tier_records_all_knowledge_plane_gates_stably() {
    let scratch = common::Scratch::new("gate-tier-sema-knowledge");
    write_tier_fixture(&scratch.path);
    let first = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--json", "tier.jet"],
    ));
    let second = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--json", "tier.jet"],
    ));
    assert_eq!(first, second, "knowledge ledger JSON is not stable");
    assert_knowledge_ledger(&first);

    let state = stdout(&run(
        &scratch.path,
        &["inspect", "gates", "--kind", "state_transition", "tier.jet"],
    ));
    assert!(state.contains("state_transition: 2"), "{state}");
    assert!(state.contains("#Transition(Pending, Confirmed)"), "{state}");
}

#[test]
fn i9_tir_tier_keeps_the_fixture_behavior() {
    let scratch = common::Scratch::new("gate-tier-tir");
    write_tier_fixture(&scratch.path);
    assert_tier_output(run(&scratch.path, &["run", "--interpret", "tier.jet"]));
}

#[test]
fn i9_aot_tier_keeps_the_fixture_behavior() {
    if !common::have_rustc() {
        eprintln!("note: skipping gate AOT tier proof (need rustc)");
        return;
    }
    let scratch = common::Scratch::new("gate-tier-aot");
    write_tier_fixture(&scratch.path);
    assert_tier_output(run(&scratch.path, &["run", "--release", "tier.jet"]));
}

#[test]
fn i9_jit_and_dev_tiers_keep_the_fixture_behavior() {
    let scratch = common::Scratch::new("gate-tier-jit-dev");
    write_tier_fixture(&scratch.path);
    assert_tier_output(run(&scratch.path, &["run", "tier.jet"]));
    assert_tier_output(run(&scratch.path, &["dev", "tier.jet", "--watch=off"]));
}

#[test]
fn i9_comptime_tier_keeps_the_compile_time_value() {
    let scratch = common::Scratch::new("gate-tier-comptime");
    write_tier_fixture(&scratch.path);
    let text = stdout(&run(&scratch.path, &["run", "tier.jet"]));
    assert_eq!(
        text,
        "42\ncomptime_range=9\ncomptime_exactness=true\ncomptime_unit=2.0\ncomptime_state=state\ncomptime_classification=world\nexactness=true\nunit=2.0\nstate=state\nclassification=world\nrange=9\n"
    );
}

#[test]
fn i9_repl_tier_keeps_the_fixture_behavior() {
    let scratch = common::Scratch::new("gate-tier-repl");
    write_tier_fixture(&scratch.path);
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
        .write_all(b":load tier.jet\n:run\n:quit\n")
        .expect("write REPL input");
    let output = child.wait_with_output().expect("finish REPL");
    let text = stdout(&output);
    assert_tier_text(&text);
    assert!(
        text.contains("loaded"),
        "expected REPL fixture load, got {text}"
    );
}

#[test]
fn i9_web_tier_keeps_the_fixture_buildable() {
    if !common::have_rustc() || !have_tool("node") {
        eprintln!("note: skipping gate web tier proof (need rustc and node)");
        return;
    }
    let scratch = common::Scratch::new("gate-tier-web");
    fs::write(scratch.join("web.jet"), knowledge_tier_web_source()).unwrap();
    let _ = stdout(&run(&scratch.path, &["build", "--target=web", "web.jet"]));
}

#[test]
fn i9_number_grid_parser_keeps_int_ranges_as_one_surface() {
    let (tokens, diagnostics) = jet::Lexer::lex(NUMBER_GRID_EXAMPLE);
    assert!(
        diagnostics.is_empty(),
        "number-grid lexer diagnostics: {diagnostics:?}"
    );
    let program = jet::Parser::parse(&tokens).expect("number-grid example must parse");

    let severity = program
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Distinct(def) if def.name == "Severity" => Some(def),
            _ => None,
        })
        .expect("number-grid distinct range declaration");
    assert_eq!(severity.range.map(|(lo, hi, _)| (lo, hi)), Some((0, 10)));

    let setter = program
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Func(func) if func.name == "set_brightness" => Some(func),
            _ => None,
        })
        .expect("number-grid inline range function");
    assert_inline_int_range(&setter.params[0].ty, 0, 100);
    assert_inline_int_range(
        setter.return_type.as_ref().expect("setter return type"),
        0,
        100,
    );
}

#[test]
fn i9_number_grid_sema_records_the_shared_interval_facts() {
    let (_scratch, bundle) = checked_number_grid_bundle("number-grid-sema");
    let module = &bundle.modules[bundle.entry];
    let severity = module
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Distinct(def) if def.name == "Severity" => Some(def),
            _ => None,
        })
        .expect("sema must retain the distinct interval");
    assert_eq!(severity.range.map(|(lo, hi, _)| (lo, hi)), Some((0, 10)));
}

#[test]
fn i9_number_grid_tir_consumes_the_interval_and_keeps_output() {
    let (_scratch, bundle) = checked_number_grid_bundle("number-grid-tir");
    let program = jet::Codegen::TIR::lower_jit_program(&bundle)
        .expect("number-grid example must lower through TIR");
    assert_eq!(program.distinct_ranges.get("Severity"), Some(&(0, 10)));

    let mut sink = jet::Comptime::DevSink::default();
    jet::Codegen::TIR::run_program(
        &program,
        &bundle.project_root,
        &mut sink,
        std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        jet::Policy::GateSet::allow(jet::Policy::PolicyKey::Impure),
    )
    .expect("number-grid TIR evaluation must succeed");
    assert_eq!(sink.stdout, NUMBER_GRID_EXPECTED);
}

#[test]
fn i9_number_grid_aot_keeps_the_golden_behavior() {
    if !common::have_rustc() {
        eprintln!("note: skipping number-grid AOT tier proof (need rustc)");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = stdout(&run(
        root,
        &[
            "run",
            "--release",
            "examples/features/types/range_types.jet",
        ],
    ));
    assert_eq!(output, NUMBER_GRID_EXPECTED);
}

#[test]
fn i9_number_grid_jit_keeps_the_golden_behavior() {
    let scratch = common::Scratch::new("number-grid-jit");
    fs::write(scratch.join("range_types.jet"), NUMBER_GRID_EXAMPLE).unwrap();
    let output = stdout(&run(&scratch.path, &["run", "range_types.jet"]));
    assert_eq!(output, NUMBER_GRID_EXPECTED);
}

#[test]
fn i9_number_grid_dev_keeps_the_golden_behavior() {
    let scratch = common::Scratch::new("number-grid-dev");
    fs::write(scratch.join("range_types.jet"), NUMBER_GRID_EXAMPLE).unwrap();
    let output = stdout(&run(
        &scratch.path,
        &["dev", "range_types.jet", "--watch=off"],
    ));
    assert_eq!(output, NUMBER_GRID_EXPECTED);
}

#[test]
fn i9_number_grid_comptime_keeps_the_exact_inline_value() {
    let (_scratch, bundle) = checked_number_grid_bundle("number-grid-comptime");
    let constant = bundle.modules[bundle.entry]
        .items
        .iter()
        .find_map(|item| match item {
            jet::AST::Item::Const(def) if def.name == "comptime_brightness" => Some(def),
            _ => None,
        })
        .expect("number-grid comptime binding");
    assert!(constant.is_comptime);
    assert!(matches!(
        constant.ct.as_ref(),
        Some(jet::AST::CtValue::Int(42))
    ));
}

#[test]
fn i9_number_grid_repl_keeps_the_inline_range_behavior() {
    let transcript = jet::REPL::run_transcript(
        &[
            "fn set_brightness(level: Int(0..100)) Int(0..100) -> level",
            "fn checked_inline(raw: Int) Int(0..100) String! -> Int(0..100).from_int(raw)",
            "print(set_brightness(42))",
            "print(checked_inline(3) ?? Int(0..100).from_int(0))",
        ],
        None,
    );
    assert_eq!(transcript, "ok\nok\n42\n3\n");
}

#[test]
fn i9_number_grid_web_keeps_the_shared_runtime_behavior() {
    let output =
        jet::compile_web_with_path(NUMBER_GRID_WEB_SOURCE, "tests/fixtures/number_grid_web.jet")
            .unwrap_or_else(|diagnostics| {
                panic!("number-grid web source was rejected: {diagnostics:#?}")
            });
    let web = output
        .web
        .expect("number-grid web target must produce artifacts");
    assert!(web.js_app.contains("function jet_inline_range_from_int"));
    assert!(web.wasm_rust.contains("jet_inline_range_from_int"));

    if !have_tool("rustc") || !have_tool("node") || !have_wasm_target() {
        eprintln!("note: skipping number-grid web execution (need rustc, node, and wasm32 target)");
        return;
    }

    let scratch = common::Scratch::new("number-grid-web");
    fs::write(scratch.join("app.js"), &web.js_app).unwrap();
    fs::write(scratch.join("jet_dom_runtime.js"), &web.dom_runtime).unwrap();
    fs::write(scratch.join("app_wasm.rs"), &web.wasm_rust).unwrap();
    fs::write(scratch.join("package.json"), r#"{"type":"module"}"#).unwrap();

    let wasm = Command::new("rustc")
        .current_dir(&scratch.path)
        .args([
            "--edition",
            "2021",
            "--target",
            "wasm32-unknown-unknown",
            "--crate-type",
            "cdylib",
            "-O",
            "app_wasm.rs",
            "-o",
            "app.wasm",
        ])
        .output()
        .expect("spawn number-grid web rustc");
    assert!(
        wasm.status.success(),
        "rustc rejected number-grid web output: {}",
        String::from_utf8_lossy(&wasm.stderr)
    );

    let node = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .expect("spawn number-grid web app");
    assert!(
        node.status.success(),
        "number-grid web app failed: stdout={} stderr={}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&node.stdout), "42\n3\n");
}
