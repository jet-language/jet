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

use crate::Syntax;
use crate::AST::{FfiLink, Item, ProgramBundle, Type};
pub(crate) use crate::AST::{mangle, mangle_generated, mangle_path};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Mutex, OnceLock};

pub(crate) use jet_foundation::MIR::{
    MirArtifactPlan, MirProgram, MirRuntimePartId,
};

/// The generated-name prefix used by emitter format strings. Keeping this as
/// one allocator argument lets every emitted temporary share the same machine
/// lane without embedding a second spelling in an emitter template.
pub(crate) fn generated_prefix() -> String {
    mangle_generated("")
}

pub(crate) fn canonical_prefix() -> String {
    mangle("")
}

/// Render the backend name for a source trait. Built-in traits with a
/// canonical Prelude declaration may keep a different Rust name from the
/// source spelling; ordinary user traits use the normal mangler.
pub(crate) fn rust_trait_name(trait_name: &str) -> String {
    crate::Generics::rust_trait_bound(trait_name)
        .map(str::to_string)
        .unwrap_or_else(|| mangle(trait_name))
}


// Native cache identity must digest the emitted stdlib closure, not the whole
// compiler binary. Keep the fixed Prelude digest for the process and keep the
// used-Core digests bounded: a long-lived compiler service can see many
// programs, but the digest cache must not become another unbounded cache.
const CORELIB_DIGEST_CACHE_LIMIT: usize = 32;

pub const JET_CANONICAL_FONT_BYTES: &[u8] =
    include_bytes!("../../../../site/assets/fonts/exo2-700.ttf");
pub const JET_CANONICAL_ARABIC_FONT_BYTES: &[u8] =
    include_bytes!("../../../../site/assets/fonts/noto-sans-arabic-regular.ttf");
pub const JET_CANONICAL_SYMBOLS_FONT_BYTES: &[u8] =
    include_bytes!("../../../../site/assets/fonts/noto-sans-symbols2-regular.ttf");

/// Emit the checked canonical font bytes into generated native sources.  The
/// generated program must not resolve a host filesystem font, or glyph IDs and
/// positions would vary across tiers and machines.
fn canonical_font_prelude() -> String {
    format!(
        "const JET_CANONICAL_FONT_BYTES: &[u8] = &{:?};\n\
         const JET_CANONICAL_ARABIC_FONT_BYTES: &[u8] = &{:?};\n\
         const JET_CANONICAL_SYMBOLS_FONT_BYTES: &[u8] = &{:?};\n",
        JET_CANONICAL_FONT_BYTES,
        JET_CANONICAL_ARABIC_FONT_BYTES,
        JET_CANONICAL_SYMBOLS_FONT_BYTES
    )
}
static CACHED_RUNTIME_FINGERPRINT: OnceLock<String> = OnceLock::new();
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CoreEmissionFingerprintKey {
    used_core: Vec<String>,
    active_os: String,
    edition: String,
    force_corelib: bool,
    test_harness: bool,
    release_inspect: Option<String>,
    devtools_panel_code: bool,
    devtools_stream_code: bool,
}

static CORELIB_EMISSION_FINGERPRINTS: OnceLock<
    Mutex<BTreeMap<CoreEmissionFingerprintKey, String>>,
> = OnceLock::new();

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

mod Context;
pub mod Embedding;
mod Imports;
mod Items;
pub use crate::task_group;
pub mod Library;
pub mod Plugin;
mod Statement;
pub mod TIR;
pub mod MIREval;
mod Receipt;
pub mod MIRRust;
pub mod MIRWeb;
mod Tuples;
mod Utils;
mod VariadicBound;
mod Web;

/// D-REPORT-TEST1=A: host-side `jet prove` uses the same source that test
/// harnesses receive below as their report prelude.
#[allow(dead_code)]
pub mod test_report {
    include!("../../../jet-foundation/src/Report.rs");
    include!("../../../jet-foundation/src/Evidence.rs");
    include!("../Prelude/Core/TestEvidence.rs");
    include!("../Prelude/CoreLib/Top/TestingShared.rs");
    include!("../Prelude/TestReport.rs");
}

pub(crate) use Context::*;
pub use Embedding::{export_surface, ExportFunction, ExportScalar};
pub(crate) use Imports::*;
pub use Library::{emit_library, LibraryArtifacts, LibraryExport};
pub use Plugin::{emit_plugin, PluginArtifacts};
pub(crate) use Statement::*;
pub(crate) use Tuples::*;
pub(crate) use Utils::*;
pub use Web::build_wasm_jet_source_map;

/// Build the interpreter's bundle-wide Core alias map from the same import
/// resolver used by AOT and JIT lowering. In particular, member-list imports
/// such as `use core.math.[abs, min]` must not fall back to a second policy.
pub fn core_imports_for_bundle(bundle: &ProgramBundle) -> BTreeMap<String, String> {
    let mut imports = BTreeMap::new();
    for module_idx in 0..bundle.modules.len() {
        for (alias, module) in core_import_map(bundle, module_idx) {
            imports.entry(alias).or_insert(module);
        }
    }
    imports
}

/// Generated-only wrappers around the Foundation-independent native host
/// registry. The compile-time policy is emitted immediately before the
/// Prelude, so release artifacts can dead-code-eliminate every callback.
const DEVTOOLS_NATIVE_WRAPPERS_PRELUDE: &str = r#"
#[inline(always)]
pub fn jet_devtools_install_native_host(
    session_id: impl Into<String>,
    host: JetDevtoolsNativeHostHandle,
) -> Result<JetDevtoolsNativeHostGuard, String> {
    if !JET_DEVTOOLS_RUNTIME_ENABLED {
        return Err("native devtools host is disabled by the release policy".to_string());
    }
    jet_devtools_install_native_host_unchecked(session_id, host)
}

#[inline(always)]
pub fn jet_devtools_native_bind_session(session_id: &str) -> Result<(), String> {
    if !JET_DEVTOOLS_RUNTIME_ENABLED {
        return Err("native devtools session is disabled by the release policy".to_string());
    }
    jet_devtools_native_bind_session_unchecked(session_id)
}

#[inline(always)]
pub fn jet_devtools_native_clear_session(session_id: &str) {
    if JET_DEVTOOLS_RUNTIME_ENABLED {
        jet_devtools_native_clear_session_unchecked(session_id);
    }
}

#[inline(always)]
pub fn jet_devtools_publish_event_for_session(session_id: &str, event: JetDevtoolsEvent) {
    if JET_DEVTOOLS_RUNTIME_ENABLED {
        jet_devtools_publish_event_for_session_unchecked(session_id, event);
    }
}

#[inline(always)]
pub fn jet_devtools_native_window_open(width: u32, height: u32) {
    if JET_DEVTOOLS_RUNTIME_ENABLED {
        jet_devtools_native_window_open_unchecked(width, height);
    }
}

#[inline(always)]
pub fn jet_devtools_native_window_close() {
    if JET_DEVTOOLS_RUNTIME_ENABLED {
        jet_devtools_native_window_close_unchecked();
    }
}

#[inline(always)]
pub fn jet_devtools_native_input(input: JetDevtoolsNativeInput) -> bool {
    if JET_DEVTOOLS_RUNTIME_ENABLED {
        return jet_devtools_native_input_unchecked(input);
    }
    false
}

#[inline(always)]
pub fn jet_devtools_native_frame_begin(
    frame_index: u64,
    width: u32,
    height: u32,
) -> Vec<JetDevtoolsNativeDrawCommand> {
    if JET_DEVTOOLS_RUNTIME_ENABLED {
        return jet_devtools_native_frame_begin_unchecked(frame_index, width, height);
    }
    Vec::new()
}

#[inline(always)]
pub fn jet_devtools_native_draw(command: JetDevtoolsNativeDrawCommand) {
    if JET_DEVTOOLS_RUNTIME_ENABLED {
        jet_devtools_native_draw_unchecked(command);
    }
}

#[inline(always)]
pub fn jet_devtools_native_frame_end() {
    if JET_DEVTOOLS_RUNTIME_ENABLED {
        jet_devtools_native_frame_end_unchecked();
    }
}
"#;

/// Emitted at the top of every program: core runtime helpers used by generated Rust.
/// Parts stay in source order so splitting ownership never changes generated bytes.
///
/// Every part here is spliced flat into the generated crate root, so one
/// namespace holds all of them: item names must be unique across parts, a part
/// may only use `pub`, `pub(crate)`, or private visibility (`pub(super)` has
/// no parent at the root), and Foundation items are named unqualified or
/// through the `jet_foundation` facade (`push_foundation_facade`).
///
/// `Outcome.rs` is not listed: it is the `outcome` root of
/// `EMBEDDED_PRELUDE_PARTS`, whose dependency closure (JSON kernel,
/// `RuntimeDiagnosticCore`) `push_cached_runtime_body` emits before these.
const PRELUDE_PARTS: &[&str] = &[
    // D-SHAPE-ONE1=A: the compiler and every generated tier consume this
    // single shape vocabulary; no Prelude-local shape enum is permitted.
    include_str!("../../../jet-foundation/src/Shape.rs"),
    // D-DEVR-LAW1=A / I9: every execution tier receives the same development
    // act receipt shape and serializer.
    include_str!("../Prelude/DevelopmentReceipt.rs"),
    include_str!("../Prelude/Core/Receipt.rs"),
    // D-DX-DEVTOOLS1: one typed observation protocol shared by every execution tier.
    // `Prelude/Devtools.rs` is a symlink to the Foundation module; its
    // host-crate native region is cut by `push_prelude` (see
    // `HOST_DEVTOOLS_NATIVE_BEGIN`) and replaced by the two parts below, so the
    // native registry source enters the program exactly once.
    DEVTOOLS_SOURCE,
    include_str!("../../../jet-foundation/src/DevtoolsNative.rs"),
    DEVTOOLS_NATIVE_WRAPPERS_PRELUDE,
    include_str!("../Prelude/FaultInjection.rs"),
    // D-BENCH-KEEP1=A: every engine includes the same black-box sink source;
    // resident adapters only marshal their carrier into this function.
    include_str!("../Prelude/Core/Keep.rs"),
    include_str!("../Prelude/JobQueueTypes.rs"),
    include_str!("../Prelude/Job.rs"),
    include_str!("../Prelude/Core/Option.rs"),
    include_str!("../Prelude/Core/FixedList.rs"),
    // D-SOA-TIER1=A: THE shared column store and the one gather read, plus the
    // Prelude-owned `[S]` facade over it. Right after FixedList because the
    // read reuses that shared bounds stop; the Cranelift host and the
    // interpreter ambient include the same store source.
    include_str!("../Prelude/Core/Columns.rs"),
    include_str!("../Prelude/Core/ColumnList.rs"),
    include_str!("../Prelude/Core/UnicodeString.rs"),
    include_str!("../Prelude/Core/Ascii.rs"),
    include_str!("../Prelude/Core/ProcessArgs.rs"),
    // D-STR-CONCAT1: the owned String `+`/`+=` result is one kernel for every
    // execution tier; the evaluator and JIT include this same source.
    include_str!("../Prelude/Core/StringConcat.rs"),
    // D-MEM-COPYSEM1=A: one Rust Prelude copy kernel for native and wasm.
    include_str!("../Prelude/Core/ViewCopy.rs"),
    include_str!("../Prelude/Core/Loadable.rs"),
    include_str!("../Prelude/Core/Values.rs"),
    include_str!("../Prelude/Core/TextValues.rs"),
    include_str!("../Prelude/Core/RangeBounds.rs"),
    include_str!("../Prelude/Core/InlineRange.rs"),
    include_str!("../Prelude/Core/Disjoint.rs"),
    include_str!("../Prelude/Core/ExpiringSecret.rs"),
    include_str!("../Prelude/Core/SetAlgebra.rs"),
    include_str!("../Prelude/Core/Duration.rs"),
    include_str!("../Prelude/Core/Measurement.rs"),
    include_str!("../Prelude/Core/TimeMonotonic.rs"),
    include_str!("../Prelude/Core/Time.rs"),
    include_str!("../Prelude/Core/Sketch.rs"),
    include_str!("../Prelude/Core/Contracts.rs"),
    include_str!("../Prelude/Core/MapKey.rs"),
    include_str!("../Prelude/Core/RuntimeStack.rs"),
    include_str!("../Prelude/Core/Authority.rs"),
    // D-WRAP-SCOPE1=A / I9: one fixed-width arithmetic operation table for
    // AOT, JIT, TIR evaluation, comptime, and web adapters.
    include_str!("../Prelude/Core/FixedArithmetic.rs"),
    include_str!("../Prelude/Core/FloatOrdering.rs"),
    include_str!("../Prelude/Core/ParallelKernel.rs"),
    // D-CLAIM1: every generated producer writes the same typed evidence
    // records as Foundation; Core.rs calls its root writer below.
    // Evidence names crate::Facts and crate::ResourceSchedule. Keep those
    // modules in the fixed runtime so a print-only program still typechecks.
    RESOURCE_FACTS_PRELUDE,
    include_str!("../../../jet-foundation/src/Evidence.rs"),
    include_str!("../Prelude/Core/TestEvidence.rs"),
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
    include_str!("../Prelude/Core/Bytes.rs"),
    // The one parallel-walk kernel. AOT embeds it here; the resident JIT host,
    // the interpreter ambient and the comptime evaluator `include!` the same
    // file, so no tier re-implements directory traversal (I9). A nested
    // `include!` inside an embedded part cannot work: the generated crate has
    // no Prelude tree to read from.
    include_str!("../Prelude/Core/FSIgnore.rs"),
    include_str!("../Prelude/Core/FSWalk.rs"),
    include_str!("../Prelude/CoreLib/JetStd/Iter.rs"),
    include_str!("../Prelude/Core/CollectionFailure.rs"),
    include_str!("../Prelude/Core/SortKernel.rs"),
    include_str!("../Prelude/Core/Collections.rs"),
    include_str!("../Prelude/Memo.rs"),
    include_str!("../Prelude/SharedProtocol.rs"),
    TERM_PRELUDE,
    // D-DX-HUMANOUTPUT1: capability-aware human rendering shared by every tier.
    include_str!("../Prelude/Core/HumanOutput.rs"),
    // D-TERM1 / I9: the one terminal key kernel. AOT embeds it here; the
    // canonical TIR evaluator and the resident JIT host `include!` the same
    // file, so no tier re-decodes key bytes or re-states raw-mode entry.
    include_str!("../Prelude/Core/TermKey.rs"),
    include_str!("../Prelude/Core/RuntimeControl.rs"),
    include_str!("../../../jet-foundation/src/NumericConversion.rs"),
    include_str!("../Prelude/Core/NumericRuntime.rs"),
    include_str!("../Prelude/Observe.rs"),
    include_str!("../../../jet-foundation/src/ExactUnitConversion.rs"),
    include_str!("../Prelude/Core/NumericWeb.rs"),
    include_str!("../../../jet-foundation/src/StructuralDebug.rs"),
    // D-SHIFT1: `binary.Reader` / `text.Cursor`. Owned by jet-foundation so the
    // AOT prelude and the canonical TIR evaluator run one kernel (I9).
    include_str!("../../../jet-foundation/src/StreamCursor.rs"),
    // D-BINPAT1 / I9: one binary-pattern scan kernel. AST-shaped engines only
    // marshal pattern parts and project its values.
    include_str!("../../../jet-foundation/src/Prelude/MatchScan.rs"),
];

const OUTCOME_SOURCE: &str = include_str!("../../../jet-foundation/src/Outcome.rs");
const HOST_RUNTIME_STOP_BEGIN: &str = "// JET_HOST_RUNTIME_STOP_BEGIN";
const HOST_RUNTIME_SENTRY_BEGIN: &str = "// JET_HOST_RUNTIME_SENTRY_BEGIN";
const HOST_RUNTIME_SENTRY_END: &str = "// JET_HOST_RUNTIME_SENTRY_END";
const HOST_RUNTIME_STOP_END: &str = "// JET_HOST_RUNTIME_STOP_END";
const DEVTOOLS_SOURCE: &str = include_str!("../Prelude/Devtools.rs");
const TERM_PRELUDE: &str = concat!(
    "\n// JET_VETTED_UNSAFE_BEGIN: jet_term_kernel\n\
     // AUDIT: D-IO-TERM1 keeps terminal detection, stream ownership, and the\n\
     // platform raw-mode ABI in this compiler-owned shared kernel.\n",
    include_str!("../Prelude/Term.rs"),
    "\n// JET_VETTED_UNSAFE_END: jet_term_kernel\n",
);
const HOST_DEVTOOLS_NATIVE_BEGIN: &str = "// JET_HOST_DEVTOOLS_NATIVE_BEGIN";
const HOST_DEVTOOLS_NATIVE_END: &str = "// JET_HOST_DEVTOOLS_NATIVE_END";

/// Embedded Prelude parts are a dependency graph, not a list of incidental
/// imports. A root part pulls in every transitive part it names before its own
/// source. Web and native emission use the same closure for the fixed
/// Outcome/report seam, so a boundary signature cannot strand a dependency.
enum EmbeddedPreludePartSource {
    Static(&'static str),
    Outcome,
}

struct EmbeddedPreludePart {
    name: &'static str,
    dependencies: &'static [&'static str],
    source: EmbeddedPreludePartSource,
}

const EMBEDDED_PRELUDE_PARTS: &[EmbeddedPreludePart] = &[
    EmbeddedPreludePart {
        name: "fixed_allocator",
        dependencies: &[],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\n// JET_VETTED_UNSAFE_BEGIN: jet_fixed_kernel\npub mod jet_fixed_kernel {\n",
            include_str!("../Prelude/Core/FixedAllocator.rs"),
            "\n}\n// JET_VETTED_UNSAFE_END: jet_fixed_kernel\n",
        )),
    },
    EmbeddedPreludePart {
        name: "runtime_diagnostic_core",
        dependencies: &[],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\npub mod RuntimeDiagnosticCore {\n",
            include_str!("../../../jet-foundation/src/RuntimeDiagnosticCore.rs"),
            "\n}\n",
        )),
    },
    EmbeddedPreludePart {
        name: "encoding_errors",
        dependencies: &[],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\nmod jet_encoding_errors {\n",
            include_str!("../../../jet-foundation/src/EncodingErrors.rs"),
            "\n}\n"
        )),
    },
    EmbeddedPreludePart {
        name: "json_number",
        dependencies: &[],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\nmod jet_json_number {\n",
            include_str!("../../../jet-foundation/src/JSONNumber.rs"),
            "\n}\n"
        )),
    },
    EmbeddedPreludePart {
        name: "encoding_json",
        dependencies: &["encoding_errors", "json_number"],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\nmod jet_encoding_json {\n",
            include_str!("../../../jet-foundation/src/EncodingJson.rs"),
            "\n}\n\n#[allow(non_snake_case)]\nmod EncodingJson { pub use crate::jet_encoding_json::*; }\n"
        )),
    },
    EmbeddedPreludePart {
        name: "outcome",
        dependencies: &["encoding_json", "runtime_diagnostic_core"],
        source: EmbeddedPreludePartSource::Outcome,
    },
    // The generated runtime uses the same rooted, no-follow SHA-256
    // implementation as Foundation. Keeping the full module here closes the
    // `core.fs` dependency without a second host-only wrapper.
    EmbeddedPreludePart {
        name: "sha256",
        dependencies: &[],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\n// JET_VETTED_UNSAFE_BEGIN: jet_foundation_sha256\n\
             // AUDIT: this Foundation SHA-256 module owns the bounded hostile-input\n\
             // tree-hash file boundary; its rooted authority uses checked no-follow\n\
             // descriptors and keeps every raw descriptor operation in this module.\n\
             #[allow(non_snake_case)]\nmod SHA256 {\n",
            include_str!("../../../jet-foundation/src/SHA256.rs"),
            "\n}\n// JET_VETTED_UNSAFE_END: jet_foundation_sha256\n",
        )),
    },
    EmbeddedPreludePart {
        name: "performance_budget",
        dependencies: &["sha256"],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\n#[allow(non_snake_case)]\nmod PerformanceBudget {\n",
            include_str!("../../../jet-foundation/src/PerformanceBudget.rs"),
            "\n}\n",
        )),
    },
    // D-DATA-FLOW / I9: the one data-loader kernel (identity, redaction,
    // snapshot facts) behind `core.data`; `LazyTablePlan.rs` and `DataPlot.rs`
    // derive column identity from it in every tier.
    EmbeddedPreludePart {
        name: "prelude_data_flow",
        dependencies: &["sha256"],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\nmod jet_prelude_data_flow {\n",
            include_str!("../../../jet-foundation/src/PreludeDataFlow.rs"),
            "\n}\n",
        )),
    },
    // Checked Arrow C-data ownership behind `core.data.arrow`'s
    // `DataArrowBatch`. Its pointer reads are Foundation-vetted (I1).
    EmbeddedPreludePart {
        name: "arrow_data",
        dependencies: &[],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\n// JET_VETTED_UNSAFE_BEGIN: jet_arrow_data\n\
             // AUDIT: this region imports the Arrow C data interface, validates\n\
             // schema/length/offset/buffer bounds, and retains release callbacks.\n\
             // Safe Rust cannot express foreign ownership and callback ABIs.\n\
             // Callers must provide live C records and transfer each owner once;\n\
             // violating that contract can read outside a buffer or double-free.\n\
             mod jet_arrow_data {\n",
            include_str!("../../../jet-foundation/src/ArrowData.rs"),
            "\n}\n// JET_VETTED_UNSAFE_END: jet_arrow_data\n\
             #[allow(non_snake_case)]\n\
             mod ArrowData { pub use crate::jet_arrow_data::*; }\n",
        )),
    },
    // Checked Arrow file-reader registration depends on the C-data carrier
    // above and uses the generated crate's DataTree/ArrowData facades.
    EmbeddedPreludePart {
        name: "arrow_file_reader",
        dependencies: &["arrow_data"],
        source: EmbeddedPreludePartSource::Static(concat!(
            "\n// JET_VETTED_UNSAFE_BEGIN: jet_arrow_file_reader\n\
             // AUDIT: this region crosses the registered Arrow file-reader\n\
             // provider boundary and transfers imported batch ownership.\n\
             mod jet_arrow_file_reader {\n",
            include_str!("../../../jet-foundation/src/ArrowFileReader.rs"),
            "\n}\n// JET_VETTED_UNSAFE_END: jet_arrow_file_reader\n",
        )),
    },
];

