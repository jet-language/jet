//! Taint tracking tests (D-TAINT1): `#Tainted` value-fact propagation, the
//! `#Sanitizer fn` taint-strip contract, and the tainted→sink error E0721. Taint
//! is a compile-time proof, erased in codegen (I3).

fn codes(src: &str) -> Vec<&'static str> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code).collect(),
    }
}

/// A tainted value reaching an `Exec` sink (`process.run`) directly is E0721.
#[test]
fn tainted_to_exec_sink_is_error() {
    let src = r#"
use core.process as process
fn main() {
    name @= #Tainted "world; rm -rf /"
    process.run(["echo", name]) ?? return
}
"#;
    assert!(codes(src).contains(&"E0721"), "direct tainted→sink must be E0721");
}

/// Routing the tainted value through a `#Sanitizer fn` clears taint — the sink
/// call is then accepted.
#[test]
fn sanitized_value_reaches_sink_ok() {
    let src = r#"
use core.process as process
#Sanitizer fn clean(raw: String) -> String { return raw.split(" ")[0] }
fn main() {
    name @= #Tainted "world; rm -rf /"
    safe := clean(name)
    process.run(["echo", safe]) ?? return
}
"#;
    assert!(codes(src).is_empty(), "sanitized value at sink must compile: {:?}", codes(src));
}

/// Taint spreads through a binding: a value derived from a tainted value (here a
/// reassignment) is itself tainted and still trips the sink check.
#[test]
fn taint_propagates_through_binding() {
    let src = r#"
use core.process as process
fn main() {
    raw @= #Tainted "evil"
    cmd := raw
    process.run(["echo", cmd]) ?? return
}
"#;
    assert!(codes(src).contains(&"E0721"), "taint must propagate through a binding");
}

/// Taint spreads through string interpolation: a tainted value spliced into a
/// string taints the result.
#[test]
fn taint_propagates_through_interpolation() {
    let src = r#"
use core.process as process
fn main() {
    user @= #Tainted "bob"
    arg := "hello {user}"
    process.run(["echo", arg]) ?? return
}
"#;
    assert!(codes(src).contains(&"E0721"), "taint must propagate through interpolation");
}

/// A reassignment to a clean value clears taint — the local is no longer tainted.
#[test]
fn reassign_to_clean_clears_taint() {
    let src = r#"
use core.process as process
fn main() {
    x := #Tainted "evil"
    x = "safe-literal"
    process.run(["echo", x]) ?? return
}
"#;
    assert!(codes(src).is_empty(), "reassign to a clean value must clear taint: {:?}", codes(src));
}

/// An ordinary (untainted) value at a sink is fine — no false positive.
#[test]
fn clean_value_at_sink_ok() {
    let src = r#"
use core.process as process
fn main() {
    process.run(["echo", "hello"]) ?? return
}
"#;
    assert!(codes(src).is_empty(), "clean value at sink must compile: {:?}", codes(src));
}

/// A tainted value at a non-sink call (an ordinary `print`, an `Io` effect — not
/// a sink) is NOT E0721. Only `Db`/`Exec`/`Net` are sinks.
#[test]
fn tainted_at_non_sink_is_ok() {
    let src = r#"
fn main() {
    name @= #Tainted "world"
    print(name)
}
"#;
    assert!(!codes(src).contains(&"E0721"), "a non-sink call is not a taint sink");
}

/// I3: taint is erased in codegen — a `#Tainted` value and its bare twin generate
/// byte-identical Rust. The `#Tainted` tag leaves no runtime trace.
#[test]
fn taint_is_erased_in_codegen() {
    let tagged = r#"
fn main() {
    name @= #Tainted "world"
    print(name)
}
"#;
    let plain = r#"
fn main() {
    name @= "world"
    print(name)
}
"#;
    let a = jet::compile(tagged).expect("tagged compiles").rust;
    let b = jet::compile(plain).expect("plain compiles").rust;
    assert_eq!(a, b, "the #Tainted tag must leave no trace in generated Rust (I3)");
}

/// A `#Sanitizer fn` is a real, callable function; its body and signature behave
/// exactly like any other function (the modifier only affects the taint pass).
#[test]
fn sanitizer_fn_is_a_normal_function() {
    let src = r#"
#Sanitizer fn clean(raw: String) -> String { return raw.split(" ")[0] }
fn main() {
    print(clean("a b c"))
}
"#;
    assert!(codes(src).is_empty(), "a #Sanitizer fn must compile and run: {:?}", codes(src));
}
