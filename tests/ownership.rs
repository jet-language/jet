//! Tests for M2 ownership / borrow transpiler rules (SAFETY DEFAULTS).

mod common;

use std::fs;
use std::process::Command;

#[test]
fn discard_binding_is_not_a_referenceable_local() {
    let src = r#"
fn consume(value: ^String) {
    print(value)
}

fn run() {
    _ :: "discarded"
    consume(^_)
}
"#;
    let diags = jet::compile(src).expect_err("the discard name must not be referenceable");
    assert!(
        diags.iter().any(|diagnostic| diagnostic.code == "E0107"),
        "expected the ordinary unknown-name error: {diags:?}"
    );
    assert!(
        diags
            .iter()
            .all(|diagnostic| !diagnostic.fix.contains("~_")),
        "a discard must never receive a copy suggestion: {diags:?}"
    );
}

/// D-MEM1/S2: no clone is ever silent (I8) — the former lint (`L0201`) is now
/// a hard error (`E0209`), regardless of liveness.
#[test]
fn implicit_clone_is_error_not_lint() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn run() {
msg :: "hello"
    consume(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(
        diags.iter().any(|d| d.code == "E0209"),
        "expected E0209 implicit-clone hard error"
    );
}

/// D-MEM1/S2 ("signatures can't lie"): an unmarked param is always `Read` —
/// no body-usage elevation. A body write through it is a hard error (E0205)
/// with a fix-it naming the `&` sigil, same as a non-`&self` receiver.
#[test]
fn body_write_to_unmarked_param_is_error() {
    let src = r#"
struct Counter {
    n: Int
}
fn bump(c: Counter) {
    c.n = c.n + 1
}
fn run() {
    c :: Counter{ n: 0 }
    bump(c)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0205")
        .expect("expected E0205: body write to an unmarked (Read) param");
    assert!(
        d.fix.contains("&Counter"),
        "fix should point at adding `&` to the param, got: {}",
        d.fix
    );
}

#[test]
fn generic_clone_bound_is_usage_sensitive() {
    let src = r#"
fn inspect<T>(value: T) Int -> 1
fn duplicate<T>(value: T) T -[..E]> { return ~value }
fn increment(value: Int) Int -[]> { return value + 1 }

fn run() {
    callback :: increment
    print(inspect(callback))
    print(duplicate(4))
}
"#;
    let out = jet::compile(src).expect("usage-sensitive generic bounds should compile");
    assert!(
        out.rust.contains("fn __jet_inspect<T>"),
        "read-only generic must not require Clone: {}",
        out.rust
    );
    assert!(
        out.rust.contains("fn __jet_duplicate<T: Clone>"),
        "explicit copy must require Clone: {}",
        out.rust
    );
}

#[test]
fn nested_plain_parameter_read_does_not_clone() {
    let src = r#"
struct Leaf { text: String }
struct Branch { leaf: Leaf }
fn show(branch: Branch) { print(branch.leaf.text) }
fn run() {
    branch :: Branch{leaf: Leaf{text: "read"}}
    show(branch)
}
"#;
    let out = jet::compile(src).expect("nested read should compile");
    assert!(
        !out.rust
            .contains("__jet_branch.__jet_leaf.__jet_text).clone()"),
        "nested read must stay borrowed: {}",
        out.rust
    );
}

/// D-CONC-SHARE1=A (card #1561): plain field access on a `Shared<T>` is the
/// only source spelling. Sema desugars a field read into ONE locked read and a
/// field write into ONE locked edit, both through the same Prelude seam the
/// retired closure forms used, so no execution tier grows a second mechanism.
/// The synthesized payload parameter is `__jet_shared_value`, a name no user
/// program can spell, so counting its closures counts the locks taken.
#[test]
fn shared_callbacks_receive_exactly_one_host_borrow() {
    let src = r#"
struct Config { hits: Int }
fn run() {
    config := shared Config{ hits: 0 }
    print(config.hits)
    config.hits += 1
}
"#;
    let out = jet::compile(src).expect("plain shared access must lower without double borrowing");
    assert!(
        out.rust.contains("|__jet_shared_value: &__jet_Config|"),
        "{}",
        out.rust
    );
    assert!(
        out.rust.contains("|__jet_shared_value: &mut __jet_Config|"),
        "{}",
        out.rust
    );
    assert!(!out.rust.contains("&&__jet_Config"), "{}", out.rust);
    assert!(!out.rust.contains("&mut &__jet_Config"), "{}", out.rust);
    // The single-write property. `config.hits += 1` is a read-modify-write, and
    // the desugar must keep both halves inside ONE edit so no update is lost. A
    // two-step lowering would either add a second read closure (the `+=` load)
    // or nest a read inside the edit; either way the counts below move. One
    // read closure (`print(config.hits)`) and one edit closure (`+= 1`) is the
    // only shape in which the write's load and store share a single lock.
    assert_eq!(
        out.rust
            .matches("|__jet_shared_value: &__jet_Config|")
            .count(),
        1,
        "the field read is one locked read: {}",
        out.rust
    );
    assert_eq!(
        out.rust
            .matches("|__jet_shared_value: &mut __jet_Config|")
            .count(),
        1,
        "the read-modify-write is one locked edit: {}",
        out.rust
    );
}

#[test]
fn shared_edit_guard_spans_helper_calls() {
    let src = r#"
struct Jobs { items: [Int] }

impl Jobs {
    fn add(&self, value: Int) {
        self.items.push(value)
    }

    fn count(self) Int -[]> {
        return self.items.len()
    }
}

fn run() {
    queue := shared Jobs{ items: [Int]{} }
    other := shared Jobs{ items: [Int]{} }
    guard :: queue.guard_edit()
    other_guard :: other.guard_read()
    guard.value.add(1)
    guard.value.add(2)
    print(other_guard.value.count())
    print(guard.value.count())
}
"#;
    let output =
        jet::compile(src).expect("an edit guard must preserve one write loan across helper calls");
    // The receipt names the lock by the Rust binding it acquires through, so
    // the identity carries the one machine prefix every generated local wears
    // (`Syntax::GENERATED_NAME_PREFIX`, `__jet_`); it was spelled `user_`
    // before that prefix was unified.
    let first = output
        .rust
        .find("jet-shared-lock-order-receipt: lock=__jet_queue; acquire=guard_edit; order=source")
        .expect("first lock receipt needs identity and acquisition mode");
    let second = output
        .rust
        .find("jet-shared-lock-order-receipt: lock=__jet_other; acquire=guard_read; order=source")
        .expect("second lock receipt needs identity and acquisition mode");
    assert!(
        first < second,
        "nested-lock audit receipts must preserve source acquisition order"
    );
}

#[test]
fn shared_read_guard_value_is_not_writable() {
    let src = r#"
struct Counter { value: Int }

fn run() {
    counter := shared Counter{ value: 0 }
    guard :: counter.guard_read()
    guard.value.value += 1
}
"#;
    let diagnostics =
        jet::compile(src).expect_err("a read guard must not grant write access to its value");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0205"),
        "expected the ordinary read-only write diagnostic: {diagnostics:?}"
    );
}

#[test]
fn shared_guard_public_type_annotation_resolves() {
    let src = r#"
fn inspect(guard: SharedGuard<Int>) {
    print(guard.value)
}

fn run() {
    cell := shared 1
    guard :: cell.guard_edit()
    inspect(guard)
    guard.value += 1
}
"#;
    jet::compile(src).expect("SharedGuard<T> must be a public, annotatable expert type");
}

#[test]
fn shared_read_guard_cannot_enter_write_helper() {
    let src = r#"
fn bump(&guard: SharedGuard<Int>) {
    guard.value += 1
}

fn run() {
    cell := shared 1
    guard := cell.guard_read()
    bump(&guard)
}
"#;
    let diagnostics =
        jet::compile(src).expect_err("a write helper must require an edit SharedGuard");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0205"),
        "expected read-vs-edit guard diagnostic: {diagnostics:?}"
    );
}

#[test]
fn returned_public_guard_is_read_only_without_write_helper_access() {
    let src = r#"
fn acquire(handle: Shared<Int>) SharedGuard<Int> -[]> {
    return handle.guard_edit()
}

fn run() {
    cell := shared 1
    guard := acquire(cell)
    guard.value += 1
}
"#;
    let diagnostics =
        jet::compile(src).expect_err("an untagged returned guard must not regain edit access");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0205"),
        "expected returned guard access diagnostic: {diagnostics:?}"
    );
}

#[test]
fn shared_guard_map_and_split_preserve_disjoint_places() {
    let src = r#"
struct Pair { left: Int, right: Int }

fn run() {
    first := shared Pair{ left: 1, right: 2 }
    mapped :: first.guard_edit().map(value -> value.left)
    mapped.value += 1

    second := shared Pair{ left: 3, right: 4 }
    (left, right) :: second.guard_edit().split(
        value -> value.left,
        value -> value.right
    )
    left.value += 1
    right.value += 1
}
"#;
    jet::compile(src).expect("mapped and split guards must preserve safe field provenance");
}

#[test]
fn shared_guard_split_rejects_overlapping_places() {
    let src = r#"
struct Pair { left: Int, right: Int }

fn run() {
    cell := shared Pair{ left: 1, right: 2 }
    guard :: cell.guard_edit()
    _ :: guard.split(value -> value.left, value -> value.left)
}
"#;
    let diagnostics = jet::compile(src).expect_err("split guard projections must be disjoint");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0216"),
        "expected a projection error: {diagnostics:?}"
    );
}

#[test]
fn shared_guard_is_owned_and_noncloneable() {
    let src = r#"
fn run() {
    cell := shared 1
    guard :: cell.guard_read()
    _ :: ~guard
}
"#;
    let diagnostics = jet::compile(src).expect_err("a lock token must have one owner");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0211"),
        "expected the noncloneable-value diagnostic: {diagnostics:?}"
    );
}

#[test]
fn shared_guard_wait_requires_edit_access_and_bool_predicate() {
    let read_src = r#"
fn run() {
    cell := shared 1
    changed := Condition.new()
    guard :: cell.guard_read()
    _ :: guard.wait(changed, value -> value == 1)
}
"#;
    let diagnostics = jet::compile(read_src).expect_err("waiting must require an exclusive guard");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0205"),
        "expected the read-guard wait diagnostic: {diagnostics:?}"
    );

    let predicate_src = r#"
fn run() {
    cell := shared 1
    changed := Condition.new()
    guard :: cell.guard_edit()
    _ :: guard.wait(changed, value -> value)
}
"#;
    let diagnostics = jet::compile(predicate_src).expect_err("a wait predicate must return Bool");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.code.as_str(), "E0112" | "E0113")),
        "expected the predicate diagnostic: {diagnostics:?}"
    );
}

#[test]
fn shared_guard_long_scope_has_lock_order_lint() {
    let src = r#"
fn run() {
    cell := shared 1
    guard :: cell.guard_edit()
    print(guard.value)
    print(guard.value)
    print(guard.value)
    print(guard.value)
    print(guard.value)
    print(guard.value)
    print(guard.value)
    print(guard.value)
}
"#;
    let output = jet::compile(src).expect("a long guard scope is legal expert code");
    let lint = output
        .lints
        .iter()
        .find(|diagnostic| diagnostic.code == "L0206")
        .expect("long guard scopes must produce an audit lint");
    assert!(
        lint.fix.contains("stable order"),
        "the lint must teach nested lock order: {lint:?}"
    );
}

#[test]
fn local_cell_surface_uses_read_receivers_and_host_borrows() {
    let source = r#"
struct Pair { left: Int, right: Int }
fn update(cell: Cell<Pair>) Int -[]> {
    cell.set(Pair{ left: 2, right: 3 })
    old :: cell.replace(Pair{ left: 4, right: 5 })
    cell.edit(pair -> pair.left += old.left)
    return cell.read(pair -> pair.left + pair.right)
}
fn run() {
    cell :: Cell.new(Pair{ left: 0, right: 1 })
    print(update(cell))
}
"#;
    let output = jet::compile(source).expect("local Cell surface must compile");
    assert!(
        output.rust.contains("jet_std::JetCell<__jet_Pair>"),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("|__jet_pair: &__jet_Pair|"),
        "{}",
        output.rust
    );
    assert!(
        output.rust.contains("|__jet_pair: &mut __jet_Pair|"),
        "{}",
        output.rust
    );
}

#[test]
fn local_cell_runtime_has_no_atomic_or_os_lock_storage() {
    let cell = include_str!("../crates/jet-codegen/src/Prelude/LocalCell.rs");
    assert!(cell.contains("std::rc::Rc"), "{cell}");
    assert!(cell.contains("std::cell::UnsafeCell"), "{cell}");
    assert!(!cell.contains("std::sync::Arc"), "{cell}");
    assert!(!cell.contains("Mutex"), "{cell}");
    assert!(!cell.contains("RwLock"), "{cell}");
}

#[test]
fn local_cell_runtime_releases_original_loans_on_drop_and_unwind() {
    if !common::have_rustc() {
        return;
    }
    let cell = include_str!("../crates/jet-codegen/src/Prelude/LocalCell.rs");
    // f855ee91f (2026-08-06) stated LocalCell's optional-slot policy over the
    // one outcome carrier: `T?` is `JetOutcome<T, JetAbsent>`
    // (D-FAIL-CARRIER1=A, crates/jet-foundation/src/Outcome.rs:15-22). This
    // harness compiles LocalCell.rs on its own, so it has to supply that
    // carrier — and supplying a DIFFERENT optional shape is precisely the
    // second representation D-FAIL-CARRIER1 exists to remove. Take the
    // declarations from the canonical file verbatim and prove they are still
    // the canonical declarations, the same way the FFI bridge projects them
    // into its standalone crate (crates/jet-pkg-model/src/FFI.rs:1448-1463).
    let outcome = include_str!("../crates/jet-foundation/src/Outcome.rs");
    let carrier = "pub type JetOutcome<T, E> = Result<T, E>;";
    let absent_derive =
        "#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]";
    let absent = "pub struct JetAbsent;";
    for line in [carrier, absent_derive, absent] {
        assert!(
            outcome.contains(line),
            "the outcome carrier moved; this harness now declares a second shape: {line}"
        );
    }
    let harness = [
        "mod jet_cell {",
        carrier,
        absent_derive,
        absent,
        cell,
        "}",
        r#"
use jet_cell::JetCell;
use std::panic::{catch_unwind, AssertUnwindSafe};

#[derive(Clone)]
struct Pair {
    left: i64,
    right: i64,
}

#[test]
fn loans_release_after_every_exit() {
    let cell = JetCell::new(Pair { left: 1, right: 2 });
    let first = cell.guard_read();
    let second = cell.guard_read();
    assert!(catch_unwind(AssertUnwindSafe(|| cell.guard_edit())).is_err());
    drop(first);
    assert!(catch_unwind(AssertUnwindSafe(|| cell.guard_edit())).is_err());
    drop(second);

    let edit = cell.guard_edit();
    assert!(catch_unwind(AssertUnwindSafe(|| cell.guard_read())).is_err());
    drop(edit);

    assert!(catch_unwind(AssertUnwindSafe(|| {
        cell.read::<_, ()>(|_| panic!("read unwind"))
    }))
    .is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| {
        cell.edit::<_, ()>(|_| panic!("edit unwind"))
    }))
    .is_err());
    cell.edit(|pair| pair.left = 3);
    assert_eq!(cell.read(|pair| pair.left), 3);

    let guard = cell.guard_edit();
    assert!(catch_unwind(AssertUnwindSafe(|| {
        guard.edit(|_| guard.edit(|_| ()))
    }))
    .is_err());
    guard.set(Pair { left: 4, right: 5 });
    drop(guard);
    assert_eq!(cell.read(|pair| pair.left), 4);
}

