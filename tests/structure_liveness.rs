//! D-STRUCT-LIVE1=A: the four liveness verdicts share the structure fact plane,
//! policy gate, source fixes, and every applicable execution tier.

use jet_foundation::Names::StructureFactKind;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn example() -> &'static str {
    "examples/features/tooling/structure_liveness.jet"
}

fn run_jet(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_jet"))
        .current_dir(repo())
        .env("NO_COLOR", "1")
        .args(args)
        .output()
        .expect("run jet")
}

fn assert_liveness_warnings(label: &str, output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    for code in ["L0101", "L0103", "L0104", "L0105"] {
        assert!(stderr.contains(code), "{label} missing {code}: {stderr}");
    }
}

fn assert_liveness_policy_denied(label: &str, output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "{label} unexpectedly ran: {stderr}"
    );
    assert!(output.stdout.is_empty(), "{label} executed user code");
    assert!(
        stderr.contains("E1293") && stderr.contains("L0101"),
        "{label} missing denied liveness lint: {stderr}"
    );
    assert!(
        !stderr.contains("Warning [L0101]"),
        "{label} printed denied lint as warning: {stderr}"
    );
}

#[test]
fn liveness_facts_and_fixes_are_one_checked_result() {
    let source = fs::read_to_string(repo().join(example())).expect("liveness example source");
    let mut bundle = jet::Loader::load_entry(example()).expect("liveness example loads");
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);

    let mut codes: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect();
    codes.sort_unstable();
    assert_eq!(codes, ["L0101", "L0103", "L0104", "L0105"]);
    assert!(diagnostics.iter().all(|diagnostic| {
        diagnostic.severity == jet::Diagnostics::Severity::Lint
            && diagnostic
                .edit
                .as_ref()
                .is_some_and(|edit| edit.new_text.is_empty() || edit.new_text.starts_with('_'))
    }));

    let facts: Vec<_> = bundle
        .name_ledger
        .structure_facts()
        .iter()
        .filter(|fact| fact.kind == StructureFactKind::Liveness)
        .collect();
    assert_eq!(facts.len(), 4);
    assert!(facts.iter().all(|fact| {
        fact.kind == StructureFactKind::Liveness
            && fact.gate.as_deref() == Some("_name")
            && fact.source == example()
    }));
    assert!(facts.iter().any(|fact| fact.subject == "support"));
    assert!(facts.iter().any(|fact| fact.subject == "unused_binding"));
    assert!(facts.iter().any(|fact| fact.subject == "unused_private"));
    assert!(facts.iter().any(|fact| fact.subject == "package_export"));
    assert!(!facts.iter().any(|fact| fact.subject == "library_pub"));
    for diagnostic in &diagnostics {
        let span = diagnostic.span.expect("liveness diagnostic span");
        let edit = diagnostic.edit.as_ref().expect("liveness source edit");
        if edit.new_text.is_empty() {
            assert!(matches!(diagnostic.code.as_str(), "L0101" | "L0104"));
            assert!(edit.span.start <= span.start && edit.span.end >= span.end);
        } else {
            assert_eq!(edit.span, span);
            assert_eq!(edit.new_text, format!("_{}", &source[span.start..span.end]));
        }
    }

    jet::Codegen::TIR::lower_jit_program(&bundle).expect("liveness example lowers to TIR");

    let text = run_jet(&["inspect", "structure", example()]);
    assert!(text.status.success(), "structure inspect failed: {text:?}");
    let text = String::from_utf8(text.stdout).expect("structure text is utf8");
    let expected_text = fs::read_to_string(
        repo().join("examples/features/expected/tooling/structure_liveness.structure.out"),
    )
    .expect("structure text golden");
    assert_eq!(text, expected_text);
    for subject in [
        "support",
        "unused_binding",
        "unused_private",
        "package_export",
    ] {
        assert!(text.contains(subject), "missing {subject} in {text}");
    }
    assert!(text.contains("Structure.Liveness"));
    assert!(text.contains("gates=_name"));

    let json = run_jet(&["inspect", "structure", "--json", example()]);
    assert!(
        json.status.success(),
        "JSON structure inspect failed: {json:?}"
    );
    let json = String::from_utf8(json.stdout).expect("structure JSON is utf8");
    let expected_json = fs::read_to_string(
        repo().join("examples/features/expected/tooling/structure_liveness.structure.json"),
    )
    .expect("structure JSON golden");
    assert_eq!(json, expected_json);
    assert!(json.contains("\"registry\":\"Structure.Liveness\""));
    assert!(json.contains("\"gate\":\"_name\""));

    let cache = std::env::temp_dir().join(format!(
        "jet_structure_liveness_tiers_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&cache);
    for (mode, args) in [
        ("release", vec!["run", "--release", example()]),
        ("default", vec!["run", example()]),
        ("interpret", vec!["run", "--interpret", example()]),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .current_dir(repo())
            .env("NO_COLOR", "1")
            .env("JET_RUN_CACHE_DIR", cache.join(mode).join("run"))
            .env("JET_CACHE_DIR", cache.join(mode).join("build"))
            .args(args)
            .output()
            .expect("run liveness example tier");
        assert!(
            output.status.success(),
            "{mode} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"structure liveness\n", "{mode} output");
        assert_liveness_warnings(mode, &output);
    }

    let dev = run_jet(&["dev", example(), "--watch=off"]);
    assert!(
        dev.status.success(),
        "dev failed: stdout={} stderr={}",
        String::from_utf8_lossy(&dev.stdout),
        String::from_utf8_lossy(&dev.stderr)
    );
    assert_eq!(dev.stdout, b"structure liveness\n", "dev output");
    assert_liveness_warnings("dev", &dev);

    for (mode, args) in [
        (
            "policy-aot",
            vec!["run", "--release", "tests/ui/lint_policy_liveness/run.jet"],
        ),
        (
            "policy-jit",
            vec!["run", "tests/ui/lint_policy_liveness/run.jet"],
        ),
        (
            "policy-interpreter",
            vec![
                "run",
                "--interpret",
                "tests/ui/lint_policy_liveness/run.jet",
            ],
        ),
        (
            "policy-dev",
            vec![
                "dev",
                "tests/ui/lint_policy_liveness/run.jet",
                "--watch=off",
            ],
        ),
    ] {
        let output = run_jet(&args);
        assert_liveness_policy_denied(mode, &output);
    }
    let _ = fs::remove_dir_all(cache);
}

#[test]
fn liveness_web_compile_keeps_the_same_front_end_result() {
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
        eprintln!("note: skipping liveness web tier proof (wasm target unavailable)");
        return;
    }
    let output = run_jet(&["build", "--target=web", example()]);
    assert!(
        output.status.success(),
        "web build failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_liveness_warnings("web", &output);
}
