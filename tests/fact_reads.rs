//! D-FACT-READ1: one typed, compile-time reader for every registered plane.

mod common;

use jet_foundation::Facts::BuildStamp;
use std::fs;

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
    assert!(
        !output.rust.contains("fact_read") && !output.rust.contains("jet.fact"),
        "a folded fact must not emit a runtime reader or dispatch path"
    );
}

#[test]
fn derive_bodies_read_the_same_typed_fact() {
    let output = jet::compile(
        "derive T.Debug {\n    states :: T.@states\n    emit(\"fn derived_fact_read() => String {{ return \\\"ok\\\" }}\")\n}\n\nstate Report { Draft, Published }\n\n#Debug\nstruct Report {\n    value: Int\n}\n\nfn run() {}\n",
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
        !output.rust.contains("@build") && !output.rust.contains("fact_read"),
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
