//! I9: `print` of every primitive and ordinary branching keep one observable
//! meaning across the resident Cranelift JIT (default `jet run`), the
//! interpreter, and AOT.  Assertions are exact stdout bytes: an exit-status
//! check let the JIT print unrelated heap memory for `print(1)` (an `Int`
//! carrier reinterpreted as a `String` handle) and let the AOT tier reject
//! the same program at rustc.

mod common;

use std::fs;
use std::process::Command;

const PACKAGE_SOURCE: &str = r#"
name: "print_branch_tiers"
version: "0.1.0"
authority: { holds: { allow: [IO] } }
"#;

/// Every primitive reaches the canonical print operation and renders the same
/// bytes on every tier.  The bound locals cover the resident JIT reading a
/// `let` back through a different place row than the one it wrote (one stack
/// slot per local, not per place row); the interpolated line covers the shared
/// Display route and the allocation layer AOT needs to build it.
const PRINT_PRIMITIVES: &str = r#"
fn run() {
    print(1)
    print(-7)
    print(1 + 2)
    print(true)
    print(false)
    print(1.5)
    print(2.0)
    print("s")
    print('c')
    print(1, "two", 3)
    x :: 3
    print(x)
    s :: "text"
    print(s)
    print("v={x + 1} b={x > 2} s={s}")
}
"#;

const PRINT_PRIMITIVES_STDOUT: &str =
    "1\n-7\n3\ntrue\nfalse\n1.5\n2.0\ns\nc\n1\ntwo\n3\n3\ntext\nv=4 b=true s=text\n";

/// A concrete automatic Printable, an explicit Display override, and one
/// variadic call whose arguments print while they are evaluated.  The helper
/// and operator both return `D` so the tier must preserve the checked
/// Printable value until the backend renders it; the variadic output proves
/// both left-to-right argument evaluation and delayed emission.
const PRINTABLE_DISPLAY_VARIADIC: &str = r#"
struct D {
    value: Int
}

struct Pair {
    a: Int
    b: Int
}

fn align(left: D, right: D) Pair -> Pair{a: left.value, b: right.value}

impl D.Add {
    fn add(self, rhs: D) D -> {
        pair :: align(self, rhs)
        return D{value: pair.a + pair.b}
    }
}

enum Envelope {
    Pair(left: D, right: D)
}

struct Shown {
    value: Int
}

impl Shown.Display {
    fn display(self) String -> "shown={self.value}"
}

fn mark(label: String, value: Int) D -[IO]> {
    print(label)
    return D{value: value}
}

fn run() {
    print(D{value: 3})
    print(D{value: 1} + D{value: 2})
    print(Envelope.Pair{left: D{value: 4}, right: D{value: 5}})
    print(Shown{value: 9})
    print(mark("left", 1), mark("right", 2))
}
"#;

const PRINTABLE_DISPLAY_VARIADIC_STDOUT: &str =
    "D { value: 3 }\nD { value: 3 }\nPair { left: D { value: 4 }, right: D { value: 5 } }\nshown=9\nleft\nright\nD { value: 1 }\nD { value: 2 }\n";

/// Plain `if`, `if`/`else`, nested `if`, an `if` whose arms both produce a
/// value, a branch inside a loop, and an early `return` inside a branch.
const BRANCH_SHAPES: &str = r#"
fn plain(flag: Bool) {
    if flag { print("plain") }
    print("after plain")
}

fn either(flag: Bool) {
    if flag { print("then") } else { print("else") }
}

fn nested(outer: Bool, inner: Bool) {
    if outer {
        if inner { print("outer inner") } else { print("outer only") }
    } else {
        print("neither")
    }
}

fn pick(flag: Bool) Int -> {
    if flag -> 10 else -> 20
}

fn count_odd(limit: Int) Int -> {
    odd := 0
    loop i in 0..<limit {
        if i % 2 == 1 { odd += 1 }
    }
    odd
}

fn early(n: Int) String -> {
    if n == 0 { return "zero" }
    "nonzero"
}

fn run() {
    plain(true)
    plain(false)
    either(true)
    either(false)
    nested(true, true)
    nested(true, false)
    nested(false, true)
    print(pick(true))
    print(pick(false))
    print(count_odd(7))
    print(early(0))
    print(early(3))
}
"#;

