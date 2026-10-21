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
            ("<corelib>/Core/Encoding/Encoding.jet", "core_encoding", "#Error struct EncodingError { reason: String }\n", &[]),
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
        (&format!("<corelib>/{}", part.source.path), part.source.alias, "enum PathPlatform { Posix Windows }\n", &[]),
        ("<corelib>/Core/Sys/Sys.jet", "core_core_sys", "use __core_files_path_components as host_path\nuse __core_files_path_components.[PathPlatform]\nfn platform(value: PathPlatform) -> PathPlatform Never! { value.copy() }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn private_native_email_view_is_nameable_only_by_its_core_owner() {
    let source = "fn subject(message: __EmailMessageView) -> String Never! { message.subject.copy() }\n";
    let errors = check_sources(&[
        ("<corelib>/Core/Email/Email.jet", "core_core_email", source, &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    let errors = check_sources(&[
        ("<corelib>/Core/Files/Files.jet", "core_files", source, &[]),
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
fn csv_public_reader_report_is_handled_in_value_and_statement_matches() {
    let source = r#"
use core.encoding.csv as csv
use core.encoding.csv.[CSVReader]
use core.encoding.[EncodingError, EncodingLimits]
use core.files.[FileReader]
fn reader(input: ^FileReader, limits: EncodingLimits) -[FS]> CSVReader? EncodingError! {
    row :: if csv.reader(^input, limits, ",", false, false) == {
        .Ok(value) -> Val(^value)
        .Err(cause) -> { return Err(cause.copy()) }
    }
    row
}
fn statement_reader(input: ^FileReader, limits: EncodingLimits) -[FS]> CSVReader? EncodingError! {
    row := CSVReader?{None}
    if csv.reader(^input, limits, ",", false, false) == {
        .Ok(value) -> { row = Val(^value) }
        .Err(cause) -> { return Err(cause.copy()) }
    }
    row
}
fn run() {}
"#;
    let errors = check_sources(&[
        ("pkg/main.jet", "main", source, &[]),
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
        ("<corelib>/Core/Service/Service.jet", "core_core_service", "#Error enum ServiceError { Full(String) }\n", &[]),
        ("<corelib>/Core/Jobs/Jobs.jet", "core_core_jobs", "use core as core\nuse core.service.[ServiceError]\nuse core.service as service\nstruct Wrapped { error: service.ServiceError }\nfn wrap(error: ServiceError) -> Wrapped Never! { Wrapped{error: error} }\nfn forward() ServiceError! {\n endpoint :: core.service.__active_endpoint() ?? return Err(err)\n parts :: core.service.__endpoint_authority_parts(endpoint) ?? return Err(err)\n Ok(())\n}\n", &[]),
        ("<corelib>/Core/Crypto/Crypto.jet", "core_core_crypto", "use bridge as jetc\nstruct Secret { bytes: [U8] }\nfn wrap() -> jetc.Wrapped Never! { jetc.Wrapped{key: Secret{bytes: [U8]{}}} }\n", &[("jetc", 3)]),
        ("pkg/bridge.jet", "bridge", "use core.crypto as crypto\nstruct Wrapped { key: crypto.Secret }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn native_core_enum_and_error_constructors_resolve_without_public_exports() {
    let errors = check_sources(&[
        ("<corelib>/Core/Service/Service.jet", "core_core_service", "use core as core\nfn adapter() -> __TreeStateAdapter Never! { __TreeStateAdapter.Empty }\nfn failure() -> TaskFailure Never! { TaskFailure.Cancelled }\n", &[]),
        ("<corelib>/Core/Time/Time.jet", "core_time", "use core as core\nfn expired() -> Int Expired! { Err(Expired{}) }\nstruct Clock {}\nimpl Clock { fn now(self) -> Int Never! { 1 } }\nfn sample(clock: Clock) -> Int Never! { clock.now() }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    let errors = check_sources(&[
        ("<corelib>/Core/Files/Files.jet", "core_files", "fn adapter() -> __TreeStateAdapter Never! { __TreeStateAdapter.Empty }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.iter().any(|error| error.starts_with("E0119:")), "{errors:?}");
}

#[test]
fn source_owned_clock_methods_precede_native_time_contracts() {
    let core = r#"
use core as core
struct Clock { value: Int }
impl Clock {
    fn now(self) -> Int Never! { self.value }
    fn set(&self, value: Int) Never! { self.value = value }
    fn tick(&self, millis: Int) -> Int Never! {
        &self.set(self.now() + millis)
        self.now()
    }
    fn advance(&self, to_ms: Int) -> Int Never! {
        &self.set(to_ms)
        self.now()
    }
    fn wait(&self, duration: Duration) -> Int Never! { &self.tick(duration.ns) }
}
fn monotonic() -> Int Never! { core.time.__clock_mono_ns() }
fn sample(clock: &Clock, other: Clock, duration: Duration) -> Int Never! {
    &clock.set(1)
    tick :: &clock.tick(2)
    advance :: &clock.advance(3)
    wait :: &clock.wait(duration)
    equal :: clock.equal(other)
    if equal -> tick + advance + wait else -> clock.now()
}
"#;
    let errors = check_sources(&[
        ("<corelib>/Core/Time/Time.jet", "core_time", core, &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn retained_core_arguments_accept_fresh_values_but_not_unmarked_places() {
    let source = "use core as core\nfn append(tree: &ServiceTree, event: String) Never! { core.service.__tree_event_push(&tree, event.copy()) }\n";
    for value in ["event.copy()", "\"fresh\""] {
        let source = source.replace("event.copy()", value);
        let errors = check_sources(&[
            ("<corelib>/Core/Service/Service.jet", "core_core_service", &source, &[]),
            ("pkg/main.jet", "main", "fn run() {}\n", &[]),
        ]);
        assert!(errors.is_empty(), "{errors:?}");
    }
    let errors = check_sources(&[
        ("<corelib>/Core/Service/Service.jet", "core_core_service", "use core as core\nfn group(tree: &ServiceTree, events: [String]) Never! { core.service.__group_push(&tree, \"fresh\", events[0..<1]) }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    let source = source.replace("event.copy()", "event");
    let errors = check_sources(&[
        ("<corelib>/Core/Service/Service.jet", "core_core_service", &source, &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.iter().any(|error| error.starts_with("E0201:")), "{errors:?}");
}

#[test]
fn private_xml_alias_keeps_the_engine_enum_owner() {
    let errors = check_sources(&[
        ("<corelib>/Core/Encoding/XmlEngine.jet", "__core_encoding_xml_engine", "enum EntityPolicy { Preserve Expand }\nenum RenderEncoding { UTF8 UTF16 }\n", &[]),
        ("<corelib>/Core/Encoding/Xml.jet", "core_xml", "struct XMLReader {}\n", &[]),
        ("<corelib>/Core/Encoding/XmlKernel.jet", "__core_encoding_xml_kernel", "use __core_encoding_xml_engine as xml\nfn policy() -> xml.EntityPolicy Never! { xml.EntityPolicy.Preserve }\nfn encoding() -> xml.RenderEncoding Never! { xml.RenderEncoding.UTF8 }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn same_named_source_records_keep_their_declared_fields_and_nominal_owner() {
    let engine = "struct Limits { max_depth: Int }\nfn safe() -> Limits Never! { Limits{max_depth: 8} }\n";
    let kernel = "use __core_encoding_xml_engine as xml\nfn safe() -> xml.Limits Never! { xml.Limits{max_depth: 8} }\n";
    let errors = check_sources(&[
        ("<corelib>/Core/Email/Email.jet", "core_core_email", "struct Limits { max_reply_line_bytes: Int }\n", &[]),
        ("<corelib>/Core/Encoding/XmlEngine.jet", "__core_encoding_xml_engine", engine, &[]),
        ("<corelib>/Core/Encoding/XmlKernel.jet", "__core_encoding_xml_kernel", kernel, &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    let errors = check_sources(&[
        ("pkg/first.jet", "first", "struct Limits { value: Int }\n", &[]),
        ("pkg/second.jet", "second", "struct Limits { value: Int }\n", &[]),
        ("pkg/main.jet", "main", "use first as first\nuse second as second\nfn mismatch(value: first.Limits) -> second.Limits Never! { value.copy() }\nfn run() {}\n", &[("first", 0), ("second", 1)]),
    ]);
    assert!(errors.iter().any(|error| error.starts_with("E0113:")), "{errors:?}");
}

#[test]
fn labeled_enum_brace_constructors_accept_reordered_fields_and_reject_missing_fields() {
    let source = "enum Part { Text(value: String, raw: String) }\nfn make() -> Part Never! { Part.Text{raw: \"raw\", value: \"text\"} }\nfn run() {}\n";
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(errors.is_empty(), "{errors:?}");
    for body in ["Part.Text{value: \"text\"}", "Part.Text(value: \"text\", raw: \"raw\")"] {
        let source = source.replace("Part.Text{raw: \"raw\", value: \"text\"}", body);
        let errors = check_sources(&[("pkg/main.jet", "main", &source, &[])]);
        assert!(errors.iter().any(|error| error.starts_with("E0303:")), "{errors:?}");
    }
}

#[test]
fn temporary_list_and_call_ranges_are_owned_not_place_views() {
    let source = "fn bytes() -> [U8] Never! { [U8]{1, 2} }\nfn literal() -> [U8] Never! { [U8]{1, 2}[0..<1] }\nfn call() -> [U8] Never! { bytes()[0..<1] }\nfn need(value: View<U8>) Never! {}\nfn bound() Never! { data :: [U8]{1, 2}\n need(data[0..<1]) }\nfn run() {}\n";
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(errors.is_empty(), "{errors:?}");
    for value in ["[U8]{1, 2}[0..<1]", "bytes()[0..<1]"] {
        let source = format!("{source}\nfn temporary() Never! {{ need({value}) }}\n");
        let errors = check_sources(&[("pkg/main.jet", "main", &source, &[])]);
        assert!(errors.iter().any(|error| error.starts_with("E0112:")), "{errors:?}");
    }
}

#[test]
fn core_sources_share_private_declarations_without_public_exports() {
    let time = "fn __expiry_deadline(now_ms: Int, ttl: Duration) -> Int Never! { now_ms + ttl.ns }\nstruct __Observer { value: Int }\n";
    let core = "use core.time as time\nfn failure() -> Int time.Expired! { Err(time.Expired{}) }\nfn deadline(ttl: Duration) -> Int Never! { time.__expiry_deadline(1, ttl) }\nfn observe(value: time.__Observer) -> Int Never! { value.value }\n";
    let errors = check_sources(&[
        ("<corelib>/Core/Time/Time.jet", "core_time", time, &[]),
        ("<corelib>/Core/Crypto/Vault.jet", "core_core_crypto_vault", core, &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    let errors = check_sources(&[
        ("<corelib>/Core/Time/Time.jet", "core_time", time, &[]),
        ("pkg/main.jet", "main", "use core.time as time\nfn failure() -> Int time.Expired! { Err(time.Expired{}) }\nfn run() {}\n", &[]),
    ]);
    assert!(!errors.is_empty(), "Core-private native carrier leaked to user code");
    for source in [
        "use core.time as time\nfn deadline(ttl: Duration) -> Int Never! { time.__expiry_deadline(1, ttl) }\nfn run() {}\n",
        "use core.time as time\nfn observe(value: time.__Observer) -> Int Never! { value.value }\nfn run() {}\n",
    ] {
        let (_, diagnostics) = Lexer::lex(source);
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0067"), "{diagnostics:?}");
    }
}

#[test]
fn loaded_core_source_functions_resolve_without_a_native_catalog_row() {
    let errors = check_sources(&[
        ("<corelib>/Core/Text/Text.jet", "core_text", "fn encode_cp(value: Int) -> String Never! { \"scalar\" }\n", &[]),
        ("<corelib>/Core/Encoding/XmlEngine.jet", "__core_encoding_xml_engine", "use core.text as unicode\nfn carriage_return() -> String Never! { unicode.encode_cp(13) }\n", &[]),
        ("pkg/main.jet", "main", "fn run() {}\n", &[]),
    ]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn private_crypto_preallocation_admits_only_exact_reserve() {
    for element in ["I64", "U8"] {
        let source = format!(
            "use core.mem as mem\nfn reserve(&data: [{element}]) Never! {{\n\
             #Unsafe(\"exact private crypto preallocation\") {{\n\
             mem.__list_reserve_exact<{element}>(&data, 32)\n\
             }}\n}}\n"
        );
        let errors = check_sources(&[
            ("<corelib>/Core/Crypto/ExpertCt.jet", "__core_crypto_ct", &source, &[]),
            ("pkg/main.jet", "main", "fn run() {}\n", &[]),
        ]);
        assert!(errors.is_empty(), "{errors:?}");
        for (path, alias) in [
            ("<corelib>/Core/Crypto/Jetc.jet", "__core_crypto_jetc"),
            ("<corelib>/Core/Crypto/Vault.jet", "core_core_crypto_vault"),
        ] {
            let errors = check_sources(&[
                (path, alias, &source, &[]),
                ("pkg/main.jet", "main", "fn run() {}\n", &[]),
            ]);
            assert!(errors.iter().any(|error| error.starts_with("E0510:")), "{errors:?}");
        }
    }
    for source in [
        "use core.mem as mem\nfn capacity(data: [U8]) -> Int Never! { mem.__list_capacity<U8>(data) }\n",
        "use core.mem as mem\nfn size() -> Int Never! { mem.__type_size<U8>() }\n",
    ] {
        let errors = check_sources(&[
            ("<corelib>/Core/Crypto/ExpertCt.jet", "__core_crypto_ct", source, &[]),
            ("pkg/main.jet", "main", "fn run() {}\n", &[]),
        ]);
        assert!(errors.iter().any(|error| error.starts_with("E0510:")), "{errors:?}");
    }
}

#[test]
fn clock_labeled_state_constructs_and_updates_inside_shared_values() {
    let source = r#"
enum ClockState { System(started_ns: Int, offset_ms: Int) }
struct ClockStateOwner { state: Shared<ClockState> }
fn make() -> ClockStateOwner Never! {
    state := ClockState.System{started_ns: 1, offset_ms: 0}
    state = ClockState.System{started_ns: 2, offset_ms: 1}
    ClockStateOwner{state: shared state}
}
fn run() {}
"#;
    let errors = check_sources(&[("pkg/main.jet", "main", source, &[])]);
    assert!(errors.is_empty(), "{errors:?}");
}
