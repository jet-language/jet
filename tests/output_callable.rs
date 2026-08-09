use jet::Interpreter::RunOutcome;
use jet::AST::{Expr, Item, OutputKind, Type};
use jet::JitBackend::JitBackend;
use jet_jit::CraneliftBackend;

mod common;

struct RejectJitFallback;

impl JitBackend for RejectJitFallback {
    fn run(&mut self, _: &jet::AST::ProgramBundle, _: bool) -> RunOutcome {
        panic!("resident JIT unexpectedly used its fallback")
    }

    fn hot_swap(
        &mut self,
        _: &str,
        _: &jet::AST::ProgramBundle,
        _: bool,
    ) -> Result<RunOutcome, Vec<jet::Diagnostics::Diagnostic>> {
        panic!("resident JIT unexpectedly used its fallback")
    }

    fn restart(&mut self, _: &jet::AST::ProgramBundle, _: bool) -> RunOutcome {
        panic!("resident JIT unexpectedly used its fallback")
    }
}

fn checked_bundle(source: &str, tag: &str, mode: jet::Sema::CompileMode) -> jet::AST::ProgramBundle {
    let dir = common::unique_tmp(tag);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(&file, source).unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, mode);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    bundle
}

#[test]
fn parser_retains_typed_output_function_reference() {
    let source = "app: Output :: .Executable.{ name: \"demo\", entry: run }\nfn run() {}\n";
    let (tokens, lex) = jet::Lexer::lex(source);
    assert!(lex.is_empty(), "{lex:#?}");
    let program = jet::Parser::parse(&tokens).expect("Output syntax parses");
    let output = program
        .items
        .iter()
        .find_map(|item| match item {
            Item::Const(value) if value.name == "app" => Some(value),
            _ => None,
        })
        .expect("typed Output item");
    assert!(matches!(output.ty, Some(Type::Named(ref name)) if name == "Output"));
    assert!(matches!(
        output.value,
        Expr::EnumLit { ref variant, .. }
            if OutputKind::from_name(variant) == Some(OutputKind::Executable)
    ));
}

