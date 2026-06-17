//! Regression battery for four codegen ICEs (I2/I3 violations) where the
//! front end emitted Rust that rustc rejected instead of compiling correct
//! code. Each case must pass the front end and — when rustc is available —
//! build (rustc accepting the output is the whole point: a rejection here is
//! a P0 front-end soundness bug, invariant I2).
//!
//! B1 — `JSON.Text(x)` on a borrowed (`view`) parameter moved out of the
//!      borrow; sema must insert a clone.
//! B2 — field access on a std struct (`ProcessResult.code`) mangled the field
//!      name to `user_code`; the std struct has unprefixed fields.
//! B3 — `.get(k)` on a `Map` bound from an `Object(root)` pattern lowered to
//!      list indexing; the binding must keep its `Map` type.
//! B4 — `for k, v in recv.field { … }` parsed `recv.field { … }` as a struct
//!      literal instead of a loop body.

use std::fs;
use std::process::Command;

/// Front-end-compile `src`, then (when rustc is present) build the generated
/// Rust, asserting it is accepted. `name` only labels temp files / failures.
/// Goes through `compile_with_path` (from a real temp file) so `use core.*`
/// std imports resolve, exactly like `jet run`.
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
    assert!(!out.rust.contains("unsafe"), "{name}: invariant I1");

    if Command::new("rustc").arg("--version").output().is_err() {
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
use core.json as json;
fn wrap(x: String) -> String {
    val j = JSON.Text(x);
    return json.render(j);
}
fn main() {
    print(wrap("hi"));
}
"#,
    );
}

#[test]
fn b2_std_struct_field_uses_plain_name() {
    assert_compiles(
        "b2_process_result_field",
        r#"
use core.process as process;
fn main() {
    val result = process.run(["echo", "hi"]) ?? panic("run failed");
    print(result.code);
    print(result.output);
    print(result.errors);
}
"#,
    );
}

#[test]
fn b3_map_get_through_object_pattern() {
    assert_compiles(
        "b3_map_get_object_pattern",
        r#"
use core.json as json;
fn main() {
    val data = json.parse("{{\"a\":1}}") ?? panic("bad");
    if data == Object(root) {
        val v = root.get("a") ?? JSON.Null;
        print(json.render(v));
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
    items: [String, Int],
}
fn main() {
    var h = Holder { items: [:] };
    h.items["x"] = 1;
    loop k, v in h.items {
        print(k);
        print(v);
    }
}
"#,
    );
}
