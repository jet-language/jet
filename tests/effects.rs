//! Effect system tests (D-EFF1, D-QUAL1): per-function effect inference over the
//! call graph, the `#(…)` boundary check (E0740), and `@Pure` reconciliation.

mod common;

fn codes(src: &str) -> Vec<String> {
    match jet::compile(src) {
        Ok(_) => Vec::new(),
        Err(diags) => diags.iter().map(|d| d.code.clone()).collect(),
    }
}

/// I3 / D-EFF1: effect annotations (`#(…)`, `@Pure`, trait-method bounds) are a
/// compile-time proof only — they must leave NO trace in generated Rust. The
/// annotated program and its annotation-stripped twin generate byte-identical
/// Rust.
#[test]
fn effect_annotations_are_erased() {
    let annotated = r#"
trait Shape { @Pure fn area(self) -> Int; }
struct Square { side: Int }
impl Square.Shape { fn area(self) -> Int { return self.side * self.side; } }
@Pure fn sq(n: Int) -> Int { return n * n; }
fn load(p: String) #(Io) { print(p); }
fn invoke(n: Int) #(Io) { load("{sq(n)}"); }
fn run() { s :: Square.{ side: 3 }; print("{s.area()}"); invoke(2); }
"#;
    let plain = r#"
trait Shape { fn area(self) -> Int; }
struct Square { side: Int }
impl Square.Shape { fn area(self) -> Int { return self.side * self.side; } }
fn sq(n: Int) -> Int { return n * n; }
fn load(p: String) { print(p); }
fn invoke(n: Int) { load("{sq(n)}"); }
fn run() { s :: Square.{ side: 3 }; print("{s.area()}"); invoke(2); }
"#;
    let a = jet::compile(annotated).expect("annotated compiles").rust;
    let b = jet::compile(plain).expect("plain compiles").rust;
    assert_eq!(
        a, b,
        "effect annotations must leave no trace in generated Rust (I3)"
    );
}

/// I3: a `#Caps(…)` region lowers to a plain lexical block — the generated Rust
/// carries no effect machinery (no `Caps`, no `#(`, no effect runtime), and the
/// body runs unchanged.
#[test]
fn caps_region_erases_to_plain_block() {
    let src = r#"
fn run() {
    #Caps(Io) {
        print("inside");
    }
    print("outside");
}
"#;
    let rust = jet::compile(src).expect("compiles").rust;
    assert!(
        !rust.contains("Caps"),
        "generated Rust must not mention Caps:\n{rust}"
    );
    assert!(
        !rust.contains("#("),
        "generated Rust must carry no effect annotation:\n{rust}"
    );
    // The region body is still emitted (a plain block).
    assert!(
        rust.contains("inside") && rust.contains("outside"),
        "region body must survive:\n{rust}"
    );
}

/// A `#(Fs)` bound that matches the body's only effect compiles clean.
#[test]
fn declared_bound_matching_body_ok() {
    let src = r#"
use core.files as fs
fn load(path: String) #(Fs) -> String {
    return fs.read(copy path) ?? "";
}
fn run() { print(load("x")); }
"#;
    assert!(
        codes(src).is_empty(),
        "matching bound should compile: {:?}",
        codes(src)
    );
}

/// A bound that omits an effect the body uses is E0740.
#[test]
fn out_of_set_effect_is_e0740() {
    let src = r#"
use core.files as fs
fn load(path: String) #(Net) -> String {
    return fs.read(path) ?? "";
}
fn run() { print(load("x")); }
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0740"),
        "expected E0740, got {:?}",
        codes(src)
    );
}

/// Effects propagate transitively along calls: `load` reaches `Fs` only through
/// `helper`, yet its `#(Net)` bound is still violated.
#[test]
fn effects_propagate_transitively() {
    let src = r#"
use core.files as fs
fn helper(p: String) -> String { return fs.read(p) ?? ""; }
fn load(path: String) #(Net) -> String { return helper(path); }
fn run() { print(load("x")); }
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0740"),
        "transitive Fs should trip E0740: {:?}",
        codes(src)
    );
}

