//! Codegen is deliberately dumb (invariant I3): no checking happens here.
//! If a Program reaches this module, emission must always succeed and the
//! resulting Rust must always compile.
//!
//! Type-alignment rules (so emitted Rust always typechecks):
//!   - scalar params (Int/Float/Bool) pass by value, String by `&String`;
//!     `mut` params are `&mut T`; `take` params are `T` by value
//!   - a name bound to a `&T`/`&mut T` parameter is always emitted as the
//!     place `(*user_x)`, so every name has its plain Jet type
//!   - every printed/interpolated value goes through the `JetShow` trait
//!     in the prelude (Float keeps its decimal part there, S21)
//!   - every operator result is fully parenthesized

use crate::AST::{
    BenchDef, Item,
    Program, ProgramBundle, TestDef,
};
use crate::FFI::FfiLink;
use crate::Traits;
use crate::Sema::CompileMode;
use crate::Syntax;


mod CModule;
mod Context;
mod Imports;
mod Items;
mod Statement;
mod TIR;
mod Tuples;
mod Utils;

pub(crate) use CModule::*;
pub(crate) use Context::*;
pub(crate) use Imports::*;
pub(crate) use Items::*;
pub(crate) use Statement::*;
pub(crate) use Tuples::*;
pub(crate) use Utils::*;

/// Emitted at the top of every program: core runtime helpers used by generated Rust.
const PRELUDE: &str = include_str!("../Prelude/Core.rs");

