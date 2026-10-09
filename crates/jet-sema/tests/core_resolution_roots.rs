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

#[test]
fn jobs_optional_projection_preserves_absence_while_forwarding_report() {
    let source = r#"
#Error enum ServiceError { Invalid }
fn optional_text() -> String? ServiceError! { None }
fn project() -> String? ServiceError! {
    key :: if optional_text() == {
        .Val(value) -> Val(value)
        .None -> None
        .Err(error) -> { return Err(error) }
    }
    key
}
fn run() {}
"#;
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn xml_marked_optional_dispatch_and_result_fallback_own_the_call_carrier() {
    let source = r#"
use core.encoding.[Dyn]
#Error enum XMLFault { Invalid }
struct Scanner {
    fn next(&self) -> Dyn? XMLFault! { None }
    fn required(&self) -> Dyn XMLFault! { Err(XMLFault.Invalid) }
}
fn next(scanner: &Scanner) -> Dyn? XMLFault! {
    candidate :: if (&scanner.next()) == {
        .Val(event) -> Val(event)
        .None -> None
        .Err(error) -> { return Err(error) }
    }
    if candidate == .Val(event) -> return Val(event)
    None
}
fn required(scanner: &Scanner) -> Dyn XMLFault! {
    &scanner.required() ?? return Err(err)
}
fn run() {}
"#;
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn csv_internal_report_is_handled_without_public_error_domain_validation() {
    let source = r#"
use core.encoding as encoding
use core.files.[FileReader]
#Error enum StreamError { IO(String) }
fn byte(input: &FileReader) -> U8? StreamError! {
    byte :: if encoding.__read_byte(&input) == {
        .Ok(value) -> value
        .Err(cause) -> { return Err(StreamError.IO(cause.message)) }
    }
    byte
}
fn statement_byte(input: &FileReader) -> U8? StreamError! {
    byte := U8?{None}
    if encoding.__read_byte(&input) == {
        .Ok(value) -> { byte = value }
        .Err(cause) -> { return Err(StreamError.IO(cause.message)) }
    }
    byte
}
"#;
    let errors = check_sources(&[
        ("<corelib>/Core/encoding/csv.jet", "core_csv", source, &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn value_match_braced_error_arm_keeps_optional_sibling_context() {
    for arms in [
        ".Val(value) -> Val(value)\n.None -> None\n.Err(error) -> { return Err(error) }",
        ".None -> None\n.Val(value) -> Val(value)\n.Err(error) -> { return Err(error) }",
    ] {
        let source = format!("#Error enum Fault {{ Bad }}\nfn read() -> Int? Fault! {{ None }}\nfn project() -> Int? Fault! {{\n value :: if read() == {{ {arms} }}\n value\n}}\nfn run() {{}}\n");
        let errors = check_sources(&[("pkg/main.jet", "main", &source, &[])]);
        assert!(errors.is_empty(), "{errors:?}");
    }
}

#[test]
fn source_core_enum_and_struct_keep_their_declaration_identity() {
    let errors = check_sources(&[
        ("<corelib>/Core/service/service.jet", "core_core_service", "#Error enum ServiceError { Full(String) }\n", &[]),
        ("<corelib>/Core/jobs/jobs.jet", "core_core_jobs", "use core.service.[ServiceError]\nuse core.service as service\nstruct Wrapped { error: service.ServiceError }\nfn wrap(error: ServiceError) -> Wrapped Never! { Wrapped{error: error} }\n", &[]),
        ("<corelib>/Core/crypto/crypto.jet", "core_core_crypto", "use bridge as jetc\nstruct Secret { bytes: [U8] }\nfn wrap() -> jetc.Wrapped Never! { jetc.Wrapped{key: Secret{bytes: [U8]{}}} }\n", &[("jetc", 3)]),
        ("pkg/bridge.jet", "bridge", "use core.crypto as crypto\nstruct Wrapped { key: crypto.Secret }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn native_core_enum_and_error_constructors_resolve_without_public_exports() {
    let errors = check_sources(&[
        ("<corelib>/Core/service/service.jet", "core_core_service", "use core as core\nfn adapter() -> __TreeStateAdapter Never! { __TreeStateAdapter.Empty }\nfn failure() -> TaskFailure Never! { TaskFailure.Cancelled }\n", &[]),
        ("<corelib>/Core/time/time.jet", "core_time", "use core as core\nfn expired() -> Int Expired! { Err(Expired{}) }\nstruct Clock {}\nimpl Clock { fn now(self) -> Int Never! { 1 } }\nfn sample(clock: Clock) -> Int Never! { clock.now() }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    let errors = check_sources(&[
        ("<corelib>/Core/files/files.jet", "core_files", "fn adapter() -> __TreeStateAdapter Never! { __TreeStateAdapter.Empty }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.iter().any(|error| error.starts_with("E0119:")), "{errors:?}");
}

#[test]
fn retained_core_arguments_accept_fresh_values_but_not_unmarked_places() {
    let source = "use core as core\nfn append(tree: &ServiceTree, event: String) Never! { core.service.__tree_event_push(&tree, event.copy()) }\n";
    for value in ["event.copy()", "\"fresh\""] {
        let source = source.replace("event.copy()", value);
        let errors = check_sources(&[
            ("<corelib>/Core/service/service.jet", "core_core_service", &source, &[]),
            ("pkg/main.jet", "main", "fn run() {}\n", &[]),
        ]);
        assert!(errors.is_empty(), "{errors:?}");
    }
    let errors = check_sources(&[
        ("<corelib>/Core/service/service.jet", "core_core_service", "use core as core\nfn group(tree: &ServiceTree, events: [String]) Never! { core.service.__group_push(&tree, \"fresh\", events[0..<1]) }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    let source = source.replace("event.copy()", "event");
    let errors = check_sources(&[
        ("<corelib>/Core/service/service.jet", "core_core_service", &source, &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.iter().any(|error| error.starts_with("E0201:")), "{errors:?}");
}