#[test]
fn sema_resolves_sole_executable_entry_before_codegen() {
    let dir = common::unique_tmp("jet_output_callable");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("run.jet");
    std::fs::write(
        &file,
        "app: Output :: .Executable.{ name: \"demo\", entry: start }\nfn start() {}\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let output = bundle.modules[bundle.entry]
        .items
        .iter()
        .find_map(|item| match item {
            Item::Const(value) => value.resolved_output.as_ref(),
            _ => None,
        })
        .expect("sema resolved Output entry");
    assert_eq!(output.kind, OutputKind::Executable);
    assert_eq!(output.source_name, "start");
    assert_eq!(output.module, bundle.entry);
}

#[test]
fn aot_dev_and_jit_consume_the_resolved_entry() {
    let dir = common::unique_tmp("jet_output_callable_parity");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("run.jet");
    std::fs::write(
        &file,
        "app: Output :: .Executable.{ name: \"demo\", entry: start }\nfn start() {}\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");

    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(rust.contains("jet_runtime_boundary(|| user_start())"));
    assert!(matches!(
        jet::Interpreter::run_checked(&bundle, false),
        RunOutcome::Ran { exit_code: 0, .. }
    ));
    let jit = jet::Codegen::TIR::lower_jit_program(&bundle).expect("TIR covers start");
    assert_eq!(jit.entry, "start");
}

#[test]
fn resident_jit_runs_hot_swaps_and_reports_fallible_selected_entry_without_fallback() {
    if !jet_jit::cranelift_host_supported() {
        return;
    }
    let v1 = checked_bundle(
        "app: Output :: .Executable.{ name: \"demo\", entry: start }\nfn start() { print(\"v1\") }\n",
        "jet_output_resident_v1",
        jet::Sema::CompileMode::Run,
    );
    let v2 = checked_bundle(
        "app: Output :: .Executable.{ name: \"demo\", entry: start }\nfn start() => () ? { return Err(\"selected boom\") }\n",
        "jet_output_resident_v2",
        jet::Sema::CompileMode::Run,
    );
    assert!(jet_jit::resident_jit_safe_bundle(&v1));
    assert!(jet_jit::resident_jit_safe_bundle(&v2));

    let mut backend = CraneliftBackend::new();
    let first = backend.run(&v1, false);
    assert!(matches!(first, RunOutcome::Ran { ref stdout, exit_code: 0, .. } if stdout == "v1\n"));
    let swapped = backend.hot_swap("start", &v2, false).expect("resident hot swap");
    assert!(matches!(swapped, RunOutcome::Ran { ref stderr, exit_code: 1, .. } if stderr == "selected boom\n"));
}

#[test]
fn qualified_entry_keeps_one_definition_and_effect_identity() {
    let dir = common::unique_tmp("jet_output_callable_qualified");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("helper.jet"), "pub fn start() { print(\"ok\") }\n").unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        "use \"helper\"\napp: Output :: .Executable.{ name: \"demo\", entry: helper.start };\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let (diagnostics, facts) =
        jet::Sema::check_bundle_with_effect_facts(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let output = bundle.modules[bundle.entry]
        .items
        .iter()
        .find_map(|item| match item {
            Item::Const(value) => value.resolved_output.as_ref(),
            _ => None,
        })
        .unwrap();
    assert_eq!(bundle.modules[output.module].alias, "helper");
    assert_eq!(output.source_name, "start");
    assert!(
        !output.effects.is_empty(),
        "effect row should be copied from sema"
    );
    assert_eq!(output.authority.as_str(), "safe-jet");
    assert!(facts.name_ledger.references().values().any(|anchor| {
        anchor.module_path == output.source_path && anchor.def_span == output.definition
    }));
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(rust.contains("user_helper::user_start()"), "{rust}");
    assert!(matches!(
        jet::Interpreter::run_checked(&bundle, false),
        RunOutcome::Ran { exit_code: 0, .. }
    ));
}

#[test]
fn runnable_contracts_and_selection_fail_in_sema() {
    fn codes(source: &str, mode: jet::Sema::CompileMode) -> Vec<String> {
        let dir = common::unique_tmp("jet_output_callable_bad");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("main.jet");
        std::fs::write(&file, source).unwrap();
        let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
        jet::Sema::check_bundle(&mut bundle, mode)
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    assert!(codes(
        "app: Output :: .Executable.{ name: \"demo\", entry: \"start\" }\nfn start() {}\n",
        jet::Sema::CompileMode::Check,
    )
    .contains(&"E1321".to_string()));
    assert!(codes(
        "api: Output :: .Service.{ name: \"api\", entry: serve }\nfn serve(port: Int) {}\n",
        jet::Sema::CompileMode::Check,
    )
    .contains(&"E1321".to_string()));
    assert!(codes(
        "release: Output :: .Check.{ name: \"release\", entry: verify }\nfn verify() => Int { return 1 }\n",
        jet::Sema::CompileMode::Check,
    )
    .contains(&"E1321".to_string()));
    assert!(codes(
        "app: Output :: .Executable.{ name: \"demo\", entry: start }\n#Unsafe(\"raw boundary\") fn start() {}\n",
        jet::Sema::CompileMode::Check,
    )
    .contains(&"E1321".to_string()));
    let ambiguity = codes(
        "one: Output :: .Executable.{ name: \"one\", entry: first }\ntwo: Output :: .Executable.{ name: \"two\", entry: second }\nfn first() {}\nfn second() {}\n",
        jet::Sema::CompileMode::Run,
    );
    assert_eq!(ambiguity.iter().filter(|code| *code == "E1321").count(), 1);
    assert!(ambiguity.contains(&"E0101".to_string()), "{ambiguity:?}");

    for source in [
        "lib: Output :: .Library.{ name: \"lib\" };\n",
        "api: Output :: .Service.{ name: \"api\", entry: serve }\nfn serve() {}\n",
    ] {
        let no_entry = codes(source, jet::Sema::CompileMode::Run);
        assert!(no_entry.contains(&"E0101".to_string()), "{no_entry:?}");
    }

    let dir = common::unique_tmp("jet_output_explicit_check_bad");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        "release: Output :: .Check.{ name: \"release\", entry: verify }\nfn verify() {}\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let explicit_check = jet::Sema::check_bundle_for_output(
        &mut bundle,
        jet::Sema::CompileMode::Run,
        "release",
    );
    assert!(
        explicit_check.iter().any(|diagnostic| diagnostic.code == "E1321"),
        "{explicit_check:?}"
    );

    for source in [
        "app: Output :: .Executable.{ name: \"app\", entry: start };\ndefaults: .{ run: missing };\nfn start() {}\n",
        "app: Output :: .Executable.{ name: \"app\", entry: start };\ndefaults: .{ run: missing };\nfn start() {}\nfn run() {}\n",
        "app: Output :: .Executable.{ name: \"app\", entry: start };\napi: Output :: .Service.{ name: \"api\", entry: serve };\ndefaults: .{ run: api };\nfn start() {}\nfn serve() {}\n",
    ] {
        let stale_default = codes(source, jet::Sema::CompileMode::Run);
        assert!(
            stale_default.contains(&"E1321".to_string()),
            "{stale_default:?}"
        );
    }
}

#[test]
fn invalid_output_selection_stops_in_jet_before_codegen() {
    fn reject(source: &str, args: &[&str], code: &str) {
        let dir = common::unique_tmp("jet_output_selection_cli_bad");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("main.jet");
        std::fs::write(&file, source).unwrap();
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_jet"));
        command.arg("run");
        command.args(args);
        let output = command.arg(&file).output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{stderr}");
        assert!(stderr.contains(&format!("Error [{code}]")), "{stderr}");
        assert!(!stderr.contains("internal compiler error"), "{stderr}");
        assert!(!stderr.contains("rustc rejected"), "{stderr}");
    }

    reject(
        "lib: Output :: .Library.{ name: \"lib\" };\n",
        &[],
        "E0101",
    );
    reject(
        "api: Output :: .Service.{ name: \"api\", entry: serve };\nfn serve() {}\n",
        &[],
        "E0101",
    );
    reject(
        "release: Output :: .Check.{ name: \"release\", entry: verify };\nfn verify() {}\n",
        &["--output", "release"],
        "E1321",
    );
    reject(
        "app: Output :: .Executable.{ name: \"app\", entry: start };\ndefaults: .{ run: missing };\nfn start() {}\nfn run() {}\n",
        &[],
        "E1321",
    );
}

#[test]
fn checked_default_selects_one_of_multiple_executables() {
    let bundle = checked_bundle(
        "one: Output :: .Executable.{ name: \"one\", entry: first };\ntwo: Output :: .Executable.{ name: \"two\", entry: second };\ndefaults: .{ run: two };\nfn first() { print(\"first\") }\nfn second() { print(\"second\") }\n",
        "jet_output_checked_default",
        jet::Sema::CompileMode::Run,
    );
    let selected = bundle.modules[bundle.entry].items.iter().find_map(|item| {
        let Item::Const(value) = item else { return None };
        value.resolved_output.as_ref().filter(|output| output.selected)
    }).expect("checked default selects one Output");
    assert_eq!(selected.address, "two");
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(rust.contains("jet_runtime_boundary(|| user_second())"), "{rust}");
    assert!(!rust.contains("jet_runtime_boundary(|| user_first())"), "{rust}");
}

#[test]
fn explicit_service_address_reaches_tir_and_real_cli_runtime() {
    if !common::have_rustc() {
        return;
    }
    let dir = common::unique_tmp("jet_output_service_cli");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        "app: Output :: .Executable.{ name: \"app\", entry: launch };\napi: Output :: .Service.{ name: \"api\", entry: serve };\nfn launch() { print(\"app\") }\nfn serve() { print(\"service\") }\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle_for_output(
        &mut bundle,
        jet::Sema::CompileMode::Run,
        "api",
    );
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let tir = jet::Codegen::TIR::lower_jit_program(&bundle).expect("Service lowers through TIR");
    assert_eq!(tir.entry, "serve");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "--output", "api", file.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .expect("run explicit Service Output");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "service\n");
}