/// Extra helpers for `jet test` harnesses only (M6/S43, E2-M11 D-TOOL4).
const TEST_PRELUDE: &str = r#"
/// D-TOOL4 (E2-M11): snapshot wrapper — records or compares a golden snapshot.
struct JetExpect { value: String }
fn jet_expect(s: String) -> JetExpect { JetExpect { value: s } }
impl JetExpect {
    fn snapshot(&self, snap_path: &str) -> Result<(), String> {
        let update = std::env::var("JET_UPDATE_SNAPSHOTS").is_ok();
        if update {
            std::fs::create_dir_all(std::path::Path::new(snap_path).parent().unwrap_or(std::path::Path::new("."))).ok();
            std::fs::write(snap_path, &self.value).map_err(|e| format!("could not write snapshot {snap_path}: {e}"))?;
            return Ok(());
        }
        match std::fs::read_to_string(snap_path) {
            Ok(expected) => {
                if expected == self.value {
                    Ok(())
                } else {
                    Err(format!("snapshot mismatch at {snap_path}\n  expected: {}\n  got:      {}", expected.trim(), self.value.trim()))
                }
            }
            Err(_) => {
                Err(format!("missing snapshot {snap_path}; run `jet test --update-snapshots` to create it"))
            }
        }
    }
}
"#;
/// D-TEST1 (ratified 2026-06-22, option B): property-test runtime. Emitted into
/// the `jet test` harness only when the file declares a parameterized `#Test fn`.
/// Std-only (I6): a deterministic splitmix64 PRNG, a `JetGen` trait that
/// generates and shrinks values per type, and the driver loop that runs N cases
/// and minimizes a failing input. The seed defaults to a fixed constant so a
/// failure reproduces; `JET_PROP_SEED=<n>` overrides it.
const PROP_PRELUDE: &str = r#"
struct JetRng { s: u64 }
impl JetRng {
    fn new(seed: u64) -> JetRng { JetRng { s: seed } }
    fn next_u64(&mut self) -> u64 {
        self.s = self.s.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 { if n == 0 { 0 } else { self.next_u64() % n } }
    fn coin(&mut self) -> bool { self.next_u64() & 1 == 1 }
}
/// A type that the property runner can generate and shrink. `shrink` returns
/// progressively simpler candidates (closer to a minimal failing case); an empty
/// list means "already minimal".
trait JetGen: Sized + Clone {
    fn generate(rng: &mut JetRng) -> Self;
    fn shrink(&self) -> Vec<Self>;
    fn render(&self) -> String;
}
impl JetGen for i64 {
    fn generate(rng: &mut JetRng) -> i64 {
        // Bias toward small magnitudes (edge cases) but cover the full range.
        match rng.below(8) {
            0 => 0, 1 => 1, 2 => -1, 3 => i64::MAX, 4 => i64::MIN,
            _ => (rng.next_u64() as i64) % 1000,
        }
    }
    fn shrink(&self) -> Vec<i64> {
        if *self == 0 { return Vec::new(); }
        let mut v = vec![0i64];
        let half = *self / 2;
        if half != 0 && half != *self { v.push(half); }
        if *self > 0 { v.push(*self - 1); } else { v.push(*self + 1); }
        v
    }
    fn render(&self) -> String { format!("{}", self) }
}
impl JetGen for f64 {
    fn generate(rng: &mut JetRng) -> f64 {
        match rng.below(6) {
            0 => 0.0, 1 => 1.0, 2 => -1.0,
            _ => ((rng.next_u64() % 200000) as f64) / 1000.0 - 100.0,
        }
    }
    fn shrink(&self) -> Vec<f64> {
        if *self == 0.0 { return Vec::new(); }
        let mut v = vec![0.0f64];
        let half = *self / 2.0;
        if half != *self { v.push((half * 1000.0).round() / 1000.0); }
        v
    }
    fn render(&self) -> String { format!("{:?}", self) }
}
impl JetGen for f32 {
    fn generate(rng: &mut JetRng) -> f32 { f64::generate(rng) as f32 }
    fn shrink(&self) -> Vec<f32> { (*self as f64).shrink().into_iter().map(|x| x as f32).collect() }
    fn render(&self) -> String { format!("{:?}", self) }
}
impl JetGen for bool {
    fn generate(rng: &mut JetRng) -> bool { rng.coin() }
    fn shrink(&self) -> Vec<bool> { if *self { vec![false] } else { Vec::new() } }
    fn render(&self) -> String { format!("{}", self) }
}
impl JetGen for char {
    fn generate(rng: &mut JetRng) -> char {
        let printable = b" abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        printable[(rng.below(printable.len() as u64)) as usize] as char
    }
    fn shrink(&self) -> Vec<char> { if *self == 'a' { Vec::new() } else { vec!['a'] } }
    fn render(&self) -> String { format!("{:?}", self) }
}
impl JetGen for String {
    fn generate(rng: &mut JetRng) -> String {
        let len = rng.below(8) as usize;
        (0..len).map(|_| char::generate(rng)).collect()
    }
    fn shrink(&self) -> Vec<String> {
        if self.is_empty() { return Vec::new(); }
        let mut v = vec![String::new()];
        let chars: Vec<char> = self.chars().collect();
        let half = chars.len() / 2;
        if half > 0 && half < chars.len() { v.push(chars[..half].iter().collect()); }
        if chars.len() > 1 { v.push(chars[..chars.len() - 1].iter().collect()); }
        v
    }
    fn render(&self) -> String { format!("{:?}", self) }
}
impl<T: JetGen> JetGen for Option<T> {
    fn generate(rng: &mut JetRng) -> Option<T> {
        if rng.below(4) == 0 { None } else { Some(T::generate(rng)) }
    }
    fn shrink(&self) -> Vec<Option<T>> {
        match self {
            None => Vec::new(),
            Some(x) => {
                let mut v = vec![None];
                for s in x.shrink() { v.push(Some(s)); }
                v
            }
        }
    }
    fn render(&self) -> String {
        match self { None => "none".to_string(), Some(x) => format!("{}", x.render()) }
    }
}
impl<T: JetGen> JetGen for Vec<T> {
    fn generate(rng: &mut JetRng) -> Vec<T> {
        let len = rng.below(8) as usize;
        (0..len).map(|_| T::generate(rng)).collect()
    }
    fn shrink(&self) -> Vec<Vec<T>> {
        if self.is_empty() { return Vec::new(); }
        let mut v: Vec<Vec<T>> = vec![Vec::new()];
        // Drop the first element, then the last, then shrink the first element.
        if self.len() > 1 {
            v.push(self[1..].to_vec());
            v.push(self[..self.len() - 1].to_vec());
        }
        if let Some(first) = self.first() {
            for s in first.shrink() {
                let mut c = self.clone();
                c[0] = s;
                v.push(c);
            }
        }
        v
    }
    fn render(&self) -> String {
        let parts: Vec<String> = self.iter().map(|x| x.render()).collect();
        format!("[{}]", parts.join(", "))
    }
}
fn jet_prop_seed() -> u64 {
    std::env::var("JET_PROP_SEED").ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0x5EED_1234_ABCD_0001)
}
"#;
/// D-COV1 (`jet test --coverage`): the coverage recorder. Emitted only when the
/// harness is built with `--coverage`. `jet_cov(line)` records that the function
/// starting at `line` ran; on exit the hit set is written to the path in
/// `JET_COV_OUT` so the `jet test` driver can report per-function/per-line
/// coverage. Std-only (I6): a `Mutex<HashSet>` plus an exit guard.
const COV_PRELUDE: &str = r#"
use std::sync::Mutex;
use std::collections::BTreeSet;
static JET_COV_HITS: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());
fn jet_cov(line: usize) {
    if let Ok(mut s) = JET_COV_HITS.lock() { s.insert(line); }
}
fn jet_cov_dump() {
    if let Ok(path) = std::env::var("JET_COV_OUT") {
        if let Ok(s) = JET_COV_HITS.lock() {
            let lines: Vec<String> = s.iter().map(|l| l.to_string()).collect();
            let _ = std::fs::write(path, lines.join("\n"));
        }
    }
}
"#;
const CORELIB_PRELUDE: &str = include_str!("../Prelude/CoreLib.rs");
/// D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): allocator runtime helpers.
const MEM_PRELUDE: &str = include_str!("../Prelude/Mem.rs");

/// D-ALLOC2: the `jet_mem` arena helper carries the one vetted lifetime-extension
/// `unsafe` (D-LL1). It is part of the always-emitted prelude, but a program that
/// never touches `core.mem` allocators must not carry any `unsafe` at all (I1 —
/// golden/closures/regex/… tests assert zero `unsafe` in such output). So strip
/// the `mod jet_mem { … }` block whenever nothing references `jet_mem::`.
fn strip_unused_mem_prelude(out: String) -> String {
    let Some(start) = out.find("mod jet_mem") else {
        return out;
    };
    // Brace-match the module body to find its end.
    let bytes = out.as_bytes();
    let mut depth = 0usize;
    let mut seen = false;
    let mut end = out.len();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => { depth += 1; seen = true; }
            b'}' => {
                depth -= 1;
                if seen && depth == 0 { end = i + 1; break; }
            }
            _ => {}
        }
        i += 1;
    }
    // Referenced anywhere outside its own definition? Keep it.
    let used = out[..start].contains("jet_mem::") || out[end..].contains("jet_mem::");
    if used {
        return out;
    }
    let mut s = out[..start].to_string();
    // Drop a trailing blank line left by the removed block.
    let rest = out[end..].trim_start_matches('\n');
    s.push_str(rest);
    s
}

