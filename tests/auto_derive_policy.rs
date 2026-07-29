use jet::JitBackend::JitBackend;

fn checked_project(
    name: &str,
    manifest_policy: &str,
    source: &str,
) -> jet::AST::ProgramBundle {
    let dir = std::env::temp_dir().join(format!(
        "jet_auto_derive_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("pkg.jet"),
        format!(
            "payload: {{ name: \"{name}\", version: \"1.0.0\" }}\n{manifest_policy}\n"
        ),
    )
    .unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(&entry, source).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.severity,
                jet::Diagnostics::Severity::Error
            )
        })
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    bundle
}

#[test]
fn signed_markers_gate_generated_traits_and_jit_behavior() {
    let bundle = checked_project(
        "signed",
        "",
        r#"
#[!Debug, !Equatable, Printable]
struct Mixed { value: Int }

fn run() {
    print(Mixed.{ value: 7 })
}
"#,
    );
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(rust.contains("impl JetShow for user_Mixed"));
    assert!(!rust.contains("impl JetDebug for user_Mixed"));
    assert!(!rust.contains("impl user_Equatable for user_Mixed"));

    let mut backend = jet_jit::CraneliftBackend::new();
    let outcome = backend.run(&bundle, false);
    let jet::Interpreter::RunOutcome::Ran { stdout, .. } = outcome else {
        panic!("signed auto-derive program did not run: {outcome:?}");
    };
    assert_eq!(stdout, "Mixed { value: 7 }\n");
}

#[test]
fn package_off_requires_opt_in_and_manual_impl_wins() {
    let bundle = checked_project(
        "package_off",
        "policy: .{ auto_derive: false }",
        r#"
#[Printable, Equatable, Debug]
struct Enabled { value: Int }

struct Missing { value: Int }

#!Debug
struct Manual { value: Int }

impl Manual.Debug {
    fn debug(self) => String { return "manual" }
}

fn run() {
    a := Enabled.{ value: 3 }
    print(a)
    print("{a#Debug}")
    print(a == Enabled.{ value: 3 })
    print("{Manual.{ value: 9 }#Debug}")
}
"#,
    );
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    for implementation in [
        "impl JetShow for user_Enabled",
        "impl JetDebug for user_Enabled",
        "impl user_Equatable for user_Enabled",
    ] {
        assert!(rust.contains(implementation), "{implementation}");
    }
    for implementation in [
        "impl JetShow for user_Missing",
        "impl JetDebug for user_Missing",
        "impl user_Equatable for user_Missing",
    ] {
        assert!(!rust.contains(implementation), "{implementation}");
    }
    assert_eq!(rust.matches("impl JetDebug for user_Manual").count(), 1);

    let mut backend = jet_jit::CraneliftBackend::new();
    let outcome = backend.run(&bundle, false);
    let jet::Interpreter::RunOutcome::Ran { stdout, .. } = outcome else {
        panic!("package-off auto-derive program did not run: {outcome:?}");
    };
    assert_eq!(
        stdout,
        "Enabled { value: 3 }\nEnabled { value: 3 }\ntrue\nmanual\n"
    );
}
