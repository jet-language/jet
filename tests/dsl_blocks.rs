//! D-META-DSL1=A: declared checked text blocks are syntax islands with
//! ordinary Jet checking inside them.

mod common;

#[test]
fn sql_and_html_blocks_compile_and_formatter_round_trip() {
    let source = r#"
    struct User { id: Int }
    fn run() {
    #SQL(User) {
        query :: SQL.{"select id from users"}
        print(query.template())
    }
    #HTML {
        page :: HTML.{"<p>ready</p>"}
        print(page.text())
    }
}
"#;
    let _compiled = jet::compile(source).expect("declared library blocks should compile");
    let formatted = jet::format_source(source).expect("DSL blocks should format");
    assert!(formatted.contains("#SQL(User)"));
    assert!(formatted.contains("#HTML {"));
    assert_eq!(
        jet::format_source(&formatted).unwrap(),
        formatted,
        "DSL formatter output must be stable"
    );
}

#[test]
fn dsl_block_body_keeps_normal_sema_and_registry_rules() {
    let body_error = jet::compile(
        "fn run() { #SQL { missing :: unknown_name() } }\n",
    )
    .unwrap_err();
    assert!(
        body_error.iter().any(|diagnostic| diagnostic.code != "E0617"),
        "DSL body should use ordinary sema diagnostics: {body_error:?}"
    );

    let foreign_marker = jet::compile("fn run() { #Graph { value :: 1 } }\n").unwrap_err();
    assert!(!foreign_marker.is_empty(), "undeclared block markers must be rejected");
}

#[test]
fn undeclared_checked_text_block_reports_one_registered_error() {
    let diagnostics = jet::compile("fn run() { #Graph { value :: 1 } }\n").unwrap_err();
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "E0927")
        .collect();
    assert_eq!(errors.len(), 1, "expected one E0927: {diagnostics:?}");
    let diagnostic = errors[0];
    assert!(!diagnostic.what.is_empty());
    assert!(!diagnostic.why.is_empty());
    assert!(!diagnostic.fix.is_empty());
}

#[test]
fn declared_block_receives_the_whole_nested_region() {
    let source = r#"
marker Check(@sites: [.Block]) {
    if !target.contains("after") {
        reject(
            code: "E0927",
            what: "the complete block text is required",
            why: "the checker reads one lexical region",
            fix: "keep the trailing statement inside the block"
        )
    }
}

fn run() {
    #Check {
        if true {
            print("inside")
        }
        print("after")
    }
}
"#;
    jet::compile(source).expect("a declared block must include nested and trailing text");
}

#[test]
fn typed_text_forms_cover_sql_html_sh_and_audited_raw() {
    let source = r#"
fn run() {
    id :: 7
    query :: SQL.{"select * from users where id = {id}"}
    _template :: query.template()
    _params :: query.params()
    name :: "<unsafe>"
    page :: HTML.{"<p>{name}</p>"}
    _text :: page.text()
    _sql :: SQL.raw("select 1")
    _html :: HTML.raw("<b>audited</b>")
    command :: Sh.{"printf <%s> {name}"}
}
"#;
    jet::compile(source).expect("typed domain text should compile through one checked path");
}

#[test]
fn declared_text_head_owns_validation_holes_and_raw_escape() {
    let source = r#"
use core.regex as re

marker Pattern on [.Text] {
    check re.is_match(@body.replace("{{}}", "x"), "")?
    hole re.escape(@value)
}

fn take(pattern: Pattern) {}

fn run() {
    value :: "a.b"
    pattern :: Pattern.{"a-{value}"}
    take(pattern)
    trusted :: Pattern.raw("already checked")
    take(trusted)
}
"#;
    jet::compile(source).expect("a declared text head should own its checked construction path");
    let formatted = jet::format_source(source).expect("checked text heads should format");
    assert!(formatted.contains("marker Pattern on [.Text]"));
    assert_eq!(jet::format_source(&formatted).unwrap(), formatted);

    let invalid = r#"
marker Pattern on [.Text] {
    check @body
    hole @value
}

fn take(pattern: Pattern) {}

fn run() {
    raw :: "untrusted"
    take(raw)
}
"#;
    let diagnostics = jet::compile(invalid).expect_err("a bare String must not reach a text sink");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0149"),
        "custom text sinks should use the checked-text mismatch: {diagnostics:?}"
    );
}

#[test]
fn boundary_typed_heads_validate_and_compile() {
    let source = r#"
fn run() {
    service :: "ada/../etc"
    endpoint :: URL.{"https://api.example.com/v2/{service}"}
    log_path :: Path.{"/var/log/{service}.log"}
    stamp :: DateTime.{"2026-08-07T12:00:00Z"}
}
"#;
    jet::compile(source).expect("URL, Path, and DateTime heads should compile");

    let invalid_url = jet::compile(
        r#"fn run() {
    bad :: URL.{"https://[bad"}
}
"#,
    )
    .expect_err("an invalid URL head must fail in sema");
    assert!(
        invalid_url.iter().any(|diagnostic| diagnostic.code == "E0155"),
        "invalid URL head should use E0155: {invalid_url:?}"
    );

    let datetime_hole = jet::compile(
        r#"fn run() {
    hour :: "12"
    bad :: DateTime.{"2026-08-07T{hour}:00:00Z"}
}
"#,
    )
    .expect_err("DateTime heads must reject interpolation");
    assert!(
        datetime_hole
            .iter()
            .any(|diagnostic| diagnostic.code == "E0155"),
        "DateTime interpolation should use E0155: {datetime_hole:?}"
    );
}

#[test]
fn sql_row_header_is_a_real_declared_type_position() {
    let error = jet::compile("fn run() { #SQL(MissingRow) {} }\n").unwrap_err();
    assert!(
        error.iter().any(|diagnostic| {
            diagnostic.what.contains("MissingRow")
                || diagnostic.fix.contains("MissingRow")
        }),
        "unknown SQL row type should use ordinary declared-type diagnostics: {error:?}"
    );
}
