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
fn accept(value: Pattern) {}

fn run() {
    plain :: "plain"
    accept(plain)
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
fn accept(values: [Pattern]) {}

fn run() {
    plain :: "plain"
    accept([plain])
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
fn make() Pattern -> {
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

#Import module c.checked {
    fn put(value: Pattern) = "put"
}

fn run() {
    plain :: "plain"
    c.put(plain)
}
"#,
    );
}

#[test]
fn email_html_body_rejects_plain_string() {
    let diagnostics = compile(
        r#"
use core.email as email

fn run() {
    sender :: email.address("sender@example.com") ?? panic("sender")
    email.message(sender, [sender], [Address]{}, "subject", "body", "<b>unsafe</b>", [Attachment]{})
}
"#,
    )
    .expect_err("email HTML body accepted a plain String");

    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0149"),
        "email HTML body produced no checked-text diagnostic: {diagnostics:#?}"
    );
}