#[test]
fn check_outputs_are_plural_real_test_harness_entries_without_test_blocks() {
    if !common::have_rustc() {
        return;
    }
    let dir = common::unique_tmp("jet_output_check_cli");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        "unit: Output :: .Check.{ name: \"unit\", entry: verify_unit };\nrelease: Output :: .Check.{ name: \"release\", entry: verify_release };\nfn verify_unit() {}\nfn verify_release() => () ? { return Err(\"release blocked\") }\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Test);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let rust = jet::Codegen::emit_bundle_tests(&bundle, None);
    assert!(rust.contains("fn jet_output_check_0()"), "{rust}");
    assert!(rust.contains("fn jet_output_check_1()"), "{rust}");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["test", file.to_str().unwrap()])
        .current_dir(&dir)
        .output()
        .expect("run Check Output harness");
    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("unit: pass"), "{stdout}");
    assert!(stdout.contains("release: FAIL"), "{stdout}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("release blocked"));
}

#[test]
fn renamed_entry_is_not_recovered_from_a_stale_lock() {
    let dir = common::unique_tmp("jet_output_callable_stale");
    std::fs::create_dir_all(dir.join(".jet")).unwrap();
    std::fs::write(
        dir.join(".jet/lock"),
        "version = 1\nentry = \"helper.start\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("helper.jet"), "pub fn renamed() {}\n").unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        "use \"helper\"\napp: Output :: .Executable.{ name: \"demo\", entry: helper.start };\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E1321"));
    assert!(bundle.modules[bundle.entry]
        .items
        .iter()
        .all(|item| { !matches!(item, Item::Const(value) if value.resolved_output.is_some()) }));
}

