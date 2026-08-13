//! Codegen is deliberately dumb (invariant I3): no checking happens here.
//! If a Program reaches this module, emission must always succeed and the
//! resulting Rust must always compile.
//!
//! Type-alignment rules (so emitted Rust always typechecks):
//!   - scalar params (Int/Float/Bool) pass by value, String by `&String`;
//!     `mut` params are `&mut T`; `take` params are `T` by value
//!   - a name bound to a `&T`/`&mut T` parameter is always emitted as the
//!     place `(*__jet_x)`, so every name has its plain Jet type
//!   - every printed/interpolated value goes through the `JetShow` trait
//!     in the prelude (Float keeps its decimal part there, S21)
//!   - every operator result is fully parenthesized

use crate::Sema::CompileMode;
use crate::Syntax;
use crate::Traits;
use crate::AST::FfiLink;
use crate::AST::{
    BenchDef, Expr, Func, Item, Program, ProgramBundle, ResolvedOutput, Stmt, TestDef,
    Type,
};
use std::collections::{HashMap, HashSet};

pub(crate) use jet_foundation::Names::{mangle, mangle_generated, mangle_path};

/// The generated-name prefix used by emitter format strings. Keeping this as
/// one allocator argument lets every emitted temporary share the same machine
/// lane without embedding a second spelling in an emitter template.
pub(crate) fn generated_prefix() -> String {
    mangle_generated("")
}

pub(crate) fn canonical_prefix() -> String {
    mangle("")
}

/// Non-identifier marker used by the web source-map protocol.
pub(crate) const SOURCE_MAP_MARKER: &str = concat!("//# __jet_", "source_map");

#[macro_export]
macro_rules! jet_generated_format {
    ($fmt:literal) => {
        ::std::format!($fmt, jet_prefix = $crate::Codegen::generated_prefix())
    };
    ($fmt:literal, $($args:tt)*) => {
        $crate::jet_generated_format!(@collect $fmt; [] $($args)*)
    };
    (@collect $fmt:literal; []) => {
        ::std::format!($fmt, jet_prefix = $crate::Codegen::generated_prefix())
    };
    (@collect $fmt:literal; [$($args:tt)*]) => {
        ::std::format!(
            $fmt,
            $($args)*,
            jet_prefix = $crate::Codegen::generated_prefix()
        )
    };
    (@collect $fmt:literal; [$($args:tt)*] ,) => {
        ::std::format!(
            $fmt,
            $($args)*,
            jet_prefix = $crate::Codegen::generated_prefix()
        )
    };
    (@collect $fmt:literal; [$($args:tt)*] , $head:tt $($rest:tt)*) => {
        $crate::jet_generated_format!(@collect $fmt; [$($args)*, $head] $($rest)*)
    };
    (@collect $fmt:literal; [$($args:tt)*] $head:tt $($rest:tt)*) => {
        $crate::jet_generated_format!(@collect $fmt; [$($args)* $head] $($rest)*)
    };
}

#[macro_export]
macro_rules! jet_name_format {
    ($fmt:literal) => {
        ::std::format!($fmt, name_prefix = $crate::Codegen::canonical_prefix())
    };
    ($fmt:literal, $($args:tt)*) => {
        $crate::jet_name_format!(@collect $fmt; [] $($args)*)
    };
    (@collect $fmt:literal; []) => {
        ::std::format!($fmt, name_prefix = $crate::Codegen::canonical_prefix())
    };
    (@collect $fmt:literal; [$($args:tt)*]) => {
        ::std::format!(
            $fmt,
            $($args)*,
            name_prefix = $crate::Codegen::canonical_prefix()
        )
    };
    (@collect $fmt:literal; [$($args:tt)*] ,) => {
        ::std::format!(
            $fmt,
            $($args)*,
            name_prefix = $crate::Codegen::canonical_prefix()
        )
    };
    (@collect $fmt:literal; [$($args:tt)*] , $head:tt $($rest:tt)*) => {
        $crate::jet_name_format!(@collect $fmt; [$($args)*, $head] $($rest)*)
    };
    (@collect $fmt:literal; [$($args:tt)*] $head:tt $($rest:tt)*) => {
        $crate::jet_name_format!(@collect $fmt; [$($args)* $head] $($rest)*)
    };
}

mod CModule;
mod Context;
mod Imports;
mod Items;
pub use crate::task_group;
pub mod Library;
pub mod Plugin;
mod Statement;
pub mod TIR;
mod Tuples;
mod Utils;
mod VariadicBound;
mod Web;

/// D-REPORT-TEST1=A: host-side `jet prove` uses the same source that test
/// harnesses receive below as their report prelude.
pub mod test_report {
    include!("../Prelude/TestReport.rs");
}

pub(crate) use CModule::*;
pub(crate) use Context::*;
pub(crate) use Imports::*;
pub(crate) use Items::*;
pub use Plugin::{emit_plugin, plugin_export_shape, PluginArtifacts, PluginScalar};
pub(crate) use Statement::*;
pub(crate) use Tuples::*;
pub(crate) use Utils::*;
pub use Library::{emit_library, library_export_shape, LibraryArtifacts, LibraryExport, LibraryScalar};
pub use Web::{
    build_wasm_jet_source_map, emit_web, validate_web_tir_support, WebArtifacts, WebTirUnsupported,
};

/// Build the interpreter's bundle-wide Core alias map from the same import
/// resolver used by AOT and JIT lowering. In particular, member-list imports
/// such as `use core.math.[abs, min]` must not fall back to a second policy.
pub fn core_imports_for_bundle(bundle: &ProgramBundle) -> HashMap<String, String> {
    let mut imports = HashMap::new();
    for module_idx in 0..bundle.modules.len() {
        for (alias, module) in core_import_map(bundle, module_idx) {
            imports.entry(alias).or_insert(module);
        }
    }
    imports
}

/// Emitted at the top of every program: core runtime helpers used by generated Rust.
/// Parts stay in source order so splitting ownership never changes generated bytes.
const PRELUDE_PARTS: &[&str] = &[
    // D-FAIL-CARRIER1=A: the one carrier under `T?` and `T ? E`. First, because
    // every other part builds outcomes on top of it.
    include_str!("../../../jet-foundation/src/Outcome.rs"),
    include_str!("../Prelude/Job.rs"),
    include_str!("../Prelude/Core/Option.rs"),
    include_str!("../Prelude/Core/FixedList.rs"),
    include_str!("../Prelude/Core/FloatProvenance.rs"),
    include_str!("../Prelude/Core/UnicodeString.rs"),
    // D-STR-CONCAT1: the owned String `+`/`+=` result is one kernel for every
    // execution tier; the evaluator and JIT include this same source.
    include_str!("../Prelude/Core/StringConcat.rs"),
    include_str!("../Prelude/Core/Loadable.rs"),
    include_str!("../Prelude/Core/Values.rs"),
    include_str!("../Prelude/Core/RangeBounds.rs"),
    include_str!("../Prelude/Core/Disjoint.rs"),
    include_str!("../Prelude/Core/ExpiringSecret.rs"),
    include_str!("../Prelude/Core/SetAlgebra.rs"),
    include_str!("../Prelude/Core/Duration.rs"),
    include_str!("../Prelude/Core/Measurement.rs"),
    include_str!("../Prelude/Core/TimeMonotonic.rs"),
    include_str!("../Prelude/Core/Time.rs"),
    include_str!("../Prelude/Core/Sketch.rs"),
    include_str!("../Prelude/Core/Contracts.rs"),
    include_str!("../Prelude/Core.rs"),
    include_str!("../Prelude/Core/ViewAccess.rs"),
    // D-EXPOP1=A / D-EXPSEM1=A: `^`. Shared verbatim with the wasm module
    // (Codegen/Web.rs) so every tier runs one power.
    include_str!("../Prelude/Core/Power.rs"),
    // D-FLOORDIV1=A: `/%`. Shared the same way, so every tier rounds down
    // identically.
    include_str!("../Prelude/Core/Division.rs"),
    include_str!("../Prelude/TypedText.rs"),
    include_str!("../Prelude/Core/Progress.rs"),
    include_str!("../Prelude/Core/ByteBuffer.rs"),
    include_str!("../Prelude/Core/Collections.rs"),
    include_str!("../Prelude/SharedProtocol.rs"),
    include_str!("../Prelude/Core/RuntimeControl.rs"),
    include_str!("../Prelude/NumericWiden.rs"),
    include_str!("../Prelude/Observe.rs"),
    include_str!("../../../jet-foundation/src/ExactUnitConversion.rs"),
    include_str!("../../../jet-foundation/src/StructuralDebug.rs"),
    // D-SHIFT1: `binary.Reader` / `text.Cursor`. Owned by jet-foundation so the
    // AOT prelude and the canonical TIR evaluator run one kernel (I9).
    include_str!("../../../jet-foundation/src/StreamCursor.rs"),
];

/// Native builders split this exact block into the content-addressed runtime
/// rlib. Keep the markers stable: emitted Rust remains a complete standalone
/// program, while the AOT link seam can replace the block with one `--extern`.
pub const CACHED_RUNTIME_BEGIN: &str = "// jet:cached-runtime-begin\n";
pub const CACHED_RUNTIME_END: &str = "// jet:cached-runtime-end\n";

fn push_prelude(out: &mut String) {
    for part in PRELUDE_PARTS {
        out.push_str(part);
    }
}

fn push_ffi_reporter(out: &mut String, link: Option<&FfiLink>) {
    let Some(link) = link else {
        out.push_str("fn jet_ffi_install_reporter() {}\n\n");
        return;
    };
    out.push_str(&format!(
        concat!(
            "// JET_VETTED_UNSAFE_BEGIN: ffi_reporter\n",
            "extern \"C\" fn jet_ffi_reporter(message: *const u8, len: usize) {{\n",
            "    let _ = (message, len);\n",
            "}}\n",
            "// JET_VETTED_UNSAFE_END: ffi_reporter\n",
            "fn jet_ffi_install_reporter() {{ {}::jet_ffi_set_reporter(jet_ffi_reporter); }}\n\n"
        ),
        link.crate_name,
    ));
}

/// Traits used by both the fixed runtime and generated root-program types.
/// Imported Jet modules keep their module-local traits; the root imports these
/// from the cached runtime rlib after the native builder splits the source.
fn push_cached_runtime_traits(out: &mut String) {
    out.push_str("pub trait __jet_Display {\n");
    out.push_str("    fn display(&self) -> String;\n");
    out.push_str("}\n\n");
    out.push_str("pub trait __jet_Equatable: Sized { fn equal(&self, rhs: &Self) -> bool; }\n");
    for ty in [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
        "bool", "char", "String",
    ] {
        out.push_str(&format!(
            "impl __jet_Equatable for {ty} {{ fn equal(&self, rhs: &Self) -> bool {{ self == rhs }} }}\n"
        ));
    }
    out.push('\n');
}

fn push_cached_runtime(out: &mut String, link: Option<&FfiLink>) {
    if link.is_some() {
        push_ffi_reporter(out, link);
    }
    out.push_str(CACHED_RUNTIME_BEGIN);
    if link.is_none() {
        // EnvInit is part of cached runtime and calls this hook. Keep no-FFI
        // stub inside marker block so split rlib has symbol too.
        push_ffi_reporter(out, None);
    }
    push_cached_runtime_traits(out);
    push_prelude(out);
    out.push_str(ENV_INIT_PRELUDE);
    push_mem_prelude(out);
    push_gc_prelude(out);
    out.push_str(LAYOUT_PRELUDE);
    out.push_str(CACHED_RUNTIME_END);
}

fn is_in_cached_runtime(source: &str, position: usize) -> bool {
    let Some(begin) = source.find(CACHED_RUNTIME_BEGIN) else {
        return false;
    };
    let Some(end) = source[begin + CACHED_RUNTIME_BEGIN.len()..]
        .find(CACHED_RUNTIME_END)
        .map(|offset| begin + CACHED_RUNTIME_BEGIN.len() + offset)
    else {
        return false;
    };
    begin < position && position < end
}

/// Exact fixed-runtime identity used by the final native-binary cache as well
/// as the rlib cache. A Prelude edit must invalidate both layers.
pub fn cached_runtime_fingerprint() -> String {
    let mut source = String::new();
    push_cached_runtime(&mut source, None);
    crate::SHA256::sha256_hex(source.as_bytes())
}

