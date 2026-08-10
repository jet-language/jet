//! D-DSLBLOCK1: fixed stdlib DSL blocks are syntax islands with ordinary Jet
//! checking inside them.

mod common;

#[test]
fn sql_and_html_blocks_compile_and_formatter_round_trip() {
    let source = r#"
    struct User { id: Int }
    fn run() {
    #SQL<User> {
        query :: SQL.{"select id from users"}
        print(query.template())
    }
    #HTML {
        page :: HTML.{"<p>ready</p>"}
        print(page.text())
    }
}
"#;
    let _compiled = jet::compile(source).expect("stdlib DSL blocks should compile");
    let formatted = jet::format_source(source).expect("DSL blocks should format");
    assert!(formatted.contains("#SQL<User>"));
    assert!(formatted.contains("#HTML {"));
    assert_eq!(
        jet::format_source(&formatted).unwrap(),
        formatted,
        "DSL formatter output must be stable"
    );
}

#[test]
fn dsl_block_body_keeps_normal_sema_and_whitelist_rules() {
    let body_error = jet::compile(
        "fn run() { #SQL { missing :: unknown_name() } }\n",
    )
    .unwrap_err();
    assert!(
        body_error.iter().any(|diagnostic| diagnostic.code != "E0617"),
        "DSL body should use ordinary sema diagnostics: {body_error:?}"
    );

    let foreign_marker = jet::compile("fn run() { #Graph { value :: 1 } }\n").unwrap_err();
    assert!(!foreign_marker.is_empty(), "third-party DSL markers must not parse as a DSL");
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
    let error = jet::compile("fn run() { #SQL<MissingRow> {} }\n").unwrap_err();
    assert!(
        error.iter().any(|diagnostic| {
            diagnostic.what.contains("MissingRow")
                || diagnostic.fix.contains("MissingRow")
        }),
        "unknown SQL row type should use ordinary declared-type diagnostics: {error:?}"
    );
}
