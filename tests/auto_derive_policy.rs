use jet::JitBackend::JitBackend;
use std::process::Command;

mod common;

fn project_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jet_auto_derive_{name}_{}",
        std::process::id()
    ))
}

fn loaded_project(name: &str, manifest_policy: &str, source: &str) -> jet::AST::ProgramBundle {
    let dir = project_dir(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("package.jet"),
        format!(
            "name: \"{name}\"\nversion: \"1.0.0\"\n{manifest_policy}\n"
        ),
    )
    .unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(&entry, source).unwrap();
    jet::Loader::load_entry(entry.to_str().unwrap()).unwrap()
}

fn checked_bundle(mut bundle: jet::AST::ProgramBundle) -> jet::AST::ProgramBundle {
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

fn checked_project(
    name: &str,
    manifest_policy: &str,
    source: &str,
) -> jet::AST::ProgramBundle {
    checked_bundle(loaded_project(name, manifest_policy, source))
}

fn project_diagnostics(name: &str, manifest_policy: &str, source: &str) -> Vec<String> {
    let dir = project_dir(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("package.jet"),
        format!(
            "name: \"{name}\"\nversion: \"1.0.0\"\n{manifest_policy}\n"
        ),
    )
    .unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(&entry, source).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .map(|diagnostic| diagnostic.code)
        .collect()
}

fn collect_struct_defaults(items: &[jet::AST::Item], out: &mut Vec<(String, bool)>) {
    for item in items {
        match item {
            jet::AST::Item::Struct(def) => {
                out.push((def.name.clone(), def.auto_derive_default));
            }
            jet::AST::Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    collect_struct_defaults(body, out);
                }
            }
            jet::AST::Item::GenericModule(module) => {
                collect_struct_defaults(&module.body, out);
            }
            _ => {}
        }
    }
}

struct AotMeasurement {
    exit: i32,
    stdout: String,
    generated_rust_bytes: usize,
    binary_bytes: u64,
}

fn aot_measurement(bundle: &jet::AST::ProgramBundle, name: &str) -> Option<AotMeasurement> {
    if !common::have_rustc() {
        return None;
    }
    let dir = project_dir(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rust = dir.join("main.rs");
    let binary = dir.join("main_bin");
    let generated = jet::Codegen::emit_bundle(bundle, jet::Sema::CompileMode::Run, None);
    std::fs::write(
        &rust,
        &generated,
    )
    .unwrap();
    let built = Command::new("rustc")
        .args(["--edition", "2021", "-O"])
        .arg(&rust)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "AOT generated Rust failed:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(binary).output().unwrap();
    Some(AotMeasurement {
        exit: ran.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&ran.stdout).into_owned(),
        generated_rust_bytes: generated.len(),
        binary_bytes: std::fs::metadata(dir.join("main_bin")).unwrap().len(),
    })
}

fn aot_output(bundle: &jet::AST::ProgramBundle, name: &str) -> Option<(i32, String)> {
    aot_measurement(bundle, name).map(|measurement| (measurement.exit, measurement.stdout))
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
    assert!(rust.contains("impl JetShow for __jet_Mixed"));
    assert!(!rust.contains("impl JetDebug for __jet_Mixed"));
    assert!(!rust.contains("impl __jet_Equatable for __jet_Mixed"));

    let mut backend = jet_jit::CraneliftBackend::new();
    let outcome = backend.run(&bundle, false);
    let jet::Interpreter::RunOutcome::Ran { stdout, .. } = outcome else {
        panic!("signed auto-derive program did not run: {outcome:?}");
    };
    assert_eq!(stdout, "Mixed { value: 7 }\n");
}

#[test]
fn signed_codable_refuses_both_codec_directions() {
    let bundle = checked_project(
        "signed_codable",
        "",
        r#"
#!Codable
struct NoCodec { value: Int }

fn run() {}
"#,
    );
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let facts = &facts[bundle.entry];
    assert!(!facts.auto_encode.contains("NoCodec"));
    assert!(!facts.auto_decode.contains("NoCodec"));
}

