//! Regression battery for four codegen ICEs (I2/I3 violations) where the
//! front end emitted Rust that rustc rejected instead of compiling correct
//! code. Each case must pass the front end and — when rustc is available —
//! build (rustc accepting the output is the whole point: a rejection here is
//! a P0 front-end soundness bug, invariant I2).
//!
//! B1 — `JSON.Text(x)` on a borrowed (`view`) parameter moved out of the
//!      borrow; sema must insert a clone.
//! B2 — field access on a core struct (`ProcessResult.code`) mangled the field
//!      name to `user_code`; the core struct has unprefixed fields.
//! B3 — `.get(k)` on a `Map` bound from an `Object(root)` pattern lowered to
//!      list indexing; the binding must keep its `Map` type.
//! B4 — `for k, v in recv.field { … }` parsed `recv.field { … }` as a struct
//!      literal instead of a loop body.
//! B5 — `buf[i] = x` on a fixed-size `[T#N]` array left `IndexKind` at its
//!      `Unknown` default (sema's write-side match only knew `List`/`Map`/
//!      `User`, unlike the read-side `infer_index`); the TIR subset gate
//!      excludes any `Unknown`-kind index assign, so codegen (TIR is the
//!      only path, R7) panicked with an I2 ICE instead of emitting code.

use std::fs;
use std::process::Command;

mod common;
use common::have_rustc;

/// Front-end-compile `src`, then (when rustc is present) build the generated
/// Rust, asserting it is accepted. `name` only labels temp files / failures.
/// Goes through `compile_with_path` (from a real temp file) so `use core.*`
/// core imports resolve, exactly like `jet run`.
fn assert_compiles(name: &str, src: &str) {
    let dir = std::env::temp_dir().join(format!("jet_ice_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.jet"));
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "{name}: front end rejected a valid program:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let user_rust = common::strip_vetted_prelude_modules(&out.rust);
    assert!(!user_rust.contains("unsafe"), "{name}: invariant I1");

    if !have_rustc() {
        eprintln!("note: rustc not found; skipping build for {name}");
        return;
    }
    let rs = dir.join(format!("jet_ice_{name}.rs"));
    let bin = dir.join(format!("jet_ice_{name}"));
    fs::write(&rs, &out.rust).unwrap();
    let res = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .unwrap();
    assert!(
        res.status.success(),
        "I2 violated: rustc rejected generated code for {name} — this is a jet bug:\n{}",
        String::from_utf8_lossy(&res.stderr)
    );
}

#[test]
fn b1_json_text_clones_borrowed_view_param() {
    assert_compiles(
        "b1_json_text_view",
        r#"
use core.encoding.json as json
fn wrap(x: String) => String {
    j :: JSON.Text(~x)
    return json.to_string(j)
}
fn run() {
    print(wrap("hi"))
}
"#,
    );
}

#[test]
fn b2_std_struct_field_uses_plain_name() {
    assert_compiles(
        "b2_process_result_field",
        r#"
use core.process as process
fn run() {
    result :: process.run(["echo", "hi"]) ?? panic("run failed")
    print(result.code)
    print(result.output)
    print(result.errors)
}
"#,
    );
}

#[test]
fn b3_map_get_through_object_pattern() {
    assert_compiles(
        "b3_map_get_object_pattern",
        r#"
use core.encoding.json as json
fn run() {
    data :: json.parse("{{\"a\":1}}") ?? panic("bad")
    if data == Object(root) {
        v :: root.get("a") ?? JSON.Null
        print(json.to_string(v))
    }
}
"#,
    );
}

#[test]
fn b4_for_in_field_subject_parses_body_not_struct_lit() {
    assert_compiles(
        "b4_for_field_subject",
        r#"
struct Holder {
    items: [String: Int]
}
fn run() {
    h := Holder.{ items: [] }
    h.items["x"] = 1
    loop (k, v), h.items {
        print(k)
        print(v)
    }
}
"#,
    );
}

#[test]
fn b5_fixed_array_index_assign() {
    assert_compiles(
        "b5_fixed_array_index_assign",
        r#"
fn run() {
    buf := [Int#3].{ 1, 2, 3 }
    i := 1
    buf[i] = 99
    print(buf[0])
    print(buf[1])
    print(buf[2])
}
"#,
    );
}

/// D-EPPAYLOAD1: an OWNED LOCAL moved into an enum-payload construction, then
/// read again afterward, used to move it for real in the generated Rust
/// (`user_Wrapped::user_Value(user_s)`) with no jet-level diagnostic — the
/// later `print("{s}")` reached rustc as a raw, unreported E0382. Sema now
/// auto-clones an owning-position bare ident that is still live after the
/// construction (the same rule `clone_borrowed_struct_field_value` already
/// applied to a *borrowed-param* struct field, now widened to owned locals
/// and threaded through enum-payload construction too).
#[test]
fn b6_enum_payload_clones_owned_local_still_live() {
    assert_compiles(
        "b6_enum_payload_owned_local",
        r#"
enum Wrapped {
    Value(String)
}
fn run() {
    s := "hi"
    w := Wrapped.Value(s)
    print("{s}")
    print(w == Wrapped.Value("hi"))
}
"#,
    );
}

/// The analogous STRUCT-FIELD case: an owned local moved into a struct
/// literal field, then read again — the exact same gap, now closed by the
/// same widened `clone_borrowed_struct_field_value` check.
#[test]
fn b7_struct_field_clones_owned_local_still_live() {
    assert_compiles(
        "b7_struct_field_owned_local",
        r#"
struct Holder {
    payload: String
}
fn run() {
    s := "hi"
    h := Holder.{ payload: s }
    print("{s}")
    print(h.payload)
}
"#,
    );
}