/// D-TXN-ROLLBACK layer 1: the `jet_txn` module carries the auto-snapshot
/// restore mechanism, whose Drop-backed writeback uses one vetted raw-pointer
/// deref (sound: the transaction guard outlives nothing it points at). A program
/// that never auto-snapshots a `#Transact` value must carry no `unsafe`, so strip
/// `mod jet_txn { … }` whenever nothing references `jet_txn::` — exactly like
/// `strip_unused_mem_prelude`.
fn strip_unused_txn_prelude(out: String) -> String {
    let Some(start) = out.find("mod jet_txn") else {
        return out;
    };
    let bytes = out.as_bytes();
    let mut depth = 0usize;
    let mut seen = false;
    let mut end = out.len();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => { depth += 1; seen = true; }
            b'}' => {
                depth -= 1;
                if seen && depth == 0 { end = i + 1; break; }
            }
            _ => {}
        }
        i += 1;
    }
    let used = out[..start].contains("jet_txn::") || out[end..].contains("jet_txn::");
    if used {
        return out;
    }
    let mut s = out[..start].to_string();
    let rest = out[end..].trim_start_matches('\n');
    s.push_str(rest);
    s
}

/// D-TERM1: the `jet_term_unix` and `jet_term_windows` platform modules each
/// carry vetted `unsafe` for terminal I/O FFI. Strip them (along with the
/// `jet_term_enter`/`jet_term_leave`/`jet_term_read_key` dispatch functions and
/// the `JetKey` type) when no `live { … }` block or `core.term` call is present
/// in the generated user code — i.e., when `jet_term_enter` is never called.
fn strip_unused_term_prelude(out: String) -> String {
    // Fast path: if the term dispatchers are referenced in user code (after the
    // prelude), keep the whole term section.
    let prelude_end = out.find("fn main()").unwrap_or(out.len());
    let user_code = &out[prelude_end..];
    if user_code.contains("jet_term_enter") || user_code.contains("jet_term_read_key") {
        return out;
    }
    // The term prelude is one contiguous block: the `#[cfg(unix)]` line above
    // `mod jet_term_unix` through the end of `fn jet_term_read_key` (the two
    // platform modules plus the enter/leave/read_key dispatchers that call into
    // them). Excise it as a single span — stripping only the `mod` blocks would
    // leave the dispatchers referencing now-missing modules (I2: E0433).
    let Some(unix_mod) = out.find("mod jet_term_unix {") else { return out; };
    let block_start = out[..unix_mod].rfind("#[cfg(unix)]").unwrap_or(unix_mod);
    let Some(read_key) = out.find("fn jet_term_read_key") else { return out; };
    let bytes = out.as_bytes();
    let (mut depth, mut seen, mut i) = (0usize, false, read_key);
    let mut end = out.len();
    while i < bytes.len() {
        match bytes[i] {
            b'{' => { depth += 1; seen = true; }
            b'}' => {
                depth -= 1;
                if seen && depth == 0 { end = i + 1; break; }
            }
            _ => {}
        }
        i += 1;
    }
    let mut s = out[..block_start].to_string();
    s.push_str(out[end..].trim_start_matches('\n'));
    s
}

pub(crate) fn mangle(name: &str) -> String {
    if name == "main" {
        "main".to_string()
    } else {
        format!("user_{}", name)
    }
}

/// D-TXN-ROLLBACK layer 2: emit the `trait user_Rollback { … }` Rust trait
/// declaration when any impl block in the program references `Rollback`. Programs
/// with no `Rollback` impl produce zero output here (byte-identical to before).
pub(crate) fn emit_synthetic_rollback_trait(out: &mut String) {
    out.push_str("pub trait user_Rollback {\n");
    out.push_str("    type Snapshot;\n");
    out.push_str("    fn snapshot(&self) -> Self::Snapshot;\n");
    out.push_str("    fn restore(&mut self, _snap: Self::Snapshot);\n");
    out.push_str("}\n\n");
}

pub(crate) fn program_has_rollback_impl(items: &[Item]) -> bool {
    items.iter().any(|i| match i {
        Item::Impl(im) => im.trait_name.as_deref() == Some(Syntax::TRAIT_ROLLBACK),
        Item::Struct(s) => s.trait_impls.iter().any(|b| b.trait_name == Syntax::TRAIT_ROLLBACK),
        Item::Enum(e) => e.trait_impls.iter().any(|b| b.trait_name == Syntax::TRAIT_ROLLBACK),
        _ => false,
    })
}

