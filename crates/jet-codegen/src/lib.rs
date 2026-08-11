#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams. Sema transitively includes Parser/Lexer/Comptime/Foundation.
pub use jet_sema::{
    CanonicalAST, Collections, Comptime, Diagnostics, Formatter, Generics, Lexer, Parser, Sema,
    Syntax, TargetMachine, Traits, AST, SHA256,
};
pub mod Codegen;
mod BrowserHost;
/// D-ASYNCRT1=A: the one scheduler. AOT embeds `Prelude/Scheduler.rs` into the
/// generated program; this module compiles that same source for the Cranelift
/// JIT and the interpreter's ambient host, so no tier can drift (I9).
/// `SchedulerHost.rs` adds sibling-prelude bindings and marshalling only.
///
/// The emitted program receives these files concatenated into one flat module,
/// so the in-crate copy keeps them flat too.
#[allow(dead_code)] // the emitted-program half of this source has no in-crate caller
pub mod scheduler {
    // Emitted programs carry `Prelude/TaskGroup.rs` as `mod jet_std`; in-crate
    // it is `crate::task_group`. Same source either way.
    use crate::task_group as jet_std;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::{JetOutcome, JetTaskFailure};
    include!("Prelude/Deadline.rs");
    include!("SchedulerHost.rs");
    include!("Prelude/Scheduler.rs");
    include!("Prelude/Stream.rs");
    include!("Prelude/Observe.rs");
}
/// `Prelude/Scheduler.rs` calls `crate::jet_task_control_trace`. An emitted
/// program gets it from the flat `StructuralDebug.rs` prelude; this crate gets
/// it from the same file compiled as a seam dependency.
pub(crate) use jet_foundation::StructuralDebug::jet_task_control_trace;
/// D-LOCALCELL1=A: canonical local Cell runtime shared by emitted AOT code and
/// the TIR evaluator's deopt adapter.
pub mod local_cell {
    // The carrier is one type across every tier; the AOT copy of this file gets
    // it from the flat Prelude, this copy from jet-foundation.
    include!("Prelude/LocalCell.rs");
    // Items are order-independent; the import trails the include so a file that
    // opens with an inner doc comment still compiles as a module.
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
}
/// D-NUMWIDEN-CROSS1=E: one checked integer-to-float widening policy shared
/// by AOT emission, TIR evaluation, and the resident JIT adapter.
pub mod numeric_widen {
    include!("Prelude/NumericWiden.rs");
    // Items are order-independent; the import trails the include so a file that
    // opens with an inner doc comment still compiles as a module.
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
}
/// D-TASKGROUP-PARAM1=A: canonical structured task ownership policy. The JIT
/// compiles the same Prelude source that AOT embeds.
pub mod task_group {
    include!("Prelude/TaskGroup.rs");
    // Items are order-independent; the import trails the include so a file that
    // opens with an inner doc comment still compiles as a module.
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
}
/// D-TYPEDTEXT1=D: typed SQL/HTML/Sh semantics shared by AOT and TIR.
pub mod typed_text {
    include!("Prelude/TypedText.rs");
    // Items are order-independent; the import trails the include so a file that
    // opens with an inner doc comment still compiles as a module.
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
}
/// Card #1751: the one 80x24 terminal default, read by both AOT's
/// `TerminalPolicy::default` and this crate's `PtyConfig::default`.
#[path = "Prelude/TerminalDefault.rs"]
pub mod terminal_default;
/// D-PROCESS-SESSION1=A / #1181: shared native Unix PTY substrate used by the
/// emitted process prelude and the resident JIT adapter.
#[path = "Prelude/CoreLib/ProcessPty.rs"]
pub mod process_pty;
// Prelude/ contains include_str-embedded text files, not Rust modules.