/// A wider bound than the body uses is allowed — `#(…)` is a ceiling, not exact.
#[test]
fn wider_bound_than_body_ok() {
    let src = r#"
use core.files as fs
fn load(path: String) #(Fs, Net) -> String {
    return fs.read(copy path) ?? "";
}
fn run() { print(load("x")); }
"#;
    assert!(
        codes(src).is_empty(),
        "a wider ceiling should compile: {:?}",
        codes(src)
    );
}

/// `print` contributes `Io`; an `#(Io)` bound covers it.
#[test]
fn io_effect_from_print_ok() {
    let src = r#"
fn announce(n: Int) #(Io) { print("{n}"); }
fn run() { announce(1); }
"#;
    assert!(
        codes(src).is_empty(),
        "Io bound should cover print: {:?}",
        codes(src)
    );
}

/// An unannotated function infers freely — no bound, no E0740.
#[test]
fn unannotated_function_never_trips_e0740() {
    let src = r#"
use core.files as fs
fn load(path: String) -> String { return fs.read(path) ?? ""; }
fn run() { print(load("x")); }
"#;
    assert!(
        !codes(src).iter().any(|c| c == "E0740"),
        "unannotated fn must not trip E0740: {:?}",
        codes(src)
    );
}

/// `@Pure fn` with a non-empty `#(…)` list is the contradiction E0745.
#[test]
fn pure_with_effects_is_e0745() {
    let src = r#"
@Pure fn calc() #(Fs) -> Int { return 1; }
fn run() { print(calc()); }
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0745"),
        "expected E0745, got {:?}",
        codes(src)
    );
}

/// D-EFF3: an impl of a `@Pure` trait method that reaches an effect is E0742.
#[test]
fn trait_impl_exceeding_bound_is_e0742() {
    let src = r#"
use core.files as fs
trait Hasher { @Pure fn hash(self) -> Int; }
struct Doc { path: String }
impl Doc.Hasher {
    fn hash(self) -> Int { body :: fs.read(self.path) ?? ""; return body.len(); }
}
fn run() { d :: Doc.{ path: "x" }; print(d.hash()); }
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0742"),
        "impl exceeding @Pure bound should be E0742: {:?}",
        codes(src)
    );
}

/// A conformant impl of a bounded trait method compiles clean.
#[test]
fn trait_impl_within_bound_ok() {
    let src = r#"
trait Shape { @Pure fn area(self) -> Int; }
struct Square { side: Int }
impl Square.Shape {
    fn area(self) -> Int { return self.side * self.side; }
}
fn run() { s :: Square.{ side: 5 }; print("{s.area()}"); }
"#;
    assert!(
        codes(src).is_empty(),
        "conformant impl should compile: {:?}",
        codes(src)
    );
}

/// D-EFF2 (transparent flow-through): passing a named effectful function as a
/// callback flows its effects to the caller, surfacing at the call site.
#[test]
fn named_callback_flows_through_to_caller() {
    let src = r#"
use core.files as fs
fn readit() -> String { return fs.read("x") ?? ""; }
fn apply(f: fn() -> String) -> String { return f(); }
fn caller() #(Net) -> String { return apply(readit); }
fn run() { print(caller()); }
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0740"),
        "callback Fs should flow to caller: {:?}",
        codes(src)
    );
}

/// A lambda callback's effects flow into the enclosing function too (the lambda
/// body is walked inline), so a `#Caps` region catches an effect inside it.
#[test]
fn lambda_callback_flows_into_region() {
    let src = r#"
use core.files as fs
fn apply(f: fn() -> String) -> String { return f(); }
fn run() {
    #Caps(Net) {
        r :: apply(() => fs.read("x") ?? "");
        print(r);
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0741"),
        "lambda Fs should surface in region: {:?}",
        codes(src)
    );
}

/// A pure named callback flows nothing — a bounded caller stays clean.
#[test]
fn pure_callback_flows_nothing() {
    let src = r#"
fn inc(n: Int) -> Int { return n + 1; }
fn apply(f: fn(Int) -> Int, x: Int) -> Int { return f(x); }
fn caller() #(Io) { print("{apply(inc, 1)}"); }
fn run() { caller(); }
"#;
    assert!(
        codes(src).is_empty(),
        "pure callback must not trip a bound: {:?}",
        codes(src)
    );
}