#[test]
fn mapped_and_split_guards_keep_the_original_loan() {
    let cell = JetCell::new(Pair { left: 4, right: 5 });
    let mapped = cell.guard_read().map(|pair| &pair.left);
    assert_eq!(mapped.get(), 4);
    assert!(catch_unwind(AssertUnwindSafe(|| cell.guard_edit())).is_err());
    drop(mapped);

    let (left, right) = cell
        .guard_read()
        .split(|pair| (&pair.left, &pair.right));
    assert_eq!((left.get(), right.get()), (4, 5));
    drop(left);
    assert!(catch_unwind(AssertUnwindSafe(|| cell.guard_edit())).is_err());
    drop(right);

    let (left, right) = cell
        .guard_edit()
        .split(|pair| (&mut pair.left, &mut pair.right));
    left.set(7);
    right.set(8);
    drop(left);
    assert!(catch_unwind(AssertUnwindSafe(|| cell.guard_read())).is_err());
    drop(right);
    assert_eq!(cell.read(|pair| (pair.left, pair.right)), (7, 8));
}
"#,
    ]
    .join("\n");

    let root = common::unique_tmp("jet_local_cell_runtime");
    fs::create_dir_all(&root).unwrap();
    let source = root.join("cell_runtime.rs");
    let binary = root.join("cell_runtime_test");
    fs::write(&source, harness).unwrap();
    let compiled = Command::new("rustc")
        .args(["--edition=2021", "--test"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "Cell runtime harness failed to compile:\n{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let ran = Command::new(&binary).output().unwrap();
    assert!(
        ran.status.success(),
        "Cell runtime harness failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&ran.stdout),
        String::from_utf8_lossy(&ran.stderr)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn local_cell_guard_mapping_and_splitting_keep_projected_types() {
    let source = r#"
struct Pair { left: Int, right: Int }
fn inspect(cell: Cell<Pair>) {
    mapped :: cell.guard_read().map(pair -> pair.left)
    print(mapped.get())
}
fn edit_pair(cell: Cell<Pair>) {
    (left, right) :: cell.guard_edit().split(
        pair -> pair.left,
        pair -> pair.right
    )
    left.set(7)
    right.set(8)
}
fn run() {}
"#;
    let output = jet::compile(source).expect("guard projections must compile");
    assert!(
        output.rust.contains("JetCellReadGuard<i64>")
            && output.rust.contains("JetCellEditGuard<i64>"),
        "{}",
        output.rust
    );
}

#[test]
fn local_cell_guard_map_rejects_detached_values() {
    let source = r#"
struct Pair { left: Int, right: Int }
fn inspect(cell: Cell<Pair>) {
    _ :: cell.guard_read().map(pair -> pair.left + pair.right)
}
fn run() {}
"#;
    let diagnostics =
        jet::compile(source).expect_err("map must preserve the original dynamic loan");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0112" && diagnostic.what.contains("needs a field projection")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn local_cell_get_rejects_values_without_copy_semantics() {
    let source = r#"
fn inspect(cell: Cell<fn() Int>) {
    _ :: cell.get()
}
fn run() {}
"#;
    let diagnostics =
        jet::compile(source).expect_err("Cell.get must not defer a missing Clone bound to rustc");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0112" && diagnostic.what.contains("cannot copy its value")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn local_cell_edit_guard_split_rejects_overlapping_paths() {
    let source = r#"
struct Pair { left: Int, right: Int }
fn edit_pair(cell: Cell<Pair>) {
    _ :: cell.guard_edit().split(
        pair -> pair.left,
        pair -> pair.left
    )
}
fn run() {}
"#;
    let diagnostics =
        jet::compile(source).expect_err("split must prove projections disjoint in sema");
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E0112" && diagnostic.what.contains("projections overlap")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn mutate_required_at_call_site() {
    let src = r#"
fn touch(n: &Int) {
    print(n)
}

fn run() {
    x := 1
    touch(x)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0202"));
}

#[test]
fn move_non_clonable_is_hard_error() {
    let src = r#"
fn consume(item: ^NoClone) {
    print(0)
}

fn run() {
msg :: "hi"
    consume(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0201"));
}

#[test]
fn shared_auto_clone_in_loop_is_lint() {
    let src = r#"
fn noop(h: Shared<Int>) {
    print(0)
}

fn loop_user(h: Shared<Int>) {
    loop {
        noop(h)
    }
}

fn run() {
    print(0)
}
"#;
    let out = jet::compile(src).expect("should compile with lint");
    assert!(
        out.lints.iter().any(|d| d.code == "L0202"),
        "expected L0202 loop auto-clone lint"
    );
    // D-MEM1 S6: `Shared<T>` lowers to `jet_std::JetShared<T>` now, not a bare
    // `Arc<T>` — the auto-clone is a plain `.clone()` call (its own cheap-handle
    // `Clone` impl), not `Arc::clone(&…)`.
    assert!(out.rust.contains(").clone()"));
}

/// D-CONSTMARK1=A (syntax-decisions.md:1756), spelled out at spec.md:718-720:
/// a constant is the marked compile-time binding `@name :: value`, and
/// `#Static @` emits a Rust `static` so the value gets one stable address.
/// There is no unmarked file-level `limit :: 10` to attach to — the only
/// declaration forms at that scope are `@name ::`, `#Persist name :=`, and the
/// Output pair (crates/jet-parser/src/Parser/Items/imports_policy.rs:1275-1318).
///
/// The symbol name is fixed by the naming law, not by choice: `mangle` rewrites
/// the `@` mark to `ct_` and prefixes the one machine prefix `__jet_`
/// (crates/jet-foundation/src/Names.rs:565-571, D-NAME-SIGIL1 at
/// crates/jet-foundation/src/Syntax/predicates.rs:367), then `emit_const`
/// uppercases it (crates/jet-codegen/src/Codegen/Items.rs:2864). So `@limit`
/// can only ever become `__JET_CT_LIMIT`.
#[test]
fn const_address_taken_emits_static() {
    let src = r#"
#Static @limit :: 10

fn show(n: Int) {
    print(n)
}

fn run() {
    show(@limit)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("static __JET_CT_LIMIT"),
        "`#Static` should give the constant its own address: {}",
        out.rust
    );
    // The other half of the same law: without `#Static` the constant is
    // address-free — it folds into its use sites and emits no top-level item
    // (crates/jet-codegen/src/Codegen/Items.rs:2829). Without this the
    // assertion above would also pass on a build that emitted a symbol for
    // every constant, which is the opposite of what the marker is for.
    let inlined = jet::compile(&src.replace("#Static ", "")).expect("should compile");
    assert!(
        !inlined.rust.contains("__JET_CT_LIMIT"),
        "an unmarked constant should inline, not take an address: {}",
        inlined.rust
    );
}

#[test]
fn same_call_mut_and_read_is_error() {
    let src = r#"
fn both(a: &Int, b: Int) {
    print(b)
}

fn run() {
    x := 1
    both(&x, x)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0204"));
}

#[test]
fn nested_call_argument_cannot_read_an_active_write_place() {
    let src = r#"
fn see(x: Int) Int -[]> { return x }
fn both(a: &Int, b: Int) { a += b }

fn run() {
    x := 1
    both(&x, see(x))
}
"#;
    let diags = jet::compile(src).expect_err("nested read must fail in sema before rustc");
    assert!(
        diags.iter().any(|diag| diag.code == "E0204"),
        "expected the call-alias diagnostic: {diags:?}"
    );
}

#[test]
fn composite_argument_cannot_read_an_active_write_place() {
    let src = r#"
fn both(a: &Int, b: Int) { a += b }

fn run() {
    x := 1
    both(&x, x + 1)
}
"#;
    let diags = jet::compile(src).expect_err("composite read must fail in sema before rustc");
    assert!(
        diags.iter().any(|diag| diag.code == "E0204"),
        "expected the call-alias diagnostic: {diags:?}"
    );
}

#[test]
fn lambda_capture_cannot_read_an_active_write_place() {
    let src = r#"
fn both(a: &String, callback: fn() String) { print(a); print(callback()) }

fn run() {
    x := "jet"
    both(&x, () -> x)
}
"#;
    let diags = jet::compile(src).expect_err("lambda capture must fail in sema before rustc");
    assert!(
        diags.iter().any(|diag| diag.code == "E0204"),
        "expected the call-alias diagnostic: {diags:?}"
    );
}

#[test]
fn inferred_mutable_lambda_capture_stays_active_through_call() {
    let src = r#"
fn both(callback: fn(), values: [Int]) {
    callback()
    print(values.len())
}

fn run() {
    values := [1, 2]
    both(() -> values.push(3), values)
}
"#;
    let diags =
        jet::compile(src).expect_err("mutable lambda capture must remain active through the call");
    assert!(
        diags.iter().any(|diag| diag.code == "E0204"),
        "expected the call-alias diagnostic: {diags:?}"
    );
}

#[test]
fn nested_lambda_forms_use_the_enclosing_call_access_frame() {
    let composite = r#"
struct Work { callback: fn() Int }
fn both(values: &[Int], work: Work) { values.push(work.callback()) }

fn run() {
    values := [1, 2]
    both(&values, Work{ callback: () -> values.len() })
}
"#;
    let diags = jet::compile(composite)
        .expect_err("a lambda inside an aggregate must see the active write");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let immediate_callee = r#"
fn both(values: &[Int], count: Int) { values.push(count) }

fn run() {
    values := [1, 2]
    both(&values, (() -> values.len()).call())
}
"#;
    let diags = jet::compile(immediate_callee)
        .expect_err("an immediate lambda callee must see the outer active write");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
}

#[test]
fn move_and_fnmut_lambda_captures_have_exact_lifetimes_and_places() {
    let copy_move = r#"
fn both(callback: fn() Int, value: &Int) { value += callback() }
fn run() {
    value := 1
    both(() -> value, &value)
}
"#;
    jet::compile(copy_move).expect("a read-only move closure copies a scalar before the write");

    let disjoint = r#"
struct Pair { left: [Int], right: [Int] }
fn both(callback: fn(), values: [Int]) { callback(); print(values.len()) }
fn run() {
    pair := Pair{ left: [1], right: [2] }
    both(() -> pair.right.push(3), pair.left)
}
"#;
    jet::compile(disjoint).expect("Rust 2021 captures disjoint struct fields separately");

    let conflicting = r#"
struct Pair { left: [Int], right: [Int] }
fn both(callback: fn(), values: [Int]) { callback(); print(values.len()) }
fn run() {
    pair := Pair{ left: [1], right: [2] }
    both(() -> pair.right.push(3), pair.right)
}
"#;
    let diags =
        jet::compile(conflicting).expect_err("the same captured field must remain write-borrowed");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
}

#[test]
fn move_lambda_capture_identity_matches_rust_2021_places() {
    let disjoint_owned_field = r#"
struct Pair { left: String, right: [Int] }
fn both(callback: fn(), values: [Int]) { callback(); print(values.len()) }
fn run() {
    pair := Pair{ left: "jet", right: [1, 2] }
    both(() -> { print(pair.left) }, pair.right)
}
"#;
    jet::compile(disjoint_owned_field)
        .expect("moving one captured field must leave a disjoint field usable");

    let copy_field = r#"
struct Pair { count: Int, values: [Int] }
fn both(callback: fn(), pair: Pair) { callback(); print(pair.count) }
fn run() {
    pair := Pair{ count: 2, values: [1, 2] }
    both(() -> { print(pair.count) }, pair)
}
"#;
    jet::compile(copy_field).expect("capturing a Copy field must not move its owner");

    let view_alias = r#"
fn call(callback: fn()) { callback() }
fn run() {
    values := [1, 2]
    first :: values[0..1]
    call(() -> { print(first.len()) })
    print(values.len())
}
"#;
    jet::compile(view_alias).expect("a closure captures a View alias, not its source owner");

    let same_owned_field = r#"
struct Pair { left: String, right: [Int] }
fn both(callback: fn(), text: String) { callback(); print(text) }
fn run() {
    pair := Pair{ left: "jet", right: [1, 2] }
    both(() -> { print(pair.left) }, pair.left)
}
"#;
    let diags =
        jet::compile(same_owned_field).expect_err("the moved captured field cannot be reused");
    assert!(diags.iter().any(|diag| diag.code == "E0121"), "{diags:?}");

    let conflicting_view_alias = r#"
fn both(values: &[Int], callback: fn()) { values.push(3); callback() }
fn run() {
    values := [1, 2]
    first :: values[0..1]
    both(&values, () -> { print(first.len()) })
}
"#;
    let diags = jet::compile(conflicting_view_alias)
        .expect_err("a copied View alias still borrows its source place");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let reverse_view_alias = r#"
fn both(callback: fn() Int, values: &[Int]) { values.push(callback()) }
fn run() {
    values := [1, 2]
    first :: values[0..1]
    both(() -> first.len(), &values)
}
"#;
    let diags = jet::compile(reverse_view_alias)
        .expect_err("a closure keeps its copied View alias live across the whole call");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
}

#[test]
fn move_lambda_index_and_slice_captures_stop_at_the_rust_prefix() {
    for source in [
        r#"
fn call(callback: fn()) { callback() }
fn run() {
    values := [1, 2]
    call(() -> { print(values[0]) })
    print(values.len())
}
"#,
        r#"
fn call(callback: fn()) { callback() }
fn run() {
    values := [1, 2]
    call(() -> { print(values[0..1].len()) })
    print(values.len())
}
"#,
    ] {
        let diags =
            jet::compile(source).expect_err("indexing cannot make a move capture element-precise");
        assert!(diags.iter().any(|diag| diag.code == "E0121"), "{diags:?}");
    }

    let field_before_index = r#"
struct Pair { left: [Int], right: [Int] }
fn call(callback: fn()) { callback() }
fn run() {
    pair := Pair{ left: [1], right: [2] }
    call(() -> { print(pair.left[0]) })
    print(pair.right.len())
}
"#;
    jet::compile(field_before_index).expect("Rust may capture the field prefix before an index");
}

#[test]
fn moved_places_can_be_reinitialized_without_clearing_relatives() {
    let whole = r#"
fn consume(value: String) { print(value) }
fn run() {
    value := "jet"
    consume(value)
    value = "again"
    print(value)
}
"#;
    jet::compile(whole).expect("whole assignment reinitializes the moved binding");

    let field = r#"
struct Pair { left: String, right: [Int] }
fn call(callback: fn()) { callback() }
fn run() {
    pair := Pair{ left: "jet", right: [1] }
    call(() -> { print(pair.left) })
    pair.left = "again"
    print(pair.left)
    print(pair.right.len())
}
"#;
    jet::compile(field).expect("field assignment reinitializes the exact moved field");

    let sibling = r#"
struct Pair { left: String, right: [Int] }
fn call(callback: fn()) { callback() }
fn run() {
    pair := Pair{ left: "jet", right: [1] }
    call(() -> { print(pair.left) })
    pair.right = [2]
    print(pair.left)
}
"#;
    let diags =
        jet::compile(sibling).expect_err("assigning a sibling must not restore the moved field");
    assert!(diags.iter().any(|diag| diag.code == "E0121"), "{diags:?}");

    let ancestor = r#"
struct Pair { left: String, right: [Int] }
fn call(callback: fn()) { callback() }
fn run() {
    pair := Pair{ left: "jet", right: [1] }
    call(() -> { print(pair) })
    pair.left = "again"
}
"#;
    let diags =
        jet::compile(ancestor).expect_err("assigning a field cannot restore a moved ancestor");
    assert!(diags.iter().any(|diag| diag.code == "E0121"), "{diags:?}");
}

#[test]
fn builtin_and_composite_receivers_use_the_call_access_frame() {
    let builtin = r#"
fn run() {
    values := [1, 2]
    values.insert(0, values.remove(0, .Slot))
}
"#;
    let diags = jet::compile(builtin)
        .expect_err("a nested builtin receiver must see the outer receiver write");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let composite = r#"
fn both(value: &Int, count: Int) { value += count }
fn run() {
    value := 1
    both(&value, [value].len())
}
"#;
    let diags =
        jet::compile(composite).expect_err("a composite receiver must evaluate inside the call");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
}

#[test]
fn if_expression_prefix_reads_use_the_call_access_frame() {
    let src = r#"
fn both(value: &Int, count: Int) { value += count }
fn run() {
    value := 1
    both(&value, if true -> { seen :: value; seen } else -> { 0 })
}
"#;
    let diags =
        jet::compile(src).expect_err("an if-expression prefix read must see the active write");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
}

#[test]
fn deferred_lambda_capture_reports_once() {
    let src = r#"
fn see(value: String) String -[]> { return value }
fn both(value: &String, callback: fn() String) { print(value); print(callback()) }
fn run() {
    value := "jet"
    both(&value, () -> see(value))
}
"#;
    let diags =
        jet::compile(src).expect_err("the deferred capture conflicts with the active write");
    assert_eq!(
        diags.iter().filter(|diag| diag.code == "E0204").count(),
        1,
        "the post-inference capture summary must be the only report: {diags:?}"
    );
}

#[test]
fn evaluated_statement_accesses_are_scoped_and_mode_aware() {
    let write = r#"
fn both(values: [Int], count: Int) { print(values.len() + count) }
fn run() {
    values := [1, 2]
    both(values, if true -> { values = [3]; 1 } else -> { 0 })
}
"#;
    let diags =
        jet::compile(write).expect_err("an if-prefix assignment must conflict with the read loan");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let shadow = r#"
fn both(value: &Int, callback: fn(Int) Int) { value += callback(2) }
fn run() {
    value := 1
    both(&value, (value: Int) -> value)
}
"#;
    jet::compile(shadow)
        .expect("a lambda-local shadow must not capture the outer write-borrowed place");
}

#[test]
fn lambda_capture_access_is_projection_specific_and_transitive() {
    let mixed = r#"
struct Pair { left: [Int], right: [Int] }
fn both(callback: fn(), values: [Int]) { callback(); print(values.len()) }
fn run() {
    pair := Pair{ left: [1], right: [2] }
    both(() -> { print(pair.left.len()); pair.right.push(3) }, pair.left)
}
"#;
    jet::compile(mixed).expect("reading left and mutating right keeps projection modes separate");

    let conflict = r#"
struct Pair { left: [Int], right: [Int] }
fn both(callback: fn(), values: [Int]) { callback(); print(values.len()) }
fn run() {
    pair := Pair{ left: [1], right: [2] }
    both(() -> { print(pair.left.len()); pair.right.push(3) }, pair.right)
}
"#;
    let diags = jet::compile(conflict).expect_err("the mutated projection remains write-borrowed");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let transitive = r#"
fn both(value: &String, callback: fn() fn() String) {
    print(value)
    print(callback().call())
}
fn run() {
    value := "jet"
    both(&value, () -> () -> value)
}
"#;
    let diags =
        jet::compile(transitive).expect_err("a nested closure capture is also an outer capture");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
}

#[test]
fn composite_lambda_capture_walks_if_prefix_and_fallback_values() {
    let if_prefix = r#"
struct Work { callback: fn() Int }
fn both(values: &[Int], work: Work) { values.push(work.callback()) }
fn run() {
    values := [1, 2]
    both(&values, if true -> {
        work :: Work{ callback: () -> values.len() }
        work
    } else -> {
        Work{ callback: () -> 0 }
    })
}
"#;
    let diags =
        jet::compile(if_prefix).expect_err("a lambda created in an if prefix must be summarized");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let fallback = r#"
fn both(values: &[Int], callback: fn() Int) { values.push(callback()) }
fn run() {
    values := [1, 2]
    both(&values, Val(() -> values.len()) ?? () -> 0)
}
"#;
    let diags = jet::compile(fallback)
        .expect_err("a lambda created through fallback evaluation must retain its capture");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
}

#[test]
fn builtin_mut_receiver_uses_two_phase_reservation() {
    let accepted = r#"
fn run() {
    values := [1, 2]
    values.push(values.len())
}
"#;
    jet::compile(accepted).expect("a shared argument read is allowed during receiver reservation");

    let rejected = r#"
fn run() {
    values := [1, 2]
    values.push(values.remove(0, .Slot))
}
"#;
    let diags =
        jet::compile(rejected).expect_err("a nested write conflicts with receiver reservation");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    for eager in [
        r#"
fn run() {
    values := [2, 1]
    values.sort_by((value: Int) -> value + values.len())
}
"#,
        r#"
fn run() {
    clock := Clock.new(0)
    clock.advance(clock.now())
}
"#,
        r#"
use core.compute.solve as solve
fn run() {
    solver := solve.Solver.new(1)
    solver.require(solver.failure_count() == 0)
}
"#,
    ] {
        let diags = jet::compile(eager).expect_err("explicit-helper receiver borrows are eager");
        assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
    }
}

#[test]
fn semantic_capture_events_drive_retention_and_deferred_clone_modes() {
    let field_write = r#"
struct Bucket { values: [Int] }
fn both(callback: fn(), values: [Int]) { callback(); print(values.len()) }
fn run() {
    bucket := Bucket{ values: [1, 2] }
    both(() -> { bucket.values = [3] }, bucket.values)
}
"#;
    let diags =
        jet::compile(field_write).expect_err("a captured field assignment stays write-borrowed");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let user_method = r#"
struct Bucket { values: [Int] }
impl Bucket {
    fn clear(&self) { self.values = [0] }
}
fn both(callback: fn(), values: [Int]) { callback(); print(values.len()) }
fn run() {
    bucket := Bucket{ values: [1, 2] }
    both(() -> bucket.clear(), bucket.values)
}
"#;
    let diags = jet::compile(user_method)
        .expect_err("a captured user mutable receiver stays write-borrowed");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let reactive_clone = r#"
fn both(callback: fn(), values: &[Int]) { callback(); values.push(4) }
fn run() {
    values := [1, 2]
    both(() -> {
        #Reactive { values.push(3) }
    }, &values)
}
"#;
    let diags = jet::compile(reactive_clone)
        .expect_err("the outer move closure consumes the reactive source before the later loan");
    assert!(!diags.is_empty(), "expected a move or alias diagnostic");
}

#[test]
fn pattern_value_tests_and_reactive_clones_use_runtime_capture_places() {
    let pattern_value = r#"
struct Incident { count: Int, label: String }
fn both(values: &[Int], callback: fn()) { values.push(3); callback() }
fn run() {
    values := [1, 2]
    changed := [0]
    incident := Incident{ count: 2, label: "jet" }
    both(&values, () -> {
        changed.push(1)
        if incident == {
            { count: values.len(), label, .. } -> print(label)
            else -> {}
        }
    })
}
"#;
    let diags = jet::compile(pattern_value)
        .expect_err("a struct-pattern value expression is evaluated before its arm");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let reactive_root = r#"
struct Pair { left: [Int], right: [Int] }
fn both(values: &[Int], callback: fn()) { values.push(3); callback() }
fn run() {
    pair := Pair{ left: [1], right: [2] }
    changed := [0]
    both(&pair.right, () -> {
        changed.push(1)
        #Reactive { pair.left.push(3) }
    })
}
"#;
    let diags =
        jet::compile(reactive_root).expect_err("reactive lowering clones the whole captured root");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
}

#[test]
fn reactive_root_capture_replaces_existing_owner_projections() {
    let src = r#"
struct Pair { left: [Int], right: [Int] }
fn both(values: &[Int], callback: fn()) { values.push(3); callback() }
fn run() {
    pair := Pair{ left: [1], right: [2] }
    both(&pair.right, () -> {
        print(pair.left.len())
        #Reactive { pair.right.push(4) }
    })
}
"#;
    let diags =
        jet::compile(src).expect_err("Reactive clones the root after an earlier field capture");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let write_then_reactive = r#"
struct Pair { left: [Int], right: [Int] }
fn both(values: &[Int], callback: fn()) { values.push(3); callback() }
fn run() {
    pair := Pair{ left: [1], right: [2] }
    both(&pair.right, () -> {
        pair.left.push(4)
        #Reactive { print(pair.right.len()) }
    })
}
"#;
    let diags = jet::compile(write_then_reactive)
        .expect_err("Reactive coarsening must preserve an earlier mutable capture");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");
}

#[test]
fn reactive_codegen_clones_the_whole_root_before_running() {
    let source = r#"
struct Pair { left: [Int], right: [Int] }
fn run() {
    pair := Pair{ left: [1], right: [2] }
    #Reactive {
        pair.left.push(3)
        print(pair.left.len())
    }
    print(pair.right.len())
}
"#;
    let compiled = jet::compile(source).expect("direct Reactive cloning must remain valid");
    assert!(
        compiled
            .rust
            .contains("let mut __jet___cap_pair = (__jet_pair).clone();"),
        "{}",
        compiled.rust
    );
    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_reactive_capture", "whole_root", source);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "2\n1\n");
    }
}

#[test]
fn reactive_capture_materializes_local_views_before_rustc() {
    // `a..b` is inclusive (spec.md:127), so the window has to be narrower than
    // the owner for the length to prove anything: `jet_view_copy` must copy the
    // WINDOW, not the root. `values[0..1]` is the whole list and would pass on
    // either behaviour.
    let source = r#"
fn run() {
    values := [1, 2]
    first :: values[0..0]
    #Reactive { print(first.len()) }
}
"#;
    let compiled = jet::compile(source).expect("a stored read View gets an owning copy");
    assert!(compiled.rust.contains("jet_view_copy"), "{}", compiled.rust);
    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_reactive_view_capture", "whole_root", source);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "1\n");
    }
}

#[test]
fn move_lambda_construction_consumes_owned_nonscalar_captures() {
    for (source, expected) in [
        (
            r#"
fn both(callback: fn(), values: [Int]) { callback(); print(values.len()) }
fn run() {
    values := [1, 2]
    both(() -> values.len(), values)
}
"#,
            "E0121",
        ),
        (
            r#"
fn both(values: [Int], callback: fn()) { print(values.len()); callback() }
fn run() {
    values := [1, 2]
    both(values, () -> values.len())
}
"#,
            "E0204",
        ),
    ] {
        let diags =
            jet::compile(source).expect_err("a move closure consumes its owned non-scalar capture");
        assert!(
            diags.iter().any(|diag| diag.code == expected),
            "{expected}: {diags:?}"
        );
    }

    let scalar_copy = r#"
fn both(callback: fn(), value: Int) { callback(); print(value) }
fn run() {
    value := 2
    both(() -> { print(value) }, value)
}
"#;
    jet::compile(scalar_copy).expect("Copy scalar captures remain usable");
}

#[test]
fn semantic_capture_walker_covers_fallback_and_scope_member_arguments() {
    let fallback = r#"
fn missing() ?Int -[]> { return null }
fn both(values: &[Int], callback: fn()) { values.push(3); callback() }
fn run() {
    values := [1, 2]
    both(&values, () -> {
        found :: missing() ?? panic("length {values.len()}")
        print(found)
    })
}
"#;
    let diags = jet::compile(fallback).expect_err("panic fallback arguments are closure captures");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let scope_member = r#"
fn run() { print(0) }
#Test("member argument capture walk") {
    values := [1, 2]
    .skip("length {values.len()}") { assert(false) }
}
"#;
    jet::compile(scope_member).expect("valid scope-member arguments remain walkable");
}

#[test]
fn wrapped_and_branch_returned_views_stay_live_through_outer_calls() {
    for view in [
        "(first(values))",
        "if true -> { first(values) } else -> { first(values) }",
        "Val(first(values)) ?? first(values)",
    ] {
        let src = format!(
            r#"
fn first(values: [Int]) View<Int> -[]> {{
    return values[0..1]
}}
fn both(view: View<Int>, values: &[Int]) {{
    values.push(view[0])
}}
fn run() {{
    values := [1, 2]
    both({view}, &values)
}}
"#
        );
        let diags =
            jet::compile(&src).expect_err("wrapped returned views retain their source loan");
        assert!(
            diags.iter().any(|diag| diag.code == "E0204"),
            "{view}: {diags:?}"
        );
    }
}

#[test]
fn return_fallback_view_does_not_reach_the_enclosing_call() {
    let source = r#"
struct View<T> { value: T }
fn first(values: [Int]) View<Int> -[]> { return values[0..1] }
fn both(view: View<Int>, values: &[Int]) { values.push(view[0]) }
fn choose(other: [Int], values: &[Int]) View<Int> -[..E]> {
    both(Val(first(other)) ?? return first(values), &values)
    return first(other)
}
fn run() {
    other := [1]
    values := [2]
    chosen :: choose(other, &values)
    print(chosen[0])
}
"#;
    jet::compile(source).expect("a return fallback exits before the enclosing call can run");
}

#[test]
fn generic_constructor_nested_argument_sees_active_write_place() {
    let src = r#"
struct Pair<T> { value: T }
impl Pair {
    fn new(first: &T, second: T) Pair<T> -[]> {
        return Pair<T>{ value: second }
    }
}
fn see(value: Int) Int -[]> { return value }

fn run() {
    x := 1
    pair :: Pair.new(&x, see(x))
    print(pair.value)
}
"#;
    let diags =
        jet::compile(src).expect_err("constructor pre-inference must fail in sema before rustc");
    assert!(
        diags.iter().any(|diag| diag.code == "E0204"),
        "expected the call-alias diagnostic: {diags:?}"
    );
}

#[test]
fn dynamic_projection_operands_are_checked_but_not_retained() {
    let hostile_index = r#"
fn both(index: &Int, value: Int) { index += value }
fn run() {
    values := [10, 20]
    index := 0
    both(&index, values[index])
}
"#;
    let diags = jet::compile(hostile_index)
        .expect_err("dynamic index evaluation must fail in sema before rustc");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let hostile_slice = r#"
fn both(end: &Int, values: [Int]) { end += values.len() }
fn run() {
    values := [10, 20]
    end := 1
    both(&end, values[0..end])
}
"#;
    let diags = jet::compile(hostile_slice)
        .expect_err("dynamic slice-bound evaluation must fail in sema before rustc");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let ordered = r#"
fn both(value: Int, index: &Int) { index += value }
fn run() {
    values := [10, 20]
    index := 0
    both(values[index], &index)
}
"#;
    jet::compile(ordered)
        .expect("the index read finishes before the later write borrow is retained");
}

#[test]
fn whole_place_alias_blocks_overlapping_write_argument() {
    let src = r#"
fn both(a: &[Int], b: [Int]) {
    a[0] = b[0]
}

fn run() {
    xs := [1, 2, 3]
    alias :: xs
    both(&xs, alias)
}
"#;
    let diags =
        jet::compile(src).expect_err("whole-place read alias must fail in sema before rustc");
    let diag = diags.iter().find(|diag| diag.code == "E0204");
    assert!(
        diag.is_some(),
        "expected the call-alias diagnostic: {diags:?}"
    );
    let diag = diag.unwrap();
    assert!(
        diag.what.starts_with("`xs`"),
        "the alias must resolve through the view fact graph to its owner: {diag:?}"
    );
}

#[test]
fn rustc_oracle_rejects_the_hostile_borrow_shapes() {
    if !common::have_rustc() {
        return;
    }
    let root = common::unique_tmp("jet_call_place_rustc_oracle");
    fs::create_dir_all(&root).unwrap();
    let cases = [
        (
            "nested.rs",
            r#"
fn see(x: i64) -> i64 { x }
fn both(a: &mut i64, b: i64) { *a += b }
fn main() {
    let mut x = 1;
    both(&mut x, see(x));
}
"#,
            "E0503",
        ),
        (
            "alias.rs",
            r#"
fn both(a: &mut Vec<i64>, b: &Vec<i64>) { a[0] = b[0] }
fn main() {
    let mut xs = vec![1, 2, 3];
    let alias = &xs;
    both(&mut xs, alias);
}
"#,
            "E0502",
        ),
        (
            "lambda.rs",
            r#"
fn both<F: FnOnce() -> String>(a: &mut String, callback: F) {
    println!("{a}");
    println!("{}", callback());
}
fn main() {
    let mut x = String::from("jet");
    both(&mut x, move || x);
}
"#,
            "E0505",
        ),
        (
            "constructor.rs",
            r#"
struct Pair<T>(T);
impl<T> Pair<T> {
    fn new(_first: &mut T, second: T) -> Self { Self(second) }
}
fn see(value: i64) -> i64 { value }
fn main() {
    let mut x = 1;
    let _ = Pair::new(&mut x, see(x));
}
"#,
            "E0503",
        ),
        (
            "mutable_capture_lifetime.rs",
            r#"
fn both<F: FnMut()>(mut callback: F, values: &Vec<i64>) {
    callback();
    println!("{}", values.len());
}
fn main() {
    let mut values = vec![1, 2];
    both(|| values.push(3), &values);
}
"#,
            "E0502",
        ),
        (
            "composite_lambda.rs",
            r#"
struct Work<F> { callback: F }
fn both<F: Fn() -> usize>(values: &mut Vec<i64>, work: Work<F>) {
    values.push((work.callback)() as i64);
}
fn main() {
    let mut values = vec![1, 2];
    both(&mut values, Work { callback: move || values.len() });
}
"#,
            "E0505",
        ),
        (
            "immediate_lambda_callee.rs",
            r#"
fn both(values: &mut Vec<i64>, count: usize) { values.push(count as i64) }
fn main() {
    let mut values = vec![1, 2];
    both(&mut values, (move || values.len())());
}
"#,
            "E0505",
        ),
        (
            "conflicting_capture_field.rs",
            r#"
struct Pair { left: Vec<i64>, right: Vec<i64> }
fn both<F: FnMut()>(mut callback: F, values: &Vec<i64>) {
    callback();
    println!("{}", values.len());
}
fn main() {
    let mut pair = Pair { left: vec![1], right: vec![2] };
    both(|| pair.right.push(3), &pair.right);
}
"#,
            "E0502",
        ),
        (
            "builtin_receiver.rs",
            r#"
fn main() {
    let mut values = vec![1, 2];
    values.insert(0, values.remove(0));
}
"#,
            "E0499",
        ),
        (
            "if_prefix_write.rs",
            r#"
fn both(values: &Vec<i64>, count: i64) { println!("{}", values.len() as i64 + count) }
fn main() {
    let mut values = vec![1, 2];
    both(&values, { values = vec![3]; 1 });
}
"#,
            "E0506",
        ),
        (
            "transitive_lambda.rs",
            r#"
fn both<F, G>(value: &mut String, callback: F)
where F: FnOnce() -> G, G: FnOnce() -> String
{
    println!("{value}");
    println!("{}", callback()());
}
fn main() {
    let mut value = String::from("jet");
    both(&mut value, move || move || value);
}
"#,
            "E0505",
        ),
        (
            "eager_helper_receiver.rs",
            r#"
fn advance(clock: &mut i64, to: i64) { *clock = to; }
fn now(clock: &i64) -> i64 { *clock }
fn main() {
    let mut clock = 0;
    advance(&mut clock, now(&clock));
}
"#,
            "E0502",
        ),
        (
            "eager_sort_capture.rs",
            r#"
fn sort_by<F: Fn(i64) -> i64>(values: &mut Vec<i64>, key: F) {
    values.sort_by_key(|value| key(*value));
}
fn main() {
    let mut values = vec![2, 1];
    sort_by(&mut values, |value| value + values.len() as i64);
}
"#,
            "E0502",
        ),
        (
            "wrapped_returned_view.rs",
            r#"
fn first(values: &Vec<i64>) -> &[i64] { &values[0..1] }
fn both(view: &[i64], values: &mut Vec<i64>) { values.push(view[0]); }
fn main() {
    let mut values = vec![1, 2];
    both((first(&values)), &mut values);
}
"#,
            "E0502",
        ),
        (
            "pattern_value_capture.rs",
            r#"
struct Incident { count: usize }
fn both<F: FnMut()>(values: &mut Vec<i64>, mut callback: F) {
    values.push(3);
    callback();
}
fn main() {
    let mut values = vec![1, 2];
    let mut changed = vec![0];
    let incident = Incident { count: 2 };
    both(&mut values, || {
        changed.push(1);
        if incident.count == values.len() {}
    });
}
"#,
            "E0502",
        ),
        (
            "reactive_whole_root_clone.rs",
            r#"
#[derive(Clone)]
struct Pair { left: Vec<i64>, right: Vec<i64> }
fn both<F: FnMut()>(values: &mut Vec<i64>, mut callback: F) {
    values.push(3);
    callback();
}
fn main() {
    let mut pair = Pair { left: vec![1], right: vec![2] };
    let mut changed = vec![0];
    both(&mut pair.right, || {
        changed.push(1);
        let captured_pair = pair.clone();
        let _effect = move || println!("{}", captured_pair.left.len());
    });
}
"#,
            "E0502",
        ),
        (
            "move_capture_before_read.rs",
            r#"
fn both<F: Fn() -> usize>(callback: F, values: &Vec<i64>) {
    println!("{} {}", callback(), values.len());
}
fn main() {
    let values = vec![1, 2];
    both(move || values.len(), &values);
}
"#,
            "E0382",
        ),
        (
            "read_before_move_capture.rs",
            r#"
fn both<F: Fn() -> usize>(values: &Vec<i64>, callback: F) {
    println!("{} {}", values.len(), callback());
}
fn main() {
    let values = vec![1, 2];
    both(&values, move || values.len());
}
"#,
            "E0505",
        ),
        (
            "same_move_capture_field.rs",
            r#"
struct Pair { left: String, right: Vec<i64> }
fn both<F: Fn()>(callback: F, text: String) { callback(); println!("{text}"); }
fn main() {
    let pair = Pair { left: "jet".to_string(), right: vec![1] };
    both(move || println!("{}", pair.left), pair.left);
}
"#,
            "E0382",
        ),
        (
            "captured_view_vs_write.rs",
            r#"
fn both<F: Fn()>(values: &mut Vec<i64>, callback: F) { values.push(3); callback(); }
fn main() {
    let mut values = vec![1, 2];
    let first = &values[0..1];
    both(&mut values, move || println!("{}", first.len()));
}
"#,
            "E0502",
        ),
        (
            "captured_view_vs_later_write.rs",
            r#"
fn both<F: Fn() -> usize>(callback: F, values: &mut Vec<i64>) {
    values.push(callback() as i64);
}
fn main() {
    let mut values = vec![1, 2];
    let first = &values[0..1];
    both(move || first.len(), &mut values);
}
"#,
            "E0502",
        ),
        (
            "indexed_move_capture.rs",
            r#"
fn call<F: Fn()>(callback: F) { callback(); }
fn main() {
    let values = vec![1, 2];
    call(move || println!("{}", values[0]));
    println!("{}", values.len());
}
"#,
            "E0382",
        ),
        (
            "sliced_move_capture.rs",
            r#"
fn call<F: Fn()>(callback: F) { callback(); }
fn main() {
    let values = vec![1, 2];
    call(move || println!("{}", values[0..1].len()));
    println!("{}", values.len());
}
"#,
            "E0382",
        ),
        (
            "if_prefix.rs",
            r#"
fn both(value: &mut i64, count: i64) { *value += count }
fn main() {
    let mut value = 1;
    both(&mut value, if true { let seen = value; seen } else { 0 });
}
"#,
            "E0503",
        ),
        (
            "composite_receiver.rs",
            r#"
fn both(value: &mut i64, count: usize) { *value += count as i64 }
fn main() {
    let mut value = 1;
    both(&mut value, vec![value].len());
}
"#,
            "E0503",
        ),
        (
            "dynamic_index.rs",
            r#"
fn both(index: &mut usize, value: i64) { *index += value as usize }
fn main() {
    let values = [10, 20];
    let mut index = 0;
    both(&mut index, values[index]);
}
"#,
            "E0503",
        ),
        (
            "slice_bound.rs",
            r#"
fn both(end: &mut usize, values: &[i64]) { *end += values.len() }
fn main() {
    let values = [10, 20];
    let mut end = 1;
    both(&mut end, &values[0..end]);
}
"#,
            "E0503",
        ),
    ];
    for (name, source, expected) in cases {
        let path = root.join(name);
        fs::write(&path, source).unwrap();
        let output = Command::new("rustc")
            .arg("--edition=2021")
            .arg(&path)
            .arg("-o")
            .arg(root.join(format!("{name}.bin")))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !output.status.success() && stderr.contains(expected),
            "native oracle must reject {name} with {expected}: {stderr}"
        );
    }
}

#[test]
fn rustc_oracle_accepts_move_copy_and_disjoint_field_captures() {
    if !common::have_rustc() {
        return;
    }
    let root = common::unique_tmp("jet_call_place_rustc_positive_oracle");
    fs::create_dir_all(&root).unwrap();
    let cases = [
        (
            "copy_move.rs",
            r#"
fn both<F: Fn() -> i64>(callback: F, value: &mut i64) { *value += callback() }
fn main() {
    let mut value = 1;
    both(move || value, &mut value);
}
"#,
        ),
        (
            "disjoint_field.rs",
            r#"
struct Pair { left: Vec<i64>, right: Vec<i64> }
fn both<F: FnMut()>(mut callback: F, values: &Vec<i64>) {
    callback();
    println!("{}", values.len());
}
fn main() {
    let mut pair = Pair { left: vec![1], right: vec![2] };
    both(|| pair.right.push(3), &pair.left);
}
"#,
        ),
        (
            "two_phase_receiver.rs",
            r#"
fn main() {
    let mut values = vec![1_i64, 2];
    values.push(values.len() as i64);
}
"#,
        ),
        (
            "mixed_projection_capture.rs",
            r#"
struct Pair { left: Vec<i64>, right: Vec<i64> }
fn both<F: FnMut()>(mut callback: F, values: &Vec<i64>) {
    callback();
    println!("{}", values.len());
}
fn main() {
    let mut pair = Pair { left: vec![1], right: vec![2] };
    both(|| {
        println!("{}", pair.left.len());
        pair.right.push(3);
    }, &pair.left);
}
"#,
        ),
        (
            "move_owned_field.rs",
            r#"
struct Pair { left: String, right: Vec<i64> }
fn both<F: Fn()>(callback: F, values: Vec<i64>) { callback(); println!("{}", values.len()); }
fn main() {
    let pair = Pair { left: "jet".to_string(), right: vec![1] };
    both(move || println!("{}", pair.left), pair.right);
}
"#,
        ),
        (
            "copy_field_owner.rs",
            r#"
struct Pair { count: i64, values: Vec<i64> }
fn both<F: Fn()>(callback: F, pair: Pair) { callback(); println!("{}", pair.count); }
fn main() {
    let pair = Pair { count: 2, values: vec![1] };
    both(move || println!("{}", pair.count), pair);
}
"#,
        ),
        (
            "view_alias_capture.rs",
            r#"
fn call<F: Fn()>(callback: F) { callback(); }
fn main() {
    let values = vec![1, 2];
    let first = &values[0..1];
    call(move || println!("{}", first.len()));
    println!("{}", values.len());
}
"#,
        ),
        (
            "indexed_disjoint_field.rs",
            r#"
struct Pair { left: Vec<i64>, right: Vec<i64> }
fn call<F: Fn()>(callback: F) { callback(); }
fn main() {
    let pair = Pair { left: vec![1], right: vec![2] };
    call(move || println!("{}", pair.left[0]));
    println!("{}", pair.right.len());
}
"#,
        ),
    ];
    for (name, source) in cases {
        let path = root.join(name);
        fs::write(&path, source).unwrap();
        let output = Command::new("rustc")
            .arg("--edition=2021")
            .arg(&path)
            .arg("-o")
            .arg(root.join(format!("{name}.bin")))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "native oracle must accept {name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn imported_call_uses_the_same_place_access_rule() {
    let root = common::unique_tmp("jet_imported_call_place_access");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("helper.jet"),
        "pub fn both(a: &Int, b: Int) { a += b }\n",
    )
    .unwrap();
    let entry = root.join("main.jet");
    let src = r#"
use "./helper" as helper

fn run() {
    x := 1
    helper.both(&x, x)
}
"#;
    fs::write(&entry, src).unwrap();
    let diags = jet::compile_with_path(src, entry.to_str().unwrap())
        .expect_err("imported call alias must fail in sema before rustc");
    assert!(
        diags.iter().any(|diag| diag.code == "E0204"),
        "expected the call-alias diagnostic: {diags:?}"
    );
}

#[test]
fn generic_method_trait_and_function_value_calls_share_place_access() {
    let hostile = [
        r#"
fn both<T>(a: &T, b: T) { print(0) }
fn run() {
    value := 1
    both(&value, value)
}
"#,
        r#"
struct Editor { id: Int }
impl Editor {
    fn clash(self, other: &Editor) { print(other.id) }
}
fn run() {
    editor := Editor{ id: 1 }
    editor.clash(&editor)
}
"#,
        r#"
trait Edit {
    fn clash(self, other: &Edit)
}
fn conflict(editor: &Edit) {
    editor.clash(&editor)
}
fn run() {}
"#,
        r#"
fn length(value: String) Int -[]> { return value.len() }
fn both(value: &String, count: Int) { print(value); print(count) }
fn run() {
    callback :: length
    value := "hello"
    both(&value, callback(value))
}
"#,
        r#"
module helper {
    pub fn both(a: &Int, b: Int) { a += b }
}
fn run() {
    value := 1
    helper.both(&value, value)
}
"#,
    ];
    for src in hostile {
        let diags = jet::compile(src).expect_err("every call form must reject one aliased place");
        assert!(
            diags.iter().any(|diag| diag.code == "E0204"),
            "expected the shared call-place diagnostic: {diags:?}"
        );
    }
}

#[test]
fn trait_object_mut_self_calls_use_receiver_change_rules_inside_callbacks() {
    let rejected = r#"
trait Edit {
    fn change(&self)
}
fn call(callback: fn()) { callback() }
fn invoke(editor: Edit) {
    call(() -> { editor.change() })
}
fn run() {}
"#;
    let diags = jet::compile(rejected)
        .expect_err("a trait-object mut-self callback capture needs edit access");
    assert!(diags.iter().any(|diag| diag.code == "E0202"), "{diags:?}");

    let accepted = r#"
trait Edit {
    fn change(&self)
}
fn call(callback: fn()) { callback() }
fn invoke(editor: &Edit) {
    call(() -> { editor.change() })
}
fn run() {}
"#;
    jet::compile(accepted).expect("an editable trait-object callback capture is valid");
}

#[test]
fn multi_trait_receiver_access_follows_first_match_dispatch_order() {
    let read_first = r#"
trait Inspect {
    fn touch(self)
}
trait Edit {
    fn touch(&self)
}
fn call(callback: fn()) { callback() }
fn inspect_all(items: ...[Inspect, Edit]) {
    loop item in items {
        call(() -> { item.touch() })
    }
}
fn run() {}
"#;
    jet::compile(read_first)
        .expect("the first matching read-self method controls dispatch and capture access");

    let write_first = r#"
trait Inspect {
    fn touch(self)
}
trait Edit {
    fn touch(&self)
}
fn call(callback: fn()) { callback() }
fn edit_all(items: ...[Edit, Inspect]) {
    loop item in items {
        call(() -> { item.touch() })
    }
}
fn run() {}
"#;
    let diags = jet::compile(write_first)
        .expect_err("the first matching write-self method needs edit access");
    assert!(diags.iter().any(|diag| diag.code == "E0202"), "{diags:?}");
}

#[test]
fn implicit_borrows_persist_but_finished_scalar_reads_do_not() {
    let borrowed = r#"
fn inspect_then_edit(values: [Int], edited: &[Int]) { print(values.len() + edited.len()) }
fn run() {
    values := [1, 2, 3]
    inspect_then_edit(values, &values)
}
"#;
    let diags = jet::compile(borrowed).expect_err("implicit read borrow must remain active");
    assert!(diags.iter().any(|diag| diag.code == "E0204"), "{diags:?}");

    let ordered = r#"
fn read_then_edit(value: Int, edited: &Int) { edited += value }
fn run() {
    value := 1
    read_then_edit(value, &value)
}
"#;
    jet::compile(ordered).expect("a completed scalar read may precede a write borrow");
}

#[test]
fn call_access_frames_do_not_leak_across_control_flow_or_disjoint_places() {
    let src = r#"
fn both(a: &Int, b: Int) { a += b }
fn run() {
    left := 1
    right := 2
    if true {
        both(&left, right)
    }
    both(&right, left)
}
"#;
    jet::compile(src).expect("separate calls on disjoint places must not share access frames");
}

/// E0209 liveness gate (was D-L0201): when the value is still used after the
/// call, `^` would break that later use — E0209 still fires (hard error), but
/// the fix menu offers `~name`/reorder instead of `^`.
#[test]
fn implicit_clone_errors_with_reorder_menu_when_live_after_call() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn run() {
msg :: "hello"
    consume(msg)
    print(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0209")
        .expect("expected E0209 implicit-clone hard error");
    assert!(
        d.fix.contains("reorder"),
        "live-after fix menu should suggest reordering, got: {}",
        d.fix
    );
}

/// E0209 liveness gate (was D-L0201): when the value IS dead after the call,
/// `^` is safe (this is its last use) — the fix menu leads with `^`.
#[test]
fn implicit_clone_errors_with_move_menu_when_dead_after_call() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn run() {
msg :: "hello"
    consume(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0209")
        .expect("expected E0209 implicit-clone hard error");
    assert!(
        d.fix.contains("^msg"),
        "dead-after fix menu should lead with `^msg`, got: {}",
        d.fix
    );
}

#[test]
fn deref_outside_unsafe_is_error() {
    let src = r#"
fn run() {
x :: 1
    print(*x)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0208"));
}

/// E0209 liveness gate (was D-L0201): a clone inside a nested `if` block gets
/// the reorder/copy menu (not the `^`-leads menu) when the value is used in
/// the enclosing block after the `if`. `is_name_live_after` checks the
/// current block's tail AND all enclosing scopes — missing enclosing scopes
/// would wrongly advise `^msg` here, which would use-after-move the
/// `print(msg)` below.
#[test]
fn implicit_clone_uses_reorder_menu_when_live_in_enclosing_block() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn maybe(b: Bool) Bool -[]> { return b }

fn run() {
msg :: "hello"
    if maybe(true) {
        consume(msg)
    }
    print(msg)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0209")
        .expect("expected E0209 implicit-clone hard error");
    assert!(
        d.fix.contains("reorder"),
        "live-in-enclosing-block fix menu should suggest reordering, got: {}",
        d.fix
    );
}

/// E0209 liveness gate (was D-L0201): a clone inside a nested block where the
/// value is genuinely dead everywhere after (enclosing block included) gets
/// the `^`-leads menu.
#[test]
fn implicit_clone_uses_move_menu_when_dead_in_all_enclosing_blocks() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn maybe(b: Bool) Bool -[]> { return b }

fn run() {
msg :: "hello"
    if maybe(true) {
        consume(msg)
    }
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    let d = diags
        .iter()
        .find(|d| d.code == "E0209")
        .expect("expected E0209 implicit-clone hard error");
    assert!(
        d.fix.contains("^msg"),
        "dead-in-all-enclosing-blocks fix menu should lead with `^msg`, got: {}",
        d.fix
    );
}

/// D-MEM1 S9 / #649: owner storage cannot move while a read view still points
/// into it. Jet must reject before TIR/rustc.
#[test]
fn moving_list_owner_with_live_view_is_error() {
    let src = r#"
fn consume(xs: ^[Int]) {
    print(xs.len())
}

fn run() {
    xs := [1, 2, 3]
    window :: xs[0..1]
    consume(^xs)
    print(window.len())
}
"#;
    let diags = jet::compile(src).expect_err("moving a viewed owner must fail");
    assert!(diags.iter().any(|d| d.code == "E0212"));
}

/// D-SHAPE-PLACE1=A (#613): a bare range place is the read-window spelling.
/// Moving its owner must fail in sema before TIR/rustc.
#[test]
fn moving_list_owner_with_bare_range_window_is_error() {
    let src = r#"
fn consume(xs: ^[Int]) {
    print(xs.len())
}

fn run() {
    xs := [1, 2, 3]
    window :: xs[0..1]
    consume(^xs)
    print(window.len())
}
"#;
    let diags = jet::compile(src).expect_err("moving a windowed owner must fail");
    assert!(diags.iter().any(|d| d.code == "E0212"), "got {diags:?}");
}

/// D-SHAPE-PLACE1=A (#613): `&place` creates the exclusive write window.
/// A later overlapping read window is rejected by the unified #649 graph.
#[test]
fn write_range_window_conflicts_with_overlapping_read_window() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    edit :: &xs[0..1]
    read :: xs[1..2]
    print(edit.len() + read.len())
}
"#;
    let diags = jet::compile(src).expect_err("overlapping write/read windows must fail");
    assert!(diags.iter().any(|d| d.code == "E0212"), "got {diags:?}");
}

#[test]
fn bare_range_window_lowers_without_copy() {
    let src = r#"
fn run() {
    xs :: [1, 2, 3]
    band :: 0..1
    window :: xs[band]
    print(window[1])
}
"#;
    let out = jet::compile(src).expect("bare range window must compile");
    assert!(
        out.rust.contains("let __jet_window = jet_view_range_new"),
        "range acquisition must lower to a view: {}",
        out.rust
    );
}

#[test]
fn whole_place_window_lowers_as_borrow() {
    let src = r#"
fn run() {
    xs :: [1, 2, 3]
    all :: xs
    print(all.len())
}
"#;
    let out = jet::compile(src).expect("whole place window must compile");
    assert!(
        out.rust.contains("let __jet_all = &(__jet_xs)"),
        "{}",
        out.rust
    );
}

#[test]
fn copied_range_is_owned_and_does_not_borrow_owner() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    copied :: ~xs[0..1]
    xs.push(4)
    print(copied.len() + xs.len())
}
"#;
    jet::compile(src).expect("`~range` must stay an independent owned copy");
}

#[test]
fn write_range_window_edits_owner() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    band :: 0..1
    edit :: &xs[band]
    edit[1] = 9
    print(xs[1])
}
"#;
    let out = jet::compile(src).expect("write range window must compile");
    assert!(out.rust.contains("jet_view_mut_range_new"), "{}", out.rust);
}

