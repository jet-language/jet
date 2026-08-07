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

/// `.or_err("why")` moves an outcome from the optional view to the fallible
/// one. The payload rides through untouched, so nothing converts.
#[test]
fn or_err_lifts_a_clean_absence_into_a_failure() {
    let rust = compile(
        "or_err",
        r#"
fn birth_year(book: [String: String], name: String) => String ? Error {
    return book.get(name).or_err("nobody in the book is called that")
}

fn run() {
    print(birth_year(["ada": "1815"], "ada") ?? "unknown")
}
"#,
    );
    let body = user_body(&rust, "user_birth_year");
    assert!(
        body.contains(".or_err(\"nobody in the book is called that\""),
        "`.or_err` must reach the prelude's one meaning:\n{body}"
    );
    assert!(
        !body.contains("ok_or") && !body.contains("Some(") && !body.contains("None"),
        "the two views are one carrier, so nothing converts between them:\n{body}"
    );
}

/// The carrier's middle states: a failure that kept part of its work, and an
/// outcome that collected a note. Both read off the outcome without unwrapping.
#[test]
fn the_middle_states_read_off_the_same_carrier() {
    let rust = compile(
        "middle_states",
        r#"
struct ImportErr {
    broken: Int
    partial: [String]
}

fn import_rows(rows: [String]) => [String] ? ImportErr {
    good :: rows.filter((row) => row != "")
    broken :: rows.len() - good.len()
    if broken > 0 {
        return Err(ImportErr.{ broken: broken, partial: good })
    }
    return Ok(good)
}

fn run() {
    spotty :: import_rows(["ada", "", "alan"]).noting("one row was empty")
    print((spotty.partial() ?? []).len())
    print(spotty.notes().len())
}
"#,
    );
    let run = user_body(&rust, "user_run");
    assert!(
        run.contains("jet_partial(&(")
            && run.contains("__jet_report.user_partial.clone()"),
        "`.partial` must marshal onto the prelude's `jet_partial`:\n{run}"
    );
    assert!(
        run.contains("jet_noting(") && run.contains("jet_notes(&("),
        "notes must ride the prelude's journey, not a second one:\n{run}"
    );
    // The middle states are the same carrier, so no third type appears.
    assert!(
        !run.contains("Partial<") && !run.contains("Noted<"),
        "the middle states need no third type:\n{run}"
    );
}

/// The web tier names the same carrier. The wasm module includes the very same
/// `Outcome.rs` file the native prelude puts first — not a copy, and not a
/// second representation — so an outcome means one thing on every tier.
#[test]
fn the_web_tier_reads_the_same_carrier() {
    let src = r#"
#WasmExport
fn double(n: Int) => Int = n * 2

#Target(JS)
fn run() {
    print(double(4))
}
"#;
    let out = jet::compile_web_with_path(src, "main.jet")
        .unwrap_or_else(|diags| panic!("web fixture rejected: {diags:?}"));
    let web = out.web.expect("web target must produce artifacts");

    assert!(
        web.wasm_rust
            .contains("pub type JetOutcome<T, E> = Result<T, E>;")
            && web.wasm_rust.contains("pub struct JetAbsent;"),
        "the wasm module must include the one carrier file:\n{}",
        web.wasm_rust
    );

    // And the web emitter spells outcomes with that carrier, so a `T?` that
    // reaches the wasm module can only be the same thing every other tier
    // carries.
    let emitter = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/crates/jet-codegen/src/Codegen/Web.rs"
    ))
    .expect("read the web emitter");
    assert!(
        emitter.contains("JetOutcome<{}, JetAbsent>") && emitter.contains("Err(JetAbsent)"),
        "the web emitter must build the one carrier"
    );
    assert!(
        !emitter.contains("format!(\"Option<{}>\""),
        "no second representation may survive on the web tier"
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
