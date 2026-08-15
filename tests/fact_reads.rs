//! D-FACT-READ1: one typed, compile-time reader for every registered plane.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use jet_foundation::Facts::BuildStamp;
use jet::AST::Item;
use std::fs;
use std::process::Command;

const FIXTURE: &str = include_str!("../examples/features/reflection/fact_reads.jet");
const FIXTURE_EXPECTED: &str = include_str!("../examples/features/expected/reflection/fact_reads.out");
const AGGREGATE_FIXTURE: &str =
    include_str!("../examples/features/reflection/reflect-value.jet");
const AGGREGATE_EXPECTED: &str =
    include_str!("../examples/features/expected/reflection/reflect-value.out");

fn diagnostics(source: &str) -> Vec<jet::Diagnostics::Diagnostic> {
    jet::compile(source).expect_err("the fixture must be rejected")
}

fn stdout(outcome: jet::Interpreter::RunOutcome, tier: &str) -> String {
    match outcome {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            assert!(stderr.is_empty(), "{tier} stderr: {stderr}");
            assert_eq!(exit_code, 0, "{tier} exit code");
            stdout
        }
        other => panic!("{tier} did not run: {other:?}"),
    }
}

fn has_runtime_fact_dispatch(rust: &str) -> bool {
    rust.contains("fact_read(") || rust.contains("jet.fact")
}

fn web_stdout(name: &str, source: &str, source_path: &str) -> Option<String> {
    if Command::new("node").arg("--version").output().is_err()
        || Command::new("rustc").arg("--version").output().is_err()
    {
        return None;
    }
    let scratch = common::Scratch::new(name);
    let output = jet::compile_web_with_path(source, source_path)
        .unwrap_or_else(|diags| panic!("web fact fixture was rejected: {diags:#?}"));
    let web = output.web.expect("web fact fixture must produce web artifacts");
    fs::write(scratch.join("app.js"), web.js_app).unwrap();
    fs::write(scratch.join("app_wasm.rs"), web.wasm_rust).unwrap();
    fs::write(scratch.join("jet_dom_runtime.js"), web.dom_runtime).unwrap();
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
        .unwrap();
    assert!(
        wasm.status.success(),
        "rustc rejected web fact fixture: {}",
        String::from_utf8_lossy(&wasm.stderr)
    );
    assert!(scratch.join("app.wasm").is_file(), "web fact fixture did not produce app.wasm");
    let node = Command::new("node")
        .current_dir(&scratch.path)
        .arg("app.js")
        .output()
        .unwrap();
    assert!(
        node.status.success(),
        "web fact fixture failed: stdout={} stderr={}",
        String::from_utf8_lossy(&node.stdout),
        String::from_utf8_lossy(&node.stderr)
    );
    Some(String::from_utf8_lossy(&node.stdout).into_owned())
}

#[test]
fn runtime_fact_reads_are_refused_before_codegen() {
    let diags = diagnostics(
        "#Numeric Severity :: distinct Int(0..10)\n\nfn run() {\n    print(Severity.@range.start)\n}\n",
    );
    let diagnostic = diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E0302")
        .expect("runtime fact read should have a registered diagnostic");
    assert!(diagnostic.what.contains("compile-time only"), "{diagnostic:?}");
    assert!(diagnostic.why.contains("never selects runtime behavior"), "{diagnostic:?}");
    assert!(diagnostic.fix.contains("binding"), "{diagnostic:?}");
}

#[test]
fn fact_reads_do_not_enter_type_position() {
    let diags = diagnostics(
        "#Numeric Severity :: distinct Int(0..10)\n\nfn takes(value: Severity.@range) {}\nfn run() {}\n",
    );
    let diagnostic = diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E0119")
        .expect("a fact in type position should have a registered diagnostic");
    assert!(diagnostic.what.contains("fact value"), "{diagnostic:?}");
    assert!(diagnostic.why.contains("do not mint or select types"), "{diagnostic:?}");
}