#[test]
fn write_windows_edit_whole_field_and_index_places() {
    let src = r#"
struct Slot { value: Int }
fn run() {
    whole := 1
    whole_edit :: &whole
    whole_edit = 2

    cell := Slot{ value: 3 }
    field_edit :: &cell.value
    field_edit = 4

    xs := [5, 6]
    index_edit :: &xs[0]
    index_edit = 7
    print(whole + cell.value + xs[0])
}
"#;
    let out = jet::compile(src).expect("write windows must write through to each owner place");
    assert!(
        out.rust.contains("(*__jet_whole_edit) = 2i64"),
        "{}",
        out.rust
    );
    assert!(
        out.rust.contains("(*__jet_field_edit) = 4i64"),
        "{}",
        out.rust
    );
    assert!(
        out.rust.contains("(*__jet_index_edit) = 7i64"),
        "{}",
        out.rust
    );
}

#[test]
fn write_window_requires_mutable_owner_or_write_parameter() {
    let local = r#"
fn run() {
    xs :: [1, 2]
    edit :: &xs[0]
    print(edit)
}
"#;
    let diags = jet::compile(local).expect_err("immutable local must reject write window");
    assert!(diags.iter().any(|d| d.code == "E0202"), "{diags:?}");

    let parameter = r#"
fn edit(xs: [Int]) { window :: &xs[0]; print(window) }
fn run() { print(0) }
"#;
    let diags = jet::compile(parameter).expect_err("read parameter must reject write window");
    assert!(diags.iter().any(|d| d.code == "E0205"), "{diags:?}");
}

