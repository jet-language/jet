//! D-DSLBLOCK1: fixed stdlib DSL blocks are syntax islands with ordinary Jet
//! checking inside them.

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
