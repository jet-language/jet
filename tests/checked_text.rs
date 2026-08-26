mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use jet::compile;

const SOURCE: &str = r#"
enum TextError { Bad }

Pattern :: distinct String

impl Pattern.CheckedText {
    type Error = TextError

    fn check(text: String) () TextError! -[]> {
        if text == "" { return Err(TextError.Bad) }
        return Ok(())
    }

    fn encode_hole<T: Printable>(value: T) String -[]> {
        return "{value}"
    }
}

fn run() {
    value :: Pattern{"hello"}
    print("{value.raw()}")
}
"#;

const TIERS_SOURCE: &str = r#"
enum TextError { Bad }

Pattern :: distinct String

impl Pattern.CheckedText {
    type Error = TextError

    fn check(text: String) () TextError! -[]> {
        if text == "" { return Err(TextError.Bad) }
        return Ok(())
    }

    fn encode_hole<T: Printable>(value: T) String -[]> {
        return "[{value}]"
    }
}

fn run() {
    name :: "world"
    literal :: Pattern{"hello {name}"}
    print(literal.raw())

    good :: Pattern.from("ok")
    good ? value -> print(value.raw()) ! error -> print("good rejected")

    bad :: Pattern.from("")
    bad ? value -> print("bad accepted") ! error -> print("bad rejected")
}
"#;

#[test]
fn ordinary_checked_text_source_compiles() {
    let output = compile(SOURCE).expect("ordinary CheckedText source should compile");
    assert!(output.rust.contains("__jet_CheckedText"));
    assert!(output.rust.contains("jet_checked_text_from"));
}

#[test]
fn malformed_checked_text_impl_reports_the_trait_contract() {
    let source = r#"
Pattern :: distinct String

impl Pattern.CheckedText {
    fn check(text: String) () Error! -[]> { return Ok(()) }
    fn encode_hole<T: Printable>(value: T) String -[]> { return "{value}" }
}

fn run() {}
"#;
    let diagnostics = compile(source).expect_err("missing associated Error must be rejected");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0913"));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0907"));
}

#[test]
fn checked_text_raw_requires_unsafe() {
    let source = r#"
Pattern :: distinct String

impl Pattern.CheckedText {
    type Error = Error
    fn check(text: String) () Error! -[]> { return Ok(()) }
    fn encode_hole<T: Printable>(value: T) String -[]> { return "{value}" }
}

fn run() {
    value :: Pattern.raw("already checked")
    print(value.raw())
}
"#;
    let diagnostics = compile(source).expect_err("raw construction must require unsafe");
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0387"));
}

#[test]
fn checked_text_agrees_across_jit_interpreter_and_aot() {
    tir_support::assert_tiers_agree(
        "checked_text_tiers",
        TIERS_SOURCE,
        "hello [world]\nok\nbad rejected\n",
    );
}

#[test]
fn checked_text_web_source_compiles() {
    let output = jet::compile_web_with_path(TIERS_SOURCE, "checked_text_web.jet")
        .expect("ordinary CheckedText source should compile for web");
    assert!(output.web.is_some());
}