#[test]
fn indexed_window_conflict_ends_at_last_use() {
    let src = r#"
fn run() {
    xs := [1, 2]
    read :: xs[0..0]
    print(read[0])
    edit :: &xs[0..0]
    edit[0] = 9
    print(xs[0])
}
"#;
    jet::compile(src).expect("last use must end the earlier window conflict");
}

#[test]
fn write_mark_stops_at_maximal_place_before_method_chain() {
    let src = r#"
fn run() {
    xs := [1, 2]
    n :: &xs[0..1].len()
    print(n)
}
"#;
    let out = jet::compile(src).expect("method must receive the maximal write-window place");
    assert!(out.rust.contains("jet_view_mut_new"), "{}", out.rust);
}

#[test]
fn write_window_rejects_call_result_place() {
    let src = r#"
fn make() [Int] -[]> { return [1, 2] }
fn run() { edit :: &make()[0]; print(edit) }
"#;
    let diags = jet::compile(src).expect_err("call result has no stable owner place");
    assert!(diags.iter().any(|d| d.code == "E0213"), "{diags:?}");
}

#[test]
fn disjoint_read_write_ranges_lower_to_safe_split() {
    let src = r#"
fn run() {
    xs := [1, 2, 3, 4]
    edit :: &xs[0..1]
    read :: xs[2..3]
    edit[0] = 9
    print(read[0])
}
"#;
    let out = jet::compile(src).expect("disjoint read/write windows must compile");
    assert!(out.rust.contains("split_at_mut"), "{}", out.rust);
}

#[test]
fn disjoint_write_ranges_lower_to_safe_split() {
    let src = r#"
fn run() {
    xs := [1, 2, 3, 4]
    left :: &xs[0..1]
    right :: &xs[2..3]
    left[0] = 8
    right[0] = 9
    print(left[0] + right[0])
}
"#;
    let out = jet::compile(src).expect("disjoint write windows must compile");
    assert!(
        out.rust.matches("split_at_mut").count() >= 2,
        "{}",
        out.rust
    );
}

#[test]
fn disjoint_write_indexes_lower_to_safe_split() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    left :: &xs[0]
    right :: &xs[2]
    print(left + right)
}
"#;
    let out = jet::compile(src).expect("disjoint constant indexes must compile");
    assert_eq!(
        out.rust.matches(").split_at_mut(").count(),
        4,
        "{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("let __jet_left = &mut __jet___place_plan_")
            && out
                .rust
                .contains("let __jet_right = &mut __jet___place_plan_"),
        "{}",
        out.rust
    );
}

/// #1162: a simulation may keep statically disjoint particle edit windows
/// live while reading grid cells. D-SHAPE-PLACE1 lowers the particle windows
/// through safe structural splits. Card #1361 / I2: the owner itself may not
/// be read beside a live exclusive window (E0220) — only the windows and
/// values copied before them, exactly as
/// `examples/features/memory/place_windows.jet` is written.
#[test]
fn indexed_simulation_static_update_lowers_to_safe_splits() {
    let src = r#"
struct Particle { position: Int, velocity: Int }
struct Tile { force: Int }

fn run() {
    particles := [Particle]{
        Particle{ position: 10, velocity: 2 },
        Particle{ position: 20, velocity: 3 },
        Particle{ position: 30, velocity: 4 }
    }
    grid :: [Tile]{
        Tile{ force: 5 },
        Tile{ force: 7 },
        Tile{ force: 11 }
    }

    middle_before :: ~particles[1]
    left :: &particles[0]
    right :: &particles[2]
    left.velocity += grid[0].force
    right.velocity += grid[2].force
    left.position += left.velocity
    right.position += right.velocity
    print("{left.position},{middle_before.position},{right.position}")
}
"#;
    let out = jet::compile(src).expect("static particle indexes must compile");
    assert!(
        out.rust.matches(".split_at_mut(").count() >= 4,
        "particle edit windows must use safe structural splits: {}",
        out.rust
    );
}

/// #1162 / #1198: source-level claims that two runtime indexes differ do not
/// prove place disjointness. The existing checker must reject this case.
#[test]
fn indexed_simulation_rejects_dynamic_disjointness_claim() {
    let src = r#"
struct Particle { position: Int }

fn update_pair(particles: &[Particle], left_index: Int, right_index: Int) {
    left :: &particles[left_index]
    right :: &particles[right_index]
    left.position += 1
    right.position += 1
}

fn run() {
    particles := [Particle]{
        Particle{ position: 10 },
        Particle{ position: 20 }
    }
    update_pair(&particles, 0, 1)
}
"#;
    let diags =
        jet::compile(src).expect_err("runtime indexes stay conservatively overlapping until #1198");
    let diag = diags
        .iter()
        .find(|diag| diag.code == "E0212")
        .unwrap_or_else(|| panic!("expected the overlapping-window error: {diags:?}"));
    assert_eq!(
        diag.what,
        "`particles[…]` already has a live view that conflicts with `right`"
    );
    assert!(diag.why.contains("exclusive mutable view"), "{diag:?}");
}

/// #1162: hostile source cannot create two live edit windows to the same
/// particle, even when the bindings have different names.
#[test]
fn indexed_simulation_rejects_hostile_overlap() {
    let src = r#"
struct Particle { position: Int }

fn run() {
    particles := [Particle]{
        Particle{ position: 10 },
        Particle{ position: 20 }
    }
    first :: &particles[0]
    duplicate :: &particles[0]
    first.position += 1
    duplicate.position += 1
}
"#;
    let diags = jet::compile(src).expect_err("overlapping particle edits must fail");
    let diag = diags
        .iter()
        .find(|diag| diag.code == "E0212")
        .unwrap_or_else(|| panic!("expected the overlapping-window error: {diags:?}"));
    assert_eq!(
        diag.what,
        "`particles[…]` already has a live view that conflicts with `duplicate`"
    );
    assert!(diag.why.contains("exclusive mutable view"), "{diag:?}");
}

/// #1162: the memory example runs the indexed particle/grid update through
/// the production CLI and native backend with exact output.
#[test]
fn indexed_simulation_example_runs_production_pipeline() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "run",
            "--release",
            "examples/features/memory/place_windows.jet",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the indexed simulation through the production native CLI path");
    assert!(
        output.status.success(),
        "native indexed simulation failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("example stdout must be UTF-8"),
        "17,20,45\n"
    );
}

/// #1163: return read and write windows into an owner-backed list field.
/// Inclusive `i..i` selects one element; field writes must update the owner.
#[test]
fn owner_backed_collection_returns_element_views() {
    let src = r#"
struct Book {
    title: String,
    pages: Int
}

struct Library {
    books: [Book]
}

fn book_at(lib: Library, i: Int) View<Book> -> lib.books[i..i]

fn edit_at(lib: &Library, i: Int) ViewMut<Book> -[]> {
    return &lib.books[i..i]
}

fn run() {
    lib := Library{
        books: [
            Book{ title: "Dune", pages: 412 },
            Book{ title: "Neuromancer", pages: 271 }
        ]
    }
    first :: book_at(lib, 0)
    print(first[0].title)
    dune :: edit_at(&lib, 0)
    dune[0].pages += 10
    print(lib.books[0].pages)
}
"#;
    let out = jet::compile(src).expect("owner-backed collection views must compile");
    assert!(
        out.rust.contains(
            "fn __jet_book_at<'__jet___view>(__jet_lib: &'__jet___view __jet_Library, __jet_i: i64) -> &'__jet___view [__jet_Book]"
        ),
        "read view must tie to parameter 0: {}",
        out.rust
    );
    assert!(
        out.rust.contains(
            "fn __jet_edit_at<'__jet___view>(__jet_lib: &'__jet___view mut __jet_Library, __jet_i: i64) -> &'__jet___view mut [__jet_Book]"
        ),
        "write view must tie to parameter 0: {}",
        out.rust
    );
    // A compound field write lowers to one mutable place borrow plus a
    // read-modify-write through it, so the assignment target is the ViewMut
    // element itself and no temporary is cloned in between.
    assert!(
        out.rust
            .contains("&mut (__jet_dune)[0i64 as usize].__jet_pages"),
        "field write must assign through the ViewMut element, not a cloned temporary: {}",
        out.rust
    );
    assert!(
        !out.rust.contains("(__jet_dune).clone()"),
        "the write must not go through a cloned view: {}",
        out.rust
    );
}

/// #1163: resizing the owning list while an element view is live is E0212.
#[test]
fn owner_backed_collection_rejects_resize_while_view_live() {
    let src = r#"
struct Book {
    title: String,
    pages: Int
}

struct Library {
    books: [Book]
}

fn book_at(lib: Library, i: Int) View<Book> -> lib.books[i..i]

fn run() {
    lib := Library{
        books: [
            Book{ title: "Dune", pages: 412 },
            Book{ title: "Neuromancer", pages: 271 }
        ]
    }
    first :: book_at(lib, 0)
    lib.books.push(Book{ title: "Snow Crash", pages: 480 })
    print(first[0].title)
}
"#;
    let diags = jet::compile(src).expect_err("resize while view live must fail");
    assert!(
        diags.iter().any(|d| d.code == "E0212"),
        "expected E0212, got {diags:?}"
    );
}

/// #1163 / #1164: a plain owned String place cannot fill View<str>; only tracked
/// string-view bindings / trim|after|before may. Teaching names the ceiling.
#[test]
fn owner_backed_collection_rejects_plain_string_as_view_str_field() {
    let src = r#"
struct Book {
    title: String,
    pages: Int
}

struct Library {
    books: [Book]
}

struct TitleView {
    value: View<str>
}

fn first_title(lib: Library) TitleView -[]> {
    value :: lib.books[0].title
    return TitleView{ value: value }
}

fn run() {
    lib :: Library{
        books: [Book{ title: "Dune", pages: 412 }]
    }
    print(first_title(lib).value)
}
"#;
    let diags = jet::compile(src).expect_err("plain String into View<str> must fail");
    let e2307: Vec<_> = diags.iter().filter(|d| d.code == "E2307").collect();
    assert_eq!(e2307.len(), 1, "expected one E2307, got {diags:?}");
    assert!(
        e2307[0]
            .what
            .contains("owned `String` cannot fill a `View<str>`"),
        "teaching must name the owned-String ceiling, got {:?}",
        e2307[0]
    );
    assert!(
        e2307[0].why.contains(".trim()") || e2307[0].fix.contains(".trim()"),
        "fix must teach trim/after/before or element View, got {:?}",
        e2307[0]
    );
}

/// #1164: returning a local-owned View reports E2305 once — not twice via
/// aggregate + direct return paths.
#[test]
fn local_owned_view_return_reports_e2305_once() {
    let src = r#"
fn make() View<Int> -[]> {
    incidents := [Int]{1, 2, 3, 4, 5}
    return incidents[0..2]
}

fn run() {
    make()
    print(0)
}
"#;
    let diags = jet::compile(src).expect_err("local-owned view return must fail");
    let e2305: Vec<_> = diags.iter().filter(|d| d.code == "E2305").collect();
    assert_eq!(e2305.len(), 1, "expected exactly one E2305, got {diags:?}");
    assert!(
        e2305[0].what.contains("this function owns"),
        "expected owns-return teaching, got {:?}",
        e2305[0]
    );
}

/// #1164: returning a string view as owned String materializes the same copy
/// that `~` requests — no extra E2305 from treating the ident as a boundary.
#[test]
fn string_view_as_owned_return_materializes_copy_once() {
    let src = r#"
fn make() String -[]> {
    email := "nate@jet-lang.dev"
    d :: email.after("@")
    return d
}

fn run() {
    print(make())
}
"#;
    let compiled = jet::compile(src).expect("string view as String must materialize");
    assert!(
        compiled.rust.contains("jet_string_view_copy"),
        "{}",
        compiled.rust
    );
}

/// #1163: the memory example runs the owner-backed library through the
/// production CLI and native backend with exact output.
#[test]
fn owner_backed_collection_example_runs_production_pipeline() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args([
            "run",
            "--release",
            "examples/features/memory/owner_backed_views.jet",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the owner-backed collection through the production native CLI path");
    assert!(
        output.status.success(),
        "native owner-backed collection failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("example stdout must be UTF-8"),
        "Dune\n412\n422\n280\n"
    );
}

#[test]
fn disjoint_place_split_plans_follow_source_order() {
    let src = r#"
fn run() {
    first_owner := [1, 2]
    first_left :: &first_owner[0]
    first_right :: &first_owner[1]
    print(first_left + first_right)

    second_owner := [3, 4]
    second_left :: &second_owner[0]
    second_right :: &second_owner[1]
    print(second_left + second_right)
}
"#;
    let out = jet::compile(src).expect("disjoint place plans must compile deterministically");
    assert!(
        out.rust
            .contains("let __jet___place_plan_0_root = &mut (__jet_first_owner)"),
        "{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("let __jet___place_plan_1_root = &mut (__jet_second_owner)"),
        "{}",
        out.rust
    );
}

#[test]
fn disjoint_struct_fields_use_native_structural_borrows() {
    let src = r#"
struct Pair { left: Int, right: Int }
fn run() {
    pair := Pair{ left: 1, right: 2 }
    left :: &pair.left
    right :: &pair.right
    left = 3
    right = 4
    print(pair.left + pair.right)
}
"#;
    let out = jet::compile(src).expect("different fields are structurally disjoint");
    assert!(
        out.rust.contains("&mut ((__jet_pair).__jet_left)"),
        "{}",
        out.rust
    );
    assert!(
        out.rust.contains("&mut ((__jet_pair).__jet_right)"),
        "{}",
        out.rust
    );
}

#[test]
fn place_window_last_use_inside_branch_releases_owner() {
    let src = r#"
fn run() {
    xs := [1, 2]
    read :: xs[0..0]
    if true {
        print(read[0])
    }
    xs.push(3)
}
"#;
    jet::compile(src).expect("branch-local last use must release the owner");
}

#[test]
fn place_write_window_is_scoped_per_loop_iteration() {
    let src = r#"
fn run() {
    xs := [1, 2]
    loop i in 0..1 {
        edit :: &xs[0]
        edit = edit + 1
    }
    print(xs[0])
}
"#;
    jet::compile(src).expect("loop-local write window must end each iteration");
}

#[test]
fn generic_place_range_is_a_read_window() {
    let src = r#"
fn inspect<T>(xs: [T]) Int -[]> {
    window :: xs[0..0]
    return window.len()
}
fn run() { print(inspect([1, 2])) }
"#;
    let out = jet::compile(src).expect("generic range window must compile");
    assert!(out.rust.contains("jet_view_new"), "{}", out.rust);
}

#[test]
fn range_window_checks_bounds_before_borrowing() {
    let src = r#"
fn run() {
    xs := [1, 2]
    window :: xs[0..1]
    print(window.len())
}
"#;
    let out = jet::compile(src).expect("range window must compile");
    let check = out
        .rust
        .find(" = jet_view_range_new")
        .expect("view helper call");
    let helper = out
        .rust
        .find("fn jet_view_range_new")
        .expect("view helper definition");
    assert!(
        helper < check,
        "bounds-checking helper must exist before use"
    );
    // The window policy is one shared rule (`jet_checked_view_window` ->
    // `jet_checked_view_bounds` -> `jet_range_bounds`) for AOT, the resident
    // JIT, and the interpreter, so the check lives inside the helper and runs
    // before the borrow rather than at the call site.
    assert!(
        out.rust.contains("fn jet_checked_view_bounds"),
        "the shared view-bounds rule must be emitted: {}",
        out.rust
    );
    let helper_body = &out.rust[helper..];
    let bounds = helper_body
        .find("jet_checked_view_window(")
        .expect("the view helper must check bounds");
    let borrow = helper_body
        .find("&xs[start as usize..end as usize]")
        .expect("the view helper must borrow the checked window");
    assert!(
        bounds < borrow,
        "bounds must be checked before the window is borrowed: {}",
        out.rust
    );
}

#[test]
fn touching_write_ranges_are_rejected_before_codegen() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    left :: &xs[0..1]
    right :: &xs[1..2]
    print(left.len() + right.len())
}
"#;
    let diags = jet::compile(src).expect_err("inclusive ranges sharing an index overlap");
    assert!(diags.iter().any(|d| d.code == "E0212"), "{diags:?}");
}

