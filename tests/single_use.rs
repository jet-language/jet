//! D-LIN1 (ratified 2026-06-21): single-use (must-consume) values, `#SingleUse`.
//!
//! A type marked `#SingleUse` must be consumed exactly once on every reachable
//! path — moved to a `^` parameter or returned. Dropping it without consuming is
//! E0140 (E0141 when only one `if` branch consumes it); using it twice is E0121
//! (the move tracker); lending it (`&`/read) instead of moving is E0142. The tag
//! erases in codegen (I3) — no runtime value, no `unsafe`.

mod common;

const LOCK: &str = r#"
#SingleUse struct Lock {
    resource: String,
}
fn acquire(resource: String) -> Lock {
    return Lock.{ resource: resource }
}
fn release(lock: ^Lock) {
    print(lock.resource)
}
"#;

fn err_codes(src: &str) -> Vec<String> {
    let full = format!("{}\n{}", LOCK, src);
    codes_of(&full)
}

fn codes_of(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.into_iter().map(|d| d.code.to_string()).collect(),
    }
}

/// Consumed exactly once (moved to a `^` parameter) — compiles cleanly.
#[test]
fn consumed_once_compiles() {
    let codes = err_codes(
        r#"
fn run() {
    db :: acquire("db")
    release(^db)
}
"#,
    );
    assert!(codes.is_empty(), "expected clean compile, got {:?}", codes);
}

/// Returning the value also consumes it (a legal exit per the spec).
#[test]
fn returned_value_is_consumed() {
    let codes = err_codes(
        r#"
fn make() -> Lock {
    db :: acquire("db")
    return db
}
fn run() {
    held :: make()
    release(^held)
}
"#,
    );
    assert!(codes.is_empty(), "return should consume, got {:?}", codes);
}

/// The consume duty travels through a `^`-in / `-> T`-out pass-through.
#[test]
fn passthrough_moves_the_duty() {
    let codes = err_codes(
        r#"
fn hold(lock: ^Lock) -> Lock {
    return lock
}
fn run() {
    db :: acquire("db")
    held :: hold(^db)
    release(^held)
}
"#,
    );
    assert!(
        codes.is_empty(),
        "passthrough should be clean, got {:?}",
        codes
    );
}

/// Dropped without consuming → E0140.
#[test]
fn dropped_is_e0140() {
    let codes = err_codes(
        r#"
fn run() {
    db :: acquire("db")
}
"#,
    );
    assert!(
        codes.contains(&"E0140".to_string()),
        "expected E0140, got {:?}",
        codes
    );
}

/// Consumed on only one `if` branch → E0141.
#[test]
fn one_branch_is_e0141() {
    let codes = err_codes(
        r#"
fn run() {
    db :: acquire("db")
    early :: true
    if early {
        release(^db)
    }
}
"#,
    );
    assert!(
        codes.contains(&"E0141".to_string()),
        "expected E0141, got {:?}",
        codes
    );
    // It is NOT also flagged E0140 — the divergence is the single, precise error.
    assert!(
        !codes.contains(&"E0140".to_string()),
        "E0141 should not double-report as E0140: {:?}",
        codes
    );
}

/// Consumed on BOTH branches → clean.
#[test]
fn both_branches_consume_is_clean() {
    let codes = err_codes(
        r#"
fn run() {
    db :: acquire("db")
    early :: true
    if early {
        release(^db)
    } else {
        release(^db)
    }
}
"#,
    );
    assert!(
        codes.is_empty(),
        "both-branch consume should be clean, got {:?}",
        codes
    );
}

/// Used twice → E0121 (the move tracker catches use-after-move).
#[test]
fn used_twice_is_e0121() {
    let codes = err_codes(
        r#"
fn run() {
    db :: acquire("db")
    release(^db)
    release(^db)
}
"#,
    );
    assert!(
        codes.contains(&"E0121".to_string()),
        "expected E0121, got {:?}",
        codes
    );
}

/// Lent to a read/borrow parameter instead of moved → E0142 (no aliasing).
#[test]
fn aliased_is_e0142() {
    let codes = err_codes(
        r#"
fn peek(lock: Lock) {
    print(lock.resource)
}
fn run() {
    db :: acquire("db")
    peek(db)
    release(^db)
}
"#,
    );
    assert!(
        codes.contains(&"E0142".to_string()),
        "expected E0142, got {:?}",
        codes
    );
}

// --- D-LIN1-DROP (ratified 2026-06-25): the audited deliberate-discard hatch ---

