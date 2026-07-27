//! Taint tracking tests (D-TAINT1): `#Tainted` value-fact propagation, the
//! `#Sanitizer fn` taint-strip contract, and the tainted→sink error E0721. Taint
//! is a compile-time proof, erased in codegen (I3).

fn codes(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code.clone()).collect(),
    }
}

/// A tainted value reaching an `Exec` sink (`process.run`) directly is E0721.
#[test]
fn tainted_to_exec_sink_is_error() {
    let src = r#"
use core.process as process
fn run() {
    name :: #Tainted "world; rm -rf /"
    process.run(["echo", name]) ?? return
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0721"),
        "direct tainted→sink must be E0721"
    );
}

/// Routing the tainted value through a `#Sanitizer fn` clears taint — the sink
/// call is then accepted.
#[test]
fn sanitized_value_reaches_sink_ok() {
    let src = r#"
use core.process as process
#Sanitizer fn clean(raw: String) => String { return raw.split(" ")[0] }
fn run() {
    name :: #Tainted "world; rm -rf /"
    safe := clean(name)
    process.run(["echo", safe]) ?? return
}
"#;
    assert!(
        codes(src).is_empty(),
        "sanitized value at sink must compile: {:?}",
        codes(src)
    );
}

/// Taint spreads through a binding: a value derived from a tainted value (here a
/// reassignment) is itself tainted and still trips the sink check.
#[test]
fn taint_propagates_through_binding() {
    let src = r#"
use core.process as process
fn run() {
    raw :: #Tainted "evil"
    cmd := raw
    process.run(["echo", cmd]) ?? return
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0721"),
        "taint must propagate through a binding"
    );
}

/// Taint spreads through string interpolation: a tainted value spliced into a
/// string taints the result.
#[test]
fn taint_propagates_through_interpolation() {
    let src = r#"
use core.process as process
fn run() {
    user :: #Tainted "bob"
    arg := "hello {user}"
    process.run(["echo", arg]) ?? return
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0721"),
        "taint must propagate through interpolation"
    );
}

/// A reassignment to a clean value clears taint — the local is no longer tainted.
#[test]
fn reassign_to_clean_clears_taint() {
    let src = r#"
use core.process as process
fn run() {
    x := #Tainted "evil"
    x = "safe-literal"
    process.run(["echo", x]) ?? return
}
"#;
    assert!(
        codes(src).is_empty(),
        "reassign to a clean value must clear taint: {:?}",
        codes(src)
    );
}

/// An ordinary (untainted) value at a sink is fine — no false positive.
#[test]
fn clean_value_at_sink_ok() {
    let src = r#"
use core.process as process
fn run() {
    process.run(["echo", "hello"]) ?? return
}
"#;
    assert!(
        codes(src).is_empty(),
        "clean value at sink must compile: {:?}",
        codes(src)
    );
}

/// A tainted value at a non-sink call (an ordinary `print`, an `Io` effect — not
/// a sink) is NOT E0721. Only `Db`/`Exec`/`Net` are sinks.
#[test]
fn tainted_at_non_sink_is_ok() {
    let src = r#"
fn run() {
    name :: #Tainted "world"
    print(name)
}
"#;
    assert!(
        !codes(src).iter().any(|c| c == "E0721"),
        "a non-sink call is not a taint sink"
    );
}

/// I3: taint is erased in codegen — a `#Tainted` value and its bare twin generate
/// byte-identical Rust. The `#Tainted` tag leaves no runtime trace.
#[test]
fn taint_is_erased_in_codegen() {
    let tagged = r#"
fn run() {
    name :: #Tainted "world"
    print(name)
}
"#;
    let plain = r#"
fn run() {
    name :: "world"
    print(name)
}
"#;
    let a = jet::compile(tagged).expect("tagged compiles").rust;
    let b = jet::compile(plain).expect("plain compiles").rust;
    assert_eq!(
        a, b,
        "the #Tainted tag must leave no trace in generated Rust (I3)"
    );
}

