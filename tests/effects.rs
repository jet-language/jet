//! Effect system tests (D-EFF1, D-QUAL1): per-function effect inference over the
//! call graph, the `#(…)` boundary check (E0740), and `#Pure` reconciliation.

fn codes(src: &str) -> Vec<&'static str> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code).collect(),
    }
}

/// A `#(Fs)` bound that matches the body's only effect compiles clean.
#[test]
fn declared_bound_matching_body_ok() {
    let src = r#"
use core.fs as fs
fn load(path: String) #(Fs) -> String {
    return fs.read(path) ?? "";
}
fn main() { print(load("x")); }
"#;
    assert!(codes(src).is_empty(), "matching bound should compile: {:?}", codes(src));
}

/// A bound that omits an effect the body uses is E0740.
#[test]
fn out_of_set_effect_is_e0740() {
    let src = r#"
use core.fs as fs
fn load(path: String) #(Net) -> String {
    return fs.read(path) ?? "";
}
fn main() { print(load("x")); }
"#;
    assert!(codes(src).contains(&"E0740"), "expected E0740, got {:?}", codes(src));
}

/// Effects propagate transitively along calls: `load` reaches `Fs` only through
/// `helper`, yet its `#(Net)` bound is still violated.
#[test]
fn effects_propagate_transitively() {
    let src = r#"
use core.fs as fs
fn helper(p: String) -> String { return fs.read(p) ?? ""; }
fn load(path: String) #(Net) -> String { return helper(path); }
fn main() { print(load("x")); }
"#;
    assert!(codes(src).contains(&"E0740"), "transitive Fs should trip E0740: {:?}", codes(src));
}

/// A wider bound than the body uses is allowed — `#(…)` is a ceiling, not exact.
#[test]
fn wider_bound_than_body_ok() {
    let src = r#"
use core.fs as fs
fn load(path: String) #(Fs, Net) -> String {
    return fs.read(path) ?? "";
}
fn main() { print(load("x")); }
"#;
    assert!(codes(src).is_empty(), "a wider ceiling should compile: {:?}", codes(src));
}

/// `print` contributes `Io`; an `#(Io)` bound covers it.
#[test]
fn io_effect_from_print_ok() {
    let src = r#"
fn announce(n: Int) #(Io) { print("{n}"); }
fn main() { announce(1); }
"#;
    assert!(codes(src).is_empty(), "Io bound should cover print: {:?}", codes(src));
}

/// An unannotated function infers freely — no bound, no E0740.
#[test]
fn unannotated_function_never_trips_e0740() {
    let src = r#"
use core.fs as fs
fn load(path: String) -> String { return fs.read(path) ?? ""; }
fn main() { print(load("x")); }
"#;
    assert!(!codes(src).contains(&"E0740"), "unannotated fn must not trip E0740: {:?}", codes(src));
}

/// `#Pure fn` with a non-empty `#(…)` list is the contradiction E0745.
#[test]
fn pure_with_effects_is_e0745() {
    let src = r#"
#Pure fn calc() #(Fs) -> Int { return 1; }
fn main() { print(calc()); }
"#;
    assert!(codes(src).contains(&"E0745"), "expected E0745, got {:?}", codes(src));
}

/// An unknown effect name is E0119, and does not also trip E0740.
#[test]
fn unknown_effect_name_is_e0119_only() {
    let src = r#"
fn work() #(Bogus) { print("hi"); }
fn main() { work(); }
"#;
    let c = codes(src);
    assert!(c.contains(&"E0119"), "expected E0119, got {:?}", c);
    assert!(!c.contains(&"E0740"), "unknown name should suppress E0740: {:?}", c);
}
