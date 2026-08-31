//! D-FAIL-CONV1: one declared conversion rail, including the default Err target.

mod common;

#[test]
fn declared_conversion_reaches_default_error() {
    let source = r#"
#Error
enum StoreErr { Missing }

impl StoreErr -> Err {
    return Err("missing")
}

fn read_store() Int !StoreErr -> {
    return Err(StoreErr.Missing)
}

fn get_user() Int -> {
    return Ok(read_store())
}

fn run() {
    get_user()
}
"#;
    let compiled = jet::compile(source).expect("declared StoreErr -> Err must compile");
    assert!(compiled.rust.contains("errconv_StoreErr_to_Err"));
}

#[test]
fn conversion_names_with_to_preserve_type_identity_and_symbol_uniqueness() {
    if !common::have_rustc() {
        eprintln!("rustc unavailable; skipping AOT conversion regression");
        return;
    }

    // Qualified/generated type identities can contain `_to_`; the two pairs
    // below used to produce the same helper symbol after flattening.
    let left = jet::Sema::error_conv_fn_name("Source_to_Left", "Target");
    let right = jet::Sema::error_conv_fn_name("Source", "Left_to_Target");
    assert_ne!(
        left, right,
        "error conversion helper names must be injective"
    );

    let source = r#"
#Error
struct SourceErr {
    message: String
}

#Error
struct TargetErr {
    message: String
}

impl SourceErr -> TargetErr {
    return TargetErr{message: "converted from source"}
}

impl TargetErr -> Err {
    return Err("converted from target")
}

fn read() Int !SourceErr -> {
    return Err(SourceErr{message: "source failure"})
}

fn middle() Int !TargetErr -> {
    value :: read()
    return Ok(value)
}

fn outer() Int -> {
    value :: middle()
    return Ok(value)
}

fn run() {
    outer()
}
"#;

    let (status, stdout, stderr) =
        common::build_and_run("jet_error_conversion", "names_with_to", source);
    assert_eq!(status, 1, "expected converted error, got: {stderr}");
    assert!(stdout.is_empty(), "unexpected stdout: {stdout}");
    assert!(
        stderr.contains("type: TargetErr"),
        "converted error lost its source family: {stderr}"
    );
    assert!(
        stderr.contains("conversion: TargetErr -> Err"),
        "converted error lost its conversion trail: {stderr}"
    );
}

#[test]
fn foreign_source_may_target_default_error() {
    let source = r#"
impl String -> Err {
    return Err(self)
}

fn run() {}
"#;
    jet::compile(source).expect("foreign String source may convert into default Err");
}

#[test]
fn declared_conversion_reaches_typed_error() {
    let source = r#"
#Error
enum SourceErr { One }
#Error
enum TargetErr { One }

impl SourceErr -> TargetErr {
    return TargetErr.One
}

fn read() Int !SourceErr -> {
    return Err(SourceErr.One)
}

fn outer() Int !TargetErr -> {
    value :: read()
    return Ok(value)
}

fn run() !TargetErr {
    outer()
}
"#;
    let compiled = jet::compile(source).expect("declared typed conversion must compile");
    assert!(compiled.rust.contains("errconv_SourceErr_to_TargetErr"));
}

#[test]
fn missing_typed_conversion_reports_e2404() {
    let source = r#"
#Error
enum SourceErr { One }
#Error
enum TargetErr { One }

fn read() Int !SourceErr -> {
    return Err(SourceErr.One)
}

fn run() Int !TargetErr -> {
    value :: read()
    return Ok(value)
}
"#;
    let diagnostics = jet::compile(source).expect_err("missing typed conversion must be rejected");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E2404"),
        "expected E2404, got {diagnostics:?}"
    );
}

