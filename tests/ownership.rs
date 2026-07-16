//! Tests for M2 ownership / borrow transpiler rules (SAFETY DEFAULTS).

/// D-MEM1/S2: no clone is ever silent (I8) — the former lint (`L0201`) is now
/// a hard error (`E0209`), regardless of liveness.
#[test]
fn implicit_clone_is_error_not_lint() {
    let src = r#"
fn consume(s: ^String) {
    print(s)
}

fn run() {
msg: String :: "hello"
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
    c: Counter :: Counter.{ n: 0 }
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
fn inspect<T>(value: T) -> Int { return 1 }
fn duplicate<T>(value: T) -> T { return ~value }
fn increment(value: Int) -> Int { return value + 1 }

fn run() {
    callback :: increment
    print(inspect(callback))
    print(duplicate(4))
}
"#;
    let out = jet::compile(src).expect("usage-sensitive generic bounds should compile");
    assert!(
        out.rust.contains("fn user_inspect<T>"),
        "read-only generic must not require Clone: {}",
        out.rust
    );
    assert!(
        out.rust.contains("fn user_duplicate<T: Clone>"),
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
    branch :: Branch.{leaf: Leaf.{text: "read"}}
    show(branch)
}
"#;
    let out = jet::compile(src).expect("nested read should compile");
    assert!(
        !out.rust.contains("user_branch.user_leaf.user_text).clone()"),
        "nested read must stay borrowed: {}",
        out.rust
    );
}

#[test]
fn shared_callbacks_receive_exactly_one_host_borrow() {
    let src = r#"
struct Config { hits: Int }
fn run() {
    config := Shared.new(Config.{ hits: 0 })
    config.read((c) => c.hits)
    config.edit((c) => { c.hits += 1 })
}
"#;
    let out = jet::compile(src).expect("Shared callbacks must lower without double borrowing");
    assert!(out.rust.contains("|user_c: &user_Config|"), "{}", out.rust);
    assert!(
        out.rust.contains("|user_c: &mut user_Config|"),
        "{}",
        out.rust
    );
    assert!(!out.rust.contains("&&user_Config"), "{}", out.rust);
    assert!(!out.rust.contains("&mut &user_Config"), "{}", out.rust);
}

#[test]
fn mutate_required_at_call_site() {
    let src = r#"
fn touch(n: &Int) {
    print(n)
}

fn run() {
    x: Int := 1
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
msg: String :: "hi"
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

#[test]
fn const_address_taken_emits_static() {
    let src = r#"
const LIMIT = 10

fn show(n: Int) {
    print(n)
}

fn run() {
    show(LIMIT)
}
"#;
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains("static USER_LIMIT"),
        "address-taken const should emit static: {}",
        out.rust
    );
}

#[test]
fn same_call_mut_and_read_is_error() {
    let src = r#"
fn both(a: &Int, b: Int) {
    print(b)
}

fn run() {
    x: Int := 1
    both(&x, x)
}
"#;
    let diags = jet::compile(src).expect_err("should error");
    assert!(diags.iter().any(|d| d.code == "E0204"));
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
msg: String :: "hello"
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
msg: String :: "hello"
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
x: Int :: 1
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

fn maybe(b: Bool) -> Bool { return b }

fn run() {
msg: String :: "hello"
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

fn maybe(b: Bool) -> Bool { return b }

fn run() {
msg: String :: "hello"
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
    window :: xs.view(0..1)
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

/// D-MEM1 S9 / #649: any list operation that may change backing storage is
/// exclusive with a live view.
#[test]
fn resizing_list_owner_with_live_view_is_error() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    window :: xs.view(0..1)
    xs.push(4)
    print(window.len())
}
"#;
    let diags = jet::compile(src).expect_err("resizing a viewed owner must fail");
    assert!(diags.iter().any(|d| d.code == "E0212"));
}

/// D-MEM1 S9 / #649: replacing an owner invalidates every view, so assignment
/// is rejected while a view remains live.
#[test]
fn replacing_list_owner_with_live_view_is_error() {
    let src = r#"
fn run() {
    xs := [1, 2, 3]
    window :: xs.view(0..1)
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
        window :: xs.view(0..1)
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
    bucket := Bucket.{ values: [1, 2, 3] }
    window :: bucket.values.view(0..1)
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
    bucket := Bucket.{ values: [1, 2, 3] }
    window :: bucket.values.view(0..1)
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
    window :: xs.view(0..1)
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
    editor :: Editor.{ id: 0 }
    values := [1, 2, 3]
    window :: values.view(0..2)
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
    window :: values.view(0..2)
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
    editor :: Editor.{ id: 0 }
    viewed := [1, 2, 3]
    changed := [4, 5, 6]
    window :: viewed.view(0..2)
    editor.touch(&changed)
    print(window[0])
}
"#;
    jet::compile(src).expect("write to nonoverlapping owner must stay valid");
}

#[test]
fn returned_parameter_view_is_rejected_until_public_provenance_lands() {
    let src = r#"
fn first(xs: [Int]) -> View<Int> {
    return xs.view(0..1)
}

fn run() {
    print(0)
}
"#;
    let diags = jet::compile(src).expect_err("returned views remain a checked boundary");
    assert!(diags.iter().any(|d| d.code == "E2305"), "got {diags:?}");
}

#[test]
fn nested_returned_view_form_is_rejected() {
    let src = r#"
fn choose(xs: [Int]) -> View<Int> {
    if true {
        return xs.view(0..1)
    }
    return xs.view(1..2)
}

fn run() {
    print(0)
}
"#;
    let diags = jet::compile(src).expect_err("all returned-view paths must be rejected");
    assert!(diags.iter().any(|d| d.code == "E2305"), "got {diags:?}");
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
    let diags = jet::compile(src).expect_err("borrowed parameter binding must need explicit ownership");
    let escape = diags
        .iter()
        .find(|diag| diag.code == "E0120")
        .expect("borrowed binding must report E0120 instead of cloning");
    assert!(escape.fix.contains("~text"), "fix must name explicit copy: {escape:?}");
}

#[test]
fn generic_borrowed_parameter_cannot_return_as_owned() {
    let src = r#"
fn identity<T>(value: T) -> T {
    return value
}

fn run() {
    print(0)
}
"#;
    let diags = jet::compile(src).expect_err("generic read parameter cannot escape as owned");
    let escape = diags
        .iter()
        .find(|diag| diag.code == "E0120")
        .expect("generic borrowed return must report E0120 before codegen");
    assert!(escape.fix.contains("^T"), "fix must name take ownership: {escape:?}");
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

fn apply(f: fn(Int) -> Int, value: Int) -> Int {
    return f(value)
}

fn run() {
    text :: "hello"
    values :: [1, 2, 3]
    parcel :: Parcel.{ label: "parcel" }
    read_text(text)
    read_list(values)
    read_parcel(parcel)
    read_generic(text)
    print(apply((n: Int) => (n + 1), 41))
}
"#;
    let out = jet::compile(src).expect("all plain parameters are read borrows");
    assert!(out.rust.contains("user_text: &String"), "{}", out.rust);
    assert!(out.rust.contains("user_values: &Vec<i64>"), "{}", out.rust);
    assert!(out.rust.contains("user_parcel: &user_Parcel"), "{}", out.rust);
    assert!(out.rust.contains("user_value: &T"), "{}", out.rust);
    assert!(out.rust.contains("user_f: &Box<dyn Fn"), "{}", out.rust);
    assert!(!out.rust.contains("((*user_text)).clone()"), "{}", out.rust);
}

#[test]
fn function_value_calls_preserve_plain_parameter_read_borrows() {
    let src = r#"
fn inspect(value: String) -> Int { return value.len() }

fn apply(f: fn(String) -> Int, value: String) -> Int {
    return f(value)
}

fn run() {
    print(apply(inspect, "hello"))
}
"#;
    let out = jet::compile(src).expect("function-value call must preserve read access");
    assert!(out.rust.contains("user_f: &Box<dyn Fn(&String)"), "{}", out.rust);
    assert!(out.rust.contains("user_value: &String"), "{}", out.rust);
    assert!(out.rust.contains("((*user_f))(&((*user_value)))"), "{}", out.rust);
    assert!(!out.rust.contains("((*user_value)).clone()"), "{}", out.rust);
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
fn borrowed_parameter_subplaces_need_explicit_copy_in_owning_positions() {
    let cases = [
        r#"
struct Parcel { label: String }
fn alias_label(parcel: Parcel) { alias :: parcel.label }
fn run() { print(0) }
"#,
        r#"
struct Parcel { label: String }
struct Holder { value: String }
fn wrap(parcel: Parcel) -> Holder { return Holder.{ value: parcel.label } }
fn run() { print(0) }
"#,
        r#"
enum Wrapped { Val(String) }
fn wrap(values: [String]) -> Wrapped { return Wrapped.Val(values[0]) }
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
        let diags = jet::compile(src).expect_err("borrowed subplace must not copy implicitly");
        assert!(diags.iter().any(|d| d.code == "E0120"), "{diags:?}");
    }
}

#[test]
fn function_value_borrow_context_does_not_leak_between_arguments_or_calls() {
    let src = r#"
struct Parcel { label: String }
fn inspect(text: String, n: Int) { print(text); print(n) }
fn apply_to(parcel: Parcel, callback: fn(String, Int)) {
    callback(parcel.label, 1)
    alias :: parcel.label
}
fn run() { apply_to(Parcel.{ label: "hello" }, inspect) }
"#;
    let diags = jet::compile(src).expect_err("owning context after callback call must be restored");
    assert_eq!(
        diags.iter().filter(|d| d.code == "E0120").count(),
        1,
        "only the owning alias must fail; callback read stays borrowed: {diags:?}"
    );
}

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
fn keep(f: fn(Int) -> Int) -> fn(Int) -> Int { return f }
fn run() { print(0) }
"#;
    let escape = jet::compile(callback_src).expect_err("plain callback parameter cannot escape");
    assert!(escape.iter().any(|d| d.code == "E0120"), "{escape:?}");
}

#[test]
fn borrowed_parameter_cannot_fill_an_owned_struct_field_without_explicit_copy() {
    let src = r#"
struct Holder { value: String }
fn wrap(value: String) -> Holder {
    return Holder.{ value: value }
}
fn run() { print(0) }
"#;
    let diags = jet::compile(src).expect_err("borrowed field value must not clone silently");
    let escape = diags.iter().find(|d| d.code == "E0120").expect("E0120");
    assert!(escape.fix.contains("~value"), "{escape:?}");
}

#[test]
fn borrowed_parameter_can_feed_stored_lambda_after_explicit_copy() {
    let src = r#"
fn store(text: String) {
    owned :: ~text
    callback :: () => owned.len()
    print(callback())
}
fn run() { store("read") }
"#;
    jet::compile(src).expect("explicitly copied capture should compile");
}

#[test]
fn borrowed_parameter_can_feed_task_after_explicit_copy() {
    let src = r#"
use core.tasks
fn launch(text: String) {
    owned :: ~text
    task :: tasks.spawn(() => owned.len())
    print(task.join())
}
fn run() { launch("read") }
"#;
    jet::compile(src).expect("explicitly copied task capture should compile");
}
