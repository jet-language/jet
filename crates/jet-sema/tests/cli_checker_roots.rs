use jet_sema::AST::{CFfi, Item, LoadedModule, ProgramBundle, Type};
use jet_sema::Diagnostics::{Diagnostic, Severity};
use jet_sema::Sema::{check_bundle_with_effect_facts_for_build, CompileMode};
use jet_sema::{Lexer, Parser, Syntax};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Once;

fn check(sources: &[(&str, &str, &str)]) -> (ProgramBundle, Vec<Diagnostic>) {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(jet_codegen::Codegen::MIREval::install_mir_bridge);
    let modules = sources.iter().map(|(path, alias, source)| {
        let (tokens, diagnostics) = if path.starts_with("<corelib>/") {
            Lexer::lex_generated(source)
        } else {
            Lexer::lex(source)
        };
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let program = Parser::parse(&tokens).expect("regression parses");
        LoadedModule {
            path: PathBuf::from(path), display: (*path).into(), alias: (*alias).into(),
            imports: program.imports, items: program.items, script_body: program.script_body,
            source: (*source).into(), core_identifier_demand: Lexer::core_identifier_demand(&tokens),
            block_spans: program.block_spans, web_target_ceiling: program.web_target_ceiling,
            no_prelude: program.no_prelude, default_target: program.default_target,
            html_path: program.html_path, policy_declarations: program.policy_declarations,
            user_policy_declarations: program.user_policy_declarations, rule_facts: program.rule_facts,
        }
    }).collect();
    let mut bundle = ProgramBundle {
        entry: sources.len() - 1, project_root: PathBuf::from("pkg"), modules,
        devtools_registry: Default::default(), parse_teaching: Vec::new(),
        used_core: HashSet::new(), ffi_callback_fns: HashSet::new(), cffi: CFfi::default(),
        comptime_inputs: Vec::new(), name_ledger: Default::default(), layer_ceiling: None,
        inferred_layer: Syntax::RuntimeLayer::Core, web_partitions: HashMap::new(),
        web_partition_enforced: false, web_partition_report: None, dep_roots: HashMap::new(),
        package_guarantees: Default::default(), program_allocator: Default::default(),
        active_os: Syntax::OSTarget::host(), build_facts: Default::default(), edition: "2028".into(),
    };
    let diagnostics = check_bundle_with_effect_facts_for_build(&mut bundle, CompileMode::Run).0;
    (bundle, diagnostics)
}

fn assert_no_errors(diagnostics: &[Diagnostic]) {
    assert!(diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error),
        "{diagnostics:#?}");
}

fn constant_type(bundle: &ProgramBundle, name: &str) -> Type {
    bundle.modules[bundle.entry].items.iter().find_map(|item| match item {
        Item::Const(constant) if constant.name == name => constant.ty.clone(),
        _ => None,
    }).expect("checked constant type")
}