/// Where a Foundation module lands in the generated crate.
enum FoundationPlacement {
    /// Spliced flat: its items are the crate root's own items.
    Root,
    /// Wrapped in this generated module.
    Module(&'static str),
}

/// The Foundation modules a generated program embeds, by their Foundation
/// name. Prelude sources compile both inside host crates, where
/// `jet_foundation` is a real dependency, and inside the generated crate,
/// which has none; this table is the one contract that lets a source spell
/// `jet_foundation::<Module>::<Item>` identically in both. It emits the
/// `mod jet_foundation` facade (`push_foundation_facade`) and drives the flat
/// import merge (`flat_prelude_import_bindings`). Optional source modules
/// emitted only under `needs_fs_runtime` use their conditional generated
/// aliases instead of entering this unconditional facade.
const FOUNDATION_PLACEMENTS: &[(&str, FoundationPlacement)] = &[
    ("Outcome", FoundationPlacement::Root),
    ("Shape", FoundationPlacement::Root),
    ("Devtools", FoundationPlacement::Root),
    ("DevtoolsNative", FoundationPlacement::Root),
    ("Evidence", FoundationPlacement::Root),
    ("NumericConversion", FoundationPlacement::Root),
    ("ExactUnitConversion", FoundationPlacement::Root),
    ("StructuralDebug", FoundationPlacement::Root),
    ("StreamCursor", FoundationPlacement::Root),
    ("RuntimeDiagnosticCore", FoundationPlacement::Module("RuntimeDiagnosticCore")),
    ("EncodingErrors", FoundationPlacement::Module("jet_encoding_errors")),
    ("JSONNumber", FoundationPlacement::Module("jet_json_number")),
    ("EncodingJson", FoundationPlacement::Module("jet_encoding_json")),
    ("DataTree", FoundationPlacement::Module("jet_foundation_datatree")),
    ("ArrowData", FoundationPlacement::Module("jet_arrow_data")),
    (
        "ArrowFileReader",
        FoundationPlacement::Module("jet_arrow_file_reader"),
    ),
    ("PreludeDataFlow", FoundationPlacement::Module("jet_prelude_data_flow")),
    ("SHA256", FoundationPlacement::Module("SHA256")),
    ("PerformanceBudget", FoundationPlacement::Module("PerformanceBudget")),
    ("Numeric", FoundationPlacement::Module("jet_foundation_numeric")),
    ("Facts", FoundationPlacement::Module("Facts")),
    ("ResourceSchedule", FoundationPlacement::Module("ResourceSchedule")),
];
fn foundation_placement(module: &str) -> Option<&'static FoundationPlacement> {
    FOUNDATION_PLACEMENTS
        .iter()
        .find(|(name, _)| *name == module)
        .map(|(_, placement)| placement)
}

/// `mod jet_foundation { pub mod <Module> { … } }`: every embedded Foundation
/// module re-exported under its host-crate path. Root-flat modules re-export
/// the crate root itself, so an item keeps the visibility Foundation gave it.
/// Arrow entries are conditional because their modules are demand-driven.
fn push_foundation_facade(out: &mut String, include_arrow: bool) {
    out.push_str("\n#[allow(non_snake_case, unused_imports)]\nmod jet_foundation {\n");
    for (module, placement) in FOUNDATION_PLACEMENTS {
        if !include_arrow && matches!(*module, "ArrowData" | "ArrowFileReader") {
            continue;
        }
        let target = match placement {
            FoundationPlacement::Root => "crate".to_string(),
            FoundationPlacement::Module(path) => format!("crate::{path}"),
        };
        out.push_str(&format!("    pub mod {module} {{ pub use {target}::*; }}\n"));
    }
    out.push_str("}\n");
}

fn push_numeric_runtime(out: &mut String) {
    let start = NUMERIC_FOUNDATION_SOURCE
        .find("// ── CtFraction")
        .expect("Numeric.rs CtFraction marker missing");
    out.push_str(
        "\n// JET_VETTED_UNSAFE_BEGIN: jet_foundation_numeric\n\
         // AUDIT: D-INTBIG1/D-DECIMAL1 keep the hazard-pointer exact numeric\n\
         // carrier and its raw i64 ownership adapters in this Foundation module.\n\
         #[allow(non_snake_case, unused_imports, dead_code)]\nmod jet_foundation_numeric {\nuse crate::jet_json_number::{json_decimal_lexeme, json_exact_integer_text};\nuse crate::{AllocError, jet_alloc_error};\nuse std::alloc::{alloc, dealloc, Layout};\nuse std::cell::Cell;\nuse std::fmt;\nuse std::ptr::{self, NonNull};\nuse std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};\nmod Syntax {\n    pub const TYPE_DECIMAL: &'static str = \"Decimal\";\n    pub const TYPE_FRACTION: &'static str = \"Fraction\";\n}\n#[derive(Clone, Debug)]\nenum CtValue {\n    Int(i64),\n    Bool(bool),\n    Str(String),\n    BigInt(CtBigInt),\n    Struct { type_name: String, fields: Vec<(String, CtValue)> },\n}\n",
    );
    let body = NUMERIC_FOUNDATION_SOURCE[start..]
        .replace("crate::AST::CtValue", "CtValue")
        .replace("crate::Syntax::", "Syntax::")
        .replace("crate::NumericConversion::", "crate::")
        .replace("crate::Outcome::", "crate::");
    out.push_str(&body);
    out.push_str("\n}\n// JET_VETTED_UNSAFE_END: jet_foundation_numeric\n");
}

/// Native builders split this exact block into the content-addressed runtime
/// rlib. Keep the markers stable: emitted Rust remains a complete standalone
/// program, while the AOT link seam can replace the block with one `--extern`.
pub const CACHED_RUNTIME_BEGIN: &str = "// jet:cached-runtime-begin\n";
pub const CACHED_RUNTIME_END: &str = "// jet:cached-runtime-end\n";
pub const CACHED_CORE_BEGIN: &str = "// jet:cached-core-begin\n";
pub const CACHED_CORE_END: &str = "// jet:cached-core-end\n";

fn push_prelude(out: &mut String, devtools_enabled: bool, local_rail_enabled: bool) {
    // The typed MIR execution policy is lowered once into these constants.
    // Devtools.rs and Observe.rs can then make publication/install calls
    // compile away in stripped release artifacts without a second policy
    // surface or a host-local semantic copy.
    out.push_str(&format!(
        "\nconst JET_DEVTOOLS_RUNTIME_ENABLED: bool = {devtools_enabled};\n\
const JET_DEVTOOLS_LOCAL_RAIL_ENABLED: bool = {local_rail_enabled};\n\
#[inline(always)]\n\
fn jet_web_runtime_devtools_enabled() -> bool {{ JET_DEVTOOLS_RUNTIME_ENABLED }}\n\
#[inline(always)]\n\
fn jet_web_runtime_history_enabled() -> bool {{ JET_DEVTOOLS_LOCAL_RAIL_ENABLED }}\n"
    ));
    push_numeric_runtime(out);
    for part in PRELUDE_PARTS {
        if *part == DEVTOOLS_SOURCE {
            push_embedded_devtools(out);
        } else {
            out.push_str(part);
        }
    }
}

/// `Prelude/Devtools.rs` is the Foundation module itself. Its host-crate
/// native region (`include!("DevtoolsNative.rs")` plus always-on wrappers) is
/// cut here; the program gets `DevtoolsNative.rs` as the following Prelude
/// part and `DEVTOOLS_NATIVE_WRAPPERS_PRELUDE` as its policy-gated wrappers.
fn push_embedded_devtools(out: &mut String) {
    let native_start = DEVTOOLS_SOURCE
        .find(HOST_DEVTOOLS_NATIVE_BEGIN)
        .expect("Devtools host native region begin marker missing");
    let native_end = DEVTOOLS_SOURCE
        .find(HOST_DEVTOOLS_NATIVE_END)
        .expect("Devtools host native region end marker missing")
        + HOST_DEVTOOLS_NATIVE_END.len();
    assert!(
        native_start < native_end,
        "Devtools host native region markers are out of order"
    );
    out.push_str(&DEVTOOLS_SOURCE[..native_start]);
    out.push_str(&DEVTOOLS_SOURCE[native_end..]);
}

fn push_game_devtools_control_prelude(out: &mut String) {
    let source = include_str!("../../../jet-foundation/src/DevtoolsControl.rs")
        .replace("crate::Devtools::", "super::");
    out.push_str("\nmod jet_devtools_control {\n");
    out.push_str(&source);
    out.push_str("\n}\n");
    out.push_str(
        "pub use jet_devtools_control::{\
JetDevtoolsCommand, JetDevtoolsCommandReceipt, JetDevtoolsGameControlKind,\
JetDevtoolsGameControlRequest, JetDevtoolsGameControlCallbackGuard,\
jet_devtools_install_game_control_callback, jet_devtools_clear_commands,\
jet_devtools_enqueue_command, jet_devtools_requeue_command_front,\
jet_devtools_command_count, jet_devtools_poll_command,\
jet_devtools_poll_database_explain, jet_devtools_enqueue_game_control,\
jet_devtools_poll_game_control, jet_devtools_record_command_receipt,\
jet_devtools_update_command_receipt, jet_devtools_poll_command_receipt,\
jet_devtools_command_receipt_count, jet_devtools_clear_command_receipts,\
jet_devtools_take_game_control_relays, jet_devtools_clear_game_control_relays};\n",
    );
}
fn push_game_debug_policy_const(out: &mut String, policy: &ReleaseDevtoolsPolicy) {
    out.push_str(&format!(
        "\nconst JET_GAME_DEBUG_DATA_ENABLED: bool = {};\n",
        !policy.is_release()
    ));
}

fn push_prelude_dependency_closure(out: &mut String, roots: &[&str]) {
    let mut emitted = HashSet::new();
    for root in roots {
        push_prelude_part(out, root, &mut emitted);
    }
}

fn push_prelude_part(out: &mut String, name: &str, emitted: &mut HashSet<&'static str>) {
    let part = EMBEDDED_PRELUDE_PARTS
        .iter()
        .find(|part| part.name == name)
        .unwrap_or_else(|| panic!("unknown embedded Prelude part `{name}`"));
    if !emitted.insert(part.name) {
        return;
    }
    for dependency in part.dependencies {
        push_prelude_part(out, dependency, emitted);
    }
    match &part.source {
        EmbeddedPreludePartSource::Static(source) => out.push_str(source),
        EmbeddedPreludePartSource::Outcome => push_embedded_outcome(out),
    }
}

/// Project active runtime diagnostic rows into the standalone Prelude. This
/// keeps generated AOT/Wasm code independent of Foundation's host Registry
/// module while retaining one durable row source.
fn runtime_diagnostic_projection() -> String {
    let rows = jet_foundation::Registry::diagnostic_rows()
        .iter()
        .filter(|row| {
            row.stage == "runtime"
                && row.status == jet_foundation::Registry::DiagnosticStatus::Active
        });
    let mut out = String::from(
        "\nfn jet_runtime_diagnostic_row(code: &str) -> Option<JetRuntimeDiagnosticRow> {\n\n    match code {\n",
    );
    for row in rows {
        let holes = row
            .template_holes
            .iter()
            .map(|hole| format!("{hole:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "        {code:?} => Some(JetRuntimeDiagnosticRow {{ code: {code:?}, what: {what:?}, why: {why:?}, fix: {fix:?}, template_holes: &[{holes}] }}),\n",
            code = row.code,
            what = row.what,
            why = row.why,
            fix = row.fix,
        ));
    }
    out.push_str(
        "        _ => None,\n    }\n}\n\n\
         pub fn jet_render_runtime_stop(\n\
             code: &'static str, file: &str, line: u32, fn_name: &str,\n\
             src_line: &str, col: u32, caret_len: u32, message: &str, locals: &str,\n\
         ) -> JetRuntimeDiagnostic {\n\
             jet_render_runtime_stop_from_row(\n\
                 jet_runtime_diagnostic_row(code), code, file, line, fn_name, src_line,\n\
                 col, caret_len, message, locals,\n\
             )\n\
         }\n\n\
         pub fn jet_render_runtime_sentry(\n\
             code: &'static str, file: &str, line: u32, gate: &str,\n\
             operation: &str, obligation: &str, detail: &str,\n\
         ) -> JetRuntimeDiagnostic {\n\
             jet_render_runtime_sentry_with_context(\n\
                 code, file, line, gate, operation, obligation, detail,\n\
                 \"false\", None, None,\n\
             )\n\
         }\n\n\
         pub fn jet_render_runtime_sentry_with_context(\n\
             code: &'static str, file: &str, line: u32, gate: &str,\n\
             operation: &str, obligation: &str, detail: &str,\n\
             obligation_status: &str, foreign_component: Option<&str>,\n\
             foreign_fenced: Option<bool>,\n\
         ) -> JetRuntimeDiagnostic {\n\
             jet_render_runtime_sentry_from_row(\n\
                 jet_runtime_diagnostic_row(code), code, file, line, gate,\n\
                 operation, obligation, detail, obligation_status,\n\
                 foreign_component, foreign_fenced,\n\
             )\n\
         }\n\n",
    );
    out
}

/// Project the same active runtime rows for the no-alloc renderer. The
/// renderer itself lives in `PortableCore.rs`; this is metadata only.
fn portable_runtime_diagnostic_projection() -> String {
    let rows = jet_foundation::Registry::diagnostic_rows()
        .iter()
        .filter(|row| {
            row.stage == "runtime"
                && row.status == jet_foundation::Registry::DiagnosticStatus::Active
        });
    let mut out = String::from(
        "\nfn jet_runtime_diagnostic_row(code: &str) -> Option<JetRuntimeDiagnosticRow> {\n    match code {\n",
    );
    for row in rows {
        let holes = row
            .template_holes
            .iter()
            .map(|hole| format!("{hole:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "        {code:?} => Some(JetRuntimeDiagnosticRow {{ code: {code:?}, what: {what:?}, why: {why:?}, fix: {fix:?}, template_holes: &[{holes}] }}),\n",
            code = row.code,
            what = row.what,
            why = row.why,
            fix = row.fix,
        ));
    }
    out.push_str("        _ => None,\n    }\n}\n\n");
    out
}
fn push_embedded_outcome(out: &mut String) {
    let stop_start = OUTCOME_SOURCE
        .find(HOST_RUNTIME_STOP_BEGIN)
        .expect("Outcome host runtime-stop wrapper marker missing");
    let stop_end = OUTCOME_SOURCE
        .find(HOST_RUNTIME_STOP_END)
        .expect("Outcome host runtime-stop wrapper end marker missing")
        + HOST_RUNTIME_STOP_END.len();
    let sentry_start = OUTCOME_SOURCE
        .find(HOST_RUNTIME_SENTRY_BEGIN)
        .expect("Outcome host sentry wrapper marker missing");
    let sentry_end = OUTCOME_SOURCE
        .find(HOST_RUNTIME_SENTRY_END)
        .expect("Outcome host sentry wrapper end marker missing")
        + HOST_RUNTIME_SENTRY_END.len();
    assert!(
        stop_end <= sentry_start && sentry_start <= sentry_end,
        "Outcome host wrappers are out of order"
    );
    out.push_str(&OUTCOME_SOURCE[..stop_start]);
    out.push_str(&OUTCOME_SOURCE[stop_end..sentry_start]);
    out.push_str(&OUTCOME_SOURCE[sentry_end..]);
    out.push_str(&runtime_diagnostic_projection());
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

/// Primitive types with a total order. The cached-runtime emitter uses this
/// single list for its comparable implementations.
const COMPARABLE_PRIMITIVES: &[&str] = &[
    "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "bool", "char", "String",
];

/// Traits owned by the fixed runtime and shared by generated program code.
/// The Prelude names these traits directly, so their declarations must remain
/// inside the cached runtime block.
///
/// `__jet_Ordering` is a value type that crosses generated-module boundaries;
/// one runtime declaration avoids distinct Rust types at those boundaries.
fn push_cached_runtime_traits(out: &mut String) {
    out.push_str("pub trait __jet_Display {\n");
    out.push_str("    fn display(&self) -> String;\n");
    out.push_str("}\n\n");
    out.push_str("pub trait __jet_Equatable: Sized { fn equal(&self, rhs: &Self) -> bool; }\n");
    for ty in [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64", "bool", "char",
        "String",
    ] {
        out.push_str(&format!(
            "impl __jet_Equatable for {ty} {{ fn equal(&self, rhs: &Self) -> bool {{ self == rhs }} }}\n"
        ));
    }
    out.push('\n');
    out.push_str(ORDERING_ENUM);
    out.push_str(COMPARABLE_TRAIT);
    push_comparable_primitive_impls(out);
    for (name, method) in [
        ("Add", "add"),
        ("Sub", "sub"),
        ("Mul", "mul"),
        ("Div", "div"),
    ] {
        out.push_str(&format!(
            "pub trait __jet_{name}<Rhs = Self>: Sized {{ type Output; fn {method}(&self, rhs: &Rhs) -> Self::Output; }}\n"
        ));
    }
    out.push('\n');
}

const ORDERING_ENUM: &str = concat!(
    "#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]\n",
    "pub enum __jet_Ordering { __jet_Less, __jet_Equal, __jet_Greater }\n\n",
);
const COMPARABLE_TRAIT: &str =
    "pub trait __jet_Comparable: Sized { fn compare(&self, rhs: &Self) -> __jet_Ordering; }\n";

fn push_comparable_primitive_impls(out: &mut String) {
    for ty in COMPARABLE_PRIMITIVES {
        out.push_str(&format!(
            "impl __jet_Comparable for {ty} {{ fn compare(&self, rhs: &Self) -> __jet_Ordering {{ if self < rhs {{ __jet_Ordering::__jet_Less }} else if self > rhs {{ __jet_Ordering::__jet_Greater }} else {{ __jet_Ordering::__jet_Equal }} }} }}\n"
        ));
    }
}

/// Open the cached-runtime block. Everything from here to `CACHED_RUNTIME_END`
/// is decided by build facts alone — never by user source text — so the native
/// builder compiles it once into a content-addressed rlib and links it
/// (`jet_store::runtime`).

fn push_cached_runtime_begin_with_policy(
    out: &mut String,
    link: Option<&FfiLink>,
    include_arrow: bool,
    policy: &ReleaseDevtoolsPolicy,
) {
    if link.is_some() {
        push_ffi_reporter(out, link);
    }
    out.push_str(CACHED_RUNTIME_BEGIN);
    push_cached_runtime_body(out, link, include_arrow, policy);
}

fn push_cached_runtime_body(
    out: &mut String,
    link: Option<&FfiLink>,
    include_arrow: bool,
    policy: &ReleaseDevtoolsPolicy,
) {
    if link.is_none() {
        // EnvInit is part of cached runtime and calls this hook. Keep no-FFI
        // stub inside marker block so split rlib has symbol too.
        push_ffi_reporter(out, None);
    }
    push_cached_runtime_traits(out);
    // The fixed runtime is one dependency closure rooted at `Outcome`: its JSON
    // parser, error vocabulary, and the diagnostic renderer it imports as
    // `crate::RuntimeDiagnosticCore` all live in the same cached rlib instead
    // of the optional Core closure, which is a separate crate when native
    // runtime reuse is active.  Data-loader and Arrow C-data sources are
    // demand-driven Core dependencies; keeping them here would make a print-only
    // program carry their code and unsafe FFI surface.
    push_prelude_dependency_closure(out, &["outcome", "prelude_data_flow", "performance_budget"]);
    // Foundation's PerformanceBudget source names the canonical Syntax
    // namespace. Project the compiler-owned separator into generated native
    // code instead of maintaining a second parser constant.
    out.push_str(&format!(
        "\n#[allow(non_snake_case)]\nmod Syntax {{\n    pub const DIGIT_SEPARATOR: char = {:?};\n}}\n",
        Syntax::DIGIT_SEPARATOR,
    ));
    // The Foundation JetInt formatter is part of the generated runtime
    // directly. Fixed-width carriers remain ordinary Rust scalars.
    push_prelude(
        out,
        !policy.is_release() || policy.stream_code,
        policy.local_rail,
    );
    out.push_str(ENV_INIT_PRELUDE);
    push_mem_prelude(out);
    push_gc_prelude(out);
    out.push_str(LAYOUT_PRELUDE);
    push_foundation_facade(out, include_arrow);
}

/// The fixed part of the block on its own — what `cached_runtime_fingerprint`
/// hashes. The policy argument selects the exact fixed-runtime source.

fn push_cached_runtime_with_policy(
    out: &mut String,
    link: Option<&FfiLink>,
    policy: &ReleaseDevtoolsPolicy,
) {
    if link.is_some() {
        push_ffi_reporter(out, link);
    }
    out.push_str(CACHED_RUNTIME_BEGIN);
    push_cached_runtime_body(out, link, false, policy);
    out.push_str(CACHED_RUNTIME_END);
}


/// Exact fixed-runtime identity used by the final native-binary cache as well
/// as the rlib cache. A Prelude edit must invalidate both layers. The value is
/// memoized because it is the same relevant input for every native program;
/// unrelated compiler edits must not make each key derivation rebuild and hash
/// the fixed runtime block.
pub fn cached_runtime_fingerprint() -> String {
    CACHED_RUNTIME_FINGERPRINT
        .get_or_init(|| cached_runtime_fingerprint_with_policy(&ReleaseDevtoolsPolicy::development()))
        .clone()
}

/// Fingerprint the fixed runtime under one typed release policy. Release
/// artifacts must not share a cache entry with development or protected
/// Devtools emission.
pub fn cached_runtime_fingerprint_with_policy(policy: &ReleaseDevtoolsPolicy) -> String {
    let mut source = String::new();
    push_cached_runtime_with_policy(&mut source, None, policy);
    crate::SHA256::sha256_hex(source.as_bytes())
}

const ENV_INIT_PRELUDE: &str = include_str!("../Prelude/EnvInit.rs");

/// D-COV1: coverage state is emitted only for coverage artifacts. The
/// generated harness owns the flush, while these locks make function and
/// branch hits safe when tests run in parallel.
const COVERAGE_PRELUDE: &str = r#"
static JET_COV_FUNCTION_HITS: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<u64, u64>>> = std::sync::OnceLock::new();
static JET_COV_BRANCH_HITS: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<&'static str, (&'static str, u64, u64)>>> = std::sync::OnceLock::new();

