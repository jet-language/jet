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
    window :: xs[0..1]
    print(window[1])
}
"#;
    let out = jet::compile(src).expect("bare range window must compile");
    assert!(
        out.rust.contains("let user_window = jet_view_new"),
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
    assert!(out.rust.contains("let user_all = &(user_xs)"), "{}", out.rust);
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
    edit :: &xs[0..1]
    edit[1] = 9
    print(xs[1])
}
"#;
    let out = jet::compile(src).expect("write range window must compile");
    assert!(out.rust.contains("jet_view_mut_new"), "{}", out.rust);
}

#[test]
fn write_windows_edit_whole_field_and_index_places() {
    let src = r#"
struct Cell { value: Int }
fn run() {
    whole := 1
    whole_edit :: &whole
    whole_edit = 2

    cell := Cell.{ value: 3 }
    field_edit :: &cell.value
    field_edit = 4

    xs := [5, 6]
    index_edit :: &xs[0]
    index_edit = 7
    print(whole + cell.value + xs[0])
}
"#;
    let out = jet::compile(src).expect("write windows must write through to each owner place");
    assert!(out.rust.contains("(*user_whole_edit) = 2i64"), "{}", out.rust);
    assert!(out.rust.contains("(*user_field_edit) = 4i64"), "{}", out.rust);
    assert!(out.rust.contains("(*user_index_edit) = 7i64"), "{}", out.rust);
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
fn make() -> [Int] { return [1, 2] }
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
    assert!(out.rust.matches("split_at_mut").count() >= 2, "{}", out.rust);
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
    assert_eq!(out.rust.matches(".split_at_mut(").count(), 4, "{}", out.rust);
    assert!(
        out.rust.contains("let user_left = &mut __jet_place_plan_")
            && out.rust.contains("let user_right = &mut __jet_place_plan_"),
        "{}",
        out.rust
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
            .contains("let __jet_place_plan_0_root = &mut (user_first_owner)"),
        "{}",
        out.rust
    );
    assert!(
        out.rust
            .contains("let __jet_place_plan_1_root = &mut (user_second_owner)"),
        "{}",
        out.rust
    );
}