/// D-EFF1 reconciliation: `@Pure` is the empty effect set, so an effectful Core
/// call (here `Fs`) inside a `@Pure fn` is a purity violation (E3401) — even
/// though the legacy purity list only knew about stdin/time/random.
#[test]
fn pure_fn_with_core_effect_is_e3401() {
    let src = r#"
use core.files as fs
@Pure fn readit(p: String) -> String { return fs.read(p) ?? ""; }
fn run() { print(readit("x")); }
"#;
    assert!(
        codes(src).iter().any(|c| c == "E3401"),
        "Fs in @Pure fn should be E3401: {:?}",
        codes(src)
    );
}

/// A `#Caps(…)` region whose body stays within the cap set compiles clean.
#[test]
fn caps_region_within_set_ok() {
    let src = r#"
fn announce(n: Int) #(Io) { print("{n}"); }
fn run() {
    #Caps(Io) {
        announce(1);
    }
}
"#;
    assert!(
        codes(src).is_empty(),
        "in-set caps region should compile: {:?}",
        codes(src)
    );
}

/// An effect used inside a `#Caps(…)` region but not in its cap list is E0741.
#[test]
fn caps_region_out_of_set_is_e0741() {
    let src = r#"
use core.files as fs
fn run() {
    #Caps(Net) {
        text :: fs.read("x") ?? "";
        print(text);
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0741"),
        "expected E0741, got {:?}",
        codes(src)
    );
}

/// A `#Caps(…)` region restriction is transitive: an effect reached only through
/// a call still trips E0741.
#[test]
fn caps_region_transitive_is_e0741() {
    let src = r#"
use core.files as fs
fn helper(p: String) -> String { return fs.read(p) ?? ""; }
fn run() {
    #Caps(Io) {
        text :: helper("x");
        print(text);
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0741"),
        "transitive Fs should trip E0741: {:?}",
        codes(src)
    );
}

/// An unknown effect name is E0119, and does not also trip E0740.
#[test]
fn unknown_effect_name_is_e0119_only() {
    let src = r#"
fn work() #(Bogus) { print("hi"); }
fn run() { work(); }
"#;
    let c = codes(src);
    assert!(c.iter().any(|c| c == "E0119"), "expected E0119, got {:?}", c);
    assert!(
        !c.iter().any(|c| c == "E0740"),
        "unknown name should suppress E0740: {:?}",
        c
    );
}

// ── Scoped capabilities (D-SCAP1) ─────────────────────────────────────────────

/// D-SCAP1: a `#Grant(Fs) { caps -> … }` whose body stays within the granted set
/// compiles clean — the grant authorizes the listed effects.
#[test]
fn grant_within_set_ok() {
    let src = r#"
use core.files as fs
fn run() {
    #Grant(Fs, Io) { caps ->
        text :: fs.read("x") ?? "";
        print(text);
    }
}
"#;
    assert!(
        codes(src).is_empty(),
        "in-set grant should compile: {:?}",
        codes(src)
    );
}

/// D-SCAP1: an effect used inside a `#Grant(…)` that the grant doesn't authorize
/// has no capability — E0712 (the dual of E0741).
#[test]
fn grant_out_of_set_is_e0712() {
    let src = r#"
use core.files as fs
fn run() {
    #Grant(Net) { caps ->
        text :: fs.read("x") ?? "";
        print(text);
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0712"),
        "expected E0712, got {:?}",
        codes(src)
    );
}

/// D-SCAP1: the grant restriction is transitive — an effect reached only through
/// a call still trips E0712.
#[test]
fn grant_transitive_is_e0712() {
    let src = r#"
use core.files as fs
fn helper(p: String) -> String { return fs.read(p) ?? ""; }
fn run() {
    #Grant(Io) { caps ->
        text :: helper("x");
        print(text);
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0712"),
        "transitive Fs should trip E0712: {:?}",
        codes(src)
    );
}