fn jet_cov_function(line: u64) {
    let mut hits = JET_COV_FUNCTION_HITS
        .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *hits.entry(line).or_insert(0) += 1;
}

fn jet_cov_register_branch(id: &'static str, function: &'static str) {
    let mut branches = JET_COV_BRANCH_HITS
        .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((known_function, _, _)) = branches.get(id) {
        assert_eq!(*known_function, function, "coverage branch identity changed");
    } else {
        branches.insert(id, (function, 0, 0));
    }
}

fn jet_cov_branch(id: &'static str, function: &'static str, taken: bool) {
    let mut branches = JET_COV_BRANCH_HITS
        .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (known_function, taken_hits, not_taken_hits) = branches
        .get_mut(id)
        .unwrap_or_else(|| panic!("coverage branch {id} was not registered"));
    assert_eq!(*known_function, function, "coverage branch identity changed");
    if taken {
        *taken_hits += 1;
    } else {
        *not_taken_hits += 1;
    }
}

fn jet_cov_flush() {
    let Some(path) = std::env::var_os("JET_COV_OUT") else {
        return;
    };
    let mut output = String::new();
    {
        let hits = JET_COV_FUNCTION_HITS
            .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for line in hits.keys() {
            output.push_str("f\t");
            output.push_str(&line.to_string());
            output.push('\n');
        }
    }
    {
        let branches = JET_COV_BRANCH_HITS
            .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeMap::new()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (id, (function, taken, not_taken)) in branches.iter() {
            output.push_str("b\t");
            output.push_str(id);
            output.push('\t');
            output.push_str(function);
            output.push('\t');
            output.push_str(&taken.to_string());
            output.push('\t');
            output.push_str(&not_taken.to_string());
            output.push('\n');
        }
    }
    std::fs::write(std::path::PathBuf::from(path), output)
        .unwrap_or_else(|error| panic!("could not write coverage output: {error}"));
}
"#;

pub(crate) fn push_coverage_prelude(out: &mut String) {
    out.push_str(COVERAGE_PRELUDE);
}

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
#[derive(Clone, Debug)]
struct JetTestOutcome {
    name: String,
    ok: bool,
    expected_failure: bool,
    skipped: bool,
    stdout: String,
    stderr: String,
    property_cases: Option<u64>,
}

fn jet_test_filter() -> Option<String> {
    std::env::var("JET_TEST_FILTER")
        .ok()
        .filter(|filter| !filter.is_empty())
}
fn jet_test_serial() -> bool {
    std::env::var_os("JET_TEST_SERIAL").is_some()
}
fn jet_test_shuffle_seed() -> Option<u64> {
    std::env::var("JET_TEST_SHUFFLE_SEED")
        .ok()
        .and_then(|seed| seed.parse::<u64>().ok())
}
fn jet_test_capture_mode() -> &'static str {
    match std::env::var("JET_TEST_CAPTURE").ok().as_deref() {
        Some("all") => "all",
        Some("none") => "none",
        _ => "failed",
    }
}
fn jet_test_should_capture(ok: bool) -> bool {
    match jet_test_capture_mode() {
        "all" => true,
        "none" => false,
        _ => !ok,
    }
}
fn jet_test_json_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn jet_test_finish(results: Vec<JetTestOutcome>) -> bool {
    for result in &results {
        let Some(case_count) = result.property_cases else {
            continue;
        };
        let state = if case_count == 0 || result.stderr.starts_with("E0613:") {
            3
        } else if result.ok != result.expected_failure {
            0
        } else {
            1
        };
        jet_evidence_with_expectation(result.expected_failure, || {
            jet_proof_record(
                3,
                state,
                &result.name,
                &result.stderr,
                "",
                case_count as u32,
            );
        });
    }
    let ok = results.iter().all(|result| result.skipped || result.ok);
    let passed = results.iter().filter(|result| !result.skipped && result.ok && !result.expected_failure).count();
    let failed = results.iter().filter(|result| !result.skipped && !result.ok && !result.expected_failure).count();
    let skipped = results.iter().filter(|result| result.skipped).count();
    let expected_failures = results.iter().filter(|result| !result.skipped && result.ok && result.expected_failure).count();
    let unexpected_passes = results.iter().filter(|result| !result.skipped && !result.ok && result.expected_failure).count();
    if std::env::var_os("JET_TEST_JSON").is_some() {
        let tests = results
            .iter()
            .map(|result| {
                let stdout = if jet_test_should_capture(result.ok) {
                    result.stdout.as_str()
                } else {
                    ""
                };
                let stderr = if jet_test_should_capture(result.ok) {
                    result.stderr.as_str()
                } else {
                    ""
                };
                format!(
                    "{{\"name\":{},\"ok\":{},\"expectedFailure\":{},\"skipped\":{},\"stdout\":{},\"stderr\":{}}}",
                    jet_test_json_quote(&result.name),
                    result.ok,
                    result.expected_failure,
                    result.skipped,
                    jet_test_json_quote(stdout),
                    jet_test_json_quote(stderr)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"schema\":\"jet.test.v1\",\"ok\":{},\"passed\":{},\"failed\":{},\"skipped\":{},\"expectedFailures\":{},\"unexpectedPasses\":{},\"tests\":[{}]}}",
            ok, passed, failed, skipped, expected_failures, unexpected_passes, tests
        );
    } else {
        for result in &results {
            if jet_test_should_capture(result.ok) {
                if !result.stdout.is_empty() {
                    print!("{}", result.stdout);
                }
                if !result.stderr.is_empty() {
                    eprintln!("{}", result.stderr);
                }
            }
            let status = match (result.skipped, result.ok, result.expected_failure) {
                (true, _, _) => "skip",
                (false, true, false) => "pass",
                (false, false, false) => "FAIL",
                (false, true, true) => "expected-fail",
                (false, false, true) => "UNEXPECTED-PASS (remove expected_fail: true)",
            };
            println!("{}: {}", result.name, status);
        }
        print!("{passed} passed, {failed} failed, {skipped} skipped");
        if expected_failures != 0 {
            print!(", {expected_failures} expected-fail");
        }
        if unexpected_passes != 0 {
            print!(", {unexpected_passes} unexpected-pass");
        }
        println!();
    }
    ok
}
/// Install the test harness panic hook once. Runtime-stop carriers are an
/// internal transport detail: the harness turns them into the canonical Jet
/// report, so the previous Rust hook must not print a second panic voice.
/// Every other payload delegates to the hook that was installed before the
/// harness, preserving ordinary Rust diagnostics for generated-code defects.
static JET_TEST_PANIC_HOOK: std::sync::Once = std::sync::Once::new();
fn jet_test_install_panic_hook() {
    JET_TEST_PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().is::<JetRenderedRuntimeStop>()
                || info.payload().is::<JetRuntimeDiagnostic>()
            {
                return;
            }
            previous(info);
        }));
    });
}

/// Catch a panic carrier inside a test and retain its product diagnostic. The
/// normal program boundary owns process exit; this test-only boundary converts
/// runtime carriers into the canonical Jet report.
fn jet_test_panic_error(
    payload: Box<dyn std::any::Any + Send>,
) -> Result<(), String> {
    match payload.downcast::<JetRenderedRuntimeStop>() {
        Ok(report) => Err(report.rendered),
        Err(payload) => match payload.downcast::<JetRuntimeDiagnostic>() {
            Ok(report) => Err(report.rendered),
            Err(payload) => match payload.downcast::<String>() {
                Ok(message) => Err(format!("panic: {message}")),
                Err(payload) => match payload.downcast::<&'static str>() {
                    Ok(message) => Err(format!("panic: {message}")),
                    Err(_) => Err("panic in test".to_string()),
                },
            },
        },
    }
}

fn jet_test_run<F>(run: F) -> Result<(), String>
where
    F: FnOnce() -> Result<(), String>,
{
    let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(result) => result,
        Err(payload) => jet_test_panic_error(payload),
    };
    if result.is_err() {
        jet_test_skip_abort();
    }
    jet_test_expect_fail_abort();
    jet_test_timeout_abort();
    result
}

enum JetPropertyCaseResult {
    Accepted,
    Rejected,
}

/// Evaluate one generated contract case's pre-call eligibility (D-CLAIM1,
/// #2502). The compiler-synthesized predicate checks the candidate's own
/// `#Pre` conditions over the generated arguments BEFORE the callable runs:
/// `Ok(false)` is a rejected input and the callable never executes. Once the
/// predicate accepts, the callable runs under ordinary `jet_test_run`
/// semantics — its own `#Pre`/`#Post` and every nested callee failure are
/// real failure evidence, never reclassified as input rejection. A panic
/// while evaluating the predicate itself is failure evidence too (`Err`): the
/// sampler found an input on which the precondition cannot even be evaluated.
fn jet_test_contract_eligibility<P>(eligible: P) -> Result<bool, String>
where
    P: FnOnce() -> bool,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(eligible)) {
        Ok(eligible) => Ok(eligible),
        Err(payload) => jet_test_panic_error(payload).map(|()| true),
    }
}

