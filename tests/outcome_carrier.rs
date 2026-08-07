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

/// Run a program on the interpreter tier — the tier the corpus gate proves
/// agrees with AOT — so a claim about what a read *answers* is tested, not just
/// what it emits.
fn run_jet(name: &str, src: &str) -> String {
    let dir = std::env::temp_dir().join("jet_outcome_carrier");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.jet"));
    fs::write(&file, src).unwrap();

    // The front end and the evaluator both recurse through the program, so run
    // them with the same room the other interpreter tests give them rather than
    // the default test stack.
    let (name, src) = (name.to_string(), src.to_string());
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let path = file.to_str().unwrap();
            let mut bundle = jet::Loader::load_entry(path).expect("fixture should load");
            let diags = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run);
            assert!(
                !diags
                    .iter()
                    .any(|d| matches!(d.severity, jet::Diagnostics::Severity::Error)),
                "{name} failed the front end:\n{}",
                jet::render_diagnostics(path, &src, &diags)
            );
            let program = jet::Codegen::TIR::lower_jit_program(&bundle)
                .unwrap_or_else(|| panic!("{name} must lower for the interpreter"));
            let mut sink = jet::Comptime::DevSink::default();
            jet::Codegen::TIR::run_named_func(&program, "run", Vec::new(), &mut sink)
                .unwrap_or_else(|diag| panic!("{name} failed on the interpreter: {diag:?}"));
            sink.stdout
        })
        .expect("spawn the interpreter thread")
        .join()
        .expect("the interpreter thread must finish")
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

/// The carrier's middle states: a failure that kept part of its work, and a
/// failure that had something to say. Both live on the outcome value, so two
/// outcomes alive at once never share a fact and reading one twice answers the
/// same thing both times.
#[test]
fn the_middle_states_read_off_the_same_carrier() {
    let rust = compile(
        "middle_states",
        r#"
struct ImportErr {
    broken: Int
    partial: [String]
    notes: [String]
}

fn import_rows(label: String, rows: [String]) => [String] ? ImportErr {
    good :: rows.filter((row) => row != "")
    broken :: rows.len() - good.len()
    if broken > 0 {
        return Err(ImportErr.{ broken: broken, partial: good, notes: [~label] })
    }
    return Ok(good)
}

fn run() {
    people :: import_rows("people", ["ada", "", "alan"])
    ports :: import_rows("ports", ["80", ""])
    print(people.notes().join(""))
    print(ports.notes().join(""))
    print(people.notes().join(""))
    print((people.partial() ?? []).len())
}
"#,
    );
    let run = user_body(&rust, "user_run");
    assert!(
        run.contains("jet_partial(&(") && run.contains("__jet_report.user_partial.clone()"),
        "`.partial` must marshal onto the prelude's `jet_partial`:\n{run}"
    );
    assert!(
        run.contains("jet_notes(&(") && run.contains("__jet_report.user_notes.clone()"),
        "`.notes` must marshal onto the prelude's `jet_notes`:\n{run}"
    );
    // The middle states are the same carrier, so no third type appears.
    assert!(
        !run.contains("Partial<") && !run.contains("Noted<"),
        "the middle states need no third type:\n{run}"
    );

    // And both facts really are read off the value: two outcomes alive at once
    // answer differently, and the second read of one answers what the first
    // did. Nothing here can pass if a fact is kept beside the value.
    let out = run_jet(
        "middle_states_run",
        r#"
struct ImportErr {
    broken: Int
    partial: [String]
    notes: [String]
}

fn import_rows(label: String, rows: [String]) => [String] ? ImportErr {
    good :: rows.filter((row) => row != "")
    broken :: rows.len() - good.len()
    if broken > 0 {
        return Err(ImportErr.{ broken: broken, partial: good, notes: [~label] })
    }
    return Ok(good)
}

fn run() {
    people :: import_rows("people", ["ada", "", "alan"])
    ports :: import_rows("ports", ["80", ""])
    clean :: import_rows("clean", ["ada"])
    print(people.notes().join(""))
    print(ports.notes().join(""))
    print(people.notes().join(""))
    print((people.partial() ?? []).join(","))
    print((ports.partial() ?? []).join(","))
    print(clean.notes().len())
    print((clean.partial() ?? ["kept nothing"]).join(","))
}
"#,
    );
    assert_eq!(
        out,
        "people\nports\npeople\nada,alan\n80\n0\nkept nothing\n",
        "two outcomes must not share a fact, and a second read must answer the first"
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
    // The carrier itself, not a copy of it: these are the very types the
    // prelude embeds, so adding a field to either one fails this test.
    use jet::Outcome::{JetAbsent, JetOutcome};

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
