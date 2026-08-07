//! D-FAIL-CARRIER1=A / D-FAIL-MODEL1=A: one outcome carrier under `T?` and
//! `T ? E`. An outcome has a payload, a verdict and reports. `T?` is the view
//! whose report is the clean absence; `T ? E` is the view whose report matters.

use std::fs;

fn compile(name: &str, src: &str) -> String {
    let dir = std::env::temp_dir().join("jet_outcome_carrier");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.jet"));
    fs::write(&file, src).unwrap();
    let out = jet::compile_with_path(src, file.to_str().unwrap())
        .unwrap_or_else(|diags| panic!("{name} failed the front end: {diags:?}"));
    out.rust
}

fn user_body(rust: &str, function: &str) -> String {
    let head = format!("pub fn {function}(");
    let start = rust
        .find(&head)
        .unwrap_or_else(|| panic!("no `{function}` in generated Rust"));
    let rest = &rust[start..];
    let end = rest.find("\n}\n").expect("unterminated function") + 3;
    rest[..end].to_string()
}

/// Both ratified spellings lower onto the one carrier, and neither leaves a
/// second representation behind.
#[test]
fn both_views_lower_onto_one_carrier() {
    let rust = compile(
        "views",
        r#"
fn lookup(id: Int) => Int? {
    if id == 1 { return Val(9) }
    return None
}

fn parse(raw: String) => Int ? Error {
    if raw == "" { return Err("empty") }
    return Ok(7)
}

fn run() {
    print(lookup(1) ?? 0)
    print(parse("x") ?? 0)
}
"#,
    );
    let lookup = user_body(&rust, "user_lookup");
    let parse = user_body(&rust, "user_parse");

    assert!(
        lookup.contains("JetOutcome<i64, JetAbsent>"),
        "`T?` must lower onto the carrier:\n{lookup}"
    );
    assert!(
        parse.contains("JetOutcome<i64, String>"),
        "`T ? E` must lower onto the same carrier:\n{parse}"
    );
    assert!(
        !lookup.contains("Option<"),
        "no second representation may survive for `T?`:\n{lookup}"
    );
    // One carrier means one payload spelling and one report spelling, so the
    // optional view builds `Ok`/`Err` exactly like the fallible view.
    assert!(
        lookup.contains("Ok(9i64)") && lookup.contains("Err(JetAbsent)"),
        "the optional view builds the carrier:\n{lookup}"
    );

    let run = user_body(&rust, "user_run");
    assert!(
        !run.contains("Some(") && !run.contains("None"),
        "`??` reads both views the same way:\n{run}"
    );
}

/// The verdict erases from the happy path: reading a `T?` costs no allocation
/// and no branch beyond the one the payload already needs.
#[test]
fn an_unread_verdict_costs_nothing() {
    // The carrier's optional view, exactly as `Prelude/Outcome.rs` declares it.
    struct JetAbsent;
    type JetOutcome<T, E> = Result<T, E>;

    assert_eq!(
        std::mem::size_of::<JetOutcome<i64, JetAbsent>>(),
        std::mem::size_of::<Option<i64>>(),
        "a clean report is zero-sized, so `T?` carries no verdict word"
    );
    assert_eq!(
        std::mem::size_of::<JetOutcome<String, JetAbsent>>(),
        std::mem::size_of::<String>(),
        "the payload's niche still holds the absence"
    );

    let rust = compile(
        "erasure",
        r#"
fn first_even(n: Int) => Int? {
    if n % 2 == 0 {
        return Val(n)
    }
    return None
}

fn run() {
    print(first_even(4) ?? -1)
}
"#,
    );
    let body = user_body(&rust, "user_first_even");
    let run = user_body(&rust, "user_run");
    for shape in ["Vec::", "vec!", "to_vec()", "String::new()"] {
        assert!(
            !body.contains(shape),
            "an unread verdict must allocate nothing, found `{shape}`:\n{body}"
        );
    }
    assert_eq!(
        run.matches("match ").count(),
        1,
        "reading the payload is one branch, not one per fact:\n{run}"
    );
}
