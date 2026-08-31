#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams. Sema transitively includes Parser/Lexer/Comptime/Foundation.
#[allow(unused_imports)]
pub(crate) use jet_foundation::EncodingErrors as jet_encoding_errors;
pub use jet_sema::{
    CanonicalAST, Collections, Comptime, Diagnostics, Formatter, Generics, Lexer, Parser, Policy,
    Sema, Syntax, TargetMachine, Traits, AST, SHA256,
};
// `EncodingJson.rs` resolves exact-number validation through
// `super::jet_json_number`; supply it at the root the same way `jet-jit` does.
#[allow(unused_imports)]
pub(crate) use jet_foundation::JSONNumber as jet_json_number;
#[allow(dead_code)]
pub(crate) mod jet_encoding_json {
    include!("../../jet-foundation/src/EncodingJson.rs");
}
mod BrowserHost;
pub mod Codegen;
/// D-FAIL-BREACH1=A: the same task-local runtime stack kernel used by emitted
/// AOT code. Resident engines marshal source locations into their own report
/// carrier, but depth policy stays in this Prelude part.
pub mod runtime_stack {
    use jet_foundation::Outcome::JET_RUNTIME_STACK_LIMIT;
    include!("Prelude/Core/RuntimeStack.rs");
}
/// D-TESTFAULT1=A: the same fault schedule source used by emitted Prelude
/// code, the TIR evaluator, and the resident JIT adapters.
#[allow(dead_code)]
pub mod fault_injection {
    include!("Prelude/FaultInjection.rs");
}
/// D-ALLOC-PROGRAM1=A: canonical whole-program allocator policy compiled from
/// the same Prelude source that generated AOT programs embed.
pub mod program_allocator {
    include!("Prelude/ProgramAllocator.rs");
}
/// D-DEVR-LAW1=A / I9: the one development receipt record. AOT embeds the
/// same source through `Codegen::PRELUDE_PARTS`; resident engines include it
/// from this seam and only marshal its bytes to their host store.
pub mod development_receipt {
    include!("Prelude/DevelopmentReceipt.rs");
}

#[cfg(test)]
mod development_receipt_tests {
    use super::development_receipt::{
        is_content_address, jet_development_receipt_render, jet_production_failure_receipt_write,
        JetDevelopmentReceipt, JetDevelopmentReceiptInput,
        JET_DEVELOPMENT_RECEIPT_CLOSURE_DIGEST_ENV, JET_DEVELOPMENT_RECEIPT_DIRECTORY_ENV,
        JET_DEVELOPMENT_RECEIPT_ENTRY_ENV, JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_DIGEST_ENV,
        JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_ENV, JET_DEVELOPMENT_RECEIPT_INPUT_DIGEST_ENV,
        JET_DEVELOPMENT_RECEIPT_SOURCE_DIGEST_ENV, JET_DEVELOPMENT_RECEIPT_TARGET_DIGEST_ENV,
        JET_DEVELOPMENT_RECEIPT_TARGET_ENV,
    };

    fn receipt(inputs: Vec<JetDevelopmentReceiptInput>) -> JetDevelopmentReceipt {
        JetDevelopmentReceipt {
            act: "package-realization".into(),
            locked_closure:
                "sha256-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            inputs,
            planned_action:
                "sha256-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
            outputs: vec![JetDevelopmentReceiptInput {
                name: "out".into(),
                digest: "sha256-cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                    .into(),
            }],
            activation_proof: String::new(),
            parent_generation: String::new(),
            witness: "jetpack".into(),
            outcome: "passed".into(),
            failure_path: None,
        }
    }