#[test]
fn write_window_return_uses_mutable_parameter_provenance() {
    let src = r#"
fn edit_first(xs: &[Int]) ViewMut<Int> -[]> {
    return &xs[0..1]
}
fn run() { print(0) }
"#;
    let out = jet::compile(src).expect("parameter-rooted write view return must compile");
    assert!(
        out.rust.contains(
            "fn __jet_edit_first<'__jet___view>(__jet_xs: &'__jet___view mut Vec<i64>) -> &'__jet___view mut [i64]"
        ),
        "generated lifetime must tie the mutable view to parameter 0: {}",
        out.rust
    );
}

#[test]
fn write_window_cannot_be_stored() {
    let src = r#"
struct Holder { window: ViewMut<Int> }
fn run() {
    xs := [1, 2, 3]
    edit :: &xs[0..1]
    holder :: Holder{ window: edit }
    print(holder.window.len())
}
"#;
    let diags = jet::compile(src).expect_err("write view storage must be rejected");
    assert!(diags.iter().any(|d| d.code == "E2305"), "{diags:?}");
}

#[test]
fn write_window_cannot_cross_task_boundary() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    edit :: &xs[0..1]
    handle :: task edit.len()
    print(handle.join() ?? 0)
}
"#;
    let diags = jet::compile(src).expect_err("write view task capture must be rejected");
    assert!(diags.iter().any(|d| d.code == "E1102"), "{diags:?}");
}

#[test]
fn write_window_cannot_cross_channel_boundary() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    edit :: &xs[0..1]
    (sender, channel) :: channel<ViewMut<Int>>()
    sender.send(edit)
}
"#;
    let diags = jet::compile(src).expect_err("write view channel send must be rejected");
    assert!(diags.iter().any(|d| d.code == "E1102"), "{diags:?}");
}

/// D-MEM1 S9 / #649: any list operation that may change backing storage is
/// exclusive with a live view.
#[test]
fn resizing_list_owner_with_live_view_is_error() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    window :: xs[0..1]
    xs.push(4)
    print(window.len())
}
"#;
    let diags = jet::compile(src).expect_err("resizing a viewed owner must fail");
    assert!(diags.iter().any(|d| d.code == "E0212"));
}

#[test]
fn named_range_view_blocks_owner_resize_and_replacement() {
    for action in ["xs.push(4)", "xs = [4, 5, 6]"] {
        let src = format!(
            r#"
fn run() {{
    xs := [1, 2, 3]
    band :: 0..<2
    window :: xs[band]
    {action}
    print(window.len())
}}
"#
        );
        let diags = jet::compile(&src).expect_err("changing a viewed owner must fail");
        assert!(
            diags.iter().any(|d| d.code == "E0212"),
            "expected E0212 for `{action}`, got {diags:?}"
        );
    }
}

/// D-MEM1 S9 / #649: replacing an owner invalidates every view, so assignment
/// is rejected while a view remains live.
#[test]
fn replacing_list_owner_with_live_view_is_error() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    window :: xs[0..1]
    xs = [4, 5, 6]
    print(window.len())
}
"#;
    let diags = jet::compile(src).expect_err("replacing a viewed owner must fail");
    assert!(diags.iter().any(|d| d.code == "E0212"));
}

/// D-MEM1 S9 / #649: view facts end at lexical scope. Mutating owner after
/// nested view dies is valid.
#[test]
fn owner_can_resize_after_view_scope_ends() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    if true {
        window :: xs[0..1]
        print(window.len())
    }
    xs.push(4)
    print(xs.len())
}
"#;
    jet::compile(src).expect("expired view must not keep owner borrowed");
}

/// #649: owner identity includes field projections; a view into a field keeps
/// that field's enclosing owner from changing storage.
#[test]
fn field_view_blocks_owner_resize() {
    let src = r#"
struct Bucket {
    values: [Int]
}

fn run() {
    bucket := Bucket{ values: [1, 2, 3] }
    window :: bucket.values[0..1]
    bucket.values.push(4)
    print(window.len())
}
"#;
    let diags = jet::compile(src).expect_err("field owner mutation must conflict with view");
    assert!(diags.iter().any(|d| d.code == "E0212"), "got {diags:?}");
}

#[test]
fn field_assignment_conflicts_with_live_view() {
    let src = r#"
struct Bucket {
    values: [Int]
}

fn run() {
    bucket := Bucket{ values: [1, 2, 3] }
    window :: bucket.values[0..1]
    bucket.values = [4, 5]
    print(window.len())
}
"#;
    let diags = jet::compile(src).expect_err("field assignment must conflict with view");
    assert!(diags.iter().any(|d| d.code == "E0212"), "got {diags:?}");
}

#[test]
fn write_argument_conflicts_with_live_view() {
    let src = r#"
fn edit(xs: &[Int]) {
    print(xs.len())
}

fn run() {
    xs := [1, 2, 3]
    window :: xs[0..1]
    edit(&xs)
    print(window.len())
}
"#;
    let diags = jet::compile(src).expect_err("write argument must conflict with view");
    assert!(diags.iter().any(|d| d.code == "E0212"), "got {diags:?}");
}

#[test]
fn method_write_argument_conflicts_with_live_view_once() {
    let src = r#"
struct Editor { id: Int }

impl Editor {
    fn touch(self, values: &[Int]) {
        values.push(9)
    }
}

fn run() {
    editor :: Editor{ id: 0 }
    values := [1, 2, 3]
    window :: values[0..2]
    editor.touch(&values)
    print(window[0])
}
"#;
    let diags = jet::compile(src).expect_err("live view must block method write argument");
    assert_eq!(
        diags.iter().filter(|diag| diag.code == "E0212").count(),
        1,
        "method write argument must report E0212 once: {diags:?}"
    );
}

#[test]
fn inline_module_write_argument_conflicts_with_live_view_once() {
    let src = r#"
module edit {
    pub fn touch(values: &[Int]) {
        values.push(9)
    }
}

fn run() {
    values := [1, 2, 3]
    window :: values[0..2]
    edit.touch(&values)
    print(window[0])
}
"#;
    let diags = jet::compile(src).expect_err("live view must block inline-module write argument");
    assert_eq!(
        diags.iter().filter(|diag| diag.code == "E0212").count(),
        1,
        "inline-module write argument must report E0212 once: {diags:?}"
    );
}

#[test]
fn method_write_argument_allows_nonoverlapping_owner() {
    let src = r#"
struct Editor { id: Int }

impl Editor {
    fn touch(self, values: &[Int]) {
        values.push(9)
    }
}

fn run() {
    editor :: Editor{ id: 0 }
    viewed := [1, 2, 3]
    changed := [4, 5, 6]
    window :: viewed[0..2]
    editor.touch(&changed)
    print(window[0])
}
"#;
    jet::compile(src).expect("write to nonoverlapping owner must stay valid");
}

#[test]
fn returned_parameter_view_uses_stable_parameter_provenance() {
    let src = r#"
fn first(xs: [Int], other: [Int]) View<Int> -[]> {
    return xs[0..1]
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    first_view :: first(left, right)
    print(first_view[0])
}
"#;
    jet::compile(src).expect("parameter 0 provenance must make the returned view safe");
}

#[test]
fn returned_view_composes_through_wrapper_call() {
    let src = r#"
fn first(left: [Int], right: [Int]) View<Int> -[]> {
    return left[0..1]
}

fn wrapper(left: [Int], right: [Int]) View<Int> -[]> {
    return first(left, right)
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    result :: wrapper(left, right)
    print(result[0])
}
"#;
    jet::compile(src).expect("callee parameter 0 must map to wrapper parameter 0");
}

/// #745: a zero-copy parser can return multiple token windows into one
/// caller-owned source. Both fields must keep parameter-0 provenance.
#[test]
fn zero_copy_parser_returns_token_and_remainder_views() {
    let src = r#"
struct Token {
    text: View<str>,
    rest: View<str>
}

fn scan(source: String) Token -[]> {
    text :: source.before(":")
    rest :: source.after(":")
    return Token{ text: text, rest: rest }
}

fn parse(source: String) Token -[]> {
    return scan(source)
}

fn run() {
    source := "name:value"
    token :: parse(source)
    print(token.text)
    print(token.rest)
    expected :: "name"
    print("wrapped: {token.text}")
    print("wrapped: {expected}")
}
"#;
    let out =
        jet::compile(src).expect("a parser token and remainder may borrow one caller-owned source");
    assert!(
        out.rust.contains("pub struct __jet_Token<'__jet___view>")
            && out.rust.contains("pub __jet_text: &'__jet___view str")
            && out.rust.contains("pub __jet_rest: &'__jet___view str")
            && out.rust.contains("-> __jet_Token<'__jet___view>"),
        "both parser views must share the hidden source lifetime: {}",
        out.rust
    );
}

/// #745: D-MEM-VIEWRET1 rejects a parser that returns windows into storage
/// owned by the parser call.
#[test]
fn zero_copy_parser_rejects_locally_owned_source() {
    let src = r#"
struct Token {
    text: View<str>,
    rest: View<str>
}

fn parse_owned() Token -[]> {
    source := "name:value"
    text :: source.before(":")
    rest :: source.after(":")
    return Token{ text: text, rest: rest }
}

fn run() {
    print(parse_owned().text)
}
"#;
    let diags =
        jet::compile(src).expect_err("parser-owned storage cannot back returned token views");
    assert!(
        diags.iter().any(|diag| diag.code == "E2307"),
        "expected string-view owner error: {diags:?}"
    );
}

/// #745: hostile caller code cannot replace parser input while a returned
/// token still observes that storage.
#[test]
fn zero_copy_parser_blocks_source_replacement_while_token_is_live() {
    for (projection, field) in [("parse_text", "text"), ("parse_rest", "rest")] {
        let src = r#"
struct Token {
    text: View<str>,
    rest: View<str>
}

fn scan(source: String) Token -[]> {
    text :: source.before(":")
    rest :: source.after(":")
    return Token{ text: text, rest: rest }
}

fn parse(source: String) Token -[]> {
    return scan(source)
}

fn parse_text(source: String) View<str> -[]> {
    return parse(source).text
}

fn parse_rest(source: String) View<str> -[]> {
    return parse(source).rest
}

fn run() {
    source := "name:value"
    selected :: $PROJECTION(source)
    source = "other:data"
    print(selected)
}
"#
        .replace("$PROJECTION", projection);
        let diags =
            jet::compile(&src).expect_err("live parser views must keep caller storage stable");
        assert!(
            diags.iter().any(|diag| diag.code == "E0212"),
            "expected owner-invalidation error for token.{field}: {diags:?}"
        );
    }
}

/// #745: keep the user-facing zero-copy parser example on the same production
/// parser, sema, TIR, and codegen path as hand-written source.
#[test]
fn zero_copy_parser_example_covers_production_pipeline() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", "examples/features/memory/returned_views.jet"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run the returned-views example through the production CLI");
    assert!(
        output.status.success(),
        "native parser example failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("example stdout must be UTF-8"),
        "7\nexample.com\nname\nwrapped: name\nvalue\na longer value\nleft\nright\nmessage\nhello\n"
    );
}

#[test]
fn returned_string_view_uses_parameter_provenance() {
    let src = r#"
fn domain(email: String) View<str> -[]> {
    result :: email.after("@")
    return result
}
fn run() { print(domain("user@example.com")) }
"#;
    let out = jet::compile(src).expect("parameter-rooted string view return must compile");
    assert!(
        out.rust.contains(
            "fn __jet_domain<'__jet___view>(__jet_email: &'__jet___view String) -> &'__jet___view str"
        ),
        "generated lifetime must tie the string view to parameter 0: {}",
        out.rust
    );
}

#[test]
fn returned_string_view_cannot_outlive_local_owner() {
    let src = r#"
fn bad() View<str> -[]> {
    email := "user@example.com"
    result :: email.after("@")
    return result
}
fn run() { print(bad()) }
"#;
    let diags = jet::compile(src).expect_err("locally-owned string view return must fail");
    assert!(diags.iter().any(|d| d.code == "E2307"), "{diags:?}");
}

#[test]
fn returned_aggregate_stabilizes_string_view_field_provenance() {
    let src = r#"
struct Domain { value: View<str> }

fn domain(email: String) Domain -[]> {
    result :: email.after("@")
    return Domain{ value: result }
}
fn run() { print(domain("user@example.com").value) }
"#;
    let out = jet::compile(src).expect("parameter-rooted string view field must compile");
    assert!(
        out.rust.contains("pub struct __jet_Domain<'__jet___view>"),
        "{}",
        out.rust
    );
    assert!(
        out.rust.contains("pub __jet_value: &'__jet___view str"),
        "{}",
        out.rust
    );
}

#[test]
fn returned_view_constant_aggregate_keeps_borrowed_field_lowering() {
    let src = r#"
struct Token {
    text: View<str>
    rest: View<str>
}

fn scan(source: String) Token -[]> {
    text :: source.before(":")
    rest :: source.after(":")
    return Token{ text: text, rest: rest }
}

fn run() {
    source :: "name:value"
    token :: scan(source)
    print(token.text)
    print(token.rest)
}
"#;
    let out = jet::compile(src).expect("constant returned-view aggregate must compile");
    assert!(
        out.rust
            .contains("let __jet_token: __jet_Token = __jet_scan(&(__jet_source));"),
        "view-bearing constants must lower through the borrow-preserving call path:\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains("__jet_text: \"name\".to_string()")
            && !out.rust.contains("__jet_rest: \"value\".to_string()"),
        "view fields must not be emitted as owned Strings:\n{}",
        out.rust
    );
}

#[test]
fn returned_string_view_field_cannot_outlive_local_owner() {
    let src = r#"
struct Domain { value: View<str> }

fn bad() Domain -[]> {
    email := "user@example.com"
    result :: email.after("@")
    return Domain{ value: result }
}
fn run() { print(bad().value) }
"#;
    let diags = jet::compile(src).expect_err("locally-owned string view field must fail");
    assert!(diags.iter().any(|d| d.code == "E2307"), "{diags:?}");
}

#[test]
fn returned_string_view_rejects_temporary_call_owner_before_codegen() {
    let src = r#"
fn domain(email: String) View<str> -[]> {
    result :: email.after("@")
    return result
}
fn run() {
    result :: domain("user@example.com")
    print(result)
}
"#;
    let diags = jet::compile(src).expect_err("temporary owner must be rejected in sema");
    assert!(diags.iter().any(|d| d.code == "E2307"), "{diags:?}");
}

#[test]
fn returned_view_summary_is_independent_of_declaration_order() {
    let src = r#"
fn wrapper(left: [Int], right: [Int]) View<Int> -[]> {
    return first(left, right)
}

fn first(left: [Int], right: [Int]) View<Int> -[]> {
    return left[0..1]
}

fn run() { print(0) }
"#;
    jet::compile(src).expect("forward callable provenance must stabilize before validation");
}

#[test]
fn mutually_recursive_view_summaries_stabilize() {
    let src = r#"
fn first(values: [Int], recurse: Bool) View<Int> -[]> {
    if recurse {
        return second(values, false)
    }
    return values[0..1]
}

fn second(values: [Int], recurse: Bool) View<Int> -[]> {
    if recurse {
        return first(values, false)
    }
    return values[0..1]
}

fn run() { print(0) }
"#;
    jet::compile(src).expect("mutually recursive parameter-0 summaries must converge");
}

#[test]
fn returned_view_composes_through_inherent_method() {
    let src = r#"
struct Selector { marker: Int }

impl Selector {
    fn first(self, left: [Int], right: [Int]) View<Int> -[]> {
        return left[0..1]
    }
}

fn wrapper(selector: Selector, left: [Int], right: [Int]) View<Int> -[]> {
    return selector.first(left, right)
}

fn run() {
    selector :: Selector{ marker: 0 }
    left := [7, 8]
    right := [9, 10]
    result :: wrapper(selector, left, right)
    print(result[0])
}
"#;
    jet::compile(src).expect("method parameter 0 must compose onto wrapper parameter 1");
}

#[test]
fn trait_view_summary_is_independent_of_impl_order() {
    let src = r#"
trait Select {
    fn select(self, left: [Int], right: [Int]) View<Int>
}

struct First { marker: Int }

fn wrapper(selector: First, left: [Int], right: [Int]) View<Int> -[]> {
    return selector.select(left, right)
}

impl First.Select {
    fn select(self, left: [Int], right: [Int]) View<Int> -[]> {
        return left[0..1]
    }
}

fn run() { print(0) }
"#;
    jet::compile(src).expect("trait method provenance must stabilize before wrapper validation");
}

#[test]
fn trait_view_contract_unions_compatible_implementation_sources() {
    let src = r#"
trait Select {
    fn select(self, left: [Int], right: [Int]) View<Int>
}

struct First {}
impl First.Select {
    fn select(self, left: [Int], right: [Int]) View<Int> -[]> {
        return left[0..1]
    }
}

struct Last {}
impl Last.Select {
    fn select(self, left: [Int], right: [Int]) View<Int> -[]> {
        return right[0..1]
    }
}

fn run() { print(0) }
"#;
    jet::compile(src).expect("trait dispatch may choose any compatible implementation owner");
}

#[test]
fn aggregate_trait_view_contract_stabilizes_through_wrapper_in_either_impl_order() {
    let template = r#"
struct Pair { left: View<Int>, right: View<Int> }

trait Select {
    fn select(self, left: [Int], right: [Int]) Pair -[]>
}

fn wrapper(selector: Select, left: [Int], right: [Int]) Pair -[]> {
    return selector.select(left, right)
}

$IMPLS

fn run() { print(0) }
"#;
    let first = r#"
struct First {}
impl First.Select {
    fn select(self, left: [Int], right: [Int]) Pair -[]> {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Pair{ left: left_view, right: right_view }
    }
}
"#;
    let last = r#"
struct Last {}
impl Last.Select {
    fn select(self, left: [Int], right: [Int]) Pair -[]> {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Pair{ left: left_view, right: right_view }
    }
}
"#;
    for implementations in [format!("{first}{last}"), format!("{last}{first}")] {
        let src = template.replace("$IMPLS", &implementations);
        jet::compile(&src).expect("aggregate trait contract must stabilize before wrapper");
    }
}

#[test]
fn aggregate_trait_view_contract_unions_sources_in_either_impl_order() {
    let template = r#"
struct Pair { left: View<Int>, right: View<Int> }

trait Select {
    fn select(self, left: [Int], right: [Int]) Pair
}

$IMPLS

fn run() { print(0) }
"#;
    let first = r#"
struct First {}
impl First.Select {
    fn select(self, left: [Int], right: [Int]) Pair -[]> {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Pair{ left: left_view, right: right_view }
    }
}
"#;
    let last = r#"
struct Last {}
impl Last.Select {
    fn select(self, left: [Int], right: [Int]) Pair -[]> {
        left_view :: left[0..1]
        right_view :: left[0..1]
        return Pair{ left: left_view, right: right_view }
    }
}
"#;
    for implementations in [format!("{first}{last}"), format!("{last}{first}")] {
        let src = template.replace("$IMPLS", &implementations);
        jet::compile(&src).expect("aggregate trait implementations union compatible slot sources");
    }
}

#[test]
fn returned_view_provenance_transfers_on_binding_move() {
    let src = r#"
fn first(values: [Int]) View<Int> -[]> {
    initial :: values[0..1]
    moved :: initial
    return moved
}

fn run() {
    values := [7, 8]
    result :: first(values)
    print(result[0])
}
"#;
    jet::compile(src).expect("moving a view binding must transfer its owner provenance");
}

#[test]
fn returned_aggregate_stabilizes_view_field_provenance() {
    let src = r#"
struct Window { values: View<Int> }

fn window(values: [Int]) Window -[]> {
    selected :: values[0..1]
    return Window{ values: selected }
}

fn run() {
    values := [7, 8]
    result :: window(values)
    print(result.values[0])
}
"#;
    let out = jet::compile(src).expect("returned aggregate must keep its view tied to parameter 0");
    assert!(
        out.rust.contains("pub struct __jet_Window<'__jet___view>")
            && out.rust.contains("pub __jet_values: &'__jet___view [i64]")
            && out.rust.contains("-> __jet_Window<'__jet___view>"),
        "aggregate and return must share the hidden owner lifetime: {}",
        out.rust
    );
}