#[test]
fn ineligible_struct_does_not_expand_codec_body() {
    let bundle = checked_project(
        "ineligible_codec",
        "",
        r#"
struct Record { id: U64; flags: U32 }

fn run() {}
"#,
    );
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let facts = &facts[bundle.entry];
    assert!(!facts.auto_encode.contains("Record"));
    assert!(!facts.auto_decode.contains("Record"));

    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(!rust.contains("impl __jet_Encode for __jet_Record"));
    assert!(!rust.contains("impl __jet_Decode for __jet_Record"));
}

#[test]
fn plain_struct_auto_codable_matches_all_execution_tiers() {
    let bundle = checked_project(
        "plain_codable",
        "",
        r#"
use core.encoding.json as json

struct Record { id: Int; name: String }

fn run() {
    print(json.to_string(Record.{ id: 7, name: "plain" }))
}
"#,
    );
    let expected = "{\"id\":7,\"name\":\"plain\"}\n";
    if let Some((exit, stdout)) = aot_output(&bundle, "plain_codable_aot") {
        assert_eq!(exit, 0);
        assert_eq!(stdout, expected);
    }

    let mut backend = jet_jit::CraneliftBackend::new();
    let outcome = backend.run(&bundle, false);
    let jet::Interpreter::RunOutcome::Ran { stdout, .. } = outcome else {
        panic!("plain Codable program did not run in the default JIT: {outcome:?}");
    };
    assert_eq!(stdout, expected);

    let outcome = jet::Interpreter::dev_iteration(
        project_dir("plain_codable").join("main.jet").to_str().unwrap(),
        false,
        true,
    );
    let jet::Interpreter::RunOutcome::Ran { stdout, .. } = outcome else {
        panic!("plain Codable program did not run in the forced interpreter: {outcome:?}");
    };
    assert_eq!(stdout, expected);
}

#[test]
fn auto_derive_keeps_compile_and_binary_measurements_stable() {
    let explicit_bundle = checked_project(
        "auto_measure_before",
        "",
        r#"
use core.encoding.json as json

#[Encode, Decode]
struct Record { id: Int; name: String }

fn run() {
    print(json.to_string(Record.{ id: 7, name: "plain" }))
}
"#,
    );
    let automatic_bundle = checked_project(
        "auto_measure_after",
        "",
        r#"
use core.encoding.json as json

struct Record { id: Int; name: String }

fn run() {
    print(json.to_string(Record.{ id: 7, name: "plain" }))
}
"#,
    );
    let Some(explicit) = aot_measurement(&explicit_bundle, "auto_measure_before_aot") else {
        return;
    };
    let Some(explicit_repeat) = aot_measurement(&explicit_bundle, "auto_measure_before_aot") else {
        return;
    };
    let Some(automatic) = aot_measurement(&automatic_bundle, "auto_measure_after_aot") else {
        return;
    };
    let Some(automatic_repeat) = aot_measurement(&automatic_bundle, "auto_measure_after_aot") else {
        return;
    };
    for (label, first, repeat) in [
        ("explicit", &explicit, &explicit_repeat),
        ("automatic", &automatic, &automatic_repeat),
    ] {
        assert_eq!(first.exit, repeat.exit, "{label} exit changed between runs");
        assert_eq!(first.stdout, repeat.stdout, "{label} stdout changed between runs");
        assert_eq!(
            first.generated_rust_bytes,
            repeat.generated_rust_bytes,
            "{label} generated-Rust bytes changed between runs",
        );
        assert_eq!(
            first.binary_bytes,
            repeat.binary_bytes,
            "{label} binary bytes changed between runs",
        );
    }
    assert_eq!(explicit.exit, 0);
    assert_eq!(automatic.exit, 0);
    assert_eq!(explicit.stdout, automatic.stdout);
    let generated_rust_delta =
        explicit.generated_rust_bytes.abs_diff(automatic.generated_rust_bytes);
    let binary_delta = explicit.binary_bytes.abs_diff(automatic.binary_bytes);
    eprintln!(
        "auto-derive measurement: generated Rust {} -> {} ({generated_rust_delta} bytes); binary {} -> {} ({binary_delta} bytes)",
        explicit.generated_rust_bytes,
        automatic.generated_rust_bytes,
        explicit.binary_bytes,
        automatic.binary_bytes,
    );
    assert!(
        generated_rust_delta <= 64,
        "auto derive added {generated_rust_delta} generated-Rust bytes"
    );
    assert!(
        binary_delta <= 64,
        "auto derive changed the binary by {binary_delta} bytes"
    );
}