const BRANCH_SHAPES_STDOUT: &str = "plain\nafter plain\nafter plain\nthen\nelse\nouter inner\nouter only\nneither\n10\n20\n3\nzero\nnonzero\n";

/// The one program the tier smoke never had: locals.  Immutable `::` and
/// mutable `:=` bindings of every printable primitive, read back after their
/// initializing write, a compound assignment, and a local read inside a
/// later assignment.  The whole language failed to compile a binding once
/// while every binding-free proof above stayed green.  The float is spelled
/// `Float{…}` because an untyped `1.5` binding is a `Decimal` by default.
const LOCAL_BINDINGS: &str = r#"
fn run() {
    n :: 3
    print(n)
    f :: Float{1.5}
    print(f)
    s :: "text"
    print(s)
    flag :: true
    print(flag)
    m := 10
    m += 5
    m = m + n
    print(m)
    print(n + m)
}
"#;

const LOCAL_BINDINGS_STDOUT: &str = "3\n1.5\ntext\ntrue\n18\n21\n";

/// `if` as a value: bound to a local, consumed directly by a call, an
/// `else if` chain, nesting, and arms producing different constructors of one
/// type.  Every arm value joins through a MIR `Phi`; the legality verifier
/// once demanded each arm value dominate the join block itself, which no real
/// join can satisfy, so every value `if` was an ICE on every tier.
const VALUE_IF: &str = r#"
fn show(label: String) { print(label) }

fn band(n: Int) Int -> {
    if n < 0 -> 1 else if n == 0 -> 2 else -> 3
}

fn run() {
    x :: if true -> 1 else -> 2
    print(x)
    print(if false -> "a" else -> "b")
    print(band(-4))
    print(band(0))
    print(band(9))
    show("param ok")
}
"#;

const VALUE_IF_STDOUT: &str = "1\nb\n1\n2\n3\nparam ok\n";

#[test]
fn print_primitives_render_the_same_bytes_on_every_tier() {
    assert_cli_tiers_agree("print_primitives", PRINT_PRIMITIVES, PRINT_PRIMITIVES_STDOUT);
}

#[test]
fn printable_display_and_variadic_evaluation_agree_on_every_tier() {
    assert_cli_tiers_agree(
        "printable_display_variadic",
        PRINTABLE_DISPLAY_VARIADIC,
        PRINTABLE_DISPLAY_VARIADIC_STDOUT,
    );
}

#[test]
fn ordinary_branch_shapes_execute_on_every_tier() {
    assert_cli_tiers_agree("branch_shapes", BRANCH_SHAPES, BRANCH_SHAPES_STDOUT);
}

#[test]
fn local_bindings_execute_on_every_tier() {
    assert_cli_tiers_agree("local_bindings", LOCAL_BINDINGS, LOCAL_BINDINGS_STDOUT);
}

#[test]
fn value_if_joins_execute_on_every_tier() {
    assert_cli_tiers_agree("value_if", VALUE_IF, VALUE_IF_STDOUT);
}

fn assert_cli_tiers_agree(name: &str, src: &str, expected_stdout: &str) {
    let root = common::unique_tmp(&format!("jet_print_branch_tiers_{name}"));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.jet"), PACKAGE_SOURCE).unwrap();
    fs::write(root.join("run.jet"), src).unwrap();

    let release = common::have_rustc();
    if !release {
        eprintln!("note: skipping the AOT tier of {name} (need rustc)");
    }
    let modes = [("default", &[][..]), ("interpret", &["--interpret"][..]), ("release", &["--release"][..])];
    for (mode, flags) in modes {
        if mode == "release" && !release {
            continue;
        }
        let cache = root.join(format!("cache-{mode}"));
        let output = Command::new(env!("CARGO_BIN_EXE_jet"))
            .arg("run")
            .args(flags)
            .arg("run.jet")
            .current_dir(&root)
            .env("JET_STORE_DIR", &cache)
            .env("JETPACK_ENV", "1")
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}: {mode} run failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert_eq!(stdout, expected_stdout, "{name}: {mode} stdout");
    }
    let _ = fs::remove_dir_all(root);
}
