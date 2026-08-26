//! TIR pattern, field, and collection-receiver integration tests.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::process::Command;

use tir_support::{assert_tiers_agree, build_and_run, compile, have_rustc};

fn assert_no_hard_coded_generated_literals(path: &std::path::Path) {
    for entry in fs::read_dir(path).expect("read Codegen source directory") {
        let entry = entry.expect("read Codegen directory entry");
        let path = entry.path();
        if path.is_dir() {
            assert_no_hard_coded_generated_literals(&path);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read Codegen source");
        let bytes = source.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] != b'"' {
                at += 1;
                continue;
            }
            let start = at + 1;
            at = start;
            let mut escaped = false;
            while at < bytes.len() {
                let byte = bytes[at];
                at += 1;
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    break;
                }
            }
            let literal = &source[start..at.saturating_sub(1)];
            if literal == "//# __jet_source_map" && path.ends_with("Codegen/mod.rs") {
                continue;
            }
            for (offset, _) in literal.match_indices("__jet_") {
                if literal
                    .as_bytes()
                    .get(offset + b"__jet_".len())
                    .is_some_and(|byte| byte.is_ascii_lowercase())
                {
                    panic!(
                        "hard-coded generated literal in {}: {literal:?}",
                        path.display()
                    );
                }
            }
        }
    }
}

#[test]
fn generated_literals_use_the_canonical_allocator() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/jet-codegen/src/Codegen");
    assert_no_hard_coded_generated_literals(&root);
}

#[test]
fn generated_temporaries_do_not_collide_with_pattern_and_field_locals() {
    let src = r#"
struct Pair {
    left: Int
    right: Int
}
fn run() {
    value :: 7
    v :: 3
    range := 0
    s :: "ok"
    loop i in 1..<4 {
        range = (range + i)
    }
    pair :: Pair{ left: value, right: range }
    xs := [v, pair.right]
    xs[0] = value
    loop item in xs {
        print(item)
    }
    print("{s}:{value}:{pair.left}:{range}")
}
"#;
    let rust = compile("tir_generated_name_patterns", src);
    for stem in ["v", "i", "item"] {
        let user = jet::AST::mangle(stem);
        let generated = jet::AST::mangle_generated(stem);
        assert_ne!(
            user, generated,
            "allocator lanes must stay distinct for {stem}"
        );
        assert!(
            rust.contains(&generated),
            "generated binding {generated} missing"
        );
    }
    assert!(rust.contains(&format!("let {}", jet::AST::mangle("value"))));
    assert!(rust.contains(&format!("let {}", jet::AST::mangle_generated("v"))));
    assert_tiers_agree("tir_generated_name_patterns", src, "7\n6\nok:7:7:6\n");
}

#[test]
fn binary_pattern_width_classes_match_all_tiers() {
    let src = r#"
fn run() {
    subbyte :: [U8]{0xAB}
    if subbyte == {
        [U8]{"{hi:U4}{lo:U4}"} -> { print("sub={hi}/{lo}") }
        else -> { print("sub=miss") }
    }
    aligned :: [U8]{0x01, 0x02}
    if aligned == {
        [U8]{"{value:U16be}"} -> { print("aligned={value}") }
        else -> { print("aligned=miss") }
    }
    non_power24 :: [U8]{0x01, 0x02, 0x03}
    if non_power24 == {
        [U8]{"{be:U24be}"} -> { print("wide={be}") }
        else -> { print("wide=miss") }
    }
    if non_power24 == {
        [U8]{"{le:U24le}"} -> { print("wide-le={le}") }
        else -> { print("wide-le=miss") }
    }
    non_power40 :: [U8]{0x01, 0x02, 0x03, 0x04, 0x05}
    if non_power40 == {
        [U8]{"{be:U40be}"} -> { print("wide40={be}") }
        else -> { print("wide40=miss") }
    }
    if non_power40 == {
        [U8]{"{le:U40le}"} -> { print("wide40-le={le}") }
        else -> { print("wide40-le=miss") }
    }
    non_power48 :: [U8]{0x01, 0x02, 0x03, 0x04, 0x05, 0x06}
    if non_power48 == {
        [U8]{"{be:U48be}"} -> { print("wide48={be}") }
        else -> { print("wide48=miss") }
    }
    if non_power48 == {
        [U8]{"{le:U48le}"} -> { print("wide48-le={le}") }
        else -> { print("wide48-le=miss") }
    }
    non_power56 :: [U8]{0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07}
    if non_power56 == {
        [U8]{"{be:U56be}"} -> { print("wide56={be}") }
        else -> { print("wide56=miss") }
    }
    if non_power56 == {
        [U8]{"{le:U56le}"} -> { print("wide56-le={le}") }
        else -> { print("wide56-le=miss") }
    }
}
"#;
    assert_tiers_agree(
        "tir_binary_pattern_width_classes",
        src,
        "sub=10/11\naligned=258\nwide=66051\nwide-le=197121\nwide40=4328719365\nwide40-le=21542142465\nwide48=1108152157446\nwide48-le=6618611909121\nwide56=283686952306183\nwide56-le=1976943448883713\n",
    );
}