#[test]
fn structural_defaults_cover_codable_and_refusals_are_silent() {
    let bundle = loaded_project(
        "structural_defaults",
        "",
        r#"
struct Plain { value: Int }

#!Codable
struct Refused { value: Int }

struct Opaque { value: Shared<Int> }

fn run() {}
"#,
    );
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let facts = &facts[bundle.entry];
    for selected in [
        &facts.auto_printable,
        &facts.auto_debug,
        &facts.auto_equatable,
        &facts.auto_comparable,
        &facts.auto_encode,
        &facts.auto_decode,
    ] {
        assert!(selected.contains("Plain"), "{selected:?}");
        assert!(!selected.contains("Opaque"), "{selected:?}");
    }
    for selected in [
        &facts.auto_printable,
        &facts.auto_debug,
        &facts.auto_equatable,
        &facts.auto_comparable,
    ] {
        assert!(selected.contains("Refused"), "{selected:?}");
    }
    assert!(!facts.auto_encode.contains("Refused"));
    assert!(!facts.auto_decode.contains("Refused"));
    let _ = checked_bundle(bundle);
}

#[test]
fn recursive_container_dependencies_derive_without_approving_blocked_nominals() {
    let bundle = loaded_project(
        "recursive_containers",
        "",
        r#"
struct OptionalNode { next: OptionalNode? }
struct ListNode { children: [ListNode] }
struct MapNode { children: [String:MapNode] }

#!Printable
struct NoPrint { value: Int }
struct Blocked { child: NoPrint? }

fn run() {}
"#,
    );
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let facts = &facts[bundle.entry];
    for name in ["OptionalNode", "ListNode", "MapNode"] {
        assert!(facts.auto_printable.contains(name), "{name}");
        assert!(facts.auto_debug.contains(name), "{name}");
        assert!(facts.auto_encode.contains(name), "{name}");
        assert!(facts.auto_decode.contains(name), "{name}");
    }
    for name in ["OptionalNode", "ListNode"] {
        assert!(facts.auto_equatable.contains(name), "{name}");
    }
    assert!(!facts.auto_equatable.contains("MapNode"));
    assert!(!facts.auto_printable.contains("Blocked"));
    let _ = checked_bundle(bundle);
}

#[test]
fn project_refusal_is_a_default_fact_and_explicit_codable_still_wins() {
    let bundle = loaded_project(
        "project_refusal_fact",
        "policy: .{ lints: .{ deny: [auto_derive] } }",
        r#"
struct Defaulted { value: Int }

#Codable
struct Explicit { value: Int }

fn run() {}
"#,
    );
    let mut defaults = Vec::new();
    collect_struct_defaults(
        &bundle.modules[bundle.entry].items,
        &mut defaults,
    );
    assert_eq!(
        defaults,
        vec![("Defaulted".to_string(), false), ("Explicit".to_string(), false)]
    );
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let facts = &facts[bundle.entry];
    assert!(!facts.auto_encode.contains("Defaulted"));
    assert!(!facts.auto_decode.contains("Defaulted"));
    assert!(facts.auto_encode.contains("Explicit"));
    assert!(facts.auto_decode.contains("Explicit"));
    let bundle = checked_bundle(bundle);
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(!rust.contains("impl __jet_Encode for __jet_Defaulted"));
    assert!(!rust.contains("impl __jet_Decode for __jet_Defaulted"));
    assert!(rust.contains("impl __jet_Encode for __jet_Explicit"));
    assert!(rust.contains("impl __jet_Decode for __jet_Explicit"));
}