pub fn emit(prog: &Program, src: &str, file: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} — do not edit. Edit the .{} source instead.\n",
        Syntax::BINARY_NAME,
        Syntax::FILE_EXT
    ));
    out.push_str(&format!(
        "// If rustc rejects this file, that is a bug in {} (invariant I2).\n",
        Syntax::BINARY_NAME
    ));
    // E2-M12 D-OBS1: source-map marker lets tooling resolve generated Rust back to
    // the originating Jet file. Panic reports carry Jet file+line directly via
    // jet_panic/jet_panic_rich, so runtime error messages already show Jet terms.
    out.push_str(&format!("// jet:source-map source={}\n", file));
    out.push_str("#![allow(warnings)]\n\n");
    out.push_str(PRELUDE);
    out.push_str(MEM_PRELUDE);
    out.push('\n');

    let cx = build_cx(prog, src, file);
    let tuple_shapes = collect_tuple_shapes(&prog.items);
    emit_tuple_structs(&cx, &tuple_shapes, &mut out);

    // D-TXN-ROLLBACK layer 2: emit the synthetic Rollback trait iff needed.
    if program_has_rollback_impl(&prog.items) {
        emit_synthetic_rollback_trait(&mut out);
    }

    for item in &prog.items {
        match item {
            Item::Trait(t) => Traits::emit_trait_def(t, &mut out),
            Item::Struct(s) => emit_struct(&cx, s, &mut out),
            Item::Enum(e) => emit_enum(&cx, e, &mut out),
            Item::Distinct(d) => emit_distinct(&cx, d, &mut out),
            // D-QUAL3: emit one distinct newtype per unit-family member.
            Item::UnitFamily(uf) => {
                for d in uf.distinct_defs() {
                    emit_distinct(&cx, &d, &mut out);
                }
            }
            Item::Const(c) => emit_const(c, &mut out),
            Item::CModule(cm) => emit_c_module(cm, &mut out),
            Item::Func(_) | Item::Impl(_) | Item::Test(_) | Item::Bench(_) | Item::ExternRust(_)
            | Item::Module(_) | Item::CodeModule(_) | Item::ErrorConv(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::Migration(_) => {} // D-MIGRATE1: migration is sema-only (I3)
        }
    }

    for item in &prog.items {
        match item {
            Item::Struct(s) => {
                emit_type_impl(&cx, &s.name, &s.type_params, &s.methods, &mut out);
                for block in &s.trait_impls {
                    emit_trait_impl(&cx, &s.name, &s.type_params, block, &mut out);
                }
            }
            Item::Enum(e) => {
                emit_type_impl(&cx, &e.name, &e.type_params, &e.methods, &mut out);
                for block in &e.trait_impls {
                    emit_trait_impl(&cx, &e.name, &e.type_params, block, &mut out);
                }
            }
            Item::Impl(i) => {
                if i.trait_name.is_some() {
                    emit_external_trait_impl(&cx, i, &mut out);
                } else {
                    emit_type_impl(&cx, &i.type_name, &[], &i.methods, &mut out);
                }
            }
            // D-ERR-CONV: emit a standalone Rust function for each declared conversion.
            Item::ErrorConv(ec) => {
                emit_error_conv(&cx, ec, &mut out);
            }
            _ => {}
        }
    }

    for item in &prog.items {
        if let Item::Func(f) = item {
            emit_func(&cx, f, &mut out);
        }
    }
    strip_unused_term_prelude(strip_unused_txn_prelude(strip_unused_mem_prelude(out)))
}

/// Emit a test harness binary: all definitions plus one `main` that runs
/// every `#Test "…" { }` block (M6 phase 2).
pub fn emit_tests(prog: &Program, src: &str, file: &str) -> String {
    let tests: Vec<&TestDef> = prog
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Test(t) => Some(t),
            _ => None,
        })
        .collect();
    assert!(!tests.is_empty(), "emit_tests called with no test blocks");

    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} test harness — do not edit.\n",
        Syntax::BINARY_NAME
    ));
    out.push_str("#![allow(warnings)]\n\n");
    out.push_str(PRELUDE);
    out.push_str(MEM_PRELUDE);
    out.push_str(TEST_PRELUDE);
    if any_property_test(&tests) {
        out.push_str(PROP_PRELUDE);
    }
    out.push('\n');

    let mut cx = build_cx(prog, src, file);
    cx.test_mode = true;
    let tuple_shapes = collect_tuple_shapes(&prog.items);
    emit_tuple_structs(&cx, &tuple_shapes, &mut out);

    // D-TXN-ROLLBACK layer 2: emit the synthetic Rollback trait iff needed.
    if program_has_rollback_impl(&prog.items) {
        emit_synthetic_rollback_trait(&mut out);
    }

    for item in &prog.items {
        match item {
            Item::Trait(t) => Traits::emit_trait_def(t, &mut out),
            Item::Struct(s) => emit_struct(&cx, s, &mut out),
            Item::Enum(e) => emit_enum(&cx, e, &mut out),
            Item::Distinct(d) => emit_distinct(&cx, d, &mut out),
            // D-QUAL3: emit one distinct newtype per unit-family member.
            Item::UnitFamily(uf) => {
                for d in uf.distinct_defs() {
                    emit_distinct(&cx, &d, &mut out);
                }
            }
            Item::Const(c) => emit_const(c, &mut out),
            Item::CModule(cm) => emit_c_module(cm, &mut out),
            Item::Func(_) | Item::Impl(_) | Item::Test(_) | Item::Bench(_) | Item::ExternRust(_)
            | Item::Module(_) | Item::CodeModule(_) | Item::ErrorConv(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::Migration(_) => {} // D-MIGRATE1: migration is sema-only (I3)
        }
    }

    for item in &prog.items {
        match item {
            Item::Struct(s) => {
                emit_type_impl(&cx, &s.name, &s.type_params, &s.methods, &mut out);
                for block in &s.trait_impls {
                    emit_trait_impl(&cx, &s.name, &s.type_params, block, &mut out);
                }
            }
            Item::Enum(e) => {
                emit_type_impl(&cx, &e.name, &e.type_params, &e.methods, &mut out);
                for block in &e.trait_impls {
                    emit_trait_impl(&cx, &e.name, &e.type_params, block, &mut out);
                }
            }
            Item::Impl(i) => {
                if i.trait_name.is_some() {
                    emit_external_trait_impl(&cx, i, &mut out);
                } else {
                    emit_type_impl(&cx, &i.type_name, &[], &i.methods, &mut out);
                }
            }
            Item::ErrorConv(ec) => {
                emit_error_conv(&cx, ec, &mut out);
            }
            _ => {}
        }
    }

    for item in &prog.items {
        if let Item::Func(f) = item {
            if f.name != "main" {
                emit_func(&cx, f, &mut out);
            }
        }
    }

    emit_test_fns(&cx, &tests, &mut out);
    emit_test_main(&tests, &mut out);
    strip_unused_term_prelude(strip_unused_txn_prelude(strip_unused_mem_prelude(out)))
}

