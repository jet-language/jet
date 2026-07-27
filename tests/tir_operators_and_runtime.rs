//! TIR operator and low-level runtime-language integration tests.

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{build_and_run, have_rustc};

/// D-OPDEF1=A: user arithmetic/equality/order reuse ordinary trait methods.
#[test]
fn user_operator_traits_route_through_tir() {
    if !have_rustc() {
        return;
    }
    let src = r#"
struct Vec2 { x: Int y: Int }
struct Holder { value: Vec2 }
struct EqBox<T: Equatable> { value: T }
struct Rank<T: Comparable> { value: T derive Comparable }
struct NestedRank<T> {
    head: T?
    tail: [T]
    derive Comparable
}
struct Cell<T: Add> { value: T }

impl Vec2.Add {
    fn add(self, rhs: Vec2) => Vec2 {
        return Vec2.{ x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl Vec2.Equatable {
    fn equal(self, rhs: Vec2) => Bool { return self.x == rhs.x && self.y == rhs.y }
}

impl Vec2.Comparable {
    fn compare(self, rhs: Vec2) => Ordering {
        if self.x < rhs.x { return Ordering.Less }
        if self.x > rhs.x { return Ordering.Greater }
        return Ordering.Equal
    }
}

fn add_generic<T: Add>(left: T, right: T) => T { return left + right }
fn equal_generic<T: Equatable>(left: T, right: T) => Bool { return left == right }
fn less_generic<T: Comparable>(left: T, right: T) => Bool { return left < right }

fn marked(x: Int) => Vec2 {
    print("marked {x}")
    return Vec2.{ x: x, y: 0 }
}

fn run() {
    a :: Vec2.{ x: 1, y: 2 }
    b :: Vec2.{ x: 3, y: 4 }
    c :: add_generic(a, b)
    d := Vec2.{ x: 1, y: 2 }
    d += b
    holder := Holder.{ value: Vec2.{ x: 1, y: 2 } }
    holder.value += b
    chain :: marked(1) < marked(2) < marked(3)
    boxes_equal :: equal_generic(EqBox<Int>.{ value: 7 }, EqBox<Int>.{ value: 7 })
    ranks_ordered :: less_generic(Rank<Int>.{ value: 1 }, Rank<Int>.{ value: 2 })
    nested_ordered :: less_generic(
        NestedRank<Int>.{ head: Val(1), tail: [2, 3] },
        NestedRank<Int>.{ head: Val(1), tail: [2, 4] }
    )
    cell := Cell<Int>.{ value: 4 }
    cell.value += 3
    print("{c.x},{c.y} {d.x},{d.y} {holder.value.x},{holder.value.y} {(!equal_generic(a, b))} {less_generic(a, b)} {(b >= a)} {chain} {boxes_equal} {ranks_ordered} {nested_ordered} {cell.value}")
}
"#;
    let (code, stdout) = build_and_run("tir_user_operator_traits", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "marked 1\nmarked 2\nmarked 3\n4,6 4,6 4,6 true true true true true true true 7\n"
    );
}

/// c109 Phase 26: free-call mutate, move, and shared-read argument conventions.
#[test]
fn free_call_arg_conventions() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn show(msg: String) {
    print(msg)
}
fn bump(n: &Int) {
    n += 1
}
fn archive(name: ^String) => String {
    return name
}
fn run() {
    score := 41
    bump(&score)
    print(score)
greeting :: \"hello\"
    show(greeting)
saved :: archive(^\"vault\")
    print(saved)
}
";
    let (code, stdout) = build_and_run("tir_arg_conv", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\nhello\nvault\n");
}

/// c109 Phase 26: fan-out result-list destructuring.
#[test]
fn list_destructure() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) => Int {
    return (n * 2)
}
fn run() {
    doubled :: double.[1, 2, 3]
    [a, b, c] :: doubled
    print(a)
    print(b)
    print(c)
}
";
    let (code, stdout) = build_and_run("tir_list_destructure", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n4\n6\n");
}

/// c109 Phase 27: stored function values and struct function fields.
#[test]
fn fn_value_and_struct_fn_field() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn apply_twice(f: fn(Int) => Int, x: Int) => Int {
    return f(f(x))
}
fn double(x: Int) => Int {
    return (x * 2)
}
struct Worker {
    step: fn(Int) => Int
}
struct TextWorker {
    step: fn(String) => Int
}
fn text_len(text: String) => Int {
    return text.len()
}
fn run() {
    double_fn :: double
    print(apply_twice(double_fn, 3))
    print(apply_twice((x: Int) => (x + 1), 5))
    w :: Worker.{step: (n: Int) => (n * n)}
    print(w.step(4))
    text_worker :: TextWorker.{step: text_len}
    text :: \"read\"
    print(text_worker.step(text))
    print(text)
}
";
    let (code, stdout) = build_and_run("tir_fn_value_struct_field", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "12\n7\n16\n4\nread\n");
}

/// c109 Phase 28: sized integers, conversions, bounds, queries, and overflow modes.
#[test]
fn sized_integers() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
red :: U8.{ 255 }
channel :: I32.{ 100000 }
depth :: I8.{ -120 }
    print(red)
    print(channel)
    print(depth)
total :: I64.{ 9000000000 }
    print(total + 1)
half :: U8.{ 100 }
    print(half + half)
bytes :: [U8].{ 104, 105, 33 }
    print(bytes)
wide :: I64.{ Int.from_u8(red) }
    print(wide)
clamped :: U8.from_i32(channel) ?? 255
    print(clamped)
    print(U8.MAX)
    print(I32.MIN)
flags :: U8.{ 13 }
    print(flags.count_ones())
    print(Float.INFINITY.is_infinite())
hi :: U8.{ 200 }
lo :: U8.{ 100 }
    print(wrapping(hi + lo))
    print(saturating(hi + lo))
fallback :: U8.{ 0 }
    print(checked(hi + lo) ?? fallback)
}
";
    let (code, stdout) = build_and_run("tir_sized_integers", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "255\n100000\n-120\n9000000001\n200\n[104, 105, 33]\n255\n255\n255\n-2147483648\n3\ntrue\n44\n255\n0\n"
    );
}
