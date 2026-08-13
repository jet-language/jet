//! D-FAIL-CONV1: one declared conversion rail, including the default Err target.

#[test]
fn declared_conversion_reaches_default_error() {
    let source = r#"
enum StoreErr { Missing }

impl StoreErr => Err {
    return Err("missing")
}

fn read_store() => Int ? StoreErr {
    return Err(StoreErr.Missing)
}

fn get_user() => Int ? {
    return Ok(read_store()?)
}

fn run() ? {
    get_user()?
}
"#;
    let compiled = jet::compile(source).expect("declared StoreErr => Err must compile");
    assert!(compiled.rust.contains("errconv_StoreErr_to_Err"));
}

#[test]
fn foreign_source_may_target_default_error() {
    let source = r#"
impl String => Err {
    return Err(self)
}

fn read_store() => Int ? String {
    return Err("missing")
}

fn run() ? {
    value :: read_store()?
    print(value)
}
"#;
    jet::compile(source).expect("foreign String source may convert into default Err");
}

#[test]
fn typed_foreign_target_keeps_the_orphan_rule() {
    let source = r#"
impl Int => String {
    return "number"
}

fn run() {}
"#;
    let diagnostics = jet::compile(source).expect_err("foreign typed target must be rejected");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E2406"));
}

#[test]
fn declared_default_conversion_reaches_web_artifact() {
    let source = r#"
enum StoreErr { Missing }

impl StoreErr => Err {
    return Err("store unavailable")
}

fn read_store() => Int ? StoreErr {
    return Err(StoreErr.Missing)
}

fn get_user() => Int ? {
    value :: read_store()?
    return Ok(value)
}

#Target(Wasm)
fn run() {
    result :: get_user()
    if result == {
        .Ok(value) -> { print(value) }
        .Err(error) -> { print(error.message) }
        else -> {}
    }
}
"#;
    let output = jet::compile_web_with_path(source, "default_error_conversion_web.jet")
        .expect("declared default conversion must compile for web");
    let web = output.web.expect("web target must produce artifacts");
    assert!(web.wasm_rust.contains("store unavailable"));
    assert!(web.wasm_rust.contains("errconv_StoreErr_to_Err"));
}