/// A `#Sanitizer fn` is a real, callable function; its body and signature behave
/// exactly like any other function (the modifier only affects the taint pass).
#[test]
fn sanitizer_fn_is_a_normal_function() {
    let src = r#"
#Sanitizer fn clean(raw: String) => String { return raw.split(" ")[0] }
fn run() {
    print(clean("a b c"))
}
"#;
    assert!(
        codes(src).is_empty(),
        "a #Sanitizer fn must compile and run: {:?}",
        codes(src)
    );
}

// --- D-TAINT-SAN (ratified 2026-06-25): bare `sanitizer fn` teaching error ---

/// Bare lowercase `sanitizer fn` is the retired spelling → E0059, pointing at
/// the PascalCase marker `#Sanitizer fn` (mirrors `pure` → `#Pure` / E0053).
#[test]
fn bare_sanitizer_fn_is_e0059() {
    let src = r#"
sanitizer fn clean(raw: String) => String { return raw.split(" ")[0] }
fn run() {
    print(clean("a b c"))
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0059"),
        "bare `sanitizer fn` must teach E0059: {:?}",
        codes(src)
    );
}

/// The teaching error fires for `sanitizer pub fn` too (the modifier may precede
/// `pub`), and recovery still parses the function as a sanitizer.
#[test]
fn bare_sanitizer_pub_fn_is_e0059() {
    let src = r#"
sanitizer pub fn clean(raw: String) => String { return raw.split(" ")[0] }
fn run() {
    print(clean("a b c"))
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0059"),
        "bare `sanitizer pub fn` must teach E0059: {:?}",
        codes(src)
    );
}

/// `sanitizer` as an ordinary identifier (a variable name) is unaffected — only
/// the `sanitizer fn` modifier position triggers the teaching error.
#[test]
fn sanitizer_as_identifier_is_fine() {
    let src = r#"
fn run() {
    sanitizer :: 3
    print("{sanitizer}")
}
"#;
    assert!(
        codes(src).is_empty(),
        "`sanitizer` as a binding name must be fine: {:?}",
        codes(src)
    );
}

// ── D-TAINT2=A tests: `#Tainted(Credential)` + E0722 ─────────────────────────

/// `#Tainted(Credential)` reaching bare `print` is E0722.
#[test]
fn tainted_credential_to_print_is_e0722() {
    let src = r#"
fn run() {
    token :: #Tainted(Credential) "bearer eyJhbGci..."
    print(token)
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0722"),
        "credential reaching print must be E0722: {:?}",
        codes(src)
    );
}

/// Bare `#Tainted` (no kind) at `print` is NOT E0722 — only Credential kind triggers it.
/// (E0721 would fire only at Db/Exec/Net sinks; bare taint at print is permitted.)
#[test]
fn tainted_input_to_print_is_not_e0722() {
    let src = r#"
fn run() {
    user_input :: #Tainted "hello world"
    print(user_input)
}
"#;
    assert!(
        !codes(src).iter().any(|c| c == "E0722"),
        "bare #Tainted reaching print must NOT be E0722: {:?}",
        codes(src)
    );
}

/// Credential taint propagates through a binding — the derived variable is also E0722.
#[test]
fn tainted_credential_propagates_through_binding() {
    let src = r#"
fn run() {
    raw_token :: #Tainted(Credential) "s3cr3t"
    derived := raw_token
    print(derived)
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0722"),
        "credential taint must propagate through binding to print: {:?}",
        codes(src)
    );
}

/// Auth accepts credential-tainted tokens for verification, while the token
/// itself remains protected from accidental logging.
#[test]
fn auth_token_keeps_credential_leak_protection() {
    let src = r#"
use core.auth as auth

fn run() {
    token :: #Tainted(Credential) "a.b.c"
    key :: [U8].{ 0, 1, 2 }
    _ := auth.verify_jwt(token, key: key, audience: "gateway")
    print(token)
}
"#;
    let found = codes(src);
    assert_eq!(
        found.iter().filter(|code| code.as_str() == "E0722").count(),
        1,
        "{found:?}"
    );
}

/// A clean value (no taint) at `print` is fine — no E0722.
#[test]
fn clean_value_to_print_is_not_e0722() {
    let src = r#"
fn run() {
    msg := "hello"
    print(msg)
}
"#;
    assert!(
        codes(src).is_empty(),
        "clean value at print must produce no errors: {:?}",
        codes(src)
    );
}