#[test]
fn nested_returned_aggregate_stabilizes_each_view_output_slot() {
    let src = r#"
struct Inner { values: View<Int> }
struct Outer { inner: Inner }

fn outer(values: [Int]) Outer -[]> {
    selected :: values[0..1]
    return Outer{ inner: Inner{ values: selected } }
}

fn run() {
    values := [7, 8]
    result :: outer(values)
    print(result.inner.values[0])
}
"#;
    let out =
        jet::compile(src).expect("nested returned aggregate must carry transitive view provenance");
    assert!(
        out.rust.contains("pub struct __jet_Inner<'__jet___view>"),
        "{}",
        out.rust
    );
    assert!(
        out.rust.contains("pub struct __jet_Outer<'__jet___view>"),
        "{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("pub __jet_inner: __jet_Inner<'__jet___view>"),
        "{}",
        out.rust
    );
    assert!(
        out.rust.contains("-> __jet_Outer<'__jet___view>"),
        "{}",
        out.rust
    );
}

/// The property: a wrapper around a view-bearing value renders the view
/// lifetime on the NAMED LEAF that actually holds the window, never on the
/// wrapper itself. Put it on the wrapper and every value of that wrapper type
/// would be tied to one owner's storage, whether or not it carries a window.
///
/// The wrappers are spelled `JetOutcome<…>` now, not `Option`/`Result`.
/// D-FAIL-CARRIER1=A / D-FAIL-MODEL1=A (ratified 2026-08-06,
/// crates/jet-foundation/src/Outcome.rs:1-22) make one carrier serve both
/// surface forms: `T?` is `JetOutcome<T, JetAbsent>` and `T !E` is
/// `JetOutcome<T, E>`. A raw `Option` here would be the second optional
/// representation that decision exists to remove. The leaf-not-wrapper
/// property is untouched by the rename, so the negative guards move to the
/// carrier's own name — without them these assertions would still pass on a
/// build that parameterized the wrapper too.
#[test]
fn wrapper_returned_view_aggregates_render_lifetimes_on_named_leaves() {
    let src = r#"
struct Window { values: View<Int> }
struct Holder { maybe: ?Window }
struct GenericHolder<T> { value: T, maybe: ?Window }

fn maybe(values: [Int]) (?Window) -[]> {
    selected :: values[0..1]
    return Val(Window{ values: selected })
}

fn result(values: [Int]) Window !String -[]> {
    selected :: values[0..1]
    return Ok(Window{ values: selected })
}

fn tuple(values: [Int]) (window: Window, count: Int) -[]> {
    selected :: values[0..1]
    return (window: Window{ values: selected }, count: 1)
}

fn run() { print(0) }
"#;
    let out = jet::compile(src).expect("wrapper returns must preserve view provenance");
    assert!(
        out.rust
            .contains("JetOutcome<__jet_Window<'__jet___view>, JetAbsent>"),
        "{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("JetOutcome<__jet_Window<'__jet___view>, String>"),
        "{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("pub __jet_window: __jet_Window<'__jet___view>"),
        "{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("pub struct __jet_GenericHolder<'__jet___view, T"),
        "{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("pub __jet_maybe: JetOutcome<__jet_Window<'__jet___view>, JetAbsent>"),
        "{}",
        out.rust
    );
    assert!(
        !out.rust.contains("JetOutcome<'__jet___view"),
        "{}",
        out.rust
    );
    assert!(!out.rust.contains("Option<'__jet___view"), "{}", out.rust);
    assert!(!out.rust.contains("Result<'__jet___view"), "{}", out.rust);
}

#[test]
fn enums_and_mutable_aggregates_carry_view_provenance() {
    let src = r#"
enum Selection {
    One(View<Int>)
    Pair(PairViews)
}

struct PairViews { left: View<Int>, right: View<Int> }
struct Edit { values: ViewMut<Int> }

fn select(left: [Int], right: [Int], pair: Bool) Selection -[]> {
    if pair {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Selection.Pair(PairViews{ left: left_view, right: right_view })
    }
    selected :: right[0..1]
    return Selection.One(selected)
}

fn first(selection: Selection) View<Int> -[]> {
    if selection == {
        .One(values) -> { return values }
        .Pair(values) -> { return values.left }
    }
}

fn edit(values: &[Int]) Edit -[]> {
    selected :: &values[0..1]
    return Edit{ values: selected }
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    chosen :: select(left, right, true)
    print(first(chosen)[0])
    target := [1, 2]
    borrowed :: edit(&target)
    borrowed.values[0] = 3
    print(borrowed.values[0])
}
"#;
    let out = jet::compile(src).expect("enum and mutable aggregate views must reach codegen");
    assert!(
        out.rust.contains("pub enum __jet_Selection<'__jet___view>")
            && out.rust.contains("__jet_One(&'__jet___view [i64])")
            && out
                .rust
                .contains("__jet_Pair(__jet_PairViews<'__jet___view>)")
            && out.rust.contains("pub __jet_left: &'__jet___view [i64],")
            && out.rust.contains("pub __jet_right: &'__jet___view [i64],"),
        "{}",
        out.rust
    );
    assert!(
        out.rust.contains("pub struct __jet_Edit<'__jet___view>")
            && out
                .rust
                .contains("pub __jet_values: &'__jet___view mut [i64]"),
        "{}",
        out.rust
    );
    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_view_enum", "view_enum_aggregate", src);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "7\n3\n");
    }

    let root = common::unique_tmp("jet_view_enum_quick_run");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, src).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", path.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .expect("run mutable aggregate views through the default tier");
    assert!(
        output.status.success(),
        "default mutable aggregate run failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "7\n3\n");
}

#[test]
fn mutable_view_aggregates_are_not_cloneable() {
    let src = r#"
struct Edit { values: ViewMut<Int> }

fn edit(values: &[Int]) Edit -[]> {
    selected :: &values[0..1]
    return Edit{ values: selected }
}

fn run() {
    target := [1, 2]
    borrowed :: edit(&target)
    duplicate :: ~borrowed
    print(duplicate.values[0])
}
"#;
    let diags = jet::compile(src).expect_err("copying an exclusive view would duplicate it");
    assert!(diags.iter().any(|diag| diag.code == "E0211"), "{diags:?}");
}

#[test]
fn disjoint_mutable_views_can_travel_in_a_list_and_write_through() {
    let src = r#"
fn edits(values: &[Int]) [ViewMut<Int>] -[]> {
    return [&values[0..1], &values[2..3]]
}

fn run() {
    values := [7, 8, 9, 10]
    selected :: edits(&values)
    selected[0][0] = 11
    selected[1][0] = 12
    print(selected[0][0])
    print(selected[1][0])
}
"#;
    let out = jet::compile(src).expect("disjoint mutable view lists must pass sema");
    assert!(out.rust.contains("jet_views_mut_new"), "{}", out.rust);
    assert!(out.rust.contains("jet_index_vec_mut"), "{}", out.rust);
    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_view_mut_list", "view_mut_list", src);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "11\n12\n");
    }

    let root = common::unique_tmp("jet_view_mut_list_quick_run");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, src).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", path.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .expect("run mutable view lists through the default tier");
    assert!(
        output.status.success(),
        "default mutable view list run failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "11\n12\n");
}

#[test]
fn mutable_view_bounds_fail_at_construction_in_every_tier() {
    use jet::Interpreter::RunOutcome;
    use jet::JitBackend::JitBackend;

    let src = r#"
fn run() {
    values := [1]
    outside :: [&values[0..0], &values[2..2]]
}
"#;
    // D-FAIL-BREACH1=A: every tier renders this failure through the one
    // `jet_runtime_stop_report` boundary, over the shared bounds message
    // `Prelude/Core/RangeBounds.rs::jet_view_bounds_error` produces. No tier
    // prints the bare message, and none may drop the Jet source location.
    let stop = "Stop [E3001]: `panic: can't view 1 items from 2 to 2 (inclusive)`";
    jet::compile(src).expect("dynamic view bounds must reach runtime validation");
    if common::have_rustc() {
        let (code, _stdout, stderr) =
            common::build_and_run("jet_view_mut_bounds_aot", "view_mut_bounds_aot", src);
        assert_ne!(code, 0, "AOT accepted an invalid mutable view");
        assert!(
            stderr.contains(stop),
            "AOT reported the wrong runtime failure: {stderr}"
        );
    }

    let root = common::unique_tmp("jet_view_mut_bounds_tiers");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, src).unwrap();
    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diag| diag.severity == jet::Diagnostics::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");

    let mut jit_report = None;
    if jet_jit::cranelift_host_supported() {
        assert!(
            jet_jit::resident_jit_safe_bundle(&bundle),
            "{}",
            jet_jit::resident_jit_safe_bundle_detail(&bundle)
        );
        jet_jit::try_compile_bundle(&bundle).expect("mutable view bounds must lower to JIT");
        let mut backend = jet_jit::CraneliftBackend::new();
        match backend.run(&bundle, false) {
            RunOutcome::Ran {
                stderr, exit_code, ..
            } => {
                assert_eq!(exit_code, 70, "resident JIT exit drift: {stderr}");
                assert!(
                    stderr.starts_with(stop),
                    "resident JIT reported the wrong runtime failure: {stderr}"
                );
                jit_report = Some(stderr);
            }
            RunOutcome::Problems(diags) => {
                panic!("resident JIT returned diagnostics instead of running: {diags:?}")
            }
        }
    }

    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, true) {
        RunOutcome::Ran {
            stderr, exit_code, ..
        } => {
            assert_eq!(exit_code, 70, "forced interpreter exit drift: {stderr}");
            assert!(
                stderr.starts_with(stop),
                "forced interpreter reported the wrong runtime failure: {stderr}"
            );
            // The stop names the failing Jet place, not just the message: an
            // engine that loses its source facts reports a different thing
            // from the engine beside it (I9).
            assert!(
                stderr.contains("main.jet:4 in run"),
                "forced interpreter dropped the Jet source location: {stderr}"
            );
            if let Some(jit) = jit_report {
                assert_eq!(
                    jit, stderr,
                    "the resident JIT and the interpreter must report one failure"
                );
            }
        }
        RunOutcome::Problems(diags) => {
            panic!("forced interpreter returned diagnostics instead of running: {diags:?}")
        }
    }

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", path.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .expect("run invalid mutable view through the default tier");
    assert!(
        !output.status.success(),
        "default tier accepted an invalid mutable view"
    );
}

#[test]
fn runtime_disjoint_split_and_indexes_write_through() {
    let src = r#"
fn run() {
    split_values := [1, 2, 3, 4]
    parts :: split_values.split_write(2) ?? panic("split failed")
    parts.left[0] = 10
    parts.right[0] = 30
    print(split_values)

    indexed_values := [5, 6, 7, 8]
    edits :: indexed_values.get_disjoint_write([0, 3]) ?? panic("index proof failed")
    loop edit in edits {
        edit[0] = edit[0] + 45
    }
    print(indexed_values)
}
"#;
    let out = jet::compile(src).expect("checked runtime disjoint views must compile");
    assert!(out.rust.contains("jet_split_write"), "{}", out.rust);
    assert!(out.rust.contains("jet_get_disjoint_write"), "{}", out.rust);
    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_runtime_disjoint", "runtime_disjoint", src);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "[10, 2, 30, 4]\n[50, 6, 7, 53]\n");
    }
}

#[test]
fn runtime_disjoint_proof_reports_bounds_and_duplicates_before_mutation() {
    let bounds = r#"
fn run() {
    values := [1, 2]
    result := values.get_disjoint_write([0, 2])
    if result == {
        .Ok(_) -> panic("accepted invalid bounds")
        .Err(error) -> print(error)
    }
}
"#;
    let duplicate = r#"
fn run() {
    values := [1, 2]
    result := values.get_disjoint_write([0, 0])
    if result == {
        .Ok(_) -> panic("accepted duplicate index")
        .Err(error) -> print(error)
    }
}
"#;
    for (name, src, message) in [
        ("bounds", bounds, "outside"),
        ("duplicate", duplicate, "duplicate"),
    ] {
        jet::compile(src).expect("checked runtime proof failures are typed values");
        if common::have_rustc() {
            let (code, stdout, stderr) =
                common::build_and_run("jet_runtime_disjoint_error", name, src);
            assert_eq!(code, 0, "{stderr}");
            assert!(stdout.contains(message), "{stdout}");
        }
    }
}

#[test]
fn runtime_disjoint_views_match_aot_jit_dev_and_interpreter() {
    use jet::Interpreter::RunOutcome;

    let src = r#"
fn run() {
    values := [1, 2, 3, 4]
    parts :: values.split_write(2) ?? panic("split failed")
    parts.left[0] = 10
    parts.right[1] = 40

    values.edit_disjoint([0, 2], (left, right) -> {
        left[0] = left[0] + 1
        right[0] = right[0] + 2
    }) ?? panic("edit failed")

    selected :: values.get_disjoint_write([1, 3]) ?? panic("selection failed")
    loop edit in selected {
        edit[0] = edit[0] + 5
    }
    print(values)
}
"#;
    let expected = "[11, 7, 5, 45]\n";
    let root = common::unique_tmp("jet_runtime_disjoint_tiers");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, src).unwrap();

    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_runtime_disjoint_tiers", "aot", src);
        assert_eq!(code, 0, "AOT failed: {stderr}");
        assert_eq!(stdout, expected, "AOT output drift");
    }

    let mut bundle = jet::Loader::load_entry(path.to_str().unwrap()).unwrap();
    let errors: Vec<_> = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Run)
        .into_iter()
        .filter(|diag| diag.severity == jet::Diagnostics::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(
        jet_jit::resident_jit_safe_bundle(&bundle),
        "{}",
        jet_jit::resident_jit_safe_bundle_detail(&bundle)
    );
    jet_jit::try_compile_bundle(&bundle).expect("disjoint views must lower to resident JIT");

    for (tier, force_interpreter) in [("resident JIT", false), ("interpreter", true)] {
        jet_jit::reset_jit_trace_for_test();
        match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, force_interpreter) {
            RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(exit_code, 0, "{tier}: {stderr}");
                assert_eq!(stderr, "", "{tier} stderr drift");
                assert_eq!(stdout, expected, "{tier} output drift");
            }
            RunOutcome::Problems(diags) => panic!("{tier} failed: {diags:?}"),
        }
        if !force_interpreter {
            assert!(
                jet_jit::jit_executed_for_test(),
                "resident JIT did not execute"
            );
            assert!(
                !jet_jit::fallback_invoked_for_test() && !jet_jit::deopt_invoked_for_test(),
                "resident JIT used an interpreter fallback"
            );
        }
    }

    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", path.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .expect("run disjoint views through the default tier");
    assert!(
        output.status.success(),
        "default run failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

#[test]
fn runtime_disjoint_views_reject_alias_storage_and_owner_invalidation() {
    let alias = r#"
fn run() {
    values := [1, 2]
    selected :: values.get_disjoint_write([0, 1]) ?? panic("selection failed")
    saved := [ViewMut<Int>]{}
    loop edit in selected {
        saved.push(edit)
    }
}
"#;
    let alias_diags =
        jet::compile(alias).expect_err("a lending loop must not let an exclusive view escape");
    assert!(
        alias_diags
            .iter()
            .any(|diag| matches!(diag.code.as_str(), "E0120" | "E0212")),
        "{alias_diags:?}"
    );

    let invalidation = r#"
fn run() {
    values := [1, 2]
    selected :: values.get_disjoint_write([0, 1]) ?? panic("selection failed")
    values.push(3)
    selected[0][0] = 9
}
"#;
    let invalidation_diags =
        jet::compile(invalidation).expect_err("the owner must stay borrowed while views are live");
    assert!(
        invalidation_diags
            .iter()
            .any(|diag| matches!(diag.code.as_str(), "E0212" | "E0507")),
        "{invalidation_diags:?}"
    );
}

#[test]
fn lending_disjoint_views_reject_every_retaining_boundary() {
    let cases = [
        (
            "aggregate binding",
            r#"
fn run() {
    values := [1, 2]
    selected :: values.get_disjoint_write([0, 1]) ?? panic("selection failed")
    loop edit in selected { held :: [edit] }
}
"#,
        ),
        (
            "assignment",
            r#"
fn run() {
    values := [1, 2]
    selected :: values.get_disjoint_write([0, 1]) ?? panic("selection failed")
    held := [ViewMut<Int>]{}
    loop edit in selected { held = [edit] }
}
"#,
        ),
        (
            "named helper",
            r#"
fn retain(view: ViewMut<Int>) {}
fn run() {
    values := [1, 2]
    selected :: values.get_disjoint_write([0, 1]) ?? panic("selection failed")
    loop edit in selected { retain(edit) }
}
"#,
        ),
        (
            "return",
            r#"
fn leak(values: &[Int]) ViewMut<Int> -[]> {
    selected :: values.get_disjoint_write([0, 1]) ?? panic("selection failed")
    loop edit in selected { return edit }
    panic("unreachable")
}
fn run() {
    values := [1, 2]
    leak(&values)
}
"#,
        ),
    ];
    for (boundary, src) in cases {
        let diagnostics = jet::compile(src)
            .expect_err("a lending mutable view must not cross a retaining boundary");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0212"),
            "{boundary}: {diagnostics:?}"
        );
    }
}

#[test]
fn edit_disjoint_callback_views_reject_every_retaining_boundary() {
    let cases = [
        (
            "aggregate binding",
            r#"
fn run() {
    values := [1, 2]
    values.edit_disjoint([0, 1], (left, right) -> {
        held :: [left, right]
    })
}
"#,
        ),
        (
            "assignment",
            r#"
fn run() {
    values := [1, 2]
    held := [ViewMut<Int>]{}
    values.edit_disjoint([0, 1], (left, right) -> {
        held = [left, right]
    })
}
"#,
        ),
        (
            "named helper",
            r#"
fn retain(view: ViewMut<Int>) {}
fn run() {
    values := [1, 2]
    values.edit_disjoint([0, 1], (left, right) -> {
        retain(left)
    })
}
"#,
        ),
        (
            "return",
            r#"
fn leak(values: &[Int]) ViewMut<Int> -[]> {
    return values.edit_disjoint([0, 1], (left, right) -> {
        return left
    }) ?? panic("selection failed")
}
fn run() {
    values := [1, 2]
    leak(&values)
}
"#,
        ),
    ];
    for (boundary, src) in cases {
        let diagnostics = jet::compile(src)
            .expect_err("an edit_disjoint callback view must not cross a retaining boundary");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0212"),
            "{boundary}: {diagnostics:?}"
        );
    }
}

#[test]
fn mutable_view_extraction_transfers_struct_option_and_enum_loans() {
    let src = r#"
struct Edit { values: ViewMut<Int> }
enum Choice {
    First(ViewMut<Int>)
    Second(ViewMut<Int>)
}

fn edit(values: &[Int]) Edit -[]> {
    return Edit{ values: &values[0..1] }
}

fn maybe(values: &[Int]) ?ViewMut<Int> -[]> {
    return Val(&values[0..1])
}

fn choose(values: &[Int], first: Bool) Choice -[]> {
    if first { return Choice.First(&values[0..1]) }
    return Choice.Second(&values[0..1])
}

fn run() {
    struct_values := [1, 2]
    aggregate :: edit(&struct_values)
    struct_part :: aggregate.values
    struct_part[0] = 3
    print(struct_part[0])

    option_values := [4, 5]
    optional :: maybe(&option_values)
    if optional == Val(option_part) {
        option_part[0] = 6
        print(option_part[0])
    }

    enum_values := [7, 8]
    selected :: choose(&enum_values, false)
    if selected == {
        .First(enum_part) -> {
            enum_part[0] = 9
            print(enum_part[0])
        }
        .Second(enum_part) -> {
            enum_part[0] = 10
            print(enum_part[0])
        }
    }
}
"#;
    jet::compile(src).expect("mutable aggregate extraction must transfer the exclusive loan");
    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_view_mut_extract", "view_mut_extract", src);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "3\n6\n10\n");
    }

    let root = common::unique_tmp("jet_view_mut_extract_quick_run");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, src).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", path.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .expect("run mutable aggregate extraction through the default tier");
    assert!(
        output.status.success(),
        "default mutable aggregate extraction failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "3\n6\n10\n");
}

#[test]
fn mutable_view_extraction_retires_the_source_aggregate() {
    let struct_src = r#"
struct Edit { values: ViewMut<Int> }

fn edit(values: &[Int]) Edit -[]> {
    return Edit{ values: &values[0..1] }
}

fn run() {
    values := [1, 2]
    aggregate :: edit(&values)
    selected :: aggregate.values
    print(aggregate.values[0])
    print(selected[0])
}
"#;
    let diags = jet::compile(struct_src)
        .expect_err("extracting an exclusive view must retire its source aggregate");
    assert!(diags.iter().any(|diag| diag.code == "E0121"), "{diags:?}");

    let option_src = r#"
fn maybe(values: &[Int]) ?ViewMut<Int> -[]> {
    return Val(&values[0..1])
}

fn run() {
    values := [1, 2]
    optional :: maybe(&values)
    if optional == Val(selected) {
        print(optional)
        print(selected[0])
    }
}
"#;
    let diags = jet::compile(option_src)
        .expect_err("extracting an exclusive option payload must retire its subject");
    assert!(diags.iter().any(|diag| diag.code == "E0121"), "{diags:?}");
}

