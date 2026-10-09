use jet_sema::AST::{CFfi, ImportKind, LoadedModule, ProgramBundle};
use jet_sema::Diagnostics::Severity;
use jet_sema::Sema::{check_bundle, CompileMode};
use jet_sema::{Lexer, Parser, Syntax};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Once;

fn check_sources(sources: &[(&str, &str, &str, &[(&str, usize)])]) -> Vec<String> {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(jet_codegen::Codegen::MIREval::install_mir_bridge);
    let mut modules = Vec::new();
    let mut name_ledger = jet_sema::AST::NameLedger::default();
    for (index, (path, alias, source, targets)) in sources.iter().enumerate() {
        let (tokens, diagnostics) = if path.starts_with("<corelib>/") {
            Lexer::lex_generated(source)
        } else {
            Lexer::lex(source)
        };
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let mut program = Parser::parse(&tokens).expect("regression source parses");
        for (import_alias, target) in *targets {
            let import = program.imports.iter().find(|import|
                import.import_alias() == *import_alias
                    && !matches!(import.kind, ImportKind::Unqualified { .. })).unwrap();
            name_ledger.record_import_target(index, import.span, *target);
        }
        modules.push(LoadedModule {
            path: PathBuf::from(path), display: (*path).into(), alias: (*alias).into(),
            imports: std::mem::take(&mut program.imports), items: program.items,
            script_body: program.script_body, source: (*source).into(),
            core_identifier_demand: Lexer::core_identifier_demand(&tokens),
            block_spans: program.block_spans, web_target_ceiling: program.web_target_ceiling,
            no_prelude: program.no_prelude, default_target: program.default_target,
            html_path: program.html_path, policy_declarations: program.policy_declarations,
            user_policy_declarations: program.user_policy_declarations, rule_facts: program.rule_facts,
        });
    }
    let mut bundle = ProgramBundle {
        entry: sources.len() - 1, project_root: PathBuf::from("pkg"), modules,
        devtools_registry: Default::default(), parse_teaching: Vec::new(),
        used_core: HashSet::new(), ffi_callback_fns: HashSet::new(), cffi: CFfi::default(),
        comptime_inputs: Vec::new(), name_ledger, layer_ceiling: None,
        inferred_layer: Syntax::RuntimeLayer::Core, web_partitions: HashMap::new(),
        web_partition_enforced: false, web_partition_report: None, dep_roots: HashMap::new(),
        package_guarantees: Default::default(), program_allocator: Default::default(),
        active_os: Syntax::OSTarget::host(), build_facts: Default::default(), edition: "2027".into(),
    };
    check_bundle(&mut bundle, CompileMode::Eval).into_iter()
        .filter(|diagnostic| diagnostic.severity == Severity::Error)
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.what)).collect()
}

#[test]
fn unit_fallbacks_resolve_grouped_and_qualified_error_domains() {
    for (import, error) in [
        ("use core.encoding.[EncodingError]", "EncodingError"),
        ("use core.encoding as encoding", "encoding.EncodingError"),
    ] {
        let source = format!("{import}\nfn fail(error: {error}) {error}! {{ Err(error) }}\nfn forward(error: {error}) {error}! {{\n fail(error) ?? return Err(err)\n Ok(())\n}}\nfn run() {{}}\n");
        let errors = check_sources(&[
            ("<corelib>/Core/encoding/encoding.jet", "core_encoding", "#Error struct EncodingError { reason: String }\n", &[]),
            ("pkg/main.jet", "main", &source, &[]),
        ]);
        assert!(errors.is_empty(), "{errors:?}");
    }
}

#[test]
fn imported_signatures_and_fields_resolve_in_the_declarers_import_scope() {
    let errors = check_sources(&[
        ("pkg/model.jet", "model", "struct Token { value: Int }\n", &[]),
        ("pkg/bridge.jet", "bridge", "use model as origin\nstruct Wrapped { token: origin.Token }\nfn pass(token: origin.Token) -> origin.Token Never! { token.copy() }\nfn optional(token: origin.Token) -> origin.Token? Never! { Val(token.copy()) }\n", &[("origin", 0)]),
        ("pkg/main.jet", "main", "use model as model\nuse bridge as bridge\nfn read(token: model.Token, wrapped: bridge.Wrapped) -> Int Never! {\n passed :: bridge.pass(token)\n optional :: bridge.optional(token) ?? token.copy()\n passed.value + optional.value + wrapped.token.value\n}\nfn run() {}\n", &[("model", 0), ("bridge", 1)]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn sealed_part_grouped_import_does_not_require_its_reserved_module_alias() {
    let part = jet_foundation::CoreSourceParts::core_private_source_part_by_alias("__core_files_path_components").unwrap();
    let errors = check_sources(&[
        (&format!("<corelib>/{}", part.source.path), part.source.alias, "enum PathPlatform { Posix, Windows }\n", &[]),
        ("<corelib>/Core/sys/sys.jet", "core_core_sys", "use __core_files_path_components as host_path\nuse __core_files_path_components.[PathPlatform]\nfn platform(value: PathPlatform) -> PathPlatform Never! { value.copy() }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn private_native_email_view_is_nameable_only_by_its_core_owner() {
    let source = "fn subject(message: __EmailMessageView) -> String Never! { message.subject.copy() }\n";
    let errors = check_sources(&[
        ("<corelib>/Core/email/email.jet", "core_core_email", source, &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    let errors = check_sources(&[
        ("<corelib>/Core/files/files.jet", "core_files", source, &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.iter().any(|error| error.starts_with("E0119:")), "{errors:?}");
}