#[test]
fn missing_typed_conversion_preserves_helper_provenance() {
    let source = include_str!("ui/e2392_linked_cascade.jet");
    let diagnostics =
        jet::compile(source).expect_err("the linked conversion fixture must be rejected");
    let e2404 = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E2404")
        .expect("linked conversion fixture must contain E2404");
    let detail = e2404
        .detail
        .as_deref()
        .expect("E2404 must expose provenance detail");
    for expected in [
        "failure-domain:load_row|SourceErr|TargetErr",
        "callee: load_row",
        "effective contract: explicit !SourceErr",
        "caller contract: explicit !TargetErr",
    ] {
        assert!(
            detail.contains(expected),
            "E2404 provenance missing {expected:?}: {detail}"
        );
    }
}

#[test]
fn conversion_cascade_links_same_span_root_and_orders_it_first() {
    let source = include_str!("ui/e2392_linked_cascade.jet");
    let diagnostics =
        jet::compile(source).expect_err("the linked conversion fixture must be rejected");
    let root = diagnostics
        .iter()
        .position(|diagnostic| diagnostic.code == "E0104")
        .expect("wrong-arity root must be retained");
    let dependent = diagnostics
        .iter()
        .position(|diagnostic| diagnostic.code == "E2404")
        .expect("conversion dependent must be retained");
    assert!(
        root < dependent,
        "root diagnostics must precede dependents: {diagnostics:?}"
    );
    let root_span = diagnostics[root].span;
    let causes = &diagnostics[dependent].cause;
    assert!(
        causes
            .iter()
            .any(|cause| cause.code == "E0104" && cause.span == root_span),
        "E2404 must link the same-span E0104 root: {diagnostics:?}"
    );

    let json = jet::render_all_json(
        &jet::Diagnostics::ReportPath::from_process("e2392_linked_cascade.jet"),
        source,
        &diagnostics,
    );
    let lines = json.lines().collect::<Vec<_>>();
    assert!(
        lines
            .iter()
            .any(|line| line.contains("\"code\":\"E0104\"")
                && line.contains("\"cause\":[],\"clears\":1")),
        "JSON must expose the root and its dependent count: {json}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("\"code\":\"E2404\"")
                && line.contains("\"cause\":[\"E0104\"]")),
        "JSON must expose the linked cause chain: {json}"
    );
}

#[test]
fn repeated_conversion_sites_dedupe_but_independent_roots_remain() {
    let source = include_str!("ui/e2392_root_cascade.jet");
    let diagnostics =
        jet::compile(source).expect_err("the root cascade fixture must be rejected");
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E2404")
        .collect::<Vec<_>>();
    assert_eq!(
        mismatches.len(),
        2,
        "five repeated sites must collapse while the independent domain remains: {diagnostics:?}"
    );
    assert!(
        mismatches[0]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("failure-domain:load_row|SourceErr|TargetErr")),
        "first retained root must be load_row's domain: {diagnostics:?}"
    );
    assert!(
        mismatches[1]
            .detail
            .as_deref()
            .is_some_and(|detail| {
                detail.starts_with(
                    "failure-domain:load_other|OtherSourceErr|OtherTargetErr"
                )
            }),
        "independent load_other root must remain: {diagnostics:?}"
    );
    assert!(
        mismatches.iter().all(|diagnostic| diagnostic.cause.is_empty()),
        "independent failure domains must not be linked to one another: {diagnostics:?}"
    );
}