    #[test]
    fn receipt_identity_is_content_addressed_and_order_stable() {
        let first = receipt(vec![
            JetDevelopmentReceiptInput {
                name: "source".into(),
                digest: "sha256-dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                    .into(),
            },
            JetDevelopmentReceiptInput {
                name: "toolchain".into(),
                digest: "sha256-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                    .into(),
            },
        ]);
        let second = receipt(vec![
            JetDevelopmentReceiptInput {
                name: "toolchain".into(),
                digest: "sha256-eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
                    .into(),
            },
            JetDevelopmentReceiptInput {
                name: "source".into(),
                digest: "sha256-dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
                    .into(),
            },
        ]);
        assert_eq!(first.identity_bytes(), second.identity_bytes());
        assert_eq!(
            jet_development_receipt_render(&first),
            jet_development_receipt_render(&second)
        );
        assert!(is_content_address(&first.locked_closure));
        assert!(is_content_address(&first.inputs[0].digest));
        assert!(!is_content_address("source-v1"));
        assert!(!is_content_address(
            "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ));
    }

    #[test]
    fn development_receipt_is_one_shared_source_across_tiers() {
        let codegen = include_str!("Codegen/mod.rs");
        let jit = include_str!("../../jet-jit/src/lib.rs");
        assert_eq!(
            codegen
                .matches("include_str!(\"../Prelude/DevelopmentReceipt.rs\")")
                .count(),
            1
        );
        assert_eq!(
            jit.matches("include!(\"../../jet-codegen/src/Prelude/DevelopmentReceipt.rs\")")
                .count(),
            1
        );
    }

    #[test]
    fn production_failure_writer_redacts_dynamic_values() {
        let root = std::env::temp_dir().join(format!(
            "jet-production-receipt-unit-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let digest = |byte| format!("sha256-{}", char::from(byte).to_string().repeat(64));
        std::env::set_var(JET_DEVELOPMENT_RECEIPT_DIRECTORY_ENV, &root);
        std::env::set_var(JET_DEVELOPMENT_RECEIPT_ENTRY_ENV, "service.jet");
        std::env::set_var(JET_DEVELOPMENT_RECEIPT_SOURCE_DIGEST_ENV, digest(b'a'));
        std::env::set_var(JET_DEVELOPMENT_RECEIPT_CLOSURE_DIGEST_ENV, digest(b'b'));
        std::env::set_var(JET_DEVELOPMENT_RECEIPT_INPUT_DIGEST_ENV, digest(b'c'));
        std::env::set_var(JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_ENV, "1");
        std::env::set_var(JET_DEVELOPMENT_RECEIPT_INPUT_COUNT_DIGEST_ENV, digest(b'd'));
        std::env::set_var(JET_DEVELOPMENT_RECEIPT_TARGET_ENV, "native");
        std::env::set_var(JET_DEVELOPMENT_RECEIPT_TARGET_DIGEST_ENV, digest(b'e'));

        let path = jet_production_failure_receipt_write(
            "E3001",
            "/outside/USER_DATA_SECRET.jet",
            9,
            "run",
        )
        .unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.starts_with("jet-development-receipt-v1\n"));
        assert!(!text.contains("USER_DATA_SECRET"));
        assert!(!text.contains("argv"));
        assert!(text.contains("failure-path"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
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
    pub use jet_foundation::Outcome::{
        jet_render_runtime_stop, jet_stream_record_failure_report,
        jet_stream_take_failure_report, JetOutcome, JetRuntimeDiagnostic, JetTaskFailure,
    };
    include!("Prelude/Deadline.rs");
    include!("Prelude/WorkflowWait.rs");
    include!("SchedulerHost.rs");
    include!("Prelude/CoreLib/Top/TimeSleep.rs");
    include!("Prelude/Scheduler.rs");
    include!("Prelude/CoreLib/Top/WorkflowSleep.rs");
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
/// D-HOLE1: one option-lift operation shared by AOT, TIR, JIT, and wasm.
pub mod option_lift2 {
    include!("Prelude/Core/Option.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::{JetAbsent, JetOutcome};
}
/// D-FIXARR1: checked fixed-list indexing shared by all execution adapters.
pub mod fixed_list {
    include!("Prelude/Core/FixedList.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::jet_list_bounds_message;
}
/// D-SOA-TIER1=A: THE shared struct-of-arrays column store and the one gather
/// read, shared verbatim with emitted AOT code. The interpreter ambient and the
/// Cranelift host reach a columnar list only through this source, so no engine
/// carries its own layout, row bookkeeping, or bounds policy (I9).
pub mod columns {
    include!("Prelude/Core/Columns.rs");
    // Items are order-independent; the imports trail the include so a file that
    // opens with an inner doc comment still compiles as a module. The read reuses
    // the shared fixed-list bounds stop rather than wording its own.
    #[allow(unused_imports)]
    pub use super::fixed_list::{jet_fixed_list_index, JetFixedListIndexError};
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
/// D-MEMO1=A: the result-cache substrate compiled from the same Prelude source
/// used by generated AOT programs and the TIR interpreter.
pub mod memo {
    include!("Prelude/Memo.rs");
}
/// #2027 (I8 + I9): the one signal-handler mechanism inside the `jet` binary.
/// `Prelude/CoreLib/Top/Interrupt.rs` owns the pending count, the platform
/// handler, the arm path and the consumption rule; AOT embeds that same source
/// in the generated program. This is the single in-binary instance, so the TIR
/// evaluator ambient (`Codegen/TIR/eval/mod.rs`) and the resident Cranelift host
/// (`jet-jit/src/CoreHost.rs`) marshal into one count instead of each compiling
/// a private copy whose `signal(SIGINT, …)` install disarmed the other.
pub mod interrupt_runtime {
    include!("Prelude/CoreLib/Top/Interrupt.rs");
}
/// D-TERM1 / I9: one in-process terminal key kernel. The TIR evaluator and
/// resident JIT both call this module; neither includes a private copy.
#[allow(dead_code)]
pub mod terminal_runtime {
    include!("Prelude/Term.rs");
    include!("Prelude/Core/TermKey.rs");
}
#[cfg(unix)]
pub(crate) use terminal_runtime::jet_term_configure_fd;
/// D-PROCESS-SESSION1=A / #1181: shared native Unix PTY substrate used by the
/// emitted process prelude and the resident JIT adapter.
#[path = "Prelude/CoreLib/ProcessPty.rs"]
pub mod process_pty;
/// Card #1751: the one 80x24 terminal default, read by both AOT's
/// `TerminalPolicy::default` and this crate's `PtyConfig::default`.
#[path = "Prelude/TerminalDefault.rs"]
pub mod terminal_default;
/// D-CMD-OVERRIDE1=C: command-suite values compile from the same Prelude
/// source that AOT embeds in `mod jet_std`.
pub mod command_suite {
    include!("Prelude/CommandSuite.rs");
}

#[cfg(test)]
#[test]
fn unmatched_enum_match_fails_closed() {
    let span = Diagnostics::Span::new(4, 12);
    let diagnostic = Codegen::TIR::unmatched_enum_match_guard(true, span)
        .expect_err("a sema-proved exhaustive match must not fall through");
    assert_eq!(diagnostic.code, "E0956");
    assert_eq!(diagnostic.span, Some(span));
    assert!(diagnostic.what.contains("exhaustive match fallthrough"));
    assert!(Codegen::TIR::unmatched_enum_match_guard(false, span).is_ok());
}
// Prelude/ contains include_str-embedded text files, not Rust modules.