fn emit_command_metadata(
    bundle: &ProgramBundle,
    active_os: Syntax::OSTarget,
    out: &mut String,
) {
    let record = jet_foundation::CLISchema::encode_record(
        &jet_foundation::CLISchema::executable_schema(bundle),
    );
    let section = match active_os {
        Syntax::OSTarget::Linux => jet_foundation::CLISchema::ELF_SECTION,
        Syntax::OSTarget::MacOS => "__DATA,__jetcmd",
        Syntax::OSTarget::Windows => jet_foundation::CLISchema::PE_SECTION,
    };
    let bytes = record.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
    out.push_str(&format!(
        "#[used]\n#[no_mangle]\n#[link_section = {section:?}]\npub static __JET_COMMAND_SCHEMA: [u8; {}] = [{bytes}];\n\n",
        record.len(),
    ));
}
const ENV_INIT_PRELUDE: &str = include_str!("../Prelude/EnvInit.rs");

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
// D-TESTKIT1=A (parallel isolation gap): a test body's `print(...)` is routed
// here instead of straight to `println!` in test-harness builds (see the
// `TExprKind::Print` emit site). Buffered per-thread so parallel tests never
// interleave their own output; `jet_test_take_output` drains it right before
// the harness prints that test's `name: pass/FAIL` line.
thread_local! {
    static JET_TEST_OUT: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
}
fn jet_test_print(s: String) {
    JET_TEST_OUT.with(|buf| {
        let mut b = buf.borrow_mut();
        b.push_str(&s);
        b.push('\n');
    });
}
fn jet_test_take_output() -> String {
    JET_TEST_OUT.with(|buf| buf.borrow_mut().split_off(0))
}
/// D-E3-1905: the test child is an AOT binary. The release profile is encoded
/// at compile time so `jet test --release --trace-tiers` proves which binary
/// was built, without claiming that the test harness used a JIT or interpreter.
fn jet_test_trace_tier() {
    if std::env::var_os("JET_TEST_TRACE_TIERS").is_none() { return; }
    if cfg!(jet_release) {
        println!("tier aot profile=release");
    } else {
        println!("tier aot profile=default");
    }
}
/// Deterministic splitmix64 step, used by `jet test --shuffle` to reorder tests.
/// Independent of `JetRng`/`PROP_PRELUDE` (that runtime is only emitted when the
/// file has a property test) so shuffling never depends on the file's contents.
fn jet_test_shuffle_next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}
/// Fisher-Yates shuffle over test indices, seeded so a run is reproducible with
/// `--shuffle=<seed>` (or the seed jet printed, when no seed was given).
fn jet_test_shuffle_order(len: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    let mut state = seed;
    let mut i = len;
    while i > 1 {
        i -= 1;
        let j = (jet_test_shuffle_next(&mut state) % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order
}
"#;
const TEST_REPORT_PRELUDE: &str = include_str!("../Prelude/TestReport.rs");
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
        // D-E3-1905: keep common predicate landmarks visible while retaining
        // a broad random fallback. Test and fuzz use the same case seed stream.
        match rng.below(32) {
            0 => 0, 1 => 1, 2 => -1, 3 => 2, 4 => -2,
            5 => 42, 6 => -42, 7 => 99, 8 => 100, 9 => 255,
            10 => 256, 11 => 512, 12 => 1024,
            13 => i64::MIN, 14 => i64::MAX,
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
/// R10 / #437 / #1133: Core runtime emission is pay-for-what-you-call.
///
/// The JetStd brace chain (`mod jet_std {` … closing `}` in YAML.rs) must stay
/// contiguous — those files are one audited compiler/runtime kernel. Optional
/// fragments are selected from `bundle.used_core`. Package-owned Core behavior
/// may use an explicit ABI bridge, but never falls back to this path.
const CORELIB_KERNEL_PARTS: &[&str] = &[
    include_str!("../Prelude/CoreLib/JetStd/Open.rs"),
    include_str!("../Prelude/TaskGroup.rs"),
    include_str!("../Prelude/CoreLib/JetStd/Mime.rs"),
    include_str!("../Prelude/CoreLib/JetStd/UrlMime.rs"),
    include_str!("../Prelude/CoreLib/JetStd/JSONCodec.rs"),
    include_str!("../Prelude/CoreLib/JetStd/CommonTypes.rs"),
    include_str!("../Prelude/CoreLib/JetStd/DBPluginWire.rs"),
    include_str!("../Prelude/CoreLib/JetStd/WireOrder.rs"),
    include_str!("../Prelude/CoreLib/JetStd/DataTreeKind.rs"),
    include_str!("../Prelude/CoreLib/JetStd/DataTree.rs"),
    "\n// JET_VETTED_UNSAFE_BEGIN: jet_cell\nmod jet_cell {\n#[allow(unused_imports)]\nuse crate::{JetOutcome, JetAbsent};\n",
    include_str!("../Prelude/LocalCell.rs"),
    "\n}\npub use self::jet_cell::{JetCell, JetCellEditGuard, JetCellReadGuard};\n// JET_VETTED_UNSAFE_END: jet_cell\n",
    include_str!("../Prelude/CoreLib/JetStd/MathTaskMem.rs"),
    include_str!("../Prelude/CoreLib/JetStd/ReactiveEventWatch.rs"),
    include_str!("../Prelude/CoreLib/JetStd/JSONDataTree.rs"),
    include_str!("../Prelude/CoreLib/JetStd/TOML.rs"),
    include_str!("../Prelude/CoreLib/JetStd/YAML.rs"),
];

const CORE_SOURCE_CLOSURE_SCHEMA: &[u8] = b"jet-core-source-closure-v1";
const CORE_SOURCE_MARKER_PREFIX: &str = "__core_source::";
const CORE_INTRINSIC_MARKER_PREFIX: &str = "__core_intrinsic::";

// The package boundary is part of the compiler's identity. Keep the source,
// package metadata, and locked dependency graph in one content-addressed
// record so a changed package cannot reuse an older native artifact.
const CORE_ARCHIVE_SOURCE_PARTS: &[(&str, &str)] = &[
    (
        "module",
        include_str!("../../../../corelib/core.archive/pkgs/archive/archive.jet"),
    ),
    (
        "package",
        include_str!("../../../../corelib/core.archive/pkgs/archive/package.jet"),
    ),
    (
        "manifest",
        include_str!("../../../../corelib/core.archive/pkgs/archive/Cargo.toml"),
    ),
    (
        "lock",
        include_str!("../../../../corelib/core.archive/pkgs/archive/Cargo.lock"),
    ),
    (
        "abi",
        include_str!("../../../../corelib/core.archive/pkgs/archive/src/lib.rs"),
    ),
];

fn is_internal_core_usage(usage: &str) -> bool {
    usage.starts_with(CORE_SOURCE_MARKER_PREFIX)
        || usage.starts_with(CORE_INTRINSIC_MARKER_PREFIX)
}

fn is_core_package_source_usage(usage: &str) -> bool {
    let usage = usage
        .strip_prefix(CORE_SOURCE_MARKER_PREFIX)
        .unwrap_or(usage);
    usage == "core.archive" || usage.starts_with("core.archive::")
}

fn is_archive_core_usage(usage: &str) -> bool {
    is_core_package_source_usage(usage)
        || usage == "core.archive"
        || usage.starts_with("core.archive::")
}

fn core_needs_embedded_runtime(used_core: &std::collections::HashSet<String>) -> bool {
    used_core
        .iter()
        .any(|usage| !is_internal_core_usage(usage) && !is_archive_core_usage(usage))
}

fn append_identity_field(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}

fn core_source_closure_fingerprint(
    used_core: &std::collections::HashSet<String>,
) -> String {
    let mut usages: Vec<&str> = used_core.iter().map(String::as_str).collect();
    usages.sort_unstable();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(CORE_SOURCE_CLOSURE_SCHEMA);
    for usage in usages {
        append_identity_field(&mut bytes, usage.as_bytes());
    }
    if used_core
        .iter()
        .any(|usage| is_core_package_source_usage(usage))
    {
        for (label, source) in CORE_ARCHIVE_SOURCE_PARTS {
            append_identity_field(&mut bytes, label.as_bytes());
            append_identity_field(&mut bytes, source.as_bytes());
        }
    }
    crate::SHA256::sha256_hex(&bytes)
}

fn corelib_emission_identity(
    body: &str,
    used_core: &std::collections::HashSet<String>,
) -> String {
    let source = core_source_closure_fingerprint(used_core);
    let closure = crate::SHA256::sha256_hex(body.as_bytes());
    let mut identity = Vec::new();
    identity.extend_from_slice(CORE_SOURCE_CLOSURE_SCHEMA);
    append_identity_field(&mut identity, source.as_bytes());
    append_identity_field(&mut identity, closure.as_bytes());
    append_identity_field(&mut identity, body.len().to_string().as_bytes());
    let fingerprint = crate::SHA256::sha256_hex(&identity);
    format!(
        "/* jet-corelib-r10 source={source} closure={closure} len={} fp={fingerprint} */",
        body.len()
    )
}

fn core_usage_matches(used: &std::collections::HashSet<String>, prefixes: &[&str]) -> bool {
    used.iter().any(|usage| {
        prefixes.iter().any(|prefix| {
            usage == prefix
                || usage.starts_with(&format!("{prefix}::"))
                || usage.starts_with(&format!("{prefix}."))
        })
    })
}

fn push_corelib_prelude(
    out: &mut String,
    used_core: &std::collections::HashSet<String>,
    force: bool,
) {
    // `core.archive` is emitted as a reachable ordinary-Jet source module. Its
    // internal ABI calls do not require a compiler prelude fragment, so no old
    // template can become a fallback implementation.
    if !force && !core_needs_embedded_runtime(used_core) {
        return;
    }
    let mut body = String::new();
    push_corelib_prelude_body(&mut body, used_core);
    out.push_str(&corelib_emission_identity(&body, used_core));
    out.push('\n');
    out.push_str(&body);
}

fn type_uses_stream(ty: &Type) -> bool {
    match ty {
        Type::Apply { name, args } => {
            name == "Stream" || args.iter().any(type_uses_stream)
        }
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::FixedList { elem: inner, .. }
        | Type::Tagged { inner, .. } => type_uses_stream(inner),
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            type_uses_stream(key) || type_uses_stream(value)
        }
        Type::Tuple(fields) => fields.iter().any(|(_, field)| type_uses_stream(field)),
        Type::Union(members) => members.iter().any(type_uses_stream),
        Type::Fn { params, ret, .. } => {
            params.iter().any(type_uses_stream)
                || ret.as_deref().is_some_and(type_uses_stream)
        }
        _ => false,
    }
}

fn func_uses_stream(func: &crate::AST::Func) -> bool {
    func.params.iter().any(|param| type_uses_stream(&param.ty))
        || func
            .return_type
            .as_ref()
            .is_some_and(type_uses_stream)
}

fn trait_method_uses_stream(method: &crate::AST::TraitMethodSig) -> bool {
    method
        .params
        .iter()
        .any(|param| type_uses_stream(&param.ty))
        || method
            .return_type
            .as_ref()
            .is_some_and(type_uses_stream)
}

fn items_use_stream(items: &[Item]) -> bool {
    items.iter().any(|item| match item {
        Item::Func(func) => func_uses_stream(func),
        Item::Struct(def) => {
            def.fields.iter().any(|field| type_uses_stream(&field.ty))
                || def.methods.iter().any(func_uses_stream)
                || def.trait_impls.iter().any(|impl_block| {
                    impl_block.methods.iter().any(func_uses_stream)
                        || impl_block
                            .assoc_type_impls
                            .iter()
                            .any(|(_, _, ty)| type_uses_stream(ty))
                })
        }
        Item::Enum(def) => {
            def.variants.iter().any(|variant| match &variant.payload {
                crate::AST::VariantPayload::Unit => false,
                crate::AST::VariantPayload::Single(ty, _) => type_uses_stream(ty),
                crate::AST::VariantPayload::Named(fields) => {
                    fields.iter().any(|field| type_uses_stream(&field.ty))
                }
            }) || def.methods.iter().any(func_uses_stream)
                || def.trait_impls.iter().any(|impl_block| {
                    impl_block.methods.iter().any(func_uses_stream)
                        || impl_block
                            .assoc_type_impls
                            .iter()
                            .any(|(_, _, ty)| type_uses_stream(ty))
                })
        }
        Item::Distinct(def) => type_uses_stream(&def.base),
        Item::TypeAlias(def) => type_uses_stream(&def.target),
        Item::Trait(def) => def.methods.iter().any(trait_method_uses_stream),
        Item::Impl(def) => {
            def.methods.iter().any(func_uses_stream)
                || def
                    .assoc_type_impls
                    .iter()
                    .any(|(_, _, ty)| type_uses_stream(ty))
        }
        Item::ExternRust(def) => def.functions.iter().any(|func| {
            func.params.iter().any(|param| type_uses_stream(&param.ty))
                || func
                    .return_type
                    .as_ref()
                    .is_some_and(type_uses_stream)
        }),
        Item::ProtocolDecl(def) => def
            .messages
            .iter()
            .any(|message| message.fields.iter().any(|(_, ty)| type_uses_stream(ty))),
        Item::CodeModule(def) => def
            .body
            .as_deref()
            .is_some_and(items_use_stream),
        Item::GenericModule(def) => items_use_stream(&def.body),
        _ => false,
    })
}

fn uses_stream(bundle: &ProgramBundle) -> bool {
    bundle
        .modules
        .iter()
        .any(|module| items_use_stream(&module.items))
}

fn needs_embedded_runtime(bundle: &ProgramBundle) -> bool {
    core_needs_embedded_runtime(&bundle.used_core) || uses_stream(bundle)
}

/// R10 / #1133: content identity of the semantic Core closure and emitted
/// compiler/runtime fragments a program will link.
pub fn corelib_emission_fingerprint(
    used_core: &std::collections::HashSet<String>,
) -> String {
    let mut body = String::new();
    if core_needs_embedded_runtime(used_core) {
        push_corelib_prelude_body(&mut body, used_core);
    }
    corelib_emission_identity(&body, used_core)
}

fn push_corelib_prelude_body(out: &mut String, used_core: &std::collections::HashSet<String>) {
    // JetStd Open/CommonTypes + EncodingStream/Codecs name these foundation
    // modules unconditionally — always emit them with the Core kernel.
    out.push_str("\nmod jet_xml_pull {\n");
    out.push_str(include_str!("../../../jet-foundation/src/XmlPull.rs"));
    out.push_str("\n}\n");
    out.push_str("\n#[allow(non_snake_case)]\nmod XmlPull { pub use crate::jet_xml_pull::*; }\n");
    out.push_str("\nmod jet_xml_kernel {\n");
    out.push_str(include_str!("../../../jet-foundation/src/XmlKernel.rs"));
    out.push_str("\n}\n");
    out.push_str("\nmod jet_cbor_budget {\n");
    out.push_str(include_str!("../../../jet-foundation/src/CborBudget.rs"));
    out.push_str("\n}\n");
    out.push_str("\n#[allow(non_snake_case)]\nmod CborBudget { pub use crate::jet_cbor_budget::*; }\n");
    out.push_str("\nmod jet_cbor_kernel {\n");
    out.push_str(include_str!("../../../jet-foundation/src/CborKernel.rs"));
    out.push_str("\n}\n");
    out.push_str("\nmod jet_base_encoding_strict {\n");
    out.push_str(include_str!("../../../jet-foundation/src/BaseEncodingStrict.rs"));
    out.push_str("\n}\n");
    out.push_str("\nmod jet_regex_syntax {\n");
    out.push_str(include_str!("../../../jet-foundation/src/RegexSyntax.rs"));
    out.push_str("\n}\n");
    out.push_str("\nmod jet_encoding_errors {\n");
    out.push_str(include_str!("../../../jet-foundation/src/EncodingErrors.rs"));
    out.push_str("\n}\n");
    let needs_xml = core_usage_matches(used_core, &["core.encoding.xml", "core.encoding"]);
    let needs_base = core_usage_matches(
        used_core,
        &[
            "core.encoding",
            "core.encoding.hex",
            "core.encoding.cbor",
            "core.binary",
        ],
    );
    let _needs_regex = core_usage_matches(used_core, &["core.regex"]);
    // needs_xml / needs_base still drive encoding Top reachability below.

    for part in CORELIB_KERNEL_PARTS {
        // Host crates include UrlMime.rs directly, so it includes its sibling
        // MIME kernel. AOT already embeds that kernel as the preceding part.
        out.push_str(part.strip_prefix("    include!(\"Mime.rs\");\n\n").unwrap_or(part));
    }
    // D-CONC-FAIL1=A: the typed child-failure value lives in the optional
    // JetStd kernel, so emit its root value traits only beside that kernel.
    // Programs without Core runtime reachability must not name `jet_std`.
    out.push_str(
        "\nimpl JetShow for jet_std::JetTaskFailure {\n\
            fn jet_show(&self) -> String { format!(\"{self:?}\") }\n\
        }\n\
        impl JetDisplay for jet_std::JetTaskFailure {\n\
            fn jet_display(&self) -> String { self.jet_show() }\n\
        }\n\
        impl JetDebug for jet_std::JetTaskFailure {\n\
            fn jet_debug(&self) -> String { self.jet_show() }\n\
        }\n\
        impl JetDebug for jet_std::DataTree {\n\
            fn jet_debug(&self) -> String { self.jet_show() }\n\
        }\n",
    );
    out.push_str("\npub use crate::jet_std::JetTaskGroupRuntime;\n");
    // Card #1751: the one 80x24 terminal default, read by CommonTypes.rs's
    // TerminalPolicy::default (in the kernel closure above) and by
    // ProcessPty.rs's PtyConfig::default when process/PTY support is emitted.
    // Unconditional like the kernel closure, so both can always reach it.
    out.push_str("\nmod terminal_default {\n");
    out.push_str(include_str!("../Prelude/TerminalDefault.rs"));
    out.push_str("\n}\n");

    let needs_email = core_usage_matches(used_core, &["core.email"]);
    let needs_raylib = core_usage_matches(used_core, &["core.raylib"]);
    let needs_game = core_usage_matches(used_core, &["core.game"]) || needs_raylib;
    let needs_files = core_usage_matches(
        used_core,
        &[
            "core.files",
            "core.watcher",
            "core.io",
            "core.env",
            "core.os",
            "core.process",
        ],
    );
    let needs_interrupt = core_usage_matches(used_core, &["core.os::on_interrupt"]);
    let needs_text = core_usage_matches(
        used_core,
        &["core.text", "core.text.unicode", "core.fmt", "core.term"],
    );
    let needs_fs_runtime = needs_files
        || core_usage_matches(
            used_core,
            &[
                "core.args",
                "core.process",
                "core.testing",
                "core.perf",
                "core.scope",
            ],
        );
    let needs_crypto = core_usage_matches(
        used_core,
        &[
            "core.crypto",
            "core.crypto.expert",
            "core.crypto.random",
            "core.vault",
            "core.vault.expert",
            "core.uuid",
        ],
    );
    let needs_process = core_usage_matches(used_core, &["core.process"]);
    let needs_math = core_usage_matches(
        used_core,
        &["core.math", "core.random", "core.time", "core.time.date", "core.time.datetime", "core.time.expiring", "core.science.measurement"],
    );
    let needs_encoding = core_usage_matches(
        used_core,
        &[
            "core.encoding",
            "core.encoding.json",
            "core.encoding.jsonl",
            "core.encoding.csv",
            "core.encoding.toml",
            "core.encoding.yaml",
            "core.encoding.xml",
            "core.encoding.cbor",
            "core.encoding.hex",
            "core.compress.gzip",
            "core.compress.zstd",
            "core.binary",
        ],
    ) || needs_xml
        || needs_base;
    let needs_data = core_usage_matches(
        used_core,
        &[
            "core.data",
            "core.sketch.hll",
            "core.sketch.tdigest",
            "core.sketch.cms",
            "core.sketch.reservoir",
            "core.db",
        ],
    );
    // DataFmt.rs contains the data/codec helpers filed under `core.data`, but
    // its old `jet_fmt_*` helpers now live in the shared Fmt kernel. Keep the
    // two emission gates separate so a fmt-only program gets only Fmt.rs.
    let needs_fmt = core_usage_matches(used_core, &["core.fmt"]);
    let needs_data_fmt = needs_data || needs_encoding;
    let needs_compute = core_usage_matches(used_core, &["core.compute"]);
    let needs_net = core_usage_matches(
        used_core,
        &[
            "core.net",
            "core.tls",
            "core.http",
            "core.http.client",
            "core.http.server",
            "core.ws",
            "core.email",
            "core.browser",
            "core.web",
            "core.web.devserver",
            "core.web.storage",
            "core.web.storage.local",
            "core.web.storage.session",
        ],
    );
    let needs_http = core_usage_matches(
        used_core,
        &[
            "core.http",
            "core.http.client",
            "core.http.server",
            "core.web",
            "core.web.devserver",
        ],
    );
    let needs_ws = core_usage_matches(used_core, &["core.ws"]);
    let needs_browser = core_usage_matches(
        used_core,
        &[
            "core.browser",
            "core.web",
            "core.web.storage",
            "core.web.storage.local",
            "core.web.storage.session",
        ],
    );
    let needs_args = core_usage_matches(used_core, &["core.args"]);
    let needs_reflect = core_usage_matches(used_core, &["core.reflect", "core.lang"]);
    let needs_auth_tokens = core_usage_matches(used_core, &["core.auth"]) || needs_crypto;
    let needs_auth_session = core_usage_matches(used_core, &["core.auth", "app"]);
    let needs_sync = core_usage_matches(used_core, &["core.sync", "app", "core.db"]);
    let needs_services = core_usage_matches(used_core, &["core.services"]);
    let needs_mod = core_usage_matches(used_core, &["core.mod"]);
    if needs_mod {
        // The generated loader must compare against the compiler that emitted
        // this program. Keep the value in the shared Prelude, not in a host.
        out.push_str(&format!(
            "\nconst __JET_COMPILER_VERSION: &str = {:?};\n",
            env!("CARGO_PKG_VERSION")
        ));
        out.push_str(include_str!("../Prelude/CoreLib/Top/Mod.rs"));
        out.push('\n');
    }

    // Kernel closure: JetStd brace-chain files name these Top symbols
    // (FileReader, text fold, JSON frames, TCPStream, deadlines, TLS entropy).
    // NetHTTP is TCP/TLS only — HTTP serve/router lives in HTTPServer (gated).
    out.push_str(include_str!("../Prelude/CoreLib/Top/HandlesRaylib.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/UnicodeTables.rs"));
    out.push_str(include_str!("../Prelude/Core/Path.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/Text.rs"));
    out.push_str(include_str!("../Prelude/Core/Codec.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/EncodingTraits.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/EncodingHostileIo.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/EncodingStream.rs"));
    out.push_str(include_str!("../Prelude/Core/EncodingBase.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/EncodingCodecs.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/SHA256Raw.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/SHAFamily.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/RingCsvLogTimeCrypto.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/CryptoEntropy.rs"));
    out.push_str("use jet_crypto_entropy::{jet_crypto_entropy_fill, JetCryptoEntropyError};\n");
    out.push_str(include_str!("../Prelude/CoreLib/Top/DNSResolverPolicy.rs"));
    out.push_str(include_str!("../Prelude/Deadline.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/TimeSleep.rs"));
    out.push_str(include_str!("../Prelude/Core/NetPure.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/NetHTTP.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/Solver.rs"));
    out.push_str(include_str!("../Prelude/Core/SeededRandom.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/MathRandomTime.rs"));

    if needs_email {
        out.push_str(include_str!("../Prelude/CoreLib/Email.rs"));
    }
    if needs_raylib {
        // File/DB/Plugin handles already emitted in the kernel closure above;
        // raylib-only when explicitly used stays a no-op re-include guard via
        // the same source (idempotent struct defs would conflict). Skip.
    }
    if needs_game {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Game.rs"));
    }
    if needs_files {
        out.push_str(include_str!("../Prelude/CoreLib/Top/PathFiles.rs"));
    }
    if needs_fs_runtime && !needs_files {
        // FSIoEnvOsTesting uses the Path value and atomic-write helpers even
        // for `core.testing`-only programs (for example `testing.snap`).
        // Keep that transitive prelude dependency in the shared closure.
        out.push_str(include_str!("../Prelude/CoreLib/Top/PathFiles.rs"));
    }
    if needs_text {
        // Text/Unicode already in kernel closure.
    }
    // Process helpers are also used by FSIoEnvOsTesting (`jet_process_command` /
    // `jet_process_spec_run_inner`) — emit whenever either surface is needed (I9).
    // Process must come before FSIoEnvOsTesting so those symbols are in scope.
    if needs_process || needs_fs_runtime {
        out.push_str("\nmod jet_process_pty {\n");
        out.push_str(include_str!("../Prelude/CoreLib/ProcessPty.rs"));
        out.push_str("\n}\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/ProcessPolicy.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/ProcessSpec.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/Process.rs"));
    }
    if needs_fs_runtime {
        out.push_str(include_str!("../Prelude/CoreLib/Top/TestingShared.rs"));
        // #1480: split out of FSIoEnvOsTesting.rs so the JIT host can
        // `include!` this exact source (I9 — single Prelude source of truth).
        out.push_str(include_str!("../Prelude/CoreLib/Top/IoLineStream.rs"));
        // D-OSINTERRUPT1: pending-count, registration-order, and boundary
        // policy are shared by AOT, resident JIT, and the interpreter. Their
        // callback storage remains an engine adapter. Keep the whole
        // interrupt Prelude out of ordinary `core.os` programs; FSIo's
        // dispatcher is stripped below when `on_interrupt` is unused.
        if needs_interrupt {
            out.push_str(include_str!("../Prelude/CoreLib/Top/Interrupt.rs"));
        }
        out.push_str(include_str!("../Prelude/CoreLib/Top/FSIoEnvOsTesting.rs"));
        // #1465: identity / release / POSIX control — after FSIoEnvOsTesting so
        // jet_std_os_pid / env helpers and jet_std_process_exit stay in scope.
        // Vetted region: OsExtra carries POSIX `unsafe` at crate root (not only
        // inside `mod jet_os_sys`); golden I1 strips this delimiter.
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_os_extra\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/OsExtra.rs"));
        out.push_str("// JET_VETTED_UNSAFE_END: jet_os_extra\n");
    }
    if needs_crypto {
        // CryptoEntropy already in kernel closure (TLS identity + JetStd).
    }
    if needs_math {
        // Math and random helpers first — LinalgFns and the rest of the math
        // surface call them. MathLibPure is shared with JIT/comptime (I9).
        out.push_str(include_str!("../Prelude/CoreLib/Top/MathLibPure.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/MathRandomFns.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/LinalgFns.rs"));
    }
    if needs_encoding {
        // Encoding templates already in kernel closure.
    }
    if needs_data {
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataPlot.rs"));
    }
    if needs_fmt {
        out.push_str(include_str!("../Prelude/Core/Fmt.rs"));
    }
    if needs_data_fmt {
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataFmt.rs"));
    }
    if needs_data {
        // #1657: the one `core.data` statistics kernel. The JIT host and the
        // comptime tier `include!` this same file, so every tier runs the same
        // compensated arithmetic and reports the same `DataError` (I9).
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataStats.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataFlow.rs"));
    }
    if needs_compute {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Compute.rs"));
    }
    if needs_net {
        // DNS + NetHTTP (TCP/TLS) already in kernel closure.
    }
    if needs_http {
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPMessage.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPRoute.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPClient.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPServer.rs"));
    } else if needs_ws || needs_browser {
        // Ws.rs shares the canonical HTTP request/header value types even for
        // browser-only programs; keep the reachability split without emitting
        // the full HTTP client/server surface.
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPMessage.rs"));
    }
    if needs_ws || needs_http || needs_browser {
        out.push_str(include_str!("../Prelude/CoreLib/Top/WsClient.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/Ws.rs"));
    }
    if needs_browser {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Browser.rs"));
    }
    if needs_args {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Args.rs"));
    }
    if needs_reflect {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Reflect.rs"));
    }
    if needs_auth_tokens {
        // D-AUTH2=A: JWT/PASETO verify prelude.
        out.push_str(include_str!("../Prelude/CoreLib/Top/Auth.rs"));
    }
    if needs_auth_session {
        // D-AUTH1=A: sessions + `app.auth` prelude.
        out.push_str(include_str!("../Prelude/CoreLib/Top/AuthSession.rs"));
    }
    if needs_sync {
        // D-SYNC1=A / D-DBPOLICY1=A: CRDT values + row policies.
        out.push_str("mod jet_sync {\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/Sync.rs"));
        out.push_str("\n}\npub(crate) use jet_sync::*;\n");
    }
    if needs_services {
        out.push_str(include_str!("../Prelude/CoreLib/Top/ServiceAuthority.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/Services.rs"));
    }
}
const SCHEDULER_PRELUDE_RAW: &str = include_str!("../Prelude/Scheduler.rs");
const STREAM_PRELUDE_RAW: &str = include_str!("../Prelude/Stream.rs");
/// D-RENDERTGT1=A + D-RENDERTGT2=A (c133 M1): UI backend trait seam + null backend.
const UI_PRELUDE: &str = include_str!("../Prelude/Ui.rs");
/// D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend. Emitted only when
/// a program constructs `core.ui.gtk_backend()` (`uses_gtk_backend`), so no
/// other program carries the gtk `extern "C"` surface or needs `-lgtk-4`.
const UI_GTK_PRELUDE: &str = include_str!("../Prelude/UiGtk.rs");
/// c-devserver (owner-directed 2026-07-01): `core.web.devserver` — the
/// configurable `jet dev` server value (`for_app`/`.html`/`.port`/`.serve`).
const DEVSERVER_PRELUDE: &str = include_str!("../Prelude/DevServer.rs");
/// D-WEBAPP1=D: `core.web.app` full-stack application builder.
const APP_PRELUDE: &str = include_str!("../Prelude/App.rs");
const LIVEQUERY_PRELUDE: &str = include_str!("../Prelude/CoreLib/Top/LiveQuery.rs");
/// D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): allocator runtime helpers.
const MEM_PRELUDE: &str = include_str!("../Prelude/Mem.rs");
const UNINIT_PRELUDE: &str = include_str!("../Prelude/Uninit.rs");

fn push_app_preludes(out: &mut String, used_core: &std::collections::HashSet<String>) {
    // App/DevServer call into HTTPServer helpers. Emit them only when the
    // program uses web/HTTP surfaces — bare `app.live` / `app.auth` must not
    // drag the full HTTP server templates (R10).
    let needs_app_runtime = core_usage_matches(
        used_core,
        &[
            "core.web",
            "core.http",
            "core.http.server",
            "core.http.client",
            "core.web.devserver",
        ],
    );
    let needs_live = core_usage_matches(used_core, &["app", "core.web", "core.db"]);
    if needs_app_runtime {
        out.push_str(DEVSERVER_PRELUDE);
        out.push_str(APP_PRELUDE);
    }
    if needs_live {
        out.push_str(LIVEQUERY_PRELUDE);
    }
}

fn push_mem_prelude(out: &mut String) {
    out.push_str("mod jet_uninit_semantics {\n");
    out.push_str(UNINIT_PRELUDE);
    out.push_str("\n}\n");
    out.push_str(MEM_PRELUDE);
}
/// D-DEP-GC1=A: one collector source backs jet-rt JIT/dev and emitted AOT code.
const GC_RUNTIME_PRELUDE: &str = include_str!("../../../jet-rt/src/__gc.rs");