#[test]
fn borrowed_mutable_view_enum_requires_take_before_destructuring() {
    let src = r#"
enum Choice {
    First(ViewMut<Int>)
    Second(ViewMut<Int>)
}

fn inspect(choice: Choice) {
    if choice == {
        .First(selected) -> print(selected[0])
        .Second(selected) -> print(selected[0])
    }
}

fn run() {
    values := [1, 2]
    choice :: Choice.First(&values[0..1])
    inspect(choice)
}
"#;
    let diags =
        jet::compile(src).expect_err("a borrowed non-cloneable enum cannot move out its payload");
    let matching: Vec<_> = diags.iter().filter(|diag| diag.code == "E0120").collect();
    assert_eq!(matching.len(), 1, "{diags:?}");
    assert!(matching[0].fix.contains("choice: ^Choice"), "{diags:?}");

    let take_src = src
        .replace("fn inspect(choice: Choice)", "fn inspect(choice: ^Choice)")
        .replace("inspect(choice)", "inspect(^choice)");
    jet::compile(&take_src).expect("taking the enum must permit moving out its exclusive view");
    if common::have_rustc() {
        let (code, _stdout, stderr) =
            common::build_and_run("jet_view_mut_enum_take", "view_mut_enum_take", &take_src);
        assert_eq!(code, 0, "{stderr}");
    }
}

#[test]
fn mutable_view_list_elements_must_be_disjoint() {
    let src = r#"
fn duplicate(values: &[Int]) [ViewMut<Int>] -[]> {
    return [&values[0..1], &values[0..1]]
}

fn run() { print(0) }
"#;
    let diags =
        jet::compile(src).expect_err("coexisting list elements cannot alias one mutable range");
    assert!(diags.iter().any(|diag| diag.code == "E0212"), "{diags:?}");
}

#[test]
fn enum_pattern_payload_keeps_its_original_owner_live() {
    let src = r#"
enum Selection { One(View<Int>) }

fn select(values: [Int]) Selection -[]> {
    selected :: values[0..1]
    return Selection.One(selected)
}

fn run() {
    values := [7, 8]
    selected :: select(values)
    if selected == {
        .One(part) -> {
            values.push(9)
            print(part[0])
        }
    }
}
"#;
    let diags =
        jet::compile(src).expect_err("a matched payload must keep the source owner borrowed");
    assert!(diags.iter().any(|diag| diag.code == "E0212"), "{diags:?}");
}

#[test]
fn recursive_view_summaries_converge_to_every_possible_owner() {
    let src = r#"
fn alpha(left: [Int], right: [Int], choose_left: Bool) View<Int> -[]> {
    if choose_left {
        selected :: left[0..1]
        return selected
    }
    return beta(left, right, choose_left)
}

fn beta(left: [Int], right: [Int], choose_left: Bool) View<Int> -[]> {
    if !choose_left {
        selected :: right[0..1]
        return selected
    }
    return alpha(left, right, choose_left)
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    selected :: alpha(left, right, false)
    right.push(11)
    print(selected[0])
}
"#;
    let diags =
        jet::compile(src).expect_err("recursive summaries must retain the right-owner path");
    assert!(diags.iter().any(|diag| diag.code == "E0212"), "{diags:?}");
}

#[test]
fn recursive_view_aggregate_graph_terminates_without_ice() {
    let src = r#"
struct Node { next: ?Node, values: View<Int> }

fn node(values: [Int]) Node -[]> {
    selected :: values[0..1]
    return Node{ next: None, values: selected }
}

fn run() { print(0) }
"#;
    let out = jet::compile(src).expect("recursive view graph must terminate in sema and codegen");
    assert!(
        out.rust.contains("pub struct __jet_Node<'__jet___view>"),
        "{}",
        out.rust
    );
    assert!(
        out.rust.contains("__jet_Node<'__jet___view>"),
        "{}",
        out.rust
    );
}

#[test]
fn returned_aggregate_accepts_distinct_sources_per_output_slot() {
    let src = r#"
struct Pair { left: View<Int>, right: View<Int> }

fn pair(left: [Int], right: [Int]) Pair -[]> {
    left_view :: left[0..1]
    right_view :: right[0..1]
    return Pair{ left: left_view, right: right_view }
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    result :: pair(left, right)
    print(result.left[0])
    print(result.right[0])
}
"#;
    jet::compile(src).expect("each returned output field may name its own stable owner");
}

#[test]
fn multi_source_returned_aggregate_still_blocks_owner_invalidation() {
    for (owner, field) in [("left", "left"), ("right", "right")] {
        let src = r#"
struct Pair { left: View<Int>, right: View<Int> }

fn pair(left: [Int], right: [Int]) Pair -[]> {
    left_view :: left[0..1]
    right_view :: right[0..1]
    return Pair{ left: left_view, right: right_view }
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    result :: pair(left, right)
    $OWNER.push(11)
    print(result.$FIELD[0])
}
"#
        .replace("$OWNER", owner)
        .replace("$FIELD", field);
        let diags =
            jet::compile(&src).expect_err("each returned output slot must keep its own owner live");
        assert!(
            diags.iter().any(|diag| diag.code == "E0212"),
            "{owner}/{field}: {diags:?}"
        );
    }
}

#[test]
fn aggregate_view_slot_composes_through_parameter_projection() {
    let src = r#"
struct Pair { left: View<Int>, right: View<Int> }

fn pair(left: [Int], right: [Int]) Pair -[]> {
    left_view :: left[0..1]
    right_view :: right[0..1]
    return Pair{ left: left_view, right: right_view }
}

fn first(pair: Pair) View<Int> -[]> {
    return pair.left
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    result :: pair(left, right)
    selected :: first(result)
    left.push(11)
    print(selected[0])
}
"#;
    let diags =
        jet::compile(src).expect_err("the projected left slot must retain its original owner");
    assert!(diags.iter().any(|diag| diag.code == "E0212"), "{diags:?}");
}

#[test]
fn returned_view_aggregate_cannot_cross_task_boundary() {
    let src = r#"
struct Window { values: View<Int> }

fn window(values: [Int]) Window -[]> {
    selected :: values[0..1]
    return Window{ values: selected }
}

fn run() {
    handle :: task window([7, 8])
    print((handle.join() ?? Window{ values: [] }).values[0])
}
"#;
    let diags = jet::compile(src).expect_err("view aggregates are not task-sendable");
    assert!(diags.iter().any(|d| d.code == "E1102"), "{diags:?}");
}

#[test]
fn nested_returned_view_paths_with_same_source_compile() {
    let src = r#"
fn choose(xs: [Int]) View<Int> -[]> {
    if true {
        return xs[0..1]
    }
    return xs[1..2]
}

fn run() {
    print(0)
}
"#;
    jet::compile(src).expect("all returned-view paths share parameter 0");
}

#[test]
fn returned_view_paths_form_a_safe_source_union() {
    let src = r#"
fn choose(left: [Int], right: [Int], first: Bool) View<Int> -[]> {
    if first {
        return left[0..1]
    }
    return right[0..1]
}

fn run() {
    left := [7, 8]
    right := [9, 10]
    print(choose(left, right, true)[0])
    print(choose(left, right, false)[0])
}
"#;
    let out = jet::compile(src).expect("both compatible parameter owners form one source union");
    assert!(
        out.rust.contains("__jet_left: &'__jet___view Vec<i64>")
            && out.rust.contains("__jet_right: &'__jet___view Vec<i64>")
            && out.rust.contains("-> &'__jet___view [i64]"),
        "{}",
        out.rust
    );
}

#[test]
fn borrowed_parameter_binding_never_inserts_hidden_copy() {
    let src = r#"
fn bind_again(text: String) {
    alias :: text
    print(alias)
}

fn run() {
    bind_again("hello")
}
"#;
    let diags =
        jet::compile(src).expect_err("borrowed parameter binding must need explicit ownership");
    let escape = diags
        .iter()
        .find(|diag| diag.code == "E0120")
        .expect("borrowed binding must report E0120 instead of cloning");
    assert!(
        escape.fix.contains("~text"),
        "fix must name explicit copy: {escape:?}"
    );
}

#[test]
fn ownership_copy_edit_grade_tracks_cloneability() {
    let safe_src = r#"
#Policy(copies: .Explicit)
struct Holder {
    value: String
}

fn wrap(value: String) Holder -> { return Holder{value: value} }

fn run() { print(0) }
"#;
    let safe_diags = jet::compile(safe_src).expect_err("explicit copy policy must report E0120");
    let safe = safe_diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E0120")
        .expect("safe E0120");
    assert_eq!(
        safe.applicability,
        Some(jet::Diagnostics::FixApplicability::Safe)
    );
    assert_eq!(
        safe.safety,
        Some(jet::Diagnostics::FixSafety::BehaviorPreserving)
    );
    let safe_edit = safe.edit.as_ref().expect("safe E0120 must carry an edit");
    assert_eq!(safe_edit.new_text, "~");
    assert_eq!(&safe_src[safe_edit.span.start..safe_edit.span.end], "");
    let safe_fixed = jet::LSP::apply_edit(safe_src, safe_edit);
    assert!(
        jet::compile(&safe_fixed).is_ok(),
        "the safe `~` edit must clear the E0120 report: {safe_fixed}"
    );

    let suggested_src = r#"
struct Holder {
    value: fn(Int) Int
}

fn wrap(value: fn(Int) Int) Holder -> { return Holder{value: value} }

fn run() { print(0) }
"#;
    let suggested_diags =
        jet::compile(suggested_src).expect_err("non-cloneable copy must report E0120");
    let suggested = suggested_diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E0120")
        .expect("suggested E0120");
    assert_eq!(
        suggested.applicability,
        Some(jet::Diagnostics::FixApplicability::Suggested)
    );
    assert_eq!(
        suggested.safety,
        Some(jet::Diagnostics::FixSafety::NeedsReview)
    );
    let suggested_edit = suggested
        .edit
        .as_ref()
        .expect("suggested E0120 must carry an edit");
    assert_eq!(suggested_edit.new_text, "~");
    assert_eq!(
        &suggested_src[suggested_edit.span.start..suggested_edit.span.end],
        ""
    );
    let suggested_fixes = jet::LSP::fixes_from_diagnostics(suggested_diags.clone());
    assert_eq!(suggested_fixes.len(), 1);
    assert!(
        jet::LSP::safe_fixes(&suggested_fixes).is_empty(),
        "a review-only `~` suggestion must never be auto-applied"
    );
}

#[test]
fn core_string_view_copy_edit_names_consuming_expression() {
    let src = include_str!("ui/core_string_view_reuse_after_json.jet");
    let diags = jet::compile(src).expect_err("explicit-copy Core boundary must report E2307");
    let diagnostic = diags
        .iter()
        .find(|diagnostic| diagnostic.code == "E2307")
        .expect("the Core string-view refusal must be present");
    assert_eq!(
        diagnostic.applicability,
        Some(jet::Diagnostics::FixApplicability::Safe)
    );
    assert_eq!(
        diagnostic.safety,
        Some(jet::Diagnostics::FixSafety::BehaviorPreserving)
    );
    let edit = diagnostic
        .edit
        .as_ref()
        .expect("the named Core expression must carry a structured copy edit");
    assert_eq!(edit.new_text, "~");
    assert_eq!(&src[edit.span.start..edit.span.end], "");
    let diagnostic_span = diagnostic.span.expect("the Core expression must be highlighted");
    assert_eq!(edit.span.start, diagnostic_span.start);
    assert_eq!(&src[diagnostic_span.start..diagnostic_span.end], "view");
    let fixed = jet::LSP::apply_edit(src, edit);
    assert!(
        jet::compile(&fixed).is_ok(),
        "applying the named Core copy edit must clear the report: {fixed}"
    );

    // This is a non-Jetpack witness for the complete fix path. The explicit
    // copy is the only source change; both execution tiers must preserve the
    // output produced by the now-owning JSON value and the later view.
    let expected = "alue\n\"value\"\n";
    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_string_view_core_reuse", "core_reuse", &fixed);
        assert_eq!(code, 0, "AOT witness failed: {stderr}");
        assert_eq!(stdout, expected, "AOT witness output drift");
    }

    let root = common::unique_tmp("jet_string_view_core_reuse_default");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("main.jet");
    fs::write(&path, fixed).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jet"))
        .args(["run", path.to_str().unwrap()])
        .current_dir(&root)
        .output()
        .expect("run the fixed Core witness through the default tier");
    assert!(
        output.status.success(),
        "default witness failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
}

/// D-MEM-COPYSEM1=A: a cloneable read value entering an owning destination is
/// materialized automatically (spec.md:339-340), and the return slot is an
/// owning destination — `tests/ui/return_borrowed_param.stderr` is the same
/// shape for a concrete `String` and expects no errors at all. E0120 survives
/// only for the cases spec.md:340 and diagnostic-rows.md:115 name: a
/// non-cloneable value or `copies: .Explicit`.
///
/// The property this test defends is unchanged: a read parameter never escapes
/// as owned WITHOUT an ownership decision. For a generic the decision cannot be
/// made at the definition, so the compiler pays for it visibly — it borrows the
/// parameter, materializes at the return, and puts the resulting `Clone`
/// obligation in the signature where every caller must satisfy it. Assert all
/// three, because dropping any one of them is a different (and wrong) compiler:
/// `&T` alone would allow a move out of a borrow, and `T: Clone` alone would
/// allow the parameter to be taken by value.
#[test]
fn generic_borrowed_parameter_materializes_an_owned_return() {
    let src = r#"
fn identity<T>(value: T) T -[]> {
    return value
}

fn run() {
    print(0)
}
"#;
    let out =
        jet::compile(src).expect("a cloneable read parameter materializes at an owned return");
    assert!(
        out.rust
            .contains("pub fn __jet_identity<T: Clone>(__jet_value: &T) -> T {"),
        "the escape must be paid for in the signature, not laundered: {}",
        out.rust
    );
    assert!(
        out.rust.contains("return ((*__jet_value)).clone();"),
        "the owned return must copy through the borrow, never move out of it: {}",
        out.rust
    );
    // The boundary, from the same output: nothing turned the read parameter
    // into an owned one. A compiler that "solved" the escape by taking the
    // parameter by value would satisfy both assertions above while quietly
    // changing the signature's access.
    assert!(
        !out.rust.contains("__jet_value: T"),
        "the parameter must stay a read borrow, not become an owned slot: {}",
        out.rust
    );
    // The refusal rail this test used to own now lives with the cases
    // diagnostic-rows.md:115 names — a non-cloneable value or
    // `copies: .Explicit` (tests/ui/param_owned_field_needs_copy_explicit) —
    // and the `Clone` obligation is only added where a copy is actually
    // demanded (`generic_clone_bound_is_usage_sensitive`, above).
}

#[test]
fn every_unmarked_nonscalar_parameter_has_read_borrow_rust_shape() {
    let src = r#"
struct Parcel {
    label: String
}

impl Parcel {
    fn show(self) {
        print(self.label)
    }
}

fn read_text(text: String) {
    if true {
        print(text)
    }
}

fn read_list(values: [Int]) {
    print(values.len())
}

fn read_parcel(parcel: Parcel) {
    parcel.show()
}

fn read_generic<T>(value: T) {
    print(0)
}

fn apply(f: fn(Int) Int, value: Int) Int -[]> {
    return f(value)
}

fn run() {
    text :: "hello"
    values :: [1, 2, 3]
    parcel :: Parcel{ label: "parcel" }
    read_text(text)
    read_list(values)
    read_parcel(parcel)
    read_generic(text)
    print(apply((n: Int) -> (n + 1), 41))
}
"#;
    let out = jet::compile(src).expect("all plain parameters are read borrows");
    assert!(out.rust.contains("__jet_text: &String"), "{}", out.rust);
    assert!(out.rust.contains("__jet_values: &Vec<i64>"), "{}", out.rust);
    assert!(
        out.rust.contains("__jet_parcel: &__jet_Parcel"),
        "{}",
        out.rust
    );
    assert!(out.rust.contains("__jet_value: &T"), "{}", out.rust);
    // A function value is stored as `Rc` since f003de290 (2026-07-31) so
    // collections can clone it. The property under test is the leading `&`:
    // every unmarked non-scalar parameter, callbacks included, arrives borrowed
    // (spec.md:338-341 — "allocation-free for every non-scalar shape,
    // including strings, collections, structs, generic values, and callbacks").
    assert!(
        out.rust.contains("__jet_f: &std::rc::Rc<dyn Fn"),
        "{}",
        out.rust
    );
    // The read-only generic takes no `Clone`: a borrow that never escapes must
    // not levy a copy obligation on callers (`generic_clone_bound_is_usage_
    // sensitive` states the other direction).
    assert!(
        out.rust
            .contains("pub fn __jet_read_generic<T>(__jet_value: &T)"),
        "a read-only generic parameter must stay bound-free: {}",
        out.rust
    );
    assert!(
        !out.rust.contains("((*__jet_text)).clone()"),
        "{}",
        out.rust
    );
}

#[test]
fn function_value_calls_preserve_plain_parameter_read_borrows() {
    let src = r#"
fn inspect(value: String) Int -[]> { return value.len() }

fn apply(f: fn(String) Int, value: String) Int -[]> {
    return f(value)
}

fn run() {
    print(apply(inspect, "hello"))
}
"#;
    let out = jet::compile(src).expect("function-value call must preserve read access");
    // f003de290 (2026-07-31) stores function values as `Rc` so a collection can
    // clone them; `Box` was the container, never the property. The property is
    // the `&`: the callback crosses this call as a read borrow, so the caller
    // still owns its function value afterwards. Pin the whole prefix — the
    // parameter's own borrow AND the callback's read-borrowed `&String`
    // argument — so neither half can drift into an owned slot unnoticed.
    assert!(
        out.rust.contains("__jet_f: &std::rc::Rc<dyn Fn(&String)"),
        "the function value must arrive as a read borrow: {}",
        out.rust
    );
    assert!(out.rust.contains("__jet_value: &String"), "{}", out.rust);
    assert!(
        out.rust.contains("((*__jet_f))(&((*__jet_value)))"),
        "{}",
        out.rust
    );
    assert!(
        !out.rust.contains("((*__jet_value)).clone()"),
        "{}",
        out.rust
    );
}

#[test]
fn function_value_returning_view_preserves_owner_provenance() {
    let src = r#"
fn first(values: [Int]) View<Int> -[]> {
    return values[0..1]
}

fn run() {
    callback :: first
    values := [7, 8]
    result :: callback(values)
    values.push(9)
    print(result[0])
}
"#;
    let diags = jet::compile(src).expect_err("callback result must keep its owner live");
    assert!(diags.iter().any(|d| d.code == "E0212"), "{diags:?}");
}

/// The callback's returned window must stay tied to the list the callback was
/// handed, and the wrapper's own return must stay tied to the wrapper's own
/// parameter. Both are one-lifetime-on-both-ends facts, and they are what makes
/// a later `values.push(…)` reachable as E0212 through a function value.
///
/// 56f4f7bd4 (2026-07-31) gave the callback's binder its own name: reusing the
/// enclosing function's `'__jet___view` inside a `for<…>` binder is an E0496
/// shadow whenever a view-returning fn takes a view-returning callback. The
/// rename is why the old spelling no longer appears; the provenance it carries
/// is unchanged, so assert the whole trait shape rather than one lifetime name,
/// and assert the two binders stay distinct.
#[test]
fn parameter_rooted_lambda_and_generic_callback_preserve_view_provenance() {
    let src = r#"
fn apply(callback: fn([Int]) View<Int>, values: [Int]) View<Int> -[]> {
    return callback(values)
}

fn run() {
    values := [7, 8]
    selected :: apply((items: [Int]) -> items[0..1], values)
    print(selected[0])
}
"#;
    let out = jet::compile(src).expect("callback provenance is hidden in the function value");
    // One binder, on the callback's argument AND on its result: the window the
    // callback returns can only come from the list it was given.
    assert!(
        out.rust.contains(
            "dyn for<'__jet___fn_view> Fn(&'__jet___fn_view Vec<i64>) -> &'__jet___fn_view [i64]"
        ),
        "the callback must return a window into its own argument: {}",
        out.rust
    );
    // The wrapper's own return is tied to the wrapper's own parameter.
    assert!(
        out.rust
            .contains("__jet_values: &'__jet___view Vec<i64>) -> &'__jet___view [i64]"),
        "the wrapper's returned window must be tied to its list parameter: {}",
        out.rust
    );
    // The E0496 half of the same shape: the callback's binder is not the
    // enclosing function's lifetime, so the two windows are never conflated.
    assert!(
        !out.rust.contains("dyn for<'__jet___view>"),
        "the callback binder must not shadow the enclosing view lifetime: {}",
        out.rust
    );
}

#[test]
fn function_values_keep_exact_view_owner_identity() {
    let src = r#"
fn first(left: [Int], right: [Int]) View<Int> -[]> {
    return left[0..1]
}

fn run() {
    left := [7, 8]
    right := [9]
    callback :: first
    selected :: callback(left, right)
    right.push(10)
    print(selected[0])
    print(right[1])
}
"#;
    jet::compile(src).expect("unrelated callback arguments must not borrow the returned view");
    if common::have_rustc() {
        let (code, stdout, stderr) = common::build_and_run(
            "jet_exact_callback_view_owner",
            "exact_callback_view_owner",
            src,
        );
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "7\n10\n");
    }
}

#[test]
fn lambdas_keep_exact_view_owner_identity() {
    let src = r#"
fn run() {
    left := [7, 8]
    right := [9]
    callback :: (first: [Int], second: [Int]) -> first[0..1]
    selected :: callback(left, right)
    right.push(10)
    print(selected[0])
    print(right[1])
}
"#;
    jet::compile(src).expect("unrelated lambda arguments must not borrow the returned view");
    if common::have_rustc() {
        let (code, stdout, stderr) = common::build_and_run(
            "jet_exact_lambda_view_owner",
            "exact_lambda_view_owner",
            src,
        );
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "7\n10\n");
    }
}

