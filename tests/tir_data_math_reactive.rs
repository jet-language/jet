//! TIR data math reactive integration tests.

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;

use tir_support::{build_and_run, have_rustc};

// --- D-SOA1 / D-SOA2A-D: `#Layout(columnar)` struct-of-arrays --------------

/// Compile a program to Rust (front end only) for source-level assertions.
fn compile_rust(name: &str, src: &str) -> String {
    let dir = std::env::temp_dir().join(format!("jet_tir_test_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    jet::compile_with_path(src, &shown)
        .unwrap_or_else(|diags| {
            panic!(
                "front end rejected:\n{}",
                jet::render_diagnostics(&shown, src, &diags)
            )
        })
        .rust
}

const COLUMNAR_PROG: &str = "\
#Layout(columnar)
struct P {
    x: Float
    mass: Float
}
fn total(ps: [P]) -> Float {
    s := 0.0
    loop p in ps {
        s = s + p.mass
    }
    return s
}
fn run() {
    ps: [P] := [P.{ x: 0.0, mass: 1.0 }, P.{ x: 1.0, mass: 2.0 }]
    ps.push(P.{ x: 2.0, mass: 3.0 })
    print(ps.len())
    print(ps[2].x)
    print(ps[1].mass)
    print(total(ps))
}
";

/// The whole columnar surface (construct, push, len, index-read, field-read,
/// iterate) compiles and runs with AoS-identical behavior.
#[test]
fn columnar_list_core_surface_runs() {
    if !have_rustc() {
        return;
    }
    let (code, stdout) = build_and_run("tir_columnar_core", COLUMNAR_PROG);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n2.0\n2.0\n6.0\n");
}

/// Codegen emits the struct-of-arrays storage type and routes the list ops
/// through it — and emits ZERO `unsafe` (I1 golden grep).
#[test]
fn columnar_lowers_to_struct_of_arrays_no_unsafe() {
    let rust = compile_rust("tir_columnar_gen", COLUMNAR_PROG);
    assert!(
        rust.contains("struct user_P_columns"),
        "expected a generated struct-of-arrays type"
    );
    assert!(
        rust.contains("from_aos") && rust.contains("gather_at") && rust.contains("iter_aos"),
        "expected the columnar inherent API in the output"
    );
    // I1: no `unsafe` anywhere in generated columnar code.
    assert!(
        !tir_support::strip_vetted_prelude_modules(&rust).contains("unsafe"),
        "columnar codegen must emit no `unsafe`"
    );
}

/// D-SOA2D: serialization is transparent — a columnar `[S]` encodes identically
/// to the array-of-structs form.
#[test]
fn columnar_serialization_is_transparent() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.encoding.json as json
@[Codable]
#Layout(columnar)
struct Pt { a: Int, b: Int }
@[Codable]
struct PlainPt { a: Int, b: Int }
fn run() {
    cs: [Pt] :: [Pt.{ a: 1, b: 2 }, Pt.{ a: 3, b: 4 }]
    ps: [PlainPt] :: [PlainPt.{ a: 1, b: 2 }, PlainPt.{ a: 3, b: 4 }]
    print(json.to_string(cs) == json.to_string(ps))
    print(json.to_string(cs))
}
";
    let (code, stdout) = build_and_run("tir_columnar_serde", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "true\n[{\"a\":1,\"b\":2},{\"a\":3,\"b\":4}]\n");
}

/// D-LINALG1: vector + matrix math (constructors, dot/cross, operators,
/// matrix-vector transform) compiles and runs with the right results.
#[test]
fn linalg_vectors_and_matrices() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
a: Vec3 :: Vec3(1.0, 2.0, 3.0)
b: Vec3 :: Vec3(4.0, 5.0, 6.0)
    print(a.dot(b))
c: Vec3 :: a + b
    print(c.to_array())
    print(a.cross(b).to_array())
scale: Mat3 :: Mat3(2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0)
out: Vec3 :: scale * a
    print(out.to_array())
}
";
    let (code, stdout) = build_and_run("tir_linalg", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "32.0\n[5.0, 7.0, 9.0]\n[-3.0, 6.0, -3.0]\n[2.0, 4.0, 6.0]\n"
    );
}

/// D-SIMD2: SIMD lane construction, element-wise operators, splat, lane index,
/// reductions (named + `reduce(#Op)`), and the `[F32#4]` array bridge.
#[test]
fn simd_lanes_ops_and_reductions() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
v: F32x4 :: F32x4(1.0, 2.0, 3.0, 4.0)
w: F32x4 :: F32x4(10.0, 20.0, 30.0, 40.0)
s: F32x4 :: v + w
    print(s.to_array())
    print(v[2])
    print(v.sum())
    print(v.reduce(#Max))
    print(v.reduce(#Mul))
    print(F32x4.splat(5.0).to_array())
d: F64x2 :: F64x2.from_array([1.5, 2.5])
    print(d.sum())
}
";
    let (code, stdout) = build_and_run("tir_simd", src);
    assert_eq!(code, 0);
    assert_eq!(
        stdout,
        "[11.0, 22.0, 33.0, 44.0]\n3.0\n10.0\n4.0\n24.0\n[5.0, 5.0, 5.0, 5.0]\n4.0\n"
    );
}

/// A user struct named `Vec3` (a built-in math name) keeps its own semantics —
/// the built-in family yields to user types (no silent hijack).
#[test]
fn user_type_shadows_builtin_math_name() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Vec3 { x: Int, y: Int }
fn run() {
    v: Vec3 :: Vec3.{ x: 3, y: 4 }
    print(v.x)
    print(v.y)
}
";
    let (code, stdout) = build_and_run("tir_user_vec3", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n4\n");
}

/// D-REACT1=B: a signal + derived + effect reactive flow. A signal change
/// recomputes the derived and re-runs the effect (a real reactive update).
#[test]
fn reactive_signal_derived_effect() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.reactive as reactive
fn run() {
    n := reactive.signal(1)
    doubled := reactive.derived(() => (n.get() * 2))
    print(doubled.get())
    reactive.effect(() => {
        print(doubled.get())
    })
    n.set(5)
    print(doubled.get())
}
";
    let (code, stdout) = build_and_run("tir_reactive_flow", src);
    assert_eq!(code, 0, "reactive program should run cleanly");
    // initial derived (2), effect runs now (2), effect re-runs on set (10),
    // final direct read (10).
    assert_eq!(stdout, "2\n2\n10\n10\n");
}

/// D-REACTCORE1 + D-SIGNAL1: `#Reactive { … }` and `reactive.computed`.
#[test]
fn reactive_scope_marker() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.reactive as reactive
fn run() {
    n := reactive.signal(1)
    doubled := reactive.computed(() => (n.get() * 2))
    print(doubled.get())
    #Reactive {
        print(doubled.get())
    }
    n.set(5)
    print(doubled.get())
}
";
    let (code, stdout) = build_and_run("tir_reactive_scope", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n2\n10\n10\n");
}

/// D-REACT1=B: a `Signal<String>` carries non-numeric data; `.set` notifies a
/// derived that concatenates it.
#[test]
fn reactive_string_signal() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.reactive as reactive
fn run() {
    name := reactive.signal(\"world\")
    greeting := reactive.derived(() => \"hello, {name.get()}\")
    print(greeting.get())
    name.set(\"jet\")
    print(greeting.get())
}
";
    let (code, stdout) = build_and_run("tir_reactive_string", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "hello, world\nhello, jet\n");
}