fn jet_test_run_property<F>(run: F) -> Result<JetPropertyCaseResult, String>
where
    F: FnOnce() -> Result<(), String>,
{
    jet_test_run(run).map(|()| JetPropertyCaseResult::Accepted)
}
/// D-E3-1905: the test child is an AOT binary. The release profile is encoded
/// at compile time so `jet test --release --trace-tiers` proves which binary
/// was built, without claiming that the test harness used a JIT or interpreter.
fn jet_test_trace_tier() {
    if std::env::var_os("JET_TEST_TRACE_TIERS").is_none() {
        return;
    }
    let line = if cfg!(jet_release) {
        "tier aot profile=release"
    } else {
        "tier aot profile=default"
    };
    if std::env::var_os("JET_TEST_JSON").is_some() {
        eprintln!("{line}");
    } else {
        println!("{line}");
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
const TESTING_SHARED_PRELUDE: &str = include_str!("../Prelude/CoreLib/Top/TestingShared.rs");
const REPORT_PRELUDE: &str = include_str!("../../../jet-foundation/src/Report.rs");
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
        // a uniform full-range fallback. Test and fuzz use the same case seed
        // stream; the explicit landmarks make boundary predicates observable.
        match rng.below(32) {
            0 => 0, 1 => 1, 2 => -1, 3 => 2, 4 => -2,
            5 => 42, 6 => -42, 7 => 99, 8 => 100, 9 => 255,
            10 => 256, 11 => 512, 12 => 1024,
            13 => i64::MIN, 14 => i64::MAX,
            _ => rng.next_u64() as i64,
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
macro_rules! impl_jet_gen_int {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl JetGen for $ty {
                fn generate(rng: &mut JetRng) -> $ty {
                    rng.next_u64() as $ty
                }
                fn shrink(&self) -> Vec<$ty> {
                    if *self == 0 {
                        return Vec::new();
                    }
                    let mut values = vec![0 as $ty];
                    let half = *self / 2 as $ty;
                    if half != 0 && half != *self {
                        values.push(half);
                    }
                    if *self > 0 {
                        values.push(*self - 1 as $ty);
                    } else {
                        values.push(*self + 1 as $ty);
                    }
                    values
                }
                fn render(&self) -> String {
                    format!("{}", self)
                }
            }
        )+
    };
}
impl_jet_gen_int!(i8, i16, i32, u8, u16, u32, u64, u128, isize, usize, i128);
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
impl<T: JetGen> JetGen for JetOutcome<T, JetAbsent> {
    fn generate(rng: &mut JetRng) -> JetOutcome<T, JetAbsent> {
        if rng.below(4) == 0 {
            Err(JetAbsent)
        } else {
            Ok(T::generate(rng))
        }
    }
    fn shrink(&self) -> Vec<JetOutcome<T, JetAbsent>> {
        match self {
            Err(_) => Vec::new(),
            Ok(value) => {
                let mut values = vec![Err(JetAbsent)];
                for candidate in value.shrink() {
                    values.push(Ok(candidate));
                }
                values
            }
        }
    }
    fn render(&self) -> String {
        match self {
            Err(_) => "none".to_string(),
            Ok(value) => value.render(),
        }
    }
}
impl<T: JetGen, const N: usize> JetGen for [T; N] {
    fn generate(rng: &mut JetRng) -> [T; N] {
        std::array::from_fn(|_| T::generate(rng))
    }
    fn shrink(&self) -> Vec<[T; N]> {
        let mut values = Vec::new();
        if let Some(first) = self.first() {
            for candidate in first.shrink() {
                let mut value = self.clone();
                value[0] = candidate;
                values.push(value);
            }
        }
        values
    }
    fn render(&self) -> String {
        let parts: Vec<String> = self.iter().map(|value| value.render()).collect();
        format!("[{}]", parts.join(", "))
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
fn jet_prop_cases() -> u64 {
    1_000
}
fn jet_prop_replay_seed() -> Option<u64> {
    std::env::var("JET_PROP_REPLAY_SEED")
        .ok()
        .and_then(|seed| seed.parse::<u64>().ok())
}
fn jet_prop_replay_case() -> Option<u64> {
    std::env::var("JET_PROP_REPLAY_CASE")
        .ok()
        .and_then(|case_index| case_index.parse::<u64>().ok())
}
fn jet_prop_trace_sample(engine: &str, case_index: u64, seed: u64, input: &str) {
    if std::env::var_os("JET_PROP_TRACE").is_none() {
        return;
    }
    let line = format!(
        "JET_PROP_SAMPLE engine={} case={} seed={} input={}",
        engine, case_index, seed, input
    );
    if std::env::var_os("JET_TEST_JSON").is_some() {
        eprintln!("{line}");
    } else {
        println!("{line}");
    }
}
"#;
/// R10 / #437 / #1133: Core runtime emission is pay-for-what-you-call.
///
/// The JetStd brace chain (`mod jet_std {` … closing `}` in YAML.rs) must stay
/// contiguous — those files are one audited compiler/runtime kernel. Optional
/// fragments are selected from `bundle.used_core`. Package-owned Core behavior
/// may use an explicit ABI bridge, but never falls back to this path.
/// The DataTree carrier is owned by Foundation. Keep its source verbatim in a
/// private generated module, expose the protocol path expected by Foundation
/// encoding kernels, and let `jet_std` re-export that same type below.
const DATATREE_FOUNDATION_ROOT: &str = concat!(
    "\n// D-SERDE2: the generated Prelude uses Foundation's canonical carrier.\n",
    "mod jet_foundation_datatree {\n",
    include_str!("../../../jet-foundation/src/DataTree.rs"),
    "\n}\n",
    "#[allow(non_snake_case)]\nmod DataTree {\n",
    "    pub use crate::jet_foundation_datatree::DataTree;\n",
    "}\n",
);

const DATATREE_PRELUDE_REEXPORT: &str =
    "\npub use crate::DataTree::DataTree;\n";

const RESOURCE_FACTS_PRELUDE: &str = concat!(
    "\n#[allow(non_snake_case, unused_imports)]\nmod ResourceSchedule {\n",
    include_str!("../../../jet-foundation/src/ResourceSchedule.rs"),
    "\n}\n",
    "\n#[allow(non_snake_case, unused_imports)]\nmod Facts {\n",
    include_str!("../../../jet-foundation/src/FactsDerivation.rs"),
    "\n}\n",
);

const NUMERIC_FOUNDATION_SOURCE: &str =
    include_str!("../../../jet-foundation/src/Numeric.rs");




const MAPPED_FILE_PRELUDE: &str =
    include_str!("../Prelude/CoreLib/JetStd/MappedFile.rs");
const MATH_TASK_MEM_PRELUDE: &str = concat!(
    "\n// JET_VETTED_UNSAFE_BEGIN: jet_std_math_task_mem\n\
     // AUDIT: D-SIMD2/D-SIMD3/D-SHARED-REVISION1 keep lane representation,\n\
     // permit-gated UnsafeCell projections, and consumed revision tickets in\n\
     // this compiler-owned JetStd source.\n",
    include_str!("../Prelude/CoreLib/JetStd/MathTaskMem.rs"),
    "\n// JET_VETTED_UNSAFE_END: jet_std_math_task_mem\n",
);
const JETSTD_COMMON_TYPES_PRELUDE: &str = concat!(
    "\n// JET_VETTED_UNSAFE_BEGIN: jet_std_common_types\n\
     // AUDIT: D-INTBIG1 keeps the exact-Int raw ownership adapters and the\n\
     // platform terminal handle seam inside this compiler-owned source.\n",
    include_str!("../Prelude/CoreLib/JetStd/CommonTypes.rs"),
    "\n// JET_VETTED_UNSAFE_END: jet_std_common_types\n",
);
const JETSTD_REACTIVE_EVENT_WATCH_PRELUDE: &str = concat!(
    "\n// JET_VETTED_UNSAFE_BEGIN: jet_std_reactive_event_watch\n\
     // AUDIT: D-REACT1/D-DATARACE1 keep re-entrant staged values and synchronized\n\
     // signal storage behind this compiler-owned, opt-in reactive source.\n",
    include_str!("../Prelude/CoreLib/JetStd/ReactiveEventWatch.rs"),
    "\n// JET_VETTED_UNSAFE_END: jet_std_reactive_event_watch\n",
);
const JETSTD_FFI_CALLBACKS_PRELUDE: &str = concat!(
    "\n// JET_VETTED_UNSAFE_BEGIN: jet_std_ffi_callbacks\n\
     // AUDIT: D-FFI-CALLBACK2 keeps callback registration, in-flight accounting,\n\
     // native shutdown acknowledgement, and release in one managed source.\n",
    include_str!("../Prelude/CoreLib/JetStd/FfiCallbacks.rs"),
    "\n// JET_VETTED_UNSAFE_END: jet_std_ffi_callbacks\n",
);
const SHARED_ROUTES_PRELUDE: &str =
    include_str!("../Prelude/CoreLib/Top/SharedRoutes.rs");

const CORELIB_KERNEL_PARTS: &[&str] = &[
    include_str!("../Prelude/CoreLib/JetStd/Open.rs"),
    include_str!("../Prelude/CoreLib/JetStd/Regex.rs"),
    include_str!("../Prelude/TaskGroup.rs"),
    include_str!("../Prelude/CoreLib/JetStd/Mime.rs"),
    include_str!("../Prelude/CoreLib/JetStd/UrlMime.rs"),
    include_str!("../Prelude/CoreLib/JetStd/JSONCodec.rs"),
    include_str!("../Prelude/CoreLib/JetStd/EncodingTypes.rs"),
    JETSTD_COMMON_TYPES_PRELUDE,
    MAPPED_FILE_PRELUDE,
    include_str!("../Prelude/CommandSuite.rs"),
    // D-DBPOLICY1=A: the closed row-policy language, compiled once. `DBPluginWire`
    // below and `Top/Sync.rs` both read it instead of re-deriving the rule (I9).
    include_str!("../Prelude/CoreLib/JetStd/RowPolicy.rs"),
    "\npub mod plugin_wire {\n",
    include_str!("../../../jet-foundation/src/PluginWire.rs"),
    "\n}\n",
    include_str!("../Prelude/CoreLib/JetStd/DBPluginWire.rs"),
    include_str!("../Prelude/CoreLib/JetStd/WireOrder.rs"),
    include_str!("../Prelude/CoreLib/JetStd/DataTreeKind.rs"),
    // D-VALIDATE-DECODE1=B: keep the canonical FieldError rendering beside
    // DataTree's JetShow/JetDisplay implementations in the same JetStd scope.
    include_str!("../Prelude/Core/FieldError.rs"),
    // D-VALIDATE-DECODE1=B: the accumulated decode/validate failure has one
    // rendering. Splice it beside the type so `impl JetShow for FieldError`
    // below, the Cranelift host, and the TIR evaluator cannot drift (I9).
    DATATREE_PRELUDE_REEXPORT,
    include_str!("../Prelude/CoreLib/JetStd/DataTree.rs"),
    include_str!("../Prelude/CoreLib/JetStd/TestingComparison.rs"),
    "\njet_datatree_decode_helpers!();\n",
    // `EncodingStream.rs` validates exact JSON number tokens through
    // `jet_std::validate_json_number`. The root `jet_json_number` module holds
    // the one implementation; name it here so the generated program, the JIT
    // host, and comptime all reach the same function (I9).
    "\n#[allow(unused_imports)]\npub use crate::jet_json_number::validate_json_number;\n",
    "\n// JET_VETTED_UNSAFE_BEGIN: jet_cell\nmod jet_cell {\n#[allow(unused_imports)]\nuse crate::{JetOutcome, JetAbsent};\n",
    include_str!("../Prelude/LocalCell.rs"),
    "\n}\npub use self::jet_cell::{JetCell, JetCellEditGuard, JetCellReadGuard};\n// JET_VETTED_UNSAFE_END: jet_cell\n",
    MATH_TASK_MEM_PRELUDE,
    JETSTD_REACTIVE_EVENT_WATCH_PRELUDE,
    // D-FFI-CALLBACK2=A: generated callback registrations share one managed
    // ownership/ACK/quiescence runtime with the AOT and fixture paths.
    JETSTD_FFI_CALLBACKS_PRELUDE,
    // The DataTree renderer below and TOML.rs (`super::quote_json`) quote
    // through the Foundation JSON kernel. The host that splices these files
    // imports the entry beside them (jet-jit/src/Encoding.rs does the same).
    "\nuse crate::jet_encoding_json::quote_json;\n",
    include_str!("../Prelude/CoreLib/JetStd/JSONDataTree.rs"),
    include_str!("../Prelude/CoreLib/JetStd/TOML.rs"),
    include_str!("../Prelude/CoreLib/JetStd/YAML.rs"),
    SHARED_ROUTES_PRELUDE,

];
#[derive(Clone, Copy, Default)]
struct CorePreludeForces {
    mapped_file: bool,
    shared: bool,
    arrow: bool,
    testing_history: bool,
}

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
    usage.starts_with(CORE_SOURCE_MARKER_PREFIX) || usage.starts_with(CORE_INTRINSIC_MARKER_PREFIX)
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

fn core_source_closure_fingerprint(used_core: &std::collections::HashSet<String>) -> String {
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

fn corelib_emission_identity(body: &str, used_core: &std::collections::HashSet<String>) -> String {
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

/// Translate the checked Core-use closure into target-neutral runtime parts.
///
/// This is the one lowering-time boundary that may inspect the legacy
/// Core-use labels. Runtime adapters consume the resulting typed IDs and do
/// not reconstruct this selection from strings.
pub(crate) fn runtime_parts_for_used_core(
    used_core: &BTreeSet<String>,
) -> BTreeSet<MirRuntimePartId> {
    use MirRuntimePartId as Part;

    let matches = |prefixes: &[&str]| {
        used_core.iter().any(|usage| {
            prefixes.iter().any(|prefix| {
                usage == prefix
                    || usage.starts_with(&format!("{prefix}::"))
                    || usage.starts_with(&format!("{prefix}."))
            })
        })
    };
    let mut parts = BTreeSet::new();
    if matches(&["core.event"]) {
        parts.insert(Part::Event);
    }
    if matches(&["core.rt", "core.realtime"]) {
        parts.insert(Part::Realtime);
    }
    if matches(&["core.hardware", "core.embedded", "core.board", "core.device"]) {
        parts.insert(Part::EmbeddedHardware);
    }
    if matches(&["core.ui", "core.font", "core.web", "app"]) {
        parts.insert(Part::Ui);
    }
    if matches(&["core.ui", "core.devtools", "core.web", "app"]) {
        parts.insert(Part::Devtools);
    }
    if matches(&["core.ui::gtk_backend", "core.ui.gtk_backend"]) {
        parts.insert(Part::Gtk);
    }
    if matches(&[
        "app",
        "core.web",
        "core.http",
        "core.http.client",
        "core.http.server",
        "core.web.devserver",
        "core.db",
        "core.sync",
        "core.net.ws",
        "core.web.browser",
    ]) {
        parts.insert(Part::Apps);
    }
    if matches(&["core.email"]) {
        parts.insert(Part::Email);
    }
    if matches(&["core.game", "core.game.raylib", "core.raylib"]) {
        parts.insert(Part::Game);
    }
    if matches(&[
        "core.files",
        "core.watcher",
        "core.term",
        "core.sys",
        "core.process",
    ]) {
        parts.insert(Part::Files);
    }
    if matches(&[
        "core.sys::on_interrupt",
        "core.sys.on_interrupt",
        "core.process::on_signal",
        "core.process.on_signal",
    ]) {
        parts.insert(Part::Interrupt);
    }
    if matches(&[
        "core.files",
        "core.watcher",
        "core.term",
        "core.sys",
        "core.process",
        "core.args",
        "core.testing",
        "core.perf",
        "core.mem.scope",
    ]) {
        parts.insert(Part::FsRuntime);
    }
    if matches(&["core.crypto"]) {
        parts.insert(Part::Crypto);
    }
    if matches(&[
        "core.math",
        "core.math.random",
        "core.time",
        "core.time.expiring",
        "core.units",
    ]) {
        parts.insert(Part::Math);
    }
    if matches(&[
        "core.encoding",
        "core.encoding.json",
        "core.encoding.jsonl",
        "core.encoding.csv",
        "core.encoding.toml",
        "core.encoding.yaml",
        "core.encoding.xml",
        "core.encoding.cbor",
        "core.encoding.hex",
        "core.archive.gzip",
        "core.archive.zstd",
    ]) {
        parts.insert(Part::Encoding);
    }
    if matches(&[
        "core.data",
        "core.data.sketch.hll",
        "core.data.sketch.tdigest",
        "core.data.sketch.cms",
        "core.data.sketch.reservoir",
        "core.db",
        "core.encoding.csv",
    ]) {
        parts.insert(Part::Data);
    }
    if matches(&["core.text.fmt"]) {
        parts.insert(Part::Fmt);
    }
    if matches(&[
        "core.data",
        "core.data.sketch.hll",
        "core.data.sketch.tdigest",
        "core.data.sketch.cms",
        "core.data.sketch.reservoir",
        "core.db",
        "core.encoding",
        "core.encoding.json",
        "core.encoding.jsonl",
        "core.encoding.csv",
        "core.encoding.toml",
        "core.encoding.yaml",
        "core.encoding.xml",
        "core.encoding.cbor",
        "core.encoding.hex",
        "core.archive.gzip",
        "core.archive.zstd",
    ]) {
        parts.insert(Part::DataFmt);
    }
    if matches(&["core.compute"]) {
        parts.insert(Part::Compute);
    }
    if matches(&[
        "core.http",
        "core.http.client",
        "core.http.server",
        "core.web",
        "core.web.devserver",
    ]) {
        parts.insert(Part::Http);
    }
    if matches(&[
        "core.net.ws",
        "app",
        "core.web",
        "core.db",
        "core.http",
        "core.http.client",
        "core.http.server",
        "core.web.browser",
    ]) {
        parts.insert(Part::WebSocket);
    }
    if matches(&[
        "core.web.browser",
        "core.web",
        "core.web.storage",
        "core.web.storage.local",
        "core.web.storage.session",
    ]) {
        parts.insert(Part::Browser);
    }
    if matches(&["core.args"]) {
        parts.insert(Part::Args);
    }
    if matches(&["core.reflect", "core.compiler.lang"]) {
        parts.insert(Part::Reflect);
    }
    if matches(&["core.auth"]) || parts.contains(&Part::Crypto) {
        parts.insert(Part::AuthTokens);
    }
    if matches(&["core.auth", "app", "core.web"]) {
        parts.insert(Part::AuthSession);
    }
    if matches(&["core.sync", "app", "core.web", "core.db"]) {
        parts.insert(Part::Sync);
    }
    if matches(&["core.service", "core.jobs"]) {
        parts.insert(Part::Services);
    }
    if matches(&["core.mod"]) {
        parts.insert(Part::Mod);
    }
    parts
}



fn push_corelib_prelude_with_policy(
    out: &mut String,
    used_core: &std::collections::HashSet<String>,
    force: bool,
    policy: &ReleaseDevtoolsPolicy,
) {
    push_corelib_prelude_inner(out, used_core, force, false, policy);
}

fn push_corelib_prelude_for_test_harness_with_policy(
    out: &mut String,
    used_core: &std::collections::HashSet<String>,
    force: bool,
    policy: &ReleaseDevtoolsPolicy,
) {
    push_corelib_prelude_for_test_harness_with_forces(
        out,
        used_core,
        force,
        CorePreludeForces::default(),
        policy,
    );
}

fn push_corelib_prelude_for_test_harness_with_forces(
    out: &mut String,
    used_core: &std::collections::HashSet<String>,
    force: bool,
    forces: CorePreludeForces,
    policy: &ReleaseDevtoolsPolicy,
) {
    push_corelib_prelude_inner_with_forces(out, used_core, force, true, forces, policy);
    out.push_str(REPORT_PRELUDE);
    out.push_str(TEST_REPORT_PRELUDE);
    out.push_str(TESTING_SHARED_PRELUDE);
    out.push_str(TEST_PRELUDE);
    out.push_str(PROP_PRELUDE);
}

fn push_corelib_prelude_inner(
    out: &mut String,
    used_core: &std::collections::HashSet<String>,
    force: bool,
    omit_testing_shared: bool,
    policy: &ReleaseDevtoolsPolicy,
) {
    push_corelib_prelude_inner_with_forces(
        out,
        used_core,
        force,
        omit_testing_shared,
        CorePreludeForces::default(),
        policy,
    );
}

fn push_corelib_prelude_inner_with_forces(
    out: &mut String,
    used_core: &std::collections::HashSet<String>,
    force: bool,
    omit_testing_shared: bool,
    forces: CorePreludeForces,
    policy: &ReleaseDevtoolsPolicy,
) {
    // `core.archive` is emitted as a reachable ordinary-Jet source module. Its
    // internal ABI calls do not require a compiler prelude fragment, so no old
    // template can become a fallback implementation.
    if !force && !core_needs_embedded_runtime(used_core) {
        return;
    }
    let mut body = String::new();
    push_corelib_prelude_body(&mut body, used_core, omit_testing_shared, forces, policy);
    out.push_str(&corelib_emission_identity(&body, used_core));
    out.push('\n');
    out.push_str(&body);
}



fn type_uses_stream(ty: &Type) -> bool {
    match ty {
        Type::Apply { name, args } => name == "Stream" || args.iter().any(type_uses_stream),
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::FixedList { elem: inner, .. }
        | Type::Tagged { inner, .. } => type_uses_stream(inner),
        Type::Map { key, value, .. }
        | Type::Result {
            ok: key,
            err: value,
        } => type_uses_stream(key) || type_uses_stream(value),
        Type::Tuple(fields) => fields.iter().any(|(_, field)| type_uses_stream(field)),
        Type::Union(members) => members.iter().any(type_uses_stream),
        Type::Fn { params, ret, .. } => {
            params.iter().any(type_uses_stream) || ret.as_deref().is_some_and(type_uses_stream)
        }
        _ => false,
    }
}

fn func_uses_stream(func: &crate::AST::Func) -> bool {
    func.params.iter().any(|param| type_uses_stream(&param.ty))
        || func.return_type.as_ref().is_some_and(type_uses_stream)
}

fn trait_method_uses_stream(method: &crate::AST::TraitMethodSig) -> bool {
    method
        .params
        .iter()
        .any(|param| type_uses_stream(&param.ty))
        || method.return_type.as_ref().is_some_and(type_uses_stream)
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
                || func.return_type.as_ref().is_some_and(type_uses_stream)
        }),
        Item::ProtocolDecl(def) => def
            .messages
            .iter()
            .any(|message| message.fields.iter().any(|(_, ty)| type_uses_stream(ty))),
        Item::CodeModule(def) => def.body.as_deref().is_some_and(items_use_stream),
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

fn force_corelib_prelude(bundle: &ProgramBundle) -> bool {
    // Stream values still require the contiguous JetStd kernel even when no
    // optional Core module import records that need.
    uses_stream(bundle)
}

/// The exact R10 Core closure that rides in its content-addressed rlib. Keep
/// this assembler shared by emission and cache identity: hashing only the
/// JetStd kernel would let scheduler/UI/app edits reuse the wrong digest.
fn core_runtime_body(bundle: &ProgramBundle, test_harness: bool) -> String {
    core_runtime_body_with_policy(bundle, test_harness, &ReleaseDevtoolsPolicy::development())
}

fn core_runtime_body_with_policy(
    bundle: &ProgramBundle,
    test_harness: bool,
    policy: &ReleaseDevtoolsPolicy,
) -> String {
    let force_corelib = force_corelib_prelude(bundle);
    if !force_corelib && !core_needs_embedded_runtime(&bundle.used_core) {
        return String::new();
    }
    let mut body = String::new();
    // `Prelude/CoreLib/Top/EncodingCodecs.rs` reads this constant. It belongs
    // to the Core closure, whose identity includes the package edition.
    push_package_edition(&mut body, bundle);
    body.push_str(&core_runtime_body_for_with_policy(
        &bundle.used_core,
        bundle.active_os,
        force_corelib,
        test_harness,
        policy,
    ));
    body
}


fn core_runtime_body_for_with_policy(
    used_core: &std::collections::HashSet<String>,
    active_os: Syntax::OSTarget,
    force_corelib: bool,
    test_harness: bool,
    policy: &ReleaseDevtoolsPolicy,
) -> String {
    if !force_corelib && !core_needs_embedded_runtime(used_core) {
        return String::new();
    }
    let mut body = String::new();
    if test_harness {
        push_corelib_prelude_for_test_harness_with_policy(
            &mut body,
            used_core,
            force_corelib,
            policy,
        );
    } else {
        push_corelib_prelude_with_policy(&mut body, used_core, force_corelib, policy);
    }
    body.push_str(scheduler_prelude_for_emit(uses_native_scheduler_for(
        used_core,
    )));
    if force_corelib || core_usage_matches(used_core, &["core.event"]) {
        body.push_str(CORE_STREAM_PRELUDE_RAW);
        body.push_str(CORE_STREAM_DURATION_ADAPTER);
    }
    body.push_str(include_str!("../Prelude/Core/CollectionSources.rs"));
    if core_usage_matches(used_core, &["core.rt", "core.realtime"]) {
        body.push_str(CORE_REALTIME_PRELUDE_RAW);
    }
    if core_usage_matches(
        used_core,
        &["core.board", "core.hardware", "core.device", "core.embedded"],
    ) {
        body.push_str(&flat_prelude_body(CORE_EMBEDDED_HARDWARE_PRELUDE_RAW));
    }
    // Explicit parallel operations use the same typed planning facts as every
    // host adapter. The source is flat-imported once with the scheduler.
    body.push_str(&flat_prelude_body(CORE_PARALLEL_PLAN_PRELUDE_RAW));
    let needs_ui_host = core_usage_matches(used_core, &["core.ui", "core.font", "core.web", "app"]);
    let needs_ui_surface = core_usage_matches(used_core, &["core.ui", "core.web", "app"]);
    if needs_ui_host {
        body.push_str(&canonical_font_prelude());
        body.push_str(include_str!("../Prelude/Core/HostServices.rs"));
    }
    if needs_ui_surface {
        body.push_str("\nmod jet_tui_kernel {\n");
        body.push_str(TUI_KERNEL_PRELUDE);
        body.push_str("\n}\n");
        body.push_str(UI_PRELUDE);
        body.push_str(PREVIEW_PRELUDE);
    }
    if policy.panel_code
        && core_usage_matches(used_core, &["core.ui", "core.devtools"])
    {
        body.push_str(DEVTOOLS_PANEL_PRELUDE);
        body.push_str(DEVTOOLS_PANEL_CATALOG_PRELUDE_RAW);
        body.push_str(DEVTOOLS_DATABASE_PANEL_PRELUDE_RAW);
        body.push_str(DEVTOOLS_JOBS_PANEL_PRELUDE_RAW);
        body.push_str(DEVTOOLS_REQUEST_PANEL_PRELUDE_RAW);
        body.push_str(DEVTOOLS_TELEMETRY_PANEL_PRELUDE_RAW);
        body.push_str(DEVTOOLS_TOPOLOGY_PANEL_PRELUDE_RAW);
    }
    if uses_gtk_backend_for(used_core, active_os) {
        body.push_str(UI_GTK_PRELUDE);
    }
    push_app_preludes(&mut body, used_core);
    body
}

/// Emit the checked no-OS Prelude closure. Unlike the hosted path above, this
/// assembler never starts from `Core.rs`: the portable sources are an explicit
/// source closure whose target operations are filled from the selected machine
/// dossier. The generated body remains flat so MIR's existing symbol ABI does
/// not acquire a second namespace.
pub(crate) fn push_portable_corelib_prelude(
    out: &mut String,
    program: &MirProgram,
    artifact: &MirArtifactPlan,
) {
    let dossier = &program.facts.target_dossier;
    let machine = dossier
        .machine
        .as_deref()
        .unwrap_or_else(|| panic!("portable Prelude emission requires a checked target machine"));
    assert!(
        machine.no_os,
        "portable Prelude emission requires a no-OS target machine"
    );
    assert_eq!(
        artifact.target,
        jet_foundation::MIR::MirArtifactTarget::RustAot,
        "portable Prelude emission requires a RustAot artifact"
    );
    match &machine.panic {
        jet_foundation::TargetMachine::PanicPolicy::Abort => {}
        jet_foundation::TargetMachine::PanicPolicy::Report { .. } => {}
        jet_foundation::TargetMachine::PanicPolicy::HostedDefault
        | jet_foundation::TargetMachine::PanicPolicy::Unspecified => {
            panic!("portable Prelude requires an explicit no-OS panic policy");
        }
    }
    let include_hardware = artifact
        .runtime_parts
        .contains(&MirRuntimePartId::EmbeddedHardware);
    let include_alloc = match dossier.layer {
        jet_foundation::RingLayer::RuntimeLayer::Core => false,
        jet_foundation::RingLayer::RuntimeLayer::Alloc => true,
        jet_foundation::RingLayer::RuntimeLayer::Std => {
            panic!("portable Prelude cannot emit the hosted runtime layer")
        }
    };
    if include_alloc
        && !matches!(
            &machine.allocator,
            jet_foundation::TargetMachine::AllocatorPolicy::Fixed { .. }
        )
    {
        match &machine.allocator {
            jet_foundation::TargetMachine::AllocatorPolicy::Provider { provider } => {
                assert!(
                    provider.abi.calling_convention == "C" && provider.abi.version == "1",
                    "portable target allocator provider `{}` requires the C/1 ABI",
                    provider.provider
                );
            }
            jet_foundation::TargetMachine::AllocatorPolicy::None => {
                panic!("portable Alloc closure requires a declared allocator");
            }
            jet_foundation::TargetMachine::AllocatorPolicy::Unspecified => {
                panic!("portable Alloc closure requires an explicit allocator policy");
            }
            jet_foundation::TargetMachine::AllocatorPolicy::HostedDefault
            | jet_foundation::TargetMachine::AllocatorPolicy::Counting { .. } => {
                panic!("portable Alloc closure cannot use a hosted allocator policy");
            }
            jet_foundation::TargetMachine::AllocatorPolicy::Fixed { .. } => unreachable!(),
        }
    }
    let provider_allocator = matches!(
        &machine.allocator,
        jet_foundation::TargetMachine::AllocatorPolicy::Provider { .. }
    );

    let include_atomic =
        machine.provides_capability(jet_foundation::TargetMachine::TargetCapability::Atomic64);
    if include_atomic {
        // D-PLACE1=A: Atomic<T> is one root Prelude carrier shared by hosted and
        // portable AOT emission; unsupported no-OS targets never receive its
        // AtomicU64 source and therefore do not defer rejection to rustc.
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_atomic_carrier\n");
        out.push_str(&flat_prelude_body(CORE_ATOMIC_PRELUDE_RAW));
        out.push_str("// JET_VETTED_UNSAFE_END: jet_atomic_carrier\n");
        // Portable Core has no exact-Int heap.  Reuse its canonical checked
        // scalar failure rail rather than inventing a second overflow policy.
        out.push_str(
            "\nmod jet_std {\n\
             \x20   #[inline(always)]\n\
             \x20   pub fn jet_int_add(left: i64, right: i64) -> i64 {\n\
             \x20       left.checked_add(right).unwrap_or_else(|| {\n\
             \x20           super::jet_arithmetic_stop(\n\
             \x20               \"\",\n\
             \x20               0,\n\
             \x20               super::JET_ARITHMETIC_ADD_OVERFLOW,\n\
             \x20           )\n\
             \x20       })\n\
             \x20   }\n\
             }\n",
        );
        out.push('\n');
    }
    push_prelude_dependency_closure(out, &["runtime_diagnostic_core"]);
    out.push_str(&portable_runtime_diagnostic_projection());
    let runtime_parts = artifact
        .runtime_parts
        .iter()
        .map(|part| format!("{:?}", part.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "\nconst __JET_TARGET_NAME: &str = {:?};\n\
         const __JET_TARGET_TRIPLE: &str = {:?};\n\
         const __JET_TARGET_PROVIDER_IDENTITY: &str = {:?};\n\
         const __JET_TARGET_CLOSURE_IDENTITY: &str = {:?};\n\
         const __JET_TARGET_LINKER_IDENTITY: &str = {:?};\n\
         const __JET_TARGET_ARTIFACT_IDENTITY: &str = {:?};\n\
         const __JET_TARGET_RUNTIME_LAYER: &str = {:?};\n\
         const __JET_TARGET_RUNTIME_PARTS: &[&str] = &[{}];\n\
         const __JET_CHECKED_CORE_CALL_COUNT: usize = {};\n\
         const __JET_CHECKED_PRELUDE_CALL_COUNT: usize = {};\n",
        machine.name,
        machine.triple,
        artifact.provider_identity,
        artifact.closure_identity,
        dossier.linker_identity,
        artifact.artifact_identity,
        dossier.layer.as_str(),
        runtime_parts,
        program.core_calls.len(),
        program.prelude_calls.len(),
    ));
    // Every portable fragment below is emitted through `flat_prelude_body`,
    // so its leading imports are declared once here for the whole module.
    push_portable_prelude_imports(out, include_atomic, include_hardware, include_alloc);
    out.push_str(&flat_prelude_body(CORE_FIXED_ARITHMETIC_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(PORTABLE_CORE_PRELUDE_RAW));
    out.push_str(include_str!("../../../jet-foundation/src/NumericConversion.rs"));
    out.push_str(include_str!("../Prelude/Core/NumericRuntime.rs"));
    out.push_str(&flat_prelude_body(CORE_POWER_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(CORE_DIVISION_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(TARGET_ADAPTERS_PRELUDE_RAW));
    if include_hardware {
        out.push_str(&flat_prelude_body(portable_embedded_hardware_source()));
    }
    if include_alloc {
        push_prelude_dependency_closure(out, &["fixed_allocator"]);
        out.push_str(&flat_prelude_body(PORTABLE_ALLOC_PRELUDE_RAW));
        if provider_allocator {
            out.push_str(
                "\n#[global_allocator]\n\
                 static __JET_TARGET_ALLOCATOR: JetTargetAllocator =\n\
                     JetTargetAllocator::from_callbacks(jet_target_alloc_impl, jet_target_dealloc_impl);\n",
            );
        } else {
            out.push_str(
                "\n#[global_allocator]\n\
                 static __JET_TARGET_ALLOCATOR: JetHeapAllocator = JetHeapAllocator::new();\n",
            );
        }
    }
    if artifact.runtime_parts.contains(&MirRuntimePartId::Gc) {
        push_gc_prelude_for_target(out, true);
    }
    push_portable_target_bindings(out, machine, include_alloc);
    out.push('\n');
}

fn push_portable_target_bindings(
    out: &mut String,
    machine: &jet_foundation::TargetMachine::TargetMachine,
    include_alloc: bool,
) {
    let panic_provider = match &machine.panic {
        jet_foundation::TargetMachine::PanicPolicy::Report { provider } => Some(provider),
        jet_foundation::TargetMachine::PanicPolicy::Abort
        | jet_foundation::TargetMachine::PanicPolicy::HostedDefault
        | jet_foundation::TargetMachine::PanicPolicy::Unspecified => None,
    };
    let provider_allocator = matches!(
        &machine.allocator,
        jet_foundation::TargetMachine::AllocatorPolicy::Provider { .. }
    );
    let (read_provider, write_provider, report_provider) = match &machine.byte_sink {
        jet_foundation::TargetMachine::ByteSinkPolicy::Provider {
            read,
            write,
            report,
        } => (read.as_ref(), write.as_ref(), report.as_ref()),
        jet_foundation::TargetMachine::ByteSinkPolicy::HostedDefault => {
            panic!("portable Prelude cannot inherit a hosted byte sink")
        }
        jet_foundation::TargetMachine::ByteSinkPolicy::None
        | jet_foundation::TargetMachine::ByteSinkPolicy::Unspecified => (None, None, None),
    };
    if panic_provider.is_some() && report_provider.is_none() {
        panic!("portable panic-report policy requires a report byte-sink provider");
    }
    let wall_provider = match &machine.wall_clock {
        jet_foundation::TargetMachine::ClockPolicy::Provider { provider } => Some(provider),
        jet_foundation::TargetMachine::ClockPolicy::HostedDefault => {
            panic!("portable Prelude cannot inherit a hosted wall clock")
        }
        jet_foundation::TargetMachine::ClockPolicy::None
        | jet_foundation::TargetMachine::ClockPolicy::Unspecified => None,
    };
    let monotonic_provider = match &machine.monotonic_clock {
        jet_foundation::TargetMachine::ClockPolicy::Provider { provider } => Some(provider),
        jet_foundation::TargetMachine::ClockPolicy::HostedDefault => {
            panic!("portable Prelude cannot inherit a hosted monotonic clock")
        }
        jet_foundation::TargetMachine::ClockPolicy::None
        | jet_foundation::TargetMachine::ClockPolicy::Unspecified => None,
    };
    let sleep_provider = match &machine.sleep {
        jet_foundation::TargetMachine::ClockPolicy::Provider { provider } => Some(provider),
        jet_foundation::TargetMachine::ClockPolicy::HostedDefault => {
            panic!("portable Prelude cannot inherit a hosted sleep provider")
        }
        jet_foundation::TargetMachine::ClockPolicy::None
        | jet_foundation::TargetMachine::ClockPolicy::Unspecified => None,
    };
    let entropy_provider = match &machine.entropy {
        jet_foundation::TargetMachine::EntropyPolicy::Provider { provider } => Some(provider),
        jet_foundation::TargetMachine::EntropyPolicy::HostedDefault => {
            panic!("portable Prelude cannot inherit hosted entropy")
        }
        jet_foundation::TargetMachine::EntropyPolicy::None
        | jet_foundation::TargetMachine::EntropyPolicy::Unspecified => None,
    };
    let mmio_provider = match &machine.mmio {
        jet_foundation::TargetMachine::MmioPolicy::Provider { provider } => Some(provider),
        jet_foundation::TargetMachine::MmioPolicy::HostedDefault => {
            panic!("portable Prelude cannot inherit a hosted MMIO provider")
        }
        jet_foundation::TargetMachine::MmioPolicy::None
        | jet_foundation::TargetMachine::MmioPolicy::Unspecified => None,
    };
    let scheduler_provider = match &machine.scheduler {
        jet_foundation::TargetMachine::SchedulerPolicy::Cooperative { provider }
        | jet_foundation::TargetMachine::SchedulerPolicy::InterruptDriven { provider }
        | jet_foundation::TargetMachine::SchedulerPolicy::BoardRuntime { provider } => {
            Some(provider)
        }
        jet_foundation::TargetMachine::SchedulerPolicy::HostedDefault => {
            panic!("portable Prelude cannot inherit a hosted scheduler")
        }
        jet_foundation::TargetMachine::SchedulerPolicy::None
        | jet_foundation::TargetMachine::SchedulerPolicy::Unspecified => None,
    };
    for (operation, provider) in [
        ("read", read_provider),
        ("write", write_provider),
        ("report", report_provider),
        ("panic_report", panic_provider),
        ("wall_clock", wall_provider),
        ("monotonic_clock", monotonic_provider),
        ("sleep", sleep_provider),
        ("entropy", entropy_provider),
        ("mmio", mmio_provider),
        ("scheduler", scheduler_provider),
    ] {
        if let Some(provider) = provider {
            assert!(
                provider.abi.calling_convention == "C" && provider.abi.version == "1",
                "portable target provider `{}` for {operation} requires the C/1 ABI",
                provider.provider
            );
        }
    }

    let mut declarations = BTreeSet::new();
    if read_provider.is_some() {
        declarations.insert("__jet_target_read");
    }
    if write_provider.is_some() {
        declarations.insert("__jet_target_write");
    }
    if report_provider.is_some() {
        declarations.insert("__jet_target_report");
    }
    if wall_provider.is_some() {
        declarations.insert("__jet_target_wall_clock");
    }
    if monotonic_provider.is_some() {
        declarations.insert("__jet_target_monotonic_clock");
    }
    if sleep_provider.is_some() {
        declarations.insert("__jet_target_sleep");
    }
    if entropy_provider.is_some() {
        declarations.insert("__jet_target_entropy");
    }
    if mmio_provider.is_some() {
        declarations.insert("__jet_target_mmio_read");
        declarations.insert("__jet_target_mmio_write");
    }
    if scheduler_provider.is_some() {
        declarations.insert("__jet_target_scheduler_yield");
    }
    if include_alloc && provider_allocator {
        declarations.insert("__jet_target_alloc");
        declarations.insert("__jet_target_dealloc");
    }
    if !declarations.is_empty() {
        out.push_str("\nextern \"C\" {\n");
        for declaration in declarations {
            match declaration {
                "__jet_target_read" => out.push_str(
                    "    fn __jet_target_read(dst: *mut u8, cap: usize, used: *mut usize) -> i32;\n",
                ),
                "__jet_target_write" => out.push_str(
                    "    fn __jet_target_write(src: *const u8, len: usize, used: *mut usize) -> i32;\n",
                ),
                "__jet_target_report" => out.push_str(
                    "    fn __jet_target_report(src: *const u8, len: usize, used: *mut usize) -> i32;\n",
                ),
                "__jet_target_alloc" => out.push_str(
                    "    fn __jet_target_alloc(size: usize, align: usize) -> *mut u8;\n",
                ),
                "__jet_target_dealloc" => out.push_str(
                    "    fn __jet_target_dealloc(ptr: *mut u8, size: usize, align: usize);\n",
                ),
                "__jet_target_wall_clock" => {
                    out.push_str("    fn __jet_target_wall_clock(out: *mut i64) -> i32;\n")
                }
                "__jet_target_monotonic_clock" => {
                    out.push_str("    fn __jet_target_monotonic_clock(out: *mut u64) -> i32;\n")
                }
                "__jet_target_sleep" => {
                    out.push_str("    fn __jet_target_sleep(nanoseconds: u64) -> i32;\n")
                }
                "__jet_target_entropy" => out.push_str(
                    "    fn __jet_target_entropy(dst: *mut u8, len: usize) -> i32;\n",
                ),
                "__jet_target_mmio_read" => out.push_str(
                    "    fn __jet_target_mmio_read(address: u64, dst: *mut u8, len: usize, used: *mut usize) -> i32;\n",
                ),
                "__jet_target_mmio_write" => out.push_str(
                    "    fn __jet_target_mmio_write(address: u64, src: *const u8, len: usize, used: *mut usize) -> i32;\n",
                ),
                "__jet_target_scheduler_yield" => {
                    out.push_str("    fn __jet_target_scheduler_yield();\n")
                }
                _ => unreachable!("unknown portable provider declaration"),
            }
        }
        out.push_str("}\n");
    }
    out.push_str(&format!(
        "\nconst __JET_TARGET_PANIC_REPORT: bool = {};\n",
        panic_provider.is_some()
    ));
    if include_alloc && provider_allocator {
        out.push_str(
            "\n#[inline(always)]\n\
             unsafe fn jet_target_alloc_impl(size: usize, align: usize) -> *mut u8 {\n\
                 unsafe { __jet_target_alloc(size, align) }\n\
             }\n\
             #[inline(always)]\n\
             unsafe fn jet_target_dealloc_impl(ptr: *mut u8, size: usize, align: usize) {\n\
                 unsafe { __jet_target_dealloc(ptr, size, align) };\n\
             }\n",
        );
    }
    out.push('\n');

    if read_provider.is_some() {
        out.push_str(
            "fn jet_target_read_bytes_impl(buffer: &mut [u8]) -> Result<usize, JetTargetError> {\n\
                 let mut used = 0usize;\n\
                 let status = unsafe { __jet_target_read(buffer.as_mut_ptr(), buffer.len(), &mut used) };\n\
                 if status == 0 { Ok(used) } else { Err(JetTargetError::provider(\"read\", status)) }\n\
             }\n\n",
        );
    } else {
        out.push_str(
            "fn jet_target_read_bytes_impl(_buffer: &mut [u8]) -> Result<usize, JetTargetError> {\n\
                 Err(JetTargetError::unavailable(\"read\"))\n\
             }\n\n",
        );
    }
    if write_provider.is_some() {
        out.push_str(
            "fn jet_target_write_bytes_impl(buffer: &[u8]) -> Result<usize, JetTargetError> {\n\
                 let mut used = 0usize;\n\
                 let status = unsafe { __jet_target_write(buffer.as_ptr(), buffer.len(), &mut used) };\n\
                 if status == 0 { Ok(used) } else { Err(JetTargetError::provider(\"write\", status)) }\n\
             }\n\n",
        );
    } else {
        out.push_str(
            "fn jet_target_write_bytes_impl(_buffer: &[u8]) -> Result<usize, JetTargetError> {\n\
                 Err(JetTargetError::unavailable(\"write\"))\n\
             }\n\n",
        );
    }
    if report_provider.is_some() {
        out.push_str(
            "fn jet_target_report_bytes_impl(buffer: &[u8]) -> Result<usize, JetTargetError> {\n\
                 let mut used = 0usize;\n\
                 let status = unsafe { __jet_target_report(buffer.as_ptr(), buffer.len(), &mut used) };\n\
                 if status == 0 { Ok(used) } else { Err(JetTargetError::provider(\"report\", status)) }\n\
             }\n\n",
        );
    } else {
        out.push_str(
            "fn jet_target_report_bytes_impl(_buffer: &[u8]) -> Result<usize, JetTargetError> {\n\
                 Err(JetTargetError::unavailable(\"report\"))\n\
             }\n\n",
        );
    }
    if wall_provider.is_some() {
        out.push_str(
            "fn jet_target_wall_clock_impl() -> Result<i64, JetTargetError> {\n\
                 let mut value = 0i64;\n\
                 let status = unsafe { __jet_target_wall_clock(&mut value) };\n\
                 if status == 0 { Ok(value) } else { Err(JetTargetError::provider(\"wall_clock\", status)) }\n\
             }\n\n",
        );
    } else {
        out.push_str(
            "fn jet_target_wall_clock_impl() -> Result<i64, JetTargetError> {\n\
                 Err(JetTargetError::unavailable(\"wall_clock\"))\n\
             }\n\n",
        );
    }
    if monotonic_provider.is_some() {
        out.push_str(
            "fn jet_target_monotonic_clock_impl() -> Result<u64, JetTargetError> {\n\
                 let mut value = 0u64;\n\
                 let status = unsafe { __jet_target_monotonic_clock(&mut value) };\n\
                 if status == 0 { Ok(value) } else { Err(JetTargetError::provider(\"monotonic_clock\", status)) }\n\
             }\n\n",
        );
    } else {
        out.push_str(
            "fn jet_target_monotonic_clock_impl() -> Result<u64, JetTargetError> {\n\
                 Err(JetTargetError::unavailable(\"monotonic_clock\"))\n\
             }\n\n",
        );
    }
    if sleep_provider.is_some() {
        out.push_str(
            "fn jet_target_sleep_impl(nanoseconds: u64) -> Result<(), JetTargetError> {\n\
                 let status = unsafe { __jet_target_sleep(nanoseconds) };\n\
                 if status == 0 { Ok(()) } else { Err(JetTargetError::provider(\"sleep\", status)) }\n\
             }\n\n",
        );
    } else {
        out.push_str(
            "fn jet_target_sleep_impl(_nanoseconds: u64) -> Result<(), JetTargetError> {\n\
                 Err(JetTargetError::unavailable(\"sleep\"))\n\
             }\n\n",
        );
    }
    if entropy_provider.is_some() {
        out.push_str(
            "fn jet_target_entropy_impl(buffer: &mut [u8]) -> Result<(), JetTargetError> {\n\
                 let status = unsafe { __jet_target_entropy(buffer.as_mut_ptr(), buffer.len()) };\n\
                 if status == 0 { Ok(()) } else { Err(JetTargetError::provider(\"entropy\", status)) }\n\
             }\n\n",
        );
    } else {
        out.push_str(
            "fn jet_target_entropy_impl(_buffer: &mut [u8]) -> Result<(), JetTargetError> {\n\
                 Err(JetTargetError::unavailable(\"entropy\"))\n\
             }\n\n",
        );
    }
    if mmio_provider.is_some() {
        out.push_str(
            "fn jet_target_mmio_read_impl(address: u64, buffer: &mut [u8]) -> Result<usize, JetTargetError> {\n\
                 let mut used = 0usize;\n\
                 let status = unsafe { __jet_target_mmio_read(address, buffer.as_mut_ptr(), buffer.len(), &mut used) };\n\
                 if status == 0 { Ok(used) } else { Err(JetTargetError::provider(\"mmio_read\", status)) }\n\
             }\n\n\
             fn jet_target_mmio_write_impl(address: u64, buffer: &[u8]) -> Result<usize, JetTargetError> {\n\
                 let mut used = 0usize;\n\
                 let status = unsafe { __jet_target_mmio_write(address, buffer.as_ptr(), buffer.len(), &mut used) };\n\
                 if status == 0 { Ok(used) } else { Err(JetTargetError::provider(\"mmio_write\", status)) }\n\
             }\n\n",
        );
    } else {
        out.push_str(
            "fn jet_target_mmio_read_impl(_address: u64, _buffer: &mut [u8]) -> Result<usize, JetTargetError> {\n\
                 Err(JetTargetError::unavailable(\"mmio_read\"))\n\
             }\n\n\
             fn jet_target_mmio_write_impl(_address: u64, _buffer: &[u8]) -> Result<usize, JetTargetError> {\n\
                 Err(JetTargetError::unavailable(\"mmio_write\"))\n\
             }\n\n",
        );
    }
    if scheduler_provider.is_some() {
        out.push_str(
            "fn jet_target_scheduler_yield_impl() {\n\
                 unsafe { __jet_target_scheduler_yield() };\n\
             }\n\n",
        );
    } else {
        out.push_str(
            "fn jet_target_scheduler_yield_impl() {\n\
                 jet_target_failure_bytes(b\"scheduler provider unavailable\", -1)\n\
             }\n\n",
        );
    }
}

/// Emit the fixed Core closure plus the optional fragments selected by the
/// checked MIR runtime-part IDs. This path deliberately never turns the IDs
/// back into Core module strings.

pub(crate) fn push_full_corelib_prelude_with_policy(
    out: &mut String,
    active_os: Syntax::OSTarget,
    test_harness: bool,
    edition: &str,
    runtime_parts: &BTreeSet<MirRuntimePartId>,
    uses_shared: bool,
    uses_arrow: bool,
    policy: &ReleaseDevtoolsPolicy,
) {
    push_package_edition_value(out, edition);
    let used_core = HashSet::new();
    let forces = CorePreludeForces {
        mapped_file: runtime_parts.contains(&MirRuntimePartId::FsRuntime),
        shared: uses_shared,
        // DataFlow carries the Arrow adapter surface in the Data closure.
        arrow: uses_arrow || runtime_parts.contains(&MirRuntimePartId::Data),
        // MIR closure emission uses the canonical typed history carriers for
        // function values even when no `core.testing` call is reached.
        testing_history: true,
    };
    if test_harness {
        push_corelib_prelude_for_test_harness_with_forces(
            out,
            &used_core,
            true,
            forces,
            policy,
        );
    } else {
        push_corelib_prelude_inner_with_forces(out, &used_core, true, false, forces, policy);
    }
    out.push_str(scheduler_prelude_for_emit(true));
    out.push_str(CORE_STREAM_PRELUDE_RAW);
    out.push_str(CORE_STREAM_DURATION_ADAPTER);
    out.push_str(include_str!("../Prelude/Core/CollectionSources.rs"));
    if runtime_parts.contains(&MirRuntimePartId::Realtime) {
        out.push_str(CORE_REALTIME_PRELUDE_RAW);
    }
    if runtime_parts.contains(&MirRuntimePartId::EmbeddedHardware) {
        out.push_str(&flat_prelude_body(CORE_EMBEDDED_HARDWARE_PRELUDE_RAW));
    }
    out.push_str(&flat_prelude_body(CORE_PARALLEL_PLAN_PRELUDE_RAW));
    if runtime_parts.contains(&MirRuntimePartId::Ui) {
        out.push_str(&canonical_font_prelude());
        out.push_str(include_str!("../Prelude/Core/HostServices.rs"));
        out.push_str("\nmod jet_tui_kernel {\n");
        out.push_str(TUI_KERNEL_PRELUDE);
        out.push_str("\n}\n");
        out.push_str(UI_PRELUDE);
        out.push_str(PREVIEW_PRELUDE);
    }
    if policy.panel_code && runtime_parts.contains(&MirRuntimePartId::Devtools) {
        out.push_str(DEVTOOLS_PANEL_PRELUDE);
        out.push_str(DEVTOOLS_PANEL_CATALOG_PRELUDE_RAW);
        out.push_str(DEVTOOLS_DATABASE_PANEL_PRELUDE_RAW);
        out.push_str(DEVTOOLS_JOBS_PANEL_PRELUDE_RAW);
        out.push_str(DEVTOOLS_REQUEST_PANEL_PRELUDE_RAW);
        out.push_str(DEVTOOLS_TELEMETRY_PANEL_PRELUDE_RAW);
        out.push_str(DEVTOOLS_TOPOLOGY_PANEL_PRELUDE_RAW);
    }
    if runtime_parts.contains(&MirRuntimePartId::Gtk)
        && matches!(active_os, Syntax::OSTarget::Linux)
    {
        out.push_str(UI_GTK_PRELUDE);
    }
    push_typed_core_optional_parts(out, runtime_parts, test_harness, policy);
    push_typed_app_preludes(out, runtime_parts);
}
fn push_typed_core_optional_parts(
    out: &mut String,
    runtime_parts: &BTreeSet<MirRuntimePartId>,
    omit_testing_shared: bool,
    policy: &ReleaseDevtoolsPolicy,
) {
    let needs_data = runtime_parts.contains(&MirRuntimePartId::Data);
    if runtime_parts.contains(&MirRuntimePartId::Game) {
        push_game_debug_policy_const(out, policy);
        out.push_str(&flat_prelude_body(CORE_GAME_DEV_KERNEL_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_ASSET_PIPELINE_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_HOT_SWAP_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_WORLD_INSPECTOR_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_FRAME_PROFILER_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_OVERLAY_PRELUDE_RAW));
        push_game_devtools_control_prelude(out);
        out.push_str(include_str!("../Prelude/CoreLib/Top/GameDevProtocol.rs"));
        out.push_str(&flat_prelude_body(GAME_ASSETS_IMPORT_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(GAME_ASSETS_RUNTIME_PRELUDE_RAW));
        out.push_str(include_str!("../Prelude/CoreLib/Top/Game.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::Files) {
        out.push_str(include_str!("../Prelude/CoreLib/Top/PathFiles.rs"));
    } else if runtime_parts.contains(&MirRuntimePartId::FsRuntime) {
        out.push_str(include_str!("../Prelude/CoreLib/Top/PathFiles.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::Interrupt)
        || runtime_parts.contains(&MirRuntimePartId::Process)
        || runtime_parts.contains(&MirRuntimePartId::FsRuntime)
    {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Interrupt.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::Process)
        || runtime_parts.contains(&MirRuntimePartId::FsRuntime)
    {
        out.push_str("\nmod jet_process_pty {\n");
        out.push_str(include_str!("../Prelude/CoreLib/ProcessPty.rs"));
        out.push_str("\n}\n");
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_process_sandbox\n");
        out.push_str("\nmod jet_process_sandbox {\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/ProcessSandbox.rs"));
        out.push_str(include_str!(
            "../Prelude/CoreLib/Top/ProcessWindowsSandbox.rs"
        ));
        out.push_str("\n}\n");
        out.push_str("// JET_VETTED_UNSAFE_END: jet_process_sandbox\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/ProcessPolicy.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/ProcessSpec.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/Process.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::FsRuntime) {
        if !omit_testing_shared {
            out.push_str(include_str!("../Prelude/CoreLib/Top/TestingShared.rs"));
        }
        out.push_str(include_str!("../Prelude/CoreLib/Top/IoLineStream.rs"));
        out.push_str(include_str!("../Prelude/Core/EnvConfig.rs"));
        out.push_str(include_str!("../Prelude/Core/EnvProjection.rs"));
        out.push_str(include_str!("../Prelude/Core/FSOps.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/FileStream.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/FSRuntimeOps.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/FSIoEnvOsTesting.rs"));
        out.push_str(include_str!("../Prelude/Core/CollectionIoSources.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/FSWriteOps.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/PlatformFamily.rs"));
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_os_extra\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/OsExtra.rs"));
        out.push_str("// JET_VETTED_UNSAFE_END: jet_os_extra\n");
    }
    if runtime_parts.contains(&MirRuntimePartId::Math) {
        out.push_str(include_str!("../Prelude/CoreLib/Top/MathLibPure.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/MathComplexTraits.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/MathRandomFns.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/LinalgFns.rs"));
    }
    if needs_data {
        out.push_str(&data_plot_prelude_body());
    }
    if runtime_parts.contains(&MirRuntimePartId::DataFmt) {
        // JetTablePlan is the internal semantic plan for Query values.
        out.push_str(&data_query_prelude_source());
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataFmt.rs"));
    }
    if needs_data {
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataStats.rs"));
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_data_flow\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataFlow.rs"));
        out.push_str("\n// JET_VETTED_UNSAFE_END: jet_data_flow\n");
    }
    if runtime_parts.contains(&MirRuntimePartId::Compute) {
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_compute\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/Compute.rs"));
        out.push_str("\n// JET_VETTED_UNSAFE_END: jet_compute\n");
    }
    if runtime_parts.contains(&MirRuntimePartId::Http) {
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPMessage.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPRoute.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPClient.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPServer.rs"));
    } else if runtime_parts.contains(&MirRuntimePartId::WebSocket)
        || runtime_parts.contains(&MirRuntimePartId::Browser)
    {
        out.push_str(include_str!("../Prelude/CoreLib/Top/HTTPMessage.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::WebSocket)
        || runtime_parts.contains(&MirRuntimePartId::Http)
        || runtime_parts.contains(&MirRuntimePartId::Browser)
    {
        out.push_str(include_str!("../Prelude/CoreLib/Top/WsClient.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/Ws.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::Browser) {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Browser.rs"));
        out.push_str(include_str!("../Prelude/BrowserTest.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::Args) {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Args.rs"));
        out.push_str(include_str!("../Prelude/Core/ArgsProjectionCore.rs"));
        out.push_str(include_str!("../Prelude/Core/ArgsProjection.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::Reflect) {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Reflect.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::AuthTokens) {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Auth.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::AuthSession) {
        out.push_str(include_str!("../Prelude/CoreLib/Top/AuthSession.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::Sync) {
        out.push_str("\nmod jet_sync {\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/Sync.rs"));
        out.push_str("\n}\npub(crate) use jet_sync::*;\n");
    }
    push_runtime_devtools_panel_preludes(
        out,
        policy.panel_code,
        runtime_parts.contains(&MirRuntimePartId::Devtools),
        runtime_parts.contains(&MirRuntimePartId::Data)
            || runtime_parts.contains(&MirRuntimePartId::DataFmt),
        runtime_parts.contains(&MirRuntimePartId::Http),
        runtime_parts.contains(&MirRuntimePartId::Process)
            || runtime_parts.contains(&MirRuntimePartId::FsRuntime)
            || runtime_parts.contains(&MirRuntimePartId::Http)
            || runtime_parts.contains(&MirRuntimePartId::Services),
    );
    if runtime_parts.contains(&MirRuntimePartId::Services) {
        out.push_str(service_authority_prelude_for_emit());
        out.push_str(JOB_QUEUE_NATIVE_PRELUDE_RAW);
        out.push_str(include_str!("../Prelude/CoreLib/Top/Services.rs"));
    }
    if runtime_parts.contains(&MirRuntimePartId::Mod) {
        out.push_str(&format!(
            "\nconst __JET_COMPILER_VERSION: &str = {:?};\n",
            env!("CARGO_PKG_VERSION")
        ));
        out.push_str(include_str!("../Prelude/CoreLib/Top/Mod.rs"));
        out.push('\n');
    }
}

/// Emit only the typed fact definitions needed by ordinary Core producers when
/// the full panel surface is compiled out. The shared publish gate is
/// const-folded in release, so these producer-only facts carry no live
/// observation path in the resulting artifact.
fn push_runtime_devtools_panel_preludes(
    out: &mut String,
    devtools_panel_code: bool,
    has_devtools_panel: bool,
    needs_database_panel: bool,
    needs_request_panel: bool,
    needs_topology_panel: bool,
) {
    if devtools_panel_code && has_devtools_panel {
        return;
    }
    if needs_database_panel {
        out.push_str(DEVTOOLS_DATABASE_PANEL_PRELUDE_RAW);
    }
    if needs_request_panel {
        out.push_str(DEVTOOLS_REQUEST_PANEL_PRELUDE_RAW);
    }
    if needs_topology_panel {
        out.push_str(DEVTOOLS_TOPOLOGY_PANEL_PRELUDE_RAW);
    }
}

fn push_typed_app_preludes(out: &mut String, runtime_parts: &BTreeSet<MirRuntimePartId>) {
    if !runtime_parts.contains(&MirRuntimePartId::Apps) {
        return;
    }
    push_app_middleware_prelude(out);
    out.push_str(APP_PRELUDE);
    out.push_str(LIVEQUERY_PRELUDE);
    out.push_str(&flat_prelude_body(CORE_WEB_PENDING_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(WEB_ROUTER_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(OPENAPI_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(WEB_QUERY_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(WEB_FORMS_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(WEB_TABLE_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(WEB_VIRTUAL_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(WEB_STORE_PRELUDE_RAW));
    out.push_str(&flat_prelude_body(WEBSERVERFN_PRELUDE_RAW));
}


/// The R10 Core closure rides in its own content-addressed rlib. This includes
/// scheduler/UI/app templates: Core calls them, so splitting only the kernel
/// would create a circular dependency or force an inline fallback.

/// R10 / #1133: content identity of the semantic Core closure and emitted
/// compiler/runtime fragments a program will link. The cache key is the sorted
/// used-Core closure plus every input to the shared Core-body assembler. The
/// bounded cache is process-local; the emitted source remains the source of
/// truth.
pub fn corelib_emission_fingerprint(bundle: &ProgramBundle, test_harness: bool) -> String {
    corelib_emission_fingerprint_with_policy(
        bundle,
        test_harness,
        &ReleaseDevtoolsPolicy::development(),
    )
}

pub fn corelib_emission_fingerprint_with_policy(
    bundle: &ProgramBundle,
    test_harness: bool,
    policy: &ReleaseDevtoolsPolicy,
) -> String {
    let mut used_core = bundle.used_core.iter().cloned().collect::<Vec<_>>();
    used_core.sort_unstable();
    let cache_key = CoreEmissionFingerprintKey {
        used_core,
        active_os: bundle.active_os.name().to_string(),
        edition: bundle.edition.clone(),
        force_corelib: force_corelib_prelude(bundle),
        test_harness,
        release_inspect: policy.release_cfg_value().map(str::to_string),
        devtools_panel_code: policy.panel_code,
        devtools_stream_code: policy.stream_code,
    };
    let cache = CORELIB_EMISSION_FINGERPRINTS.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(fingerprint) = cache.lock().unwrap().get(&cache_key).cloned() {
        return fingerprint;
    }

    let body = core_runtime_body_with_policy(bundle, test_harness, policy);
    let fingerprint = corelib_emission_identity(&body, &bundle.used_core);
    let mut entries = cache.lock().unwrap();
    if entries.len() >= CORELIB_DIGEST_CACHE_LIMIT {
        if let Some(oldest) = entries.keys().next().cloned() {
            entries.remove(&oldest);
        }
    }
    entries.insert(cache_key, fingerprint.clone());
    fingerprint
}

/// The exact Core closure selected for one checked bundle.  `used_calls` is
/// the sema-owned direct-call set; `synthesized_calls` is the set of actual
/// Prelude function symbols present in the assembled closure.  The adapter
/// labels are descriptive evidence only — they do not execute any adapter or
/// re-implement Core policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreClosureProof {
    pub used_calls: Vec<String>,
    pub synthesized_calls: Vec<String>,
    pub adapter_routes: Vec<String>,
    pub fingerprint: String,
}

/// Project the same Core source closure used by emission and cache identity.
/// This is intentionally side-effect free and does not compile or link the
/// assembled Rust body.
pub fn core_closure_proof(bundle: &ProgramBundle, test_harness: bool) -> CoreClosureProof {
    let mut used_calls = bundle.used_core.iter().cloned().collect::<Vec<_>>();
    used_calls.sort_unstable();
    let body = core_runtime_body(bundle, test_harness);
    let mut synthesized_calls = body
        .lines()
        .filter_map(|line| {
            let name = line
                .trim_start()
                .strip_prefix("pub fn ")
                .or_else(|| line.trim_start().strip_prefix("fn "))?
                .split('(')
                .next()?;
            name.starts_with("jet_").then(|| name.to_string())
        })
        .collect::<Vec<_>>();
    synthesized_calls.sort_unstable();
    synthesized_calls.dedup();
    let imports = core_imports_for_bundle(bundle);
    let adapter_routes = if body.is_empty() && imports.is_empty() {
        vec!["none".to_string()]
    } else {
        vec![
            "aot:embedded-prelude".to_string(),
            "jit:core-import-map".to_string(),
            "interpreter:core-import-map".to_string(),
        ]
    };
    CoreClosureProof {
        used_calls,
        synthesized_calls,
        adapter_routes,
        fingerprint: corelib_emission_fingerprint(bundle, test_harness),
    }
}

fn push_corelib_prelude_body(
    out: &mut String,
    used_core: &std::collections::HashSet<String>,
    omit_testing_shared: bool,
    forces: CorePreludeForces,
    policy: &ReleaseDevtoolsPolicy,
) {
    // Typed live watches and the global app registry share one lifecycle
    // transition kernel. Emit it at the Core root because DataWatch lives in
    // the unconditional JetStd data carrier, while LiveQuery remains optional.
    out.push_str(LIVE_LIFECYCLE_PRELUDE);
    // Foundation EncodingJson/JSON/diagnostic kernels import
    // `crate::DataTree::DataTree`; emit the canonical carrier at crate root
    // before the JetStd brace chain begins. JetStd re-exports this same type
    // through DATATREE_PRELUDE_REEXPORT below.
    out.push_str(DATATREE_FOUNDATION_ROOT);
    // D-SIMD1/D-SIMD2/D-SIMD3 / I9: MathTaskMem's fixed-array lane values
    // call this root-level kernel. JIT and TIR/comptime include the same
    // source directly and marshal their resident carriers into its slice API.
    out.push_str(include_str!("../Prelude/Core/SimdLanes.rs"));
    // D-PLACE1=A: keep the checked Atomic<T> carrier at crate root so its
    // #Layout(c) representation is identical across generated AOT tiers.
    out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_atomic_carrier\n");
    out.push_str(&flat_prelude_body(CORE_ATOMIC_PRELUDE_RAW));
    out.push_str("// JET_VETTED_UNSAFE_END: jet_atomic_carrier\n");
    out.push('\n');
    out.push('\n');
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
    out.push_str(
        "\n#[allow(non_snake_case)]\nmod CborBudget { pub use crate::jet_cbor_budget::*; }\n",
    );
    out.push_str("\nmod jet_cbor_kernel {\n");
    out.push_str(include_str!("../../../jet-foundation/src/CborKernel.rs"));
    out.push_str("\n}\n");
    out.push_str("\nmod jet_csv_kernel {\n");
    out.push_str(include_str!("../../../jet-foundation/src/CsvKernel.rs"));
    out.push_str("\n}\n");
    out.push_str("\nmod jet_base_encoding_strict {\n");
    out.push_str(include_str!(
        "../../../jet-foundation/src/BaseEncodingStrict.rs"
    ));
    out.push_str("\n}\n");
    out.push_str("\nmod jet_regex_syntax {\n");
    out.push_str(include_str!("../../../jet-foundation/src/RegexSyntax.rs"));
    out.push_str("\n}\n");
    let needs_xml = core_usage_matches(used_core, &["core.encoding.xml", "core.encoding"]);
    let needs_base = core_usage_matches(
        used_core,
        &[
            "core.encoding",
            "core.encoding.hex",
            "core.encoding.cbor",
            "core.encoding",
        ],
    );
    let _needs_regex = core_usage_matches(used_core, &["core.regex"]);
    // needs_xml / needs_base still drive encoding Top reachability below.
    let needs_files = core_usage_matches(
        used_core,
        &[
            "core.files",
            "core.watcher",
            "core.term",
            "core.sys",
            "core.process",
        ],
    );
    let needs_fs_runtime = needs_files
        || core_usage_matches(
            used_core,
            &[
                "core.args",
                "core.process",
                "core.testing",
                "core.perf",
                "core.mem.scope",
            ],
        );
    let needs_mapped_file = forces.mapped_file || needs_fs_runtime;
    let needs_shared =
        forces.shared || core_usage_matches(used_core, &["core.mem::pool_shared", "core.mem.pool_shared"]);


    for part in CORELIB_KERNEL_PARTS {
        if (!needs_mapped_file && *part == MAPPED_FILE_PRELUDE)
            || (!needs_shared && *part == SHARED_ROUTES_PRELUDE)
        {
            continue;
        }
        // Host crates include UrlMime.rs directly, so it includes its sibling
        // MIME kernel. AOT already embeds that kernel as the preceding part.
        out.push_str(
            part.strip_prefix("    include!(\"Mime.rs\");\n\n")
                .unwrap_or(part),
        );
    }
    // Typed query plans are shared semantic facts. Keep the root-level
    // definitions available to JetStd's Query carrier even when no optional
    // DataFmt fragment is selected.
    out.push_str(&flat_prelude_body(CORE_LAZY_TABLE_PLAN_PRELUDE_RAW));
    // #1451: `Prelude/CommandSuite.rs` is an unconditional kernel part in the
    // loop above, so `jet_std::jet_test_suite_run` always exists. The TIR
    // emitter spells the `suite.run()` handle op unqualified at crate root and
    // never consults `used_core`: a program that only *receives* a `TestSuite`
    // (the `fn test` override body, which `--show-default` leaves in the
    // program as ordinary user code) reaches the runner without ever calling
    // `core.testing`. Gating these root adapters on `needs_fs_runtime` emitted
    // that call without its definition — rustc E0425 on generated code, an I2
    // compiler bug. So the adapters are pinned to the kernel part that defines
    // them: the harness carries `#![allow(warnings)]`, eight unused forwarders
    // cost nothing, and a definition that is never gated cannot drift from an
    // ungated use again (same fix shape as `push_package_edition`).
    out.push_str(include_str!("../Prelude/CoreLib/Top/CommandSuite.rs"));
    // NEVER add `JetTaskFailure` here. `jet-foundation/src/Outcome.rs` declares
    // it and rides in `PRELUDE_PARTS`, so it is already a flat top-level item in
    // every generated program. Importing it into that same scope is E0255.
    // Flat fragments outside `mod jet_std` name it unqualified.
    out.push_str("\npub use crate::jet_std::JetTaskGroupRuntime;\n");
    // D-SERDE-ACCESS=B: the Foundation carrier's accessors are a trait in the
    // kernel's `DataTree.rs`; flat user code calls them by method syntax.
    out.push_str("\npub use crate::jet_std::JetDataTreeAccess;\n");
    // CommonTypes' exact-Int formatting adapters are always emitted.
    out.push_str(include_str!("../Prelude/Core/Fmt.rs"));
    out.push_str(include_str!("../Prelude/Core/FmtAot.rs"));
    // Card #1751: the one 80x24 terminal default, read by CommonTypes.rs's
    // TerminalPolicy::default (in the kernel closure above) and by
    // ProcessPty.rs's PtyConfig::default when process/PTY support is emitted.
    // Unconditional like the kernel closure, so both can always reach it.
    out.push_str("\nmod terminal_default {\n");
    out.push_str(include_str!("../Prelude/TerminalDefault.rs"));
    out.push_str("\n}\n");

    let needs_email = core_usage_matches(used_core, &["core.email"]);
    let needs_raylib = core_usage_matches(used_core, &["core.game.raylib"]);
    let needs_game = core_usage_matches(used_core, &["core.game"]) || needs_raylib;
    let needs_text = core_usage_matches(used_core, &["core.text", "core.text.fmt", "core.term"]);
    let needs_crypto = core_usage_matches(
        used_core,
        &[
            "core.crypto",
            "core.crypto.expert",
            "core.crypto.random",
            "core.crypto.vault",
            "core.crypto.uuid",
        ],
    );
    let needs_process = core_usage_matches(used_core, &["core.process"]);
    let needs_math = core_usage_matches(
        used_core,
        &[
            "core.math",
            "core.math.random",
            "core.time",
            "core.time.expiring",
            "core.units",
        ],
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
            "core.archive.gzip",
            "core.archive.zstd",
        ],
    ) || needs_xml
        || needs_base;
    let needs_data = core_usage_matches(
        used_core,
        &[
            "core.data",
            "core.data.sketch.hll",
            "core.data.sketch.tdigest",
            "core.data.sketch.cms",
            "core.data.sketch.reservoir",
            "core.db",
            "core.encoding.csv",
        ],
    );
    let needs_data_fmt = needs_data || needs_encoding;
    let needs_arrow =
        forces.arrow || core_usage_matches(used_core, &["core.data.arrow"]) || needs_data;
    if needs_arrow {
        push_prelude_dependency_closure(out, &["arrow_file_reader"]);
    }
    let needs_compute = core_usage_matches(used_core, &["core.compute"]);
    let needs_net = core_usage_matches(
        used_core,
        &[
            "core.net",
            "core.net.tls",
            "core.http",
            "core.http.client",
            "core.http.server",
            "core.net.ws",
            "core.email",
            "core.web.browser",
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
    // D-LIVEQUERY1: live results use the existing core.net.ws writer path. Keep
    // the transport adapter in the same generated program whenever a live
    // surface can create or publish a query.
    let needs_ws = core_usage_matches(
        used_core,
        &[
            "core.net.ws",
            "app",
            "core.web",
            "core.db",
            "core.http",
            "core.http.client",
            "core.http.server",
            "core.web.browser",
        ],
    );
    let needs_browser = core_usage_matches(
        used_core,
        &[
            "core.web.browser",
            "core.web",
            "core.web.storage",
            "core.web.storage.local",
            "core.web.storage.session",
        ],
    );
    let needs_args = core_usage_matches(used_core, &["core.args"]);
    let needs_reflect = core_usage_matches(used_core, &["core.reflect", "core.compiler.lang"]);
    let needs_auth_tokens = core_usage_matches(used_core, &["core.auth"]) || needs_crypto;
    let needs_auth_session = core_usage_matches(used_core, &["core.auth", "app", "core.web"]);
    let needs_sync = core_usage_matches(used_core, &["core.sync", "app", "core.web", "core.db"]);
    let needs_services = core_usage_matches(used_core, &["core.service"]);
    let needs_jobs = core_usage_matches(used_core, &["core.jobs"]);
    push_runtime_devtools_panel_preludes(
        out,
        policy.panel_code,
        core_usage_matches(used_core, &["core.ui", "core.devtools"]),
        needs_data_fmt,
        needs_http,
        needs_process || needs_fs_runtime || needs_http || needs_services,
    );
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
    // Keep the shared Raylib parser/transcript inside the existing marker so
    // unused programs can still remove the complete unsafe bridge.
    out.push_str("// jet:raylib-begin\n");
    out.push_str(include_str!("../Prelude/Core/Raylib.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/HandlesRaylib.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/DbPool.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/UnicodeTables.rs"));
    out.push_str(include_str!("../Prelude/Core/Path.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/Text.rs"));
    out.push_str(include_str!("../Prelude/Core/Codec.rs"));
    out.push_str(&flat_prelude_body(ENCODING_TRAITS_PRELUDE_RAW));
    // D-SOA2D: columnar-list transparency rides with the codec traits it names,
    // so it exists exactly when a program has encoding at all.
    out.push_str(include_str!("../Prelude/Core/ColumnListCodec.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/EncodingHostileIo.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/EncodingStream.rs"));
    out.push_str(include_str!("../Prelude/Core/EncodingError.rs"));
    out.push_str(include_str!("../Prelude/Core/EncodingBase.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/EncodingCodecs.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/SHA256Raw.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/SHAFamily.rs"));
    out.push_str("fn jet_log_write_line(line: &str) { eprintln!(\"{}\", line); }\n");
    out.push_str("fn jet_log_process_exit(code: i64) { std::process::exit(code as i32); }\n");
    out.push_str(include_str!("../Prelude/Core/LogState.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/Log.rs"));
    out.push_str(include_str!(
        "../Prelude/CoreLib/Top/RingCsvLogTimeCrypto.rs"
    ));
    out.push_str(include_str!("../Prelude/CoreLib/Top/CryptoEntropy.rs"));
    out.push_str("use jet_crypto_entropy::{jet_crypto_entropy_fill, JetCryptoEntropyError};\n");
    out.push_str(include_str!("../Prelude/CoreLib/Top/DNSResolverPolicy.rs"));
    out.push_str(include_str!("../Prelude/Deadline.rs"));
    out.push_str(include_str!("../Prelude/WorkflowWait.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/TimeSleep.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/WorkflowSleep.rs"));
    out.push_str(include_str!("../Prelude/Core/NetPure.rs"));
    out.push_str(include_str!("../Prelude/Core/NetError.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/NetHTTP.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/Solver.rs"));
    // Scheduler waits always call the deadline raiser, and FakeData below
    // always uses the seeded stream. They are kernel dependencies, not
    // conditional math surface.
    out.push_str(include_str!("../Prelude/Core/SeededRandom.rs"));
    out.push_str(include_str!("../Prelude/Core/StringBytes.rs"));
    out.push_str(include_str!("../Prelude/Core/TimeInstant.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/MathRandomTime.rs"));
    out.push_str(include_str!("../Prelude/CoreLib/Top/FakeData.rs"));

    if needs_email {
        out.push_str(include_str!("../Prelude/CoreLib/Email.rs"));
    }
    if needs_raylib {
        // File/DB/Plugin handles already emitted in the kernel closure above;
        // raylib-only when explicitly used stays a no-op re-include guard via
        // the same source (idempotent struct defs would conflict). Skip.
    }
    if needs_game {
        push_game_debug_policy_const(out, policy);
        // adapters; each is one flat source and none calls a host.
        out.push_str(&flat_prelude_body(CORE_GAME_DEV_KERNEL_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_ASSET_PIPELINE_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_HOT_SWAP_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_WORLD_INSPECTOR_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_FRAME_PROFILER_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(CORE_GAME_OVERLAY_PRELUDE_RAW));
        push_game_devtools_control_prelude(out);
        out.push_str(include_str!("../Prelude/CoreLib/Top/GameDevProtocol.rs"));
        // Asset import hashing uses the raw SHA-256 helper emitted above.
        out.push_str(&flat_prelude_body(GAME_ASSETS_IMPORT_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(GAME_ASSETS_RUNTIME_PRELUDE_RAW));
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
        // Process.rs and FSIoEnvOsTesting share the one dispatcher support
        // source. Emit it before either adapter for process-only closures too.
        out.push_str(include_str!("../Prelude/CoreLib/Top/Interrupt.rs"));
        out.push_str("\nmod jet_process_pty {\n");
        out.push_str(include_str!("../Prelude/CoreLib/ProcessPty.rs"));
        out.push_str("\n}\n");
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_process_sandbox\n");
        out.push_str("\nmod jet_process_sandbox {\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/ProcessSandbox.rs"));
        out.push_str(include_str!(
            "../Prelude/CoreLib/Top/ProcessWindowsSandbox.rs"
        ));
        out.push_str("\n}\n");
        out.push_str("// JET_VETTED_UNSAFE_END: jet_process_sandbox\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/ProcessPolicy.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/ProcessSpec.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/Process.rs"));
    }
    let needs_testing_history =
        forces.testing_history || core_usage_matches(used_core, &["core.testing"]);
    if needs_testing_history {
        // D-TEST-COMPARE1=A: every generated tier reaches the same Foundation
        // verdict engine.  The Top carrier only marshals checked callables and
        // observations into this embedded source module.
        out.push_str("\nmod jet_testing_comparison_foundation {\n");
        out.push_str(include_str!("../../../jet-foundation/src/TestingComparison.rs"));
        out.push_str("\n}\n");
        // The Foundation history module is embedded once beside the Top
        // adapter. Its host source names the canonical JSON and comparison
        // modules at crate root; these aliases keep generated native and Wasm
        // programs on that same source path without adding unused global
        // facade entries to programs that do not use core.testing.
        out.push_str(
            "\n#[allow(non_snake_case)]\nmod TestingComparison { pub use crate::jet_testing_comparison_foundation::*; }\n\
             #[allow(non_snake_case)]\nmod JSON {\n",
        );
        out.push_str(include_str!("../../../jet-foundation/src/JSON.rs"));
        out.push_str(
            "\n}\n// JET_VETTED_UNSAFE_BEGIN: jet_testing_history_foundation\n\
             // AUDIT: D-TEST-HISTORY1 and the Foundation SAFETY CONTRACT keep\n\
             // weak callback metadata non-owning and reads tied to live owners.\n\
             mod jet_testing_history_foundation {\n",
        );
        out.push_str(include_str!("../../../jet-foundation/src/TestingHistory.rs"));
        out.push_str("\n}\n// JET_VETTED_UNSAFE_END: jet_testing_history_foundation\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/TestingComparison.rs"));
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_testing_history_top\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/TestingHistory.rs"));
        out.push_str("\n// JET_VETTED_UNSAFE_END: jet_testing_history_top\n");
    }
    if needs_fs_runtime {
        if !omit_testing_shared {
            out.push_str(include_str!("../Prelude/CoreLib/Top/TestingShared.rs"));
        }
        // #1480: split out of FSIoEnvOsTesting.rs so the JIT host can
        // `include!` this exact source (I9 — single Prelude source of truth).
        out.push_str(include_str!("../Prelude/CoreLib/Top/IoLineStream.rs"));
        // D-CONFIG-ENV1 / I9: source overlay and dotenv parsing are one
        // Prelude fragment for AOT and the interpreter ambient adapter.
        out.push_str(include_str!("../Prelude/Core/EnvConfig.rs"));
        out.push_str(include_str!("../Prelude/Core/EnvProjection.rs"));
        out.push_str(include_str!("../Prelude/Core/FSOps.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/FileStream.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/FSRuntimeOps.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/FSIoEnvOsTesting.rs"));
        out.push_str(include_str!("../Prelude/Core/CollectionIoSources.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/FSWriteOps.rs"));
        // #1465: identity / release / POSIX control — after FSIoEnvOsTesting so
        // jet_std_os_pid / env helpers and jet_std_process_exit stay in scope.
        // Vetted region: OsExtra carries POSIX `unsafe` at crate root (not only
        // inside `mod jet_os_sys`); golden I1 strips this delimiter.
        out.push_str(include_str!("../Prelude/CoreLib/Top/PlatformFamily.rs"));
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
        out.push_str(include_str!("../Prelude/CoreLib/Top/MathComplexTraits.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/MathRandomFns.rs"));
        out.push_str(include_str!("../Prelude/CoreLib/Top/LinalgFns.rs"));
    }
    if needs_encoding {
        // Encoding templates already in kernel closure.
    }
    if needs_data {
        out.push_str(&data_plot_prelude_body());
    }
    if needs_data_fmt {
        out.push_str(&data_query_prelude_source());
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataFmt.rs"));
    }
    if needs_data {
        // #1657: the one `core.data` statistics kernel. The JIT host and the
        // comptime tier `include!` this same file, so every tier runs the same
        // compensated arithmetic and reports the same `DataError` (I9).
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataStats.rs"));
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_data_flow\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/DataFlow.rs"));
        out.push_str("\n// JET_VETTED_UNSAFE_END: jet_data_flow\n");
    }
    if needs_compute {
        out.push_str("// JET_VETTED_UNSAFE_BEGIN: jet_compute\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/Compute.rs"));
        out.push_str("\n// JET_VETTED_UNSAFE_END: jet_compute\n");
    }
    if needs_net {
        // DNS + NetHTTP (TCP/TLS) already in kernel closure.
    }
    if needs_http && !needs_sync {
        // HTTP handlers install the request identity for DB scopes when Sync
        // is present. Keep HTTP-only programs source-complete with a no-op
        // guard rather than making the HTTP prelude depend on core.sync.
        out.push_str(
            "struct JetDbRequestScope;\n\
impl JetDbRequestScope {{ fn enter(_: Option<String>) -> Self {{ Self }} }}\n",
        );
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
        out.push_str(include_str!("../Prelude/BrowserTest.rs"));
    }
    if needs_args {
        out.push_str(include_str!("../Prelude/CoreLib/Top/Args.rs"));
        out.push_str(include_str!("../Prelude/Core/ArgsProjectionCore.rs"));
        out.push_str(include_str!("../Prelude/Core/ArgsProjection.rs"));
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
        // The module header and its crate-root re-export each need their own
        // line: the cached-runtime export pass reads the block line by line to
        // widen `mod`/`pub(crate) use` for the split rlib, so a header glued to
        // a previous fragment's last line would stay crate-private and hide the
        // whole `core.sync` surface from the program crate.
        out.push_str("\nmod jet_sync {\n");
        out.push_str(include_str!("../Prelude/CoreLib/Top/Sync.rs"));
        out.push_str("\n}\npub(crate) use jet_sync::*;\n");
    }
    if needs_services || needs_jobs {
        out.push_str(service_authority_prelude_for_emit());
        out.push_str(JOB_QUEUE_NATIVE_PRELUDE_RAW);
        out.push_str(include_str!("../Prelude/CoreLib/Top/Services.rs"));
    }
}
const SCHEDULER_PRELUDE_RAW: &str = include_str!("../Prelude/Scheduler.rs");
const STREAM_PRELUDE_RAW: &str = include_str!("../Prelude/Stream.rs");
/// D-RENDERTGT1=A + D-RENDERTGT2=A (c133 M1): UI backend trait seam + null backend.
const UI_PRELUDE: &str = include_str!("../Prelude/Ui.rs");
/// One physical TUI policy kernel is embedded in foundation and AOT output.
const TUI_KERNEL_PRELUDE: &str =
    include_str!("../../../jet-foundation/src/Prelude/TuiKernel.rs");
/// D-DX-PREVIEW1=A: typed named previews/playgrounds use the canonical UiNode.
const PREVIEW_PRELUDE: &str = include_str!("../Prelude/Core/Preview.rs");
/// D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend. Emitted only when
/// a program constructs `core.ui.gtk_backend()` (`uses_gtk_backend_for`), so no
/// other program carries the gtk `extern "C"` surface or needs `-lgtk-4`.
const UI_GTK_PRELUDE: &str = include_str!("../Prelude/UiGtk.rs");
/// D-DX-PLUGIN1=D: typed panel values share the Devtools protocol and UI node.
const DEVTOOLS_PANEL_PRELUDE: &str = include_str!("../Prelude/Core/DevtoolsPanel.rs");
const CORE_STREAM_PRELUDE_RAW: &str = include_str!("../Prelude/Core/Stream.rs");
const DATA_PLOT_PRELUDE_RAW: &str = include_str!("../Prelude/CoreLib/Top/DataPlot.rs");
const DATA_PLOT_PRELUDE_IMPORTS: &str =
    "use std::collections::{BTreeMap, BTreeSet};\nuse std::sync::Arc as JetDataPlotArc;\n";
const CORE_STREAM_DURATION_ADAPTER: &str = r#"
impl JetStreamDuration for jet_std::Duration {
    fn stream_nanoseconds(self) -> i64 {
        self.as_nanos()
    }
}
"#;
const CORE_REALTIME_PRELUDE_RAW: &str = include_str!("../Prelude/Core/Realtime.rs");
const CORE_ATOMIC_PRELUDE_RAW: &str = include_str!("../Prelude/Core/Atomic.rs");
const CORE_EMBEDDED_HARDWARE_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/EmbeddedHardware.rs");
const CORE_EMBEDDED_HARDWARE_HOST_BEGIN: &str =
    "// JET_EMBEDDED_HARDWARE_HOST_BEGIN:";
const CORE_FIXED_ARITHMETIC_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/FixedArithmetic.rs");
const CORE_POWER_PRELUDE_RAW: &str = include_str!("../Prelude/Core/Power.rs");
const CORE_DIVISION_PRELUDE_RAW: &str = include_str!("../Prelude/Core/Division.rs");
const PORTABLE_CORE_PRELUDE_RAW: &str = include_str!("../Prelude/PortableCore.rs");
const TARGET_ADAPTERS_PRELUDE_RAW: &str = include_str!("../Prelude/TargetAdapters.rs");
const PORTABLE_ALLOC_PRELUDE_RAW: &str = include_str!("../Prelude/PortableAlloc.rs");
const CORE_LAZY_TABLE_PLAN_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/LazyTablePlan.rs");
const CORE_PARALLEL_PLAN_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/ParallelPlan.rs");
const CORE_GAME_ASSET_PIPELINE_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/GameAssetPipeline.rs");
const CORE_GAME_HOT_SWAP_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/GameHotSwap.rs");
const CORE_GAME_WORLD_INSPECTOR_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/GameWorldInspector.rs");
const CORE_GAME_FRAME_PROFILER_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/GameFrameProfiler.rs");
const CORE_GAME_OVERLAY_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/GameOverlay.rs");
const CORE_GAME_DEV_KERNEL_PRELUDE_RAW: &str =
    include_str!("../../../jet-foundation/src/Game.rs");
const CORE_WEB_PENDING_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/WebPending.rs");
const DEVTOOLS_PANEL_CATALOG_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/DevtoolsPanelCatalog.rs");
const DEVTOOLS_DATABASE_PANEL_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/DevtoolsDatabasePanel.rs");
const DEVTOOLS_JOBS_PANEL_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/DevtoolsJobsPanel.rs");
const DEVTOOLS_REQUEST_PANEL_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/DevtoolsRequestPanel.rs");
const DEVTOOLS_TELEMETRY_PANEL_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/DevtoolsTelemetryPanel.rs");
const DEVTOOLS_TOPOLOGY_PANEL_PRELUDE_RAW: &str =
    include_str!("../Prelude/Core/DevtoolsTopologyPanel.rs");
/// c-devserver (owner-directed 2026-07-01): `core.web.devserver` — the
/// configurable `jet dev` server value (`for_app`/`.html`/`.port`/`.serve`).
const DEVSERVER_PRELUDE: &str = include_str!("../Prelude/DevServer.rs");
/// D-WEBAPP1=D: `core.web.app` full-stack application builder.
const APP_PRELUDE: &str = include_str!("../Prelude/App.rs");
const APP_MIDDLEWARE_PRELUDE: &str =
    include_str!("../../../jet-foundation/src/AppMiddleware.rs");
fn push_app_middleware_prelude(out: &mut String) {
    out.push_str("mod jet_app_middleware {\n");
    out.push_str(APP_MIDDLEWARE_PRELUDE);
    out.push_str("\n}\n");
}
const LIVE_LIFECYCLE_PRELUDE: &str = include_str!("../../../jet-foundation/src/LiveLifecycle.rs");
const LIVEQUERY_PRELUDE: &str = include_str!("../Prelude/CoreLib/Top/LiveQuery.rs");
const GAME_ASSETS_IMPORT_PRELUDE_RAW: &str =
    include_str!("../Prelude/CoreLib/Top/GameAssetsImport.rs");
const GAME_ASSETS_RUNTIME_PRELUDE_RAW: &str =
    include_str!("../Prelude/CoreLib/Top/GameAssetsRuntime.rs");
const WEB_ROUTER_PRELUDE_RAW: &str = include_str!("../Prelude/CoreLib/Top/WebRouter.rs");
const OPENAPI_PRELUDE_RAW: &str = include_str!("../Prelude/CoreLib/Top/OpenAPI.rs");
const WEB_QUERY_PRELUDE_RAW: &str = include_str!("../Prelude/CoreLib/Top/WebQuery.rs");
const WEB_FORMS_PRELUDE_RAW: &str = include_str!("../Prelude/CoreLib/Top/WebForms.rs");
const WEB_TABLE_PRELUDE_RAW: &str = include_str!("../Prelude/CoreLib/Top/WebTable.rs");
const WEB_VIRTUAL_PRELUDE_RAW: &str = include_str!("../Prelude/CoreLib/Top/WebVirtual.rs");
const WEB_STORE_PRELUDE_RAW: &str = include_str!("../Prelude/CoreLib/Top/WebStore.rs");
const WEBSERVERFN_PRELUDE_RAW: &str =
    include_str!("../Prelude/CoreLib/Top/WebServerFn.rs");
const ENCODING_TRAITS_PRELUDE_RAW: &str =
    include_str!("../Prelude/CoreLib/Top/EncodingTraits.rs");
/// D-ALLOC1/D-ALLOC-C/D-ALLOC-D (ratified 2026-06-19): allocator runtime helpers.
const MEM_PRELUDE: &str = include_str!("../Prelude/Mem.rs");
/// D-MEM-SENTRY1: the one sentry kernel is shared verbatim with foundation.
const MEM_SENTRY_PRELUDE: &str = include_str!("../../../jet-foundation/src/MemSentry.rs");
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
    let needs_live = core_usage_matches(
        used_core,
        &[
            "app",
            "core.web",
            "core.db",
            "core.sync",
            "core.net.ws",
            "core.http",
            "core.http.client",
            "core.http.server",
            "core.web.browser",
        ],
    );
    let needs_web_suite = core_usage_matches(used_core, &["core.web"]);
    if needs_app_runtime {
        push_app_middleware_prelude(out);
        out.push_str(DEVSERVER_PRELUDE);
        out.push_str(APP_PRELUDE);
    }
    if needs_live {
        out.push_str(LIVEQUERY_PRELUDE);
    }
    if needs_web_suite {
        out.push_str(&flat_prelude_body(CORE_WEB_PENDING_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(WEB_ROUTER_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(OPENAPI_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(WEB_QUERY_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(WEB_FORMS_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(WEB_TABLE_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(WEB_VIRTUAL_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(WEB_STORE_PRELUDE_RAW));
        out.push_str(&flat_prelude_body(WEBSERVERFN_PRELUDE_RAW));
    }
}

fn push_mem_prelude(out: &mut String) {
    push_prelude_dependency_closure(out, &["fixed_allocator"]);
    out.push_str("mod jet_uninit_semantics {\n");
    out.push_str(UNINIT_PRELUDE);
    out.push_str("\n}\n");
    out.push_str("mod jet_mem {\n    mod jet_sentry {\n");
    out.push_str(MEM_SENTRY_PRELUDE);
    out.push_str("\n    }\n");
    out.push_str(MEM_PRELUDE);
    out.push_str("\n}\n");
}

/// D-DEP-GC1=A: one collector source backs jet-rt JIT/dev and emitted AOT code.
const GC_RUNTIME_PRELUDE: &str = include_str!("../../../jet-rt/src/__gc_core.rs");
const GC_HOST_PRELUDE: &str = include_str!("../../../jet-rt/src/__gc_host.rs");
const GC_PORTABLE_PRELUDE: &str = include_str!("../../../jet-rt/src/__gc_portable.rs");

fn push_gc_prelude(out: &mut String) {
    push_gc_prelude_for_target(out, false);
}

fn push_gc_prelude_for_target(out: &mut String, portable: bool) {
    out.push_str("mod jet_gc {\nmod __gc_platform {\n");
    out.push_str(if portable { GC_PORTABLE_PRELUDE } else { GC_HOST_PRELUDE });
    out.push_str("\n}\nuse __gc_platform::*;\n");
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

const SERVICE_AUTHORITY_PRELUDE_RAW: &str =
    include_str!("../Prelude/CoreLib/Top/ServiceAuthority.rs");
const JOB_QUEUE_NATIVE_PRELUDE_RAW: &str =
    include_str!("../Prelude/CoreLib/Top/JobQueueNative.rs");

/// Every fragment concatenated into the generated program's ONE flat Rust
/// module that brings its own top-level imports. `FLAT_PRELUDE_IMPORTS` merges
/// them; each fragment is emitted through `flat_prelude_body`.
const FLAT_PRELUDE_SOURCES: [&str; 21] = [
    CORE_ATOMIC_PRELUDE_RAW,
    ENCODING_TRAITS_PRELUDE_RAW,
    SCHEDULER_PRELUDE_RAW,
    STREAM_PRELUDE_RAW,
    CORE_EMBEDDED_HARDWARE_PRELUDE_RAW,
    CORE_LAZY_TABLE_PLAN_PRELUDE_RAW,
    DATA_PLOT_PRELUDE_IMPORTS,
    CORE_PARALLEL_PLAN_PRELUDE_RAW,
    CORE_GAME_FRAME_PROFILER_PRELUDE_RAW,
    SERVICE_AUTHORITY_PRELUDE_RAW,
    JOB_QUEUE_NATIVE_PRELUDE_RAW,
    GAME_ASSETS_IMPORT_PRELUDE_RAW,
    GAME_ASSETS_RUNTIME_PRELUDE_RAW,
    WEB_ROUTER_PRELUDE_RAW,
    OPENAPI_PRELUDE_RAW,
    WEB_QUERY_PRELUDE_RAW,
    WEB_FORMS_PRELUDE_RAW,
    WEB_TABLE_PRELUDE_RAW,
    WEB_VIRTUAL_PRELUDE_RAW,
    WEB_STORE_PRELUDE_RAW,
    WEBSERVERFN_PRELUDE_RAW,
];

/// A fragment's leading top-level `use` items, split from the rest of its
/// source. Only the leading block, and only plain `use`: a fragment's later
/// `pub use jet_devserver_impl::…` re-export is part of its API and stays put.
fn split_leading_top_level_uses(source: &str) -> (Vec<&str>, String) {
    let mut imports = Vec::new();
    let mut body = String::with_capacity(source.len());
    let mut rest = source;
    while !rest.is_empty() {
        let line_end = rest.find('\n').map_or(rest.len(), |index| index + 1);
        let line = &rest[..line_end];
        let trimmed = line.trim();
        // Header comments and inner attributes precede the import block.
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
            body.push_str(line);
            rest = &rest[line_end..];
            continue;
        }
        if !line.starts_with("use ") {
            break;
        }
        let mut end = line_end;
        while !rest[..end].contains(';') && end < rest.len() {
            end += rest[end..]
                .find('\n')
                .map_or(rest.len() - end, |index| index + 1);
        }
        imports.push(rest[..end].trim());
        rest = &rest[end..];
    }
    body.push_str(rest);
    (imports, body)
}
fn data_plot_prelude_body() -> String {
    DATA_PLOT_PRELUDE_RAW
        .replace(
            "use std::collections::{BTreeMap, BTreeSet};\nuse std::sync::Arc as JetDataPlotArc;\n",
            "",
        )
}

/// One flat fragment's source as the assembler emits it: its own imports removed,
/// because `FLAT_PRELUDE_IMPORTS` declares them for the whole module instead.
/// DataQuery.rs include!s SqlQuery.rs relative to the Prelude tree. AOT pastes
/// that source into build/.work.*, where the nested include cannot resolve.
/// Inline the kernel so rustc never sees a Prelude-relative path.
fn data_query_prelude_source() -> String {
    include_str!("../Prelude/CoreLib/Top/DataQuery.rs").replace(
        "    include!(\"../../Core/SqlQuery.rs\");\n",
        include_str!("../Prelude/Core/SqlQuery.rs"),
    )
}

fn flat_prelude_body(source: &str) -> String {
    split_leading_top_level_uses(source).1
}

// Reached only from `Codegen::MIRRust`; kept while its printer body is restored.
#[allow(dead_code)]
fn portable_embedded_hardware_source() -> &'static str {
    CORE_EMBEDDED_HARDWARE_PRELUDE_RAW
        .split_once(CORE_EMBEDDED_HARDWARE_HOST_BEGIN)
        .map(|(portable, _)| portable)
        .unwrap_or_else(|| panic!("embedded hardware host bridge marker is missing"))
}

// Reached only from `Codegen::MIRRust`; kept while its printer body is restored.
#[allow(dead_code)]
fn push_portable_prelude_imports(
    out: &mut String,
    include_atomic: bool,
    include_hardware: bool,
    include_alloc: bool,
) {
    let mut sources = vec![PORTABLE_CORE_PRELUDE_RAW, TARGET_ADAPTERS_PRELUDE_RAW];
    if include_atomic {
        sources.push(CORE_ATOMIC_PRELUDE_RAW);
    }
    if include_hardware {
        sources.push(portable_embedded_hardware_source());
    }
    if include_alloc {
        sources.push(PORTABLE_ALLOC_PRELUDE_RAW);
    }
    out.push_str(&merge_flat_prelude_imports(&sources));
}

/// One binding a fragment's leading top-level `use` brings into the flat scope:
/// `use path::{name as binding}`. `binding == name` without `as`.
struct FlatPreludeImport<'a> {
    path: &'a str,
    name: &'a str,
    binding: &'a str,
}

impl FlatPreludeImport<'_> {
    /// Two spellings of one item: `core`/`alloc` paths are `std` re-exports, so
    /// a `no_std`-capable fragment and a hosted one may import the same item
    /// under either root without binding one name to two items.
    fn identity(&self) -> (String, &str) {
        let path = self
            .path
            .strip_prefix("core::")
            .or_else(|| self.path.strip_prefix("alloc::"))
            .map_or_else(|| self.path.to_string(), |rest| format!("std::{rest}"));
        (path, self.name)
    }
}

/// The bindings a fragment's leading top-level `use` items bring in. One
/// parser backs the hosted and the portable import merge (I8).
fn flat_prelude_import_bindings(source: &str) -> Vec<FlatPreludeImport<'_>> {
    let mut bindings = Vec::new();
    for statement in split_leading_top_level_uses(source).0 {
        let item = statement
            .trim_start_matches("use ")
            .trim_end_matches(';')
            .trim();
        let (path, names) = match item.split_once("::{") {
            Some((path, names)) => (path.trim(), names.trim_end_matches('}')),
            None => item.rsplit_once("::").unwrap_or(("", item)),
        };
        for name in names.split(',') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let (name, binding) = match name.split_once(" as ") {
                Some((name, binding)) => (name.trim(), binding.trim()),
                None => (name, name),
            };
            bindings.push(FlatPreludeImport { path, name, binding });
        }
    }
    bindings
}

/// Merge the leading imports of every flat fragment into one import block for
/// the crate root. Keyed by the *binding* each import creates, because that is
/// what rustc rejects twice (E0252): two fragments may import one item under
/// one name through different spellings, but never two items under one name.
///
/// `jet_foundation::<Module>` imports go through `FOUNDATION_PLACEMENTS`: the
/// module must be one the program embeds, and an unaliased import of a
/// root-flat module's item is already satisfied by flattening (at the root it
/// would be a self-import, E0255), so it is omitted; every other spelling
/// resolves through the `jet_foundation` facade.
fn merge_flat_prelude_imports(sources: &[&str]) -> String {
    let mut bindings: BTreeMap<&str, FlatPreludeImport<'_>> = BTreeMap::new();
    for source in sources {
        for import in flat_prelude_import_bindings(source) {
            if let Some(module) = import
                .path
                .strip_prefix("jet_foundation::")
                .map(|rest| rest.split("::").next().unwrap_or(rest))
            {
                match foundation_placement(module) {
                    Some(FoundationPlacement::Root) if import.binding == import.name => continue,
                    Some(_) => {}
                    None => panic!(
                        "flat Prelude fragment imports `jet_foundation::{module}`, which the \
                         generated program does not embed; add its placement to \
                         FOUNDATION_PLACEMENTS"
                    ),
                }
            }
            match bindings.entry(import.binding) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(import);
                }
                std::collections::btree_map::Entry::Occupied(slot) => {
                    let kept = slot.get();
                    assert!(
                        kept.identity() == import.identity(),
                        "flat Prelude fragments bind `{}` to two items: `{}::{}` and `{}::{}`",
                        import.binding,
                        kept.path,
                        kept.name,
                        import.path,
                        import.name
                    );
                }
            }
        }
    }
    let mut groups: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for import in bindings.values() {
        let spelling = if import.binding == import.name {
            import.name.to_string()
        } else {
            format!("{} as {}", import.name, import.binding)
        };
        groups.entry(import.path).or_default().push(spelling);
    }
    let mut rendered = String::new();
    for (path, names) in groups {
        if let [only] = names.as_slice() {
            rendered.push_str(&format!("use {path}::{only};\n"));
        } else {
            rendered.push_str(&format!("use {path}::{{{}}};\n", names.join(", ")));
        }
    }
    rendered.push('\n');
    rendered
}


/// The generated program is ONE flat Rust module assembled from many Prelude
/// fragments, but each fragment is also a real Rust file compiled inside a
/// module of its own crate — `Prelude/Scheduler.rs` as `jet_codegen::scheduler`,
/// `Prelude/CoreLib/Top/ServiceAuthority.rs` inside `jet-comptime`'s
/// `Comptime/ServicesLite.rs` — so a fragment must keep its own imports there.
///
/// Concatenated, those per-fragment imports land in one scope, and two fragments
/// importing one name make rustc reject the generated file: Scheduler.rs and
/// ServiceAuthority.rs both imported `std::time::Duration`, so every program
/// using `core.service` failed to build with E0252 — an internal compiler
/// error (I2), never a user diagnostic.
///
/// Neither per-fragment workaround closes that class. Qualifying paths inline
/// cannot work for trait imports, because method resolution needs the trait in
/// scope (Scheduler.rs calls `TcpStream::write`). Wrapping these fragments in a
/// private module the way `jet_gc`/`jet_sync` are wrapped cannot work either,
/// because generated code calls their private items flat — `TIR/emit/core_calls.rs`
/// emits 24 private `jet_services_*` calls and `Context.rs` three private types,
/// and a glob re-export carries only `pub` ones.
///
/// So the assembler owns the flat module's import set instead (I8). It is
/// derived, not restated: add an import to any flat fragment and it is merged
/// here automatically, so no second collision can be introduced by hand.
/// Emitted unconditionally with the scheduler, which every embedded runtime
/// carries — the harness carries `#![allow(warnings)]`, so an import a program
/// never uses costs nothing, and no user name can shadow one because every
/// emitted user type goes through `mangle_path` (`__jet_` prefix).
static FLAT_PRELUDE_IMPORTS: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| merge_flat_prelude_imports(&FLAT_PRELUDE_SOURCES));

/// `Prelude/CoreLib/Top/ServiceAuthority.rs` as the assembler emits it.
static SERVICE_AUTHORITY_PRELUDE: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| flat_prelude_body(SERVICE_AUTHORITY_PRELUDE_RAW));

fn service_authority_prelude_for_emit() -> &'static str {
    SERVICE_AUTHORITY_PRELUDE.as_str()
}

/// The flat import set, then `Prelude/Scheduler.rs` + `Prelude/Stream.rs` bodies.
fn assemble_scheduler_prelude(scheduler: String) -> String {
    let stream = flat_prelude_body(STREAM_PRELUDE_RAW);
    let mut source =
        String::with_capacity(FLAT_PRELUDE_IMPORTS.len() + scheduler.len() + stream.len());
    source.push_str(FLAT_PRELUDE_IMPORTS.as_str());
    source.push_str(&scheduler);
    source.push_str(&stream);
    source
}

static SCHEDULER_PRELUDE_NATIVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    assemble_scheduler_prelude(
        flat_prelude_body(SCHEDULER_PRELUDE_RAW).replace("feature = \"jet_native_io\"", "all()"),
    )
});

static SCHEDULER_PRELUDE_PORTABLE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    assemble_scheduler_prelude(["notify", "epoll", "kqueue", "iocp"].into_iter().fold(
        flat_prelude_body(SCHEDULER_PRELUDE_RAW),
        strip_scheduler_region,
    ))
});

fn scheduler_prelude_for_emit(native_io: bool) -> &'static str {
    if native_io {
        SCHEDULER_PRELUDE_NATIVE.as_str()
    } else {
        SCHEDULER_PRELUDE_PORTABLE.as_str()
    }
}

fn uses_native_scheduler_for(used_core: &std::collections::HashSet<String>) -> bool {
    used_core.iter().any(|usage| {
        [
            "core.tasks",
            "core.net",
            "core.http",
            "core.process",
            "core.files",
            "core.watcher",
            "core.term",
            "core.sys",
            "core.sys",
            "core.args",
            "core.testing",
            "core.perf",
            "core.mem.scope",
        ]
        .iter()
        .any(|module| {
            usage.strip_prefix(module).is_some_and(|suffix| {
                suffix.is_empty() || suffix.starts_with("::") || suffix.starts_with('.')
            })
        })
    })
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sema::CompileMode;

    #[test]
    fn aot_ffi_host_installs_bridge_reporter() {
        let link = FfiLink {
            crate_name: "jet_ffi_fixture".into(),
            cache_identity: "fixture-identity".into(),
            provenance_path: "fixture.provenance".into(),
            rlib_path: "fixture.rlib".into(),
            cdylib_path: "fixture.so".into(),
            target_deps_dir: "deps".into(),
            host_deps_dir: "deps".into(),
            helper_bin_path: None,
            secrets_helper_bin_path: None,
            handle_facts: vec![],
            link_closure: crate::AST::FfiLinkClosure::default(),
        };
        let mut source = String::new();
        push_ffi_reporter(&mut source, Some(&link));
        assert!(source.contains("jet_ffi_fixture::jet_ffi_set_reporter(jet_ffi_reporter)"));
        // a1dec295b ("feat(failure): centralize entry exit handling") deleted this
        // hook's `eprintln!` copy of the panic message. The single owner is the FFI
        // shim's `ffi_panic` (jet-pkg-model/src/FFI.rs), and that crate's own test
        // forbids a second `eprintln!` of the text (I8: one error channel). So the
        // generated hook must exist, stay vetted-unsafe bracketed, consume its
        // arguments, and NOT re-carry the message.
        assert!(source.contains("// JET_VETTED_UNSAFE_BEGIN: ffi_reporter"));
        assert!(source.contains("// JET_VETTED_UNSAFE_END: ffi_reporter"));
        assert!(source.contains(concat!(
            "extern \"C\" fn jet_ffi_reporter(message: *const u8, len: usize) {\n",
            "    let _ = (message, len);\n",
            "}\n"
        )));
        assert!(!source.contains("panic: a foreign function panicked"));

        let mut cached = String::new();
        push_cached_runtime_with_policy(
            &mut cached,
            Some(&link),
            &ReleaseDevtoolsPolicy::development(),
        );
        let reporter = cached
            .find("jet_ffi_fixture::jet_ffi_set_reporter(jet_ffi_reporter)")
            .unwrap();
        let marker = cached.find(CACHED_RUNTIME_BEGIN).unwrap();
        assert!(reporter < marker);
        assert!(!cached[marker..].contains("jet_ffi_fixture::jet_ffi_set_reporter"));
    }
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    fn core_fingerprint(used_core: &HashSet<String>, test_harness: bool) -> String {
        let mut bundle = checked_generic_bundle("fn run() {}", "core-fingerprint");
        bundle.used_core = used_core.clone();
        corelib_emission_fingerprint(&bundle, test_harness)
    }


    #[test]
    fn relevant_stdlib_digest_is_exact_and_core_closure_sensitive() {
        let mut runtime_source = String::new();
        push_cached_runtime_with_policy(
            &mut runtime_source,
            None,
            &ReleaseDevtoolsPolicy::development(),
        );
        assert_eq!(
            cached_runtime_fingerprint(),
            crate::SHA256::sha256_hex(runtime_source.as_bytes()),
            "native cache identity must hash the emitted fixed runtime"
        );
        assert!(
            !runtime_source.contains("__JET_PACKAGE_EDITION"),
            "package-specific edition data must not poison the fixed runtime object"
        );

        let files = HashSet::from(["core.files::read".to_string()]);
        let net = HashSet::from(["core.net::tcp_connect".to_string()]);
        assert_ne!(
            core_fingerprint(&files, false),
            core_fingerprint(&net, false),
            "native cache identity must include the relevant Core closure"
        );
        assert_eq!(
            core_fingerprint(&files, false),
            core_fingerprint(&files, false),
            "the same relevant Core closure must reuse one stable digest"
        );
    }

    #[test]
    fn core_digest_covers_the_complete_emitted_core_block() {
        let files = HashSet::from(["core.files::read".to_string()]);
        let mut bundle = checked_generic_bundle("fn run() {}", "core-fingerprint-block");
        bundle.used_core = files.clone();
        let body = core_runtime_body(&bundle, false);
        let mut emitted = String::new();
        if !body.is_empty() {
            emitted.push_str(CACHED_CORE_BEGIN);
            emitted.push_str(&body);
            emitted.push_str(CACHED_CORE_END);
        }
        assert!(emitted.contains(scheduler_prelude_for_emit(true)));
        assert!(body.contains("__JET_PACKAGE_EDITION"));
        assert!(body.contains("fn jet_deadline_exceeded"));
        assert!(body.contains("fn jet_seeded_rng_int"));
        assert!(emitted.contains(body.as_str()));
        assert_eq!(
            corelib_emission_fingerprint(&bundle, false),
            corelib_emission_identity(&body, &files),
            "the relevant digest must hash the exact Core body that gets emitted"
        );

        let testing = HashSet::from(["core.testing".to_string()]);
        assert_ne!(
            core_fingerprint(&testing, false),
            core_fingerprint(&testing, true),
            "test-harness Core emission must have its own digest"
        );
    }

    #[test]
    fn r10_corelib_emits_only_reachable_top_modules() {
        // R10 / #1133: a files-only program must not drag HTTP/Browser/Game
        // templates into generated source.
        let files_only = HashSet::from(["core.files::read".to_string()]);
        let mut files_out = String::new();
        push_corelib_prelude_with_policy(
            &mut files_out,
            &files_only,
            false,
            &ReleaseDevtoolsPolicy::development(),
        );
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
        push_corelib_prelude_with_policy(
            &mut net_out,
            &net_only,
            false,
            &ReleaseDevtoolsPolicy::development(),
        );
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
        push_corelib_prelude_with_policy(
            &mut http_out,
            &http,
            false,
            &ReleaseDevtoolsPolicy::development(),
        );
        assert!(
            http_out.contains("JetHTTPServer") || http_out.contains("fn jet_http_"),
            "http usage must emit HTTP templates"
        );

        let files_fp = core_fingerprint(&files_only, false);
        let net_fp = core_fingerprint(&net_only, false);
        assert_ne!(
            files_fp, net_fp,
            "R10 cache identity must differ when Top-module reachability differs"
        );
        assert_eq!(
            files_fp,
            core_fingerprint(&files_only, false),
            "R10 fingerprint must be stable for the same used_core set"
        );

        let compute_only = HashSet::from(["core.compute::zeros".to_string()]);
        let mut compute_out = String::new();
        push_corelib_prelude_with_policy(
            &mut compute_out,
            &compute_only,
            false,
            &ReleaseDevtoolsPolicy::development(),
        );
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
    fn simd_core_marker_selects_shared_lane_closure_without_selecting_plain_programs() {
        let mut plain = String::new();
        push_corelib_prelude_with_policy(
            &mut plain,
            &HashSet::new(),
            false,
            &ReleaseDevtoolsPolicy::development(),
        );
        assert!(
            plain.is_empty(),
            "a program with no Core reachability must not emit the SIMD closure"
        );

        let simd = HashSet::from(["core.math::__mathtypes__".to_string()]);
        let mut simd_out = String::new();
        push_corelib_prelude_with_policy(
            &mut simd_out,
            &simd,
            false,
            &ReleaseDevtoolsPolicy::development(),
        );
        assert!(
            simd_out.contains("fn jet_scalar_loop_barrier"),
            "SIMD reachability must emit the shared scalar-loop barrier"
        );
        assert!(
            simd_out.contains("jet_lane_type!(F32x8"),
            "SIMD reachability must emit the lane type closure"
        );
    }

    /// #1451: the `suite.run()` / `suite.result` handle ops are emitted
    /// unqualified at crate root and are not gated on `used_core` — a
    /// `fn test(suite: TestSuite)` body reaches them with no `core.testing`
    /// call anywhere. Every root adapter those ops can name must therefore be
    /// emitted beside the `Prelude/CommandSuite.rs` kernel part that defines
    /// its callee, for any Core-reaching program. Re-gating them reintroduces
    /// rustc E0425 on generated code (I2).
    #[test]
    fn command_suite_root_adapters_are_never_gated() {
        // Deliberately a `used_core` set with no testing, args, process or
        // files surface: that is what `--show-default` compiles for a file
        // whose only suite contact is an unused `fn test(suite: TestSuite)`.
        let no_testing = HashSet::from(["core.compute::zeros".to_string()]);
        let mut out = String::new();
        push_corelib_prelude_with_policy(
            &mut out,
            &no_testing,
            false,
            &ReleaseDevtoolsPolicy::development(),
        );
        // The whole root fragment, not a symbol list: the `jet_std` kernel
        // defines same-named `pub fn`s, so a name check would pass even with
        // the root adapters gated away.
        assert!(
            out.contains(include_str!("../Prelude/CoreLib/Top/CommandSuite.rs")),
            "the crate-root suite adapters must ship with the kernel part that \
             defines their callees; the handle-op call site names them with no \
             `used_core` guard"
        );
    }








    fn checked_generic_bundle(src: &str, root: &str) -> crate::AST::ProgramBundle {
        checked_generic_bundle_with_facts(src, root).0
    }

    fn checked_generic_bundle_with_facts(
        src: &str,
        root: &str,
    ) -> (crate::AST::ProgramBundle, crate::Sema::SemIndexEffectFacts) {
        let (tokens, lex) = crate::Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        let mut program = crate::Parser::parse(&tokens).expect("parse");
        let mut bundle = crate::AST::ProgramBundle {
            entry: 0,
            project_root: PathBuf::from(root),
            modules: vec![crate::AST::LoadedModule {
                path: PathBuf::from(root).join("main.jet"),
                display: "main.jet".into(),
                source: src.into(),
                alias: "main".into(),
                imports: std::mem::take(&mut program.imports),
                items: std::mem::take(&mut program.items),
                script_body: std::mem::take(&mut program.script_body),
                block_spans: std::mem::take(&mut program.block_spans),
                web_target_ceiling: program.web_target_ceiling,
                pub_file: program.pub_file,
                no_prelude: program.no_prelude,
                default_target: program.default_target,
                html_path: program.html_path,
                policy_declarations: program.policy_declarations.clone(),
                user_policy_declarations: program.user_policy_declarations.clone(),
                rule_facts: std::mem::take(&mut program.rule_facts),
            }],
            devtools_registry: crate::AST::DevtoolsRegistry::default(),
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
            package_guarantees: Default::default(),
            program_allocator: Default::default(),
            active_os: crate::Syntax::OSTarget::host(),
            build_facts: Default::default(),
            edition: "2027".to_string(),
        };
        let (diagnostics, facts) =
            crate::Sema::check_bundle_with_effect_facts(&mut bundle, CompileMode::Run);
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == crate::Diagnostics::Severity::Error),
            "{diagnostics:#?}"
        );
        (bundle, facts)
    }
}




/// D-UIDEVSHELL1=A (c134 Phase 8): true when the native GTK4 backend prelude
/// should be emitted — the program constructs `core.ui.gtk_backend()` AND the
/// active target OS is Linux. `used_core` is collected before `@if
/// @build.os` folds, so a Linux-only backend used under a `.Linux` arm still
/// shows up on a macOS/Windows build; the `active_os` gate is what actually
/// keeps the gtk `extern "C"` surface out of a non-Linux target (the backend is
/// Linux-only). The `.Linux` dispatch arm's construction is likewise folded out
/// of `main` on those targets, so nothing references it.
fn uses_gtk_backend_for(
    used_core: &std::collections::HashSet<String>,
    active_os: Syntax::OSTarget,
) -> bool {
    active_os == Syntax::OSTarget::Linux && used_core.iter().any(|u| u == "core.ui::gtk_backend")
}

/// `Prelude/CoreLib/Top/EncodingCodecs.rs` reads the package edition as
/// `__JET_PACKAGE_EDITION`, and the CoreLib kernel is emitted whenever a program
/// mentions any default `Int`. Every harness therefore needs the constant, so one
/// helper emits it: three hand-written copies had already drifted, leaving the
/// fuzz harness with the three uses and no definition (rustc E0425 on generated
/// code, an I2 violation). Unconditional on purpose — the harness carries
/// `#![allow(warnings)]`, so an unused const costs nothing, and a definition that
/// is never gated cannot drift from the use guard again.
fn push_package_edition_value(out: &mut String, edition: &str) {
    let edition_year = edition.parse::<u16>().unwrap_or(2027);
    out.push_str(&format!(
        "const __JET_PACKAGE_EDITION: u16 = {edition_year};\n\n"
    ));
}

fn push_package_edition(out: &mut String, bundle: &ProgramBundle) {
    push_package_edition_value(out, &bundle.edition);
}
