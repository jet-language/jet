//! Jet evaluator host services.
//!
//! The host side of the Source evaluator and the interpreter lens: the typed
//! ambient Core services the evaluator calls (`ambient_interp` and the
//! per-domain `register_*_interpreter_ambient` adapters), the Source entry
//! handoff and its resource/callback/interface/shared-interop owners used by
//! the bootstrap Runner, and the job, migration, and console host seams.

// This crate includes shared Prelude source that several hosts compile, each
// using a different subset, so dead-code reports here are about the other
// hosts' usage, not about this one.
#![expect(
    dead_code,
    reason = "#804: shared Prelude host adapters are compiled by multiple hosts"
)]
#![expect(
    non_snake_case,
    reason = "#804: shared Prelude symbols retain canonical Jet names"
)]

/// Canonical devtools event records are shared by every host, including the
/// generated job Prelude. Keep the names at the crate root so that an included
/// Prelude sees the same types as the compiler/comptime host.
pub use jet_foundation::Devtools::{JetDevtoolsEvent, JET_DEVTOOLS_MAX_HISTORY};
pub use jet_foundation::Dyn;
/// Canonical outcome carriers used by the included Foundation encoding helpers.
pub use jet_foundation::Outcome;

/// D-JOB-SUBCMD1=C: the evaluator imports the same Prelude selector as
/// generated AOT mains. Its only extra work is mapping the selected name onto
/// the already checked MIR entry.
pub mod Job {
    include!("../../jet-codegen/src/Prelude/JobQueueTypes.rs");
    include!("../../jet-codegen/src/Prelude/Job.rs");
}

/// D-DEVR-LAW1=A / I9: the same development-act receipt the AOT prelude embeds.
pub mod development_receipt {
    include!("../../jet-codegen/src/Prelude/DevelopmentReceipt.rs");
}


/// The dev watcher sleeps through the same Prelude TimerWheel as language
/// timers. This is only the host adapter; timer ownership stays in
/// `Prelude/Scheduler.rs`.
pub fn scheduler_sleep_ms(millis: u64) {
    jet_codegen::scheduler::jet_scheduler_sleep_ms(millis);
}

#[allow(unused_imports)]
pub(crate) use jet_foundation::EncodingErrors as jet_encoding_errors;
// Included Foundation and Prelude sources name the exact-Int carrier through `crate::Numeric`.
pub(crate) use jet_foundation::Numeric;
pub(crate) mod jet_json_number {
    include!("../../jet-foundation/src/JSONNumber.rs");
}
pub(crate) mod jet_encoding_json {
    include!("../../jet-foundation/src/EncodingJson.rs");
}
mod ambient_interp;
mod CLI;
/// D-DX-JOBGRAPH1=A: the host job-graph adapter validates root job argv with
/// the same parser the evaluator entry decodes with.
pub use CLI::validate_entry_argv;
mod Concurrency;
mod Crypto;
mod DB;

pub fn install_job_queue_provider() {
    DB::jit_job_queue_install_provider();
}

mod Receipt;
mod Plugin;
/// The evaluator always supplies checked authority-needs metadata through its
/// hidden plugin-load argument. This root symbol only satisfies the shared
/// Prelude's AOT default entry and fails closed if that path is used.
fn jet_plugin_declared_authority_needs() -> Result<Vec<String>, String> {
    Err("plugin load requires compiler-supplied declared authority needs".to_string())
}
mod Ffi;
mod Mod;
mod ProcessPrelude;
/// Invocation-scoped Source resource owners and the native loop producer
/// bridge.  The module is public so the Foundation Source bootstrap can use
/// the typed capability boundary without reaching integer handle tables.
pub mod SourceResources;
/// Private physical transport for compiler-implementation `Shared<T>` roots.
/// This is distinct from the Source evaluator's logical `Shared(index)` values.
pub mod SourceSharedInterop;
/// Session-local generic transport for retained Source callback payloads.
/// Semantic evaluation and logical Source/Eval state remain owned by the
/// generated compiler callback pump.
pub mod SourceCallbacks;
/// Invocation-scoped native implementations of private MIR traits.
pub mod SourceInterfaces;
pub use SourceCallbacks::{
    SourceCallbackJob, SourceCallbackJobDrainOutcome, SourceCallbackJobOwner,
    SourceCallbackJobSessionRegistration,
    SourceCallbackAbandonedEvent, SourceCallbackAbandonment, SourceCallbackCleanup,
    SourceCallbackDrainStatus, SourceCallbackEnqueueError, SourceCallbackError,
    SourceCallbackEvent, SourceCallbackId, SourceCallbackInvocation, SourceCallbackLease,
    SourceCallbackRegisterError, SourceCallbackRelease, SourceCallbackReply,
    SourceCallbackReplyCleanup, SourceCallbackReplyError, SourceCallbackRequestId,
    SourceCallbackResponder, SourceCallbackRetireError, SourceCallbackSession,
};
mod IO;
mod Reactive;
mod net_http_rt;
mod CoreHost;
mod Memory;
mod enc_stream;
mod Web;
mod Encoding;
mod Collections;
mod Text;
mod Time;
mod host_seam;
pub(crate) use jet_codegen::fault_injection;
mod Process;
mod Raylib;
mod testing_shared {
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/TestingShared.rs");
}
mod Ui;