/// c109 (builtin-name collision): a user method whose name collides with a builtin
/// (`get`/`len`) was mis-dispatched by `emit_builtin_method` (name-keyed, not
/// receiver-typed) → `b.get()` emitted garbage, `b.len()` → E0599. The fix dispatches
/// to the USER method (`__jet_<method>`) when `recv_type == Some(T)` and `(T, method) ∈
/// cx.method_sigs`. `main` and both methods route through the TIR.
#[test]
fn user_method_shadowing_builtin_name() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Crate {
    items: [Int]

    fn get(self) Int {
        return 42
    }
    fn len(self) Int {
        return 7
    }
}
fn run() {
    b :: Crate{ items: [1, 2, 3] }
    print(b.get())
    print(b.len())
}
";
    let (code, stdout) = build_and_run("tir_builtin_collision", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "42\n7\n");
}

/// c109 (builtin-name collision, receiver side) — the sibling of the test above.
/// The builtin table is receiver-TYPED, but the receiver-type resolver answered
/// `None` for a struct FIELD read, and a `None` there does not read as "unknown",
/// it reads as "take the List surface". So `entry.relative.replace(a, b)` on a
/// `String` field lowered to `jet_list_replace(&String, String, String)` — which
/// wants `&[T]`, `i64`, `T` — and rustc was handed Jet's own ill-typed output,
/// an internal compiler error by I2 (found by tests/agent_workloads
/// repository-marker-scan). Every name the String and List surfaces share had the
/// same hole, `len` included. The fix reads the field's DECLARED type
/// (`builtin_recv_ty` / `declared_field_ty`, TIR/lower/builtins.rs), so a String
/// field takes the String arm, a list field keeps the index/element arm, and a
/// receiver no table row claims (a bare list literal) keeps the legacy list
/// fallback. All three shapes, and all three tiers, in one snippet.
#[test]
fn builtin_dispatch_reads_the_field_receiver_type() {
    let src = "\
struct ScanEntry {
    relative: String
    marks: [Int]
}
fn run() {
    entry :: ScanEntry{ relative: \"a-b-c\", marks: [1, 2, 1] }
    print(entry.relative.replace(\"-\", \"/\"))
    print(entry.relative.len())
    print(entry.marks.replace(1, 9))
    print([1, 2, 1].replace(1, 9))
}
";
    assert_tiers_agree(
        "tir_builtin_field_receiver_dispatch",
        src,
        "a/b/c\n5\n[1, 9, 1]\n[1, 9, 1]\n",
    );
}

/// c109 (`is_empty` Bool fix): `Collections::*_method_return` typed `is_empty` as
/// `Int`, so `e := xs.is_empty()` emitted `let e: i64 = (…).is_empty()` (bool ≠ i64
/// → rustc E0308) and `if xs.is_empty()` was E0110 at sema. The fix returns `Bool`;
/// `is_empty` is now covered (`TBuiltinOp::IsEmpty`) on list/map/string receivers.
#[test]
fn is_empty_returns_bool() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn check(xs: [Int]) {
    if xs.is_empty() {
        print(\"empty\")
    } else {
        print(\"not empty\")
    }
}
fn run() {
    e :: [1, 2, 3].is_empty()
    print(e)
    m :: [1: 2]
    print(m.is_empty())
    s :: \"hi\"
    print(s.is_empty())
empty :: [Int]{}
    check(empty)
    check([9])
}
";
    let (code, stdout) = build_and_run("tir_is_empty", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "false\nfalse\nfalse\nempty\nnot empty\n");
}

/// c109 (bare `?? return` fix): `infer_or_fallback`'s logic was inverted — a bare
/// `?? return` (no value) was sema-rejected in a UNIT fn (E0405) and accepted in a
/// NON-unit fn (where rustc then rejected the emitted `return;` → E0069). The fix
/// accepts a bare `?? return` ONLY in a unit fn (`return;` is valid) and rejects it
/// in a value-returning fn. The unit-fn form routes through the TIR
/// (`orfallback_rhs_in_subset → Return(None)`, emitting `None => return`).
#[test]
fn bare_or_return_in_unit_fn() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn f(xs: [Int]) {
    x := xs.first() ?? return
    print(x)
}
fn run() {
    f([10, 20])
empty :: [Int]{}
    f(empty)
    print(99)
}
";
    let (code, stdout) = build_and_run("tir_bare_or_return", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "10\n99\n");
}

/// c109 (boxed recursive field read): reading a self-referential struct field
/// (`t.child`, Rust type `Box<…>` via `cx.boxed_edges`) miscompiled — the read
/// yielded a `Box<…>` where the unboxed type was wanted (rustc E0308). The fix
/// derefs the `Box` (`(*(…))`) on a boxed-field read. With the read fixed, a
/// recursive struct is now a covered VALUE type, so a fn that builds AND traverses
/// a `Tree` (binds a boxed child, matches, recurses) routes through the TIR.
#[test]
fn boxed_recursive_struct_field_read() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Tree {
    value: Int
    child: ?Tree
}
fn sum(t: Tree) Int {
    total := t.value
kid ::  t.child 
    if kid == {
        Val(c) -> {
            total = total + sum(c)
        }
        None -> {}
    }
    return total
}
fn run() {
    root :: Tree{
        value: 3,
        child: Val(Tree{
            value: 2,
            child: Val(Tree{ value: 1, child: None })
        })
    }
    print(sum(root))
}
";
    let (code, stdout) = build_and_run("tir_boxed_field_read", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "6\n");
}

/// c109 (borrowed struct-lit value clone): a struct literal whose field value is a
/// bare borrowed-in-env non-Copy ident (`Person{ name: n }` where `n: String` is a
/// `read` param → `&String`) emitted `__jet_name: (*__jet_n)` → rustc E0507 ("cannot
/// move out of `*user_n`"). `field_read_to_clone` clones owning field READS but not a
/// bare borrowed ident used as a struct-lit value; the fix clones it in sema's
/// elaboration. `make` (struct lit + the sema-inserted clone) routes through the TIR.
#[test]
fn borrowed_struct_lit_field_value_cloned() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Person {
    name: String
}
fn make(n: String) Person {
    return Person{ name: n }
}
fn run() {
    p :: make(\"Ada\")
    print(p.name)
}
";
    let (code, stdout) = build_and_run("tir_borrowed_struct_lit", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "Ada\n");
}

/// c109 (B3): a struct-destructuring binding `Type{ x, y } :: p` routes through
/// the TIR and prints the field sum, matching the old `BindPattern::Struct` baseline.
#[test]
fn struct_destructure_binding() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Point { x: Int, y: Int }
fn run() {
    p :: Point{ x: 1, y: 2 }
    Point{ x, y } :: p
    print(x + y)
}
";
    let (code, stdout) = build_and_run("tir_struct_destructure", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "3\n");
}

