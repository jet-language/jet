use jet::Interpreter::RunOutcome;
use jet::AST::{Expr, Item, OutputKind, Type};

mod common;

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
    assert!(facts.reference_anchors.values().any(|anchor| {
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
        "release: Output :: .Check.{ name: \"release\", entry: verify }\nfn verify() -> Int { return 1 }\n",
        jet::Sema::CompileMode::Check,
    )
    .contains(&"E1321".to_string()));
    let ambiguity = codes(
        "one: Output :: .Executable.{ name: \"one\", entry: first }\ntwo: Output :: .Executable.{ name: \"two\", entry: second }\nfn first() {}\nfn second() {}\n",
        jet::Sema::CompileMode::Run,
    );
    assert_eq!(ambiguity.iter().filter(|code| *code == "E1321").count(), 1);
    assert!(!ambiguity.contains(&"E0101".to_string()), "{ambiguity:?}");
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
        "@[Cli]\nstruct Args { value: Int }\n\napp: Output :: .Executable.{ name: \"demo\", entry: launch };\n\nfn launch(args: Args) { print(args.value) }\n",
    )
    .unwrap();
    let mut bundle = jet::Loader::load_entry(file.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
    assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    let schema = jet_foundation::CliSchema::entry_schema_for_bundle(&bundle)
        .expect("typed Output owns one checked CLI schema");
    assert_eq!(schema.entry_type, "Args");
    let rust = jet::Codegen::emit_bundle(&bundle, jet::Sema::CompileMode::Run, None);
    assert!(rust.contains("__jet_cli_spec_Args"), "{rust}");
    assert!(rust.contains("user_launch(&__args)"), "{rust}");
}