fn push_gc_prelude(out: &mut String) {
    out.push_str("mod jet_gc {\n");
    out.push_str(GC_RUNTIME_PRELUDE);
    out.push_str("\n}\n");
}
/// D-LAYOUT1 / D-LAYOUT-GATES1 (ratified 2026-06-28/29): the `layout NAME { … }`
/// constraint-solver runtime (`jet_layout`). Pure safe Rust (no `unsafe`), so —
/// unlike `MEM_PRELUDE` — it never needs stripping; included everywhere
/// `MEM_PRELUDE`/private collector runtime are (not just the UI-specific sites), since a
/// `layout {}` block isn't limited to UI code.
const LAYOUT_PRELUDE: &str = include_str!("../Prelude/Layout.rs");

/// Tower #126: emitted AOT programs that use tasks/networking/process/fs-runtime ship AND select the
/// native readiness backend (epoll on Linux, kqueue on the BSD/Apple family), not
/// just the portable poll. Other Core users retain the safe portable scheduler
/// compatibility surface without inheriting unrelated native FFI (I1).
///
/// The prelude gates its native syscall paths behind `feature = "jet_native_io"`
/// for the in-crate JIT copy (`jet_codegen::scheduler`, whose Cargo manifest turns
/// that feature on by default). Emitted user programs are built by a bare `rustc`
/// with no Cargo features, so that predicate would always be false and the program
/// would silently fall back to portable poll. Rewrite the predicate to a
/// vacuously-true `all()` at emit time: native then selects purely on `target_os`,
/// so a Linux build compiles in and uses epoll.
///
/// I1: the raw epoll/kqueue syscalls stay inside the `jet:scheduler-native`
/// vetted region — the only `unsafe` in the emitted scheduler. `tests/golden.rs`
/// ignores exactly that region in its I1 unsafe scan, matching the vetted-internal
/// pattern used by `jet_os_unix`/`jet_term_unix`.
fn strip_scheduler_region(mut source: String, name: &str) -> String {
    let begin = format!("// jet:scheduler-native-{name}-begin");
    let end = format!("// jet:scheduler-native-{name}-end");
    let Some(start) = source.find(&begin) else {
        return source;
    };
    let Some(relative_end) = source[start..].find(&end) else {
        return source;
    };
    let end_offset = start + relative_end + end.len();
    source.replace_range(start..end_offset, "");
    source
}

fn scheduler_prelude_for_emit(native_io: bool) -> &'static str {
    static NATIVE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    static PORTABLE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if native_io {
        NATIVE.get_or_init(|| {
            let mut source = SCHEDULER_PRELUDE_RAW.replace("feature = \"jet_native_io\"", "all()");
            source.push_str(STREAM_PRELUDE_RAW);
            source
        })
    } else {
        PORTABLE.get_or_init(|| {
            let mut source = ["notify", "epoll", "kqueue", "iocp"]
                .into_iter()
                .fold(SCHEDULER_PRELUDE_RAW.to_string(), strip_scheduler_region);
            source.push_str(STREAM_PRELUDE_RAW);
            source
        })
    }
}

fn uses_native_scheduler(bundle: &ProgramBundle) -> bool {
    bundle.used_core.iter().any(|usage| {
        [
            "core.tasks",
            "core.net",
            "core.http",
            "core.process",
            "core.files",
            "core.watcher",
            "core.io",
            "core.env",
            "core.os",
            "core.args",
            "core.testing",
            "core.perf",
            "core.scope",
        ]
            .iter()
            .any(|module| {
                usage.strip_prefix(module).is_some_and(|suffix| {
                    suffix.is_empty() || suffix.starts_with("::") || suffix.starts_with('.')
                })
            })
    })
}