#[test]
fn seeded_error_cascade_metric_keeps_one_first_root_per_domain() {
    let source = include_str!("ui/e2392_root_cascade.jet");
    let diagnostics =
        jet::compile(source).expect_err("the seeded cascade fixture must be rejected");
    assert_eq!(
        diagnostics.len(),
        2,
        "the seeded witness must emit only its two independent roots: {diagnostics:?}"
    );
    let emitted = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E2404")
        .count();
    assert_eq!(
        emitted, 2,
        "five load_row sites plus two independent load_other sites must emit one root per domain: {diagnostics:?}"
    );
    let seeded = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic
                .detail
                .as_deref()
                .is_some_and(|detail| detail.starts_with("failure-domain:load_row|"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        seeded.len(),
        1,
        "the seeded helper must emit one report for five repeated sites: {diagnostics:?}"
    );
    let first = diagnostics
        .first()
        .expect("the seeded cascade must emit a diagnostic");
    assert_eq!(first.code, "E2404");
    assert!(
        first.what.contains("from `load_row`"),
        "the first emitted report must name the seeded helper: {diagnostics:?}"
    );
    assert!(
        first.cause.is_empty(),
        "the first report must be a root: {diagnostics:?}"
    );
}

#[test]
fn fixing_the_named_helper_domain_clears_all_repeated_sites() {
    let source = r#"
#Error
enum SourceErr { One }
#Error
enum TargetErr { One }

fn fetch() Int !SourceErr -> {
    return Err(SourceErr.One)
}

fn run() Int !TargetErr -> {
    fetch()
    fetch()
    fetch()
    fetch()
    fetch()
    return Ok(0)
}
"#;
    let diagnostics = jet::compile(source).expect_err("the repeated domain must be rejected");
    assert_eq!(
        diagnostics.len(),
        1,
        "one seeded bad helper feeding five sites must emit one report: {diagnostics:?}"
    );
    let first = diagnostics.first().expect("the seeded report must exist");
    assert_eq!(first.code, "E2404");
    assert!(first.what.contains("from `fetch`"), "{first:?}");
    assert!(
        first.cause.is_empty(),
        "the seeded report must be root-first: {first:?}"
    );
    let mismatches = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E2404")
        .collect::<Vec<_>>();
    assert_eq!(mismatches.len(), 1, "one helper root must represent five sites");
    assert!(mismatches[0]
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("callee: fetch")));

    let fixed = source
        .replacen("fn fetch() Int !SourceErr", "fn fetch() Int !TargetErr", 1)
        .replacen("Err(SourceErr.One)", "Err(TargetErr.One)", 1);
    jet::compile(&fixed).expect("changing the named helper domain must clear every site");
}

#[test]
fn typed_foreign_target_keeps_the_orphan_rule() {
    let source = r#"
impl Int -> String {
    return "number"
}

fn run() {}
"#;
    let diagnostics = jet::compile(source).expect_err("foreign typed target must be rejected");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E2406"));
}

#[test]
fn declared_default_conversion_reaches_web_artifact() {
    let source = r#"
#Error
enum StoreErr { Missing }

impl StoreErr -> Err {
    return Err("store unavailable")
}

fn read_store() Int !StoreErr -> {
    return Err(StoreErr.Missing)
}

fn get_user() Int -> {
    value :: read_store()
    return Ok(value)
}

#Target(Wasm)
fn run() {
    value :: get_user()
}
"#;
    let output = jet::compile_web_with_path(source, "default_error_conversion_web.jet")
        .expect("declared default conversion must compile for web");
    let web = output.web.expect("web target must produce artifacts");
    assert!(web.wasm_rust.contains("store unavailable"));
    assert!(web.wasm_rust.contains("errconv_StoreErr_to_Err"));
}

#[test]
fn conversion_registration_does_not_depend_on_type_order() {
    let source = r#"
impl SourceErr -> TargetErr { return TargetErr.One }

enum SourceErr { One }
enum TargetErr { One }

fn run() {}
"#;
    jet::compile(source).expect("a local conversion may precede its type declarations");
}

#[test]
fn cyclic_error_conversions_report_e2418() {
    let source = r#"
enum SourceErr { One }
enum TargetErr { One }

impl SourceErr -> TargetErr { return TargetErr.One }
impl TargetErr -> SourceErr { return SourceErr.One }

fn run() {}
"#;
    let diagnostics = jet::compile(source).expect_err("cyclic conversions must be rejected");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E2418"));
}

#[test]
fn branched_error_conversions_report_e2419() {
    let source = r#"
enum RootErr { One }
enum LeftErr { One }
enum RightErr { One }
enum TargetErr { One }

impl RootErr -> LeftErr { return LeftErr.One }
impl RootErr -> RightErr { return RightErr.One }
impl LeftErr -> TargetErr { return TargetErr.One }
impl RightErr -> TargetErr { return TargetErr.One }

fn run() {}
"#;
    let diagnostics = jet::compile(source).expect_err("ambiguous conversions must be rejected");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "E2419"));
}