/// D-TEST1/S43: the shared reporting `main` for a `jet test` harness. Each test
/// (unit or property) is invoked through its `jet_test_N()` entry; the loop is
/// identical whichever kind it is.
fn emit_test_main(tests: &[&TestDef], out: &mut String) {
    emit_test_main_cov(tests, out, false)
}

fn emit_test_main_cov(tests: &[&TestDef], out: &mut String, coverage: bool) {
    out.push_str("fn main() {\n");
    out.push_str("    let mut passed = 0usize;\n");
    out.push_str("    let mut failed = 0usize;\n");
    for (i, test) in tests.iter().enumerate() {
        let name = escape_rust_str(&test.name);
        out.push_str(&format!("    match jet_test_{}() {{\n", i));
        out.push_str("        Ok(()) => {\n");
        out.push_str(&format!(
            "            println!(\"{{}}: pass\", {});\n",
            name
        ));
        out.push_str("            passed += 1;\n");
        out.push_str("        }\n");
        out.push_str("        Err(msg) => {\n");
        out.push_str(&format!(
            "            println!(\"{{}}: FAIL\", {});\n",
            name
        ));
        out.push_str("            eprintln!(\"  {}\", msg);\n");
        out.push_str("            failed += 1;\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
    }
    out.push_str("    println!(\"{} passed, {} failed\", passed, failed);\n");
    if coverage {
        // D-COV1: write the hit set before any `exit` (which would skip Drop).
        out.push_str("    jet_cov_dump();\n");
    }
    out.push_str("    if failed > 0 { std::process::exit(1); }\n");
    out.push_str("}\n");
}

/// Does any test in the set declare property parameters (D-TEST1)? Drives whether
/// the harness needs `PROP_PRELUDE`.
fn any_property_test(tests: &[&TestDef]) -> bool {
    tests.iter().any(|t| !t.params.is_empty())
}

/// D-TEST1: emit the per-test functions. A unit test (`#Test "name" { … }`, no
/// params) becomes `fn jet_test_N() -> Result<(), String>` exactly as before. A
/// property test (`#Test fn name(p…) { … }`) becomes a body fn `jet_prop_N(p…)`
/// plus a driver `jet_test_N()` that generates inputs, runs cases, and shrinks
/// the first failure to a minimal counterexample. Either way `jet_test_N()` is
/// the single entry the main loop calls, so the reporting loop is shared.
fn emit_test_fns(cx: &Cx, tests: &[&TestDef], out: &mut String) {
    const CASES: usize = 200;
    const SHRINK_STEPS: usize = 2000;
    for (i, test) in tests.iter().enumerate() {
        if test.params.is_empty() {
            out.push_str(&format!("fn jet_test_{}() -> Result<(), String> {{\n", i));
            emit_test_body(cx, &test.body, out);
            out.push_str("    Ok(())\n");
            out.push_str("}\n\n");
            continue;
        }
        // Property body: takes each generated input by value, returns the body's
        // Result. Param Rust types come from `cx.rust_type` so the signature
        // matches what the body expects.
        let sig: Vec<String> = test
            .params
            .iter()
            .map(|p| format!("{}: {}", mangle(&p.name), cx.rust_type(&p.ty)))
            .collect();
        out.push_str(&format!(
            "fn jet_prop_{}({}) -> Result<(), String> {{\n",
            i,
            sig.join(", ")
        ));
        TIR::emit_tir_property_test_body(&test.body, &test.params, cx, out);
        out.push_str("    Ok(())\n");
        out.push_str("}\n\n");

        // Driver: generate a tuple of inputs per case; on the first failing case,
        // shrink each component greedily while it still fails, then report the
        // minimal counterexample plus the assertion message.
        let n = test.params.len();
        let types: Vec<String> = test.params.iter().map(|p| cx.rust_type(&p.ty)).collect();
        let tuple_ty = format!("({},)", types.join(", "));
        out.push_str(&format!("fn jet_test_{}() -> Result<(), String> {{\n", i));
        out.push_str("    let mut rng = JetRng::new(jet_prop_seed());\n");
        // call helper that takes the tuple, returns Result
        let call_args: Vec<String> = (0..n).map(|k| format!("input.{}.clone()", k)).collect();
        out.push_str(&format!(
            "    let run = |input: &{}| -> Result<(), String> {{ jet_prop_{}({}) }};\n",
            tuple_ty, i, call_args.join(", ")
        ));
        out.push_str(&format!("    for _ in 0..{} {{\n", CASES));
        let gen_components: Vec<String> = types
            .iter()
            .map(|t| format!("<{} as JetGen>::generate(&mut rng)", t))
            .collect();
        out.push_str(&format!(
            "        let mut input: {} = ({},);\n",
            tuple_ty,
            gen_components.join(", ")
        ));
        out.push_str("        if let Err(first_msg) = run(&input) {\n");
        out.push_str("            let mut msg = first_msg;\n");
        out.push_str("            let mut improved = true;\n");
        out.push_str("            let mut steps = 0usize;\n");
        out.push_str(&format!(
            "            while improved && steps < {} {{\n",
            SHRINK_STEPS
        ));
        out.push_str("                improved = false;\n");
        for k in 0..n {
            out.push_str(&format!(
                "                for cand in input.{}.shrink() {{\n",
                k
            ));
            out.push_str("                    steps += 1;\n");
            out.push_str("                    let mut trial = input.clone();\n");
            out.push_str(&format!("                    trial.{} = cand;\n", k));
            out.push_str("                    if let Err(m) = run(&trial) {\n");
            out.push_str("                        input = trial; msg = m; improved = true; break;\n");
            out.push_str("                    }\n");
            out.push_str("                }\n");
        }
        out.push_str("            }\n");
        // Render the minimized counterexample as `name = value` pairs.
        let renders: Vec<String> = test
            .params
            .iter()
            .enumerate()
            .map(|(k, p)| format!("format!(\"{} = {{}}\", input.{}.render())", p.name, k))
            .collect();
        out.push_str(&format!(
            "            let args = vec![{}];\n",
            renders.join(", ")
        ));
        out.push_str("            return Err(format!(\"property failed for {}\\n  {}\", args.join(\", \"), msg));\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
        out.push_str("    Ok(())\n");
        out.push_str("}\n\n");
    }
}

/// c109: emit a `#Test` block body through the TIR (R7 — the only codegen seam). A test
/// body is a bare statement list (no params, unit context), emitted at indent 1 inside the
/// `fn jet_test_N()` wrapper the caller opened. A gate-miss is an internal compiler error
/// (I2-class), never an AST fallback — every `#Test` body routes through the TIR.
fn emit_test_body(cx: &Cx, body: &[crate::AST::Stmt], out: &mut String) {
    if TIR::tir_covers_test_body(body, cx) {
        TIR::emit_tir_test_body(body, cx, out);
        return;
    }
    panic!(
        "internal compiler error: codegen reached a #Test body construct the typed IR does not cover — compiler bug (I2/R7)"
    );
}

pub fn emit_bundle(bundle: &ProgramBundle, _mode: CompileMode, link: Option<&FfiLink>) -> String {
    let entry = &bundle.modules[bundle.entry];
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} — do not edit. Edit the .{} source instead.\n",
        Syntax::BINARY_NAME,
        Syntax::FILE_EXT
    ));
    // E2-M12 D-OBS1: source-map marker for tooling and debuggers.
    out.push_str(&format!("// jet:source-map source={}\n", entry.display));
    out.push_str("#![allow(warnings)]\n\n");
    if let Some(ffi) = link {
        out.push_str(&format!("extern crate {};\n\n", ffi.crate_name));
    }
    out.push_str(PRELUDE);
    out.push_str(MEM_PRELUDE);
    if !bundle.used_core.is_empty() {
        out.push_str(CORELIB_PRELUDE);
    }
    out.push('\n');

    let import_mods = import_mod_map(bundle, bundle.entry);
    let extern_funcs = bundle_extern_funcs(bundle);

    for (i, module) in bundle.modules.iter().enumerate() {
        if i == bundle.entry {
            continue;
        }
        let ns = module.alias.clone();
        out.push_str(&format!("mod user_{ns} {{\n"));
        out.push_str(MOD_USE);
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        cx.import_mods = import_mod_map(bundle, i);
        cx.foreign_types = foreign_type_map(bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        cx.reexport_calls = reexport_call_map(bundle, i);
        cx.import_sigs = import_sig_map(bundle, i);
        cx.import_rets = import_ret_map(bundle, i);
        cx.core_imports = core_import_map(bundle, i);
        cx.used_core = bundle.used_core.clone();
        cx.root_prefix = "super::".to_string();
        let (uinline, ufile) = unqualified_import_maps(bundle, i);
        cx.unqualified_inline = uinline;
        cx.unqualified_file = ufile;
        emit_program_items(&cx, &module.items, &mut out, true);
        out.push_str("}\n\n");
    }

    let mut cx = build_cx_items(
        &entry.items,
        &entry.source,
        &entry.display,
        link,
        &extern_funcs,
    );
    cx.import_mods = import_mods;
    cx.foreign_types = foreign_type_map(bundle, bundle.entry);
    register_foreign_enum_variants(&mut cx, bundle, bundle.entry);
    update_cloneability_with_foreign_types(&mut cx, &entry.items);
    cx.reexport_calls = reexport_call_map(bundle, bundle.entry);
    cx.import_sigs = import_sig_map(bundle, bundle.entry);
    cx.import_rets = import_ret_map(bundle, bundle.entry);
    cx.core_imports = core_import_map(bundle, bundle.entry);
    cx.used_core = bundle.used_core.clone();
    let (uinline, ufile) = unqualified_import_maps(bundle, bundle.entry);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
    emit_program_items(&cx, &entry.items, &mut out, true);
    strip_unused_term_prelude(strip_unused_txn_prelude(strip_unused_mem_prelude(out)))
}