/// D-DESTRUCT1: struct-shaped dispatch arm heads bind fields and test literal
/// fields in the same arm. This is the source-level dispatch spelling; the
/// internal Rust lowering may still call the helper path a switch.
#[test]
fn struct_pattern_dispatch_arm_head_runs() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Incident {
    kind: String
    title: String
    retries: Int
}
fn route(i: Incident) String {
    if i == {
        { kind: \"page\", title, .. } -> { return title }
        { kind: \"ticket\", title, .. } -> { return title }
        else -> { return \"other\" }
    }
}
fn run() {
    page :: Incident{ kind: \"page\", title: \"database\", retries: 2 }
    ticket :: Incident{ kind: \"ticket\", title: \"docs\", retries: 1 }
    other :: Incident{ kind: \"note\", title: \"memo\", retries: 0 }
    print(route(page))
    print(route(ticket))
    print(route(other))
}
";
    let (code, stdout) = build_and_run("tir_struct_pattern_dispatch", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "database\ndocs\nother\n");
}

/// c109 (B4): a user-enum variant if-let condition `if m == .Ping(n) { } else { }`
/// routes through the TIR and binds the payload, matching the old if-let baseline.
#[test]
fn user_enum_variant_if_let_condition() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Msg { Ping(Int) Pong }
fn f(m: Msg) Int {
    if m == .Ping(n) {
        return n
    } else {
        return -1
    }
}
fn run() {
    print(f(Msg.Ping(7)))
    print(f(Msg.Pong))
}
";
    let (code, stdout) = build_and_run("tir_user_enum_if_let", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n-1\n");
}

/// c109 (B2): a fixed-size-list type `[E#N]` as a param and
/// as a struct field routes through the TIR (rendered `Vec<E>`).
#[test]
fn fixed_size_list_param_and_field() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn double(n: Int) Int {
    return (n * 2)
}
struct Grid { row: [Int#3] }
fn firstof(xs: [Int#3]) Int {
    return xs[0]
}
fn run() {
    print(firstof([Int#3]{ double(1), double(2), double(3) }))
    g :: Grid{ row: [Int#3]{ double(1), double(2), double(3) } }
    print(g.row[1])
}
";
    let (code, stdout) = build_and_run("tir_fixed_list", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "2\n4\n");
}

/// D-FIXARR1: an inferred fixed-list local keeps its array type until the
/// ordinary call boundary decides to copy it into the callee's `Vec` slot.
#[test]
fn inferred_fixed_list_widens_at_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn total(xs: [Int]) Int {
    return xs[0] + xs[1] + xs[2]
}
fn run() {
    values :: [Int#3]{ 1, 2, 3 }
    print(total(values))
}
";
    let (code, stdout) = build_and_run("tir_fixed_list_inferred_call", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "6\n");
}

/// D-FIXARR1 / D-CRYPTO-DIAG1: Core calls use the same fixed-list widening as
/// ordinary calls after sema consumes the compile-known length fact.
#[test]
fn fixed_size_list_widens_at_core_call() {
    if !have_rustc() {
        return;
    }
    let src = "\
use core.crypto.expert as expert

fn run() {
    seed :: [U8#32]{ 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31 }
    #Unsafe(\"fixed signature vector\") {
        signature :: expert.ed25519_sign(seed, [])
    }
    print(\"ok\")
}
";
    let (code, stdout) = build_and_run("tir_fixed_list_core_call", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "ok\n");
}

/// c109 (B1): a mixed-switch over a NON-IDENT subject (a field access) with a
/// payload-binding arm head. The deleted emitter once produced
/// `matches!(…, Some(c))` then used the unbound `c` (E0425); TIR emits the Rust
/// `match` that binds the payload. The subject is evaluated once.
#[test]
fn mixed_switch_non_ident_subject_binds_payload() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Holder { val: ?Int }
fn f(h: Holder) Int {
    if h.val == {
        Val(c) -> { return c }
        else -> { return 0 }
    }
}
fn run() {
    hold :: Holder{ val: Val(5) }
    print(f(hold))
    empty :: Holder{ val: None }
    print(f(empty))
}
";
    let (code, stdout) = build_and_run("tir_mixed_nonident_payload", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "5\n0\n");
}

/// c109 (B1): a mixed-switch over a NON-IDENT subject (a call) with unit-variant arm
/// heads. Previously the AST emitted a bare unqualified `(subj == (__jet_Red))` and
/// re-evaluated the call per arm (E0425); now it routes through the Rust `match` over
/// the qualified variants, subject evaluated once.
#[test]
fn mixed_switch_non_ident_subject_qualifies_variants() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Light { Red Green Yellow }
fn pick() Light {
    return Light.Red
}
fn classify() Int {
    if pick() == {
        .Red -> { return 1 }
        .Green -> { return 2 }
        else -> { return 0 }
    }
}
fn run() {
    print(classify())
}
";
    let (code, stdout) = build_and_run("tir_mixed_nonident_variant", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

/// c109 (S57/M9.5): a comptime LOCAL `@name :: expr` in a function body. Sema
/// evaluates `build()` at compile time and codegen emits the result as literal data
/// (`let __jet_xs: Vec<i64> = vec![10i64, 20i64, 30i64];`). The TIR reproduces that
/// serialized literal verbatim; the runtime `init` expr is never emitted. Mirrors
/// `tests/comptime_diff.rs::local_comptime_is_literal_data`.
#[test]
fn comptime_local_is_literal_data() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn build() [Int] {
    xs := [Int]{}
    loop i in 1..3 {
        xs.push(i * 10)
    }
    return xs
}
fn run() {
    @xs :: build()
    print(\"{@xs}\")
    print(\"{@xs[1]}\")
}
";
    let (code, stdout) = build_and_run("tir_comptime_local", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "[10, 20, 30]\n20\n");
}

/// c109 Phase 6b: a `Shared<T>` value passed to a FREE (non-method) call inside a loop
/// auto-clones the handle — `emit_call_args` emits `(…).clone()` (D-MEM1 S6: `Shared<T>`
/// lowers to `jet_std::JetShared<T>`, a newtype with its own cheap-handle `Clone` impl,
/// not a bare `Arc<T>` — was `Arc::clone(&…)` before this stage) and the receiving
/// `Shared<T>` `Read` param borrows it (`&(…)`). The gate previously excluded
/// `shared_auto_clone` on plain `Call` args, routing `loop_user`/`noop` through the AST
/// path; both now route through the TIR with a byte-identical emit. A `Shared<T>` value
/// has no surface constructor (it only ever arrives as a param), so this is a compile +
/// byte-exact-Rust assertion (the same surface `tests/ownership.rs` and `tests/ui_lint`
/// exercise) rather than a build+run. rustc accepting the output proves I2.
#[test]
fn shared_auto_clone_in_free_call_arg() {
    if !have_rustc() {
        return;
    }
    let src = "\
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
";
    let dir = std::env::temp_dir().join(format!("jet_tir_shared_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("shared.jet");
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    // Byte-exact auto-clone emit: the free-call arg auto-clones the handle, then
    // the `Read` non-scalar `Shared<Int>` param borrows it. (D-MEM1 S6: `Shared<T>`
    // now lowers to `jet_std::JetShared<T>`, not a bare `std::sync::Arc<T>` — its
    // own `Clone` impl is a cheap handle clone, so plain `.clone()` replaces the
    // old `Arc::clone(&…)` text.)
    assert!(
        out.rust.contains(
            "let __jet___arg91_0 = &(((*__jet_h)).clone());\n    __jet_noop(__jet___arg91_0)"
        ),
        "shared auto-clone free-call arg not byte-exact:\n{}",
        out.rust
    );
    // The receiving param signature is the shared `rust_param_type` form.
    assert!(
        out.rust
            .contains("pub fn __jet_noop(__jet_h: &jet_std::JetShared<i64>)"),
        "Shared<Int> param signature not byte-exact:\n{}",
        out.rust
    );
    // I2: rustc accepts the generated Rust.
    let rs = dir.join("shared.rs");
    let bin = dir.join("shared");
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
}

/// c109: an owning field read of a NON-SCALAR field (`s :: p.name`, `name:
/// String`). Sema rewrites the read in owning position to `(p.name).clone()`;
/// the TIR emits `((__jet_p).__jet_name).clone()`. The single-uppercase-letter
/// struct name `P` is a concrete declared type (not a type var), so `main`
/// routes through the TIR. Runs (the two clones print independently) and is
/// byte-exact on the owning-clone emit.
#[test]
fn owning_nonscalar_field_read_clones() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct P {
    name: String
}

fn run() {
    p :: P{ name: \"x\" }
    s :: p.name
    t :: p.name
    print(s)
    print(t)
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust
            .contains("let __jet_s: String = ((__jet_p).__jet_name).clone();"),
        "owning non-scalar field-read clone not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_owning_field_clone", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "x\nx\n");
}

/// c109: an indexed map-assign whose index BASE is a struct field read
/// (`s.scores["a"] = 1`, `scores: [String:Int]`). The `LValue::Index` gate
/// admits a field-read base + the sema-resolved `IndexKind::Map`; `main` routes
/// through the TIR and the assign emits the `jet_map_insert` helper form
/// byte-for-byte. Runs (insert then index-read prints the value).
#[test]
fn indexed_map_assign_through_field() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct S {
    scores: [String:Int]
}

fn run() {
    s := S{ scores: [] }
    s.scores[\"a\"] = 1
    print(s.scores[\"a\"])
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust
            .contains("jet_map_insert(&mut ((__jet_s).__jet_scores),"),
        "map-assign through field not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_map_assign_field", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "1\n");
}

/// c109: a map builtin (`.len()`) on a struct-FIELD-read receiver
/// (`s.scores.len()`), where the field came from an empty-map struct-literal
/// field (`scores: []` takes its type from the struct field). The builtin gate
/// admits a field-read receiver; `main` routes through the TIR and emits
/// `((__jet_s).__jet_scores).len() as i64` byte-for-byte. Runs (empty map → 0).
#[test]
fn map_builtin_on_struct_field_receiver() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct S {
    scores: [String:Int]
}

fn run() {
    s := S{ scores: [] }
    print(s.scores.len())
}
";
    let out = jet::compile(src).expect("empty map literal in field position should typecheck");
    assert!(
        out.rust.contains("((__jet_s).__jet_scores).len() as i64"),
        "map builtin on field receiver not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_map_builtin_field", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "0\n");
}

/// c109: a field read off a comptime-const STRUCT value (`@pair_value :: Pair{…}`;
/// `pair_value.left`) and an `==` against a comptime-const ENUM value (`@light_value ::
/// Light.Green`; `light_value == Light.Green`). The struct field read folds to the
/// projected comptime value; the comparison uses the canonical ordering hook.
/// `main` routes through the TIR; runs to the round-trip output.
#[test]
fn field_read_and_eq_on_inlined_comptime_values() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Pair {
    left: Int
    right: String
}

enum Light {
    Red
    Green
}

@pair_value :: Pair{left: 7, right: \"seven\"}
@light_value :: Light.Green

fn run() {
    p :: Pair{left: 7, right: \"seven\"}
    l :: Light.Green
    print(\"{@pair_value.left}\")
    print(\"{p.left}\")
    print(\"{@pair_value.right}\")
    print(\"{p.right}\")
    print(\"{@light_value == Light.Green}\")
    print(\"{l == Light.Green}\")
}
";
    let out = jet::compile(src).expect("should compile");
    // A comptime const is compile-time DATA: sema folds `@pair_value.left`
    // to the projected value (`CheckerInfer/expr.rs::fold_comptime_struct_field`),
    // so no field read of it survives into the emitted Rust.
    assert!(
        out.rust.contains("jet_std::jet_int_to_string(7i64)")
            && out.rust.contains("(\"seven\".to_string()).jet_display()"),
        "comptime struct field read was not folded to its value:\n{}",
        out.rust
    );
    // Byte-exact: the RUNTIME twin of each read is still the ordinary TIR
    // field read, so the fold above is the only difference between them.
    assert!(
        out.rust
            .contains("jet_std::jet_int_to_string((__jet_p).__jet_left)")
            && out.rust.contains("((__jet_p).__jet_right).jet_display()"),
        "runtime struct field read not byte-exact:\n{}",
        out.rust
    );
    // Byte-exact: user-enum equality routes through a canonical Jet hook, never
    // a native Rust `==` on the mangled enum. `Light` derives BOTH Comparable
    // and Equatable, and a type with both defines `==` by its ONE ordering law
    // (`compare(…) == Ordering.Equal`, D-CMP spaceship), so `==`, `<` and `>`
    // can never disagree.
    assert!(
        out.rust.contains(
            "((__jet_Light::__jet_Green).compare(&(__jet_Light::__jet_Green))) == (__jet_Ordering::__jet_Equal)"
        ),
        "comptime enum equality hook not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_comptime_struct_enum_values", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n7\nseven\nseven\ntrue\ntrue\n");
}

/// c109 (D-PATW): a user-enum variant if-let condition with a WILDCARD payload
/// slot (`if w == .Some(_)`). The `_` binds nothing; the if-let head renders
/// `if let __jet_Wrapper::__jet_Some(_) = __jet_w` byte-for-byte. `main` routes
/// through the TIR; runs (the `Some(42)` value matches the wildcard).
#[test]
fn wildcard_enum_payload_if_let() {
    if !have_rustc() {
        return;
    }
    let src = "\
enum Wrapper {
    Some(Int)
    Empty
}
fn run() {
    w :: Wrapper.Some(42)
    if w == .Some(_) {
        print(\"has value\")
    }
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust
            .contains("if let __jet_Wrapper::__jet_Some(_) = __jet_w"),
        "wildcard enum-payload if-let not byte-exact:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_wildcard_payload_iflet", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "has value\n");
}

/// c97/D-STRPARSE1: `String.lines()` (→ `[String]`) and `Int.parse(text)` (→
/// `Int !ParseError`). Both are compiler built-ins, so `main` routes
/// through the TIR — proven by the emitted `jet_string_lines` helper call and
/// the Prelude parse-kernel call. `Int.parse` composes with `??`: a good parse
/// yields the value, a bad one (`"abc"`) takes the fallback.
#[test]
fn string_lines_and_int_parse() {
    if !have_rustc() {
        return;
    }
    let src = "\
fn run() {
    n :: Int.parse(\"42\") ?? -1
    print((n + 1))
    bad :: Int.parse(\"abc\") ?? -1
    print(bad)
    lines :: \"a\\nb\\nc\".lines()
    print(lines.len())
    loop line in lines {
        print(line)
    }
    total := 0
    loop row in \"10\\n20\\n30\".lines() {
        total += (Int.parse(row) ?? 0)
    }
    print(total)
}
";
    let out = jet::compile(src).expect("should compile");
    // TIR routing: `lines()` lowers to the `jet_string_lines` helper, `Int.parse`
    // to `TBuiltinOp::ParseInt`. (The AST emit path is gone — these prove the TIR.)
    assert!(
        out.rust.contains("jet_string_lines(&("),
        "lines() did not lower through the TIR (no jet_string_lines):\n{}",
        out.rust
    );
    // I9: `ParseInt` calls the ONE parse kernel every tier runs
    // (`jet_int_parse` = trim + arbitrary-precision parse + the same failure
    // text, `Prelude/CoreLib/JetStd/CommonTypes.rs`), not an inlined
    // `.trim().parse::<i64>()` that would reject an `Int` too big for `i64`.
    assert!(
        out.rust.contains("jet_std::jet_int_parse("),
        "Int.parse did not lower through the TIR (no parse form):\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_string_parse", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "43\n-1\n3\na\nb\nc\n60\n");
}

#[test]
fn array_of_structs_field_mutation() {
    if !have_rustc() {
        return;
    }
    let src = "\
struct Point {
    x: Int
}
fn run() {
    points := [Point{x: 1}, Point{x: 2}]
    points[0].x = 11
    points[0].x += 1
    print(points[0].x)
}
";
    let out = jet::compile(src).expect("should compile");
    assert!(
        out.rust.contains(
            "{ let __jet___v = 11i64; (__jet_points)[0i64 as usize].__jet_x = __jet___v; }"
        ),
        "plain indexed field assignment did not mutate the list element:\n{}",
        out.rust
    );
    // The compound form still goes through a CHECKED add, never Rust's `+`:
    // `Int` addition is the one Prelude spine `jet_std::jet_int_add`, which
    // `checked_add`s the packed small case and promotes to the arbitrary-
    // precision value instead of wrapping (`Prelude/CoreLib/JetStd/
    // CommonTypes.rs`). The element it reads comes from the bounds-checked
    // `jet_index_vec`, and the write is the same element place as above.
    assert!(
        out.rust.contains(
            "{ let __jet___v = jet_std::jet_int_add((jet_index_vec(&(__jet_points), 0i64, \"input.jet\", 7)).__jet_x, 1i64); (__jet_points)[0i64 as usize].__jet_x = __jet___v; }"
        ),
        "compound indexed field assignment did not use the checked add spine:\n{}",
        out.rust
    );
    assert!(
        !out.rust.contains(".__jet_x +="),
        "indexed field compound assignment leaked to Rust +=:\n{}",
        out.rust
    );
    let (code, stdout) = build_and_run("tir_struct_list_mutation", src);
    assert_eq!(code, 0);
    assert_eq!(stdout, "12\n");
}

#[test]
fn indexed_struct_field_compound_rejects_user_operator_before_codegen() {
    let src = r#"
struct Vec2 {
    x: Int
    y: Int
}

struct Holder {
    value: Vec2
}

impl Vec2.Add {
    fn add(self, rhs: Vec2) Vec2 {
        return Vec2{ x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

fn run() {
    hs := [Holder{ value: Vec2{ x: 1, y: 2 } }]
    hs[0].value += Vec2{ x: 3, y: 4 }
    print("{hs[0].value.x},{hs[0].value.y}")
}
"#;
    let diags = jet::compile(src).expect_err("indexed user operator needs a stable place");
    assert!(
        diags.iter().any(|diag| diag.code == "E0362"),
        "indexed field compound assignment reached codegen instead of E0362: {diags:#?}"
    );
}