/// `consume(x)` inside an `#Unsafe("reason") { … }` block counts as terminal
/// consumption — the duty is discharged, so the program compiles cleanly.
#[test]
fn drop_inside_unsafe_block_satisfies_single_use() {
    let codes = err_codes(
        r#"
fn run() {
    db :: acquire("db")
    #Unsafe("the resource is already gone; nothing to release") {
        consume(db)
    }
}
"#,
    );
    assert!(
        codes.is_empty(),
        "audited drop should compile, got {:?}",
        codes
    );
}

/// `consume(x)` inside an `#Unsafe fn` body is equally audited.
/// the audit note, so a `#SingleUse` local it owns may be discarded with `drop`.
#[test]
fn drop_inside_unsafe_fn_satisfies_single_use() {
    let codes = err_codes(
        r#"
#Unsafe("voids a freshly-acquired lock whose resource is already gone")
fn void_one() {
    db :: acquire("db")
    consume(db)
}
fn run() {
    #Unsafe("calling the audited voider") {
        void_one()
    }
}
"#,
    );
    assert!(
        codes.is_empty(),
        "drop in #Unsafe fn should compile, got {:?}",
        codes
    );
}

/// `consume(x)` outside `#Unsafe` → E0143,
/// telling the user to wrap it in `#Unsafe("reason")`.
#[test]
fn drop_outside_unsafe_is_e0143() {
    let codes = err_codes(
        r#"
fn run() {
    db :: acquire("db")
    consume(db)
}
"#,
    );
    assert!(
        codes.contains(&"E0143".to_string()),
        "expected E0143, got {:?}",
        codes
    );
    // E0143 is the single, precise error — not buried under a cascade E0140.
    assert!(
        !codes.contains(&"E0140".to_string()),
        "E0143 should not also report E0140: {:?}",
        codes
    );
}

/// After an audited `drop`, the value is gone: reusing it is E0121 (use-after-move).
#[test]
fn reuse_after_drop_is_e0121() {
    let codes = err_codes(
        r#"
fn run() {
    db :: acquire("db")
    #Unsafe("done with it") {
        consume(db)
    }
    release(^db)
}
"#,
    );
    assert!(
        codes.contains(&"E0121".to_string()),
        "expected E0121 after drop, got {:?}",
        codes
    );
}

/// A user-defined `drop` function shadows the builtin (it is not reserved).
#[test]
fn user_drop_fn_shadows_builtin() {
    let codes = err_codes(
        r#"
fn drop(n: Int) -> Int {
    return n + 1
}
fn run() {
    db :: acquire("db")
    release(^db)
    x :: drop(41)
    print("{x}")
}
"#,
    );
    assert!(
        codes.is_empty(),
        "user `drop` fn should shadow the builtin, got {:?}",
        codes
    );
}

/// `consume` erases to Rust `drop(...)` — no `unsafe` in generated code.
#[test]
fn drop_erases_no_unsafe_in_codegen() {
    let src = format!(
        "{}\nfn run() {{ db :: acquire(\"db\"); #Unsafe(\"gone\") {{ consume(db) }} }}\n",
        LOCK
    );
    let out = jet::compile(&src).expect("should compile");
    assert!(
        out.rust.contains("drop("),
        "consume should lower to a Rust drop(...) call"
    );
}

/// A `#SingleUse` enum gets the same treatment as a struct.
#[test]
fn single_use_enum_dropped_is_e0140() {
    let src = r#"
#SingleUse enum Ticket {
    Open
    Closed
}
fn make() -> Ticket {
    return Ticket.Open
}
fn close(t: ^Ticket) {
    print("closed")
}
fn run() {
    t :: make()
}
"#;
    let codes = codes_of(src);
    assert!(
        codes.contains(&"E0140".to_string()),
        "expected E0140 for enum, got {:?}",
        codes
    );
}

/// The `#SingleUse` tag erases in codegen: the generated Rust is a plain struct,
/// with no marker artifact and no `unsafe` (I3 / I1).
#[test]
fn tag_erases_in_codegen() {
    let src = format!(
        "{}\nfn run() {{ db :: acquire(\"db\"); release(^db) }}\n",
        LOCK
    );
    let out = jet::compile(&src).expect("should compile");
    assert!(
        !common::strip_vetted_prelude_modules(&out.rust).contains("unsafe"),
        "I1: no unsafe in generated code"
    );
    assert!(
        !out.rust.contains("SingleUse"),
        "tag must erase, found SingleUse in output"
    );
    assert!(
        out.rust.contains("struct user_Lock"),
        "Lock should lower to a plain struct"
    );
}