#[test]
fn generic_deny_list_refuses_auto_derive() {
    let bundle = checked_project(
        "package_off",
        "policy: .{ lints: .{ deny: [auto_derive] } }",
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
    print("{a:Debug}")
    print(a == Enabled.{ value: 3 })
    print("{Manual.{ value: 9 }:Debug}")
}
"#,
    );
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    for implementation in [
        "impl JetShow for __jet_Enabled",
        "impl JetDebug for __jet_Enabled",
        "impl __jet_Equatable for __jet_Enabled",
    ] {
        assert!(rust.contains(implementation), "{implementation}");
    }
    for implementation in [
        "impl JetShow for __jet_Missing",
        "impl JetDebug for __jet_Missing",
        "impl __jet_Equatable for __jet_Missing",
    ] {
        assert!(!rust.contains(implementation), "{implementation}");
    }
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let facts = &facts[bundle.entry];
    assert!(!facts.auto_encode.contains("Missing"));
    assert!(!facts.auto_decode.contains("Missing"));
    assert_eq!(rust.matches("impl JetDebug for __jet_Manual").count(), 1);

    let mut backend = jet_jit::CraneliftBackend::new();
    let outcome = backend.run(&bundle, false);
    let jet::Interpreter::RunOutcome::Ran { stdout, .. } = outcome else {
        panic!("package-off auto-derive program did not run: {outcome:?}");
    };
    assert_eq!(
        stdout,
        "Enabled { value: 3 }\nEnabled { value: 3 }\ntrue\nmanual\n"
    );

    let outcome = jet::Interpreter::dev_iteration(
        project_dir("package_off")
            .join("main.jet")
            .to_str()
            .unwrap(),
        false,
        true,
    );
    let jet::Interpreter::RunOutcome::Ran { stdout, .. } = outcome else {
        panic!("package-off auto-derive program did not interpret: {outcome:?}");
    };
    assert_eq!(
        stdout,
        "Enabled { value: 3 }\nEnabled { value: 3 }\ntrue\nmanual\n"
    );
}

#[test]
fn old_auto_derive_key_is_rejected() {
    let dir = project_dir("old_key");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let legacy_key = ["auto", "derive"].join("_");
    std::fs::write(
        dir.join("package.jet"),
        format!(
            "name: \"old-key\"\nversion: \"1\"\npolicy: .{{ {legacy_key}: false }}\n"
        ),
    )
    .unwrap();
    let entry = dir.join("main.jet");
    std::fs::write(&entry, "fn run() {}\n").unwrap();

    let diagnostics = jet::Loader::load_entry(entry.to_str().unwrap())
        .expect_err("retired policy key must be rejected");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
    assert_eq!(diagnostics[0].code, "E1206");
    assert!(diagnostics[0].what.contains("policy.auto_derive"), "{diagnostics:#?}");
    assert!(diagnostics[0].fix.contains("policy.lints.deny"), "{diagnostics:#?}");
}

