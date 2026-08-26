mod common;

use jet::compile;

const DECLS: &str = r#"
enum TextError { Bad }

Pattern :: distinct String

impl Pattern.CheckedText {
    type Error = TextError

    fn check(text: String) !TextError -[]> {
        if text == "" { return Err(TextError.Bad) }
        return Ok(())
    }

    fn encode_hole<T: Printable>(value: T) String -[]> {
        return "{value}"
    }
}
"#;

fn assert_plain_string_rejected(label: &str, body: &str) {
    let source = format!("{DECLS}\n{body}");
    let diagnostics = compile(&source).expect_err(label);
    assert!(
        diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.code.as_str(),
                "E0112" | "E0108" | "E0113" | "E0905" | "E0702" | "E3203"
            )
        }),
        "{label} produced no checked-text type-boundary diagnostic: {diagnostics:#?}"
    );
}

#[test]
fn plain_string_cannot_cross_checked_text_boundaries() {
    assert_plain_string_rejected(
        "checked text parameter",
        r#"
fn take(value: Pattern) {}

fn run() {
    plain :: "plain"
    take(plain)
}
"#,
    );
    assert_plain_string_rejected(
        "checked text field",
        r#"
struct Envelope {
    value: Pattern
}

fn run() {
    plain :: "plain"
    envelope :: Envelope{ value: plain }
}
"#,
    );
    assert_plain_string_rejected(
        "checked text collection",
        r#"
fn take(values: [Pattern]) {}

fn run() {
    plain :: "plain"
    take([plain])
}
"#,
    );
    assert_plain_string_rejected(
        "checked text generic",
        r#"
struct Envelope<T> {
    value: T
}

fn run() {
    plain :: "plain"
    envelope :: Envelope<Pattern>{ value: plain }
}
"#,
    );
    assert_plain_string_rejected(
        "checked text return",
        r#"
fn make() Pattern {
    plain :: "plain"
    return plain
}

fn run() {}
"#,
    );
    assert_plain_string_rejected(
        "checked text trait method",
        r#"
trait Sink {
    fn put(self, value: Pattern)
}

struct Worker {}

impl Worker.Sink {
    fn put(self, value: Pattern) {}
}

fn run() {
    worker :: Worker{}
    plain :: "plain"
    worker.put(plain)
}
"#,
    );
    assert_plain_string_rejected(
        "checked text foreign boundary",
        r#"
use c.checked as c

#Extern module c.checked {
    fn put(value: Pattern) = "put"
}

fn run() {
    plain :: "plain"
    c.put(plain)
}
"#,
    );
}