/// D-ALLOC2: the `jet_mem` arena helper carries the one vetted lifetime-extension
/// `unsafe` (D-LL1). It is part of the always-emitted prelude, but a program that
/// never touches `core.mem` allocators must not carry any `unsafe` at all (I1 —
/// golden/closures/regex/… tests assert zero `unsafe` in such output). So strip
/// the `mod jet_mem { … }` block whenever nothing references `jet_mem::`.
fn strip_unused_mem_prelude(out: String) -> String {
    let Some(start) = out.find("mod jet_mem") else {
        return out;
    };
    if is_in_cached_runtime(&out, start) {
        return out;
    }
    // Brace-match the module body to find its end.
    let bytes = out.as_bytes();
    let mut depth = 0usize;
    let mut seen = false;
    let mut end = out.len();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                seen = true;
            }
            b'}' => {
                depth -= 1;
                if seen && depth == 0 {
                    end = i + 1;
                    break;
                }
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

/// D-OPTGC1: strip `mod jet_gc { … }` when the program never references `jet_gc::`.
fn strip_unused_gc_prelude(out: String) -> String {
    let Some(start) = out.find("mod jet_gc") else {
        return out;
    };
    if is_in_cached_runtime(&out, start) {
        return out;
    }
    let bytes = out.as_bytes();
    let mut depth = 0usize;
    let mut seen = false;
    let mut end = out.len();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                seen = true;
            }
            b'}' => {
                depth -= 1;
                if seen && depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let used = out[..start].contains("jet_gc::") || out[end..].contains("jet_gc::");
    if used {
        return out;
    }
    let mut s = out[..start].to_string();
    let rest = out[end..].trim_start_matches('\n');
    s.push_str(rest);
    s
}

/// D-FLAGSHIP-RAYLIB1=A: the raylib bridge carries vetted FFI `unsafe`.
/// Programs that never call `core.raylib` must not inherit that unsafe prelude.
fn strip_unused_raylib_prelude(out: String) -> String {
    let prelude_end = [
        out.find("fn __jet_"),
        out.find("pub fn __jet_"),
        out.find("fn main()"),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(out.len());
    if out[prelude_end..].contains("jet_raylib_") {
        return out;
    }
    const BEGIN: &str = "// jet:raylib-begin";
    const END: &str = "// jet:raylib-end";
    let Some(start) = out.find(BEGIN) else {
        return out;
    };
    let Some(end_marker) = out.find(END) else {
        return out;
    };
    let end = end_marker + END.len();
    let mut s = out[..start].to_string();
    s.push_str(out[end..].trim_start_matches('\n'));
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
    if is_in_cached_runtime(&out, start) {
        return out;
    }
    let bytes = out.as_bytes();
    let mut depth = 0usize;
    let mut seen = false;
    let mut end = out.len();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                seen = true;
            }
            b'}' => {
                depth -= 1;
                if seen && depth == 0 {
                    end = i + 1;
                    break;
                }
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
    let prelude_end = [
        out.find("fn __jet_"),
        out.find("pub fn __jet_"),
        out.find("fn main()"),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(out.len());
    let user_code = &out[prelude_end..];
    if user_code.contains("jet_term_enter")
        || user_code.contains("jet_term_read_key")
        // The always-emitted helper is type-checked even when user code does
        // not call it, so its secret-mode dispatchers must remain reachable.
        || out.contains("fn jet_std_io_input_secret")
    {
        return out;
    }
    // The term prelude is one contiguous block: the `#[cfg(unix)]` line above
    // `mod jet_term_unix` through the end of `fn jet_term_read_key` (the two
    // platform modules plus the enter/leave/read_key dispatchers that call into
    // them). Excise it as a single span — stripping only the `mod` blocks would
    // leave the dispatchers referencing now-missing modules (I2: E0433).
    let Some(unix_mod) = out.find("mod jet_term_unix {") else {
        return out;
    };
    if is_in_cached_runtime(&out, unix_mod) {
        return out;
    }
    let block_start = out[..unix_mod].rfind("#[cfg(unix)]").unwrap_or(unix_mod);
    let Some(read_key) = out.find("fn jet_term_read_key") else {
        return out;
    };
    let bytes = out.as_bytes();
    let (mut depth, mut seen, mut i) = (0usize, false, read_key);
    let mut end = out.len();
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                seen = true;
            }
            b'}' => {
                depth -= 1;
                if seen && depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let mut s = out[..block_start].to_string();
    s.push_str(out[end..].trim_start_matches('\n'));
    s
}

/// D-OSFACTS1: `core.os.on_interrupt` uses vetted Unix/Windows platform FFI.
/// Keep ordinary programs `unsafe`-free by stripping the whole dispatcher
/// unless generated user code actually calls it.
fn strip_unused_os_signal_prelude(out: String) -> String {
    let prelude_end = [
        out.find("fn __jet_"),
        out.find("pub fn __jet_"),
        out.find("fn main()"),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(out.len());
    if out[prelude_end..].contains("jet_std_os_on_interrupt") {
        return out;
    }

    let Some(block_start) = out.find("mod jet_os_interrupt {") else {
        return out;
    };
    let Some(wrapper_fn) = out[block_start..]
        .find("fn jet_std_os_on_interrupt")
        .map(|i| i + block_start)
    else {
        return out;
    };

    let bytes = out.as_bytes();
    let (mut depth, mut seen, mut i) = (0usize, false, wrapper_fn);
    let mut end = out.len();
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                seen = true;
            }
            b'}' => {
                depth -= 1;
                if seen && depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut s = out[..block_start].to_string();
    s.push_str(out[end..].trim_start_matches('\n'));
    s
}

pub(crate) fn emit_synthetic_display_trait(out: &mut String, include_runtime_owned: bool) {
    if include_runtime_owned {
        out.push_str("pub trait __jet_Display {\n");
        out.push_str("    fn display(&self) -> String;\n");
        out.push_str("}\n\n");
    }
    out.push_str("pub trait __jet_Debug {\n");
    out.push_str("    fn debug(&self) -> String;\n");
    out.push_str("}\n\n");
}

pub(crate) fn emit_synthetic_operator_traits(out: &mut String, include_runtime_owned: bool) {
    out.push_str("#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]\n");
    out.push_str("pub enum __jet_Ordering { __jet_Less, __jet_Equal, __jet_Greater }\n\n");
    for (name, method, ret) in [
        ("Add", "add", "Self"),
        ("Sub", "sub", "Self"),
        ("Mul", "mul", "Self"),
        ("Div", "div", "Self"),
        ("Equatable", "equal", "bool"),
        ("Comparable", "compare", "__jet_Ordering"),
    ] {
        if name == "Equatable" && !include_runtime_owned {
            continue;
        }
        if matches!(name, "Add" | "Sub" | "Mul" | "Div") {
            let trait_rust = mangle(name);
            out.push_str(&crate::jet_name_format!(
                "pub trait {trait_rust}: Sized {{ fn {method}(&self, rhs: &Self) -> Self; fn {name_prefix}{method}_at(&self, rhs: &Self, _file: &str, _line: u32) -> Self {{ self.{method}(rhs) }} }}\n"
            ));
        } else {
            let trait_rust = mangle(name);
            out.push_str(&format!(
                "pub trait {trait_rust}: Sized {{ fn {method}(&self, rhs: &Self) -> {ret}; }}\n"
            ));
        }
    }
    for ty in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        for (trait_name, method, checked) in [
            ("Add", "add", "jet_add"),
            ("Sub", "sub", "jet_sub"),
            ("Mul", "mul", "jet_mul"),
            ("Div", "div", "jet_div"),
        ] {
            let trait_rust = mangle(trait_name);
            out.push_str(&crate::jet_name_format!(
                "impl {trait_rust} for {ty} {{ fn {method}(&self, rhs: &Self) -> Self {{ (*self).{checked}(*rhs, \"<built-in {trait_name}>\", 0) }} fn {name_prefix}{method}_at(&self, rhs: &Self, file: &str, line: u32) -> Self {{ (*self).{checked}(*rhs, file, line) }} }}\n"
            ));
        }
    }
    for ty in ["f32", "f64"] {
        for (trait_name, method, op) in [
            ("Add", "add", "+"),
            ("Sub", "sub", "-"),
            ("Mul", "mul", "*"),
            ("Div", "div", "/"),
        ] {
            let trait_rust = mangle(trait_name);
            out.push_str(&format!(
                "impl {trait_rust} for {ty} {{ fn {method}(&self, rhs: &Self) -> Self {{ *self {op} *rhs }} }}\n"
            ));
        }
    }
    if include_runtime_owned {
        for ty in [
            "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64",
            "bool", "char", "String",
        ] {
            out.push_str(&format!(
                "impl __jet_Equatable for {ty} {{ fn equal(&self, rhs: &Self) -> bool {{ self == rhs }} }}\n"
            ));
        }
    }
    for ty in [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "bool", "char",
        "String",
    ] {
        out.push_str(&format!(
            "impl __jet_Comparable for {ty} {{ fn compare(&self, rhs: &Self) -> __jet_Ordering {{ if self < rhs {{ __jet_Ordering::__jet_Less }} else if self > rhs {{ __jet_Ordering::__jet_Greater }} else {{ __jet_Ordering::__jet_Equal }} }} }}\n"
        ));
    }
    out.push('\n');
}

/// D-SHAPE-RESOURCE2=A: the one nominal consuming, infallible cleanup trait.
pub(crate) fn emit_synthetic_close_trait(out: &mut String) {
    out.push_str("pub trait __jet_Close {\n");
    out.push_str("    fn close(self);\n");
    out.push_str("}\n\n");
    out.push_str("struct JetResource<T: __jet_Close>(Option<T>);\n");
    out.push_str("impl<T: __jet_Close> JetResource<T> { fn new(value: T) -> Self { Self(Some(value)) } fn take(&mut self) -> T { self.0.take().expect(\"resource already consumed\") } fn close(&mut self) { if let Some(value) = self.0.take() { __jet_Close::close(value); } } }\n");
    out.push_str("impl<T: __jet_Close> std::ops::Deref for JetResource<T> { type Target = T; fn deref(&self) -> &T { self.0.as_ref().expect(\"resource already consumed\") } }\n");
    out.push_str("impl<T: __jet_Close> std::ops::DerefMut for JetResource<T> { fn deref_mut(&mut self) -> &mut T { self.0.as_mut().expect(\"resource already consumed\") } }\n");
    out.push_str("impl<T: __jet_Close> Drop for JetResource<T> { fn drop(&mut self) { self.close(); } }\n\n");
}

fn collect_allocator_constructors(
    stmts: &[Stmt],
    cx: &Cx,
    locals: &mut HashSet<String>,
    found: &mut HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Val(binding) => {
                if let Expr::MethodCall { receiver, method, .. } = &binding.init {
                    if let Some(name) = TIR::alloc_new_type(receiver, method, cx, locals) {
                        found.insert(name.to_string());
                    }
                }
                locals.insert(binding.name.clone());
            }
            Stmt::For { var, var2, body, .. } => {
                let mut names = vec![var.as_str()];
                if let Some((name, _)) = var2 {
                    names.push(name);
                }
                collect_allocator_nested(body, cx, locals, found, &names);
            }
            Stmt::Switch { arms, else_body, .. }
            | Stmt::ComptimeSwitch { arms, else_body, .. } => {
                for arm in arms {
                    collect_allocator_nested(&arm.body, cx, locals, found, &[]);
                }
                if let Some(body) = else_body {
                    collect_allocator_nested(body, cx, locals, found, &[]);
                }
            }
            Stmt::CountedLoop { init, body, step, .. } => {
                let mut scope = locals.clone();
                scope.insert(init.name.clone());
                collect_allocator_constructors(body, cx, &mut scope, found);
                if let Some(step) = step.as_deref() {
                    collect_allocator_constructors(std::slice::from_ref(step), cx, &mut scope, found);
                }
            }
            // D-CANVASSTATE1=D: an `#Off` body is never emitted.
            Stmt::Switched { marker, .. } if crate::AST::switched_off(marker) => {}
            Stmt::While { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Switched { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::ScopeMember { body, .. } => {
                collect_allocator_nested(body, cx, locals, found, &[])
            }
            Stmt::ComptimeIf { then_body, else_body, selected_then, .. } => {
                match selected_then {
                    Some(true) => collect_allocator_nested(then_body, cx, locals, found, &[]),
                    Some(false) => {
                        if let Some(body) = else_body {
                            collect_allocator_nested(body, cx, locals, found, &[]);
                        }
                    }
                    None => {
                        collect_allocator_nested(then_body, cx, locals, found, &[]);
                        if let Some(body) = else_body {
                            collect_allocator_nested(body, cx, locals, found, &[]);
                        }
                    }
                }
            }
            Stmt::Expr(_)
            | Stmt::DeferClose { .. }
            | Stmt::Assign { .. }
            | Stmt::Return(..)
            | Stmt::Break(_)
            | Stmt::BreakValue(..)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(..)
            | Stmt::BreakLabelValue(..)
            | Stmt::ContinueLabel(..)
            | Stmt::Yield(..) => {}
        }
    }
}

fn collect_allocator_nested(
    body: &[Stmt],
    cx: &Cx,
    locals: &HashSet<String>,
    found: &mut HashSet<String>,
    extra: &[&str],
) {
    let mut scope = locals.clone();
    scope.extend(extra.iter().map(|name| (*name).to_string()));
    collect_allocator_constructors(body, cx, &mut scope, found);
}

fn collect_func_allocator_constructors(func: &Func, cx: &Cx, found: &mut HashSet<String>) {
    let mut locals = func.params.iter().map(|param| param.name.clone()).collect();
    collect_allocator_constructors(&func.body, cx, &mut locals, found);
}

fn allocator_constructor_types(items: &[Item], cx: &Cx) -> HashSet<String> {
    let mut found = HashSet::new();
    for item in items {
        match item {
            Item::Func(func) => collect_func_allocator_constructors(func, cx, &mut found),
            Item::Struct(def) => {
                for func in &def.methods {
                    collect_func_allocator_constructors(func, cx, &mut found);
                }
                for block in &def.trait_impls {
                    for func in &block.methods {
                        collect_func_allocator_constructors(func, cx, &mut found);
                    }
                }
            }
            Item::Enum(def) => {
                for func in &def.methods {
                    collect_func_allocator_constructors(func, cx, &mut found);
                }
                for block in &def.trait_impls {
                    for func in &block.methods {
                        collect_func_allocator_constructors(func, cx, &mut found);
                    }
                }
            }
            Item::Impl(def) => {
                for func in &def.methods {
                    collect_func_allocator_constructors(func, cx, &mut found);
                }
            }
            Item::Test(def) => {
                let mut locals = HashSet::new();
                collect_allocator_constructors(&def.body, cx, &mut locals, &mut found);
            }
            Item::Bench(def) => {
                let mut locals = HashSet::new();
                collect_allocator_constructors(&def.body, cx, &mut locals, &mut found);
            }
            Item::CodeModule(def) => {
                if let Some(body) = &def.body {
                    found.extend(allocator_constructor_types(body, cx));
                }
            }
            _ => {}
        }
    }
    found
}

pub(crate) fn emit_synthetic_close_builtin_impls(cx: &Cx, items: &[Item], out: &mut String) {
    let root = &cx.root_prefix;
    let uses = |module: &str| {
        cx.used_core.iter().any(|usage| {
            usage.strip_prefix(module).is_some_and(|suffix| {
                suffix.is_empty() || suffix.starts_with("::") || suffix.starts_with('.')
            })
        })
    };
    if uses("core.files") {
        for ty in [
            format!("{root}JetFileReader"),
            format!("{root}JetFileWriter"),
            format!("{root}jet_std::FileLock"),
        ] {
            out.push_str(&format!(
                "impl __jet_Close for {ty} {{ fn close(self) {{ drop(self); }} }}\n"
            ));
        }
    }
    if uses("core.net") {
        for ty in [format!("{root}JetTCPStream"), format!("{root}JetUnixStream")] {
            out.push_str(&format!(
                "impl __jet_Close for {ty} {{ fn close(self) {{ drop(self); }} }}\n"
            ));
        }
        out.push_str(&format!(
            "impl __jet_Close for {root}JetTLSStream {{ fn close(mut self) {{ let _ = {root}jet_net_tls_close(&mut self); }} }}\n"
        ));
    }
    let uses_mem = uses(crate::Syntax::CORE_MEM_MODULE)
        || uses(crate::Syntax::CORE_MEM_ALLOC_MODULE);
    let constructed_allocators = allocator_constructor_types(items, cx);
    for (name, ty) in [
        ("Arena", format!("{root}jet_mem::JetArena")),
        ("Bump", format!("{root}jet_mem::JetBump")),
        ("Pool", format!("{root}jet_mem::JetPool")),
        ("Fixed", format!("{root}jet_mem::JetFixed")),
    ] {
        if uses_mem || constructed_allocators.contains(name) {
            out.push_str(&format!(
                "impl __jet_Close for {ty} {{ fn close(self) {{ drop(self); }} }}\n"
            ));
        }
    }
    if uses("core.db") {
        // D-DBPOLICY-BIND1: `Driver` is a policy-bearing scope, never the raw
        // connection. Generic calls use the same policy-enforcing helpers as
        // concrete `DBScope` calls.
        out.push_str(&format!(
            "trait JetDBDriver {{\n\
             \tfn query(&mut self, sql: String, params: Vec<{root}jet_std::DBValue>) -> Result<Vec<{root}jet_std::JetDBRow>, {root}jet_std::DBError>;\n\
             \tfn query_one(&mut self, sql: String, params: Vec<{root}jet_std::DBValue>) -> Result<JetOutcome<{root}jet_std::JetDBRow, JetAbsent>, {root}jet_std::DBError>;\n\
             \tfn execute(&mut self, sql: String, params: Vec<{root}jet_std::DBValue>) -> Result<i64, {root}jet_std::DBError>;\n\
             \tfn begin(&mut self) -> bool;\n\
             \tfn commit(&mut self) -> bool;\n\
             \tfn rollback(&mut self) -> bool;\n\
             }}\n"
        ));
        if let Some(ffi) = &cx.ffi_crate {
            out.push_str(&format!(
                "impl JetDBDriver for {root}JetDbScope {{\n\
                 \tfn query(&mut self, sql: String, params: Vec<{root}jet_std::DBValue>) -> Result<Vec<{root}jet_std::JetDBRow>, {root}jet_std::DBError> {{\n\
                 \t\tjet_db_scope_query(self, &sql, &params)\n\
                 \t}}\n\
                 \tfn query_one(&mut self, sql: String, params: Vec<{root}jet_std::DBValue>) -> Result<JetOutcome<{root}jet_std::JetDBRow, JetAbsent>, {root}jet_std::DBError> {{\n\
                 \t\tjet_db_scope_query(self, &sql, &params).map({root}jet_std::jet_db_first_row)\n\
                 \t}}\n\
                 \tfn execute(&mut self, sql: String, params: Vec<{root}jet_std::DBValue>) -> Result<i64, {root}jet_std::DBError> {{\n\
                 \t\tjet_db_scope_execute(self, &sql, &params)\n\
                 \t}}\n\
                 \tfn begin(&mut self) -> bool {{ {ffi}::jet_db_begin(self.handle) }}\n\
                 \tfn commit(&mut self) -> bool {{ {ffi}::jet_db_commit(self.handle) }}\n\
                 \tfn rollback(&mut self) -> bool {{ {ffi}::jet_db_rollback(self.handle) }}\n\
                 }}\n"
            ));
            out.push_str(&format!(
                "fn jet_db_scope_execute(scope: &{root}JetDbScope, sql: &String, params: &Vec<{root}jet_std::DBValue>) -> Result<i64, {root}jet_std::DBError> {{\n\
let (__sql, __params) = {root}jet_std::jet_db_apply_policy(sql, params, &scope.policy.table, &scope.policy.expression, &scope.user)?;\n\
{root}jet_std::jet_db_decode_execute_result(&{ffi}::jet_db_execute(scope.handle, &__sql, &{root}jet_std::jet_db_encode_params(&__params)))\n\
}}\n\
fn jet_db_scope_query(scope: &{root}JetDbScope, sql: &String, params: &Vec<{root}jet_std::DBValue>) -> Result<Vec<{root}jet_std::JetDBRow>, {root}jet_std::DBError> {{\n\
let (__sql, __params) = {root}jet_std::jet_db_apply_policy(sql, params, &scope.policy.table, &scope.policy.expression, &scope.user)?;\n\
{root}jet_std::jet_db_decode_query_result(&{ffi}::jet_db_query(scope.handle, &__sql, &{root}jet_std::jet_db_encode_params(&__params)))\n\
}}\n"
            ));
            out.push_str(&format!(
                "fn jet_db_scope_execute_migration(scope: &{root}JetDbScope, sql: &String, params: &Vec<{root}jet_std::DBValue>) -> Result<i64, {root}jet_std::DBError> {{\n\
let (__sql, __params) = {root}jet_std::jet_db_apply_migration_policy(sql, params, &scope.policy.table, &scope.policy.expression, &scope.user)?;\n\
{root}jet_std::jet_db_decode_execute_result(&{ffi}::jet_db_execute(scope.handle, &__sql, &{root}jet_std::jet_db_encode_params(&__params)))\n\
}}\n\
fn jet_db_scope_query_migration(scope: &{root}JetDbScope, sql: &String, params: &Vec<{root}jet_std::DBValue>) -> Result<Vec<{root}jet_std::JetDBRow>, {root}jet_std::DBError> {{\n\
let (__sql, __params) = {root}jet_std::jet_db_apply_migration_policy(sql, params, &scope.policy.table, &scope.policy.expression, &scope.user)?;\n\
{root}jet_std::jet_db_decode_query_result(&{ffi}::jet_db_query(scope.handle, &__sql, &{root}jet_std::jet_db_encode_params(&__params)))\n\
}}\n"
            ));
            out.push_str(&format!(
                "struct JetDbScopeBackend<'a> {{ scope: &'a {root}JetDbScope }}\n\
impl {root}jet_std::JetDBBackend for JetDbScopeBackend<'_> {{\n\
fn begin(&mut self) -> bool {{ {ffi}::jet_db_begin(self.scope.handle) }}\n\
fn commit(&mut self) -> bool {{ {ffi}::jet_db_commit(self.scope.handle) }}\n\
fn rollback(&mut self) {{ let _ = {ffi}::jet_db_rollback(self.scope.handle); }}\n\
fn execute(&mut self, sql: &String, params: &Vec<{root}jet_std::DBValue>, allow_schema: bool) -> Result<i64, {root}jet_std::DBError> {{\n\
let (__sql, __params) = if allow_schema {{ {root}jet_std::jet_db_apply_migration_policy(sql, params, &self.scope.policy.table, &self.scope.policy.expression, &self.scope.user)? }} else {{ {root}jet_std::jet_db_apply_policy(sql, params, &self.scope.policy.table, &self.scope.policy.expression, &self.scope.user)? }};\n\
{root}jet_std::jet_db_decode_execute_result(&{ffi}::jet_db_execute(self.scope.handle, &__sql, &{root}jet_std::jet_db_encode_params(&__params)))\n\
}}\n\
fn query(&mut self, sql: &String, params: &Vec<{root}jet_std::DBValue>, allow_schema: bool) -> Result<Vec<{root}jet_std::JetDBRow>, {root}jet_std::DBError> {{\n\
let (__sql, __params) = if allow_schema {{ {root}jet_std::jet_db_apply_migration_policy(sql, params, &self.scope.policy.table, &self.scope.policy.expression, &self.scope.user)? }} else {{ {root}jet_std::jet_db_apply_policy(sql, params, &self.scope.policy.table, &self.scope.policy.expression, &self.scope.user)? }};\n\
{root}jet_std::jet_db_decode_query_result(&{ffi}::jet_db_query(self.scope.handle, &__sql, &{root}jet_std::jet_db_encode_params(&__params)))\n\
}}\n\
}}\n\
fn jet_db_scope_transaction(scope: &{root}JetDbScope, label: &String, steps: &Vec<String>) -> Result<i64, {root}jet_std::DBError> {{\n\
let mut backend = JetDbScopeBackend {{ scope }};\n\
{root}jet_std::jet_db_transaction(&mut backend, label, steps)\n\
}}\n"
            ));
            out.push_str(&format!(
                "fn jet_db_scope_migrate(scope: &{root}JetDbScope, name: &String, steps: &Vec<String>) -> Result<i64, {root}jet_std::DBError> {{\n\
let mut backend = JetDbScopeBackend {{ scope }};\n\
{root}jet_std::jet_db_migrate(&mut backend, name, steps)\n\
}}\n"
            ));
            out.push_str(&format!(
                "impl __jet_Close for {root}JetDbConnection {{ fn close(self) {{ let _ = {ffi}::jet_db_close(self.handle); }} }}\n"
            ));
            out.push_str(&format!(
                "impl __jet_Close for {root}JetDbScope {{ fn close(self) {{ let _ = {ffi}::jet_db_close(self.handle); }} }}\n"
            ));
        } else {
            out.push_str(&format!(
                "impl __jet_Close for {root}JetDbConnection {{ fn close(self) {{ drop(self); }} }}\nimpl __jet_Close for {root}JetDbScope {{ fn close(self) {{ drop(self); }} }}\n"
            ));
        }
    }
    if uses("core.vault") {
        if let Some(ffi) = &cx.ffi_crate {
            out.push_str(&format!(
                "impl<T> JetDisplay for {ffi}::JetVaultKeyRef<T> {{ fn jet_display(&self) -> String {{ self.to_string() }} }}\nimpl<T> JetDebug for {ffi}::JetVaultKeyRef<T> {{ fn jet_debug(&self) -> String {{ format!(\"{{self:?}}\") }} }}\nimpl JetDisplay for {ffi}::JetWrappedVaultKey {{ fn jet_display(&self) -> String {{ self.to_string() }} }}\nimpl JetDebug for {ffi}::JetWrappedVaultKey {{ fn jet_debug(&self) -> String {{ format!(\"{{self:?}}\") }} }}\n"
            ));
        }
    }
    out.push('\n');
}

/// D-ITER-HOOK / D-INDEX-HOOK: emit Iterable/Iterator/Index/IndexMut when used.
pub(crate) fn emit_synthetic_iter_index_traits(
    out: &mut String,
    has_iterable: bool,
    has_iterator: bool,
    has_index: bool,
    has_index_mut: bool,
) {
    if has_iterable {
        out.push_str("pub trait __jet_Iterable {\n");
        out.push_str("    type Iter;\n");
        out.push_str("    fn iter(self) -> Self::Iter;\n");
        out.push_str("}\n\n");
    }
    if has_iterator {
        out.push_str("pub trait __jet_Iterator {\n");
        out.push_str("    type Item;\n");
        out.push_str("    fn next(&mut self) -> JetOutcome<Self::Item, JetAbsent>;\n");
        out.push_str("}\n\n");
    }
    if has_index {
        out.push_str("pub trait __jet_Index {\n");
        out.push_str("    type Key;\n");
        out.push_str("    type Value;\n");
        out.push_str("    fn get(&self, k: Self::Key) -> JetOutcome<Self::Value, JetAbsent>;\n");
        out.push_str("}\n\n");
    }
    if has_index_mut {
        out.push_str("pub trait __jet_IndexMut: __jet_Index {\n");
        out.push_str("    fn set(&mut self, k: <Self as __jet_Index>::Key, v: <Self as __jet_Index>::Value);\n");
        out.push_str("}\n\n");
    }
}

pub(crate) fn program_iter_index_usage(items: &[Item]) -> (bool, bool, bool, bool) {
    let mut has_iterable = false;
    let mut has_iterator = false;
    let mut has_index = false;
    let mut has_index_mut = false;
    for item in items {
        match item {
            Item::Impl(i) => match i.trait_name.as_deref() {
                Some(Syntax::TRAIT_ITERABLE) => has_iterable = true,
                Some(Syntax::TRAIT_ITERATOR) => has_iterator = true,
                Some(Syntax::TRAIT_INDEX) => has_index = true,
                Some(Syntax::TRAIT_INDEX_MUT) => has_index_mut = true,
                _ => {}
            },
            Item::Struct(s) => {
                for block in &s.trait_impls {
                    match block.trait_name.as_str() {
                        Syntax::TRAIT_ITERABLE => has_iterable = true,
                        Syntax::TRAIT_ITERATOR => has_iterator = true,
                        Syntax::TRAIT_INDEX => has_index = true,
                        Syntax::TRAIT_INDEX_MUT => has_index_mut = true,
                        _ => {}
                    }
                }
            }
            Item::Enum(e) => {
                for block in &e.trait_impls {
                    match block.trait_name.as_str() {
                        Syntax::TRAIT_ITERABLE => has_iterable = true,
                        Syntax::TRAIT_ITERATOR => has_iterator = true,
                        Syntax::TRAIT_INDEX => has_index = true,
                        Syntax::TRAIT_INDEX_MUT => has_index_mut = true,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    (has_iterable, has_iterator, has_index, has_index_mut)
}

/// D-TXN-ROLLBACK layer 2: emit the `trait __jet_Rollback { … }` Rust trait
/// declaration when any impl block in the program references `Rollback`. Programs
/// with no `Rollback` impl produce zero output here (byte-identical to before).
pub(crate) fn emit_synthetic_rollback_trait(out: &mut String) {
    out.push_str("pub trait __jet_Rollback {\n");
    out.push_str("    type Snapshot;\n");
    out.push_str("    fn snapshot(&self) -> Self::Snapshot;\n");
    out.push_str("    fn restore(&mut self, _snap: Self::Snapshot);\n");
    out.push_str("}\n\n");
}

pub(crate) fn program_has_rollback_impl(items: &[Item]) -> bool {
    items.iter().any(|i| match i {
        Item::Impl(im) => im.trait_name.as_deref() == Some(Syntax::TRAIT_ROLLBACK),
        Item::Struct(s) => s
            .trait_impls
            .iter()
            .any(|b| b.trait_name == Syntax::TRAIT_ROLLBACK),
        Item::Enum(e) => e
            .trait_impls
            .iter()
            .any(|b| b.trait_name == Syntax::TRAIT_ROLLBACK),
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
    for item in &prog.items {
        if let Item::CodeModule(module) = item {
            if let Some(identity) = &module.instance_identity {
                out.push_str(&format!("// jet:generic-instance module={} fingerprint={} full-key={}\n", module.name, identity.fingerprint, identity.full_key.iter().map(|byte| format!("{byte:02x}")).collect::<String>()));
            }
        }
    }
    out.push_str("#![allow(warnings)]\n\n");
    push_ffi_reporter(&mut out, None);
    push_prelude(&mut out);
    out.push_str(ENV_INIT_PRELUDE);
    push_mem_prelude(&mut out);
    push_gc_prelude(&mut out);
    out.push_str(LAYOUT_PRELUDE);
    out.push('\n');

    let cx = build_cx(prog, src, file);
    let tuple_shapes = collect_tuple_shapes(&prog.items);
    emit_tuple_structs(&cx, &tuple_shapes, &mut out);
    emit_anonymous_unions(&cx, &prog.items, &mut out);

    emit_synthetic_display_trait(&mut out, true);
    emit_synthetic_operator_traits(&mut out, true);
    emit_synthetic_close_trait(&mut out);
    emit_synthetic_close_builtin_impls(&cx, &prog.items, &mut out);
    let (hi, hj, hk, hm) = program_iter_index_usage(&prog.items);
    emit_synthetic_iter_index_traits(&mut out, hi, hj, hk, hm);

    // D-TXN-ROLLBACK layer 2: emit the synthetic Rollback trait iff needed.
    if program_has_rollback_impl(&prog.items) {
        emit_synthetic_rollback_trait(&mut out);
    }

    for item in &prog.items {
        match item {
            Item::Trait(t) => Traits::emit_trait_def(t, &mut out, |ty, assoc| {
                cx.rust_type_with_view_lifetime_assoc(ty, assoc)
            }),
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
            Item::CModule(cm) => emit_c_module(&cx, cm, &mut out),
            Item::EffectDecl(_)
            | Item::MarkerDecl(_)
            | Item::FactDecl(_)
            | Item::Func(_) | Item::Impl(_) | Item::Test(_) | Item::Bench(_) | Item::ExternRust(_)
            | Item::Module(_) | Item::CodeModule(_) | Item::ErrorConv(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::TypeAlias(_) // D-TYPEALIAS1: erases
            | Item::Migration(_) // D-MIGRATE1: migration is sema-only (I3)
            | Item::StateDecl(_) // D-STATE-DECL: state-set decls erase (I3)
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: erase (expanded in sema)
            | Item::GenericModule(_) // D-CONF-GENSPELL1=A: template — erases
            | Item::ModuleAlias(_) => {} // D-CONF-GENSPELL1=A: alias — erases after expansion
        }
    }

    for item in &prog.items {
        match item {
            Item::Struct(s) => {
                emit_type_impl(&cx, &s.name, &s.type_params, &s.methods, &mut out);
                for block in &s.trait_impls {
                    emit_trait_impl(&cx, &s.name, &s.type_params, block, Some(s), &mut out);
                }
            }
            Item::Enum(e) => {
                emit_type_impl(&cx, &e.name, &e.type_params, &e.methods, &mut out);
                for block in &e.trait_impls {
                    emit_trait_impl(&cx, &e.name, &e.type_params, block, None, &mut out);
                }
            }
            Item::Impl(i) => {
                // D-OSTARGET1=A: skip an `impl` gated to a non-active native OS.
                if i.os_target.is_some_and(|os| os != cx.active_os) {
                    continue;
                }
                if i.trait_name.is_some() {
                    let struct_def = prog.items.iter().find_map(|item| match item {
                        Item::Struct(s) if s.name == i.type_name => Some(s),
                        _ => None,
                    });
                    emit_external_trait_impl(&cx, i, struct_def, &mut out);
                } else {
                    emit_type_impl(
                        &cx,
                        &i.type_name,
                        type_params_for_name(&prog.items, &i.type_name),
                        &i.methods,
                        &mut out,
                    );
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
    // S12/D-CLIFLAG1: Jet's only program entry is `fn run`; Rust still needs
    // `fn main`, so synthesize that wrapper for zero-arg and typed-CLI forms.
    emit_cli_entry_if_needed(&cx, &prog.items, &prog.items, &mut out);
    strip_unused_os_signal_prelude(strip_unused_raylib_prelude(strip_unused_term_prelude(strip_unused_gc_prelude(
        strip_unused_txn_prelude(strip_unused_mem_prelude(out)),
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aot_ffi_host_installs_bridge_reporter() {
        let link = FfiLink {
            crate_name: "jet_ffi_fixture".into(),
            rlib_path: "fixture.rlib".into(),
            cdylib_path: "fixture.so".into(),
            target_deps_dir: "deps".into(),
            host_deps_dir: "deps".into(),
            helper_bin_path: None,
            secrets_helper_bin_path: None,
        };
        let mut source = String::new();
        push_ffi_reporter(&mut source, Some(&link));
        assert!(source.contains("jet_ffi_fixture::jet_ffi_set_reporter(jet_ffi_reporter)"));
        assert!(source.contains("panic: a foreign function panicked"));

        let mut cached = String::new();
        push_cached_runtime(&mut cached, Some(&link));
        let reporter = cached
            .find("jet_ffi_fixture::jet_ffi_set_reporter(jet_ffi_reporter)")
            .unwrap();
        let marker = cached.find(CACHED_RUNTIME_BEGIN).unwrap();
        assert!(reporter < marker);
        assert!(!cached[marker..].contains("jet_ffi_fixture::jet_ffi_set_reporter"));
    }
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    #[test]
    fn gc_runtime_source_is_shared_by_aot_and_jit() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let runtime = std::fs::read_to_string(root.join("../jet-rt/src/__gc.rs")).unwrap();
        assert_eq!(GC_RUNTIME_PRELUDE, runtime);

        let mut emitted = String::new();
        push_gc_prelude(&mut emitted);
        assert!(emitted.starts_with("mod jet_gc {\n"));
        assert_eq!(emitted.matches(runtime.as_str()).count(), 1);

        let unused = strip_unused_gc_prelude(format!("{emitted}fn main() {{}}\n"));
        assert!(!unused.contains("mod jet_gc"));
        let used = strip_unused_gc_prelude(format!(
            "{emitted}fn main() {{ jet_gc::runtime_or_exit(jet_gc::initialize_trace()); }}\n"
        ));
        assert!(used.contains("mod jet_gc"));
    }

    #[test]
    fn r10_corelib_emits_only_reachable_top_modules() {
        // R10 / #1133: a files-only program must not drag HTTP/Browser/Game
        // templates into generated source.
        let files_only = HashSet::from(["core.files::read".to_string()]);
        let mut files_out = String::new();
        push_corelib_prelude(&mut files_out, &files_only, false);
        assert!(
            files_out.contains("struct JetPath"),
            "files usage must emit PathFiles"
        );
        assert!(
            !files_out.contains("JetHTTPServer")
                && !files_out.contains("struct JetBrowser")
                && !files_out.contains("fn jet_game_"),
            "files-only Core must not emit HTTP server/Browser/Game templates"
        );
        assert!(
            files_out.contains("struct JetTCPStream"),
            "JetStd kernel closure always needs JetTCPStream"
        );

        let net_only = HashSet::from(["core.net::tcp_connect".to_string()]);
        let mut net_out = String::new();
        push_corelib_prelude(&mut net_out, &net_only, false);
        assert!(
            net_out.contains("struct JetTCPStream") && net_out.contains("fn jet_net_tcp_connect"),
            "net usage must emit NetHTTP"
        );
        assert!(
            !net_out.contains("JetHTTPServer") && !net_out.contains("struct JetBrowser"),
            "pure core.net must not emit HTTP server or Browser templates"
        );

        let http = HashSet::from(["core.http.client::get".to_string()]);
        let mut http_out = String::new();
        push_corelib_prelude(&mut http_out, &http, false);
        assert!(
            http_out.contains("JetHTTPServer") || http_out.contains("fn jet_http_"),
            "http usage must emit HTTP templates"
        );

        let files_fp = corelib_emission_fingerprint(&files_only);
        let net_fp = corelib_emission_fingerprint(&net_only);
        assert_ne!(
            files_fp, net_fp,
            "R10 cache identity must differ when Top-module reachability differs"
        );
        assert_eq!(
            files_fp,
            corelib_emission_fingerprint(&files_only),
            "R10 fingerprint must be stable for the same used_core set"
        );

        let compute_only = HashSet::from(["core.compute::zeros".to_string()]);
        let mut compute_out = String::new();
        push_corelib_prelude(&mut compute_out, &compute_only, false);
        assert!(
            compute_out.contains("fn jet_compute_zeros")
                && compute_out.contains("struct JetTensor"),
            "core.compute usage must emit Compute.rs"
        );
        assert!(
            !compute_out.contains("JetHTTPServer") && !compute_out.contains("struct JetBrowser"),
            "compute-only Core must not emit HTTP/Browser templates"
        );
    }

    #[test]
    fn core_prelude_stays_split_by_runtime_ownership() {
        const MAX_MODULE_LINES: usize = 2500;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let outcome =
            std::fs::read_to_string(root.join("../jet-foundation/src/Outcome.rs")).unwrap();
        let job = std::fs::read_to_string(root.join("src/Prelude/Job.rs")).unwrap();
        let option = std::fs::read_to_string(root.join("src/Prelude/Core/Option.rs")).unwrap();
        let fixed_list =
            std::fs::read_to_string(root.join("src/Prelude/Core/FixedList.rs")).unwrap();
        let float_provenance = std::fs::read_to_string(
            root.join("src/Prelude/Core/FloatProvenance.rs"),
        )
        .unwrap();
        let unicode =
            std::fs::read_to_string(root.join("src/Prelude/Core/UnicodeString.rs")).unwrap();
        let string_concat =
            std::fs::read_to_string(root.join("src/Prelude/Core/StringConcat.rs")).unwrap();
        let loadable =
            std::fs::read_to_string(root.join("src/Prelude/Core/Loadable.rs")).unwrap();
        let values = std::fs::read_to_string(root.join("src/Prelude/Core/Values.rs")).unwrap();
        let range_bounds =
            std::fs::read_to_string(root.join("src/Prelude/Core/RangeBounds.rs")).unwrap();
        let disjoint =
            std::fs::read_to_string(root.join("src/Prelude/Core/Disjoint.rs")).unwrap();
        let expiring_secret =
            std::fs::read_to_string(root.join("src/Prelude/Core/ExpiringSecret.rs")).unwrap();
        let set_algebra =
            std::fs::read_to_string(root.join("src/Prelude/Core/SetAlgebra.rs")).unwrap();
        let duration =
            std::fs::read_to_string(root.join("src/Prelude/Core/Duration.rs")).unwrap();
        let measurement =
            std::fs::read_to_string(root.join("src/Prelude/Core/Measurement.rs")).unwrap();
        let time_monotonic =
            std::fs::read_to_string(root.join("src/Prelude/Core/TimeMonotonic.rs")).unwrap();
        let time = std::fs::read_to_string(root.join("src/Prelude/Core/Time.rs")).unwrap();
        let sketch = std::fs::read_to_string(root.join("src/Prelude/Core/Sketch.rs")).unwrap();
        let contracts =
            std::fs::read_to_string(root.join("src/Prelude/Core/Contracts.rs")).unwrap();
        let core = std::fs::read_to_string(root.join("src/Prelude/Core.rs")).unwrap();
        let view_access =
            std::fs::read_to_string(root.join("src/Prelude/Core/ViewAccess.rs")).unwrap();
        let power = std::fs::read_to_string(root.join("src/Prelude/Core/Power.rs")).unwrap();
        let division =
            std::fs::read_to_string(root.join("src/Prelude/Core/Division.rs")).unwrap();
        let typed_text =
            std::fs::read_to_string(root.join("src/Prelude/TypedText.rs")).unwrap();
        let progress =
            std::fs::read_to_string(root.join("src/Prelude/Core/Progress.rs")).unwrap();
        let byte_buffer =
            std::fs::read_to_string(root.join("src/Prelude/Core/ByteBuffer.rs")).unwrap();
        let collections =
            std::fs::read_to_string(root.join("src/Prelude/Core/Collections.rs")).unwrap();
        let shared_protocol =
            std::fs::read_to_string(root.join("src/Prelude/SharedProtocol.rs")).unwrap();
        let runtime_control =
            std::fs::read_to_string(root.join("src/Prelude/Core/RuntimeControl.rs")).unwrap();
        let numeric_widen =
            std::fs::read_to_string(root.join("src/Prelude/NumericWiden.rs")).unwrap();
        let observe = std::fs::read_to_string(root.join("src/Prelude/Observe.rs")).unwrap();
        let exact_units =
            std::fs::read_to_string(root.join("../jet-foundation/src/ExactUnitConversion.rs"))
                .unwrap();
        let structural_debug =
            std::fs::read_to_string(root.join("../jet-foundation/src/StructuralDebug.rs")).unwrap();
        let stream_cursor =
            std::fs::read_to_string(root.join("../jet-foundation/src/StreamCursor.rs")).unwrap();
        for (relative, source) in [
            ("../jet-foundation/src/Outcome.rs", outcome.as_str()),
            ("src/Prelude/Job.rs", job.as_str()),
            ("src/Prelude/Core/Option.rs", option.as_str()),
            ("src/Prelude/Core/FixedList.rs", fixed_list.as_str()),
            (
                "src/Prelude/Core/FloatProvenance.rs",
                float_provenance.as_str(),
            ),
            ("src/Prelude/Core/UnicodeString.rs", unicode.as_str()),
            ("src/Prelude/Core/StringConcat.rs", string_concat.as_str()),
            ("src/Prelude/Core/Loadable.rs", loadable.as_str()),
            ("src/Prelude/Core/Values.rs", values.as_str()),
            ("src/Prelude/Core/RangeBounds.rs", range_bounds.as_str()),
            ("src/Prelude/Core/Disjoint.rs", disjoint.as_str()),
            (
                "src/Prelude/Core/ExpiringSecret.rs",
                expiring_secret.as_str(),
            ),
            ("src/Prelude/Core/SetAlgebra.rs", set_algebra.as_str()),
            ("src/Prelude/Core/Duration.rs", duration.as_str()),
            ("src/Prelude/Core/Measurement.rs", measurement.as_str()),
            (
                "src/Prelude/Core/TimeMonotonic.rs",
                time_monotonic.as_str(),
            ),
            ("src/Prelude/Core/Time.rs", time.as_str()),
            ("src/Prelude/Core/Sketch.rs", sketch.as_str()),
            ("src/Prelude/Core/Contracts.rs", contracts.as_str()),
            ("src/Prelude/Core.rs", core.as_str()),
            ("src/Prelude/Core/ViewAccess.rs", view_access.as_str()),
            ("src/Prelude/Core/Power.rs", power.as_str()),
            ("src/Prelude/Core/Division.rs", division.as_str()),
            ("src/Prelude/TypedText.rs", typed_text.as_str()),
            ("src/Prelude/Core/Progress.rs", progress.as_str()),
            ("src/Prelude/Core/ByteBuffer.rs", byte_buffer.as_str()),
            ("src/Prelude/Core/Collections.rs", collections.as_str()),
            ("src/Prelude/SharedProtocol.rs", shared_protocol.as_str()),
            (
                "src/Prelude/Core/RuntimeControl.rs",
                runtime_control.as_str(),
            ),
            ("src/Prelude/NumericWiden.rs", numeric_widen.as_str()),
            ("src/Prelude/Observe.rs", observe.as_str()),
            (
                "../jet-foundation/src/ExactUnitConversion.rs",
                exact_units.as_str(),
            ),
            (
                "../jet-foundation/src/StructuralDebug.rs",
                structural_debug.as_str(),
            ),
            (
                "../jet-foundation/src/StreamCursor.rs",
                stream_cursor.as_str(),
            ),
        ] {
            assert!(
                source.lines().count() < MAX_MODULE_LINES,
                "{relative} must stay below the card #510 module boundary"
            );
            assert!(
                !source.contains("include!(") && !source.contains("#[path"),
                "{relative} must remain owned source, never a code-splice shell"
            );
        }

        let codegen = std::fs::read_to_string(root.join("src/Codegen/mod.rs")).unwrap();
        let production_codegen = codegen.split("#[cfg(test)]\nmod tests").next().unwrap();
        let outcome_pos = production_codegen
            .find("include_str!(\"../../../jet-foundation/src/Outcome.rs\")")
            .unwrap();
        let job_pos = production_codegen
            .find("include_str!(\"../Prelude/Job.rs\")")
            .unwrap();
        let option_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Option.rs\")")
            .unwrap();
        let fixed_list_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/FixedList.rs\")")
            .unwrap();
        let float_provenance_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/FloatProvenance.rs\")")
            .unwrap();
        let unicode_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/UnicodeString.rs\")")
            .unwrap();
        let string_concat_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/StringConcat.rs\")")
            .unwrap();
        let loadable_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Loadable.rs\")")
            .unwrap();
        let values_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Values.rs\")")
            .unwrap();
        let range_bounds_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/RangeBounds.rs\")")
            .unwrap();
        let disjoint_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Disjoint.rs\")")
            .unwrap();
        let expiring_secret_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/ExpiringSecret.rs\")")
            .unwrap();
        let set_algebra_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/SetAlgebra.rs\")")
            .unwrap();
        let duration_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Duration.rs\")")
            .unwrap();
        let measurement_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Measurement.rs\")")
            .unwrap();
        let time_monotonic_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/TimeMonotonic.rs\")")
            .unwrap();
        let time_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Time.rs\")")
            .unwrap();
        let time_monotonic_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/TimeMonotonic.rs\")")
            .unwrap();
        let sketch_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Sketch.rs\")")
            .unwrap();
        let contracts_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Contracts.rs\")")
            .unwrap();
        let core_pos = production_codegen
            .find("include_str!(\"../Prelude/Core.rs\")")
            .unwrap();
        let view_access_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/ViewAccess.rs\")")
            .unwrap();
        let collections_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/Collections.rs\")")
            .unwrap();
        let control_pos = production_codegen
            .find("include_str!(\"../Prelude/Core/RuntimeControl.rs\")")
            .unwrap();
        let observe_pos = production_codegen
            .find("include_str!(\"../Prelude/Observe.rs\")")
            .unwrap();
        let exact_units_pos = production_codegen
            .find("include_str!(\"../../../jet-foundation/src/ExactUnitConversion.rs\")")
            .unwrap();
        let structural_debug_pos = production_codegen
            .find("include_str!(\"../../../jet-foundation/src/StructuralDebug.rs\")")
            .unwrap();
        let stream_cursor_pos = production_codegen
            .find("include_str!(\"../../../jet-foundation/src/StreamCursor.rs\")")
            .unwrap();
        assert!(
            outcome_pos < unicode_pos
                && outcome_pos < job_pos
                && job_pos < option_pos
                && outcome_pos < option_pos
                && option_pos < fixed_list_pos
                && fixed_list_pos < float_provenance_pos
                && float_provenance_pos < unicode_pos
                && unicode_pos < loadable_pos
                && unicode_pos < string_concat_pos
                && string_concat_pos < loadable_pos
                && loadable_pos < values_pos
                && values_pos < range_bounds_pos
                && range_bounds_pos < disjoint_pos
                && disjoint_pos < expiring_secret_pos
                && expiring_secret_pos < set_algebra_pos
                && set_algebra_pos < duration_pos
                && duration_pos < measurement_pos
                && measurement_pos < time_monotonic_pos
                && time_monotonic_pos < time_pos
                && measurement_pos < time_pos
                && time_pos < sketch_pos
                && sketch_pos < contracts_pos
                && contracts_pos < core_pos
                && sketch_pos < core_pos
                && core_pos < view_access_pos
                && view_access_pos < collections_pos
                && collections_pos < control_pos
                && control_pos < observe_pos
                && observe_pos < exact_units_pos
                && exact_units_pos < structural_debug_pos
                && structural_debug_pos < stream_cursor_pos,
            "prelude ownership order is generated-byte order"
        );
        assert!(production_codegen.contains("for part in PRELUDE_PARTS"));
        assert!(!production_codegen.contains("include!("));
        assert_eq!(
            PRELUDE_PARTS,
            [
                outcome.as_str(),
                job.as_str(),
                option.as_str(),
                fixed_list.as_str(),
                float_provenance.as_str(),
                unicode.as_str(),
                string_concat.as_str(),
                loadable.as_str(),
                values.as_str(),
                range_bounds.as_str(),
                disjoint.as_str(),
                expiring_secret.as_str(),
                set_algebra.as_str(),
                duration.as_str(),
                measurement.as_str(),
                time_monotonic.as_str(),
                time.as_str(),
                sketch.as_str(),
                contracts.as_str(),
                core.as_str(),
                view_access.as_str(),
                power.as_str(),
                division.as_str(),
                typed_text.as_str(),
                progress.as_str(),
                byte_buffer.as_str(),
                collections.as_str(),
                shared_protocol.as_str(),
                runtime_control.as_str(),
                numeric_widen.as_str(),
                observe.as_str(),
                exact_units.as_str(),
                structural_debug.as_str(),
                stream_cursor.as_str(),
            ],
            "PRELUDE_PARTS must list every owned module exactly once in generated-byte order"
        );

        let mut emitted = String::new();
        push_prelude(&mut emitted);
        let expected = [
            outcome.as_str(),
            job.as_str(),
            option.as_str(),
            fixed_list.as_str(),
            float_provenance.as_str(),
            unicode.as_str(),
            string_concat.as_str(),
            loadable.as_str(),
            values.as_str(),
            range_bounds.as_str(),
            disjoint.as_str(),
            expiring_secret.as_str(),
            set_algebra.as_str(),
            duration.as_str(),
            measurement.as_str(),
            time_monotonic.as_str(),
            time.as_str(),
            sketch.as_str(),
            contracts.as_str(),
            core.as_str(),
            view_access.as_str(),
            power.as_str(),
            division.as_str(),
            typed_text.as_str(),
            progress.as_str(),
            byte_buffer.as_str(),
            collections.as_str(),
            shared_protocol.as_str(),
            runtime_control.as_str(),
            numeric_widen.as_str(),
            observe.as_str(),
            exact_units.as_str(),
            structural_debug.as_str(),
            stream_cursor.as_str(),
        ]
        .concat();
        assert_eq!(
            emitted, expected,
            "owned prelude modules must concatenate without byte loss or boundary changes"
        );
        assert_eq!(emitted.len(), 407_169, "split changed prelude byte length");
        assert_eq!(
            crate::SHA256::sha256_hex(emitted.as_bytes()),
            "4c8f22c0585ae088291188c5c78b656e7ae91d886cc9b22d96bcf7a336eb80f2",
            "split changed prelude bytes, order, or boundary newline"
        );
    }

    #[test]
    fn cached_runtime_block_covers_every_fixed_runtime_source_part() {
        let mut emitted = String::new();
        push_cached_runtime(&mut emitted, None);
        let body = emitted
            .strip_prefix(CACHED_RUNTIME_BEGIN)
            .and_then(|source| source.strip_suffix(CACHED_RUNTIME_END))
            .expect("cached runtime markers must enclose one exact block");
        assert!(body.contains("fn jet_ffi_install_reporter() {}"));
        for part in PRELUDE_PARTS {
            assert!(
                body.contains(part),
                "every PRELUDE_PARTS byte string must affect the runtime cache key"
            );
        }
        for part in [
            ENV_INIT_PRELUDE,
            UNINIT_PRELUDE,
            MEM_PRELUDE,
            GC_RUNTIME_PRELUDE,
            LAYOUT_PRELUDE,
        ] {
            assert!(
                body.contains(part),
                "every fixed runtime source part must affect the runtime cache key"
            );
        }
        assert_eq!(emitted.matches(CACHED_RUNTIME_BEGIN).count(), 1);
        assert_eq!(emitted.matches(CACHED_RUNTIME_END).count(), 1);

        let program = format!("{emitted}fn main() {{}}\n");
        let pruned = strip_unused_term_prelude(strip_unused_gc_prelude(
            strip_unused_txn_prelude(strip_unused_mem_prelude(program)),
        ));
        assert!(
            pruned.starts_with(emitted.as_str()),
            "program-specific pruning must not create runtime-cache variants"
        );
    }

    fn checked_generic_bundle(src: &str, root: &str) -> crate::AST::ProgramBundle {
        let (tokens, lex) = crate::Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let mut program = crate::Parser::parse(&tokens).expect("parse");
        let mut bundle = crate::AST::ProgramBundle {
            entry: 0,
            project_root: PathBuf::from(root),
            modules: vec![crate::AST::LoadedModule {
                path: PathBuf::from(root).join("main.jet"), display: "main.jet".into(), source: src.into(), alias: "main".into(),
                imports: std::mem::take(&mut program.imports), items: std::mem::take(&mut program.items), script_body: std::mem::take(&mut program.script_body),
                block_spans: std::mem::take(&mut program.block_spans),
                web_target_ceiling: program.web_target_ceiling, pub_file: program.pub_file,
                no_prelude: program.no_prelude,
                default_target: program.default_target, html_path: program.html_path,
                no_alloc_policy: program.no_alloc_policy,
                policy_declarations: program.policy_declarations.clone(),
                rule_facts: std::mem::take(&mut program.rule_facts),
            }],
            parse_teaching: Vec::new(), used_core: HashSet::new(), ffi_callback_fns: HashSet::new(), cffi: crate::AST::CFfi::default(),
            comptime_inputs: Vec::new(), name_ledger: crate::AST::NameLedger::default(), layer_ceiling: None,
            inferred_layer: crate::Syntax::RuntimeLayer::Core, web_partitions: HashMap::new(),
            web_partition_enforced: false, web_partition_report: None, dep_roots: HashMap::new(),
            active_os: crate::Syntax::OSTarget::host(),
            build_facts: Default::default(),
            edition: "2027".to_string(),
        };
        let diagnostics = crate::Sema::check_bundle(&mut bundle, CompileMode::Run);
        assert!(!diagnostics.iter().any(|d| d.severity == crate::Diagnostics::Severity::Error), "{diagnostics:#?}");
        bundle
    }

    #[test]
    fn generic_instance_provenance_reaches_tir_and_generated_rust() {
        let source = "module boxed<T>(n: Int) { fn value() => Int { return n } }\nmodule a :: boxed<Int>(3)\nmodule b :: boxed<Int>(3)\nfn run() {}";
        let bundle = checked_generic_bundle(source, "pkg-a");
        let fingerprint = bundle.modules[0].items.iter().find_map(|item| match item {
            crate::AST::Item::CodeModule(module) => module.instance_identity.as_ref().map(|identity| identity.fingerprint.clone()),
            _ => None,
        }).expect("instance identity");
        let tir = crate::Codegen::TIR::lower_jit_program(&bundle).expect("JIT TIR");
        let expected = crate::Codegen::TIR::instance_provenance(&bundle);
        assert_eq!(tir.instance_provenance, expected);
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0].fingerprint, fingerprint);
        assert!(!expected[0].full_key_hex.is_empty());
        let rust = emit_bundle(&bundle, CompileMode::Run, None);
        assert_eq!(rust.matches("// jet:generic-instance").count(), 1);
        assert!(rust.contains(&format!("fingerprint={fingerprint}")));
        assert!(rust.contains(&format!("module={} fingerprint={} full-key={}", expected[0].canonical_module, expected[0].fingerprint, expected[0].full_key_hex)));

        let semantic_edit = checked_generic_bundle(
            "module boxed<T>(n: Int) { fn value() => Int { return n + 1 } }\nmodule a :: boxed<Int>(3)\nfn run() {}", "pkg-a");
        let edited_rust = emit_bundle(&semantic_edit, CompileMode::Run, None);
        assert!(edited_rust.contains(&format!("fingerprint={fingerprint}")), "body shape is a cache input, not nominal instance identity");
        assert_ne!(rust, edited_rust, "semantic body edits still change generated code/cache material");

        let distinct = checked_generic_bundle(
            "module boxed<T>(n: Int) { fn value() => Int { return n } }\nmodule a :: boxed<Int>(3)\nmodule b :: boxed<Int>(4)\nfn run() {}", "pkg-a");
        let distinct_tir = crate::Codegen::TIR::lower_jit_program(&distinct).expect("distinct JIT TIR");
        assert_eq!(distinct_tir.instance_provenance, crate::Codegen::TIR::instance_provenance(&distinct));
        assert_eq!(distinct_tir.instance_provenance.len(), 2);
        assert_ne!(distinct_tir.instance_provenance[0].full_key_hex, distinct_tir.instance_provenance[1].full_key_hex);
    }

    #[test]
    fn raylib_window_example_reaches_sema_and_codegen() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let shown = "examples/features/game/raylib_window.jet";
        let path = root.join(shown);
        let src = std::fs::read_to_string(&path).expect("raylib example exists");

        let (toks, lex_diags) = crate::Lexer::lex(&src);
        assert!(lex_diags.is_empty(), "lex diagnostics: {lex_diags:?}");
        let mut prog = crate::Parser::parse(&toks).expect("raylib example parses");
        let mut bundle = crate::AST::ProgramBundle {
            entry: 0,
            project_root: root,
            modules: vec![crate::AST::LoadedModule {
                path,
                display: shown.to_string(),
                source: src,
                alias: "main".to_string(),
                imports: std::mem::take(&mut prog.imports),
                items: std::mem::take(&mut prog.items),
                script_body: std::mem::take(&mut prog.script_body),
                block_spans: std::mem::take(&mut prog.block_spans),
                web_target_ceiling: prog.web_target_ceiling,
                pub_file: prog.pub_file,
                no_prelude: prog.no_prelude,
                default_target: prog.default_target,
                html_path: prog.html_path.clone(),
                no_alloc_policy: prog.no_alloc_policy,
                policy_declarations: prog.policy_declarations.clone(),
                rule_facts: std::mem::take(&mut prog.rule_facts),
            }],
            parse_teaching: Vec::new(),
            used_core: HashSet::new(),
            ffi_callback_fns: HashSet::new(),
            cffi: crate::AST::CFfi::default(),
            comptime_inputs: Vec::new(),
            name_ledger: crate::AST::NameLedger::default(),
            layer_ceiling: None,
            inferred_layer: crate::Syntax::RuntimeLayer::Core,
            web_partitions: HashMap::new(),
            web_partition_enforced: false,
            web_partition_report: None,
            dep_roots: HashMap::new(),
            active_os: crate::Syntax::OSTarget::host(),
            build_facts: Default::default(),
            edition: "2027".to_string(),
        };

        let diags = crate::Sema::check_bundle(&mut bundle, CompileMode::Run);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == crate::Diagnostics::Severity::Error)
            .collect();
        assert!(errors.is_empty(), "sema diagnostics: {errors:?}");

        let rust = emit_bundle(&bundle, CompileMode::Run, None);
        assert!(rust.contains("fn main()"), "generated Rust has no main");
        assert!(rust.contains("jet_raylib_window_open"));
        assert!(rust.contains(&format!("let {}: RaylibWindow", mangle("window"))));
        assert!(
            !rust.contains("jet_std::Raylib"),
            "raylib bridge handles must lower to top-level prelude types"
        );
        assert!(rust.contains("jet_raylib_begin_drawing"));
        assert!(rust.contains("jet_raylib_draw_rectangle"));
        assert!(rust.contains("jet_raylib_draw_text"));
        assert!(rust.contains("jet_raylib_key_down"));
        assert!(rust.contains("jet_raylib_set_target_fps"));
        assert!(rust.contains("jet_raylib_close_window"));
        assert!(rust.contains("dlopen"));
        assert!(rust.contains("JET_RAYLIB_DISPLAY"));
        assert!(
            !rust.contains("unsafe fn __jet_"),
            "raylib user functions must stay safe; unsafe is confined to the vetted bridge"
        );
    }
}