#[test]
fn folded_fact_reads_emit_values_without_runtime_dispatch() {
    let output = jet::compile(
        "#Numeric Severity :: distinct Int(0..10)\n\n@range :: Severity.@range\n\nfn run() {\n    print(@range.start)\n}\n",
    )
    .expect("a comptime fact read should compile");
    assert!(output.rust.contains("JetRange"), "the typed fact carrier is missing");
    assert!(!has_runtime_fact_dispatch(&output.rust), "a folded fact must not emit a runtime reader or dispatch path");
}

#[test]
fn every_typed_fact_member_uses_the_one_registry_reader() {
    for (member, _) in jet::Syntax::FACT_READS {
        assert!(
            jet_foundation::Registry::fact_read(member).is_some(),
            "unregistered fact member: {member}"
        );
    }
}

#[test]
fn parser_preserves_the_typed_fact_fixture() {
    let (tokens, lexer_diagnostics) = jet::Lexer::lex(FIXTURE);
    assert!(lexer_diagnostics.is_empty(), "lex: {lexer_diagnostics:?}");
    let program = jet::Parser::parse(&tokens).expect("typed fact fixture must parse");
    assert!(program.items.iter().any(|item| matches!(item, Item::Const(binding) if binding.is_comptime)));
}

#[test]
fn typed_fact_fixture_folds_all_planes_without_runtime_dispatch() {
    let output = jet::compile(FIXTURE).expect("typed fact fixture must compile");
    for value in ["report"] {
        assert!(output.rust.contains(value), "folded fact value missing: {value}");
    }
    assert!(!has_runtime_fact_dispatch(&output.rust));
}

#[test]
fn aggregate_reflection_reads_typed_facts_without_runtime_dispatch() {
    let output = jet::compile(AGGREGATE_FIXTURE).expect("aggregate fact fixture must compile");
    for value in ["Range"] {
        assert!(output.rust.contains(value), "aggregate fact value missing: {value}");
    }
    assert!(!has_runtime_fact_dispatch(&output.rust));
}

#[test]
fn nested_reflection_fact_reads_fold_across_all_native_tiers() {
    tir_support::assert_example_cli_tiers_agree("reflection/reflect-value", AGGREGATE_EXPECTED);
    if let Some(stdout) = web_stdout(
        "reflection-fact-reads-web",
        AGGREGATE_FIXTURE,
        "examples/features/reflection/reflect-value.jet",
    ) {
        assert_eq!(stdout, AGGREGATE_EXPECTED);
    }
}

#[test]
fn typed_fact_fixture_matches_aot_default_and_interpreter() {
    tir_support::assert_tiers_agree("fact_reads", FIXTURE, FIXTURE_EXPECTED);
}

#[test]
fn typed_fact_fixture_is_accepted_by_comptime_repl_and_web() {
    let transcript = jet::REPL::run_transcript(
        &["@answer :: report.@attribution.source", "print(@answer)"],
        None,
    );
    assert!(transcript.contains("report"), "REPL fact read failed: {transcript}");

    let web = jet::compile_web_with_path(FIXTURE, "examples/features/reflection/fact_reads.jet")
        .expect("web fact fixture must compile")
        .web
        .expect("web fact fixture must produce artifacts");
    assert!(!has_runtime_fact_dispatch(&web.wasm_rust));
    if let Some(stdout) = web_stdout(
        "fact-reads-web",
        FIXTURE,
        "examples/features/reflection/fact_reads.jet",
    ) {
        assert_eq!(stdout, FIXTURE_EXPECTED);
    }
}

#[test]
fn derive_bodies_read_the_same_typed_fact() {
    let output = jet::compile(
        "derive T.Debug {\n    states :: T.@states\n    fn derived_fact_read() => String :: \"ok\"\n}\n\nstate Report { Draft, Published }\n\n#Debug\nstruct Report {\n    value: Int\n}\n\nfn run() {}\n",
    )
    .expect("derive fact read should compile");
    assert!(output.rust.contains("derived_fact_read"));
}