/// D-SCAP1: the capability handle may not escape its grant — aliasing it to
/// another binding is E0711.
#[test]
fn grant_handle_alias_is_e0711() {
    let src = r#"
fn run() {
    #Grant(Io) { caps ->
        alias :: caps;
        print("hi");
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0711"),
        "aliasing the handle should be E0711: {:?}",
        codes(src)
    );
}

/// D-SCAP1: not naming the handle anywhere (never escaping it) compiles clean —
/// the grant is the authorizing context, the handle need not be used.
#[test]
fn grant_unused_handle_ok() {
    let src = r#"
fn run() {
    #Grant(Io) { caps ->
        print("granted");
    }
}
"#;
    assert!(
        codes(src).is_empty(),
        "an unused grant handle should compile: {:?}",
        codes(src)
    );
}

/// D-SCAP1: an unknown effect name in a `#Grant(…)` list is E0119 (shared with
/// `#Caps`/`#(…)`), and suppresses the E0712 subset check.
#[test]
fn grant_unknown_effect_is_e0119() {
    let src = r#"
fn run() {
    #Grant(Bogus) { caps ->
        print("hi");
    }
}
"#;
    let c = codes(src);
    assert!(c.iter().any(|c| c == "E0119"), "expected E0119, got {:?}", c);
    assert!(
        !c.iter().any(|c| c == "E0712"),
        "unknown name should suppress E0712: {:?}",
        c
    );
}

/// I3: a `#Grant(…)` region lowers to a plain lexical block — the generated Rust
/// carries no capability machinery (no handle value, no grant/revoke), no effect
/// annotation, and NO `unsafe`. The body runs unchanged.
#[test]
fn grant_region_erases_to_plain_block() {
    let src = r#"
fn run() {
    #Grant(Io) { caps ->
        print("inside");
    }
    print("outside");
}
"#;
    let rust = jet::compile(src).expect("compiles").rust;
    assert!(
        !rust.contains("grant"),
        "generated Rust must not mention grant:\n{rust}"
    );
    assert!(
        !rust.contains("Capability"),
        "generated Rust must not mention the handle type:\n{rust}"
    );
    assert!(
        !rust.contains("#("),
        "generated Rust must carry no effect annotation:\n{rust}"
    );
    assert!(
        !common::strip_vetted_prelude_modules(&rust).contains("unsafe"),
        "grant codegen must contain no `unsafe`:\n{rust}"
    );
    assert!(
        rust.contains("inside") && rust.contains("outside"),
        "region body must survive:\n{rust}"
    );
}

/// I3 (D-SCAP1): a `#Grant(…)` region lowers to the SAME plain Rust block as the
/// already-erased `#Caps(…)` region — the grant carries no machinery `#Caps`
/// doesn't, so swapping `#Grant(E) { h -> … }` for `#Caps(E) { … }` is identical
/// generated code (the handle is sema-only and erased).
#[test]
fn grant_lowers_like_caps_region() {
    let granted = r#"
fn run() {
    #Grant(Io) { caps ->
        print("a");
        print("b");
    }
}
"#;
    let caps = r#"
fn run() {
    #Caps(Io) {
        print("a");
        print("b");
    }
}
"#;
    let a = jet::compile(granted).expect("granted compiles").rust;
    let b = jet::compile(caps).expect("caps compiles").rust;
    assert_eq!(
        a, b,
        "#Grant region must lower identically to the erased #Caps region (I3)"
    );
}

// ── Transactions (D-TXN1–D-TXN4) ──────────────────────────────────────────────

/// D-TXN2: an irreversible Core effect (Fs) reached DIRECTLY inside a
/// `#Transact` block is E0746 at the call site.
#[test]
fn transact_irreversible_fs_is_e0746() {
    let src = r#"
use core.files as fs
fn run() {
    #Transact(tx) {
        text :: fs.read("x") ?? "";
        print(text);
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0746"),
        "Fs in #Transact should be E0746: {:?}",
        codes(src)
    );
}

/// D-TXN2: a Net effect directly in the block is also rejected (E0746).
#[test]
fn transact_irreversible_net_is_e0746() {
    let src = r#"
use core.http as http
fn run() {
    #Transact(tx) {
        r :: http.get("http://x") ?? "";
        print(r);
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0746"),
        "Net in #Transact should be E0746: {:?}",
        codes(src)
    );
}