struct TestCase<'a> {
    test: &'a TestDef,
    /// Rust module path that owns the test. `None` means the generated root.
    module: Option<String>,
    index: usize,
}

fn test_fn_path(test: &TestCase<'_>) -> String {
    let name = format!("jet_test_{}", test.index);
    test.module
        .as_deref()
        .map_or(name.clone(), |module| format!("{module}::{name}"))
}

/// Emit a test harness binary: all definitions plus one `main` that runs
/// every `#Test "…" { }` block (M6 phase 2).
pub fn emit_tests(prog: &Program, src: &str, file: &str) -> String {
    let tests: Vec<TestCase<'_>> = prog
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Test(t) => Some(t),
            _ => None,
        })
        .enumerate()
        .map(|(index, test)| TestCase {
            test,
            module: None,
            index,
        })
        .collect();
    assert!(!tests.is_empty(), "emit_tests called with no test blocks");

    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} test harness — do not edit.\n",
        Syntax::BINARY_NAME
    ));
    out.push_str("#![allow(warnings)]\n\n");
    push_ffi_reporter(&mut out, None);
    push_prelude(&mut out);
    out.push_str(ENV_INIT_PRELUDE);
    push_mem_prelude(&mut out);
    push_gc_prelude(&mut out);
    out.push_str(LAYOUT_PRELUDE);
    out.push_str(TEST_PRELUDE);
    out.push_str(TEST_REPORT_PRELUDE);
    if any_property_test(&tests) {
        out.push_str(PROP_PRELUDE);
    }
    out.push('\n');

    let mut cx = build_cx(prog, src, file);
    cx.test_mode = true;
    let tuple_shapes = collect_tuple_shapes(&prog.items);
    emit_tuple_structs(&cx, &tuple_shapes, &mut out);
    emit_anonymous_unions(&cx, &prog.items, &mut out);

    emit_synthetic_display_trait(&mut out, true);
    emit_synthetic_operator_traits(&mut out, true);
    emit_synthetic_close_trait(&mut out);
    emit_synthetic_close_builtin_impls(&cx, &prog.items, &mut out);
    let (hi, hj, hk, hm) = program_iter_index_usage(&prog.items);
    emit_synthetic_iter_index_traits(&mut out, hi, hj, hk, hm);

    // D-TXN-ROLLBACK layer 2: emit the synthetic Rollback trait iff needed.
    if program_has_rollback_impl(&prog.items) {
        emit_synthetic_rollback_trait(&mut out);
    }

    for item in &prog.items {
        match item {
            Item::Trait(t) => Traits::emit_trait_def(t, &mut out, |ty, assoc| {
                cx.rust_type_with_view_lifetime_assoc(ty, assoc)
            }),
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
            Item::CModule(cm) => emit_c_module(&cx, cm, &mut out),
            Item::EffectDecl(_)
            | Item::MarkerDecl(_)
            | Item::FactDecl(_)
            | Item::Func(_) | Item::Impl(_) | Item::Test(_) | Item::Bench(_) | Item::ExternRust(_)
            | Item::Module(_) | Item::CodeModule(_) | Item::ErrorConv(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::TypeAlias(_) // D-TYPEALIAS1: erases
            | Item::Migration(_) // D-MIGRATE1: migration is sema-only (I3)
            | Item::StateDecl(_) // D-STATE-DECL: state-set decls erase (I3)
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: erase (expanded in sema)
            | Item::GenericModule(_) // D-CONF-GENSPELL1=A: template — erases
            | Item::ModuleAlias(_) => {} // D-CONF-GENSPELL1=A: alias — erases after expansion
        }
    }

    for item in &prog.items {
        match item {
            Item::Struct(s) => {
                emit_type_impl(&cx, &s.name, &s.type_params, &s.methods, &mut out);
                for block in &s.trait_impls {
                    emit_trait_impl(&cx, &s.name, &s.type_params, block, Some(s), &mut out);
                }
            }
            Item::Enum(e) => {
                emit_type_impl(&cx, &e.name, &e.type_params, &e.methods, &mut out);
                for block in &e.trait_impls {
                    emit_trait_impl(&cx, &e.name, &e.type_params, block, None, &mut out);
                }
            }
            Item::Impl(i) => {
                // D-OSTARGET1=A: skip an `impl` gated to a non-active native OS.
                if i.os_target.is_some_and(|os| os != cx.active_os) {
                    continue;
                }
                if i.trait_name.is_some() {
                    let struct_def = prog.items.iter().find_map(|item| match item {
                        Item::Struct(s) if s.name == i.type_name => Some(s),
                        _ => None,
                    });
                    emit_external_trait_impl(&cx, i, struct_def, &mut out);
                } else {
                    emit_type_impl(
                        &cx,
                        &i.type_name,
                        type_params_for_name(&prog.items, &i.type_name),
                        &i.methods,
                        &mut out,
                    );
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
            emit_func(&cx, f, &mut out);
        }
    }

    emit_test_fns(&cx, &tests, None, &mut out);
    emit_test_main(&tests, &mut out);
    strip_unused_os_signal_prelude(strip_unused_raylib_prelude(strip_unused_term_prelude(strip_unused_gc_prelude(
        strip_unused_txn_prelude(strip_unused_mem_prelude(out)),
    ))))
}

/// D-TEST1/S43: the shared reporting `main` for a `jet test` harness. Each test
/// (unit or property) is invoked through its `jet_test_N()` entry; the loop is
/// identical whichever kind it is.
fn emit_test_main(tests: &[TestCase<'_>], out: &mut String) {
    emit_test_main_cov(tests, &[], out, false)
}

/// D-TESTKIT1=A (gaps #3/#4): filter, shuffle, and parallel-with-isolation. The
/// harness builds a `slots` list of `(name, skip, run fn ptr)` in source order,
/// then:
///   - `JET_TEST_FILTER=<substr>` (CLI `--filter`) keeps only matching names;
///   - `JET_TEST_SHUFFLE_SEED=<n>` (CLI `--shuffle[=seed]`) reorders `slots`
///     with a seeded Fisher-Yates shuffle before running (order-dependence
///     detection — a real bug still fails the same way, just in a different
///     sequence);
///   - runs are parallel by default (one thread per test; `jet_testing_temp_dir`
///     folds in the thread id for isolation, and test-body `print()` is routed
///     to a per-thread buffer flushed right before that test's result line —
///     see `jet_test_print`/`TExprKind::Print`), or serial with `JET_TEST_SERIAL`
///     set (CLI `--serial`).
/// Reporting always walks results in (possibly shuffled) `slots` order, so
/// output is deterministic regardless of which thread finishes first.
fn emit_test_main_cov(
    tests: &[TestCase<'_>],
    checks: &[&ResolvedOutput],
    out: &mut String,
    coverage: bool,
) {
    out.push_str("#[derive(Clone, Copy)]\n");
    out.push_str("struct JetTestSlot { name: &'static str, skip: bool, property: bool, run: fn() -> Result<(), String> }\n");
    out.push_str("fn main() {\n");
    out.push_str("    jet_std_env_init();\n");
    out.push_str("    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n");
    out.push_str("    jet_test_trace_tier();\n");
    out.push_str("    if let Ok(path) = std::env::var(\"JET_TEST_PROOF_REPORT\") { if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) { use std::io::Write as _; if file.metadata().map(|m| m.len() == 0).unwrap_or(false) { let _ = file.write_all(b\"JETTEST2\"); } } }\n");
    out.push_str("    let mut slots: Vec<JetTestSlot> = vec![\n");
    for test in tests {
        let def = test.test;
        let name = escape_rust_str(
            def.name
                .as_deref()
                .expect("sema resolves every test marker name before codegen"),
        );
        let skip = whole_test_skip(def);
        out.push_str(&format!(
            "        JetTestSlot {{ name: {}, skip: {}, property: {}, run: {} }},\n",
            name,
            skip,
            !def.params.is_empty(),
            test_fn_path(test),
        ));
    }
    for (i, check) in checks.iter().enumerate() {
        let name = escape_rust_str(&check.output_name);
        out.push_str(&format!(
            "        JetTestSlot {{ name: {}, skip: false, property: false, run: jet_output_check_{} }},\n",
            name, i
        ));
    }
    out.push_str("    ];\n");
    // Filter (D-TESTKIT1 gap #4): `--filter=<substr>` keeps names containing it.
    out.push_str("    if let Ok(filter) = std::env::var(\"JET_TEST_FILTER\") {\n");
    out.push_str("        slots.retain(|s| s.name.contains(filter.as_str()));\n");
    out.push_str("    }\n");
    // Shuffle (gap #4): `--shuffle[=seed]` reorders before running; the seed is
    // always printed so a shuffled run's order is reproducible.
    out.push_str("    if let Ok(seed_str) = std::env::var(\"JET_TEST_SHUFFLE_SEED\") {\n");
    out.push_str("        if let Ok(seed) = seed_str.parse::<u64>() {\n");
    out.push_str("            println!(\"shuffle: seed={}\", seed);\n");
    out.push_str("            let order = jet_test_shuffle_order(slots.len(), seed);\n");
    out.push_str("            slots = order.into_iter().map(|i| slots[i]).collect();\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    // Run (gap #3): parallel by default (one thread per test, own temp-dir/output
    // isolation), serial with `--serial` (`JET_TEST_SERIAL`) or when there is at
    // most one test (no isolation benefit, and keeps single-test runs allocation-
    // free of the thread machinery).
    out.push_str("    let serial = std::env::var(\"JET_TEST_SERIAL\").is_ok();\n");
    out.push_str("    let results: Vec<(String, bool, bool, Option<Result<(), String>>, String, Option<JetTestFailure>)> = if serial || slots.len() <= 1 {\n");
    out.push_str("        slots.iter().map(|s| {\n");
    out.push_str("            let res = if s.skip { None } else { Some((s.run)()) };\n");
    out.push_str("            let output = jet_test_take_output();\n");
    out.push_str("            let failure = jet_test_take_failure();\n");
    out.push_str("            (s.name.to_string(), s.skip, s.property, res, output, failure)\n");
    out.push_str("        }).collect()\n");
    out.push_str("    } else {\n");
    out.push_str("        let handles: Vec<_> = slots.iter().map(|s| {\n");
    out.push_str("            let name = s.name.to_string();\n");
    out.push_str("            let skip = s.skip;\n");
    out.push_str("            let property = s.property;\n");
    out.push_str("            let run = s.run;\n");
    out.push_str("            std::thread::spawn(move || {\n");
    out.push_str("                let res = if skip { None } else { Some(run()) };\n");
    out.push_str("                let output = jet_test_take_output();\n");
    out.push_str("                let failure = jet_test_take_failure();\n");
    out.push_str("                (name, skip, property, res, output, failure)\n");
    out.push_str("            })\n");
    out.push_str("        }).collect();\n");
    out.push_str("        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| (\"<thread panicked>\".to_string(), false, false, Some(Err(\"test thread panicked\".to_string())), String::new(), None))).collect()\n");
    out.push_str("    };\n");
    out.push_str("    let mut report = JetTestReport::new(0, 0, 0);\n");
    out.push_str("    for (name, skip, property, res, output, failure) in results {\n");
    out.push_str("        if !output.is_empty() { print!(\"{}\", output); }\n");
    out.push_str("        match (skip, res) {\n");
    out.push_str("            (true, _) => { println!(\"{}: skip\", name); jet_proof_record(0, 2, &name, \"\", \"\", 0); report.skipped += 1; }\n");
    out.push_str("            (false, Some(Ok(()))) => { println!(\"{}: pass\", name); if !property { jet_proof_record(0, 0, &name, \"\", \"\", 0); } report.passed += 1; }\n");
    out.push_str("            (false, Some(Err(msg))) => { let mut failure = failure.unwrap_or_else(|| JetTestFailure::fallback(&msg)); if property { failure.message = msg; } println!(\"{}: FAIL\", name); eprint!(\"{}\", failure.render_detail()); if !property { jet_proof_record(0, 1, &name, &failure.message, &failure.file, failure.line); } report.failed += 1; }\n");
    out.push_str("            (false, None) => unreachable!(),\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    println!(\"{}\", report.summary());\n");
    if coverage {
        // D-COV1: write the hit set before any `exit` (which would skip Drop).
        out.push_str("    jet_cov_dump();\n");
    }
    out.push_str("    if report.failed > 0 { std::process::exit(1); }\n");
    out.push_str("}\n");
}

fn emit_output_check_fns(checks: &[&ResolvedOutput], out: &mut String) {
    for (i, check) in checks.iter().enumerate() {
        out.push_str(&format!("fn jet_output_check_{i}() -> Result<(), String> {{\n"));
        let fallible = matches!(check.return_type, Some(Type::Result { .. }));
        if fallible {
            out.push_str(&format!("    {}()\n", check.lowered_name));
        } else {
            out.push_str(&format!("    {}();\n    Ok(())\n", check.lowered_name));
        }
        out.push_str("}\n\n");
    }
}

/// Does any test in the set declare property parameters (D-TEST1)? Drives whether
/// the harness needs `PROP_PRELUDE`.
fn any_property_test(tests: &[TestCase<'_>]) -> bool {
    tests.iter().any(|t| !t.test.params.is_empty())
}

/// D-DOTSCOPE1: is this test whole-test-skipped — i.e. does a `.skip` scope
/// member appear as its FIRST statement? The whole test is then not run and
/// reports `name: skip`. A `.skip` later in the body is a region-skip instead.
fn whole_test_skip(test: &TestDef) -> bool {
    matches!(
        test.body.first(),
        Some(crate::AST::Stmt::ScopeMember { name, .. }) if name == Syntax::SCOPE_TEST_SKIP
    )
}

/// D-TEST1: emit the per-test functions. A unit test (`#Test "name" { … }`, no
/// params) becomes `fn jet_test_N() -> Result<(), String>` exactly as before. A
/// property test (`#Test fn name(p…) { … }`) becomes a body fn `jet_prop_N(p…)`
/// plus a driver `jet_test_N()` that generates inputs, runs cases, and shrinks
/// the first failure to a minimal counterexample. Either way `jet_test_N()` is
/// the single entry the main loop calls, so the reporting loop is shared.
fn emit_test_fns(
    cx: &Cx,
    tests: &[TestCase<'_>],
    module: Option<&str>,
    out: &mut String,
) {
    const CASES: usize = 200;
    const SHRINK_STEPS: usize = 2000;
    let visibility = if module.is_some() { "pub " } else { "" };
    for test_case in tests {
        if test_case.module.as_deref() != module {
            continue;
        }
        let test = test_case.test;
        let i = test_case.index;
        if test.params.is_empty() {
            out.push_str(&format!("{visibility}fn jet_test_{}() -> Result<(), String> {{\n", i));
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
            "{visibility}fn jet_prop_{}({}) -> Result<(), String> {{\n",
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
        out.push_str(&format!("{visibility}fn jet_test_{}() -> Result<(), String> {{\n", i));
        out.push_str("    let seed = jet_prop_seed();\n");
        out.push_str("    let mut driver_rng = JetRng::new(seed);\n");
        // call helper that takes the tuple, returns Result
        let call_args: Vec<String> = (0..n).map(|k| format!("input.{}.clone()", k)).collect();
        out.push_str(&format!(
            "    let run = |input: &{}| -> Result<(), String> {{ jet_prop_{}({}) }};\n",
            tuple_ty,
            i,
            call_args.join(", ")
        ));
        out.push_str(&format!("    for case_index in 0..{} {{\n", CASES));
        out.push_str("        let case_seed = driver_rng.next_u64();\n");
        out.push_str("        let mut rng = JetRng::new(case_seed);\n");
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
            out.push_str(
                "                        input = trial; msg = m; improved = true; break;\n",
            );
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
        out.push_str(&format!(
            "            jet_proof_record(3, 1, {}, &args.join(\", \"), &seed.to_string(), (case_index + 1) as u32);\n",
            escape_rust_str(
                test.name
                    .as_deref()
                    .expect("sema resolves every test marker name before codegen"),
            )
        ));
        out.push_str("            return Err(format!(\"property failed for {}\\n  {}\", args.join(\", \"), msg));\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
        out.push_str(&format!(
            "    jet_proof_record(3, 0, {}, \"\", &seed.to_string(), {});\n",
            escape_rust_str(
                test.name
                    .as_deref()
                    .expect("sema resolves every test marker name before codegen"),
            ), CASES
        ));
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
    jet_foundation::ice!(
        None,
        "codegen reached a #Test body construct the typed IR does not cover — compiler bug (I2/R7)"
    );
}

/// D-UIDEVSHELL1=A (c134 Phase 8): true when the native GTK4 backend prelude
/// should be emitted — the program constructs `core.ui.gtk_backend()` AND the
/// active target OS is Linux. `used_core` is collected before `@if
/// @build.os` folds, so a Linux-only backend used under a `.Linux` arm still
/// shows up on a macOS/Windows build; the `active_os` gate is what actually
/// keeps the gtk `extern "C"` surface out of a non-Linux target (the backend is
/// Linux-only). The `.Linux` dispatch arm's construction is likewise folded out
/// of `main` on those targets, so nothing references it.
fn uses_gtk_backend(bundle: &ProgramBundle) -> bool {
    bundle.active_os == Syntax::OSTarget::Linux
        && bundle.used_core.iter().any(|u| u == "core.ui::gtk_backend")
}

pub fn emit_bundle(bundle: &ProgramBundle, _mode: CompileMode, link: Option<&FfiLink>) -> String {
    emit_bundle_dbg(bundle, link, false, Syntax::OSTarget::host())
}

/// D-DBG3 step 2 (dap-debugger): identical to `emit_bundle`, but with
/// `debug_linemap = true` every generated statement gets a `// jet:line N` marker
/// (`TStmt::LineMarker`) the native debug backend's line table reads back. Used ONLY
/// by the `jet debug` native build path; `emit_bundle` (linemap off) stays
/// byte-identical to today's output for every other build (golden tests, JIT).
///
/// D-OSTARGET1=A (ratified 2026-07-01, c134): `active_os` is the resolved
/// native OS bucket this build targets (from `--target=<triple>`, or the host
/// OS when absent) — an `impl` gated to a different `#Target(OS.*)` is
/// skipped entirely (`Codegen/Imports.rs::emit_program_items`).
pub fn emit_bundle_dbg(
    bundle: &ProgramBundle,
    link: Option<&FfiLink>,
    debug_linemap: bool,
    active_os: Syntax::OSTarget,
) -> String {
    // D-DATAFLOW1 / D-REL3: fixed_sigs and edition-gated helpers read the TLS
    // package edition. Keep codegen on the same edition sema checked.
    jet_foundation::PackageEdition::with_package_edition(&bundle.edition, || {
    let entry = &bundle.modules[bundle.entry];
    let bundle_auto_derives =
        crate::Traits::TraitRegistry::bundle_auto_derives(bundle, &bundle.name_ledger);
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} — do not edit. Edit the .{} source instead.\n",
        Syntax::BINARY_NAME,
        Syntax::FILE_EXT
    ));
    // E2-M12 D-OBS1: source-map marker for tooling and debuggers.
    out.push_str(&format!("// jet:source-map source={}\n", entry.display));
    for provenance in TIR::instance_provenance(bundle) {
        out.push_str(&format!("// jet:generic-instance module={} fingerprint={} full-key={}\n", provenance.canonical_module, provenance.fingerprint, provenance.full_key_hex));
    }
    out.push_str("#![allow(warnings)]\n\n");
    let edition_year = bundle.edition.parse::<u16>().unwrap_or(2027);
    out.push_str(&format!("const __JET_PACKAGE_EDITION: u16 = {edition_year};\n\n"));
    emit_command_metadata(bundle, active_os, &mut out);
    if let Some(ffi) = link {
        out.push_str(&format!("extern crate {};\n\n", ffi.crate_name));
    }
    push_cached_runtime(&mut out, link);
    if needs_embedded_runtime(bundle) {
        push_corelib_prelude(&mut out, &bundle.used_core, uses_stream(bundle));
        out.push_str(scheduler_prelude_for_emit(uses_native_scheduler(bundle)));
        out.push_str(UI_PRELUDE);
        if uses_gtk_backend(bundle) {
            out.push_str(UI_GTK_PRELUDE);
        }
        push_app_preludes(&mut out, &bundle.used_core);
    }
    out.push('\n');

    let import_mods = import_mod_map(bundle, bundle.entry);
    let extern_funcs = bundle_extern_funcs(bundle);

    for (i, module) in bundle.modules.iter().enumerate() {
        if i == bundle.entry {
            continue;
        }
        let ns = module.alias.clone();
        out.push_str(&format!("mod {} {{\n", mangle(&ns)));
        out.push_str(MOD_USE);
        out.push_str("use super::jet_stack_enter;\n");
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        cx.foreign_undos = bundle_foreign_undos(bundle);
        apply_auto_derives(&mut cx, &bundle_auto_derives[i]);
        cx.module_alias = module.alias.clone();
        register_bundle_reflect_paths(&mut cx, bundle, i);
        cx.core_archive_source = bundle.modules.iter().any(|module| module.alias == "core_archive");
        // D-DBG3 step 2: line markers stay scoped to the entry file only (v1, same
        // restriction as the step-1 interpreter debugger) — a bare `// jet:line N`
        // can't disambiguate which file N belongs to across modules.
        cx.import_mods = import_mod_map(bundle, i);
        cx.foreign_types = foreign_type_map(bundle, i);
        TIR::register_imported_struct_shapes(&mut cx, bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        cx.reexport_calls = reexport_call_map(bundle, i);
        cx.import_sigs = import_sig_map(bundle, i);
        cx.import_rets = import_ret_map(bundle, i);
        cx.core_imports = core_import_map(bundle, i);
        register_core_import_surfaces(&mut cx);
        cx.used_core = bundle.used_core.clone();
        cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
        register_bundle_unit_metadata(&mut cx, bundle, i);
        cx.root_prefix = "super::".to_string();
        cx.active_os = active_os;
        let (uinline, ufile) = unqualified_import_maps(bundle, i);
        cx.unqualified_inline = uinline;
        cx.unqualified_file = ufile;
        let (inline, file, names, reexports) = inline_import_maps(bundle, i);
        cx.inline_unqualified = inline;
        cx.inline_unqualified_file = file;
        cx.inline_import_names = names;
        cx.inline_reexport_inline = reexports;
        let (inline_core, reexport_core) = inline_core_import_maps(bundle, i);
        cx.inline_core_imports = inline_core;
        cx.inline_reexport_core = reexport_core;
        cx.inline_foreign_imports = inline_foreign_import_maps(bundle, i);
        let (inline_foreign_sigs, inline_foreign_rets) =
            inline_foreign_import_signature_maps(bundle, i);
        cx.inline_foreign_sigs = inline_foreign_sigs;
        cx.inline_foreign_rets = inline_foreign_rets;
        cx.inline_reexport_foreign = inline_foreign_reexport_maps(bundle, i);
        let (inline_foreign_reexport_sigs, inline_foreign_reexport_rets) =
            inline_foreign_reexport_signature_maps(bundle, i);
        cx.inline_foreign_reexport_sigs = inline_foreign_reexport_sigs;
        cx.inline_foreign_reexport_rets = inline_foreign_reexport_rets;
        emit_program_items(&cx, &module.items, &mut out, true, true);
        out.push_str("}\n\n");
    }

    let mut cx = build_cx_items(
        &entry.items,
        &entry.source,
        &entry.display,
        link,
        &extern_funcs,
    );
    cx.foreign_undos = bundle_foreign_undos(bundle);
    apply_auto_derives(&mut cx, &bundle_auto_derives[bundle.entry]);
    cx.module_alias = entry.alias.clone();
    cx.core_archive_source = bundle.modules.iter().any(|module| module.alias == "core_archive");
    cx.debug_linemap = debug_linemap;
    cx.active_os = active_os;
    cx.import_mods = import_mods;
    cx.foreign_types = foreign_type_map(bundle, bundle.entry);
    TIR::register_imported_struct_shapes(&mut cx, bundle, bundle.entry);
    register_foreign_enum_variants(&mut cx, bundle, bundle.entry);
    update_cloneability_with_foreign_types(&mut cx, &entry.items);
    cx.reexport_calls = reexport_call_map(bundle, bundle.entry);
    cx.import_sigs = import_sig_map(bundle, bundle.entry);
    cx.import_rets = import_ret_map(bundle, bundle.entry);
    cx.core_imports = core_import_map(bundle, bundle.entry);
    register_core_import_surfaces(&mut cx);
    cx.used_core = bundle.used_core.clone();
    cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
    register_bundle_unit_metadata(&mut cx, bundle, bundle.entry);
    register_bundle_reflect_paths(&mut cx, bundle, bundle.entry);
    for import in &entry.imports {
        if bundle
            .name_ledger
            .effective_alias(bundle.entry, &import.import_alias())
            .is_none()
        {
            continue;
        }
        let Some(target) = bundle.name_ledger.import_target(bundle.entry, import.span) else {
            continue;
        };
        let imported = &bundle.modules[target];
        let has_unit_display = imported.items.iter().any(|item| {
            let Item::Impl(implementation) = item else {
                return false;
            };
            implementation.trait_name.as_deref() == Some(Syntax::TRAIT_DISPLAY)
                && imported.items.iter().any(|item| {
                    matches!(
                        item,
                        Item::UnitFamily(family)
                            if family.distinct_defs().iter().any(|definition| {
                                definition.name == implementation.type_name
                            })
                    )
                })
        });
        if !has_unit_display {
            continue;
        }
        out.push_str(&format!(
            "use {}::__jet_Display as _;\n",
            mangle(&imported.alias)
        ));
    }
    let (uinline, ufile) = unqualified_import_maps(bundle, bundle.entry);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
    let (inline, file, names, reexports) = inline_import_maps(bundle, bundle.entry);
    cx.inline_unqualified = inline;
    cx.inline_unqualified_file = file;
    cx.inline_import_names = names;
    cx.inline_reexport_inline = reexports;
    let (inline_core, reexport_core) = inline_core_import_maps(bundle, bundle.entry);
    cx.inline_core_imports = inline_core;
    cx.inline_reexport_core = reexport_core;
    cx.inline_foreign_imports = inline_foreign_import_maps(bundle, bundle.entry);
    let (inline_foreign_sigs, inline_foreign_rets) =
        inline_foreign_import_signature_maps(bundle, bundle.entry);
    cx.inline_foreign_sigs = inline_foreign_sigs;
    cx.inline_foreign_rets = inline_foreign_rets;
    cx.inline_reexport_foreign = inline_foreign_reexport_maps(bundle, bundle.entry);
    let (inline_foreign_reexport_sigs, inline_foreign_reexport_rets) =
        inline_foreign_reexport_signature_maps(bundle, bundle.entry);
    cx.inline_foreign_reexport_sigs = inline_foreign_reexport_sigs;
    cx.inline_foreign_reexport_rets = inline_foreign_reexport_rets;
    emit_program_items(&cx, &entry.items, &mut out, true, false);
    // D-CLIFLAG1: a typed `fn run(args: T)` is the Jet entry (S12). Synthesize
    // the Rust `fn main` wrapper that parses `io.args()` and dispatches to it.
    // No-op when the entry file has no `run` (sema's E0101 already rejected it).
    let cli_items = jet_foundation::CLISchema::entry_type_module(bundle)
        .map(|module| bundle.modules[module].items.as_slice())
        .unwrap_or(entry.items.as_slice());
    emit_cli_entry_if_needed(&cx, &entry.items, cli_items, &mut out);
    strip_unused_os_signal_prelude(strip_unused_raylib_prelude(strip_unused_term_prelude(strip_unused_gc_prelude(
        strip_unused_txn_prelude(strip_unused_mem_prelude(out)),
    ))))
    })
}

pub fn emit_bundle_tests(bundle: &ProgramBundle, link: Option<&FfiLink>) -> String {
    emit_bundle_tests_cov(bundle, link, false)
}

/// D-COV1: emit the `jet test` harness, optionally with coverage instrumentation.
/// `coverage = false` is byte-identical to the historical `emit_bundle_tests`
/// (golden tests rely on this), so the probes/prelude only appear under
/// `jet test --coverage`.
pub fn emit_bundle_tests_cov(
    bundle: &ProgramBundle,
    link: Option<&FfiLink>,
    coverage: bool,
) -> String {
    let entry = &bundle.modules[bundle.entry];
    let bundle_auto_derives =
        crate::Traits::TraitRegistry::bundle_auto_derives(bundle, &bundle.name_ledger);
    let tests: Vec<TestCase<'_>> = bundle
        .modules
        .iter()
        .enumerate()
        .flat_map(|(owner, module)| {
            let module_path = (owner != bundle.entry).then(|| mangle(&module.alias));
            module.items.iter().filter_map(move |item| match item {
                Item::Test(test) => Some((module_path.clone(), test)),
                _ => None,
            })
        })
        .enumerate()
        .map(|(index, (module, test))| TestCase {
            test,
            module,
            index,
        })
        .collect();
    let checks = bundle.modules.iter().flat_map(|module| module.items.iter()).filter_map(|item| {
        let Item::Const(value) = item else { return None };
        value.resolved_output.as_ref().filter(|output| {
            output.selected && output.kind == crate::AST::OutputKind::Check
        })
    }).collect::<Vec<_>>();
    assert!(
        !tests.is_empty() || !checks.is_empty(),
        "emit_bundle_tests called with no test blocks or Check Outputs"
    );
    let want_prop_prelude = any_property_test(&tests);

    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} test harness — do not edit.\n",
        Syntax::BINARY_NAME
    ));
    out.push_str("#![allow(warnings)]\n\n");
    let edition_year = bundle.edition.parse::<u16>().unwrap_or(2027);
    out.push_str(&format!(
        "const __JET_PACKAGE_EDITION: u16 = {edition_year};\n\n"
    ));
    if let Some(ffi) = link {
        out.push_str(&format!("extern crate {};\n\n", ffi.crate_name));
    }
    push_cached_runtime(&mut out, link);
    out.push_str(TEST_PRELUDE);
    out.push_str(TEST_REPORT_PRELUDE);
    if want_prop_prelude {
        out.push_str(PROP_PRELUDE);
    }
    if coverage {
        out.push_str(COV_PRELUDE);
    }
    if needs_embedded_runtime(bundle) {
        push_corelib_prelude(&mut out, &bundle.used_core, uses_stream(bundle));
        out.push_str(scheduler_prelude_for_emit(uses_native_scheduler(bundle)));
        out.push_str(UI_PRELUDE);
        if uses_gtk_backend(bundle) {
            out.push_str(UI_GTK_PRELUDE);
        }
        push_app_preludes(&mut out, &bundle.used_core);
    }
    out.push('\n');

    let import_mods = import_mod_map(bundle, bundle.entry);
    let extern_funcs = bundle_extern_funcs(bundle);

    for (i, module) in bundle.modules.iter().enumerate() {
        if i == bundle.entry {
            continue;
        }
        let module_path = mangle(&module.alias);
        out.push_str(&format!("mod {} {{\n", module_path));
        out.push_str(MOD_USE);
        out.push_str("use super::jet_stack_enter;\n");
        out.push_str("use super::{jet_proof_record, jet_test_failure, jet_test_print};\n");
        if tests.iter().any(|test| {
            test.module.as_deref() == Some(module_path.as_str()) && !test.test.params.is_empty()
        }) {
            out.push_str("use super::{jet_prop_seed, JetGen, JetRng};\n");
        }
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        cx.foreign_undos = bundle_foreign_undos(bundle);
        apply_auto_derives(&mut cx, &bundle_auto_derives[i]);
        cx.module_alias = module.alias.clone();
        cx.core_archive_source = bundle.modules.iter().any(|module| module.alias == "core_archive");
        cx.test_mode = true;
        cx.coverage = coverage; // inline Core scope setup follows below
        cx.import_mods = import_mod_map(bundle, i);
        cx.foreign_types = foreign_type_map(bundle, i);
        TIR::register_imported_struct_shapes(&mut cx, bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        cx.reexport_calls = reexport_call_map(bundle, i);
        cx.import_sigs = import_sig_map(bundle, i);
        cx.import_rets = import_ret_map(bundle, i);
        cx.core_imports = core_import_map(bundle, i);
        register_core_import_surfaces(&mut cx);
        cx.used_core = bundle.used_core.clone();
        cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
        cx.root_prefix = "super::".to_string();
        let (uinline, ufile) = unqualified_import_maps(bundle, i);
        cx.unqualified_inline = uinline;
        cx.unqualified_file = ufile;
        let (inline, file, names, reexports) = inline_import_maps(bundle, i);
        cx.inline_unqualified = inline;
        cx.inline_unqualified_file = file;
        cx.inline_import_names = names;
        cx.inline_reexport_inline = reexports;
        let (inline_core, reexport_core) = inline_core_import_maps(bundle, i);
        cx.inline_core_imports = inline_core;
        cx.inline_reexport_core = reexport_core;
        cx.inline_foreign_imports = inline_foreign_import_maps(bundle, i);
        let (inline_foreign_sigs, inline_foreign_rets) =
            inline_foreign_import_signature_maps(bundle, i);
        cx.inline_foreign_sigs = inline_foreign_sigs;
        cx.inline_foreign_rets = inline_foreign_rets;
        cx.inline_reexport_foreign = inline_foreign_reexport_maps(bundle, i);
        let (inline_foreign_reexport_sigs, inline_foreign_reexport_rets) =
            inline_foreign_reexport_signature_maps(bundle, i);
        cx.inline_foreign_reexport_sigs = inline_foreign_reexport_sigs;
        cx.inline_foreign_reexport_rets = inline_foreign_reexport_rets;
        emit_program_items(&cx, &module.items, &mut out, false, true);
        emit_test_fns(&cx, &tests, Some(&module_path), &mut out);
        out.push_str("}\n\n");
    }

    let mut cx = build_cx_items(
        &entry.items,
        &entry.source,
        &entry.display,
        link,
        &extern_funcs,
    );
    cx.foreign_undos = bundle_foreign_undos(bundle);
    apply_auto_derives(&mut cx, &bundle_auto_derives[bundle.entry]);
    cx.module_alias = entry.alias.clone();
    cx.core_archive_source = bundle.modules.iter().any(|module| module.alias == "core_archive");
    cx.test_mode = true;
    cx.coverage = coverage;
    cx.import_mods = import_mods;
    cx.foreign_types = foreign_type_map(bundle, bundle.entry);
    TIR::register_imported_struct_shapes(&mut cx, bundle, bundle.entry);
    register_foreign_enum_variants(&mut cx, bundle, bundle.entry);
    update_cloneability_with_foreign_types(&mut cx, &entry.items);
    cx.reexport_calls = reexport_call_map(bundle, bundle.entry);
    cx.import_sigs = import_sig_map(bundle, bundle.entry);
    cx.import_rets = import_ret_map(bundle, bundle.entry);
    cx.core_imports = core_import_map(bundle, bundle.entry);
    register_core_import_surfaces(&mut cx);
    cx.used_core = bundle.used_core.clone();
    cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
    let (uinline, ufile) = unqualified_import_maps(bundle, bundle.entry);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
    let (inline, file, names, reexports) = inline_import_maps(bundle, bundle.entry);
    cx.inline_unqualified = inline;
    cx.inline_unqualified_file = file;
    cx.inline_import_names = names;
    cx.inline_reexport_inline = reexports;
    let (inline_core, reexport_core) = inline_core_import_maps(bundle, bundle.entry);
    cx.inline_core_imports = inline_core;
    cx.inline_reexport_core = reexport_core;
    cx.inline_foreign_imports = inline_foreign_import_maps(bundle, bundle.entry);
    let (inline_foreign_sigs, inline_foreign_rets) =
        inline_foreign_import_signature_maps(bundle, bundle.entry);
    cx.inline_foreign_sigs = inline_foreign_sigs;
    cx.inline_foreign_rets = inline_foreign_rets;
    cx.inline_reexport_foreign = inline_foreign_reexport_maps(bundle, bundle.entry);
    let (inline_foreign_reexport_sigs, inline_foreign_reexport_rets) =
        inline_foreign_reexport_signature_maps(bundle, bundle.entry);
    cx.inline_foreign_reexport_sigs = inline_foreign_reexport_sigs;
    cx.inline_foreign_reexport_rets = inline_foreign_reexport_rets;
    emit_program_items(&cx, &entry.items, &mut out, false, false);

    emit_test_fns(&cx, &tests, None, &mut out);
    emit_output_check_fns(&checks, &mut out);
    emit_test_main_cov(&tests, &checks, &mut out, coverage);
    strip_unused_os_signal_prelude(strip_unused_raylib_prelude(strip_unused_term_prelude(strip_unused_gc_prelude(
        strip_unused_txn_prelude(strip_unused_mem_prelude(out)),
    ))))
}

/// D-TESTKIT1=A (c308 pass 2, gap #1): pick which property `#Test fn` a `jet
/// fuzz` run targets. `test_name` is the CLI's optional second positional
/// (`jet fuzz <file> [<name>]`).
///   - named: must exist and must be a property test (have params) — else a
///     plain-English `Err` naming the problem (CLI-level selection error, not
///     a compiler diagnostic, same tier as `run_bench`'s missing-file message).
///   - unnamed: exactly one property test in the file is picked automatically;
///     zero or more-than-one is an `Err` (the latter lists the candidates).
fn select_fuzz_target(
    tests: &[TestCase<'_>],
    test_name: Option<&str>,
) -> Result<usize, String> {
    if let Some(name) = test_name {
        match tests
            .iter()
            .position(|test| test.test.name.as_deref() == Some(name))
        {
            Some(i) if !tests[i].test.params.is_empty() => Ok(i),
            Some(_) => Err(format!(
                "`{}` is a unit `#Test`, not a property test — `jet fuzz` needs a parameterized `#Test fn` (D-TEST1)",
                name
            )),
            None => Err(format!("no `#Test` named `{}` in this file", name)),
        }
    } else {
        let candidates: Vec<usize> = tests
            .iter()
            .enumerate()
            .filter(|(_, t)| !t.test.params.is_empty())
            .map(|(i, _)| i)
            .collect();
        match candidates.len() {
            0 => Err(
                "no property `#Test fn` (D-TEST1) found to fuzz — `jet fuzz` needs one \
                 parameterized `#Test fn(...)`, not a unit `#Test(\"name\") { ... }`"
                    .to_string(),
            ),
            1 => Ok(candidates[0]),
            _ => {
                let names: Vec<&str> = candidates
                    .iter()
                    .map(|&index| {
                        tests[index]
                            .test
                            .name
                            .as_deref()
                            .expect("sema resolves every test marker name before codegen")
                    })
                    .collect();
                Err(format!(
                    "multiple property tests in this file — say which one: {}\n  fix: `jet fuzz <file> <name>`, e.g. `jet fuzz <file> \"{}\"`",
                    names.join(", "),
                    names[0]
                ))
            }
        }
    }
}

/// D-TESTKIT1=A (c308 pass 2, gap #1): `jet fuzz <file> [<name>]` — reuses the
/// whole `jet test` harness (same prelude, same `jet_test_fns` for every test,
/// same `JetRng`/`JetGen`/shrink machinery from `PROP_PRELUDE`) but swaps the
/// reporting `main` for a fuzz driver over exactly one property test:
///   - replays the on-disk corpus first (each entry is a seed that reproduced
///     a failure before — deterministic replay, not the raw decoded value, so
///     no bespoke value serialization is needed, D-TEST1's `JetRng` already
///     makes a seed a full, exact reproduction);
///   - then generates fresh cases from a seeded, incrementing PRNG until the
///     iteration or wall-clock budget runs out;
///   - on the first failure, shrinks with the identical greedy algorithm the
///     property-test driver uses, saves the (pre-shrink) seed to the corpus
///     directory, and prints a `jet test`-shaped repro line.
/// Returns `Err(message)` for a CLI-level target-selection problem (no/wrong/
/// ambiguous test) rather than a compiler diagnostic — this is argument
/// validation, not a semantic error in the user's program.
pub fn emit_bundle_fuzz(
    bundle: &ProgramBundle,
    link: Option<&FfiLink>,
    file_label: &str,
    test_name: Option<&str>,
) -> Result<String, String> {
    let entry = &bundle.modules[bundle.entry];
    let bundle_auto_derives =
        crate::Traits::TraitRegistry::bundle_auto_derives(bundle, &bundle.name_ledger);
    let tests: Vec<TestCase<'_>> = entry
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Test(t) => Some(t),
            _ => None,
        })
        .enumerate()
        .map(|(index, test)| TestCase {
            test,
            module: None,
            index,
        })
        .collect();
    if tests.is_empty() {
        return Err(
            "no `#Test` blocks in this file — `jet fuzz` needs a parameterized `#Test fn(...)`"
                .to_string(),
        );
    }
    let target = select_fuzz_target(&tests, test_name)?;

    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} fuzz harness — do not edit.\n",
        Syntax::BINARY_NAME
    ));
    out.push_str("#![allow(warnings)]\n\n");
    if let Some(ffi) = link {
        out.push_str(&format!("extern crate {};\n\n", ffi.crate_name));
    }
    push_cached_runtime(&mut out, link);
    out.push_str(TEST_PRELUDE);
    out.push_str(TEST_REPORT_PRELUDE);
    // Fuzzing always targets a property test, so the JetRng/JetGen/shrink
    // runtime is always needed (unlike `jet test`, which only emits it when a
    // property test is present).
    out.push_str(PROP_PRELUDE);
    if needs_embedded_runtime(bundle) {
        push_corelib_prelude(&mut out, &bundle.used_core, uses_stream(bundle));
        out.push_str(scheduler_prelude_for_emit(uses_native_scheduler(bundle)));
        out.push_str(UI_PRELUDE);
        if uses_gtk_backend(bundle) {
            out.push_str(UI_GTK_PRELUDE);
        }
        push_app_preludes(&mut out, &bundle.used_core);
    }
    out.push('\n');

    let import_mods = import_mod_map(bundle, bundle.entry);
    let extern_funcs = bundle_extern_funcs(bundle);

    for (i, module) in bundle.modules.iter().enumerate() {
        if i == bundle.entry {
            continue;
        }
        let ns = module.alias.clone();
        out.push_str(&format!("mod {} {{\n", mangle(&ns)));
        out.push_str(MOD_USE);
        out.push_str("use super::jet_stack_enter;\n");
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        cx.foreign_undos = bundle_foreign_undos(bundle);
        apply_auto_derives(&mut cx, &bundle_auto_derives[i]);
        cx.module_alias = module.alias.clone();
        cx.core_archive_source = bundle.modules.iter().any(|module| module.alias == "core_archive");
        cx.test_mode = true;
        cx.import_mods = import_mod_map(bundle, i);
        cx.foreign_types = foreign_type_map(bundle, i);
        TIR::register_imported_struct_shapes(&mut cx, bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        cx.reexport_calls = reexport_call_map(bundle, i);
        cx.import_sigs = import_sig_map(bundle, i);
        cx.import_rets = import_ret_map(bundle, i);
        cx.core_imports = core_import_map(bundle, i);
        register_core_import_surfaces(&mut cx);
        cx.used_core = bundle.used_core.clone();
        cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
        cx.root_prefix = "super::".to_string();
        let (uinline, ufile) = unqualified_import_maps(bundle, i);
        cx.unqualified_inline = uinline;
        cx.unqualified_file = ufile;
        let (inline, file, names, reexports) = inline_import_maps(bundle, i);
        cx.inline_unqualified = inline;
        cx.inline_unqualified_file = file;
        cx.inline_import_names = names;
        cx.inline_reexport_inline = reexports;
        let (inline_core, reexport_core) = inline_core_import_maps(bundle, i);
        cx.inline_core_imports = inline_core;
        cx.inline_reexport_core = reexport_core;
        cx.inline_foreign_imports = inline_foreign_import_maps(bundle, i);
        let (inline_foreign_sigs, inline_foreign_rets) =
            inline_foreign_import_signature_maps(bundle, i);
        cx.inline_foreign_sigs = inline_foreign_sigs;
        cx.inline_foreign_rets = inline_foreign_rets;
        cx.inline_reexport_foreign = inline_foreign_reexport_maps(bundle, i);
        let (inline_foreign_reexport_sigs, inline_foreign_reexport_rets) =
            inline_foreign_reexport_signature_maps(bundle, i);
        cx.inline_foreign_reexport_sigs = inline_foreign_reexport_sigs;
        cx.inline_foreign_reexport_rets = inline_foreign_reexport_rets;
        emit_program_items(&cx, &module.items, &mut out, false, true);
        out.push_str("}\n\n");
    }

    let mut cx = build_cx_items(
        &entry.items,
        &entry.source,
        &entry.display,
        link,
        &extern_funcs,
    );
    cx.foreign_undos = bundle_foreign_undos(bundle);
    apply_auto_derives(&mut cx, &bundle_auto_derives[bundle.entry]);
    cx.module_alias = entry.alias.clone();
    cx.core_archive_source = bundle.modules.iter().any(|module| module.alias == "core_archive");
    cx.test_mode = true;
    cx.import_mods = import_mods;
    cx.foreign_types = foreign_type_map(bundle, bundle.entry);
    TIR::register_imported_struct_shapes(&mut cx, bundle, bundle.entry);
    register_foreign_enum_variants(&mut cx, bundle, bundle.entry);
    update_cloneability_with_foreign_types(&mut cx, &entry.items);
    cx.reexport_calls = reexport_call_map(bundle, bundle.entry);
    cx.import_sigs = import_sig_map(bundle, bundle.entry);
    cx.import_rets = import_ret_map(bundle, bundle.entry);
    cx.core_imports = core_import_map(bundle, bundle.entry);
    register_core_import_surfaces(&mut cx);
    cx.used_core = bundle.used_core.clone();
    cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
    let (uinline, ufile) = unqualified_import_maps(bundle, bundle.entry);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
    let (inline, file, names, reexports) = inline_import_maps(bundle, bundle.entry);
    cx.inline_unqualified = inline;
    cx.inline_unqualified_file = file;
    cx.inline_import_names = names;
    cx.inline_reexport_inline = reexports;
    let (inline_core, reexport_core) = inline_core_import_maps(bundle, bundle.entry);
    cx.inline_core_imports = inline_core;
    cx.inline_reexport_core = reexport_core;
    cx.inline_foreign_imports = inline_foreign_import_maps(bundle, bundle.entry);
    let (inline_foreign_sigs, inline_foreign_rets) =
        inline_foreign_import_signature_maps(bundle, bundle.entry);
    cx.inline_foreign_sigs = inline_foreign_sigs;
    cx.inline_foreign_rets = inline_foreign_rets;
    cx.inline_reexport_foreign = inline_foreign_reexport_maps(bundle, bundle.entry);
    let (inline_foreign_reexport_sigs, inline_foreign_reexport_rets) =
        inline_foreign_reexport_signature_maps(bundle, bundle.entry);
    cx.inline_foreign_reexport_sigs = inline_foreign_reexport_sigs;
    cx.inline_foreign_reexport_rets = inline_foreign_reexport_rets;
    emit_program_items(&cx, &entry.items, &mut out, false, false);

    emit_test_fns(&cx, &tests, None, &mut out);
    emit_fuzz_main(&cx, &tests[target], target, file_label, &mut out);
    Ok(strip_unused_os_signal_prelude(strip_unused_raylib_prelude(strip_unused_term_prelude(strip_unused_gc_prelude(
        strip_unused_txn_prelude(strip_unused_mem_prelude(out)),
    )))))
}

/// See `emit_bundle_fuzz`'s doc comment for the overall shape. `test` is the
/// chosen property test, `idx` its position (`jet_prop_{idx}` is its body fn,
/// already emitted by `emit_test_fns`).
fn emit_fuzz_main(
    cx: &Cx,
    test_case: &TestCase<'_>,
    idx: usize,
    file_label: &str,
    out: &mut String,
) {
    const SHRINK_STEPS: usize = 2000;
    let test = test_case.test;
    let n = test.params.len();
    let types: Vec<String> = test.params.iter().map(|p| cx.rust_type(&p.ty)).collect();
    let tuple_ty = format!("({},)", types.join(", "));
    let call_args: Vec<String> = (0..n).map(|k| format!("input.{}.clone()", k)).collect();
    let gen_components: Vec<String> = types
        .iter()
        .map(|t| format!("<{} as JetGen>::generate(rng)", t))
        .collect();
    let renders: Vec<String> = test
        .params
        .iter()
        .enumerate()
        .map(|(k, p)| format!("format!(\"{} = {{}}\", input.{}.render())", p.name, k))
        .collect();
    let name_lit = escape_rust_str(
        test.name
            .as_deref()
            .expect("sema resolves every test marker name before codegen"),
    );
    let file_lit = escape_rust_str(file_label);

    out.push_str("fn main() {\n");
    out.push_str("    jet_std_env_init();\n");
    out.push_str("    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n");
    out.push_str("    let corpus_dir = std::env::var(\"JET_FUZZ_CORPUS\").unwrap_or_else(|_| \".jet-fuzz-corpus\".to_string());\n");
    out.push_str("    let _ = std::fs::create_dir_all(&corpus_dir);\n");
    out.push_str("    let iterations: u64 = std::env::var(\"JET_FUZZ_ITERATIONS\").ok().and_then(|s| s.parse().ok()).unwrap_or(1000);\n");
    out.push_str("    let time_budget_ms: Option<u64> = std::env::var(\"JET_FUZZ_TIME_MS\").ok().and_then(|s| s.parse().ok());\n");
    out.push_str("    let base_seed: u64 = std::env::var(\"JET_FUZZ_SEED\").ok().and_then(|s| s.parse().ok()).unwrap_or(0x5EED_1234_ABCD_0001u64);\n");
    out.push_str(&format!("    let name = {};\n", name_lit));
    out.push_str(&format!("    let file_label = {};\n", file_lit));
    out.push_str(&format!(
        "    let run = |input: &{}| -> Result<(), String> {{ jet_prop_{}({}) }};\n",
        tuple_ty,
        idx,
        call_args.join(", ")
    ));
    out.push_str(&format!(
        "    let gen_input = |rng: &mut JetRng| -> {} {{ ({},) }};\n",
        tuple_ty,
        gen_components.join(", ")
    ));

    // Corpus replay: each entry is the seed of a past failure. A seed alone is
    // a full, exact reproduction (same PRNG, same first draw), so the corpus
    // never needs to serialize the generated value itself.
    out.push_str("    let mut corpus_seeds: Vec<u64> = Vec::new();\n");
    out.push_str("    if let Ok(entries) = std::fs::read_dir(&corpus_dir) {\n");
    out.push_str("        for e in entries.flatten() {\n");
    out.push_str("            if let Ok(s) = std::fs::read_to_string(e.path()) {\n");
    out.push_str("                if let Ok(seed) = s.trim().parse::<u64>() { corpus_seeds.push(seed); }\n");
    out.push_str("            }\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    corpus_seeds.sort();\n");
    out.push_str("    for seed in &corpus_seeds {\n");
    out.push_str("        let mut rng = JetRng::new(*seed);\n");
    out.push_str("        let input = gen_input(&mut rng);\n");
    out.push_str("        if let Err(msg) = run(&input) {\n");
    out.push_str("            println!(\"{}: FAIL (corpus replay, seed={})\", name, seed);\n");
    out.push_str("            eprintln!(\"  {}\", msg);\n");
    out.push_str(&format!("            let args = vec![{}];\n", renders.join(", ")));
    out.push_str("            println!(\"  input: {}\", args.join(\", \"));\n");
    out.push_str("            println!(\"repro: JET_PROP_SEED={} jet test {}\", seed, file_label);\n");
    out.push_str("            std::process::exit(1);\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    println!(\"corpus: {} case(s) replayed clean\", corpus_seeds.len());\n");

    out.push_str("    let start = std::time::Instant::now();\n");
    out.push_str("    let mut driver_rng = JetRng::new(base_seed);\n");
    out.push_str("    let mut n: u64 = 0;\n");
    out.push_str("    loop {\n");
    out.push_str("        if n >= iterations { break; }\n");
    out.push_str("        if let Some(ms) = time_budget_ms { if start.elapsed().as_millis() as u64 >= ms { break; } }\n");
    out.push_str("        let seed = driver_rng.next_u64();\n");
    out.push_str("        let mut rng = JetRng::new(seed);\n");
    out.push_str("        let mut input = gen_input(&mut rng);\n");
    out.push_str("        n += 1;\n");
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
        out.push_str(
            "                        input = trial; msg = m; improved = true; break;\n",
        );
        out.push_str("                    }\n");
        out.push_str("                }\n");
    }
    out.push_str("            }\n");
    out.push_str(&format!("            let args = vec![{}];\n", renders.join(", ")));
    out.push_str("            let file_name = format!(\"{}/seed_{}.txt\", corpus_dir, seed);\n");
    out.push_str("            let _ = std::fs::write(&file_name, format!(\"{}\", seed));\n");
    out.push_str("            println!(\"{}: FAIL (after {} iteration(s))\", name, n);\n");
    out.push_str("            eprintln!(\"  {}\", msg);\n");
    out.push_str("            println!(\"  minimized input: {}\", args.join(\", \"));\n");
    out.push_str("            println!(\"  seed: {}\", seed);\n");
    out.push_str("            println!(\"  saved: {}\", file_name);\n");
    out.push_str("            println!(\"repro: JET_PROP_SEED={} jet test {}\", seed, file_label);\n");
    out.push_str("            std::process::exit(1);\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("    println!(\"{}: {} iteration(s), no failure found\", name, n);\n");
    out.push_str("}\n");
}

/// D-BENCH1: emit a benchmark harness binary — every definition plus a `main`
/// that times each `#Bench("…") { }` region and reports ns/iter + ops/sec.
/// Mirrors `emit_bundle_tests`; the only divergence is the per-block tail,
/// which wraps each body in an auto-scaled timed loop instead of a pass/fail
/// check. Each body is emitted exactly like a `#Test` body (a bare statement
/// list in a `Result<(), String>` fn), so `return Err(…)` from `require` stays
/// valid; the timing wrapper aborts the benchmark command on such an error
/// instead of printing false timings.
pub fn emit_bundle_benches(bundle: &ProgramBundle, link: Option<&FfiLink>) -> String {
    let entry = &bundle.modules[bundle.entry];
    let bundle_auto_derives =
        crate::Traits::TraitRegistry::bundle_auto_derives(bundle, &bundle.name_ledger);
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
    let edition_year = bundle.edition.parse::<u16>().unwrap_or(2027);
    out.push_str(&format!(
        "const __JET_PACKAGE_EDITION: u16 = {edition_year};\n\n"
    ));
    if let Some(ffi) = link {
        out.push_str(&format!("extern crate {};\n\n", ffi.crate_name));
    }
    push_cached_runtime(&mut out, link);
    out.push_str(TEST_PRELUDE);
    out.push_str(TEST_REPORT_PRELUDE);
    if want_prop_prelude {
        out.push_str(PROP_PRELUDE);
    }
    if coverage {
        out.push_str(COV_PRELUDE);
    }
    if needs_embedded_runtime(bundle) {
        push_corelib_prelude(&mut out, &bundle.used_core, uses_stream(bundle));
        out.push_str(scheduler_prelude_for_emit(uses_native_scheduler(bundle)));
        out.push_str(UI_PRELUDE);
        if uses_gtk_backend(bundle) {
            out.push_str(UI_GTK_PRELUDE);
        }
        push_app_preludes(&mut out, &bundle.used_core);
    }
    out.push('\n');

    let import_mods = import_mod_map(bundle, bundle.entry);
    let extern_funcs = bundle_extern_funcs(bundle);

    for (i, module) in bundle.modules.iter().enumerate() {
        if i == bundle.entry {
            continue;
        }
        let ns = module.alias.clone();
        out.push_str(&format!("mod {} {{\n", mangle(&ns)));
        out.push_str(MOD_USE);
        out.push_str("use super::jet_stack_enter;\n");
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        cx.foreign_undos = bundle_foreign_undos(bundle);
        apply_auto_derives(&mut cx, &bundle_auto_derives[i]);
        cx.module_alias = module.alias.clone();
        cx.core_archive_source = bundle.modules.iter().any(|module| module.alias == "core_archive");
        cx.test_mode = true;
        cx.coverage = coverage;
        cx.import_mods = import_mod_map(bundle, i);
        cx.foreign_types = foreign_type_map(bundle, i);
        TIR::register_imported_struct_shapes(&mut cx, bundle, i);
        register_foreign_enum_variants(&mut cx, bundle, i);
        update_cloneability_with_foreign_types(&mut cx, &module.items);
        cx.reexport_calls = reexport_call_map(bundle, i);
        cx.import_sigs = import_sig_map(bundle, i);
        cx.import_rets = import_ret_map(bundle, i);
        cx.core_imports = core_import_map(bundle, i);
        register_core_import_surfaces(&mut cx);
        cx.used_core = bundle.used_core.clone();
        cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
        cx.root_prefix = "super::".to_string();
        let (uinline, ufile) = unqualified_import_maps(bundle, i);
        cx.unqualified_inline = uinline;
        cx.unqualified_file = ufile;
        let (inline, file, names, reexports) = inline_import_maps(bundle, i);
        cx.inline_unqualified = inline;
        cx.inline_unqualified_file = file;
        cx.inline_import_names = names;
        cx.inline_reexport_inline = reexports;
        let (inline_core, reexport_core) = inline_core_import_maps(bundle, i);
        cx.inline_core_imports = inline_core;
        cx.inline_reexport_core = reexport_core;
        cx.inline_foreign_imports = inline_foreign_import_maps(bundle, i);
        let (inline_foreign_sigs, inline_foreign_rets) =
            inline_foreign_import_signature_maps(bundle, i);
        cx.inline_foreign_sigs = inline_foreign_sigs;
        cx.inline_foreign_rets = inline_foreign_rets;
        cx.inline_reexport_foreign = inline_foreign_reexport_maps(bundle, i);
        let (inline_foreign_reexport_sigs, inline_foreign_reexport_rets) =
            inline_foreign_reexport_signature_maps(bundle, i);
        cx.inline_foreign_reexport_sigs = inline_foreign_reexport_sigs;
        cx.inline_foreign_reexport_rets = inline_foreign_reexport_rets;
        emit_program_items(&cx, &module.items, &mut out, false, true);
        out.push_str("}\n\n");
    }

    let mut cx = build_cx_items(
        &entry.items,
        &entry.source,
        &entry.display,
        link,
        &extern_funcs,
    );
    cx.foreign_undos = bundle_foreign_undos(bundle);
    apply_auto_derives(&mut cx, &bundle_auto_derives[bundle.entry]);
    cx.module_alias = entry.alias.clone();
    cx.core_archive_source = bundle.modules.iter().any(|module| module.alias == "core_archive");
    cx.test_mode = true;
    cx.coverage = coverage;
    cx.import_mods = import_mods;
    cx.foreign_types = foreign_type_map(bundle, bundle.entry);
    TIR::register_imported_struct_shapes(&mut cx, bundle, bundle.entry);
    register_foreign_enum_variants(&mut cx, bundle, bundle.entry);
    update_cloneability_with_foreign_types(&mut cx, &entry.items);
    cx.reexport_calls = reexport_call_map(bundle, bundle.entry);
    cx.import_sigs = import_sig_map(bundle, bundle.entry);
    cx.import_rets = import_ret_map(bundle, bundle.entry);
    cx.core_imports = core_import_map(bundle, bundle.entry);
    register_core_import_surfaces(&mut cx);
    cx.used_core = bundle.used_core.clone();
    cx.ffi_callback_fns = bundle.ffi_callback_fns.clone();
    let (uinline, ufile) = unqualified_import_maps(bundle, bundle.entry);
    cx.unqualified_inline = uinline;
    cx.unqualified_file = ufile;
    let (inline, file, names, reexports) = inline_import_maps(bundle, bundle.entry);
    cx.inline_unqualified = inline;
    cx.inline_unqualified_file = file;
    cx.inline_import_names = names;
    cx.inline_reexport_inline = reexports;
    let (inline_core, reexport_core) = inline_core_import_maps(bundle, bundle.entry);
    cx.inline_core_imports = inline_core;
    cx.inline_reexport_core = reexport_core;
    cx.inline_foreign_imports = inline_foreign_import_maps(bundle, bundle.entry);
    let (inline_foreign_sigs, inline_foreign_rets) =
        inline_foreign_import_signature_maps(bundle, bundle.entry);
    cx.inline_foreign_sigs = inline_foreign_sigs;
    cx.inline_foreign_rets = inline_foreign_rets;
    cx.inline_reexport_foreign = inline_foreign_reexport_maps(bundle, bundle.entry);
    let (inline_foreign_reexport_sigs, inline_foreign_reexport_rets) =
        inline_foreign_reexport_signature_maps(bundle, bundle.entry);
    cx.inline_foreign_reexport_sigs = inline_foreign_reexport_sigs;
    cx.inline_foreign_reexport_rets = inline_foreign_reexport_rets;
    emit_program_items(&cx, &entry.items, &mut out, false, false);

    out.push_str(
        "fn jet_bench_check(result: Result<(), String>) {\n\
    if let Err(error) = result {\n\
        eprintln!(\"bench region failed: {}\", error);\n\
        std::process::exit(70);\n\
    }\n\
}\n\n",
    );

    // One body fn + one timing wrapper per bench. The body fn is shaped exactly
    // like a test fn (so `require`'s `return Err(…)` compiles); the wrapper
    // auto-scales the iteration count until a batch lasts >= 1ms, then collects
    // 20 exact batch-duration samples and returns (elapsed_ns, iters). The
    // command layer alone projects means/stddev or feeds exact rationals into
    // the canonical performance-budget provider/evaluator.
    for (i, bench) in benches.iter().enumerate() {
        out.push_str(&format!(
            "fn jet_bench_body_{}() -> Result<(), String> {{\n",
            i
        ));
        emit_test_body(&cx, &bench.body, &mut out);
        out.push_str("    Ok(())\n");
        out.push_str("}\n\n");

        out.push_str(&format!("fn jet_bench_{}() -> (Vec<u128>, Vec<(usize, usize)>, u64) {{\n", i));
        out.push_str("    let mut iters: u64 = 1;\n");
        out.push_str("    while iters < (1u64 << 30) {\n");
        out.push_str("        let t0 = std::time::Instant::now();\n");
        out.push_str(&format!(
            "        for _ in 0..iters {{ jet_bench_check(std::hint::black_box(jet_bench_body_{}())); }}\n",
            i
        ));
        out.push_str("        if t0.elapsed().as_millis() >= 1 { break; }\n");
        out.push_str("        iters = iters.saturating_mul(2);\n");
        out.push_str("    }\n");
        out.push_str("    let mut samples: Vec<u128> = Vec::new();\n");
        out.push_str("    let mut allocations: Vec<(usize, usize)> = Vec::new();\n");
        out.push_str("    for _ in 0..20 {\n");
        out.push_str("        jet_allocation_probe_reset();\n");
        out.push_str("        let t0 = std::time::Instant::now();\n");
        out.push_str(&format!(
            "        for _ in 0..iters {{ jet_bench_check(std::hint::black_box(jet_bench_body_{}())); }}\n",
            i
        ));
        out.push_str("        samples.push(t0.elapsed().as_nanos());\n");
        out.push_str("        allocations.push(jet_allocation_probe_take());\n");
        out.push_str("    }\n");
        out.push_str("    (samples, allocations, iters)\n");
        out.push_str("}\n\n");
    }

    out.push_str("fn main() {\n");
    out.push_str("    jet_std_env_init();\n");
    out.push_str("    jet_gc::runtime_or_exit(jet_gc::initialize_trace());\n");
    out.push_str("    let bench_filter = std::env::var(\"JET_BENCH_FILTER\").ok();\n");
    out.push_str("    fn hex(bytes: &[u8]) -> String { const H: &[u8; 16] = b\"0123456789abcdef\"; let mut out = String::with_capacity(bytes.len() * 2); for byte in bytes { out.push(H[(byte >> 4) as usize] as char); out.push(H[(byte & 15) as usize] as char); } out }\n");
    for (i, bench) in benches.iter().enumerate() {
        let name = escape_rust_str(
            bench
                .name
                .as_deref()
                .expect("sema resolves every benchmark marker name before codegen"),
        );
        out.push_str(&format!(
            "    {{\n        let name = {};\n        if bench_filter.as_ref().map_or(true, |filter| name.contains(filter.as_str())) {{\n        let (samples, allocations, iters) = jet_bench_{}();\n",
            name,
            i
        ));
        out.push_str("        print!(\"JETBENCH1\\t{}\\t{}\", hex(name.as_bytes()), iters);\n");
        out.push_str("        for sample in samples { print!(\"\\t{}\", sample); }\n        println!();\n");
        out.push_str("        print!(\"JETALLOC1\\t{}\\t{}\", hex(name.as_bytes()), iters);\n");
        out.push_str("        for (count, bytes) in allocations { print!(\"\\t{}:{}\", count, bytes); }\n        println!();\n");
        out.push_str("        }\n    }\n");
    }
    out.push_str("}\n");
    strip_unused_os_signal_prelude(strip_unused_raylib_prelude(strip_unused_term_prelude(strip_unused_gc_prelude(
        strip_unused_txn_prelude(strip_unused_mem_prelude(out)),
    ))))
}