#[test]
fn registered_build_facts_are_folded_in_value_position() {
    let output = jet::compile(
        "fn run() {\n    print(@build.package.name)\n    print(@build.package.version)\n    print(@build.profile)\n}\n",
    )
    .expect("registered build facts are values, not runtime readers");
    assert!(output.rust.contains("input"), "filename identity was not folded");
    assert!(output.rust.contains("0.0.0"), "script version was not folded");
    assert!(output.rust.contains("dev"), "default profile was not folded");
    assert!(
        !output.rust.contains("@build") && !has_runtime_fact_dispatch(&output.rust),
        "a build fact must not reach generated runtime code"
    );
}

#[test]
fn build_fact_reads_do_not_enter_type_position() {
    let diags = diagnostics("fn takes(value: @build.os) {}\nfn run() {}\n");
    let diagnostic = diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E0119")
        .expect("a build fact in type position should have a registered diagnostic");
    assert!(diagnostic.what.contains("fact value"), "{diagnostic:?}");
    assert!(diagnostic.why.contains("do not mint or select types"), "{diagnostic:?}");
}

#[test]
fn package_facts_seed_build_identity() {
    let scratch = common::Scratch::new("build-facts-package");
    fs::write(
        scratch.join("package.jet"),
        "name: \"luna_demo\"\nversion: \"2.3.4\"\n",
    )
    .unwrap();
    let entry = scratch.join("main.jet");
    fs::write(
        &entry,
        "fn run() {\n    print(@build.package.name)\n    print(@build.package.version)\n}\n",
    )
    .unwrap();

    let output = jet::compile_with_path("", entry.to_str().unwrap())
        .expect("package facts should seed the registered build rows");
    assert!(output.rust.contains("luna_demo"));
    assert!(output.rust.contains("2.3.4"));
}

#[test]
fn bare_script_facts_have_filename_and_zero_version() {
    let scratch = common::Scratch::new("build-facts-script");
    let entry = scratch.join("hello.jet");
    fs::write(
        &entry,
        "print(@build.package.name)\nprint(@build.package.version)\n",
    )
    .unwrap();
    let output = jet::compile_with_path("", entry.to_str().unwrap())
        .expect("a bare script should expose its filename identity");
    assert!(output.rust.contains("hello"));
    assert!(output.rust.contains("0.0.0"));
}

#[test]
fn aot_default_and_interpreter_fold_the_same_build_facts() {
    let scratch = common::Scratch::new("build-facts-tiers");
    let entry = scratch.join("main.jet");
    fs::write(
        &entry,
        "fn run() {\n    print(@build.package.name)\n    print(@build.package.version)\n    print(@build.os)\n    print(@build.profile)\n}\n",
    )
    .unwrap();
    let path = entry.to_str().unwrap();
    let aot = jet::compile_with_path("", path).expect("AOT front end should fold build facts");
    assert!(aot.rust.contains("main"));
    assert!(aot.rust.contains("0.0.0"));
    assert!(aot.rust.contains("dev"));

    let jit = stdout(jet::Interpreter::run_jit_once(path), "default");
    let interpreter = stdout(
        jet::Interpreter::run_interpreter_once_with_args(path, &[]),
        "interpreter",
    );
    assert_eq!(jit, interpreter);
    assert_eq!(jit, format!("main\n0.0.0\n{}\ndev\n", jet_foundation::OSTarget::OSTarget::host().name()));
}