#[test]
fn rejected_auto_traits_fail_in_sema_before_codegen() {
    let diagnostics = project_diagnostics(
        "reject_use",
        "",
        r#"
#!Printable
struct NoPrint { value: Int }
struct OuterNoPrint { inner: NoPrint }

#!Debug
struct NoDebug { value: Int }
struct OuterNoDebug { inner: NoDebug }

#!Equatable
struct NoEquality { value: Int }
struct OuterNoEquality { inner: NoEquality }

struct OuterReader { reader: FileReader }

#[!Printable, !Debug, !Equatable]
struct Hidden<T> { value: T }
struct OuterHidden { value: Hidden<Int> }

fn reject_opaque_core(reader: FileReader) {
    print(reader)
    print("{reader:Debug}")
}

fn reject_outer_reader(value: OuterReader) {
    print(value)
}

fn run() {
    print(OuterNoPrint.{ inner: NoPrint.{ value: 1 } })
    print("{OuterNoDebug.{ inner: NoDebug.{ value: 2 } }:Debug}")
    print(OuterNoEquality.{ inner: NoEquality.{ value: 3 } } == OuterNoEquality.{ inner: NoEquality.{ value: 3 } })
    print(OuterHidden.{ value: Hidden.{ value: 4 } })
}
"#,
    );
    assert!(
        diagnostics
            .iter()
            .filter(|code| code.as_str() == "E0112")
            .count()
            >= 6,
        "{diagnostics:?}"
    );
    assert!(diagnostics.iter().any(|code| code == "E0312"), "{diagnostics:?}");

    let entry = project_dir("reject_use").join("main.jet");
    let bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let facts = &facts[bundle.entry];
    for (type_name, selected) in [
        ("OuterReader", &facts.auto_printable),
        ("OuterHidden", &facts.auto_printable),
        ("OuterHidden", &facts.auto_debug),
        ("OuterHidden", &facts.auto_equatable),
    ] {
        assert!(!selected.contains(type_name), "{type_name}");
    }
    for type_name in ["NoEquality", "OuterNoEquality"] {
        assert!(!facts.auto_equatable.contains(type_name), "{type_name}");
        assert!(facts.auto_comparable.contains(type_name), "{type_name}");
    }
    for (tier, force_interpreter) in [("JIT", false), ("interpreter", true)] {
        let outcome =
            jet::Interpreter::dev_iteration(entry.to_str().unwrap(), false, force_interpreter);
        let jet::Interpreter::RunOutcome::Problems(diagnostics) = outcome else {
            panic!("{tier} bypassed sema: {outcome:?}");
        };
        assert!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "E0112")
                .count()
                >= 6,
            "{tier}: {diagnostics:#?}"
        );
    }
}

#[test]
fn package_default_reaches_nested_and_dependency_modules() {
    let nested_dir = project_dir("nested");
    let _ = std::fs::remove_dir_all(&nested_dir);
    std::fs::create_dir_all(&nested_dir).unwrap();
    std::fs::write(
        nested_dir.join("package.jet"),
        "name: \"nested\"\nversion: \"1\"\npolicy: .{ lints: .{ deny: [auto_derive] } }\n",
    )
    .unwrap();
    std::fs::write(
        nested_dir.join("main.jet"),
        "struct Outer { value: Int }\nmodule inner { struct Inner { value: Int } }\nfn run() {}\n",
    )
    .unwrap();
    let nested = jet::Loader::load_entry(nested_dir.join("main.jet").to_str().unwrap()).unwrap();
    let mut defaults = Vec::new();
    for module in &nested.modules {
        collect_struct_defaults(&module.items, &mut defaults);
    }
    assert_eq!(
        defaults,
        vec![
            ("Outer".to_string(), false),
            ("Inner".to_string(), false)
        ]
    );

    let workspace = project_dir("multi_package");
    let app = workspace.join("app");
    let dep = workspace.join("dep");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&dep).unwrap();
    std::fs::write(
        app.join("package.jet"),
        "name: \"app\"\nversion: \"1\"\ndeps: { dep: ../dep }\n",
    )
    .unwrap();
    std::fs::write(
        app.join("main.jet"),
        "use dep;\nstruct AppType { value: Int }\nstruct ImportedOuter { value: dep.DepType }\nfn reject(value: ImportedOuter) { print(value) }\nfn run() {}\n",
    )
    .unwrap();
    std::fs::write(
        dep.join("package.jet"),
        "name: \"dep\"\nversion: \"1\"\npolicy: .{ lints: .{ deny: [auto_derive] } }\n",
    )
    .unwrap();
    std::fs::write(
        dep.join("dep.jet"),
        "pub struct DepType { value: Int }\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(app.join("main.jet").to_str().unwrap()).unwrap();
    let mut by_module = std::collections::HashMap::new();
    for module in &bundle.modules {
        let mut module_defaults = Vec::new();
        collect_struct_defaults(&module.items, &mut module_defaults);
        by_module.insert(module.path.clone(), module_defaults);
    }
    assert_eq!(
        by_module[&app.join("main.jet")],
        vec![
            ("AppType".to_string(), true),
            ("ImportedOuter".to_string(), true),
        ]
    );
    assert_eq!(
        by_module[&dep.join("dep.jet")],
        vec![("DepType".to_string(), false)]
    );
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let facts = &facts[bundle.entry];
    assert!(!facts.auto_printable.contains("ImportedOuter"));
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(
        errors.iter().any(|diagnostic| diagnostic.code == "E0112"),
        "{errors:#?}"
    );
}