pub fn emit_bundle_tests(bundle: &ProgramBundle, link: Option<&FfiLink>) -> String {
    emit_bundle_tests_cov(bundle, link, false)
}

/// D-COV1: emit the `jet test` harness, optionally with coverage instrumentation.
/// `coverage = false` is byte-identical to the historical `emit_bundle_tests`
/// (golden tests rely on this), so the probes/prelude only appear under
/// `jet test --coverage`.
pub fn emit_bundle_tests_cov(bundle: &ProgramBundle, link: Option<&FfiLink>, coverage: bool) -> String {
    let entry = &bundle.modules[bundle.entry];
    let tests: Vec<&TestDef> = entry
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Test(t) => Some(t),
            _ => None,
        })
        .collect();
    assert!(
        !tests.is_empty(),
        "emit_bundle_tests called with no test blocks"
    );
    let want_prop_prelude = any_property_test(&tests);

    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} test harness — do not edit.\n",
        Syntax::BINARY_NAME
    ));
    out.push_str("#![allow(warnings)]\n\n");
    if let Some(ffi) = link {
        out.push_str(&format!("extern crate {};\n\n", ffi.crate_name));
    }
    out.push_str(PRELUDE);
    out.push_str(MEM_PRELUDE);
    out.push_str(TEST_PRELUDE);
    if want_prop_prelude {
        out.push_str(PROP_PRELUDE);
    }
    if coverage {
        out.push_str(COV_PRELUDE);
    }
    if !bundle.used_core.is_empty() {
        out.push_str(CORELIB_PRELUDE);
    }
    out.push('\n');

    let import_mods = import_mod_map(bundle, bundle.entry);
    let extern_funcs = bundle_extern_funcs(bundle);

    for (i, module) in bundle.modules.iter().enumerate() {
        if i == bundle.entry {
            continue;
        }
        let ns = module.alias.clone();
        out.push_str(&format!("mod user_{ns} {{\n"));
        out.push_str(MOD_USE);
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        cx.test_mode = true;
        cx.coverage = coverage;
        cx.import_mods = import_mod_map(bundle, i);
        cx.foreign_types = foreign_type_map(bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        cx.reexport_calls = reexport_call_map(bundle, i);
        cx.import_sigs = import_sig_map(bundle, i);
        cx.import_rets = import_ret_map(bundle, i);
        cx.core_imports = core_import_map(bundle, i);
        cx.used_core = bundle.used_core.clone();
        cx.root_prefix = "super::".to_string();
        let (uinline, ufile) = unqualified_import_maps(bundle, i);
        cx.unqualified_inline = uinline;
        cx.unqualified_file = ufile;
        emit_program_items(&cx, &module.items, &mut out, false);
        out.push_str("}\n\n");
    }

    let mut cx = build_cx_items(
        &entry.items,
        &entry.source,
        &entry.display,
        link,
        &extern_funcs,
    );
    cx.test_mode = true;
    cx.coverage = coverage;
    cx.import_mods = import_mods;
    cx.foreign_types = foreign_type_map(bundle, bundle.entry);
    register_foreign_enum_variants(&mut cx, bundle, bundle.entry);
    update_cloneability_with_foreign_types(&mut cx, &entry.items);
    cx.reexport_calls = reexport_call_map(bundle, bundle.entry);
    cx.import_sigs = import_sig_map(bundle, bundle.entry);
    cx.import_rets = import_ret_map(bundle, bundle.entry);
    cx.core_imports = core_import_map(bundle, bundle.entry);
    cx.used_core = bundle.used_core.clone();
    let (uinline, ufile) = unqualified_import_maps(bundle, bundle.entry);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
    emit_program_items(&cx, &entry.items, &mut out, false);

    emit_test_fns(&cx, &tests, &mut out);
    emit_test_main_cov(&tests, &mut out, coverage);
    strip_unused_term_prelude(strip_unused_txn_prelude(strip_unused_mem_prelude(out)))
}

