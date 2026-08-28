//! Regression battery for four codegen ICEs (I2/I3 violations) where the
//! front end emitted Rust that rustc rejected instead of compiling correct
//! code. Each case must pass the front end and — when rustc is available —
//! build (rustc accepting the output is the whole point: a rejection here is
//! a P0 front-end soundness bug, invariant I2).
//!
//! B1 — `JSON.Text(x)` on a borrowed (`view`) parameter moved out of the
//!      borrow; sema must insert a clone.
//! B2 — field access on a core struct (`ProcessResult.code`) mangled the field
//!      name to `__jet_code`; the core struct has unprefixed fields.
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
fn wrap(x: String) String -[]> {
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
    if data == .Object(root) {
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
    items: [String:Int]
}
fn run() {
    h := Holder{ items: [] }
    h.items["x"] = 1
    loop (k, v) in h.items {
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
    buf := [Int#3]{ 1, 2, 3 }
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
/// (`__jet_Wrapped::__jet_Value(__jet_s)`) with no jet-level diagnostic — the
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
    h := Holder{ payload: s }
    print("{s}")
    print(h.payload)
}
"#,
    );
}

#[test]
fn b6_generated_trait_protocol_returns_are_raw() {
    let dir = std::env::temp_dir().join(format!("jet_ice_b6_protocol_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("b6_generated_trait_protocol.jet");
    let source = r#"
use core.encoding.json as json

#CLI
struct Args {
    #Doc("count") count: Int{1}
}

struct Pair {
    left: Int
    right: Int
}

enum Mark {
    Low
    High
}

#Codable
struct Envelope<T> {
    value: T
}

fn run(args: Args) {
    pair :: Pair{left: args.count, right: 2}
    other :: Pair{left: 2, right: 3}
    print(pair == other)
    print(pair < other)
    print(Mark.Low == Mark.Low)
    wire :: json.to_string(Envelope<Int>{value: args.count})
    decoded :: json.decode<Envelope<Int>>(wire) ?? panic("decode")
    print(decoded.value)
}
"#;
    fs::write(&path, source).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(source, &shown).unwrap_or_else(|diags| {
        panic!(
            "b6_generated_trait_protocol_returns_are_raw: front end rejected a valid program:\n{}",
            jet::render_diagnostics(&shown, source, &diags)
        )
    });
    let rust = common::strip_vetted_prelude_modules(&out.rust);

    for (trait_name, method_name, return_type) in [
        ("__jet_Equatable", "equal", "bool"),
        ("__jet_Comparable", "compare", "__jet_Ordering"),
        ("__jet_Encode", "jet_encode", "jet_std::DataTree"),
    ] {
        let marker = format!("{trait_name} for __jet_");
        let mut found = false;
        let mut search_from = 0;
        while let Some(relative) = rust[search_from..].find(&marker) {
            let hit = search_from + relative;
            let start = rust[..hit]
                .rfind("\nimpl ")
                .map(|index| index + 1)
                .unwrap_or(0);
            let block = &rust[start..];
            let end = block.find("\n}\n").unwrap_or(block.len());
            let block = &block[..end];
            let signature = block
                .lines()
                .find(|line| line.contains(&format!("fn {method_name}(")))
                .unwrap_or_else(|| panic!("missing {trait_name}.{method_name} in:\n{block}"));
            assert!(
                signature.contains(&format!(" -> {return_type} {{")),
                "{trait_name}.{method_name} has wrong ABI: {signature}"
            );
            assert!(
                !block.contains("return Ok("),
                "{trait_name}.{method_name} wraps its raw protocol return:\n{block}"
            );
            found = true;
            search_from = start + end;
        }
        assert!(found, "missing generated {trait_name} implementation");
    }
    assert!(
        rust.contains("__jet_Encode for __jet_Envelope<"),
        "generic #Codable witness did not emit its encode implementation"
    );
    assert!(
        rust.contains("fn jet_decode(") && rust.contains("-> Result<"),
        "generated decode lost its declared Result ABI"
    );
    assert!(
        !rust.contains("unsafe"),
        "b6_generated_trait_protocol_returns_are_raw: invariant I1"
    );

    if have_rustc() {
        let rs = dir.join("b6_generated_trait_protocol.rs");
        let bin = dir.join("b6_generated_trait_protocol");
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
            "I2 violated: rustc rejected generated code for b6_generated_trait_protocol_returns_are_raw:\n{}",
            String::from_utf8_lossy(&res.stderr)
        );
    }
}