#[test]
fn same_named_dependency_type_keeps_its_own_auto_derive_policy() {
    let workspace = project_dir("same_named_types");
    let app = workspace.join("app");
    let dep = workspace.join("dep");
    let open_dep = workspace.join("open_dep");
    let _ = std::fs::remove_dir_all(&workspace);
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&dep).unwrap();
    std::fs::create_dir_all(&open_dep).unwrap();
    std::fs::write(
        app.join("package.jet"),
        "name: \"app\"\n\
         version: \"1\"\n\
         deps: {\n\
             dep: ../dep,\n\
             open_dep: ../open_dep,\n\
         }\n",
    )
    .unwrap();
    std::fs::write(
        app.join("main.jet"),
        r#"
use dep as vendor

struct Token { value: Int }
struct LocalEnvelope { token: Token }
struct DependencyEnvelope { token: vendor.Token }
struct SharedEnvelope { value: Shared<Int> }

fn reject(value: vendor.Token) {
    print(value)
}

fn run() {
    print(LocalEnvelope.{ token: Token.{ value: 7 } })
}
"#,
    )
    .unwrap();
    std::fs::write(
        dep.join("package.jet"),
        "name: \"dep\"\nversion: \"1\"\npolicy: .{ lints: .{ deny: [auto_derive] } }\n",
    )
    .unwrap();
    std::fs::write(
        dep.join("dep.jet"),
        "pub struct Token { value: Int }\n",
    )
    .unwrap();
    std::fs::write(
        open_dep.join("package.jet"),
        "name: \"open_dep\"\nversion: \"1\"\n",
    )
    .unwrap();
    std::fs::write(
        open_dep.join("open_dep.jet"),
        "pub struct Badge { pub value: Int }\n",
    )
    .unwrap();

    let mut bundle =
        jet::Loader::load_entry(app.join("main.jet").to_str().unwrap()).unwrap();
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let app_facts = &facts[bundle.entry];
    for selected in [
        &app_facts.auto_printable,
        &app_facts.auto_debug,
        &app_facts.auto_equatable,
    ] {
        assert!(!selected.contains("SharedEnvelope"), "{selected:?}");
    }
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert_eq!(
        errors
            .iter()
            .filter(|diagnostic| diagnostic.code == "E0112")
            .count(),
        1,
        "{errors:#?}"
    );

    std::fs::write(
        app.join("main.jet"),
        r#"
use dep as vendor
use open_dep as library

struct Token { value: Int }
struct LocalEnvelope { token: Token }
struct DependencyEnvelope { token: vendor.Token }
struct MapEnvelope { values: [String:Int] }
struct UnionEnvelope { value: Int | String }

fn run() {
    token :: Token.{ value: 7 }
    print(token)
    print("{token:Debug}")
    print(token == Token.{ value: 7 })

    badge :: library.Badge.{ value: 9 }
    print(badge)
    print("{badge:Debug}")
    print(badge == library.Badge.{ value: 9 })

    map :: MapEnvelope.{ values: [String:Int].{ "one": 1 } }
    print(map)
    print("{map:Debug}")

    union :: UnionEnvelope.{ value: 3 }
    print(union)
    print("{union:Debug}")
    print(union == UnionEnvelope.{ value: 3 })
}
"#,
    )
    .unwrap();
    let mut bundle =
        jet::Loader::load_entry(app.join("main.jet").to_str().unwrap()).unwrap();
    let facts = jet::Traits::TraitRegistry::bundle_auto_derives(&bundle, &bundle.name_ledger);
    let app_facts = &facts[bundle.entry];
    let dep_idx = bundle
        .modules
        .iter()
        .position(|module| module.path == dep.join("dep.jet"))
        .unwrap();
    let open_dep_idx = bundle
        .modules
        .iter()
        .position(|module| module.path == open_dep.join("open_dep.jet"))
        .unwrap();
    let badge_identity = bundle
        .name_ledger
        .nominal_identity(open_dep_idx, "Badge")
        .unwrap();
    assert!(badge_identity.contains("::"), "{badge_identity}");
    let dep_facts = &facts[dep_idx];
    for selected in [
        &app_facts.auto_printable,
        &app_facts.auto_debug,
        &app_facts.auto_equatable,
    ] {
        assert!(selected.contains("Token"), "{selected:?}");
        assert!(selected.contains("LocalEnvelope"), "{selected:?}");
        assert!(selected.contains("UnionEnvelope"), "{selected:?}");
        assert!(!selected.contains("DependencyEnvelope"), "{selected:?}");
        assert!(!selected.contains("vendor.Token"), "{selected:?}");
        assert!(selected.contains(&badge_identity), "{selected:?}");
    }
    // Anonymous unions support structural equality, but there is no total
    // ordering between members of different types.
    let comparable = &app_facts.auto_comparable;
    assert!(comparable.contains("Token"), "{comparable:?}");
    assert!(comparable.contains("LocalEnvelope"), "{comparable:?}");
    assert!(!comparable.contains("UnionEnvelope"), "{comparable:?}");
    assert!(!comparable.contains("DependencyEnvelope"), "{comparable:?}");
    assert!(!comparable.contains("vendor.Token"), "{comparable:?}");
    assert!(comparable.contains(&badge_identity), "{comparable:?}");
    assert!(app_facts.auto_printable.contains("MapEnvelope"));
    assert!(app_facts.auto_debug.contains("MapEnvelope"));
    assert!(!app_facts.auto_equatable.contains("MapEnvelope"));
    assert!(!app_facts.auto_comparable.contains("MapEnvelope"));
    for selected in [
        &dep_facts.auto_printable,
        &dep_facts.auto_debug,
        &dep_facts.auto_equatable,
        &dep_facts.auto_comparable,
    ] {
        assert!(!selected.contains("Token"), "{selected:?}");
    }
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");


    let expected = "\
Token { value: 7 }\n\
Token { value: 7 }\n\
true\n\
Badge { value: 9 }\n\
Badge { value: 9 }\n\
true\n\
MapEnvelope { values: [:\"one\": 1] }\n\
MapEnvelope { values: [:\"one\": 1] }\n\
UnionEnvelope { value: 3 }\n\
UnionEnvelope { value: 3 }\n\
true\n";
    if let Some((exit, stdout)) = aot_output(&bundle, "same_named_types_aot") {
        assert_eq!(exit, 0);
        assert_eq!(stdout, expected);
    }
    let mut backend = jet_jit::CraneliftBackend::new();
    let outcome = backend.run(&bundle, false);
    let jet::Interpreter::RunOutcome::Ran { stdout, .. } = outcome else {
        panic!("same-name program did not run in the default JIT: {outcome:?}");
    };
    assert_eq!(stdout, expected);
    let outcome = jet::Interpreter::dev_iteration(
        app.join("main.jet").to_str().unwrap(),
        false,
        true,
    );
    let jet::Interpreter::RunOutcome::Ran { stdout, .. } = outcome else {
        panic!("same-name program did not run in the forced interpreter: {outcome:?}");
    };
    assert_eq!(stdout, expected);
}
