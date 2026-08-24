//! D-STRUCT-PLANE1=A: structure rows, facts, gates, inspection registration,
//! and runtime erasure all use the one existing fact machinery.

use jet::Diagnostics::Span;
use jet_foundation::Names::{NameLedger, StructureFact, StructureFactKind};
use jet_foundation::Registry;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const FACT_SOURCE: &str = r#"
fact StructureLiveness(@name: "Structure.Liveness", @holds: .Build, @safe: .Gain, @gates: ["_name"], @decision: "D-STRUCT-PLANE1")

fn run() {
    print("structure facts erase")
}
"#;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn structure_example() -> PathBuf {
    repo().join("examples/features/tooling/structure_plane.jet")
}

fn run_jet(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .current_dir(repo())
        .env("NO_COLOR", "1")
        .args(args)
        .output()
        .expect("run jet")
}

fn successful_stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "jet failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("jet stdout is utf8")
}

#[test]
fn structure_rows_share_the_one_registry_and_law() {
    let rows: Vec<_> = Registry::structure_rows().collect();
    assert_eq!(
        rows.iter().map(|row| row.name).collect::<Vec<_>>(),
        vec![
            "Structure.Liveness",
            "Structure.Lifecycle",
            "Structure.ImportEdge",
        ]
    );
    assert_eq!(rows[0].gates, &["_name"][..]);
    assert_eq!(rows[1].gates, &["policy allow"][..]);
    assert_eq!(rows[2].gates, &["manifest rule edit"][..]);
    for kind in [
        StructureFactKind::Liveness,
        StructureFactKind::Lifecycle,
        StructureFactKind::ImportEdge,
    ] {
        assert!(Registry::structure_row(kind).is_some());
    }
    assert!(rows.iter().all(|row| {
        row.safe_direction != Registry::SafeDirection::None && !row.gates.is_empty()
    }));
    assert!(
        Registry::law_violations().is_empty(),
        "registry drift: {:?}",
        Registry::law_violations()
    );
}

#[test]
fn structure_facts_and_gates_use_one_ledger() {
    let mut names = NameLedger::default();
    let fact = StructureFact::new(
        StructureFactKind::ImportEdge,
        "app.ui -> app.db",
        "package.jet",
        Span::new(11, 20),
        "denied",
        "boundaries[0]",
        Some("manifest rule edit".to_string()),
    );
    names.record_structure_fact(fact.clone());
    names.record_structure_fact(fact);
    assert_eq!(names.structure_facts().len(), 1);

    let mut gates = jet::Sema::GateLedger::GateLedger::default();
    gates.append_structure_facts(&names);
    assert_eq!(gates.entries().len(), 1);
    assert_eq!(
        gates.entries()[0].kind,
        jet::Sema::GateLedger::GateKind::Structure
    );
    assert_eq!(gates.entries()[0].scope, "import-edge");
}

#[test]
fn inspect_structure_is_registered_in_the_cli() {
    let (_, action) = jet::CLI::nested_command("inspect", "structure")
        .expect("ratified structure inspection action");
    assert_eq!(action.usage, "structure [--json] <file.jet>");
    assert_eq!(action.handler.dispatch_word(), "structure");
}

#[test]
fn inspect_structure_text_and_json_expose_the_three_rows() {
    let example = "examples/features/tooling/structure_plane.jet";
    let text = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "structure", example])
        .output()
        .expect("run structure inspection");
    assert!(text.status.success(), "structure text failed: {text:?}");
    let text = String::from_utf8(text.stdout).expect("structure text is utf8");
    for expected in [
        "Structure.Liveness",
        "Structure.Lifecycle",
        "Structure.ImportEdge",
        "safe=gain",
        "safe=shrink",
        "_name",
        "policy allow",
        "manifest rule edit",
    ] {
        assert!(text.contains(expected), "missing {expected} in {text}");
    }

    let json = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["inspect", "structure", "--json", example])
        .output()
        .expect("run JSON structure inspection");
    assert!(json.status.success(), "structure JSON failed: {json:?}");
    let json = String::from_utf8(json.stdout).expect("structure JSON is utf8");
    assert!(json.starts_with("{\"schema_version\":1,"));
    for expected in [
        "Structure.Liveness",
        "Structure.Lifecycle",
        "Structure.ImportEdge",
        "\"safe_direction\":\"gain\"",
        "\"safe_direction\":\"shrink\"",
        "policy allow",
        "manifest rule edit",
    ] {
        assert!(json.contains(expected), "missing {expected} in {json}");
    }
}