/// D-BENCH1: emit a benchmark harness binary — every definition plus a `main`
/// that times each `#Bench "…" { }` region and reports ns/iter + ops/sec.
/// Mirrors `emit_bundle_tests`; the only divergence is the per-block tail,
/// which wraps each body in an auto-scaled timed loop instead of a pass/fail
/// check. Each body is emitted exactly like a `#Test` body (a bare statement
/// list in a `Result<(), String>` fn), so `return Err(…)` from `require` stays
/// valid; the timing wrapper ignores that result.
pub fn emit_bundle_benches(bundle: &ProgramBundle, link: Option<&FfiLink>) -> String {
    let entry = &bundle.modules[bundle.entry];
    let benches: Vec<&BenchDef> = entry
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Bench(b) => Some(b),
            _ => None,
        })
        .collect();
    assert!(
        !benches.is_empty(),
        "emit_bundle_benches called with no bench blocks"
    );
    // Benches never declare property params, so they never need the generator prelude.
    let want_prop_prelude = false;
    // D-COV1: coverage instrumentation is a `jet test` feature; benches don't use it.
    let coverage = false;

    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} bench harness — do not edit.\n",
        Syntax::BINARY_NAME
    ));
    out.push_str("#![allow(warnings)]\n\n");
    if let Some(ffi) = link {
        out.push_str(&format!("extern crate {};\n\n", ffi.crate_name));
    }
    out.push_str(PRELUDE);
    out.push_str(MEM_PRELUDE);
    out.push_str(TEST_PRELUDE);
    if want_prop_prelude {
        out.push_str(PROP_PRELUDE);
    }
    if coverage {
        out.push_str(COV_PRELUDE);
    }
    if !bundle.used_core.is_empty() {
        out.push_str(CORELIB_PRELUDE);
    }
    out.push('\n');

    let import_mods = import_mod_map(bundle, bundle.entry);
    let extern_funcs = bundle_extern_funcs(bundle);

    for (i, module) in bundle.modules.iter().enumerate() {
        if i == bundle.entry {
            continue;
        }
        let ns = module.alias.clone();
        out.push_str(&format!("mod user_{ns} {{\n"));
        out.push_str(MOD_USE);
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        cx.test_mode = true;
        cx.coverage = coverage;
        cx.import_mods = import_mod_map(bundle, i);
        cx.foreign_types = foreign_type_map(bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        cx.reexport_calls = reexport_call_map(bundle, i);
        cx.import_sigs = import_sig_map(bundle, i);
        cx.import_rets = import_ret_map(bundle, i);
        cx.core_imports = core_import_map(bundle, i);
        cx.used_core = bundle.used_core.clone();
        cx.root_prefix = "super::".to_string();
        let (uinline, ufile) = unqualified_import_maps(bundle, i);
        cx.unqualified_inline = uinline;
        cx.unqualified_file = ufile;
        emit_program_items(&cx, &module.items, &mut out, false);
        out.push_str("}\n\n");
    }

    let mut cx = build_cx_items(
        &entry.items,
        &entry.source,
        &entry.display,
        link,
        &extern_funcs,
    );
    cx.test_mode = true;
    cx.coverage = coverage;
    cx.import_mods = import_mods;
    cx.foreign_types = foreign_type_map(bundle, bundle.entry);
    register_foreign_enum_variants(&mut cx, bundle, bundle.entry);
    update_cloneability_with_foreign_types(&mut cx, &entry.items);
    cx.reexport_calls = reexport_call_map(bundle, bundle.entry);
    cx.import_sigs = import_sig_map(bundle, bundle.entry);
    cx.import_rets = import_ret_map(bundle, bundle.entry);
    cx.core_imports = core_import_map(bundle, bundle.entry);
    cx.used_core = bundle.used_core.clone();
    let (uinline, ufile) = unqualified_import_maps(bundle, bundle.entry);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
    emit_program_items(&cx, &entry.items, &mut out, false);

    // One body fn + one timing wrapper per bench. The body fn is shaped exactly
    // like a test fn (so `require`'s `return Err(…)` compiles); the wrapper
    // auto-scales the iteration count until a batch lasts >= 1ms, then collects
    // 10 per-iteration samples and returns (mean_ns, stddev_ns, iters).
    for (i, bench) in benches.iter().enumerate() {
        out.push_str(&format!(
            "fn jet_bench_body_{}() -> Result<(), String> {{\n",
            i
        ));
        emit_test_body(&cx, &bench.body, &mut out);
        out.push_str("    Ok(())\n");
        out.push_str("}\n\n");

        out.push_str(&format!("fn jet_bench_{}() -> (f64, f64, u64) {{\n", i));
        out.push_str("    let mut iters: u64 = 1;\n");
        out.push_str("    while iters < (1u64 << 30) {\n");
        out.push_str("        let t0 = std::time::Instant::now();\n");
        out.push_str(&format!(
            "        for _ in 0..iters {{ let _ = std::hint::black_box(jet_bench_body_{}()); }}\n",
            i
        ));
        out.push_str("        if t0.elapsed().as_millis() >= 1 { break; }\n");
        out.push_str("        iters = iters.saturating_mul(2);\n");
        out.push_str("    }\n");
        out.push_str("    let mut samples: Vec<f64> = Vec::new();\n");
        out.push_str("    for _ in 0..10 {\n");
        out.push_str("        let t0 = std::time::Instant::now();\n");
        out.push_str(&format!(
            "        for _ in 0..iters {{ let _ = std::hint::black_box(jet_bench_body_{}()); }}\n",
            i
        ));
        out.push_str("        samples.push(t0.elapsed().as_nanos() as f64 / iters as f64);\n");
        out.push_str("    }\n");
        out.push_str("    let n = samples.len() as f64;\n");
        out.push_str("    let mean = samples.iter().sum::<f64>() / n;\n");
        out.push_str(
            "    let var = samples.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n;\n",
        );
        out.push_str("    (mean, var.sqrt(), iters)\n");
        out.push_str("}\n\n");
    }

    out.push_str("fn main() {\n");
    for (i, bench) in benches.iter().enumerate() {
        let name = escape_rust_str(&bench.name);
        out.push_str(&format!(
            "    {{\n        let (mean, sd, _it) = jet_bench_{}();\n",
            i
        ));
        out.push_str("        let ops = if mean > 0.0 { 1.0e9 / mean } else { 0.0 };\n");
        out.push_str(&format!(
            "        println!(\"{{}}  {{:.1}} ns/iter (\\u{{00b1}}{{:.1}})  {{:.0}} ops/sec\", {}, mean, sd, ops);\n",
            name
        ));
        out.push_str("    }\n");
    }
    out.push_str("}\n");
    strip_unused_term_prelude(strip_unused_txn_prelude(strip_unused_mem_prelude(out)))
}
