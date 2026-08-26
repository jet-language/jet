//! D-FAIL-CONV1: one declared conversion rail, including the default Err target.

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
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E2404"),
        "expected E2404, got {diagnostics:?}"
    );
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
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E2418"));
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
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E2419"));
}
