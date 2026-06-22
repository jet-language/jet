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

/// D-EFF3: an impl of a `#Pure` trait method that reaches an effect is E0742.
#[test]
fn trait_impl_exceeding_bound_is_e0742() {
    let src = r#"
use core.fs as fs
trait Hasher { #Pure fn hash(self) -> Int; }
struct Doc { path: String }
impl Doc: Hasher {
    fn hash(self) -> Int { body @= fs.read(self.path) ?? ""; return body.len(); }
}
fn main() { d @= Doc { path: "x" }; print(d.hash()); }
"#;
    assert!(codes(src).contains(&"E0742"), "impl exceeding #Pure bound should be E0742: {:?}", codes(src));
}

/// A conformant impl of a bounded trait method compiles clean.
#[test]
fn trait_impl_within_bound_ok() {
    let src = r#"
trait Shape { #Pure fn area(self) -> Int; }
struct Square { side: Int }
impl Square: Shape {
    fn area(self) -> Int { return self.side * self.side; }
}
fn main() { s @= Square { side: 5 }; print("{s.area()}"); }
"#;
    assert!(codes(src).is_empty(), "conformant impl should compile: {:?}", codes(src));
}

/// D-EFF2 (transparent flow-through): passing a named effectful function as a
/// callback flows its effects to the caller, surfacing at the call site.
#[test]
fn named_callback_flows_through_to_caller() {
    let src = r#"
use core.fs as fs
fn readit() -> String { return fs.read("x") ?? ""; }
fn apply(f: fn() -> String) -> String { return f(); }
fn caller() #(Net) -> String { return apply(readit); }
fn main() { print(caller()); }
"#;
    assert!(codes(src).contains(&"E0740"), "callback Fs should flow to caller: {:?}", codes(src));
}

/// A lambda callback's effects flow into the enclosing function too (the lambda
/// body is walked inline), so a `#Caps` region catches an effect inside it.
#[test]
fn lambda_callback_flows_into_region() {
    let src = r#"
use core.fs as fs
fn apply(f: fn() -> String) -> String { return f(); }
fn main() {
    #Caps(Net) {
        r @= apply(() => fs.read("x") ?? "");
        print(r);
    }
}
"#;
    assert!(codes(src).contains(&"E0741"), "lambda Fs should surface in region: {:?}", codes(src));
}

/// A pure named callback flows nothing — a bounded caller stays clean.
#[test]
fn pure_callback_flows_nothing() {
    let src = r#"
fn inc(n: Int) -> Int { return n + 1; }
fn apply(f: fn(Int) -> Int, x: Int) -> Int { return f(x); }
fn caller() #(Io) { print("{apply(inc, 1)}"); }
fn main() { caller(); }
"#;
    assert!(codes(src).is_empty(), "pure callback must not trip a bound: {:?}", codes(src));
}

/// D-EFF1 reconciliation: `#Pure` is the empty effect set, so an effectful Core
/// call (here `Fs`) inside a `#Pure fn` is a purity violation (E3401) — even
/// though the legacy purity list only knew about stdin/time/random.
#[test]
fn pure_fn_with_core_effect_is_e3401() {
    let src = r#"
use core.fs as fs
#Pure fn readit(p: String) -> String { return fs.read(p) ?? ""; }
fn main() { print(readit("x")); }
"#;
    assert!(codes(src).contains(&"E3401"), "Fs in #Pure fn should be E3401: {:?}", codes(src));
}

/// A `#Caps(…)` region whose body stays within the cap set compiles clean.
#[test]
fn caps_region_within_set_ok() {
    let src = r#"
fn announce(n: Int) #(Io) { print("{n}"); }
fn main() {
    #Caps(Io) {
        announce(1);
    }
}
"#;
    assert!(codes(src).is_empty(), "in-set caps region should compile: {:?}", codes(src));
}

/// An effect used inside a `#Caps(…)` region but not in its cap list is E0741.
#[test]
fn caps_region_out_of_set_is_e0741() {
    let src = r#"
use core.fs as fs
fn main() {
    #Caps(Net) {
        text @= fs.read("x") ?? "";
        print(text);
    }
}
"#;
    assert!(codes(src).contains(&"E0741"), "expected E0741, got {:?}", codes(src));
}

/// A `#Caps(…)` region restriction is transitive: an effect reached only through
/// a call still trips E0741.
#[test]
fn caps_region_transitive_is_e0741() {
    let src = r#"
use core.fs as fs
fn helper(p: String) -> String { return fs.read(p) ?? ""; }
fn main() {
    #Caps(Io) {
        text @= helper("x");
        print(text);
    }
}
"#;
    assert!(codes(src).contains(&"E0741"), "transitive Fs should trip E0741: {:?}", codes(src));
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