#[test]
fn checked_in_lock_stamp_is_a_byte_stable_golden() {
    let scratch = common::Scratch::new("build-stamp-golden");
    let lock_path = scratch.join(".jet/lock");
    fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    let golden = include_str!("fixtures/build_stamp.lock");
    fs::write(&lock_path, golden).unwrap();

    let lock = jet::Lock::parse(golden).expect("checked-in lock golden must parse");
    assert_eq!(jet::Lock::write(&lock), golden);
    let stamp = jet::Lock::build_stamp(&scratch.path, true).expect("locked stamp replay");
    assert_eq!(stamp.at, "2026-08-13T12:34:56.000000000Z");
    assert_eq!(stamp.git.as_deref(), Some("abc123-dirty"));
    assert!(stamp.dirty);
    assert_eq!(stamp.toolchain, "1.0.0");
}

#[test]
fn registered_build_stamp_facts_fold_from_the_lock() {
    let scratch = common::Scratch::new("build-stamp-facts");
    let lock_path = scratch.join(".jet/lock");
    fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
    fs::write(&lock_path, include_str!("fixtures/build_stamp.lock")).unwrap();
    let entry = scratch.join("main.jet");
    fs::write(
        &entry,
        "fn run() {\n    print(@build.stamp.git ?? \"none\")\n    print(@build.stamp.dirty)\n    print(@build.stamp.toolchain)\n    print(@build.stamp.at)\n}\n",
    )
    .unwrap();

    let output = jet::compile_with_path("", entry.to_str().unwrap())
        .expect("registered stamp facts should fold from the lock");
    for value in [
        "abc123-dirty",
        "true",
        "1.0.0",
        "2026-08-13T12:34:56.000000000Z",
    ] {
        assert!(output.rust.contains(value), "folded stamp is missing {value}");
    }
    assert!(!output.rust.contains("@build"));

    let path = entry.to_str().unwrap();
    let expected = "abc123-dirty\ntrue\n1.0.0\n2026-08-13T12:34:56.000000000Z\n";
    let default = stdout(jet::Interpreter::run_jit_once(path), "default");
    let interpreter = stdout(
        jet::Interpreter::run_interpreter_once_with_args(path, &[]),
        "interpreter",
    );
    assert_eq!(default, expected);
    assert_eq!(interpreter, expected);
}

#[test]
fn lock_stamp_is_captured_once_and_replayed_without_a_clock() {
    let scratch = common::Scratch::new("build-stamp-replay");
    let generated = [jet::AST::ComptimeInput {
        path: ".jet/generated/main.jet".to_string(),
        hash: "sha256-fixed".to_string(),
    }];
    let first = BuildStamp {
        git: Some("fixed-revision".to_string()),
        dirty: false,
        toolchain: "1.0.0".to_string(),
        at: "2026-08-13T12:34:56.000000000Z".to_string(),
    };
    jet::Lock::record_generated_inputs(&scratch.path, &generated, false, &first)
        .expect("unlocked generated provenance should write the lock");
    let before = fs::read(scratch.join(".jet/lock")).unwrap();

    let changed = BuildStamp {
        git: Some("different-revision".to_string()),
        dirty: true,
        toolchain: "different-toolchain".to_string(),
        at: "2099-01-01T00:00:00.000000000Z".to_string(),
    };
    jet::Lock::record_generated_inputs(&scratch.path, &generated, false, &changed)
        .expect("replaying generated provenance should remain valid");
    let after = fs::read(scratch.join(".jet/lock")).unwrap();
    assert_eq!(before, after, "repeated builds must preserve lock bytes");
    assert_eq!(jet::Lock::build_stamp(&scratch.path, true).unwrap(), first);

    let hostile = common::Scratch::new("build-stamp-missing");
    let hostile_lock = hostile.join(".jet/lock");
    fs::create_dir_all(hostile_lock.parent().unwrap()).unwrap();
    fs::write(&hostile_lock, "version = 1\n").unwrap();
    let error = jet::Lock::build_stamp(&hostile.path, true).expect_err("missing stamp must fail locked");
    assert!(error.contains("[build.stamp]"), "{error}");
}
