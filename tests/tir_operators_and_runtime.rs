//! TIR operator and low-level runtime-language integration tests.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use tir_support::{
    assert_example_cli_tiers_agree, assert_tiers_agree, build_and_run, compile, compile_source,
    have_rustc, jit_run_with_env,
};

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
#Comparable
struct Tier<T: Comparable> { value: T }
#Comparable
struct NestedTier<T: Comparable> {
    head: ?T
    tail: [T]
}
struct Adder<T: Add> { value: T }

impl Vec2.Add {
    fn add(self, rhs: Vec2) Vec2 -> {
        return Vec2{ x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl Vec2.Equatable {
    fn equal(self, rhs: Vec2) Bool -> { return self.x == rhs.x && self.y == rhs.y }
}

impl Vec2.Comparable {
    fn compare(self, rhs: Vec2) Ordering -> {
        if self.x < rhs.x { return Ordering.Less }
        if self.x > rhs.x { return Ordering.Greater }
        return Ordering.Equal
    }
}

fn add_generic<T: Add>(left: T, right: T) T -> { return left + right }
fn equal_generic<T: Equatable>(left: T, right: T) Bool -> { return left == right }
fn less_generic<T: Comparable>(left: T, right: T) Bool -> { return left < right }

fn marked(x: Int) Vec2 -> {
    print("marked {x}")
    return Vec2{ x: x, y: 0 }
}

fn run() {
    a :: Vec2{ x: 1, y: 2 }
    b :: Vec2{ x: 3, y: 4 }
    c :: add_generic(a, b)
    d := Vec2{ x: 1, y: 2 }
    d = { x: 1, y: 2 }
    d += { x: 3, y: 4 }
    holder := Holder{ value: Vec2{ x: 1, y: 2 } }
    holder.value = { x: 1, y: 2 }
    holder.value += { x: 3, y: 4 }
    box := EqBox<Int>{ value: 1 }
    box = { value: 7 }
    chain :: marked(1) < marked(2) < marked(3)
    boxes_equal :: equal_generic(EqBox<Int>{ value: 7 }, EqBox<Int>{ value: 7 })
    ranks_ordered :: less_generic(Tier<Int>{ value: 1 }, Tier<Int>{ value: 2 })
    nested_ordered :: less_generic(
        NestedTier<Int>{ head: Val(1), tail: [2, 3] },
        NestedTier<Int>{ head: Val(1), tail: [2, 4] }
    )
    cell := Adder<Int>{ value: 4 }
    cell.value += 3
    print("{c.x},{c.y} {d.x},{d.y} {holder.value.x},{holder.value.y} {(!equal_generic(a, b))} {less_generic(a, b)} {(b >= a)} {chain} {boxes_equal} {ranks_ordered} {nested_ordered} {cell.value} {box.value}")
}
"#;
    let (code, stdout) = build_and_run("tir_user_operator_traits", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "marked 1\nmarked 2\nmarked 3\n4,6 4,6 4,6 true true true true true true true 7 7\n"
    );
}

/// D-CMP3WAY1=B: `<=>` is the primitive Ordering result, and a Comparable
/// hook derives all six Boolean comparison operators from that same result.
#[test]
fn spaceship_and_ordering_route_through_tir() {
    if !have_rustc() {
        return;
    }
    let src = r#"
struct Score { points: Int }

impl Score.Comparable {
    fn compare(self, rhs: Score) Ordering -> {
        if self.points < rhs.points { return Ordering.Less }
        if self.points > rhs.points { return Ordering.Greater }
        return Ordering.Equal
    }
}

fn run() {
    low :: Score{ points: 10 }
    high :: Score{ points: 20 }
    int_cmp :: 1 <=> 2
    text_cmp :: "alpha" <=> "beta"
    chained :: (low <=> high).then(high <=> high)
    then_greater :: (low <=> low).then(high <=> low)
    reverse_greater :: (high <=> low).reverse()
    reverse_equal :: (low <=> low).reverse()
    numbers := [3, 1, 2]
    numbers.sort_by((left: Int, right: Int) -> left <=> right)
    scores := [Score{ points: 30 }, Score{ points: 10 }, Score{ points: 20 }]
    scores.sort_by((left: Score, right: Score) -> left.compare(right))
    print("{(low < high)} {(low <= high)} {(high > low)} {(high >= low)} {(low == low)} {(low != high)}")
    print("{(int_cmp == Ordering.Less)} {(text_cmp == Ordering.Less)} {(chained == Ordering.Less)} {(then_greater == Ordering.Greater)} {(reverse_greater == Ordering.Less)} {(reverse_equal == Ordering.Equal)}")
    print("{numbers[0]} {numbers[1]} {numbers[2]}")
    print("{scores[0].points} {scores[1].points} {scores[2].points}")
}
"#;
    assert_tiers_agree(
        "tir_spaceship_ordering",
        src,
        "true true true true true true\ntrue true true true true true\n1 2 3\n10 20 30\n",
    );
    let rust = compile("tir_spaceship_compare_desugar", src);
    assert!(
        rust.contains("Comparable::compare"),
        "`<=>` must lower through Comparable::compare:\n{rust}"
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
fn archive(name: ^String) String -> {
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

/// c109 Phase 26: fixed-size result-list destructuring.
#[test]
fn list_destructure() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) Int -> {
    return (n * 2)
}
fn run() {
    doubled :: [double(1), double(2), double(3)]
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
fn apply_twice(f: fn(Int) Int, x: Int) Int -> {
    return f(f(x))
}
fn double(x: Int) Int -> {
    return (x * 2)
}
struct Worker {
    step: fn(Int) Int
}
struct TextWorker {
    step: fn(String) Int
}
fn text_len(text: String) Int -> {
    return text.len()
}
fn run() {
    double_fn :: double
    print(apply_twice(double_fn, 3))
    print(apply_twice((x: Int) -> (x + 1), 5))
    w :: Worker{step: (n: Int) -> (n * n)}
    print(w.step(4))
    text_worker :: TextWorker{step: text_len}
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
red :: U8{ 255 }
channel :: I32{ 100000 }
depth :: I8{ -120 }
    print(red)
    print(channel)
    print(depth)
total :: I64{ 9000000000 }
    print(total + 1)
half :: U8{ 100 }
    print(half + half)
bytes :: [U8]{ 104, 105, 33 }
    print(bytes)
wide :: I64{ Int.from_u8(red) }
    print(wide)
clamped :: U8.from_i32(channel) ?? 255
    print(clamped)
    print(U8.MAX)
    print(I32.MIN)
flags :: U8{ 13 }
    print(flags.count_ones())
    print(Float.INFINITY.is_infinite())
hi :: U8{ 200 }
lo :: U8{ 100 }
    print(wrapping(hi + lo))
    print(saturating(hi + lo))
fallback :: U8{ 0 }
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

/// D-INT-WIDEN1: sized integer locals passed to exact `Int` parameters use
/// the canonical widening path, including both 64-bit representation limits.
#[test]
fn sized_integer_locals_widen_to_int_at_i64_u64_boundaries() {
    if !have_rustc() {
        return;
    }
    let src = r#"
fn accept(value: Int) Int -> { return value }
fn pick(values: [U8], slot: I64) U8 -> { return values[slot] }

fn run() {
    i64_min :: I64.MIN
    i64_max :: I64.MAX
    u64_zero :: U64{0}
    u64_max :: U64.MAX
    values :: [U8]{7}
    print(accept(i64_min))
    print(accept(i64_max))
    print(accept(u64_zero))
    print(accept(u64_max))
    print(pick(values, I64{0}))
}
"#;
    assert_tiers_agree(
        "tir_sized_integer_argument_boundaries",
        src,
        "-9223372036854775808\n9223372036854775807\n0\n18446744073709551615\n7\n",
    );

    for (name, bad_source) in [
        (
            "tir_reject_negative_to_unsigned",
            r#"
fn accept(value: U64) U64 -> { return value }
fn run() {
    negative :: I64{-1}
    print(accept(negative))
}
"#,
        ),
        (
            "tir_reject_i64_to_i8",
            r#"
fn accept(value: I8) I8 -> { return value }
fn run() {
    wide :: I64{128}
    print(accept(wide))
}
"#,
        ),
    ] {
        let diagnostics = compile_source(name, bad_source)
            .expect_err("incompatible sized integer argument must be rejected");
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == "E0112"),
            "{name} produced unexpected diagnostics: {diagnostics:#?}"
        );
    }
}

/// D-WRAP-SCOPE1=A / I9: one lexical policy covers functions, methods, and
/// blocks. It changes only fixed-width add/subtract/multiply/power; division
/// remains the checked operation inside the wrapping block.
#[test]
fn lexical_arithmetic_policy_covers_fixed_width_operations_on_every_tier() {
    let src = r#"
#Arithmetic(.Wrapping)
fn wrapped(left: U8, right: U8) U8 -> {
    return left + right
}

#Arithmetic(.Saturating)
fn saturated(left: U8, right: U8) U8 -> {
    return left + right
}

struct Accumulator { value: U8 }
impl Accumulator {
    #Arithmetic(.Wrapping)
    fn step(self, right: U8) U8 -> {
        return self.value + right
    }
}

fn run() {
    left :: U8{200}
    right :: U8{100}
    #Arithmetic(.Wrapping) {
        print(left + right)
        print(left - U8{250})
        print(left * U8{2})
        print(U8{3} ^ U8{5})
        print(left / U8{2})
    }
    print(wrapped(left, right))
    print(saturated(left, right))
    print(Accumulator{value: left}.step(right))
}
"#;
    assert_tiers_agree(
        "tir_lexical_arithmetic_policy",
        src,
        "44\n206\n144\n243\n100\n44\n255\n44\n",
    );
}

/// D-WRAP-SCOPE1=A: a checked island overrides an enclosing wrapping policy,
/// and the runtime value prevents comptime folding from hiding the trap.
#[test]
fn nested_checked_arithmetic_policy_still_traps() {
    let src = r#"
use core.sys as env

fn run() {
    raw :: Int.parse(env.get("JET_ARITHMETIC_VALUE") ?? "0") ?? 0
    value :: U8.from_int(raw) ?? U8{0}
    #Arithmetic(.Wrapping) {
        print(value + U8{100})
        #Arithmetic(.Checked) {
            print(value + U8{100})
        }
    }
}
"#;
    let (code, stdout, stderr) = jit_run_with_env(
        "tir_nested_checked_arithmetic_policy",
        src,
        &[("JET_ARITHMETIC_VALUE", "200")],
    );
    assert_eq!(
        code, 70,
        "checked island must trap: stdout={stdout} stderr={stderr}"
    );
    assert_eq!(stdout, "44\n");
    assert!(stderr.contains("Stop [E3010]"), "{stderr}");
    assert!(stderr.contains("overflow"), "{stderr}");
}

/// D-WRAP-SCOPE1=A: wrapping is deliberately narrower than the existing
/// safety checks. A checked island still traps, and division, shifts,
/// conversions, and indexing remain checked inside a wrapping block.
#[test]
fn non_arithmetic_checks_remain_checked_inside_wrapping_policy() {
    let src = r#"
use core.sys as env

fn run() {
    case :: Int.parse(env.get("JET_ARITHMETIC_CASE") ?? "0") ?? 0
    raw :: Int.parse(env.get("JET_ARITHMETIC_VALUE") ?? "0") ?? 0
    value :: U8.from_int(raw) ?? U8{0}
    #Arithmetic(.Wrapping) {
        if case == 0 {
            #Arithmetic(.Checked) {
                print(value + U8{100})
            }
        }
        if case == 1 { print(U8{1} / value) }
        if case == 2 { print(U8{1} << value) }
        if case == 3 { print(U8{raw}) }
        if case == 4 {
            values :: [U8]{U8{10}, U8{20}}
            print(values[raw])
        }
    }
}
"#;

    for (case, value, label) in [
        ("0", "200", "nested checked arithmetic"),
        ("1", "0", "division by zero"),
        ("2", "8", "invalid shift"),
        ("3", "256", "conversion overflow"),
        ("4", "2", "out-of-bounds indexing"),
    ] {
        let (code, stdout, stderr) = jit_run_with_env(
            &format!("tir_wrapping_safety_{case}"),
            src,
            &[("JET_ARITHMETIC_CASE", case), ("JET_ARITHMETIC_VALUE", value)],
        );
        assert_eq!(code, 70, "{label} must trap: stdout={stdout} stderr={stderr}");
        assert!(stderr.contains("Stop [E3010]"), "{label}: {stderr}");
    }
}

#[test]
fn murmur3_port_matches_golden_on_default_release_and_interpreter() {
    assert_example_cli_tiers_agree(
        "ports/murmur3_x86_32",
        include_str!("../examples/features/expected/ports/murmur3_x86_32.out"),
    );
}

#[test]
fn fixed_width_rotation_boundary_counts_match_every_tier() {
    let src = r#"
fn run() {
    value :: U32{0x80000001}
    print(value.rotate_left(0))
    print(value.rotate_left(31))
    print(value.rotate_right(0))
    print(value.rotate_right(31))
}
"#;
    assert_tiers_agree(
        "tir_fixed_width_rotation_boundaries",
        src,
        "2147483649\n3221225472\n2147483649\n3\n",
    );
}