#[test]
fn prep_keeps_explicit_byte_list_elements_including_empty_lists() {
    let (bundle, diagnostics) = check(&[("pkg/main.jet", "main", r#"
BYTES :: prep { [U8]{97, 117, 116, 111} }
EMPTY :: prep { [U8]{} }
fn same(value: [U8]) -> Bool Never! { value == BYTES }
fn accept(value: [U8]) -> Int Never! { value.len() }
fn empty_count() -> Int Never! { accept(EMPTY) }
fn run() {}
"#)]);
    assert_no_errors(&diagnostics);
    let bytes = Type::List(Box::new(Type::IntN { signed: false, bits: 8 }));
    assert_eq!(constant_type(&bundle, "BYTES"), bytes);
    assert_eq!(constant_type(&bundle, "EMPTY"), bytes);
}

#[test]
fn prep_keeps_never_failing_function_and_row_element_types() {
    let (bundle, diagnostics) = check(&[("pkg/main.jet", "main", r#"
struct Row { value: Int }
fn symbols() -> [String] Never! { [String]{"stop"} }
fn row() -> Row Never! { Row{value: 1} }
SYMBOLS :: prep { symbols() }
ROWS :: prep { [Row]{row()} }
OPTIONALS :: prep { [Row?]{Val(row()), None} }
fn accept(rows: [Row]) -> Int Never! { rows.len() }
fn row_and_symbol_count() -> Int Never! { accept(ROWS) + SYMBOLS.len() }
fn run() {}
"#)]);
    assert_no_errors(&diagnostics);
    assert_eq!(constant_type(&bundle, "SYMBOLS"), Type::List(Box::new(Type::String)));
    let rows = constant_type(&bundle, "ROWS");
    assert!(matches!(&rows, Type::List(inner) if matches!(inner.as_ref(), Type::Named(_))), "{rows:?}");
    let optionals = constant_type(&bundle, "OPTIONALS");
    assert!(matches!(optionals, Type::List(inner) if matches!(inner.as_ref(), Type::Option(_))));
}

#[test]
fn locals_shadow_imported_and_implicit_core_modules() {
    for imports in ["", "use core.term as term\nuse core.encoding.hex as hex\nuse core.time as time\nuse core.files as files\n"] {
        let source = format!(r#"{imports}
fn terminal(term: [U8]) -> Int Never! {{ term.len() }}
fn file_count(files: [String]) -> Int Never! {{ files.len() }}
fn hex_digits() -> Int Never! {{
    hex :: "0123456789abcdef"
    hex.bytes().len()
}}
fn time_names(name: String) -> Bool Never! {{
    time :: [String]{{"JetDate", "JetDuration"}}
    time.contains(name)
}}
fn run() {{}}
"#);
        let (_, diagnostics) = check(&[("pkg/main.jet", "main", &source)]);
        assert_no_errors(&diagnostics);
    }
}

#[test]
fn module_alias_qualified_core_enum_variants_keep_the_source_owner() {
    let (_, diagnostics) = check(&[
        ("<corelib>/Core/encoding/encoding.jet", "core_encoding", "#Equatable enum EncodingFormat { JSON CSV }\n#Equatable enum EncodingErrorKind { Unsupported Syntax }\n"),
        ("pkg/main.jet", "main", r#"
use core.encoding as comptime_encoding
fn format() -> comptime_encoding.EncodingFormat Never! { comptime_encoding.EncodingFormat.JSON }
fn kind() -> comptime_encoding.EncodingErrorKind Never! { comptime_encoding.EncodingErrorKind.Unsupported }
fn run() {}
"#),
    ]);
    assert_no_errors(&diagnostics);
}

#[test]
fn public_dyn_bytes_patterns_bind_their_payload() {
    let (_, diagnostics) = check(&[("pkg/main.jet", "main", r#"
fn run() {
    value :: Dyn.Bytes([U8]{1})
    if value == .Bytes(bytes) { print(bytes.len()) }
}
"#)]);
    assert_no_errors(&diagnostics);
}

#[test]
fn dyn_datetime_constructor_accepts_owned_zoned_payload() {
    let (_, diagnostics) = check(&[("pkg/main.jet", "main", include_str!(
        "../../../Compiler/JetSema/Tests/DynDateTimeFixtures.jet"
    ))]);
    assert_no_errors(&diagnostics);
}

#[test]
fn dyn_datetime_constructor_rejects_wrong_payload() {
    let (_, diagnostics) = check(&[("pkg/main.jet", "main",
        "fn run() { _ := Dyn.DateTime(3) }\n")]);
    assert!(diagnostics.iter().any(|diag|
        diag.code == "E0112" && diag.what.contains("ZonedDateTime")
    ), "{diagnostics:?}");
}

#[test]
fn dyn_datetime_restore_does_not_expose_number_constructor() {
    let (_, diagnostics) = check(&[("pkg/main.jet", "main",
        "fn run() { _ := Dyn.Number(\"1\") }\n")]);
    assert!(diagnostics.iter().any(|diag| diag.code == "E0304"), "{diagnostics:?}");
}

#[test]
fn dyn_datetime_toml_number_helpers_transfer_validated_text() {
    let stream = include_str!("../../../Core/encoding/stream.jet");
    let typed_number = stream.split_once("fn typed_number(").unwrap().1
        .split_once("\nfn typed_string(").unwrap().0;
    let stream_source = format!("fn typed_number({typed_number}");
    let toml = include_str!("../../../Core/encoding/toml.jet");
    let numeric_helpers = toml.split_once("fn number_tree(").unwrap().1;
    let toml_source = format!(
        "use __core_encoding_stream as stream\nfn number_tree({numeric_helpers}\n\
         fn check_integer() -> Dyn Never! {{ stream.typed_number(\"2\") }}\n\
         fn check_timestamp(token: ^String) -> Dyn Never! {{\n\
             stamp :: Dyn.Text(^token)\n\
             if stamp.decode<ZonedDateTime>() == {{\n\
                 .Ok(z) -> {{ return Dyn.DateTime(z) }}\n\
                 .Err(_) -> {{}}\n\
             }}\n\
             ^stamp\n\
         }}\n"
    );
    let (_, diagnostics) = check(&[
        ("<corelib>/Core/encoding/stream.jet", "__core_encoding_stream", &stream_source),
        ("<corelib>/Core/encoding/toml.jet", "__core_encoding_toml", &toml_source),
        ("pkg/main.jet", "main", "fn run() {}\n"),
    ]);
    assert_no_errors(&diagnostics);
}

#[test]
fn core_auth_store_replacement_transfers_owned_bytes() {
    let auth = include_str!("../../../Core/auth/auth.jet");
    let store_replace = auth.split_once("fn __store_replace(").unwrap().1
        .split_once("\n/// Register").unwrap().0;
    let auth_source = format!("use core as core\nfn __store_replace({store_replace}");
    let (_, diagnostics) = check(&[
        ("<corelib>/Core/auth/auth.jet", "core_core_auth", &auth_source),
        ("pkg/main.jet", "main", "fn run() {}\n"),
    ]);
    assert_no_errors(&diagnostics);
}