/// D-TXN2: a reversible-or-benign effect (Io via `print`) is NOT rejected inside
/// a `#Transact` block.
#[test]
fn transact_reversible_io_ok() {
    let src = r#"
fn run() {
    #Transact(tx) {
        print("reversible work");
    }
}
"#;
    assert!(
        codes(src).is_empty(),
        "Io in #Transact should compile: {:?}",
        codes(src)
    );
}

/// D-TXN2 fix-it: the same Fs effect is accepted when registered via
/// `name.on_commit(…)` — a deferred (post-commit) context.
#[test]
fn transact_fs_in_on_commit_ok() {
    let src = r#"
use core.files as fs
fn run() {
    #Transact(tx) {
        print("reversible work");
        tx.on_commit(() => {
            fs.write("x", "done") ?? panic("write failed");
        });
    }
}
"#;
    assert!(
        codes(src).is_empty(),
        "Fs inside on_commit should compile: {:?}",
        codes(src)
    );
}

/// D-TXN3/D-TXN4: `on_commit` needs a zero-parameter lambda (E0104).
#[test]
fn transact_on_commit_needs_zero_param_lambda() {
    let src = r#"
fn run() {
    #Transact(tx) {
        tx.on_commit((n: Int) => { print("{n}"); });
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0104"),
        "on_commit with a param should be E0104: {:?}",
        codes(src)
    );
}

/// I3: a `#Transact` block + `on_commit` carry no effect/transaction machinery
/// that leaks user-visible effect annotations; and generated Rust has NO `unsafe`.
#[test]
fn transact_generates_no_unsafe() {
    let src = r#"
fn run() {
    #Transact(tx) {
        print("work");
        tx.on_commit(() => { print("hook"); });
    }
}
"#;
    let rust = jet::compile(src).expect("compiles").rust;
    // The transaction lowers to the safe `JetTransaction` prelude + boxed hooks.
    assert!(
        rust.contains("jet_transaction()"),
        "expected the transaction guard: {}",
        rust
    );
    assert!(
        rust.contains(".on_commit(Box::new("),
        "expected a boxed commit hook: {}",
        rust
    );
    // No `unsafe` word anywhere in generated code (golden grep parity).
    assert!(
        !common::strip_vetted_prelude_modules(&rust).contains("unsafe"),
        "transaction codegen must contain no `unsafe`"
    );
}

/// D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() => { … })` — the mirror of
/// `on_commit`. A zero-param lambda on a `#Transact` handle compiles clean and lowers
/// to a boxed rollback hook on the transaction guard.
#[test]
fn transact_on_rollback_ok() {
    let src = r#"
fn run() {
    #Transact(tx) {
        print("work");
        tx.on_rollback(() => { print("undo"); });
    }
}
"#;
    assert!(
        codes(src).is_empty(),
        "on_rollback should compile: {:?}",
        codes(src)
    );
    let rust = jet::compile(src).expect("compiles").rust;
    assert!(
        rust.contains(".on_rollback(Box::new("),
        "expected a boxed rollback hook: {}",
        rust
    );
}

/// D-TXN-ROLLBACK (layer 3): `on_rollback` needs a zero-parameter lambda (E0104),
/// exactly like `on_commit`.
#[test]
fn transact_on_rollback_needs_zero_param_lambda() {
    let src = r#"
fn run() {
    #Transact(tx) {
        tx.on_rollback((n: Int) => { print("{n}"); });
    }
}
"#;
    assert!(
        codes(src).iter().any(|c| c == "E0104"),
        "on_rollback with a param should be E0104: {:?}",
        codes(src)
    );
}