pub use DB::{
    inspect_migrations, migration_request_checksum, run_migration, MigrationLock,
    MigrationOperation, MigrationOutcome, MigrationRequest, MigrationSql, MigrationState,
    MigrationStateRequest, MIGRATION_SCHEMA_IDENTITY,
};

pub use ambient_interp::{
    register_hardware_interpreter_ambient, with_interpreter_ambient, InterpreterAmbientContext,
};

pub fn register_encoding_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Encoding::register_interpreter_ambient(context);
}

pub fn register_receipt_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Receipt::register_interpreter_ambient(context);
}

pub fn register_db_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    DB::register_interpreter_ambient(context);
}

pub fn register_ui_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Ui::register_interpreter_ambient(context);
}

pub fn register_raylib_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Raylib::register_interpreter_ambient(context);
}

pub fn register_plugin_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Plugin::register_interpreter_ambient(context);
}

pub fn register_crypto_interpreter_ambient(context: &mut InterpreterAmbientContext) {
    Crypto::register_interpreter_ambient(context);
}

/// Shared by prelude `include!` fragments that impl `crate::JetShow`.
pub(crate) trait JetShow {
    fn jet_show(&self) -> String;
}

/// Display/debug seams for included Prelude fragments (`impl crate::JetDisplay`).
pub(crate) trait JetDisplay {
    fn jet_display(&self) -> String;
}

pub(crate) trait JetDebug {
    fn jet_debug(&self) -> String;
}

/// Canonical XML pull engine — EncodingStream refers to `crate::jet_xml_pull`.
pub mod jet_xml_pull {
    pub use jet_foundation::XmlPull::*;
}

/// Install argv for one evaluator run (`argv[0]` = entry path, then program args).
pub fn with_program_args<R>(args: &[String], run: impl FnOnce() -> R) -> R {
    // `Comptime::RUNTIME_ARGV` is the one invocation carrier for evaluator
    // runs (#778); storage belongs to the shared Prelude seam.
    jet_codegen::Comptime::with_runtime_argv(args, run)
}

/// Reset process-local Core state before a one-shot evaluator run.
///
/// AOT gets a fresh process. In-process runs call this shared boundary so a
/// prior run cannot become observable.
#[doc(hidden)]
pub fn reset_one_shot_core_state() {
    Crypto::runtime::auth_runtime_reset();
    jet_codegen::Comptime::AuthLite::auth_runtime_reset();
    jet_codegen::Codegen::MIREval::reset_world_shared_sites();
}

/// Release process-local Core owners at the end of a one-shot evaluator run,
/// on the thread that ran it.
///
/// Thread-local tables are otherwise destroyed at thread exit in no fixed
/// order, so an undrained `core.db` pool would close its drivers after the
/// native connection table was already gone.
#[doc(hidden)]
pub fn release_one_shot_core_state() {
    DB::release_db_resources();
}

/// Clear native `Mod` loads created by the current interpreter invocation.
/// The engine exposes no second lifecycle policy: `JetMod::Drop` in the shared
/// Prelude closes the OS handle and removes its staged payload.
pub fn clear_loaded_modules() {
    Mod::clear();
}

/// Run evaluator work on Jet's shared compiler worker. Callers pass a checked
/// MIR program, so this boundary adds only the stack and the program argv.
pub fn on_compiler_stack<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    if jet_foundation::CompilerStack::on_compiler_worker() {
        return work();
    }
    let argv = jet_codegen::Comptime::runtime_argv();
    let out = jet_foundation::CompilerStack::run_on_compiler_stack(move || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match argv {
            Some(args) => jet_codegen::Comptime::with_runtime_argv(&args, work),
            None => work(),
        }))
    });
    out.unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}

mod SourceEntry;
pub use SourceEntry::{
    execute_source_entry, retire_source_entry, SourceEntryError, SourceEntryExecution,
    SourceEntryReply, SourceEntryRequest, SourceExecutionCompletion,
    SourceExecutionCompletionBox, SourceExecutionCompletionDisposition,
    SourceExecutionCompletionRetirement, SourceExecutionCompletionScope,
    SourceExecutionCompletionScopeWeak, SourceExecutionCompletionWriteback,
    SourceExecutionGuard, SourceExecutionRetirement, SourceExecutionRetirementError,
    SourceExecutionRetirementFailureKind,
};
/// Declare that the program about to run owns the process's stdout and stderr,
/// so the evaluator writes its output through as it is produced instead of
/// accumulating it for a caller that reads `RunOutcome` as data. The fact
/// itself, its name, and the routing rule live in `Prelude/Term.rs` (I8/I9).
pub fn set_program_owns_streams() {
    std::env::set_var(IO::term_prelude::JET_TERM_PROGRAM_OWNS_STREAMS, "1");
}

pub use jet_codegen::scheduler::{jet_scheduler_spawn, JetSchedulerJoin, JetTaskFailure};
pub use Ffi::set_bridge_cdylib;
/// Bind the prepared native bridge before interpreter MIR execution.
///
/// The interpreter's canonical extern-call ambient uses the prepared bridge
/// cdylib; this only installs its checked descriptor table and does not
/// provide an alternate foreign implementation.
pub fn bind_interpreter_ffi(
    program: &jet_foundation::MIR::MirProgram,
    artifact: jet_foundation::MIR::MirArtifactId,
) -> Result<(), String> {
    Ffi::bind_mir_ffi(program, artifact).map_err(|error| match error {
        Ffi::BindError::Message(message) => message,
    })
}