#[test]
fn disjoint_struct_fields_use_native_structural_borrows() {
    let src = r#"
struct Pair { left: Int, right: Int }
fn run() {
    pair := Pair.{ left: 1, right: 2 }
    left :: &pair.left
    right :: &pair.right
    left = 3
    right = 4
    print(pair.left + pair.right)
}
"#;
    let out = jet::compile(src).expect("different fields are structurally disjoint");
    assert!(out.rust.contains("&mut ((user_pair).user_left)"), "{}", out.rust);
    assert!(out.rust.contains("&mut ((user_pair).user_right)"), "{}", out.rust);
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
fn inspect<T>(xs: [T]) -> Int {
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
        .find("let user_window = jet_view_new")
        .expect("view helper call");
    let helper = out.rust.find("fn jet_view_new").expect("view helper definition");
    assert!(helper < check, "bounds-checking helper must exist before use");
    assert!(out
        .rust
        .contains("a < 0 || b < 0 || a > b || b >= len"));
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
fn edit_first(xs: &[Int]) -> ViewMut<Int> {
    return &xs[0..1]
}
fn run() { print(0) }
"#;
    let out = jet::compile(src).expect("parameter-rooted write view return must compile");
    assert!(
        out.rust.contains(
            "fn user_edit_first<'__jet_view>(user_xs: &'__jet_view mut Vec<i64>) -> &'__jet_view mut [i64]"
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
    holder :: Holder.{ window: edit }
    print(holder.window.len())
}
"#;
    let diags = jet::compile(src).expect_err("write view storage must be rejected");
    assert!(diags.iter().any(|d| d.code == "E2305"), "{diags:?}");
}

#[test]
fn write_window_cannot_cross_task_boundary() {
    let src = r#"
use core.tasks
fn run() {
    xs := [1, 2, 3]
    edit :: &xs[0..1]
    task :: tasks.spawn(() => edit.len())
    print(task.join())
}
"#;
    let diags = jet::compile(src).expect_err("write view task capture must be rejected");
    assert!(diags.iter().any(|d| d.code == "E1102"), "{diags:?}");
}

#[test]
fn write_window_cannot_cross_channel_boundary() {
    let src = r#"
use core.tasks
fn run() {
    xs := [1, 2, 3]
    edit :: &xs[0..1]
    (sender, channel) :: tasks.channel<ViewMut<Int>>()
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
    bucket := Bucket.{ values: [1, 2, 3] }
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
    bucket := Bucket.{ values: [1, 2, 3] }
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
    editor :: Editor.{ id: 0 }
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
    editor :: Editor.{ id: 0 }
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
fn first(xs: [Int], other: [Int]) -> View<Int> {
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
fn first(left: [Int], right: [Int]) -> View<Int> {
    return left[0..1]
}

fn wrapper(left: [Int], right: [Int]) -> View<Int> {
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

#[test]
fn returned_string_view_uses_parameter_provenance() {
    let src = r#"
fn domain(email: String) -> View<str> {
    result :: email.after("@")
    return result
}
fn run() { print(domain("user@example.com")) }
"#;
    let out = jet::compile(src).expect("parameter-rooted string view return must compile");
    assert!(
        out.rust.contains(
            "fn user_domain<'__jet_view>(user_email: &'__jet_view String) -> &'__jet_view str"
        ),
        "generated lifetime must tie the string view to parameter 0: {}",
        out.rust
    );
}

#[test]
fn returned_string_view_cannot_outlive_local_owner() {
    let src = r#"
fn bad() -> View<str> {
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

fn domain(email: String) -> Domain {
    result :: email.after("@")
    return Domain.{ value: result }
}
fn run() { print(domain("user@example.com").value) }
"#;
    let out = jet::compile(src).expect("parameter-rooted string view field must compile");
    assert!(out.rust.contains("pub struct user_Domain<'__jet_view>"), "{}", out.rust);
    assert!(out.rust.contains("pub user_value: &'__jet_view str"), "{}", out.rust);
}

#[test]
fn returned_string_view_field_cannot_outlive_local_owner() {
    let src = r#"
struct Domain { value: View<str> }

fn bad() -> Domain {
    email := "user@example.com"
    result :: email.after("@")
    return Domain.{ value: result }
}
fn run() { print(bad().value) }
"#;
    let diags = jet::compile(src).expect_err("locally-owned string view field must fail");
    assert!(diags.iter().any(|d| d.code == "E2307"), "{diags:?}");
}

#[test]
fn returned_string_view_rejects_temporary_call_owner_before_codegen() {
    let src = r#"
fn domain(email: String) -> View<str> {
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
fn wrapper(left: [Int], right: [Int]) -> View<Int> {
    return first(left, right)
}

fn first(left: [Int], right: [Int]) -> View<Int> {
    return left[0..1]
}

fn run() { print(0) }
"#;
    jet::compile(src).expect("forward callable provenance must stabilize before validation");
}

#[test]
fn mutually_recursive_view_summaries_stabilize() {
    let src = r#"
fn first(values: [Int], recurse: Bool) -> View<Int> {
    if recurse {
        return second(values, false)
    }
    return values[0..1]
}

fn second(values: [Int], recurse: Bool) -> View<Int> {
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
    fn first(self, left: [Int], right: [Int]) -> View<Int> {
        return left[0..1]
    }
}

fn wrapper(selector: Selector, left: [Int], right: [Int]) -> View<Int> {
    return selector.first(left, right)
}

fn run() {
    selector :: Selector.{ marker: 0 }
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
    fn select(self, left: [Int], right: [Int]) -> View<Int>
}

struct First { marker: Int }

fn wrapper(selector: First, left: [Int], right: [Int]) -> View<Int> {
    return selector.select(left, right)
}

impl First.Select {
    fn select(self, left: [Int], right: [Int]) -> View<Int> {
        return left[0..1]
    }
}

fn run() { print(0) }
"#;
    jet::compile(src).expect("trait method provenance must stabilize before wrapper validation");
}

#[test]
fn trait_view_contract_rejects_disagreeing_implementations() {
    let src = r#"
trait Select {
    fn select(self, left: [Int], right: [Int]) -> View<Int>
}

struct First {}
impl First.Select {
    fn select(self, left: [Int], right: [Int]) -> View<Int> {
        return left[0..1]
    }
}

struct Last {}
impl Last.Select {
    fn select(self, left: [Int], right: [Int]) -> View<Int> {
        return right[0..1]
    }
}

fn run() { print(0) }
"#;
    let diags = jet::compile(src).expect_err("one trait method needs one stable view source");
    assert!(diags.iter().any(|d| d.code == "E2305"), "{diags:?}");
}

#[test]
fn aggregate_trait_view_contract_stabilizes_through_wrapper_in_either_impl_order() {
    let template = r#"
struct Pair { left: View<Int>, right: View<Int> }

trait Select {
    fn select(self, left: [Int], right: [Int]) -> Pair
}

fn wrapper(selector: Select, left: [Int], right: [Int]) -> Pair {
    return selector.select(left, right)
}

$IMPLS

fn run() { print(0) }
"#;
    let first = r#"
struct First {}
impl First.Select {
    fn select(self, left: [Int], right: [Int]) -> Pair {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Pair.{ left: left_view, right: right_view }
    }
}
"#;
    let last = r#"
struct Last {}
impl Last.Select {
    fn select(self, left: [Int], right: [Int]) -> Pair {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Pair.{ left: left_view, right: right_view }
    }
}
"#;
    for implementations in [format!("{first}{last}"), format!("{last}{first}")] {
        let src = template.replace("$IMPLS", &implementations);
        jet::compile(&src).expect("aggregate trait contract must stabilize before wrapper");
    }
}

#[test]
fn aggregate_trait_view_contract_rejects_disagreement_in_either_impl_order() {
    let template = r#"
struct Pair { left: View<Int>, right: View<Int> }

trait Select {
    fn select(self, left: [Int], right: [Int]) -> Pair
}

$IMPLS

fn run() { print(0) }
"#;
    let first = r#"
struct First {}
impl First.Select {
    fn select(self, left: [Int], right: [Int]) -> Pair {
        left_view :: left[0..1]
        right_view :: right[0..1]
        return Pair.{ left: left_view, right: right_view }
    }
}
"#;
    let last = r#"
struct Last {}
impl Last.Select {
    fn select(self, left: [Int], right: [Int]) -> Pair {
        left_view :: left[0..1]
        right_view :: left[0..1]
        return Pair.{ left: left_view, right: right_view }
    }
}
"#;
    for implementations in [format!("{first}{last}"), format!("{last}{first}")] {
        let src = template.replace("$IMPLS", &implementations);
        let diags = jet::compile(&src)
            .expect_err("aggregate trait implementations must agree per output slot");
        assert!(diags.iter().any(|d| d.code == "E2305"), "{diags:?}");
    }
}

#[test]
fn returned_view_provenance_transfers_on_binding_move() {
    let src = r#"
fn first(values: [Int]) -> View<Int> {
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

fn window(values: [Int]) -> Window {
    selected :: values[0..1]
    return Window.{ values: selected }
}

fn run() {
    values := [7, 8]
    result :: window(values)
    print(result.values[0])
}
"#;
    let out = jet::compile(src).expect("returned aggregate must keep its view tied to parameter 0");
    assert!(
        out.rust.contains("pub struct user_Window<'__jet_view>")
            && out.rust.contains("pub user_values: &'__jet_view [i64]")
            && out.rust.contains("-> user_Window<'__jet_view>"),
        "aggregate and return must share the hidden owner lifetime: {}",
        out.rust
    );
}

#[test]
fn nested_returned_aggregate_stabilizes_each_view_output_slot() {
    let src = r#"
struct Inner { values: View<Int> }
struct Outer { inner: Inner }

fn outer(values: [Int]) -> Outer {
    selected :: values[0..1]
    return Outer.{ inner: Inner.{ values: selected } }
}

fn run() {
    values := [7, 8]
    result :: outer(values)
    print(result.inner.values[0])
}
"#;
    let out = jet::compile(src).expect("nested returned aggregate must carry transitive view provenance");
    assert!(out.rust.contains("pub struct user_Inner<'__jet_view>"), "{}", out.rust);
    assert!(out.rust.contains("pub struct user_Outer<'__jet_view>"), "{}", out.rust);
    assert!(out.rust.contains("pub user_inner: user_Inner<'__jet_view>"), "{}", out.rust);
    assert!(out.rust.contains("-> user_Outer<'__jet_view>"), "{}", out.rust);
}

#[test]
fn wrapper_returned_view_aggregates_render_lifetimes_on_named_leaves() {
    let src = r#"
struct Window { values: View<Int> }
struct Holder { maybe: Window? }
struct GenericHolder<T> { value: T, maybe: Window? }

fn maybe(values: [Int]) -> (Window?) {
    selected :: values[0..1]
    return Val(Window.{ values: selected })
}

fn result(values: [Int]) -> Window ? String {
    selected :: values[0..1]
    return ok(Window.{ values: selected })
}

fn tuple(values: [Int]) -> (window: Window, count: Int) {
    selected :: values[0..1]
    return (window: Window.{ values: selected }, count: 1)
}

fn run() { print(0) }
"#;
    let out = jet::compile(src).expect("wrapper returns must preserve view provenance");
    assert!(out.rust.contains("Option<user_Window<'__jet_view>>"), "{}", out.rust);
    assert!(out.rust.contains("Result<user_Window<'__jet_view>, String>"), "{}", out.rust);
    assert!(out.rust.contains("pub user_window: user_Window<'__jet_view>"), "{}", out.rust);
    assert!(out.rust.contains("pub struct user_GenericHolder<'__jet_view, T"), "{}", out.rust);
    assert!(out.rust.contains("pub user_maybe: Option<user_Window<'__jet_view>>"), "{}", out.rust);
    assert!(!out.rust.contains("Option<'__jet_view"), "{}", out.rust);
    assert!(!out.rust.contains("Result<'__jet_view"), "{}", out.rust);
}

#[test]
fn recursive_view_aggregate_graph_terminates_without_ice() {
    let src = r#"
struct Node { next: Node?, values: View<Int> }

fn node(values: [Int]) -> Node {
    selected :: values[0..1]
    return Node.{ next: None, values: selected }
}

fn run() { print(0) }
"#;
    let out = jet::compile(src).expect("recursive view graph must terminate in sema and codegen");
    assert!(out.rust.contains("pub struct user_Node<'__jet_view>"), "{}", out.rust);
    assert!(out.rust.contains("user_Node<'__jet_view>"), "{}", out.rust);
}

#[test]
fn returned_aggregate_accepts_distinct_sources_per_output_slot() {
    let src = r#"
struct Pair { left: View<Int>, right: View<Int> }

fn pair(left: [Int], right: [Int]) -> Pair {
    left_view :: left[0..1]
    right_view :: right[0..1]
    return Pair.{ left: left_view, right: right_view }
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

fn pair(left: [Int], right: [Int]) -> Pair {
    left_view :: left[0..1]
    right_view :: right[0..1]
    return Pair.{ left: left_view, right: right_view }
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
        let diags = jet::compile(&src)
            .expect_err("each returned output slot must keep its own owner live");
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

fn pair(left: [Int], right: [Int]) -> Pair {
    left_view :: left[0..1]
    right_view :: right[0..1]
    return Pair.{ left: left_view, right: right_view }
}

fn first(pair: Pair) -> View<Int> {
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
    let diags = jet::compile(src)
        .expect_err("the projected left slot must retain its original owner");
    assert!(diags.iter().any(|diag| diag.code == "E0212"), "{diags:?}");
}

#[test]
fn returned_view_aggregate_cannot_cross_task_boundary() {
    let src = r#"
use core.tasks
struct Window { values: View<Int> }

fn window(values: [Int]) -> Window {
    selected :: values[0..1]
    return Window.{ values: selected }
}

fn run() {
    task :: tasks.spawn(() => window([7, 8]))
    print(task.join().values[0])
}
"#;
    let diags = jet::compile(src).expect_err("view aggregates are not task-sendable");
    assert!(diags.iter().any(|d| d.code == "E1102"), "{diags:?}");
}

#[test]
fn nested_returned_view_paths_with_same_source_compile() {
    let src = r#"
fn choose(xs: [Int]) -> View<Int> {
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
fn returned_view_paths_with_different_sources_are_rejected() {
    let src = r#"
fn choose(left: [Int], right: [Int], first: Bool) -> View<Int> {
    if first {
        return left[0..1]
    }
    return right[0..1]
}

fn run() { print(0) }
"#;
    let diags = jet::compile(src).expect_err("returned-view paths need one stable owner source");
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
fn function_value_returning_view_is_rejected_before_codegen() {
    let src = r#"
fn first(values: [Int]) -> View<Int> {
    return values[0..1]
}

fn run() {
    callback :: first
    values := [7, 8]
    result :: callback(values)
    print(result[0])
}
"#;
    let diags = jet::compile(src).expect_err("function-value provenance is erased");
    assert!(diags.iter().any(|d| d.code == "E2305"), "{diags:?}");
}

#[test]
fn stored_lambda_returning_view_is_rejected_before_codegen() {
    let src = r#"
fn first(values: [Int]) -> View<Int> {
    return values[0..1]
}

fn run() {
    values := [7, 8]
    callback :: () => first(values)
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
    fn first(self) -> View<Int> {
        return self.values[0..1]
    }
}
fn wrapper(bucket: Bucket) -> View<Int> {
    return bucket.first()
}
fn run() { print(0) }
"#;
    let out = jet::compile(src).expect("receiver-rooted method view must compose");
    assert!(out.rust.contains("&'__jet_view self"), "{}", out.rust);
}

#[test]
fn generic_returned_view_composes_through_wrapper() {
    let src = r#"
fn first<T>(values: [T]) -> View<T> {
    return values[0..1]
}
fn wrapper(values: [Int]) -> View<Int> {
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
    fn select(self, left: [Int], right: [Int]) -> View<Int>
}
fn wrapper(selector: Select, left: [Int], right: [Int]) -> View<Int> {
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
            r#"fn first(values: [Int]) -> View<Int> {{
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
fn window(values: [Int]) -> Window {
    selected :: values[0..1]
    return Window.{ values: selected }
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
fn edit(values: &[Int]) -> ViewMut<Int> {
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
fn returned_view_cannot_escape_into_untracked_list() {
    let src = r#"
fn first(values: [Int]) -> View<Int> {
    return values[0..1]
}
fn run() {
    values := [1, 2, 3]
    result :: first(values)
    windows :: [result]
    print(windows[0][0])
}
"#;
    let diags = jet::compile(src).expect_err("list container has no owner provenance slot");
    assert!(diags.iter().any(|d| d.code == "E2305"), "{diags:?}");
}

#[test]
fn stored_view_field_cannot_be_rebound_to_different_owner() {
    let src = r#"
struct Window { values: View<Int> }
fn replace(left: [Int], right: [Int]) {
    first :: left[0..1]
    holder := Window.{ values: first }
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
fn borrowed_parameter_subplaces_need_explicit_copy_in_owning_positions() {
    let cases = [
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
fn function_value_borrow_context_preserves_parameter_place_window() {
    let src = r#"
struct Parcel { label: String }
fn inspect(text: String, n: Int) { print(text); print(n) }
fn apply_to(parcel: Parcel, callback: fn(String, Int)) {
    callback(parcel.label, 1)
    alias :: parcel.label
}
fn run() { apply_to(Parcel.{ label: "hello" }, inspect) }
"#;
    let out = jet::compile(src).expect("bare parameter subplace is a read window");
    assert!(out.rust.contains("let user_alias = &"), "{}", out.rust);
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