#[test]
fn typed_executable_output_reuses_the_checked_cli_schema() {
    let dir = common::unique_tmp("jet_output_callable_cli");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        "#CLI\nstruct Args { value: Int }\n\napp: Output :: .Executable.{ name: \"demo\", entry: launch };\n\nfn launch(args: Args) { print(args.value) }\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let schema = jet_foundation::CLISchema::entry_schema_for_bundle(&bundle)
        .expect("typed Output owns one checked CLI schema");
    assert_eq!(schema.entry_type, "Args");
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(rust.contains("__jet_cli_spec_Args"), "{rust}");
    assert!(rust.contains("user_launch(&__args)"), "{rust}");
}

#[test]
fn compiled_imported_typed_fallible_entry_uses_its_defining_module() {
    if !common::have_rustc() {
        return;
    }
    let dir = common::unique_tmp("jet_output_callable_imported_cli");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("helper.jet"),
        "#CLI\npub struct Args { value: Int }\n\npub fn launch(args: Args) => () ? {\n    print(args.value)\n    return Err(\"imported boom\")\n}\n",
    )
    .unwrap();
    let file = dir.join("main.jet");
    std::fs::write(
        &file,
        "use \"helper\"\napp: Output :: .Executable.{ name: \"demo\", entry: helper.launch };\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", file.to_str().unwrap(), "--", "--value", "42"])
        .current_dir(&dir)
        .output()
        .expect("run imported typed Output");
    assert_eq!(output.status.code(), Some(1), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("imported boom"));
}