/// D-TXN-ROLLBACK (layer 1): a value mutated inside a `#Transact` block is
/// auto-snapshotted on entry — the generated Rust calls the `jet_txn::snapshot`
/// prelude helper against the transaction guard for the mutated root.
#[test]
fn transact_auto_snapshot_mutated_value() {
    let src = r#"
enum Fail { Bad }
fn transfer(from: &Int, to: &Int, amount: Int) -> Int ? Fail {
    #Transact(tx) {
        from -= amount;
        to += amount;
        if amount > 100 {
            return err(Fail.Bad);
        }
    }
    return ok(amount);
}
fn run() {
    a: Int := 100
    b: Int := 0
    n :: transfer(&a, &b, 40) ?? (-1)
    print("{n}")
}
"#;
    assert!(
        codes(src).is_empty(),
        "auto-snapshot transfer should compile: {:?}",
        codes(src)
    );
    let rust = jet::compile(src).expect("compiles").rust;
    assert!(
        rust.contains("jet_txn::snapshot(&mut user_tx, &mut (*user_from))"),
        "expected an auto-snapshot of the mutated `from`: {}",
        rust
    );
    assert!(
        rust.contains("jet_txn::snapshot(&mut user_tx, &mut (*user_to))"),
        "expected an auto-snapshot of the mutated `to`: {}",
        rust
    );
}

/// D-TXN-ROLLBACK (layer 1): the auto-snapshot mechanism's one vetted raw-pointer
/// writeback lives ONLY inside the `mod jet_txn` prelude module — never in user code.
#[test]
fn transact_auto_snapshot_unsafe_only_in_prelude() {
    let src = r#"
enum Fail { Bad }
fn bump(x: &Int) -> Int ? Fail {
    #Transact(tx) {
        x += 1;
        return err(Fail.Bad);
    }
    return ok(0);
}
fn run() { a: Int := 0; n :: bump(&a) ?? (-1); print("{n}"); }
"#;
    let rust = jet::compile(src).expect("compiles").rust;
    assert!(
        rust.contains("mod jet_txn"),
        "jet_txn module present (snapshot used)"
    );
    // The only `unsafe` is inside vetted prelude regions (mod jet_txn { … } and
    // the rest of the canonical list). Strip them all and assert none remain.
    let stripped = common::strip_vetted_prelude_modules(&rust);
    assert!(
        !stripped.contains("unsafe"),
        "auto-snapshot `unsafe` must be confined to `mod jet_txn`: {}",
        stripped
    );
}

/// D-TXN4: a bare `#Transact { … }` with no handle stays legal (no hooks).
#[test]
fn transact_bare_no_handle_ok() {
    let src = r#"
fn run() {
    #Transact {
        print("bare transaction");
    }
}
"#;
    assert!(
        codes(src).is_empty(),
        "bare #Transact should compile: {:?}",
        codes(src)
    );
}

// ---------------------------------------------------------------------------
// D-EFF2 expert levers: callback param effect bounds + `#(via f)` pass-through.
// ---------------------------------------------------------------------------

/// Lever 1: a `@Pure fn(…)` parameter handed a pure callback compiles clean.
#[test]
fn callback_pure_bound_pure_arg_ok() {
    let src = r#"
fn transform(items: [Int], f: @Pure fn(Int) -> Int) -> [Int] {
    return items.map((x) => f(x));
}
@Pure fn inc(n: Int) -> Int { return n + 1; }
fn run() { print("{transform([1, 2], inc)}"); }
"#;
    assert!(
        codes(src).is_empty(),
        "pure callback to a @Pure bound is clean: {:?}",
        codes(src)
    );
}

/// Lever 1: a `@Pure fn(…)` parameter handed an impure callback is E0747.
#[test]
fn callback_pure_bound_impure_arg_is_e0747() {
    let src = r#"
fn transform(items: [Int], f: @Pure fn(Int) -> Int) -> [Int] {
    return items.map((x) => f(x));
}
fn noisy(n: Int) -> Int { print("{n}"); return n; }
fn run() { print("{transform([1, 2], noisy)}"); }
"#;
    assert_eq!(
        codes(src),
        vec!["E0747"],
        "impure callback to a @Pure bound is E0747"
    );
}

/// Lever 1: a `#(E) fn(…)` parameter handed a callback within `E` compiles clean.
#[test]
fn callback_set_bound_within_ok() {
    let src = r#"
fn invoke(n: Int, act: #(Io) fn(Int)) {
    act(n);
}
fn show(n: Int) { print("{n}"); }
fn run() { invoke(5, show); }
"#;
    assert!(
        codes(src).is_empty(),
        "Io callback within an #(Io) bound is clean: {:?}",
        codes(src)
    );
}

