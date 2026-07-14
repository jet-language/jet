//! TIR io and ownership integration tests.

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::process::Command;

use tir_support::{build_and_run, build_and_run_multi, have_rustc};

/// Build `src` to a binary, then run it with `stdin` piped in. Like `build_and_run`
/// but feeds a deterministic stdin so an `io.input(...)` reads known lines (and EOF).
fn build_and_run_stdin(name: &str, src: &str, stdin: &str) -> (i32, String) {
    use std::io::Write;
    use std::process::Stdio;
    let dir = std::env::temp_dir().join(format!("jet_tir_test_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args([
            "--edition",
            "2021",
            rs.to_str().unwrap(),
            "-o",
            bin.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let mut child = Command::new(&bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let run = child.wait_with_output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
    )
}

/// c109 Phase 29: qualified `io.input(prompt)` (surface (H), 34_parallel_scan
/// `paths_from_prompt`). DISTINCT from the ambient bare `input()` (Phase 25 `AmbientInput`):
/// this is a `MethodCall` on a `core.io` alias, lowered to a `CoreCall`
/// (`jet_std_io_input(Some(&(prompt)))` → `Result<String, IOError>`). It composes with a
/// `?? return <value>` fallback (the early-return form, already covered since Phase 8).
/// The loop accumulates piped lines; a blank line breaks; EOF yields `Ok("")` (read_line on
/// EOF) so the loop also breaks — the `?? return` fires only on a genuine Err. Both stdin
/// shapes run deterministically.
#[test]
fn qualified_io_input_or_return() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.io as io
fn collect() -> [String] {
    out: [String] := []
    loop true {
        line :: io.input(\"> \") ?? return copy out
        if line == \"\" {
            break
        }
        out.push(line)
    }
    return out
}
fn run() {
    got :: collect()
    print(\"count={got.len()}\")
    loop g in got {
        print(g)
    }
    print(\"done\")
}
";
    // Two lines then a blank line: the loop accumulates `alpha`/`beta`, the blank breaks.
    let (code, stdout) = build_and_run_stdin("tir_io_input_lines", src, "alpha\nbeta\n\n");
    assert_eq!(code, 0);
    assert_eq!(stdout, "> > > count=2\nalpha\nbeta\ndone\n");
    // Immediate EOF: read_line yields Ok("") → the loop breaks on the empty line, no input.
    let (code, stdout) = build_and_run_stdin("tir_io_input_eof", src, "");
    assert_eq!(code, 0);
    assert_eq!(stdout, "> count=0\ndone\n");
}

/// c109 Phase 30: GENERIC functions + TRAIT-OBJECT dispatch (surface (G), 25_traits).
/// Three covered fns: a generic `largest<T: Comparable>(xs: [T]) -> (T?)` (a `>` on a
/// `Comparable`-bound type var, `[T]` indexing, a `T?` return with `value`/`None`); a
/// trait-OBJECT param `print_area(s: Shape)` (dynamic dispatch `s.name()`/`s.area()`
/// through a `Box<dyn user_Shape>`); and `main` — a `[Shape]` trait-object list built from
/// `Box::new(<lit>) as Box<dyn user_Shape>` element coercions, iterated via `.each`
/// (`jet_list_each_ref`), plus a generic call `largest(nums)` and a derived-Comparable
/// `scores.sort_by(...)`. All route `ROUTE TIR` (the Circle/Square trait methods already
/// route since Phase 12), and the whole suite is byte-identical (golden parity).
#[test]
fn generic_fns_and_trait_object_dispatch() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Shape {
    fn area(self) -> Float
    fn name(self) -> String
}
struct Circle {
    radius: Float

    impl Shape {
        fn area(self) -> Float {
            return ((3.14159 * self.radius) * self.radius)
        }
        fn name(self) -> String {
            return \"circle\"
        }
    }
}
struct Square {
    side: Float
}
impl Square.Shape {
    fn area(self) -> Float {
        return (self.side * self.side)
    }
    fn name(self) -> String {
        return \"square\"
    }
}
fn largest<T: Comparable>(xs: [T]) -> (T?) {
    if xs.len() == 0 {
        return None
    }
    best := xs[0]
    i := 1
    loop i < xs.len() {
        if xs[i] > best {
            best = xs[i]
        }
        i+= 1
    }
    return Val(best)
}
fn print_area(s: Shape) {
    print(\"{s.name()}: {s.area()}\")
}
struct Score {
    points: Int
    derive Comparable
}
fn run() {
shapes: [Shape] :: [Circle.{radius: 1.0}, Square.{side: 2.0}]
    shapes.each((s) => {
        print_area(s)
    })
    nums :: [3, 1, 4, 1, 5]
    print(largest(nums))
    scores := [Score.{points: 10}, Score.{points: 20}]
    scores.sort_by((s: Score) => s.points)
    print(scores[0].points)
}
";
    let (code, stdout) = build_and_run("tir_generic_trait_object", src);
    assert_eq!(code, 0);
    // circle/square areas via dynamic dispatch; largest([3,1,4,1,5]) = 5; scores[0].points = 10.
    assert_eq!(stdout, "circle: 3.14159\nsquare: 4.0\n5\n10\n");
}

/// c150: assigning a Read-convention (borrowed) non-Copy parameter into a struct
/// field previously emitted `((*self)).user_rows = (*user_s)` — a move out of a
/// shared reference (E0507, I2 violation). The fix clones the value when the RHS is
/// a borrowed non-scalar ident, mirroring the `lower_enum_arg` predicate.
#[test]
fn borrow_field_clone() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Ledger {
    rows: [Int]
    fn put_back(&self, s: [Int]) {
        self.rows = s
    }
}
fn run() {
    data: [Int] :: [1, 2, 3]
    ledger: Ledger := Ledger.{ rows: [] }
    ledger.put_back(data)
    print(ledger.rows[0])
    print(ledger.rows[1])
    print(ledger.rows[2])
}
";
    let (code, stdout) = build_and_run("tir_borrow_field_clone", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n2\n3\n");
}

/// D-MUTSELF1: a `mut self` method that assigns a field in place — `self.field = v`
/// and the compound `self.field += v` (S17) — lowers to `((*self)).field = …` on the
/// `&mut Self` receiver. rustc accepts it (I2); the receiver mutates as written.
#[test]
fn mut_self_field_assign() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Counter {
    n: Int
    fn bump(&self) {
        self.n = self.n + 1
    }
    fn add(&self, k: Int) {
        self.n += k
    }
}
fn run() {
    c: Counter := Counter.{ n: 0 }
    c.bump()
    c.add(10)
    print(c.n)
}
";
    let (code, stdout) = build_and_run("tir_mut_self_field", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "11\n");
}

/// D-MUTSELF1: whole-`self` reassignment — `self = New{…}` — lowers to `(*self) = …`
/// (the prior lowering I2 hole, where the `mut self` slot wasn't dereferenced on the
/// LHS, is now closed). rustc accepts the dereferenced assignment.
#[test]
fn mut_self_whole_reassignment() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Counter {
    n: Int
    fn reset(&self) {
        self = Counter.{ n: 0 }
    }
}
fn run() {
    c: Counter := Counter.{ n: 9 }
    c.reset()
    print(c.n)
}
";
    let (code, stdout) = build_and_run("tir_mut_self_whole", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

/// D-MUTSELF1: self-mutation through a TRAIT-impl `mut self` method. The trait
/// declaration and impl both render `&mut self` (was hardcoded `&self`), so the
/// in-place field write compiles. Exercises the trait emit + self-slot deref.
#[test]
fn mut_self_trait_method_field_assign() {
    if !have_rustc() {
        return;
    }
    let src = "\
trait Bumpable {
    fn bump(&self)
}
struct Counter {
    n: Int
}
impl Counter.Bumpable {
    fn bump(&self) {
        self.n = self.n + 1
    }
}
fn run() {
    c: Counter := Counter.{ n: 0 }
    c.bump()
    c.bump()
    print(c.n)
}
";
    let (code, stdout) = build_and_run("tir_mut_self_trait", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n");
}

/// c109 (recursive struct): a self-referential struct field has Rust type
/// `Box<…>` (`cx.boxed_edges`), so its construction value must be wrapped
/// `Box::new(…)` (E0308 otherwise — the AST `emit_struct_lit` was not wrapping).
/// A nested inline `Tree.{ value, child: Val(Tree { … }) }` exercises the boxed
/// wrap at multiple levels; `main` reads only the non-boxed scalar `value`.
/// Both construction levels and `main` route through the TIR.
#[test]
fn recursive_struct_construction() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Tree {
    value: Int
    child: Tree?
}
fn run() {
    root :: Tree.{
        value: 3,
        child: Val(Tree.{
            value: 2,
            child: Val(Tree.{ value: 1, child: None })
        })
    }
    print(root.value)
}
";
    let (code, stdout) = build_and_run("tir_recursive_struct", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

/// c109 (foreign struct literal): an UNqualified cross-module foreign struct literal
/// (`Note.{ text: "hi" }` written in an importing module, no `note.` namespace) must
/// prefix the foreign module (`user_notes::user_Note`) or rustc can't find the type
/// (E0422). The AST `emit_struct_lit` plain branch only prefixed via `user_type_apply_rust`
/// once `cx.foreign_types` is consulted (the fix); the TIR reproduces the prefixed head.
/// `main` constructs + reads the foreign struct and routes through the TIR.
#[test]
fn unqualified_foreign_struct_literal() {
    if !have_rustc() {
        return;
    }
    let main_src = "\
use \"notes\"
fn run() {
    n :: Note.{ text: \"hi\" }
    print(n.text)
}
";
    let notes_src = "\
pub struct Note {
    pub text: String
}
";
    let (code, stdout) = build_and_run_multi(
        "tir_foreign_struct_lit",
        "main.jet",
        &[("main.jet", main_src), ("notes.jet", notes_src)],
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "hi\n");
}