#[test]
fn inspect_structure_text_and_json_match_the_fact_report_goldens() {
    let example = "examples/features/tooling/structure_plane.jet";
    let text = successful_stdout(run_jet(&["inspect", "structure", example]));
    let expected_text = fs::read_to_string(
        repo().join("examples/features/expected/tooling/structure_plane.structure.out"),
    )
    .expect("structure text golden");
    assert_eq!(text, expected_text);

    let json = successful_stdout(run_jet(&["inspect", "structure", "--json", example]));
    let expected_json = fs::read_to_string(
        repo().join("examples/features/expected/tooling/structure_plane.structure.json"),
    )
    .expect("structure JSON golden");
    assert_eq!(json, expected_json);
}

#[test]
fn structure_plane_keeps_parser_sema_tir_and_runtime_tier_parity() {
    let path = structure_example();
    let source = fs::read_to_string(&path).expect("structure example source");
    let (tokens, lexer_diags) = jet::Lexer::lex(&source);
    assert!(lexer_diags.is_empty(), "lexer diagnostics: {lexer_diags:?}");
    jet::Parser::parse(&tokens).expect("structure example parses");

    let shown = path.to_str().expect("structure example path is utf8");
    let mut bundle = jet::Loader::load_entry(shown).expect("structure example loads");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Lint),
        "structure sema diagnostics: {diagnostics:#?}"
    );
    let mut diagnostic_codes: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    diagnostic_codes.sort_unstable();
    assert_eq!(diagnostic_codes, ["L0104", "L2001"]);
    let kinds: Vec<_> = bundle
        .name_ledger
        .structure_facts()
        .iter()
        .map(|fact| fact.kind)
        .collect();
    for kind in [
        StructureFactKind::Liveness,
        StructureFactKind::Lifecycle,
        StructureFactKind::ImportEdge,
    ] {
        assert!(kinds.contains(&kind), "missing {kind:?} in {kinds:?}");
    }
    jet::Codegen::TIR::lower_jit_program(&bundle).expect("structure example lowers to TIR");

    let cache =
        std::env::temp_dir().join(format!("jet_structure_plane_tiers_{}", std::process::id()));
    let _ = fs::remove_dir_all(&cache);
    for (mode, args) in [
        ("release", vec!["run", "--release", shown]),
        ("default", vec!["run", shown]),
        ("interpret", vec!["run", "--interpret", shown]),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .current_dir(repo())
            .env("NO_COLOR", "1")
            .env("JET_RUN_CACHE_DIR", cache.join(mode).join("run"))
            .env("JET_CACHE_DIR", cache.join(mode).join("build"))
            .args(args)
            .output()
            .expect("run structure example tier");
        assert!(
            output.status.success(),
            "{mode} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"structure fact plane\n", "{mode} output");
    }

    let dev = Command::new(env!("CARGO_BIN_EXE_jet"))
        .current_dir(repo())
        .env("NO_COLOR", "1")
        .env("JET_RUN_CACHE_DIR", cache.join("dev").join("run"))
        .env("JET_CACHE_DIR", cache.join("dev").join("build"))
        .args(["dev", shown, "--watch=off"])
        .output()
        .expect("run structure example through dev");
    assert!(
        dev.status.success(),
        "dev failed: stdout={} stderr={}",
        String::from_utf8_lossy(&dev.stdout),
        String::from_utf8_lossy(&dev.stderr)
    );
    assert_eq!(dev.stdout, b"structure fact plane\n", "dev output");
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn structure_plane_web_compile_erases_the_fact_plane() {
    let rustc = Command::new("rustc")
        .args([
            "--print",
            "target-libdir",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .output()
        .expect("probe wasm target");
    if !rustc.status.success() {
        eprintln!("note: skipping structure web tier proof (wasm target unavailable)");
        return;
    }
    let example = "examples/features/tooling/structure_plane.jet";
    let output = run_jet(&["build", "--target=web", example]);
    assert!(
        output.status.success(),
        "web build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn structure_fact_declaration_erases_before_codegen() {
    let output = jet::compile(FACT_SOURCE).expect("structure fact fixture compiles");
    assert!(output.rust.contains("structure facts erase"));
    assert!(!output.rust.contains("Structure.Liveness"));
    assert!(!output.rust.contains("@holds"));
    assert!(!output.rust.contains("manifest rule edit"));
}

#[test]
fn structure_fact_declaration_erases_for_web_codegen() {
    let output = jet::compile_web_with_path(FACT_SOURCE, "structure_fact_plane_web.jet")
        .expect("structure fact fixture compiles for web");
    let web = output.web.expect("web artifacts are present");
    for generated in [&web.wasm_rust, &web.js_app] {
        assert!(!generated.contains("Structure.Liveness"));
        assert!(!generated.contains("@holds"));
        assert!(!generated.contains("manifest rule edit"));
    }
}