/// Lever 1: a `#(E) fn(…)` parameter handed a callback that reaches an effect
/// outside `E` is E0747.
#[test]
fn callback_set_bound_exceeded_is_e0747() {
    let src = r#"
use core.files as fs
fn invoke(p: String, act: #(Io) fn(String)) {
    act(p);
}
fn read_it(p: String) { x :: fs.read(copy p) ?? ""; print("{x}"); }
fn run() { invoke("f.txt", read_it); }
"#;
    assert_eq!(
        codes(src),
        vec!["E0747"],
        "Fs callback to an #(Io) bound is E0747"
    );
}

/// Lever 1: an unknown effect name in a callback bound is E0119.
#[test]
fn callback_bound_unknown_effect_is_e0119() {
    let src = r#"
fn invoke(n: Int, act: #(Nope) fn(Int)) { act(n); }
fn show(n: Int) { print("{n}"); }
fn run() { invoke(5, show); }
"#;
    assert_eq!(
        codes(src),
        vec!["E0119"],
        "unknown effect in a callback bound is E0119"
    );
}

/// Lever 2: `#(via f)` publishes the callback's effects — a `@Pure fn` calling a
/// via-fn whose callback carries `Io` is rejected (E3401), proving the
/// pass-through surfaced the effect even though the body only calls a fn-value.
#[test]
fn effect_via_publishes_callback_effect() {
    let src = r#"
fn invoke(n: Int, act: #(Io) fn(Int)) #(via act) {
    act(n);
}
fn show(n: Int) { print("{n}"); }
@Pure fn caller() -> Int { invoke(5, show); return 0; }
fn run() { print("{caller()}"); }
"#;
    assert_eq!(
        codes(src),
        vec!["E3401"],
        "#(via act) must publish the Io effect to callers"
    );
}

/// Lever 2: `#(via f)` naming a non-existent parameter is E0748.
#[test]
fn effect_via_unknown_param_is_e0748() {
    let src = r#"
fn invoke(n: Int, act: #(Io) fn(Int)) #(via missing) { act(n); }
fn show(n: Int) { print("{n}"); }
fn run() { invoke(5, show); }
"#;
    assert_eq!(codes(src), vec!["E0748"], "#(via missing) is E0748");
}

/// Lever 2: `#(via f)` naming a non-callback parameter is E0748.
#[test]
fn effect_via_non_callback_param_is_e0748() {
    let src = r#"
fn invoke(n: Int) #(via n) { print("{n}"); }
fn run() { invoke(5); }
"#;
    assert_eq!(
        codes(src),
        vec!["E0748"],
        "#(via n) on a non-fn param is E0748"
    );
}

/// I3: the D-EFF2 levers are erased — a program using `@Pure fn(…)` callback
/// bounds and `#(via f)` generates the same Rust as its annotation-stripped twin.
#[test]
fn eff2_levers_are_erased() {
    let annotated = r#"
fn transform(items: [Int], f: @Pure fn(Int) -> Int) -> [Int] {
    return items.map((x) => f(x));
}
fn invoke(n: Int, act: #(Io) fn(Int)) #(via act) { act(n); }
@Pure fn inc(n: Int) -> Int { return n + 1; }
fn show(n: Int) { print("{n}"); }
fn run() { print("{transform([1], inc)}"); invoke(5, show); }
"#;
    let plain = r#"
fn transform(items: [Int], f: fn(Int) -> Int) -> [Int] {
    return items.map((x) => f(x));
}
fn invoke(n: Int, act: fn(Int)) { act(n); }
fn inc(n: Int) -> Int { return n + 1; }
fn show(n: Int) { print("{n}"); }
fn run() { print("{transform([1], inc)}"); invoke(5, show); }
"#;
    let a = jet::compile(annotated).expect("annotated compiles").rust;
    let b = jet::compile(plain).expect("plain compiles").rust;
    assert_eq!(
        a, b,
        "D-EFF2 levers must leave no trace in generated Rust (I3)"
    );
}