#[test]
fn stored_lambda_returning_view_is_rejected_before_codegen() {
    let src = r#"
fn first(values: [Int]) View<Int> -[]> {
    return values[0..1]
}

fn run() {
    values := [7, 8]
    callback :: () -> first(values)
    print(0)
}
"#;
    let diags = jet::compile(src).expect_err("stored lambda view source cannot stabilize");
    assert!(diags.iter().any(|d| d.code == "E2305"), "{diags:?}");
}

#[test]
fn returned_view_composes_from_receiver_field() {
    let src = r#"
struct Bucket { values: [Int] }
impl Bucket {
    fn first(self) View<Int> -[]> {
        return self.values[0..1]
    }
}
fn wrapper(bucket: Bucket) View<Int> -[]> {
    return bucket.first()
}
fn run() { print(0) }
"#;
    let out = jet::compile(src).expect("receiver-rooted method view must compose");
    assert!(out.rust.contains("&'__jet___view self"), "{}", out.rust);
}

#[test]
fn generic_returned_view_composes_through_wrapper() {
    let src = r#"
fn first<T>(values: [T]) View<T> -[]> {
    return values[0..1]
}
fn wrapper(values: [Int]) View<Int> -[]> {
    return first(values)
}
fn run() {
    values := [7, 8]
    result :: wrapper(values)
    print(result[0])
}
"#;
    jet::compile(src).expect("generic instantiation provenance must compose through wrapper");
}

#[test]
fn open_dynamic_trait_view_dispatch_is_rejected() {
    let src = r#"
trait Select {
    fn select(self, left: [Int], right: [Int]) View<Int>
}
fn wrapper(selector: Select, left: [Int], right: [Int]) View<Int> -[]> {
    return selector.select(left, right)
}
fn run() { print(0) }
"#;
    let diags = jet::compile(src).expect_err("open trait dispatch has no stable source");
    assert!(diags.iter().any(|d| d.code == "E2305"), "{diags:?}");
}

#[test]
fn returned_view_blocks_owner_resize_and_move() {
    for action in ["values.push(4)", "consume(^values)"] {
        let src = format!(
            r#"fn first(values: [Int]) View<Int> -[]> {{
    return values[0..1]
}}
fn consume(values: ^[Int]) {{ print(values.len()) }}
fn run() {{
    values := [1, 2, 3]
    result :: first(values)
    {action}
    print(result[0])
}}
"#
        );
        let diags = jet::compile(&src).expect_err("live returned view must keep owner stable");
        assert!(diags.iter().any(|d| d.code == "E0212"), "{diags:?}");
    }
}

#[test]
fn returned_view_aggregate_blocks_owner_resize() {
    let src = r#"
struct Window { values: View<Int> }
fn window(values: [Int]) Window -[]> {
    selected :: values[0..1]
    return Window{ values: selected }
}
fn run() {
    values := [1, 2, 3]
    result :: window(values)
    values.push(4)
    print(result.values[0])
}
"#;
    let diags = jet::compile(src).expect_err("stored returned view must keep owner stable");
    assert!(diags.iter().any(|d| d.code == "E0212"), "{diags:?}");
}

#[test]
fn returned_mutable_view_conflicts_with_overlapping_view() {
    let src = r#"
fn edit(values: &[Int]) ViewMut<Int> -[]> {
    return &values[0..1]
}
fn run() {
    values := [1, 2, 3]
    left :: edit(&values)
    right :: &values[0..1]
    print(left[0] + right[0])
}
"#;
    let diags = jet::compile(src).expect_err("returned mutable view must remain exclusive");
    assert!(diags.iter().any(|d| d.code == "E0212"), "{diags:?}");
}

#[test]
fn returned_view_can_be_carried_in_a_list() {
    let src = r#"
fn first(values: [Int]) View<Int> -[]> {
    return values[0..1]
}
fn run() {
    values := [1, 2, 3]
    result :: first(values)
    windows :: [result]
    print(windows[0][0])
}
"#;
    jet::compile(src).expect("list element provenance remains attached to its owner");
}

#[test]
fn stored_view_field_cannot_be_rebound_to_different_owner() {
    let src = r#"
struct Window { values: View<Int> }
fn replace(left: [Int], right: [Int]) {
    first :: left[0..1]
    holder := Window{ values: first }
    second :: right[0..1]
    holder.values = second
    print(holder.values[0])
}
fn run() { print(0) }
"#;
    let diags = jet::compile(src).expect_err("stored view field source cannot change");
    assert!(diags.iter().any(|d| d.code == "E2305"), "{diags:?}");
}

#[test]
fn named_write_and_move_functions_cannot_erase_access_as_function_values() {
    for src in [
        r#"
fn edit(value: &String) { print(value) }
fn run() { callback :: edit }
"#,
        r#"
fn consume(value: ^String) { print(value) }
fn run() { callback :: consume }
"#,
    ] {
        let diags = jet::compile(src).expect_err("function-value coercion must preserve access");
        let mismatch = diags.iter().find(|d| d.code == "E0112").expect("E0112");
        assert!(mismatch.why.contains("plain read access"), "{mismatch:?}");
    }
}

#[test]
fn function_value_call_rejects_move_and_tracks_aliases_per_call() {
    let moved = r#"
fn inspect(value: String) { print(value) }
fn run() {
    callback :: inspect
    value :: "hello"
    callback(^value)
}
"#;
    let diags = jet::compile(moved).expect_err("read callback must reject explicit move");
    assert!(diags.iter().any(|d| d.code == "E0203"), "{diags:?}");

    let aliased = r#"
fn both(a: String, b: String) { print(a); print(b) }
fn run() {
    callback :: both
    value := "hello"
    callback(&value, value)
}
"#;
    let diags = jet::compile(aliased).expect_err("callback arguments must preserve alias rules");
    assert_eq!(
        diags.iter().filter(|d| d.code == "E0204").count(),
        1,
        "alias state must stay within this call and flow across its arguments: {diags:?}"
    );
}

#[test]
fn borrowed_parameter_subplaces_copy_in_owning_positions() {
    let cases = [
        r#"
struct Parcel { label: String }
struct Holder { value: String }
fn wrap(parcel: Parcel) Holder -[]> { return Holder{ value: parcel.label } }
fn run() { print(0) }
"#,
        r#"
enum Wrapped { Val(String) }
fn wrap(values: [String]) Wrapped -[]> { return Wrapped.Val(values[0]) }
fn run() { print(0) }
"#,
        r#"
struct Parcel { label: String }
fn replace(parcel: Parcel) {
    owned := "old"
    owned = parcel.label
}
fn run() { print(0) }
"#,
    ];
    for src in cases {
        jet::compile(src).expect("an owning destination must materialize a read subplace");
    }
}

/// Two windows over the same subplace of a read parameter, in the two positions
/// that decide differently, in one function so neither can disturb the other:
/// spec.md:339-340 — "a cloneable read value entering an owning destination is
/// materialized automatically; a bare `::` binding remains a read window" — and
/// spec.md:438-439, where a place ends in "its maximal field, index, or range
/// projection", so a FIELD place is the same window a range place is.
///
/// The function value is the trap: `callback(parcel.label, 1)` is a borrow
/// position, and a copy inserted there would hand the callback a fresh
/// allocation instead of the caller's own storage, silently breaking
/// spec.md:338's allocation-free read call.
#[test]
fn function_value_borrow_context_preserves_parameter_place_window() {
    let src = r#"
struct Parcel { label: String }
fn inspect(text: String, n: Int) { print(text); print(n) }
fn apply_to(parcel: Parcel, callback: fn(String, Int)) {
    callback(parcel.label, 1)
    alias :: parcel.label
}
fn run() { apply_to(Parcel{ label: "hello" }, inspect) }
"#;
    let out = jet::compile(src).expect("bare parameter subplace is a read window");
    assert!(
        out.rust.contains("__jet_parcel: &__jet_Parcel"),
        "{}",
        out.rust
    );
    // The borrow position: the subplace is reborrowed out of the parameter's
    // own storage, not materialized.
    assert!(
        out.rust
            .contains("((*__jet_callback))(&(((*__jet_parcel)).__jet_label),"),
        "the callback argument must reborrow the subplace: {}",
        out.rust
    );
    // The bare `::` binding: still a read window.
    assert!(out.rust.contains("let __jet_alias = &"), "{}", out.rust);
    assert!(
        !out.rust.contains("((*__jet_parcel)).__jet_label).clone()"),
        "a bare `::` binding must not allocate a copy of the subplace: {}",
        out.rust
    );
}

/// Three refusal rails, one per access the caller failed to write. The middle
/// one is the load-bearing case for D-MEM-COPYSEM1: a move (`^`) parameter is
/// an owning slot, so the automatic materialization applies to it too — but a
/// bare NAME at that slot is E0209 first ("a named binding passed where it
/// would be silently cloned — Move-param arg without the move marker `^`",
/// diagnostic-rows.md:167; "no clone is ever silent", D-MEM1/S2). If the
/// materialization is allowed to rewrite the name before that check reads it,
/// the hard error disappears and the clone becomes silent — which is why the
/// convention check reads the argument as it was WRITTEN.
///
/// The third rail is the survivor of the E0120 side: a `fn` value is not
/// cloneable (`is_cloneable`, Sema/Diagnostics.rs:502), so nothing materializes
/// it and the escape is still refused outright.
#[test]
fn plain_parameter_write_take_and_escape_diagnostics_name_explicit_access() {
    let write_src = r#"
fn edit(values: [Int]) {
    values[0] = 4
}
fn run() { print(0) }
"#;
    let write = jet::compile(write_src).expect_err("plain list parameter cannot be edited");
    let write = write
        .iter()
        .find(|d| d.code == "E0205")
        .unwrap_or_else(|| panic!("expected E0205: {write:?}"));
    assert!(write.fix.contains("&[Int]"), "{write:?}");

    let take_src = r#"
fn consume<T>(value: ^T) { print(0) }
fn relay<T>(value: T) { consume(value) }
fn run() { print(0) }
"#;
    let take = jet::compile(take_src).expect_err("plain generic parameter cannot be consumed");
    let take = take.iter().find(|d| d.code == "E0209").expect("E0209");
    assert!(take.fix.contains("^value"), "{take:?}");

    let callback_src = r#"
fn keep(f: fn(Int) Int) fn(Int) Int -[]> { return f }
fn run() { print(0) }
"#;
    let escape = jet::compile(callback_src).expect_err("plain callback parameter cannot escape");
    assert!(escape.iter().any(|d| d.code == "E0120"), "{escape:?}");
}

#[test]
fn borrowed_parameter_fills_an_owned_struct_field_with_an_implicit_copy() {
    let src = r#"
struct Holder { value: String }
fn wrap(value: String) Holder -[]> {
    return Holder{ value: value }
}
fn run() { print(0) }
"#;
    jet::compile(src).expect("an owned field must materialize a read parameter");
}

#[test]
fn stored_lambda_materializes_a_read_view_capture() {
    let src = r#"
fn store(text: String) fn() String -[..E]> -[..E]> {
    domain :: text.after("@")
    return () -> domain
}
fn run() {
    callback :: store("nate@jet-lang.dev")
    print(callback())
}
"#;
    let compiled = jet::compile(src).expect("stored read-view capture must become owning");
    assert!(
        compiled.rust.contains("jet_string_view_copy"),
        "{}",
        compiled.rust
    );
    if common::have_rustc() {
        let (code, stdout, stderr) =
            common::build_and_run("jet_stored_view_capture", "whole_root", src);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(stdout, "jet-lang.dev\n");
    }
}

#[test]
fn owning_collection_elements_materialize_read_views_without_expected_type() {
    let src = r#"
fn collect(text: String) [String] -[]> {
    domain :: text.after("@")
    values :: [domain]
    return values
}
fn run() {
    print(collect("nate@jet-lang.dev")[0])
}
"#;
    let compiled = jet::compile(src).expect("an owning collection element must materialize");
    assert!(
        compiled.rust.contains("jet_string_view_copy"),
        "{}",
        compiled.rust
    );
}

#[test]
fn borrowed_parameter_can_feed_stored_lambda_after_explicit_copy() {
    let src = r#"
fn store(text: String) {
    owned :: ~text
    callback :: () -> owned.len()
    print(callback())
}
fn run() { store("read") }
"#;
    jet::compile(src).expect("explicitly copied capture should compile");
}

#[test]
fn borrowed_parameter_can_feed_task_after_explicit_copy() {
    let src = r#"
fn launch(text: String) {
    owned :: ~text
    handle :: task owned.len()
    print(handle.join() ?? 0)
}
fn run() { launch("read") }
"#;
    jet::compile(src).expect("explicitly copied task capture should compile");
}

#[test]
fn two_binding_loop_over_owned_task_list_compiles() {
    let src = r#"
fn run() {
    handles :: [task 1, task 2]
    total := 0
    loop (i, h) in handles { total += (h.join() ?? 0) + i }
    print(total)
}
"#;
    jet::compile(src).expect("owned two-binding task loop must compile");
}

#[test]
fn nested_task_list_loop_compiles() {
    let src = r#"
fn run() {
    groups :: [[task 1], [task 2]]
    n := 0
    loop g in groups { n += g.len() }
    print(n)
}
"#;
    jet::compile(src).expect("loop over [[Task]] must compile by value");
}

#[test]
fn mutable_task_list_drain_discharges_linear_duty() {
    let src = r#"
fn run() {
    workers := [Task<Int>]{}
    workers.push(task 1)
    loop workers.len() > 0 {
        worker :: workers.pop() ?? panic("missing worker")
        print(worker.join() ?? 0)
    }
}
"#;
    jet::compile(src).expect("a proven pop-to-empty loop must discharge the task-list duty");
}

#[test]
fn task_list_drain_that_reinserts_still_owes_consume() {
    let src = r#"
fn run() {
    workers := [Task<Int>]{}
    workers.push(task 1)
    loop workers.len() > 0 {
        worker :: workers.pop() ?? panic("missing worker")
        print(worker.join() ?? 0)
        workers.push(task 2)
    }
}
"#;
    let diags = jet::compile(src).expect_err("a drain that reinserts is not proven empty");
    let duty = diags.iter().find(|d| d.code == "E0140").expect("E0140");
    assert!(duty.what.contains("workers"), "{duty:?}");
}

#[test]
fn moved_task_list_parameter_loop_compiles() {
    let src = r#"
fn drain(hs: ^[Task<Int>]) Int -[]> {
    total := 0
    loop h in hs { total += h.join() ?? 0 }
    total
}
fn run() {
    handles :: [task 1, task 2]
    print(drain(^handles))
}
"#;
    jet::compile(src).expect("moved task-list parameter loop must compile");
}

#[test]
fn two_binding_borrowed_task_list_reports_e0120() {
    let src = r#"
fn drain(hs: [Task<Int>]) Int -[]> {
    total := 0
    loop (i, h) in hs { total += (h.join() ?? 0) + i }
    total
}
fn run() {
    handles :: [task 1]
    print(drain(handles))
}
"#;
    let diags = jet::compile(src).expect_err("borrowed two-binding task loop needs ^");
    assert!(diags.iter().any(|d| d.code == "E0120"), "{diags:?}");
}

#[test]
fn task_list_index_loop_reports_e0120() {
    let src = r#"
fn run() {
    groups :: [[task 1]]
    total := 0
    loop h in groups[0] { total += h.join() ?? 0 }
    print(total)
}
"#;
    let diags = jet::compile(src).expect_err("index projection cannot be consumed by value");
    let escape = diags.iter().find(|d| d.code == "E0120").expect("E0120");
    assert!(
        escape.what.contains("field or index"),
        "must not rewrite the root as the list: {escape:?}"
    );
    assert!(
        !escape.fix.contains("groups: ^"),
        "must not suggest taking the outer list as [Task]: {escape:?}"
    );
}

#[test]
fn stream_reuse_after_loop_reports_e0121() {
    let src = r#"
fn count(n: Int) Stream<Int> -[]> {
    i := 0
    loop i < n {
        yield i
        i += 1
    }
}
fn run() {
    s :: count(3)
    loop x in s { print("{x}") }
    loop x in s { print("{x}") }
}
"#;
    let diags = jet::compile(src).expect_err("Stream is consumed by the first loop");
    assert!(diags.iter().any(|d| d.code == "E0121"), "{diags:?}");
}

/// #1350: a lone write borrow must print the same value on default `jet run`
/// and AOT. Covers the measured matrix (local += / field assign / through
/// parameter / nested index).
#[test]
fn lone_write_borrow_matches_on_jit_and_aot() {
    let cases = [
        (
            "local_add",
            r#"
struct P { position: Int }
fn run() {
  ps := [P{ position: 10 }, P{ position: 20 }]
  a :: &ps[0]
  a.position += 2
  print(ps[0].position)
}
"#,
            "12\n",
        ),
        (
            "local_field",
            r#"
struct P { position: Int }
fn run() {
  ps := [P{ position: 1 }, P{ position: 2 }]
  a :: &ps[0]
  a.position = 42
  print(ps[0].position)
}
"#,
            "42\n",
        ),
        (
            "through_param",
            r#"
struct P { position: Int }
struct W { ps: [P] }
fn bump(w: &W) {
  a :: &w.ps[0]
  a.position = 42
}
fn run() {
  w := W{ ps: [P{ position: 1 }] }
  bump(&w)
  print(w.ps[0].position)
}
"#,
            "42\n",
        ),
        (
            "nested_index",
            r#"
fn run() {
  grid := [[1, 2], [3, 4]]
  a :: &grid[0][1]
  a = 42
  print(grid[0][1])
}
"#,
            "42\n",
        ),
    ];
    for (name, source, expected) in cases {
        let out = jet::compile(source)
            .unwrap_or_else(|diags| panic!("{name} must compile for AOT emit: {diags:?}"));
        if name == "nested_index" {
            assert!(
                out.rust.contains("jet_index_vec_mut"),
                "{name} must borrow the live inner list, not a clone: {}",
                out.rust
            );
            assert!(
                out.rust
                    .contains("let __jet___place_plan_0_root = &mut ((*jet_index_vec_mut("),
                "{name} split-view root must use jet_index_vec_mut: {}",
                out.rust
            );
        }
        let root = common::unique_tmp(&format!("jet_lone_write_{name}"));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("main.jet");
        fs::write(&path, source).unwrap();
        for release in [false, true] {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_jet"));
            cmd.arg("run");
            if release {
                cmd.arg("--release");
            }
            cmd.arg(path.to_str().unwrap()).current_dir(&root);
            let output = cmd
                .output()
                .unwrap_or_else(|err| panic!("jet run failed to spawn for {name}: {err}"));
            assert!(
                output.status.success(),
                "{name} release={release} failed:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                expected,
                "{name} release={release}"
            );
        }
    }
}

#[test]
fn declared_trait_from_allows_dyn_view_return_on_jit() {
    let dir = std::env::temp_dir().join(format!("jet_view_from_trait_dyn_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.jet");
    std::fs::write(
        &path,
        r#"
trait Slice {
    fn head(self) View<Int> from self -[]>
}

struct Packet {
    data: [Int]
}

impl Packet.Slice {
    fn head(self) View<Int> from self -> self.data[0..1]
}

fn first(s: Slice) View<Int> from s -[..E]> {
    return s.head()
}

fn run() {
    boxed :: [Slice]{Packet{ data: [9, 8, 7] }}
    print(first(boxed[0])[0])
}
"#,
    )
    .unwrap();
    let (code, stdout, stderr) = {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_jet"))
            .arg("run")
            .arg(&path)
            .output()
            .expect("jet run");
        (
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    };
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "9\n", "{stderr}");
}

/// D-MEMPROVENANCE3=A: undeclared `fn(String, String) View<str>` freezes every
/// non-scalar callback argument — mutating the unused second owner is E0212.
#[test]
fn undeclared_view_callback_freezes_every_non_scalar_argument() {
    let src = r#"
fn pick_first(line: String, noise: String) View<str> -[]> {
    text :: line.before(":")
    return text
}

fn apply(f: fn(String, String) View<str>, a: String, b: String) View<str> -[]> {
    return f(a, b)
}

fn run() {
    a := "hello:x"
    b := "world"
    result :: apply(pick_first, a, b)
    b = "mutated"
    print(result)
}
"#;
    let diags = jet::compile(src).expect_err("wide callback must freeze noise owner");
    assert!(
        diags.iter().any(|d| d.code == "E0212"),
        "expected E0212, got {diags:?}"
    );
    assert!(
        diags.iter().all(|d| d.code != "E2307"),
        "fixture itself must typecheck: {diags:?}"
    );
}

/// D-MEMPROVENANCE3=A: `fn(line: String, noise: String) View<str> from line`
/// only freezes the named source — mutating the unused argument stays legal.
#[test]
fn declared_view_callback_from_freezes_only_named_source() {
    let src = r#"
fn pick_first(line: String, noise: String) View<str> -[]> {
    text :: line.before(":")
    return text
}

fn apply(f: fn(line: String, noise: String) View<str> from line, a: String, b: String) View<str> -[]> {
    return f(a, b)
}

fn run() {
    a := "hello:x"
    b := "world"
    result :: apply(pick_first, a, b)
    b = "mutated"
    print(result)
}
"#;
    jet::compile(src).expect("narrow from-clause must leave noise free");
}
