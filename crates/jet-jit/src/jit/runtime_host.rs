use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Module};
use jet_codegen::scheduler::{
    JetSchedulerChannel, JetSchedulerJoin, JetSchedulerSender, JetStream, JetStreamSender,
    JetTaskControl,
};
use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};

use super::resident::resident_teardown;
use super::{
    Archive, Cell as LocalCell, Collections, Compress, Compute, Concurrency, CoreHost, Crypto,
    Encoding, Fmt, JitResultValue, Memory, Net, Numeric, Process, Random, Solver, Text, Time,
    TRY_COMPILE_PANIC_HOOK_LOCK,
};

pub(crate) mod duration_kernel {
    include!("../../../jet-codegen/src/Prelude/Core/Duration.rs");
}

mod measurement_kernel {
    include!("../../../jet-codegen/src/Prelude/Core/Measurement.rs");
}

pub(crate) mod contract_kernel {
    include!("../../../jet-codegen/src/Prelude/Core/Contracts.rs");
}

pub(crate) mod service_prelude {
    include!("../../../jet-codegen/src/Prelude/Service.rs");
}

thread_local! {
    static STRUCT_NEW_COUNT: Cell<usize> = const { Cell::new(0) };
}

#[doc(hidden)]
pub fn reset_struct_new_count_for_test() {
    STRUCT_NEW_COUNT.with(|count| count.set(0));
}

#[doc(hidden)]
pub fn struct_new_count_for_test() -> usize {
    STRUCT_NEW_COUNT.with(Cell::get)
}

pub(crate) fn catch_jit_panic<R>(context: &str, f: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
    let result = {
        let _guard = TRY_COMPILE_PANIC_HOOK_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let old_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = catch_unwind(AssertUnwindSafe(f));
        std::panic::set_hook(old_hook);
        result
    };
    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            resident_teardown();
            Err(err)
        }
        Err(payload) => {
            resident_teardown();
            let detail = if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else {
                "unknown panic payload".into()
            };
            Err(format!(
                "jit {context} panicked before returning an unsupported reason: {detail}"
            ))
        }
    }
}

/// Live heap carried across type-stable hot_swap (M2). `invocations` counts
/// how many times `main` ran without a clean restart — preserved on swap,
/// reset on restart.
#[derive(Clone)]
pub(crate) struct ReflectSlot {
    /// Present only for a field projection. The same slot is also the typed
    /// Value returned by `Field.value()`; its text is only `display`.
    pub field_name: Option<String>,
    pub type_name: String,
    pub path: String,
    pub display: String,
    pub fields: Vec<(String, i64)>,
}

/// Canonical resident representation for a runtime function value.
/// `fn_ptr` points at a Cranelift function whose ABI is either the plain
/// function signature or that signature with `env` prepended. The handle is
/// deliberately opaque to Prelude code; only the callable adapters inspect it.
#[derive(Clone, Copy)]
pub(crate) struct JitCallableSlot {
    pub fn_ptr: i64,
    pub env: i64,
    pub has_env: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JitZipValueKind {
    Int,
    Float,
    Bool,
    Char,
    String,
    Opaque,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct JitZipColumn {
    pub(crate) input: JitZipValueKind,
    pub(crate) field: JitZipValueKind,
    pub(crate) optional: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct JitZipPlan {
    pub(crate) mode: u8,
    pub(crate) fill_mode: u8,
    pub(crate) columns: Vec<JitZipColumn>,
}

pub(crate) struct JitRuntime {
    pub(crate) source_file: String,
    pub(crate) source_text: String,
    pub(crate) current_function: String,
    pub(crate) stack_depth: usize,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) heap: jet_rt::JetArena,
    pub(crate) compute: Compute::ComputeState,
    /// Compile-time string handles baked into Cranelift as `iconst` ids.
    /// `reset_run_heap` and the run-cache artifact must preserve these — clearing
    /// them leaves warm `jet run` hits with empty panic/require text (I9).
    pub(crate) compile_strings: Vec<(usize, String)>,
    /// Compile-time zip row schemas referenced by resident Cranelift code. The
    /// host receives only handles at run time; this table supplies the checked
    /// column representation and remains stable across heap resets.
    pub(crate) zip_plans: Vec<JitZipPlan>,
    pub(crate) invocations: u64,
    /// D-FIELDMEMO1=A: cached computed-field words keyed by record and the
    /// stable getter slot. The host only stores raw ABI words; the TIR/JIT
    /// lowering owns type packing and the Prelude owns cache policy elsewhere.
    pub(crate) memo_values: std::collections::HashMap<(i64, i64), i64>,
    pub(crate) channels: Vec<JetSchedulerChannel<i64>>,
    pub(crate) senders: Vec<Option<JetSchedulerSender<i64>>>,
    /// Opaque Stream handles. The pull/close/completion law lives in the
    /// shared Prelude; these maps only translate Cranelift i64 handles.
    pub(crate) stream_consumers: std::collections::HashMap<i64, JetStream<i64>>,
    pub(crate) stream_producers:
        std::collections::HashMap<i64, std::sync::Arc<JetStreamSender<i64>>>,
    pub(crate) stream_senders:
        std::collections::HashMap<i64, std::sync::Arc<JetStreamSender<i64>>>,
    pub(crate) next_stream_channel: i64,
    pub(crate) next_stream_sender: i64,
    /// Unique names for JIT-local OptionLift2 factory/adapter functions.
    /// These functions are only ABI thunks; the operation they serve lives in
    /// the shared Option Prelude.
    pub(crate) next_option_lift2_thunk: u64,
    /// Unique names for deferred Shared transaction lambda callbacks.
    pub(crate) next_shared_txn_thunk: u64,
    /// Runtime function values. Negative words are explicit callable handles;
    /// raw Cranelift addresses are normalized at the boundary before a call.
    pub(crate) jit_callables: Vec<JitCallableSlot>,
    /// Process-edge callbacks. The resident adapter invokes these after all
    /// generated scope cleanup and before it returns the run outcome.
    pub(crate) atexit_handlers: Vec<JitCallableSlot>,
    pub(crate) tasks: Vec<Option<JetSchedulerJoin<i64>>>,
    pub(crate) task_controls: Vec<std::sync::Arc<JetTaskControl>>,
    pub(crate) task_groups: Vec<Option<super::Concurrency::JitTaskGroup>>,
    /// D-LOCALCELL1=A: one-thread canonical Cell values and guards.
    pub(crate) cells: LocalCell::CellState,
    /// General `Result<T, E>` ABI arena. Handles are one-based indices; payload
    /// bits are interpreted from checked TIR types, never dynamically guessed.
    pub(crate) results: Vec<JitResultValue>,
    /// D-FAIL-ERROR1=A: Prelude-owned default error values. JIT code sees only
    /// one-based handles and marshals fields through the helpers below.
    pub(crate) errors: Vec<jet_foundation::Outcome::JetErr>,
    pub(crate) solvers: Vec<Solver::SolverState>,
    pub(crate) rngs: Vec<crate::Random::RngState>,
    /// Manual `Clock.new(ms)` handles — 1-based indices into this vec (#729 uuid).
    pub(crate) clocks: Vec<i64>,
    /// `ProcessSpec` handles — 1-based indices (#729 process builder).
    pub(crate) process_specs: Vec<Process::JitProcessSpec>,
    /// `ProcessChild` handles — 1-based indices (#729 process spawn).
    pub(crate) process_children: Vec<Process::JitProcessChild>,
    /// `core.sketch` handles (#729).
    pub(crate) sketches: Vec<crate::Sketch::SketchSlot>,
    /// `core.args` ArgsSpec / ParsedArgs handles (#729).
    pub(crate) args_specs: Vec<crate::Args::ArgsSpec>,
    pub(crate) args_parsed: Vec<crate::Args::ParsedArgs>,
    /// Encoding stream file / codec handles (#729 encoding_*_stream).
    pub(crate) file_readers: Vec<crate::enc_stream::FileReaderSlot>,
    pub(crate) file_writers: Vec<crate::enc_stream::FileWriterSlot>,
    pub(crate) json_readers: Vec<crate::enc_stream::JSONReaderSlot>,
    pub(crate) json_writers: Vec<crate::enc_stream::JSONWriterSlot>,
    pub(crate) jsonl_readers: Vec<crate::enc_stream::JsonlReaderSlot>,
    pub(crate) jsonl_writers: Vec<crate::enc_stream::JsonlWriterSlot>,
    pub(crate) csv_readers: Vec<crate::enc_stream::CSVReaderSlot>,
    pub(crate) csv_writers: Vec<crate::enc_stream::CSVWriterSlot>,
    pub(crate) xml_readers: Vec<crate::enc_stream::XmlReaderSlot>,
    pub(crate) xml_writers: Vec<crate::enc_stream::XmlWriterSlot>,
    pub(crate) cbor_readers: Vec<crate::enc_stream::CBORReaderSlot>,
    pub(crate) cbor_writers: Vec<crate::enc_stream::CBORWriterSlot>,
    /// Typed `core.data` pull streams (`csv_reader` → Event rows).
    pub(crate) data_streams: Vec<crate::Data::DataStreamSlot>,
    /// `Set<T>` handles — 1-based indices (#729 collections/set), with the
    /// parallel kind tag preserving String equality at the host boundary.
    pub(crate) sets: Vec<std::collections::HashSet<i64>>,
    /// Parallel element-kind tags: `true` means String, `false` means Int.
    pub(crate) set_string_kinds: Vec<bool>,
    /// `Deque<T>` handles — 1-based indices (#729 collections/deque). Int elems only.
    pub(crate) deques: Vec<std::collections::VecDeque<i64>>,
    /// `Bag<T>` handles — counted JIT-value bits, keyed by the checked element ABI.
    pub(crate) bags: Vec<std::collections::HashMap<i64, usize>>,
    pub(crate) sorted_sets: Vec<std::collections::BTreeSet<i64>>,
    pub(crate) sorted_set_string_kinds: Vec<bool>,
    pub(crate) priority_queues: Vec<std::collections::BinaryHeap<i64>>,
    pub(crate) lrus: Vec<Collections::LruState>,
    pub(crate) bit_sets: Vec<std::collections::BTreeSet<i64>>,
    pub(crate) byte_buffers: Vec<Collections::byte_buffer_semantics::JetByteBuffer>,
    pub(crate) allocators: Vec<Memory::AllocatorState>,
    pub(crate) pools: Vec<std::sync::Arc<std::sync::Mutex<Memory::PoolState>>>,
    pub(crate) shareds: Vec<std::sync::Arc<Memory::SharedState>>,
    pub(crate) conditions: Vec<std::sync::Arc<Memory::ConditionState>>,
    pub(crate) shared_guard_states: std::collections::HashMap<
        i64,
        std::sync::Arc<Memory::shared_protocol::JetSharedGuardState>,
    >,
    pub(crate) expirings: Vec<Memory::ExpiringState>,
    pub(crate) secrets: Vec<Option<Memory::SecretState>>,
    pub(crate) crypto_values: Vec<Option<Crypto::CryptoValue>>,
    /// `core.url` / `core.mime` / net handles (#1221).
    pub(crate) net_values: Vec<Option<Net::NetValue>>,
    /// `core.services` / `core.sync` opaque Prelude values keyed by their
    /// resident heap-record handle. Entries live for one invocation only.
    pub(crate) service_values: Vec<Option<jet_foundation::AST::CtValue>>,
    /// `core.game` scene / frame / replay / backend handles (#1218).
    pub(crate) game_scenes: Vec<crate::Game::GameSceneState>,
    pub(crate) game_frames: Vec<crate::Game::GameFrameState>,
    pub(crate) game_replays: Vec<crate::Game::GameReplayState>,
    pub(crate) game_backends: Vec<crate::Game::GameBackendState>,
    /// `core.raylib` window / color handles (#1218).
    pub(crate) raylib_windows: Vec<crate::Raylib::RaylibWindowState>,
    pub(crate) raylib_colors: Vec<crate::Raylib::RaylibColorState>,
    pub(crate) raylib_sounds: Vec<crate::Raylib::RaylibSoundState>,
    pub(crate) time_values: Vec<Option<Time::TimeValue>>,
    /// Regex / Match handles for core.regex (#1219).
    pub(crate) regex_values: Vec<Option<Text::RegexValue>>,
    /// Decimal handles for D-DECIMAL1 (#1219) — side table of CtDecimal.
    pub(crate) decimal_values: Vec<Option<jet_foundation::Numeric::CtDecimal>>,
    /// Fraction handles for D-NUMTYPE1 (#1464) — side table of CtFraction.
    pub(crate) fraction_values: Vec<Option<jet_foundation::Numeric::CtFraction>>,
    /// Set by a host shim when the user program hits a runtime panic (overflow,
    /// list index/slice OOB, a couple of concurrency panics). Non-`None` makes
    /// JIT-generated code branch to its epilogue on the next `emit_trap_check`,
    /// so the trap unwinds through pure Cranelift control flow (never a Rust
    /// panic through a JIT frame — I1). `resident_invoke` turns it into an
    /// `E0953` diagnostic, exactly as the tier-0 interpreter reports the same
    /// panic. Keeps the FIRST message; later traps on the unwind path are noise.
    pub(crate) trapped: Option<String>,
    /// Soft process exit for rich `require`/`panic` reports — stderr already
    /// holds the AOT-matching text; resident returns `Ran` with this code.
    pub(crate) exit_code: Option<i32>,

    /// Compiler-owned E3003 rendered after native code returns; never unwinds
    /// through a Cranelift frame.
    pub(crate) deadline_exceeded: Option<String>,
    pub(crate) readers: Vec<crate::Parse::ReaderSlot>,
    pub(crate) cursors: Vec<crate::Parse::CursorSlot>,
    pub(crate) reflect_values: Vec<ReflectSlot>,
    /// D-LAYOUT1: layout handles / LinExpr / Constraint slots (#1225).
    pub(crate) layout_slots: Vec<crate::Layout::LayoutSlot>,
    /// D-REACT1 / D-EVENT1: reactive + event opaque handles (#1225).
    pub(crate) reactive: crate::Reactive::ReactiveState,
    /// D-RENDERTGT*: UI backends / nodes / events (#1225).
    pub(crate) ui: crate::Ui::UiState,
    /// D-WEBAPP1 / c-devserver: web app + DevServer handles (#1226).
    pub(crate) web: crate::Web::WebState,
}

/// Control transfer for a shared Prelude stop whose Rust signature is `!`.
/// The report is recorded first; the resident caller remains the owner of
/// cleanup and the final target exit.
#[derive(Debug)]
pub(crate) struct JitRuntimeStop;

pub(crate) fn runtime_stop_unwind(code: &'static str, line: u32, message: &str) -> ! {
    with_runtime_mut(|rt| rt.set_runtime_stop(code, line, message));
    std::panic::resume_unwind(Box::new(JitRuntimeStop));
}

impl JitRuntime {
    /// Snapshot string handles allocated during lowering (baked into code).
    pub(crate) fn snapshot_compile_strings(&mut self) {
        self.compile_strings = self.heap.string_slots();
    }

    /// Record a runtime panic. Keeps the first message (the unwind branch may
    /// re-enter trap sites with dummy values before the epilogue is reached).
    fn store_trap(&mut self, msg: &str) {
        if Concurrency::in_scheduler_task() {
            Concurrency::set_task_trap(msg);
            return;
        }
        if self.trapped.is_none() {
            self.trapped = Some(msg.to_string());
        }
    }

    /// Legacy host failures still enter the one runtime-stop renderer. The
    /// caller supplies no source facts for an engine failure, so the report
    /// keeps the generic E3001 location shape.
    pub(crate) fn set_trap(&mut self, msg: &str) {
        self.set_runtime_stop("E3001", 0, msg);
    }

    pub(crate) fn set_deadline(&mut self, rendered: String) {
        if self.deadline_exceeded.is_none() {
            self.deadline_exceeded = Some(rendered);
        }
    }

    /// Marshal a runtime breach into the Foundation Prelude renderer. JIT
    /// hosts provide only source facts and keep no user-facing wording.
    pub(crate) fn set_runtime_stop(&mut self, code: &'static str, line: u32, message: &str) {
        if self.trapped.is_some() || self.exit_code.is_some() {
            return;
        }
        let src_line = self
            .source_text
            .lines()
            .nth((line as usize).saturating_sub(1))
            .unwrap_or_default();
        let (fn_name, src_line) = if code == "E3001" {
            (&self.current_function, src_line)
        } else {
            (&String::new(), "")
        };
        let report = jet_foundation::Outcome::jet_render_runtime_stop(
            code,
            &self.source_file,
            line,
            fn_name,
            src_line,
            1,
            1,
            message,
            "",
        );
        self.stderr.push_str(&report.rendered);
        self.exit_code = Some(report.exit_code);
        self.store_trap(message);
    }

    pub(crate) fn set_rendered_runtime_stop(&mut self, rendered: String, exit_code: i32) {
        if self.trapped.is_some() || self.exit_code.is_some() {
            return;
        }
        self.stderr.push_str(&rendered);
        self.exit_code = Some(exit_code);
        self.trapped = Some("__jet_rich_panic__".to_string());
    }

    pub(crate) fn stack_enter(&mut self, file: &str, line: u32, fn_name: &str, src_line: &str) {
        const LIMIT: usize = jet_foundation::Outcome::JET_RUNTIME_STACK_LIMIT;
        self.source_file = file.to_string();
        self.stack_depth = self.stack_depth.saturating_add(1);
        if self.stack_depth > LIMIT {
            let message = jet_foundation::Outcome::jet_stack_overflow_message(fn_name);
            let report = jet_foundation::Outcome::jet_render_runtime_stop(
                "E3012",
                file,
                line,
                fn_name,
                src_line,
                1,
                1,
                &message,
                "",
            );
            self.stderr.push_str(&report.rendered);
            self.exit_code = Some(report.exit_code);
            self.store_trap(&message);
        }
    }

    pub(crate) fn stack_leave(&mut self) {
        self.stack_depth = self.stack_depth.saturating_sub(1);
    }
}

pub(crate) struct ResidentModule {
    pub(crate) module: JITModule,
    pub(crate) host: HostFns,
    pub(crate) main_id: FuncId,
    pub(crate) main_returns_result: bool,
    pub(crate) main_returns_app: bool,
    pub(crate) main_returns_default_err: bool,
    pub(crate) main_error_type: Option<jet_foundation::AST::Type>,
    pub(crate) main_error_is_packed: bool,
}

fn with_runtime_mut<F: FnOnce(&mut JitRuntime)>(f: F) {
    Concurrency::with_runtime_mut(f);
}

/// Route resident output through the terminal when the caller owns a TTY.
/// Otherwise keep it in `JitRuntime` so the backend returns one ordered
/// `ProgramOutput` buffer. This is an engine adapter; terminal framing stays
/// in `Prelude/Term.rs`.
pub(crate) fn write_jit_stdout(text: &str, flush: bool) -> Result<(), String> {
    let terminal = crate::IO::term_prelude::jet_term_stdout_is_terminal();
    let direct = terminal || Concurrency::active_runtime_ptr().is_none();
    if direct {
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        out.write_all(text.as_bytes())
            .map_err(|error| format!("write stdout: {error}"))?;
        if flush || terminal {
            out.flush()
                .map_err(|error| format!("flush stdout: {error}"))?;
        }
    } else {
        with_runtime_mut(|rt| rt.stdout.push_str(text));
    }
    Ok(())
}

pub(crate) fn write_jit_stderr(text: &str, flush: bool) -> Result<(), String> {
    let terminal = crate::IO::term_prelude::jet_term_stderr_is_terminal();
    let direct = terminal || Concurrency::active_runtime_ptr().is_none();
    if direct {
        use std::io::Write;
        let mut out = std::io::stderr().lock();
        out.write_all(text.as_bytes())
            .map_err(|error| format!("write stderr: {error}"))?;
        if flush || terminal {
            out.flush()
                .map_err(|error| format!("flush stderr: {error}"))?;
        }
    } else {
        with_runtime_mut(|rt| rt.stderr.push_str(text));
    }
    Ok(())
}

fn with_runtime_trap<F: FnOnce(&mut JitRuntime)>(f: F) {
    Concurrency::with_runtime_mut(|rt| {
        if catch_unwind(AssertUnwindSafe(|| f(rt))).is_err() {
            rt.set_trap("the JIT runtime helper panicked");
        }
    });
}

fn with_runtime_result<R: Default, F: FnOnce(&mut JitRuntime) -> R>(default: R, f: F) -> R {
    Concurrency::with_runtime_mut(|rt| match catch_unwind(AssertUnwindSafe(|| f(rt))) {
        Ok(value) => value,
        Err(_) => {
            rt.set_trap("the JIT runtime helper panicked");
            default
        }
    })
}

/// Record an arithmetic overflow/div-by-zero trap. Returns normally (the
/// caller yields a dummy `0`); JIT code branches to its epilogue at the next
/// `emit_trap_check`. Message text is unchanged from the old exit-70 path.
fn jet_trap_overflow(op: &str, line: u32) {
    use jet_codegen::Comptime::MathLayout;
    let msg = match op {
        "add" => "this addition overflows the value's type (the result is outside its range)",
        "sub" => "this subtraction overflows the value's type (the result is outside its range)",
        "mul" => "this multiplication overflows the value's type (the result is outside its range)",
        "div" => "this division can't be done (dividing by zero, or overflow)",
        "pow" => MathLayout::INTEGER_POWER_OVERFLOW,
        _ => "this operation overflows the value's type (the result is outside its range)",
    };
    with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, msg));
}

pub(crate) const INTN_OP_ADD: i64 = 0;
pub(crate) const INTN_OP_SUB: i64 = 1;
pub(crate) const INTN_OP_MUL: i64 = 2;
pub(crate) const INTN_OP_DIV: i64 = 3;
pub(crate) const INTN_OP_REM: i64 = 4;
pub(crate) const INTN_OP_BIT_AND: i64 = 5;
pub(crate) const INTN_OP_BIT_OR: i64 = 6;
pub(crate) const INTN_OP_BIT_XOR: i64 = 7;
pub(crate) const INTN_OP_SHL: i64 = 8;
pub(crate) const INTN_OP_SHR: i64 = 9;
/// D-EXPSEM1=A: `^` on a fixed-width whole number.
pub(crate) const INTN_OP_POW: i64 = 10;
/// D-FLOORDIV1=A: `/%` on a fixed-width whole number.
pub(crate) const INTN_OP_FLOOR_DIV: i64 = 11;
/// D-MODSEM1=A: `%` on a fixed-width whole number.
pub(crate) const INTN_OP_MOD: i64 = 12;
pub(crate) const INTN_MODE_TRAP: i64 = 0;
pub(crate) const INTN_MODE_WRAPPING: i64 = 1;
pub(crate) const INTN_MODE_SATURATING: i64 = 2;
pub(crate) const INTN_MODE_CHECKED: i64 = 3;

/// Reads the resident runtime's trapped flag from JIT code. `1` = a trap is
/// pending (branch to epilogue); `0` = keep going.
extern "C" fn jet_jit_is_trapped() -> i64 {
    if Concurrency::local_rich_panic_pending() {
        1
    } else if Concurrency::in_scheduler_task() {
        i64::from(Concurrency::task_trap_pending())
    } else {
        Concurrency::with_runtime_mut(|rt| i64::from(rt.trapped.is_some()))
    }
}

extern "C" fn jet_jit_stack_enter(file: i64, line: i64, fn_name: i64, src_line: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        let src_line = rt.heap.clone_string(src_line).unwrap_or_default();
        rt.stack_enter(&file, line.max(0) as u32, &fn_name, &src_line);
        i64::from(rt.trapped.is_some())
    })
}

extern "C" fn jet_jit_stack_leave() {
    Concurrency::with_runtime_mut(JitRuntime::stack_leave);
}

extern "C" fn jet_jit_add_i64(a: i64, b: i64, line: u32) -> i64 {
    match a.checked_add(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("add", line);
            0
        }
    }
}

extern "C" fn jet_jit_sub_i64(a: i64, b: i64, line: u32) -> i64 {
    match a.checked_sub(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("sub", line);
            0
        }
    }
}

extern "C" fn jet_jit_mul_i64(a: i64, b: i64, line: u32) -> i64 {
    match a.checked_mul(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("mul", line);
            0
        }
    }
}

extern "C" fn jet_jit_div_i64(a: i64, b: i64, line: u32) -> i64 {
    match a.checked_div(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("div", line);
            0
        }
    }
}

/// D-EXPSEM1=A: the same exact, trapping whole-number power the Prelude runs
/// (`Prelude/Core/Power.rs`). A negative exponent has no whole-number result.
extern "C" fn jet_jit_pow_i64(a: i64, b: i64, line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if b < 0 {
        with_runtime_mut(|rt| {
            rt.set_runtime_stop("E3010", line, MathLayout::INTEGER_POWER_NEGATIVE)
        });
        return 0;
    }
    match u32::try_from(b).ok().and_then(|e| a.checked_pow(e)) {
        Some(value) => value,
        None => {
            jet_trap_overflow("pow", line);
            0
        }
    }
}

/// D-EXPSEM1=A: `^` on floats is the ordinary floating-point power.
extern "C" fn jet_jit_pow_f64(a: f64, b: f64) -> f64 {
    a.powf(b)
}

/// D-FLOORDIV1=A: the same rounding-down division the Prelude runs
/// (`Prelude/Core/Division.rs`), through the one shared rule.
extern "C" fn jet_jit_floordiv_i64(a: i64, b: i64, line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if b == 0 {
        with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, MathLayout::INTEGER_DIVIDE_ZERO));
        return 0;
    }
    match MathLayout::floor_div(a as i128, b as i128).and_then(|v| i64::try_from(v).ok()) {
        Some(value) => value,
        None => {
            with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, MathLayout::INTEGER_DIVIDE_OVERFLOW));
            0
        }
    }
}

/// D-FLOORDIV1=A: on floats `/%` divides and rounds the answer down.
extern "C" fn jet_jit_floordiv_f64(a: f64, b: f64) -> f64 {
    (a / b).floor()
}

/// D-MODSEM1=A: the floored modulo the Prelude runs
/// (`Prelude/Core/Division.rs`), through the one shared rule.
extern "C" fn jet_jit_mod_i64(a: i64, b: i64, line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if b == 0 {
        with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, MathLayout::INTEGER_DIVIDE_ZERO));
        return 0;
    }
    match MathLayout::floored_mod(a as i128, b as i128).and_then(|v| i64::try_from(v).ok()) {
        Some(value) => value,
        None => {
            with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, MathLayout::INTEGER_DIVIDE_OVERFLOW));
            0
        }
    }
}

extern "C" fn jet_jit_rem_i64(a: i64, b: i64, line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if let Some(message) = MathLayout::integer_remainder_trap(b) {
        with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, message));
        return 0;
    }
    // D-MODSEM1=A: `MIN %% -1` is 0, the same answer `%` gives.
    a.wrapping_rem(b)
}

extern "C" fn jet_jit_intn_binop(
    left: i64,
    right: i64,
    op: i64,
    mode: i64,
    signed: i64,
    bits: i64,
    right_signed: i64,
    line: u32,
) -> i64 {
    use jet_codegen::AST::BinOp;
    use jet_codegen::Comptime::{CtReport, CtValue, MathLayout};
    let op = match op {
        INTN_OP_ADD => BinOp::Add,
        INTN_OP_SUB => BinOp::Sub,
        INTN_OP_MUL => BinOp::Mul,
        INTN_OP_DIV => BinOp::Div,
        INTN_OP_REM => BinOp::Rem,
        INTN_OP_BIT_AND => BinOp::BitAnd,
        INTN_OP_BIT_OR => BinOp::BitOr,
        INTN_OP_BIT_XOR => BinOp::BitXor,
        INTN_OP_SHL => BinOp::Shl,
        INTN_OP_SHR => BinOp::Shr,
        INTN_OP_POW => BinOp::Pow,
        INTN_OP_FLOOR_DIV => BinOp::FloorDiv,
        INTN_OP_MOD => BinOp::Mod,
        _ => {
            with_runtime_mut(|rt| {
                rt.set_runtime_stop("E3010", line, "unknown fixed-width integer operation")
            });
            return 0;
        }
    };
    let signed = signed != 0;
    let bits = bits as u8;
    let right_signed = right_signed != 0;
    let shift_count = MathLayout::integer_widen(right, right_signed);
    if let Some(message) = MathLayout::integer_shift_trap(op, shift_count, bits) {
        with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, &message));
        return 0;
    }
    // D-FLOORDIV1=A: `/%` names a zero divisor exactly, rather than falling
    // into the shared "this division can't be done" wording below.
    if mode == INTN_MODE_TRAP && matches!(op, BinOp::FloorDiv | BinOp::Mod) && right == 0 {
        with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, MathLayout::INTEGER_DIVIDE_ZERO));
        return 0;
    }
    if mode == INTN_MODE_TRAP && op == BinOp::Rem {
        if let Some(message) = MathLayout::integer_remainder_trap(right) {
            with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, message));
            return 0;
        }
    }
    let span = jet_codegen::Diagnostics::Span::new(0, 0);
    let result = match mode {
        INTN_MODE_TRAP => {
            MathLayout::integer_binop(op, left, right, signed, bits, right_signed, span)
        }
        INTN_MODE_WRAPPING => MathLayout::overflow_opt(
            jet_codegen::Syntax::BUILTIN_WRAPPING,
            op,
            left,
            right,
            signed,
            bits,
            span,
        ),
        INTN_MODE_SATURATING => MathLayout::overflow_opt(
            jet_codegen::Syntax::BUILTIN_SATURATING,
            op,
            left,
            right,
            signed,
            bits,
            span,
        ),
        INTN_MODE_CHECKED => MathLayout::overflow_opt(
            jet_codegen::Syntax::BUILTIN_CHECKED,
            op,
            left,
            right,
            signed,
            bits,
            span,
        ),
        _ => unreachable!("fixed-width integer mode"),
    };
    match result {
        Ok(CtValue::Int(value)) => value,
        Ok(CtValue::Present(value)) => {
            let CtValue::Int(value) = *value else {
                return 0;
            };
            Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, true, value as u64))
        }
        Ok(CtValue::Failed(CtReport::Clean(_))) => 0,
        Ok(_) => 0,
        Err(_) if mode == INTN_MODE_CHECKED => 0,
        Err(_) => {
            // D-FLOORDIV1=A / D-MODSEM1=A: `/%` and `%` report the Prelude's own
            // overflow wording, not the shared "this division can't be done"
            // sentence `/` uses, so a fixed-width width overflow reads the same
            // here as it does on every other tier.
            match op {
                BinOp::FloorDiv | BinOp::Mod => {
                    with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, MathLayout::INTEGER_DIVIDE_OVERFLOW));
                }
                BinOp::Pow => {
                    with_runtime_mut(|rt| rt.set_runtime_stop("E3010", line, MathLayout::INTEGER_POWER_OVERFLOW));
                }
                _ => {
                    let name = match op {
                        BinOp::Add => "add",
                        BinOp::Sub => "sub",
                        BinOp::Mul => "mul",
                        BinOp::Div => "div",
                        _ => "shift",
                    };
                    jet_trap_overflow(name, line);
                }
            }
            0
        }
    }
}

extern "C" fn jet_jit_intn_to_string(value: i64, signed: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .alloc_string(jet_codegen::Comptime::MathLayout::integer_show(
                value,
                signed != 0,
            ))
    })
}

extern "C" fn jet_jit_print_i64(v: i64) {
    let _ = write_jit_stdout(&format!("{v}\n"), false);
}

extern "C" fn jet_jit_print_f64(v: f64) {
    let _ = write_jit_stdout(&format!("{}\n", jet_rt::display_f64(v)), false);
}

extern "C" fn jet_jit_print_bool(v: i8) {
    let _ = write_jit_stdout(if v == 0 { "false\n" } else { "true\n" }, false);
}

extern "C" fn jet_jit_print_char(v: i32) {
    let ch = char::from_u32(v as u32).unwrap_or('?');
    let _ = write_jit_stdout(&format!("{ch}\n"), false);
}

extern "C" fn jet_jit_print_str(id: i64) {
    let text = Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id));
    if let Some(text) = text {
        let _ = write_jit_stdout(&format!("{text}\n"), false);
    }
}

extern "C" fn jet_jit_str_begin() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_string())
}

extern "C" fn jet_jit_str_push_lit(buf_id: i64, lit_id: i64) {
    with_runtime_mut(|rt| {
        let Some(lit) = rt.heap.clone_string(lit_id) else {
            return;
        };
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&lit);
        }
    });
}

extern "C" fn jet_jit_str_push_i64(buf_id: i64, v: i64) {
    with_runtime_mut(|rt| {
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&v.to_string());
        }
    });
}

extern "C" fn jet_jit_str_push_f64(buf_id: i64, v: f64) {
    with_runtime_trap(|rt| {
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&jet_rt::display_f64(v));
        }
    });
}

extern "C" fn jet_jit_str_push_compact_f64(buf_id: i64, v: f64) {
    with_runtime_trap(|rt| {
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&v.to_string());
        }
    });
}

extern "C" fn jet_jit_str_push_bool(buf_id: i64, v: i8) {
    with_runtime_mut(|rt| {
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(if v == 0 { "false" } else { "true" });
        }
    });
}

extern "C" fn jet_jit_str_push_char(buf_id: i64, v: i32) {
    with_runtime_mut(|rt| {
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            match char::from_u32(v as u32) {
                Some(ch) => buf.push(ch),
                None => buf.push('?'),
            }
        }
    });
}

extern "C" fn jet_jit_str_push_str(buf_id: i64, str_id: i64) {
    with_runtime_mut(|rt| {
        let Some(s) = rt.heap.clone_string(str_id) else {
            return;
        };
        if let Some(buf) = rt.heap.get_string_mut(buf_id) {
            buf.push_str(&s);
        }
    });
}

extern "C" fn jet_jit_str_eq(a: i64, b: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| match (rt.heap.get_string(a), rt.heap.get_string(b)) {
        (Some(x), Some(y)) => i8::from(x == y),
        _ => 0,
    })
}

extern "C" fn jet_jit_str_contains(hay: i64, needle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| match (rt.heap.get_string(hay), rt.heap.get_string(needle)) {
        (Some(h), Some(n)) => i8::from(h.contains(n)),
        _ => 0,
    })
}

extern "C" fn jet_jit_str_starts_with(hay: i64, needle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| match (rt.heap.get_string(hay), rt.heap.get_string(needle)) {
        (Some(h), Some(n)) => i8::from(h.starts_with(n)),
        _ => 0,
    })
}

extern "C" fn jet_jit_str_ends_with(hay: i64, needle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| match (rt.heap.get_string(hay), rt.heap.get_string(needle)) {
        (Some(h), Some(n)) => i8::from(h.ends_with(n)),
        _ => 0,
    })
}

extern "C" fn jet_jit_str_len(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        rt.heap
            .get_string(id)
            .map(jet_rt::string_len_chars)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_str_byte_len(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        rt.heap
            .get_string(id)
            .map(|s| s.len() as i64)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_str_is_ascii(id: i64) -> i8 {
    with_runtime_result(0, |rt| {
        i8::from(rt.heap.get_string(id).map(|s| s.is_ascii()).unwrap_or(false))
    })
}

/// `core.text.unicode.scalars` — list of one-scalar strings (AOT `Vec<String>`).
extern "C" fn jet_jit_str_scalar_strings(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        for ch in text.chars() {
            let sid = rt.heap.alloc_string(ch.to_string());
            rt.heap
                .list_push_int(list, sid)
                .expect("jit str scalar strings: bad list handle");
        }
        list
    })
}

extern "C" fn jet_jit_str_clone(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_str_trim(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt
            .heap
            .get_string(id)
            .map(jet_rt::string_trim)
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_str_to_upper(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt
            .heap
            .get_string(id)
            .map(jet_rt::string_to_upper)
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_str_to_lower(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt
            .heap
            .get_string(id)
            .map(jet_rt::string_to_lower)
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_str_replace(id: i64, from_id: i64, to_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let from = rt.heap.clone_string(from_id).unwrap_or_default();
        let to = rt.heap.clone_string(to_id).unwrap_or_default();
        rt.heap
            .alloc_string(jet_rt::string_replace(&text, &from, &to))
    })
}

extern "C" fn jet_jit_str_lines(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        for line in text.lines() {
            let sid = rt.heap.alloc_string(line.to_string());
            rt.heap
                .list_push_int(list, sid)
                .expect("jit str lines: bad list handle");
        }
        list
    })
}

/// `String.split` JIT host: same piece sequence as AOT `jet_iter_string_split`.
///
/// AOT returns a lazy `JetIter<String>`; Cranelift host shims can only pass i64
/// handles, so this eagerly materializes that sequence into a list handle typed
/// as `Iter<String>`. Observable values for split + adapters + `to_list` match
/// AOT; true pull-based laziness waits on an Iter-capable JIT ABI.
extern "C" fn jet_jit_str_split(id: i64, sep_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        // Match AOT `jet_iter_string_split` / Rust `str::split` piece order
        // (including empty-sep Char split with leading/trailing empties).
        for part in text.split(&sep) {
            let sid = rt.heap.alloc_string(part.to_string());
            rt.heap
                .list_push_int(list, sid)
                .expect("jit str split: bad list handle");
        }
        list
    })
}

extern "C" fn jet_jit_str_rsplit(id: i64, sep_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        if sep.is_empty() {
            for part in text.split(&sep) {
                let sid = rt.heap.alloc_string(part.to_string());
                rt.heap
                    .list_push_int(list, sid)
                    .expect("jit str rsplit: bad list handle");
            }
        } else {
            let mut parts: Vec<String> = text.rsplit(&sep).map(|p| p.to_string()).collect();
            parts.reverse();
            for part in parts {
                let sid = rt.heap.alloc_string(part);
                rt.heap
                    .list_push_int(list, sid)
                    .expect("jit str rsplit: bad list handle");
            }
        }
        list
    })
}

extern "C" fn jet_jit_str_chars(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        for ch in text.chars() {
            rt.heap
                .list_push_int(list, ch as i64)
                .expect("jit str chars: bad list handle");
        }
        list
    })
}

extern "C" fn jet_jit_str_bytes(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let list = rt.heap.alloc_empty_list();
        for byte in text.into_bytes() {
            rt.heap
                .list_push_int(list, i64::from(byte))
                .expect("jit str bytes: bad list handle");
        }
        list
    })
}

extern "C" fn jet_jit_str_after(id: i64, sep_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        rt.heap
            .alloc_string(jet_rt::string_after(&text, &sep))
    })
}

extern "C" fn jet_jit_str_before(id: i64, sep_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        rt.heap
            .alloc_string(jet_rt::string_before(&text, &sep))
    })
}

extern "C" fn jet_jit_str_trim_view(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let Some(text) = rt.heap.get_string(id) else {
            return 0;
        };
        let start = text.len() - text.trim_start().len();
        let end = text.trim_end().len();
        rt.heap.alloc_string_view(id, start, end).unwrap_or(0)
    })
}

extern "C" fn jet_jit_str_after_view(id: i64, sep_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let Some(text) = rt.heap.get_string(id) else {
            return 0;
        };
        let start = text.find(&sep).map_or(0, |index| index + sep.len());
        rt.heap
            .alloc_string_view(id, start, text.len())
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_str_before_view(id: i64, sep_id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let sep = rt.heap.clone_string(sep_id).unwrap_or_default();
        let Some(text) = rt.heap.get_string(id) else {
            return 0;
        };
        let end = text.find(&sep).unwrap_or(text.len());
        rt.heap.alloc_string_view(id, 0, end).unwrap_or(0)
    })
}

/// Inclusive string slice (`s.slice(lo, hi)`). Same start/end = one char.
extern "C" fn jet_jit_str_slice(id: i64, start: i64, end: i64) -> i64 {
    with_runtime_result(0, |rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        let chars: Vec<char> = text.chars().collect();
        let len = chars.len() as i64;
        let lo = start.clamp(0, len) as usize;
        let hi = end.clamp(0, len.saturating_sub(1).max(0)) as usize;
        let sliced: String = if chars.is_empty() || start > end || lo >= chars.len() {
            String::new()
        } else {
            chars[lo..=hi.min(chars.len() - 1)].iter().collect()
        };
        rt.heap.alloc_string(sliced)
    })
}

/// `Clock.new(ms)` — manual clock handle (1-based index into `rt.clocks`).
extern "C" fn jet_jit_clock_new(ms: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.clocks.push(ms);
        rt.clocks.len() as i64
    })
}

extern "C" fn jet_jit_clock_now(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.clocks
            .get((handle as usize).wrapping_sub(1))
            .copied()
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_clock_tick(handle: i64, delta: i64) {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(now) = rt.clocks.get_mut((handle as usize).wrapping_sub(1)) {
            *now = now.saturating_add(delta);
        }
    });
}

extern "C" fn jet_jit_clock_advance(handle: i64, to_ms: i64) -> i64 {
    // D-DET-CAPAPI: absolute set — matches AOT `jet_clock_advance`.
    // parity: guard tests/dev.rs::io_cli_terminal_and_time_match_interpreter_jit_and_aot
    Concurrency::with_runtime_mut(|rt| {
        if let Some(now) = rt.clocks.get_mut((handle as usize).wrapping_sub(1)) {
            *now = to_ms;
            to_ms
        } else {
            0
        }
    })
}

extern "C" fn jet_jit_clock_wait(handle: i64, duration_ms: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if let Some(now) = rt.clocks.get_mut((handle as usize).wrapping_sub(1)) {
            *now = now.saturating_add(duration_ms);
            *now
        } else {
            0
        }
    })
}


extern "C" fn jet_jit_rich_panic(
    file: i64,
    line: i64,
    fn_name: i64,
    src_line: i64,
    col: i64,
    caret: i64,
    msg: i64,
    locals: i64,
) -> i64 {
    let in_task = Concurrency::in_jit_task();
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        let src_line = rt.heap.clone_string(src_line).unwrap_or_default();
        let msg = rt.heap.clone_string(msg).unwrap_or_default();
        let locals = rt.heap.clone_string(locals).unwrap_or_default();
        Concurrency::set_rich_panic_reason(msg.clone());
        let report = jet_foundation::Outcome::jet_render_runtime_stop(
            "E3001",
            &file,
            line.max(0) as u32,
            &fn_name,
            &src_line,
            col.max(1) as u32,
            caret.max(1) as u32,
            &msg,
            &locals,
        );
        if in_task {
            // A child failure is a typed TaskFailure. Its trap must remain
            // thread-local: the resident runtime is shared with the parent,
            // and a shared trap would make the parent skip unrelated joins.
            Concurrency::set_rich_panic_report(report.rendered);
            Concurrency::set_local_rich_panic();
        } else {
            rt.stderr.push_str(&report.rendered);
            rt.exit_code = Some(report.exit_code);
            rt.set_trap("__jet_rich_panic__");
        }
        0
    })
}

extern "C" fn jet_jit_todo_stop(line: i64, expected_type: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let expected_type = rt.heap.clone_string(expected_type).unwrap_or_default();
        let message = jet_foundation::Outcome::jet_todo_message(
            &rt.source_file,
            line.max(0) as u32,
            &expected_type,
        );
        rt.set_runtime_stop("E3011", line.max(0) as u32, &message);
        0
    })
}


extern "C" fn jet_jit_trap_panic(_unused: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap("panic");
        0
    })
}

/// D-FAIL-TIER1: the JIT only marshals contract values.  The predicate and
/// rendered report are the same Prelude functions used by AOT and TIR-eval.
extern "C" fn jet_jit_contract_check(condition: i8) -> i8 {
    i8::from(contract_kernel::jet_contract_check(condition != 0))
}

extern "C" fn jet_jit_contract_fail(msg: i64, file: i64, line: i64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let msg = rt.heap.clone_string(msg).unwrap_or_default();
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let clause = if kind == 0 { "Pre" } else { "Post" };
        rt.stderr.push_str(&contract_kernel::jet_contract_report(
            clause,
            &msg,
            &file,
            line as u32,
        ));
        rt.stderr.push('\n');
        rt.exit_code = Some(70);
        rt.set_trap("__jet_contract__");
        0
    })
}

extern "C" fn jet_jit_trace_err(file: i64, line: i64, fn_name: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        if let Some(frame) = jet_foundation::Outcome::jet_journey_frame(
            &file,
            line as u32,
            &fn_name,
            || String::new(),
        ) {
            rt.stderr.push_str(&frame);
        }
    });
}

extern "C" fn jet_jit_trace_err_note(file: i64, line: i64, fn_name: i64, note: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        let note = rt.heap.clone_string(note).unwrap_or_default();
        if let Some(frame) = jet_foundation::Outcome::jet_journey_frame(
            &file,
            line as u32,
            &fn_name,
            || note,
        ) {
            rt.stderr.push_str(&frame);
        }
    })
}

extern "C" fn jet_jit_trace_reset() {
    jet_foundation::Outcome::jet_journey_reset();
}

extern "C" fn jet_jit_parse_i64(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        match text.trim().parse::<i64>() {
            Ok(value) => alloc_jit_result(rt, true, value as u64),
            Err(_) => {
                let error = rt
                    .heap
                    .alloc_string(format!("cannot parse `{text}` as an integer"));
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

extern "C" fn jet_jit_parse_f64(id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let text = rt.heap.clone_string(id).unwrap_or_default();
        match text.trim().parse::<f64>() {
            Ok(value) => alloc_jit_result(rt, true, value.to_bits()),
            Err(_) => {
                let error = rt
                    .heap
                    .alloc_string(format!("cannot parse `{text}` as a float"));
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

fn numeric_int_bounds(kind: i64) -> (i128, i128) {
    match kind {
        0 => (i8::MIN as i128, i8::MAX as i128),
        1 => (i16::MIN as i128, i16::MAX as i128),
        2 => (i32::MIN as i128, i32::MAX as i128),
        3 => (i64::MIN as i128, i64::MAX as i128),
        4 => (u8::MIN as i128, u8::MAX as i128),
        5 => (u16::MIN as i128, u16::MAX as i128),
        6 => (u32::MIN as i128, u32::MAX as i128),
        _ => (u64::MIN as i128, u64::MAX as i128),
    }
}

extern "C" fn jet_jit_numeric_try_i64(value: i64, source_unsigned: i64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let value = if source_unsigned != 0 {
            value as u64 as i128
        } else {
            value as i128
        };
        let (lo, hi) = numeric_int_bounds(kind);
        if value >= lo && value <= hi {
            alloc_jit_result(rt, true, value as u64)
        } else {
            let error = rt.heap.alloc_string("value doesn't fit in destination type");
            alloc_jit_result(rt, false, error as u64)
        }
    })
}

extern "C" fn jet_jit_numeric_float_to_int(value: f64, kind: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let (lo, hi) = numeric_int_bounds(kind);
        let upper = hi as f64 + 1.0;
        if value.is_finite() && value >= lo as f64 && value < upper {
            alloc_jit_result(rt, true, value.trunc() as i128 as u64)
        } else {
            let error = rt.heap.alloc_string("value doesn't fit in destination type");
            alloc_jit_result(rt, false, error as u64)
        }
    })
}

extern "C" fn jet_jit_numeric_float_narrow(value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if value.is_finite() && value >= -(f32::MAX as f64) && value <= f32::MAX as f64 {
            alloc_jit_result(rt, true, ((value as f32) as f64).to_bits())
        } else {
            let error = rt.heap.alloc_string("value doesn't fit in F32");
            alloc_jit_result(rt, false, error as u64)
        }
    })
}

extern "C" fn jet_jit_numeric_checked_widen(
    raw: i64,
    source_signed: i64,
    target_f32: i64,
) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        match jet_codegen::numeric_widen::jet_numeric_checked_widen(
            raw as u64,
            source_signed != 0,
            target_f32 != 0,
        ) {
            Some(value) => value,
            None => {
                rt.set_trap(jet_codegen::numeric_widen::JET_NUMERIC_WIDEN_TRAP);
                0.0
            }
        }
    })
}

extern "C" fn jet_jit_distinct_range(value: i64, lo: i64, hi: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if value >= lo && value <= hi {
            alloc_jit_result(rt, true, value as u64)
        } else {
            let error = rt.heap.alloc_string("value is outside the distinct type's range");
            alloc_jit_result(rt, false, error as u64)
        }
    })
}

extern "C" fn jet_jit_distinct_range_result(handle: i64, lo: i64, hi: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(result) = jit_result(rt, handle) else {
            return 0;
        };
        if !result.ok {
            return handle;
        }
        let value = result.bits as i64;
        if value >= lo && value <= hi {
            handle
        } else {
            let error = rt.heap.alloc_string("value is outside the distinct type's range");
            alloc_jit_result(rt, false, error as u64)
        }
    })
}

extern "C" fn jet_jit_numeric_predicate(value: f64, op: i64) -> i8 {
    match op {
        0 => i8::from(value.is_nan()),
        1 => i8::from(value.is_infinite()),
        _ => i8::from(value.is_finite()),
    }
}

extern "C" fn jet_jit_numeric_bit_count(value: i64, op: i64, width: i64) -> i64 {
    let method = match op {
        0 => "count_ones",
        1 => "count_zeros",
        2 => "leading_zeros",
        _ => "trailing_zeros",
    };
    jet_codegen::Comptime::MathLayout::integer_bit_count(value, width as u32, method).unwrap_or(0)
}

extern "C" fn jet_jit_struct_new(n: i64) -> i64 {
    STRUCT_NEW_COUNT.with(|count| count.set(count.get() + 1));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record(n as usize))
}

extern "C" fn jet_jit_struct_assign(dst: i64, src: i64) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_assign_from(dst, src);
    });
}

extern "C" fn jet_jit_struct_get_i64(h: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.record_get_int(h, idx).unwrap_or(0))
}

extern "C" fn jet_jit_struct_get_f64(h: i64, idx: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.record_get_float(h, idx).unwrap_or(0.0))
}

extern "C" fn jet_jit_struct_get_bool(h: i64, idx: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| i8::from(rt.heap.record_get_bool(h, idx).unwrap_or(false)))
}

extern "C" fn jet_jit_struct_get_char(h: i64, idx: i64) -> i32 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .record_get_char(h, idx)
            .map(|c| c as i32)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_struct_get_str(h: i64, idx: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.record_get_string(h, idx).unwrap_or(0))
}

extern "C" fn jet_jit_struct_set_i64(h: i64, idx: i64, v: i64) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_int(h, idx, v);
    });
}

extern "C" fn jet_jit_struct_set_f64(h: i64, idx: i64, v: f64) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_float(h, idx, v);
    });
}

extern "C" fn jet_jit_struct_set_bool(h: i64, idx: i64, v: i8) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_bool(h, idx, v != 0);
    });
}

extern "C" fn jet_jit_struct_set_char(h: i64, idx: i64, v: i32) {
    with_runtime_mut(|rt| {
        let Some(ch) = char::from_u32(v as u32) else {
            return;
        };
        let _ = rt.heap.record_set_char(h, idx, ch);
    });
}

extern "C" fn jet_jit_struct_set_str(h: i64, idx: i64, v: i64) {
    with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_string(h, idx, v);
    });
}

extern "C" fn jet_jit_memo_probe(record: i64, slot: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| i8::from(rt.memo_values.contains_key(&(record, slot))))
}

extern "C" fn jet_jit_memo_get(record: i64, slot: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.memo_values.get(&(record, slot)).copied().unwrap_or(0)
    })
}

extern "C" fn jet_jit_memo_put(record: i64, slot: i64, value: i64) {
    with_runtime_mut(|rt| {
        rt.memo_values.insert((record, slot), value);
    });
}

extern "C" fn jet_jit_memo_clear(record: i64) {
    with_runtime_mut(|rt| {
        rt.memo_values.retain(|(owner, _), _| *owner != record);
    });
}

extern "C" fn jet_jit_memo_clear_slot(record: i64, slot: i64) {
    with_runtime_mut(|rt| {
        rt.memo_values.remove(&(record, slot));
    });
}

extern "C" fn jet_jit_err_new(message: i64, code: i64, cause: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        use jet_foundation::Outcome::{jet_err, JetAbsent};

        let message = rt.heap.clone_string(message).unwrap_or_default();
        let code = if code == 0 {
            Err(JetAbsent)
        } else {
            rt.heap
                .clone_string(code - 1)
                .ok_or(JetAbsent)
        };
        let cause = if cause == 0 {
            Err(JetAbsent)
        } else {
            let handle = cause - 1;
            rt.errors
                .get(handle.saturating_sub(1) as usize)
                .cloned()
                .ok_or(JetAbsent)
        };
        rt.errors.push(jet_err(message, code, cause));
        rt.errors.len() as i64
    })
}

extern "C" fn jet_jit_err_message(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(error) = rt.errors.get(handle.saturating_sub(1) as usize) else {
            return 0;
        };
        rt.heap
            .alloc_string(jet_foundation::Outcome::jet_err_message(error))
    })
}

extern "C" fn jet_jit_err_code(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(error) = rt.errors.get(handle.saturating_sub(1) as usize) else {
            return 0;
        };
        match jet_foundation::Outcome::jet_err_code(error) {
            Ok(code) => rt.heap.alloc_string(code) + 1,
            Err(_) => 0,
        }
    })
}

extern "C" fn jet_jit_err_cause(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(error) = rt.errors.get(handle.saturating_sub(1) as usize) else {
            return 0;
        };
        match jet_foundation::Outcome::jet_err_cause(error) {
            Ok(cause) => {
                rt.errors.push(cause);
                rt.errors.len() as i64 + 1
            }
            Err(_) => 0,
        }
    })
}

fn alloc_measurement(rt: &mut JitRuntime, value: (f64, f64)) -> i64 {
    let handle = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_float(handle, 0, value.0);
    let _ = rt.heap.record_set_float(handle, 1, value.1);
    handle
}

fn read_measurement(rt: &mut JitRuntime, handle: i64) -> Option<(f64, f64)> {
    Some((
        rt.heap.record_get_float(handle, 0)?,
        rt.heap.record_get_float(handle, 1)?,
    ))
}

extern "C" fn jet_jit_measurement_new(value: f64, uncertainty: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        alloc_measurement(
            rt,
            measurement_kernel::jet_measurement_kernel_new(value, uncertainty),
        )
    })
}

extern "C" fn jet_jit_measurement_arithmetic(left: i64, right: i64, op: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let (Some(left), Some(right)) = (
            read_measurement(rt, left),
            read_measurement(rt, right),
        ) else {
            rt.set_trap("the JIT received an invalid Measurement handle");
            return 0;
        };
        let value = match op {
            0 => measurement_kernel::jet_measurement_kernel_add(left, right),
            1 => measurement_kernel::jet_measurement_kernel_sub(left, right),
            2 => measurement_kernel::jet_measurement_kernel_mul(left, right),
            3 => measurement_kernel::jet_measurement_kernel_div(left, right),
            _ => {
                rt.set_trap("the JIT received an invalid Measurement operation");
                return 0;
            }
        };
        alloc_measurement(rt, value)
    })
}

extern "C" fn jet_jit_measurement_get(handle: i64, field: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .record_get_float(handle, field)
            .unwrap_or_else(|| {
                rt.set_trap("the JIT received an invalid Measurement handle");
                0.0
            })
    })
}

extern "C" fn jet_jit_measurement_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(value) = read_measurement(rt, handle) else {
            rt.set_trap("the JIT received an invalid Measurement handle");
            return 0;
        };
        let rendered = measurement_kernel::jet_measurement_kernel_show(value);
        rt.heap.alloc_string(rendered)
    })
}

const JIT_PERF_DEFAULT_FIDELITY_BITS: u32 = 1.0f32.to_bits();
// D-FIDELITY-API1=A: this signal is deliberately outside `JitRuntime` — it
// must survive `resident_teardown()` + a fresh `JitRuntime` (a "restart"),
// exactly like the AOT binary's process-global static survives every read/
// write for the life of that one running program. What must NOT happen is
// leaking one resident-JIT execution's override into a DIFFERENT one running
// concurrently on another thread (the actual bug: a process-wide
// `AtomicU32` let a parallel `cargo test` thread observe another thread's
// override mid-battery). Thread-local scoping keeps every within-thread
// restart/hot-swap/relaunch sequence exactly as before while giving each
// thread — i.e. each independent resident-JIT session — its own signal.
thread_local! {
    static JIT_PERF_FIDELITY: std::cell::Cell<u32> =
        const { std::cell::Cell::new(JIT_PERF_DEFAULT_FIDELITY_BITS) };
}

pub(crate) fn alloc_jit_result(rt: &mut JitRuntime, ok: bool, bits: u64) -> i64 {
    rt.results.push(JitResultValue { ok, bits });
    rt.results.len() as i64
}

mod service_adapter {
    use super::{alloc_jit_result, service_prelude, JitRuntime};
    use jet_foundation::AST::{CtReport, CtValue};
    use jet_foundation::Diagnostics::Span;

    const SERVICES_MODULE: i64 = 0;
    const SYNC_MODULE: i64 = 1;
    const SERVICE_AUTHORITY_MODULE: i64 = 2;

    #[derive(Clone, Copy)]
    enum ArgKind {
        String,
        Int,
        StringList,
        Slot,
        Restart,
        Delivery,
        DurationNs,
    }

    fn arg_kind(module: i64, method: &str, index: usize) -> Option<ArgKind> {
        match module {
            SERVICES_MODULE => match method {
                "runtime" => match index {
                    0 => Some(ArgKind::String),
                    1 => Some(ArgKind::DurationNs),
                    _ => None,
                },
                "tree" | "state_store" if index == 0 => Some(ArgKind::String),
                "set_restart" if index == 0 => Some(ArgKind::Slot),
                "set_restart" if index == 1 => Some(ArgKind::Restart),
                "set_delivery" if index == 0 => Some(ArgKind::Slot),
                "set_delivery" if index == 1 => Some(ArgKind::Delivery),
                "worker" if index == 0 => Some(ArgKind::Slot),
                "worker" if index == 1 => Some(ArgKind::String),
                "worker" if index == 2 => Some(ArgKind::Int),
                "group" if index == 0 => Some(ArgKind::Slot),
                "group" if index == 1 => Some(ArgKind::String),
                "group" if index == 2 => Some(ArgKind::StringList),
                "start" | "stop" if index == 0 => Some(ArgKind::Slot),
                "send" if index == 0 => Some(ArgKind::Slot),
                "send" if index == 1 => Some(ArgKind::Slot),
                "send" if index == 2 => Some(ArgKind::String),
                "receive" | "mailbox_depth" | "restarts" | "fail_worker" if index < 2 => {
                    Some(ArgKind::Slot)
                }
                "endpoint_show"
                | "tree_show"
                | "dead_letter_count"
                | "drain_dead_letters"
                | "set_state_empty"
                | "restore_snapshot"
                | "event_count"
                | "replay_events"
                | "directory_generation"
                | "handoff_generation"
                | "rollback_generation"
                | "chaos_fail"
                | "upgrade_receipt"
                | "observe"
                    if index == 0 =>
                {
                    Some(ArgKind::Slot)
                }
                "send_durable" if index < 2 => Some(ArgKind::Slot),
                "send_durable" if index < 4 => Some(ArgKind::String),
                "set_state_snapshot" | "set_state_event_log" if index == 0 => {
                    Some(ArgKind::Slot)
                }
                "set_state_snapshot" | "set_state_event_log" if index == 1 => {
                    Some(ArgKind::Slot)
                }
                "set_state_snapshot" | "set_state_event_log" if index == 2 => {
                    Some(ArgKind::String)
                }
                "set_state_snapshot" | "set_state_event_log" if index == 3 => {
                    Some(ArgKind::Int)
                }
                "set_state_snapshot" | "set_state_event_log" if index == 4 => {
                    Some(ArgKind::String)
                }
                "commit_snapshot" if index == 0 => Some(ArgKind::Slot),
                "commit_snapshot" if index == 1 => Some(ArgKind::String),
                "append_event" if index == 0 => Some(ArgKind::Slot),
                "append_event" if index == 1 => Some(ArgKind::String),
                "workflow_start" if index == 0 => Some(ArgKind::Slot),
                "workflow_start" if index == 1 => Some(ArgKind::String),
                "workflow_start" if index == 2 => Some(ArgKind::Int),
                "workflow_step" if index == 0 => Some(ArgKind::Slot),
                "workflow_step" if index == 1 => Some(ArgKind::Int),
                "workflow_step" if index == 2 => Some(ArgKind::String),
                "workflow_history" if index == 0 => Some(ArgKind::Slot),
                "workflow_history" if index == 1 => Some(ArgKind::Int),
                "directory_register" if index == 0 => Some(ArgKind::Slot),
                "directory_register" if index == 1 => Some(ArgKind::String),
                "directory_register" if index == 2 => Some(ArgKind::Slot),
                "directory_resolve" if index == 0 => Some(ArgKind::Slot),
                "directory_resolve" if index == 1 => Some(ArgKind::String),
                "drain_worker" if index < 2 => Some(ArgKind::Slot),
                _ => None,
            },
            SERVICE_AUTHORITY_MODULE => match method {
                "send" if index == 0 => Some(ArgKind::Slot),
                "send" if index == 1 => Some(ArgKind::Slot),
                "send" if index >= 2 => Some(ArgKind::String),
                "retry" | "dead_letter" | "retain" if index < 2 => {
                    if index == 0 {
                        Some(ArgKind::Slot)
                    } else {
                        Some(ArgKind::String)
                    }
                }
                "commit" if index == 0 => Some(ArgKind::Slot),
                "commit" if index == 1 => Some(ArgKind::String),
                _ => None,
            },
            SYNC_MODULE => match method {
                "text_new" if index < 2 => Some(ArgKind::String),
                "text_set" if index == 0 => Some(ArgKind::Slot),
                "text_set" if index > 0 => Some(ArgKind::String),
                "text_merge" if index < 2 => Some(ArgKind::Slot),
                "text_show" | "text_metadata" if index == 0 => Some(ArgKind::Slot),
                "text_edit" if index == 0 => Some(ArgKind::Slot),
                "text_edit" if index == 1 || index == 4 => Some(ArgKind::String),
                "text_edit" if index == 2 || index == 3 => Some(ArgKind::Int),
                "counter_new" if index == 0 => Some(ArgKind::String),
                "counter_new" if index == 1 => Some(ArgKind::Int),
                "counter_inc" if index == 0 => Some(ArgKind::Slot),
                "counter_inc" if index == 1 => Some(ArgKind::String),
                "counter_inc" if index == 2 => Some(ArgKind::Int),
                "counter_merge" if index < 2 => Some(ArgKind::Slot),
                "counter_value" if index == 0 => Some(ArgKind::Slot),
                "map_set" if index == 0 => Some(ArgKind::Slot),
                "map_set" if index > 0 => Some(ArgKind::String),
                "map_get" if index == 0 => Some(ArgKind::Slot),
                "map_get" if index == 1 => Some(ArgKind::String),
                "map_merge" if index < 2 => Some(ArgKind::Slot),
                "map_show" if index == 0 => Some(ArgKind::Slot),
                "list_push" if index == 0 => Some(ArgKind::Slot),
                "list_push" if index > 0 => Some(ArgKind::String),
                "list_merge" if index < 2 => Some(ArgKind::Slot),
                "list_show" if index == 0 => Some(ArgKind::Slot),
                "policy_new" if index < 2 => Some(ArgKind::String),
                "policy_allows" if index == 0 => Some(ArgKind::Slot),
                "policy_allows" if index > 0 => Some(ArgKind::String),
                "policy_show" if index == 0 => Some(ArgKind::Slot),
                "sync_over" | "sync" if index < 2 => Some(ArgKind::String),
                "map_new" | "list_new" => None,
                _ => None,
            },
            _ => None,
        }
    }

    fn service_value(rt: &JitRuntime, handle: i64) -> Option<CtValue> {
        let index = usize::try_from(handle).ok()?;
        rt.service_values.get(index)?.as_ref().cloned()
    }

    fn remember_service_value(rt: &mut JitRuntime, handle: i64, value: CtValue) {
        let Ok(index) = usize::try_from(handle) else {
            return;
        };
        if rt.service_values.len() <= index {
            rt.service_values.resize_with(index + 1, || None);
        }
        rt.service_values[index] = Some(value);
    }

    fn replace_service_value(rt: &mut JitRuntime, handle: i64, value: CtValue) {
        let Ok(index) = usize::try_from(handle) else {
            return;
        };
        if index < rt.service_values.len() {
            rt.service_values[index] = Some(value.clone());
            refresh_service_record(rt, handle, &value);
        }
    }

    fn convert_arg(rt: &JitRuntime, kind: ArgKind, raw: i64) -> Option<CtValue> {
        match kind {
            ArgKind::String => rt.heap.clone_string(raw).map(CtValue::Str),
            ArgKind::Int => Some(CtValue::Int(raw)),
            ArgKind::StringList => {
                let length = rt.heap.list_len(raw)?;
                if length < 0 {
                    return None;
                }
                let mut values = Vec::with_capacity(usize::try_from(length).ok()?);
                for index in 0..length {
                    let handle = rt.heap.list_get_int(raw, index)?;
                    values.push(CtValue::Str(rt.heap.clone_string(handle)?));
                }
                Some(CtValue::List(values))
            }
            ArgKind::Slot => service_value(rt, raw),
            ArgKind::Restart => Some(CtValue::Enum {
                type_name: "ServiceRestart".to_string(),
                variant: match raw {
                    0 => "OneForOne",
                    1 => "OneForAll",
                    2 => "RestForOne",
                    _ => return None,
                }
                .to_string(),
                args: Vec::new(),
            }),
            ArgKind::Delivery => Some(CtValue::Enum {
                type_name: "ServiceDelivery".to_string(),
                variant: match raw {
                    0 => "AtMostOnce",
                    1 => "DurableAtLeastOnce",
                    _ => return None,
                }
                .to_string(),
                args: Vec::new(),
            }),
            ArgKind::DurationNs => Some(CtValue::Struct {
                type_name: "Duration".to_string(),
                fields: vec![("ns".to_string(), CtValue::Int(raw))],
            }),
        }
    }

    fn set_record_field(rt: &mut JitRuntime, record: i64, index: usize, value: &CtValue) {
        let Ok(index) = i64::try_from(index) else {
            return;
        };
        match value {
            CtValue::Int(value) => {
                let _ = rt.heap.record_set_int(record, index, *value);
            }
            CtValue::Float(value) => {
                let _ = rt.heap.record_set_float(record, index, value.as_f64());
            }
            CtValue::Bool(value) => {
                let _ = rt.heap.record_set_bool(record, index, *value);
            }
            CtValue::Char(value) => {
                let _ = rt.heap.record_set_char(record, index, *value);
            }
            CtValue::Str(value) => {
                let handle = rt.heap.alloc_string(value.clone());
                let _ = rt.heap.record_set_string(record, index, handle);
            }
            CtValue::Present(value) => {
                let bits = marshal_scalar(rt, value).wrapping_add(1);
                let _ = rt.heap.record_set_int(record, index, bits);
            }
            CtValue::Failed(CtReport::Clean(_)) => {
                let _ = rt.heap.record_set_int(record, index, 0);
            }
            CtValue::Failed(CtReport::Told(value)) => {
                let bits = marshal_scalar(rt, value);
                let _ = rt.heap.record_set_int(record, index, bits);
            }
            value => {
                let bits = marshal_scalar(rt, value);
                let _ = rt.heap.record_set_int(record, index, bits);
            }
        }
    }

    fn refresh_service_record(rt: &mut JitRuntime, record: i64, value: &CtValue) {
        let CtValue::Struct { fields, .. } = value else {
            return;
        };
        for (index, (_, value)) in fields.iter().enumerate() {
            set_record_field(rt, record, index, value);
        }
    }

    fn marshal_list(rt: &mut JitRuntime, values: &[CtValue]) -> i64 {
        let list = rt.heap.alloc_empty_list();
        for value in values {
            let bits = marshal_scalar(rt, value);
            let _ = rt.heap.list_push_int(list, bits);
        }
        list
    }

    fn marshal_struct(
        rt: &mut JitRuntime,
        type_name: &str,
        fields: &[(String, CtValue)],
    ) -> i64 {
        let record = rt.heap.alloc_record(fields.len());
        remember_service_value(
            rt,
            record,
            CtValue::Struct {
                type_name: type_name.to_string(),
                fields: fields.to_vec(),
            },
        );
        for (index, (_, value)) in fields.iter().enumerate() {
            set_record_field(rt, record, index, value);
        }
        record
    }

    fn enum_discriminant(type_name: &str, variant: &str) -> Option<i64> {
        let variants: &[&str] = match type_name {
            "ServiceReceipt" => &[
                "Accepted",
                "Duplicate",
                "Retained",
                "DeadLettered",
                "Rejected",
                "Unavailable",
            ],
            "ServiceError" => &[
                "Full",
                "Ambiguous",
                "Unknown",
                "NotStarted",
                "Policy",
                "Unavailable",
                "Partitioned",
                "Revoked",
                "Stale",
                "Expired",
            ],
            "ServiceRestart" => &["OneForOne", "OneForAll", "RestForOne"],
            "ServiceDelivery" => &["AtMostOnce", "DurableAtLeastOnce"],
            "ServiceStateAdapter" => &["Empty", "Snapshot", "EventLog"],
            _ => return None,
        };
        variants
            .iter()
            .position(|name| *name == variant)
            .and_then(|index| i64::try_from(index).ok())
    }

    fn marshal_enum(
        rt: &mut JitRuntime,
        type_name: &str,
        variant: &str,
        args: &[(Option<String>, CtValue)],
    ) -> i64 {
        let Some(discriminant) = enum_discriminant(type_name, variant) else {
            return 0;
        };
        if args.len() <= 1 {
            let payload = args
                .first()
                .map(|(_, value)| marshal_scalar(rt, value))
                .unwrap_or(0);
            return payload.wrapping_shl(8) | discriminant;
        }
        let record = rt.heap.alloc_record(args.len() + 1);
        let _ = rt.heap.record_set_int(record, 0, discriminant);
        for (index, (_, value)) in args.iter().enumerate() {
            let field = i64::try_from(index + 1).unwrap_or(i64::MAX);
            match value {
                CtValue::Int(value) => {
                    let _ = rt.heap.record_set_int(record, field, *value);
                }
                CtValue::Float(value) => {
                    let _ = rt.heap.record_set_float(record, field, value.as_f64());
                }
                CtValue::Bool(value) => {
                    let _ = rt.heap.record_set_bool(record, field, *value);
                }
                CtValue::Char(value) => {
                    let _ = rt.heap.record_set_char(record, field, *value);
                }
                CtValue::Str(value) => {
                    let handle = rt.heap.alloc_string(value.clone());
                    let _ = rt.heap.record_set_string(record, field, handle);
                }
                other => {
                    let bits = marshal_scalar(rt, other);
                    let _ = rt.heap.record_set_int(record, field, bits);
                }
            }
        }
        record
    }

    fn marshal_scalar(rt: &mut JitRuntime, value: &CtValue) -> i64 {
        match value {
            CtValue::Int(value) => *value,
            CtValue::Float(value) => value.to_bits_i64(),
            CtValue::Bool(value) => i64::from(*value),
            CtValue::Char(value) => i64::from(*value as u32),
            CtValue::Str(value) => rt.heap.alloc_string(value.clone()),
            CtValue::Enum {
                type_name,
                variant,
                args,
            } => marshal_enum(rt, type_name, variant, args),
            CtValue::Present(value) => marshal_scalar(rt, value),
            CtValue::Failed(_) | CtValue::Unit => 0,
            CtValue::Bytes(values) => marshal_list(
                rt,
                &values.iter().map(|value| CtValue::Int(i64::from(*value))).collect::<Vec<_>>(),
            ),
            CtValue::List(values) => marshal_list(rt, values),
            CtValue::Struct { type_name, fields } => marshal_struct(rt, type_name, fields),
            CtValue::BigInt(_) | CtValue::Map(_) | CtValue::Closure(_) => 0,
        }
    }

    fn marshal_result(rt: &mut JitRuntime, value: CtValue) -> i64 {
        match value {
            CtValue::Present(value) => {
                let bits = marshal_scalar(rt, &value);
                alloc_jit_result(rt, true, bits as u64)
            }
            CtValue::Failed(CtReport::Told(value)) => {
                let bits = marshal_scalar(rt, &value);
                alloc_jit_result(rt, false, bits as u64)
            }
            CtValue::Failed(CtReport::Clean(_)) => alloc_jit_result(rt, false, 0),
            value => marshal_scalar(rt, &value),
        }
    }

    fn marshal_option(rt: &mut JitRuntime, value: CtValue) -> i64 {
        match value {
            CtValue::Present(value) => marshal_scalar(rt, &value).wrapping_add(1),
            CtValue::Failed(CtReport::Clean(_)) => 0,
            _ => 0,
        }
    }

    fn diagnostic_value(diagnostic: jet_foundation::Diagnostics::Diagnostic) -> CtValue {
        CtValue::failed(Box::new(CtValue::Str(format!(
            "{}: {}",
            diagnostic.code, diagnostic.what
        ))))
    }

    fn apply_services_mutation(rt: &mut JitRuntime, raw_args: &[i64], value: CtValue) -> CtValue {
        match service_prelude::services_take_mut(value) {
            Ok((tree, value)) => {
                if !matches!(tree, CtValue::Unit) {
                    if let Some(handle) = raw_args.first() {
                        replace_service_value(rt, *handle, tree);
                    }
                }
                value
            }
            Err(value) => value,
        }
    }

    fn call_ct(rt: &mut JitRuntime, module: i64, method: &str, raw_args: &[i64]) -> CtValue {
        let span = Span::new(0, 0);
        if module == SERVICES_MODULE && method == "runtime" {
            let Some(store) = raw_args.first().and_then(|handle| rt.heap.clone_string(*handle))
            else {
                return CtValue::failed(Box::new(CtValue::Str(
                    "core.services.runtime expects a store path".to_string(),
                )));
            };
            let Some(retention_ns) = raw_args.get(1).copied() else {
                return CtValue::failed(Box::new(CtValue::Str(
                    "core.services.runtime expects a Duration".to_string(),
                )));
            };
            return service_prelude::service_runtime(store, retention_ns / 1_000_000);
        }

        let mut args = Vec::with_capacity(raw_args.len());
        for (index, raw) in raw_args.iter().copied().enumerate() {
            let Some(kind) = arg_kind(module, method, index) else {
                return CtValue::failed(Box::new(CtValue::Str(format!(
                    "unsupported service adapter call: {module}.{method}"
                ))));
            };
            let Some(value) = convert_arg(rt, kind, raw) else {
                return CtValue::failed(Box::new(CtValue::Str(format!(
                    "invalid service adapter argument: {module}.{method}[{index}]"
                ))));
            };
            args.push(value);
        }

        let result = match module {
            SERVICES_MODULE => service_prelude::services_apply(method, &args, span),
            SYNC_MODULE => service_prelude::sync_apply(method, &args, span),
            SERVICE_AUTHORITY_MODULE => {
                let Some((receiver, args)) = args.split_first() else {
                    return CtValue::failed(Box::new(CtValue::Str(
                        "ServiceRuntime receiver is missing".to_string(),
                    )));
                };
                service_prelude::services_runtime_apply(receiver, method, args, span)
            }
            _ => {
                return CtValue::failed(Box::new(CtValue::Str(
                    "unknown service adapter module".to_string(),
                )))
            }
        };
        let value = match result {
            Ok(value) => value,
            Err(diagnostic) => diagnostic_value(diagnostic),
        };
        if module == SERVICES_MODULE {
            apply_services_mutation(rt, raw_args, value)
        } else {
            value
        }
    }

    pub(super) fn call(
        rt: &mut JitRuntime,
        module: i64,
        method_handle: i64,
        argc: i64,
        raw: [i64; 7],
    ) -> i64 {
        let Some(method) = rt.heap.clone_string(method_handle) else {
            return 0;
        };
        let Some(count) = usize::try_from(argc).ok().filter(|count| *count <= raw.len()) else {
            return 0;
        };
        let value = call_ct(rt, module, &method, &raw[..count]);
        if module == SYNC_MODULE && method == "map_get" {
            marshal_option(rt, value)
        } else if module == SERVICES_MODULE && method == "runtime" {
            marshal_scalar(rt, &value)
        } else {
            marshal_result(rt, value)
        }
    }

    pub(super) fn call_bool(
        rt: &mut JitRuntime,
        module: i64,
        method_handle: i64,
        argc: i64,
        raw: [i64; 7],
    ) -> i8 {
        call(rt, module, method_handle, argc, raw) as i8
    }

    pub(super) fn show(rt: &mut JitRuntime, handle: i64) -> i64 {
        let Some(value) = service_value(rt, handle) else {
            rt.set_trap("the JIT received an invalid service value handle");
            return 0;
        };
        let Some(rendered) = service_prelude::service_display(&value) else {
            rt.set_trap("the JIT received an unsupported service display value");
            return 0;
        };
        rt.heap.alloc_string(rendered)
    }
}

extern "C" fn jet_jit_service_call(
    module: i64,
    method: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        service_adapter::call(rt, module, method, argc, [a0, a1, a2, a3, a4, a5, a6])
    })
}

extern "C" fn jet_jit_service_call_bool(
    module: i64,
    method: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        service_adapter::call_bool(rt, module, method, argc, [a0, a1, a2, a3, a4, a5, a6])
    })
}

extern "C" fn jet_jit_service_show(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| service_adapter::show(rt, handle))
}

pub(crate) fn alloc_io_error_result(
    rt: &mut JitRuntime,
    variant: i64,
    operation: i64,
    resource: Option<&str>,
    cause: &str,
) -> i64 {
    let context = rt.heap.alloc_record(4);
    let _ = rt
        .heap
        .record_set_int(context, 0, operation);
    let resource = resource
        .map(|value| rt.heap.alloc_string(value.to_string()).wrapping_add(1))
        .unwrap_or(0);
    let _ = rt.heap.record_set_int(context, 1, resource);
    let _ = rt.heap.record_set_int(context, 2, 0);
    let cause = rt.heap.alloc_string(cause.to_string()).wrapping_add(1);
    let _ = rt.heap.record_set_int(context, 3, cause);
    alloc_jit_result(
        rt,
        false,
        (context as u64).wrapping_shl(8) | variant as u64,
    )
}

pub(crate) fn result_err_terminal(error: crate::IO::term_prelude::JetTermSecretError) -> i64 {
    let cause = error.message();
    let (variant_name, operation_name, resource) = match error {
        crate::IO::term_prelude::JetTermSecretError::NonTerminal => {
            ("InvalidInput", "Read", Some("stdin"))
        }
        crate::IO::term_prelude::JetTermSecretError::Echo => {
            ("Other", "Read", Some("stdin"))
        }
        crate::IO::term_prelude::JetTermSecretError::Flush(_) => ("Other", "Flush", Some("stdout")),
        crate::IO::term_prelude::JetTermSecretError::Read(_) => ("Other", "Read", Some("stdin")),
    };
    let variant = jet_foundation::Syntax::IO_ERROR_VARIANTS
        .iter()
        .position(|name| *name == variant_name)
        .expect("Prelude IOError variants must be registered") as i64;
    let operation = jet_foundation::Syntax::IO_OPERATION_VARIANTS
        .iter()
        .position(|name| *name == operation_name)
        .expect("Prelude IOOperation variants must be registered") as i64;
    Concurrency::with_runtime_mut(|rt| {
        alloc_io_error_result(rt, variant, operation, resource, &cause)
    })
}
pub(crate) fn jit_result(rt: &JitRuntime, handle: i64) -> Option<JitResultValue> {
    usize::try_from(handle)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| rt.results.get(index).copied())
}

pub(crate) fn jit_result_i64(rt: &JitRuntime, handle: i64) -> Option<i64> {
    jit_result(rt, handle).map(|result| result.bits as i64)
}

pub(crate) fn jit_result_is_ok(rt: &JitRuntime, handle: i64) -> Option<bool> {
    jit_result(rt, handle).map(|result| result.ok)
}

extern "C" fn jet_jit_result_new_i64(ok: i8, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value as u64))
}

extern "C" fn jet_jit_duration_from_int(value: i64, scale: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match duration_kernel::jet_duration_kernel_from_int(value, scale) {
        Some(ms) => alloc_jit_result(rt, true, ms as u64),
        None => alloc_jit_result(rt, false, 0),
    })
}

extern "C" fn jet_jit_duration_from_float(value: f64, scale: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        match duration_kernel::jet_duration_kernel_from_float(value, scale) {
            Some(value) => alloc_jit_result(rt, true, value as u64),
            None => alloc_jit_result(rt, false, 0),
        }
    })
}

extern "C" fn jet_jit_duration_in(value: i64, scale: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        alloc_jit_result(
            rt,
            true,
            duration_kernel::jet_duration_kernel_in(value, scale) as u64,
        )
    })
}

extern "C" fn jet_jit_duration_in_unit(value: i64, unit: i64) -> i64 {
    // DurationUnit disc order matches Prelude CommonTypes.
    let scale = match unit {
        0 => 1i64,                     // Nanoseconds
        1 => 1_000,                    // Microseconds
        2 => 1_000_000,                // Milliseconds
        3 => 1_000_000_000,            // Seconds
        4 => 60_000_000_000,           // Minutes
        5 => 3_600_000_000_000,        // Hours
        _ => 1,
    };
    jet_jit_duration_in(value, scale)
}

extern "C" fn jet_jit_duration_is_zero(value: i64) -> i8 {
    i8::from(duration_kernel::jet_duration_kernel_is_zero(value))
}

extern "C" fn jet_jit_duration_total_seconds(value: i64) -> i64 {
    duration_kernel::jet_duration_kernel_total_seconds(value)
}

extern "C" fn jet_jit_duration_difference(a: i64, b: i64) -> i64 {
    duration_kernel::jet_duration_kernel_difference(a, b)
}

extern "C" fn jet_jit_result_new_f64(ok: i8, value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value.to_bits()))
}

fn jit_callable_index(handle: i64) -> Option<usize> {
    handle
        .checked_neg()?
        .checked_sub(1)
        .and_then(|index| usize::try_from(index).ok())
}

fn jit_callable_slot(rt: &JitRuntime, handle: i64) -> Option<JitCallableSlot> {
    jit_callable_index(handle).and_then(|index| rt.jit_callables.get(index).copied())
}

pub(crate) fn register_jit_atexit(handler: i64) -> bool {
    Concurrency::with_runtime_mut(|rt| {
        let Some(slot) = jit_callable_slot(rt, handler) else {
            rt.set_trap("invalid resident atexit callback");
            return false;
        };
        rt.atexit_handlers.push(slot);
        true
    })
}

/// Invoke the callbacks in registration order. The generated function value
/// ABI is either `extern "C" fn()` or `extern "C" fn(i64)` when it carries
/// the resident environment handle.
pub(crate) fn run_jit_atexit_handlers(rt: &mut JitRuntime) {
    let handlers = std::mem::take(&mut rt.atexit_handlers);
    for handler in handlers {
        // SAFETY: `fn_ptr`, `env`, and `has_env` are written together by the
        // checked JIT callable binder. The callback signature is the zero-arg
        // `atexit` signature, with the environment word prepended only for a
        // captured closure.
        unsafe {
            if handler.has_env {
                let callback: extern "C" fn(i64) = std::mem::transmute(handler.fn_ptr as usize);
                callback(handler.env);
            } else {
                let callback: extern "C" fn() = std::mem::transmute(handler.fn_ptr as usize);
                callback();
            }
        }
    }
}

fn bind_jit_callable(rt: &mut JitRuntime, fn_ptr: i64, env: i64, has_env: bool) -> i64 {
    let index = rt.jit_callables.len();
    if index >= i64::MAX as usize - 1 {
        rt.set_trap("too many resident callable values");
        return 0;
    }
    rt.jit_callables.push(JitCallableSlot {
        fn_ptr,
        env,
        has_env,
    });
    -(index as i64) - 1
}

extern "C" fn jet_jit_callable_bind(fn_ptr: i64, env: i64, has_env: i8) -> i64 {
    with_runtime_result(0, |rt| bind_jit_callable(rt, fn_ptr, env, has_env != 0))
}

extern "C" fn jet_jit_callable_normalize(value: i64) -> i64 {
    with_runtime_result(0, |rt| {
        if jit_callable_slot(rt, value).is_some() {
            value
        } else {
            bind_jit_callable(rt, value, 0, false)
        }
    })
}

fn jit_callable_or_trap(rt: &mut JitRuntime, handle: i64) -> Option<JitCallableSlot> {
    let slot = jit_callable_slot(rt, handle);
    if slot.is_none() {
        rt.set_trap("invalid resident callable value");
    }
    slot
}

extern "C" fn jet_jit_callable_fn(handle: i64) -> i64 {
    with_runtime_result(0, |rt| jit_callable_or_trap(rt, handle).map_or(0, |slot| slot.fn_ptr))
}

extern "C" fn jet_jit_callable_env(handle: i64) -> i64 {
    with_runtime_result(0, |rt| jit_callable_or_trap(rt, handle).map_or(0, |slot| slot.env))
}

extern "C" fn jet_jit_callable_has_env(handle: i64) -> i8 {
    with_runtime_result(0, |rt| {
        jit_callable_or_trap(rt, handle).map_or(0, |slot| i8::from(slot.has_env))
    })
}

/// The JIT-side callable ABI is deliberately opaque to the Prelude. The
/// factory evaluates the function-value expression, and the adapter invokes
/// that callable with two packed payload words. The `JetOptionPacked` values
/// are only the JIT ABI carrier; the shared `jet_option_lift2` operation owns
/// presence, lazy factory creation, invocation, and result selection.
type OptionLift2Factory = unsafe extern "C" fn(i64) -> i64;
type OptionLift2Adapter = unsafe extern "C" fn(i64, i64, i64) -> i64;

extern "C" fn jet_jit_option_lift2(
    a_present: i8,
    a_value: i64,
    b_present: i8,
    b_value: i64,
    factory: i64,
    env: i64,
    adapter: i64,
) -> i64 {
    jet_codegen::option_lift2::jet_option_lift2(
        jet_codegen::option_lift2::JetOptionPacked {
            present: a_present != 0,
            value: a_value,
        },
        jet_codegen::option_lift2::JetOptionPacked {
            present: b_present != 0,
            value: b_value,
        },
        || jet_codegen::option_lift2::jet_option_pack_i64(false, 0),
        |value| jet_codegen::option_lift2::jet_option_pack_i64(true, value),
        || {
            let factory: OptionLift2Factory =
                unsafe { std::mem::transmute(factory as usize) };
            let adapter: OptionLift2Adapter =
                unsafe { std::mem::transmute(adapter as usize) };
            let callable = unsafe { factory(env) };
            move |left, right| unsafe { adapter(callable, left, right) }
        },
    )
}

extern "C" fn jet_jit_unit_convert_exact(
    value: f64,
    scale_num: i64,
    scale_den: i64,
    offset_num: i64,
    offset_den: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let ratios = [scale_num, scale_den, offset_num, offset_den]
            .map(|id| rt.heap.get_string(id).map(str::to_owned));
        let converted = match &ratios {
            [Some(scale_num), Some(scale_den), Some(offset_num), Some(offset_den)] => {
                jet_foundation::jet_unit_conversion_exact(
                    value,
                    scale_num,
                    scale_den,
                    offset_num,
                    offset_den,
                )
            }
            _ => None,
        };
        if let Some(converted) = converted {
            alloc_jit_result(rt, true, converted.to_bits())
        } else {
            let error = rt.heap.alloc_string("unit conversion would round");
            alloc_jit_result(rt, false, error as u64)
        }
    })
}

extern "C" fn jet_jit_unit_convert_rounded(
    value: f64,
    scale_num: i64,
    scale_den: i64,
    offset_num: i64,
    offset_den: i64,
    mode: i64,
    digits: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let ratios = [scale_num, scale_den, offset_num, offset_den]
            .map(|id| rt.heap.get_string(id).map(str::to_owned));
        let mode = match mode {
            0 => Some(jet_foundation::UnitRoundingMode::TowardZero),
            1 => Some(jet_foundation::UnitRoundingMode::Floor),
            2 => Some(jet_foundation::UnitRoundingMode::Ceiling),
            3 => Some(jet_foundation::UnitRoundingMode::NearestEven),
            _ => None,
        };
        let converted = match (&ratios, mode) {
            ([Some(scale_num), Some(scale_den), Some(offset_num), Some(offset_den)], Some(mode)) => {
                jet_foundation::jet_unit_conversion_rounded(
                    value,
                    scale_num,
                    scale_den,
                    offset_num,
                    offset_den,
                    mode,
                    digits,
                )
            }
            _ => Err(jet_foundation::UNIT_ROUNDING_UNREPRESENTABLE),
        };
        match converted {
            Ok(converted) => alloc_jit_result(rt, true, converted.to_bits()),
            Err(message) => {
                let error = rt.heap.alloc_string(message);
                alloc_jit_result(rt, false, error as u64)
            }
        }
    })
}

extern "C" fn jet_jit_unit_convert_implicit(
    value: f64,
    scale_num: i64,
    scale_den: i64,
    offset_num: i64,
    offset_den: i64,
) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        let ratios = [scale_num, scale_den, offset_num, offset_den]
            .map(|id| rt.heap.get_string(id).map(str::to_owned));
        let converted = match &ratios {
            [Some(scale_num), Some(scale_den), Some(offset_num), Some(offset_den)] => {
                jet_foundation::jet_unit_conversion_exact(
                    value,
                    scale_num,
                    scale_den,
                    offset_num,
                    offset_den,
                )
            }
            _ => None,
        };
        match converted {
            Some(converted) => converted,
            None => {
                rt.set_trap("unit conversion would round");
                0.0
            }
        }
    })
}

extern "C" fn jet_jit_result_new_i8(ok: i8, value: i8) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value as u8 as u64))
}

extern "C" fn jet_jit_result_new_i32(ok: i8, value: i32) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value as u32 as u64))
}

extern "C" fn jet_jit_result_is_ok(handle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| i8::from(jit_result(rt, handle).is_some_and(|r| r.ok)))
}

extern "C" fn jet_jit_result_get_i64(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| jit_result(rt, handle).map_or(0, |r| r.bits as i64))
}

extern "C" fn jet_jit_result_get_f64(handle: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        f64::from_bits(jit_result(rt, handle).map_or(0, |r| r.bits))
    })
}

extern "C" fn jet_jit_result_get_i8(handle: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| jit_result(rt, handle).map_or(0, |r| r.bits as i8))
}

extern "C" fn jet_jit_result_get_i32(handle: i64) -> i32 {
    Concurrency::with_runtime_mut(|rt| jit_result(rt, handle).map_or(0, |r| r.bits as i32))
}

extern "C" fn jet_jit_perf_fidelity() -> f64 {
    f32::from_bits(JIT_PERF_FIDELITY.with(std::cell::Cell::get)) as f64
}

extern "C" fn jet_jit_perf_default_fidelity() -> f64 {
    f32::from_bits(JIT_PERF_DEFAULT_FIDELITY_BITS) as f64
}

extern "C" fn jet_jit_perf_override_fidelity(value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            let message = format!(
                "core.perf.Perf.override_fidelity needs 0.0 through 1.0, got {}",
                value
            );
            let string = rt.heap.alloc_string(message);
            return alloc_jit_result(rt, false, string as u64);
        }
        JIT_PERF_FIDELITY.with(|c| c.set((value as f32).to_bits()));
        alloc_jit_result(rt, true, 0)
    })
}

extern "C" fn jet_jit_perf_reset_fidelity() {
    JIT_PERF_FIDELITY.with(|c| c.set(JIT_PERF_DEFAULT_FIDELITY_BITS));
}

pub(crate) fn new_jit_module() -> Result<(JITModule, HostFns), String> {
    let mut builder =
        JITBuilder::new(cranelift_module::default_libcall_names()).map_err(|e| e.to_string())?;
    register_host_symbols(&mut builder);
    Collections::register_collections_symbols(&mut builder);
    Compute::register_compute_symbols(&mut builder);
    Memory::register_memory_symbols(&mut builder);
    LocalCell::register_symbols(&mut builder);
    Concurrency::register_concurrency_symbols(&mut builder);
    CoreHost::register_core_host_symbols(&mut builder);
    Encoding::register_encoding_symbols(&mut builder);
    crate::enc_stream::register_stream_symbols(&mut builder);
    Fmt::register_fmt_symbols(&mut builder);
    Compress::register_compress_symbols(&mut builder);
    Archive::register_archive_symbols(&mut builder);
    Process::register_process_symbols(&mut builder);
    Numeric::register_numeric_symbols(&mut builder);
    Solver::register_solver_symbols(&mut builder);
    Random::register_random_symbols(&mut builder);
    crate::Text::register_text_symbols(&mut builder);
    crate::Sketch::register_sketch_symbols(&mut builder);
    crate::Args::register_args_symbols(&mut builder);
    crate::DB::register_db_symbols(&mut builder);
    Crypto::register_crypto_symbols(&mut builder);
    Net::register_net_symbols(&mut builder);
    crate::net_http_rt::register_net_http_symbols(&mut builder);
    crate::Game::register_game_symbols(&mut builder);
    crate::Raylib::register_raylib_symbols(&mut builder);
    crate::Layout::register_layout_symbols(&mut builder);
    crate::Reactive::register_reactive_symbols(&mut builder);
    crate::Ui::register_ui_symbols(&mut builder);
    crate::Web::register_web_symbols(&mut builder);
    crate::Parse::register_symbols(&mut builder);
    crate::Data::register_symbols(&mut builder);
    crate::Time::register_time_symbols(&mut builder);
    crate::IO::register_io_symbols(&mut builder);
    crate::Watcher::register_watcher_symbols(&mut builder);
    crate::Net::register_net_symbols(&mut builder);
    crate::Math::register_math_host_symbols(&mut builder);
    crate::MathExtra::register_math_extra_symbols(&mut builder);
    crate::Ffi::register_ffi_host_symbols(&mut builder);
    let mut module = JITModule::new(builder);
    let coll = Collections::declare_collections_host_fns(&mut module)?;
    let compute = Compute::declare_compute_host_fns(&mut module)?;
    let memory = Memory::declare_memory_host_fns(&mut module)?;
    let cell = LocalCell::declare_host_fns(&mut module)?;
    let conc = Concurrency::declare_concurrency_host_fns(&mut module)?;
    let core = CoreHost::declare_core_host_fns(&mut module)?;
    let encoding = Encoding::declare_encoding_host_fns(&mut module)?;
    let stream = crate::enc_stream::declare_stream_host_fns(&mut module)?;
    let fmt = Fmt::declare_fmt_host_fns(&mut module)?;
    let compress = Compress::declare_compress_host_fns(&mut module)?;
    let archive = Archive::declare_archive_host_fns(&mut module)?;
    let process = Process::declare_process_host_fns(&mut module)?;
    let num = Numeric::declare_numeric_host_fns(&mut module)?;
    let solver = Solver::declare_solver_host_fns(&mut module)?;
    let random = Random::declare_random_host_fns(&mut module)?;
    let text = crate::Text::declare_text_host_fns(&mut module)?;
    let sketch = crate::Sketch::declare_sketch_host_fns(&mut module)?;
    let args = crate::Args::declare_args_host_fns(&mut module)?;
    let db = crate::DB::declare_db_host_fns(&mut module)?;
    let crypto = Crypto::declare_crypto_host_fns(&mut module)?;
    let net = Net::declare_net_host_fns(&mut module)?;
    let net_http = crate::net_http_rt::declare_net_http_host_fns(&mut module)?;
    let game = crate::Game::declare_game_host_fns(&mut module)?;
    let raylib = crate::Raylib::declare_raylib_host_fns(&mut module)?;
    let layout = crate::Layout::declare_layout_host_fns(&mut module)?;
    let reactive = crate::Reactive::declare_reactive_host_fns(&mut module)?;
    let ui = crate::Ui::declare_ui_host_fns(&mut module)?;
    let web = crate::Web::declare_web_host_fns(&mut module)?;
    let parse = crate::Parse::declare(&mut module)?;
    let data = crate::Data::declare(&mut module)?;
    let time = crate::Time::declare_time_host_fns(&mut module)?;
    let io = crate::IO::declare_io_host_fns(&mut module)?;
    let watcher = crate::Watcher::declare_watcher_host_fns(&mut module)?;
    let math = crate::Math::declare_math_host_fns(&mut module)?;
    let math_extra = crate::MathExtra::declare_math_extra_host_fns(&mut module)?;
    let ffi = crate::Ffi::declare_ffi_host_fns(&mut module)?;
    let host = declare_host_fns(
        &mut module,
        coll,
        compute,
        memory,
        cell,
        conc,
        core,
        encoding,
        stream,
        fmt,
        compress,
        archive,
        process,
        num,
        solver,
        random,
        text,
        sketch,
        args,
        db,
        crypto,
        net,
        net_http,
        game,
        raylib,
        layout,
        reactive,
        ui,
        web,
        parse,
        data,
        time,
        io,
        watcher,
        math,
        math_extra,
        ffi,
    )?;
    Ok((module, host))
}


extern "C" fn jet_jit_reflect_of_finish(
    type_name: i64,
    path: i64,
    display: i64,
    fields: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let type_name = rt.heap.clone_string(type_name).unwrap_or_default();
        let path = rt.heap.clone_string(path).unwrap_or_default();
        let display = rt.heap.clone_string(display).unwrap_or_default();
        let field_len = rt.heap.list_len(fields).unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..field_len {
            let fh = rt.heap.list_get_int(fields, i).unwrap_or(0);
            let idx = (fh as usize).wrapping_sub(1);
            if let Some(slot) = rt.reflect_values.get(idx) {
                if let Some(name) = &slot.field_name {
                    out.push((name.clone(), fh));
                }
            }
        }
        rt.reflect_values.push(ReflectSlot {
            field_name: None,
            type_name,
            path,
            display,
            fields: out,
        });
        rt.reflect_values.len() as i64
    })
}

extern "C" fn jet_jit_reflect_field_new(
    name: i64,
    type_name: i64,
    path: i64,
    display: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let type_name = rt.heap.clone_string(type_name).unwrap_or_default();
        let path = rt.heap.clone_string(path).unwrap_or_default();
        let display = rt.heap.clone_string(display).unwrap_or_default();
        rt.reflect_values.push(ReflectSlot {
            field_name: Some(name),
            type_name,
            path,
            display,
            fields: Vec::new(),
        });
        rt.reflect_values.len() as i64
    })
}

extern "C" fn jet_jit_reflect_type_name(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let text = rt
            .reflect_values
            .get(idx)
            .map(|s| s.type_name.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_reflect_display(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let text = rt
            .reflect_values
            .get(idx)
            .map(|s| s.display.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_reflect_path(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let text = rt
            .reflect_values
            .get(idx)
            .map(|s| s.path.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_reflect_fields(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let fields = rt
            .reflect_values
            .get(idx)
            .map(|s| s.fields.clone())
            .unwrap_or_default();
        let mut ids = Vec::new();
        for (name, value) in fields {
            let value_idx = (value as usize).wrapping_sub(1);
            let Some(value_slot) = rt.reflect_values.get(value_idx).cloned() else {
                continue;
            };
            rt.reflect_values.push(ReflectSlot {
                field_name: Some(name),
                type_name: value_slot.type_name,
                path: value_slot.path,
                display: value_slot.display,
                fields: Vec::new(),
            });
            ids.push(rt.reflect_values.len() as i64);
        }
        rt.heap.alloc_int_list(ids)
    })
}

extern "C" fn jet_jit_reflect_field_name(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        let text = rt
            .reflect_values
            .get(idx)
            .and_then(|slot| slot.field_name.clone())
            .unwrap_or_default();
        rt.heap.alloc_string(text)
    })
}

extern "C" fn jet_jit_reflect_field_value(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).wrapping_sub(1);
        if rt.reflect_values.get(idx).is_some() {
            handle
        } else {
            0
        }
    })
}

extern "C" fn jet_jit_testing_temp_dir(prefix: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let prefix = rt.heap.clone_string(prefix).unwrap_or_else(|| "jet".into());
        rt.heap
            .alloc_string(crate::testing_shared::jet_testing_temp_dir_path(&prefix))
    })
}

extern "C" fn jet_jit_testing_snap(name: i64, actual: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let actual = rt.heap.clone_string(actual).unwrap_or_default();
        let safe: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .collect();
        let path = std::path::Path::new("__snapshots__").join(format!("{safe}.snap"));
        let update = std::env::var("JET_UPDATE_SNAPSHOTS").ok().as_deref() == Some("1");
        if update || !path.is_file() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            return i8::from(std::fs::write(&path, actual).is_ok());
        }
        i8::from(
            std::fs::read_to_string(path)
                .map(|s| s == actual)
                .unwrap_or(false),
        )
    })
}

/// D-CMD-OVERRIDE1=C: resident handles marshal the same Prelude-owned suite
/// snapshot as AOT. Discovery and filtering stay in the command callback.
extern "C" fn jet_jit_testing_test_suite_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let suite = jet_codegen::command_suite::jet_test_suite_new();
        let handle = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(handle, 0, suite.iteration);
        let _ = rt.heap.record_set_int(handle, 1, suite.result);
        handle
    })
}

extern "C" fn jet_jit_testing_test_suite_run(handle: i64) -> i64 {
    let (iteration, result) = Concurrency::with_runtime_mut(|rt| {
        (
            rt.heap.record_get_int(handle, 0).unwrap_or(0),
            rt.heap.record_get_int(handle, 1).unwrap_or(0),
        )
    });
    let mut suite = jet_codegen::command_suite::JetTestSuite {
        iteration,
        result,
        runner: None,
    };
    let status = jet_codegen::command_suite::jet_test_suite_run(&mut suite);
    Concurrency::with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_int(handle, 0, suite.iteration);
        let _ = rt.heap.record_set_int(handle, 1, suite.result);
    });
    status
}

extern "C" fn jet_jit_testing_bench_suite_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let suite = jet_codegen::command_suite::jet_bench_suite_new();
        let handle = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(handle, 0, suite.iteration);
        let _ = rt.heap.record_set_int(handle, 1, suite.result);
        handle
    })
}

extern "C" fn jet_jit_testing_bench_suite_run(handle: i64) -> i64 {
    let (iteration, result) = Concurrency::with_runtime_mut(|rt| {
        (
            rt.heap.record_get_int(handle, 0).unwrap_or(0),
            rt.heap.record_get_int(handle, 1).unwrap_or(0),
        )
    });
    let mut suite = jet_codegen::command_suite::JetBenchSuite {
        iteration,
        result,
        runner: None,
    };
    let status = jet_codegen::command_suite::jet_bench_suite_run(&mut suite);
    Concurrency::with_runtime_mut(|rt| {
        let _ = rt.heap.record_set_int(handle, 0, suite.iteration);
        let _ = rt.heap.record_set_int(handle, 1, suite.result);
    });
    status
}

// #1633: one host_fns! listing for the top-level table + delegate composition.
host_fns! {
    struct HostFns;
    register: register_host_symbols;
    declare: declare_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut sig_bin_i64 = Signature::new(cc);
        sig_bin_i64.params.push(AbiParam::new(types::I64));
        sig_bin_i64.params.push(AbiParam::new(types::I64));
        sig_bin_i64.params.push(AbiParam::new(types::I32));
        sig_bin_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_pow_f64 = Signature::new(cc);
        sig_pow_f64.params.push(AbiParam::new(types::F64));
        sig_pow_f64.params.push(AbiParam::new(types::F64));
        sig_pow_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_intn_binop = Signature::new(cc);
        for _ in 0..7 {
            sig_intn_binop.params.push(AbiParam::new(types::I64));
        }
        sig_intn_binop.params.push(AbiParam::new(types::I32));
        sig_intn_binop.returns.push(AbiParam::new(types::I64));

        let mut sig_i64 = Signature::new(cc);
        sig_i64.params.push(AbiParam::new(types::I64));
        let mut sig_reflect_finish = Signature::new(cc);
        for _ in 0..4 {
            sig_reflect_finish.params.push(AbiParam::new(types::I64));
        }
        sig_reflect_finish.returns.push(AbiParam::new(types::I64));
        let mut sig_reflect_field_new = Signature::new(cc);
        sig_reflect_field_new
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_reflect_field_new
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_rich_panic = Signature::new(cc);
        for _ in 0..8 {
            sig_rich_panic.params.push(AbiParam::new(types::I64));
        }
        sig_rich_panic.returns.push(AbiParam::new(types::I64));
        let mut sig_todo_stop = Signature::new(cc);
        sig_todo_stop.params.push(AbiParam::new(types::I64));
        sig_todo_stop.params.push(AbiParam::new(types::I64));
        sig_todo_stop.returns.push(AbiParam::new(types::I64));
        let mut sig_stack_enter = Signature::new(cc);
        sig_stack_enter
            .params
            .extend([AbiParam::new(types::I64); 4]);
        sig_stack_enter.returns.push(AbiParam::new(types::I64));
        let mut sig_contract_check = Signature::new(cc);
        sig_contract_check.params.push(AbiParam::new(types::I8));
        sig_contract_check.returns.push(AbiParam::new(types::I8));
        let mut sig_contract_fail = Signature::new(cc);
        for _ in 0..4 {
            sig_contract_fail.params.push(AbiParam::new(types::I64));
        }
        sig_contract_fail.returns.push(AbiParam::new(types::I64));
        let mut sig_f64 = Signature::new(cc);
        sig_f64.params.push(AbiParam::new(types::F64));
        let mut sig_i8 = Signature::new(cc);
        sig_i8.params.push(AbiParam::new(types::I8));
        let mut sig_i32 = Signature::new(cc);
        sig_i32.params.push(AbiParam::new(types::I32));
        let mut sig_str_push_lit = Signature::new(cc);
        sig_str_push_lit.params.push(AbiParam::new(types::I64));
        sig_str_push_lit.params.push(AbiParam::new(types::I64));
        let mut sig_str_push_i64 = Signature::new(cc);
        sig_str_push_i64.params.push(AbiParam::new(types::I64));
        sig_str_push_i64.params.push(AbiParam::new(types::I64));
        let mut sig_str_push_f64 = Signature::new(cc);
        sig_str_push_f64.params.push(AbiParam::new(types::I64));
        sig_str_push_f64.params.push(AbiParam::new(types::F64));
        let mut sig_str_push_bool = Signature::new(cc);
        sig_str_push_bool.params.push(AbiParam::new(types::I64));
        sig_str_push_bool.params.push(AbiParam::new(types::I8));
        let mut sig_str_push_char = Signature::new(cc);
        sig_str_push_char.params.push(AbiParam::new(types::I64));
        sig_str_push_char.params.push(AbiParam::new(types::I32));
        let mut sig_str_eq = Signature::new(cc);
        sig_str_eq.params.push(AbiParam::new(types::I64));
        sig_str_eq.params.push(AbiParam::new(types::I64));
        sig_str_eq.returns.push(AbiParam::new(types::I8));
        let mut sig_str_unary_i64 = Signature::new(cc);
        sig_str_unary_i64.params.push(AbiParam::new(types::I64));
        sig_str_unary_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_str_unary_i8 = Signature::new(cc);
        sig_str_unary_i8.params.push(AbiParam::new(types::I64));
        sig_str_unary_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_str_replace = Signature::new(cc);
        sig_str_replace.params.push(AbiParam::new(types::I64));
        sig_str_replace.params.push(AbiParam::new(types::I64));
        sig_str_replace.params.push(AbiParam::new(types::I64));
        sig_str_replace.returns.push(AbiParam::new(types::I64));
        let mut sig_str_binary_i64 = Signature::new(cc);
        sig_str_binary_i64.params.push(AbiParam::new(types::I64));
        sig_str_binary_i64.params.push(AbiParam::new(types::I64));
        sig_str_binary_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_f64_i64 = Signature::new(cc);
        sig_f64_i64.params.push(AbiParam::new(types::F64));
        sig_f64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_f64_i64_i8 = Signature::new(cc);
        sig_f64_i64_i8.params.push(AbiParam::new(types::F64));
        sig_f64_i64_i8.params.push(AbiParam::new(types::I64));
        sig_f64_i64_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_i64_i64_i64 = Signature::new(cc);
        sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_i64_i64_i64_i64 = Signature::new(cc);
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_trace_err = Signature::new(cc);
        sig_trace_err.params.push(AbiParam::new(types::I64));
        sig_trace_err.params.push(AbiParam::new(types::I64));
        sig_trace_err.params.push(AbiParam::new(types::I64));
        let mut sig_trace_err_note = Signature::new(cc);
        sig_trace_err_note.params.push(AbiParam::new(types::I64));
        sig_trace_err_note.params.push(AbiParam::new(types::I64));
        sig_trace_err_note.params.push(AbiParam::new(types::I64));
        sig_trace_err_note.params.push(AbiParam::new(types::I64));
        let sig_trace_reset = Signature::new(cc);
        let mut sig_f64_i64_i64 = Signature::new(cc);
        sig_f64_i64_i64.params.push(AbiParam::new(types::F64));
        sig_f64_i64_i64.params.push(AbiParam::new(types::I64));
        sig_f64_i64_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_str_begin = Signature::new(cc);
        sig_str_begin.returns.push(AbiParam::new(types::I64));
        let mut sig_struct_new = Signature::new(cc);
        sig_struct_new.params.push(AbiParam::new(types::I64));
        sig_struct_new.returns.push(AbiParam::new(types::I64));
        let mut sig_struct_assign = Signature::new(cc);
        sig_struct_assign.params.push(AbiParam::new(types::I64));
        sig_struct_assign.params.push(AbiParam::new(types::I64));
        let mut sig_struct_get_i64 = Signature::new(cc);
        sig_struct_get_i64.params.push(AbiParam::new(types::I64));
        sig_struct_get_i64.params.push(AbiParam::new(types::I64));
        sig_struct_get_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_struct_get_f64 = Signature::new(cc);
        sig_struct_get_f64.params.push(AbiParam::new(types::I64));
        sig_struct_get_f64.params.push(AbiParam::new(types::I64));
        sig_struct_get_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_struct_get_i8 = Signature::new(cc);
        sig_struct_get_i8.params.push(AbiParam::new(types::I64));
        sig_struct_get_i8.params.push(AbiParam::new(types::I64));
        sig_struct_get_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_struct_get_i32 = Signature::new(cc);
        sig_struct_get_i32.params.push(AbiParam::new(types::I64));
        sig_struct_get_i32.params.push(AbiParam::new(types::I64));
        sig_struct_get_i32.returns.push(AbiParam::new(types::I32));
        let mut sig_struct_set_i64 = Signature::new(cc);
        sig_struct_set_i64.params.push(AbiParam::new(types::I64));
        sig_struct_set_i64.params.push(AbiParam::new(types::I64));
        sig_struct_set_i64.params.push(AbiParam::new(types::I64));
        let mut sig_struct_set_f64 = Signature::new(cc);
        sig_struct_set_f64.params.push(AbiParam::new(types::I64));
        sig_struct_set_f64.params.push(AbiParam::new(types::I64));
        sig_struct_set_f64.params.push(AbiParam::new(types::F64));
        let mut sig_struct_set_i8 = Signature::new(cc);
        sig_struct_set_i8.params.push(AbiParam::new(types::I64));
        sig_struct_set_i8.params.push(AbiParam::new(types::I64));
        sig_struct_set_i8.params.push(AbiParam::new(types::I8));
        let mut sig_struct_set_i32 = Signature::new(cc);
        sig_struct_set_i32.params.push(AbiParam::new(types::I64));
        sig_struct_set_i32.params.push(AbiParam::new(types::I64));
        sig_struct_set_i32.params.push(AbiParam::new(types::I32));
        let mut sig_memo_probe = Signature::new(cc);
        sig_memo_probe.params.push(AbiParam::new(types::I64));
        sig_memo_probe.params.push(AbiParam::new(types::I64));
        sig_memo_probe.returns.push(AbiParam::new(types::I8));
        let mut sig_i64_i64 = Signature::new(cc);
        sig_i64_i64.params.push(AbiParam::new(types::I64));
        sig_i64_i64.params.push(AbiParam::new(types::I64));
        let mut sig_measurement_new = Signature::new(cc);
        sig_measurement_new.params.push(AbiParam::new(types::F64));
        sig_measurement_new.params.push(AbiParam::new(types::F64));
        sig_measurement_new.returns.push(AbiParam::new(types::I64));
        let mut sig_measurement_arithmetic = Signature::new(cc);
        sig_measurement_arithmetic.params.push(AbiParam::new(types::I64));
        sig_measurement_arithmetic.params.push(AbiParam::new(types::I64));
        sig_measurement_arithmetic.params.push(AbiParam::new(types::I64));
        sig_measurement_arithmetic.returns.push(AbiParam::new(types::I64));
        let mut sig_measurement_get = Signature::new(cc);
        sig_measurement_get.params.push(AbiParam::new(types::I64));
        sig_measurement_get.params.push(AbiParam::new(types::I64));
        sig_measurement_get.returns.push(AbiParam::new(types::F64));
        let mut sig_is_trapped = Signature::new(cc);
        sig_is_trapped.returns.push(AbiParam::new(types::I64));
        let mut sig_numeric_checked_widen = Signature::new(cc);
        sig_numeric_checked_widen
            .params
            .extend([AbiParam::new(types::I64); 3]);
        sig_numeric_checked_widen
            .returns
            .push(AbiParam::new(types::F64));
        let mut sig_result_new_i64 = Signature::new(cc);
        sig_result_new_i64.params.push(AbiParam::new(types::I8));
        sig_result_new_i64.params.push(AbiParam::new(types::I64));
        sig_result_new_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_result_new_f64 = Signature::new(cc);
        sig_result_new_f64.params.push(AbiParam::new(types::I8));
        sig_result_new_f64.params.push(AbiParam::new(types::F64));
        sig_result_new_f64.returns.push(AbiParam::new(types::I64));
        let mut sig_option_lift2 = Signature::new(cc);
        sig_option_lift2.params.push(AbiParam::new(types::I8));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.params.push(AbiParam::new(types::I8));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.params.push(AbiParam::new(types::I64));
        sig_option_lift2.returns.push(AbiParam::new(types::I64));
        let mut sig_callable_bind = Signature::new(cc);
        sig_callable_bind.params.push(AbiParam::new(types::I64));
        sig_callable_bind.params.push(AbiParam::new(types::I64));
        sig_callable_bind.params.push(AbiParam::new(types::I8));
        sig_callable_bind.returns.push(AbiParam::new(types::I64));
        let mut sig_callable_word = Signature::new(cc);
        sig_callable_word.params.push(AbiParam::new(types::I64));
        sig_callable_word.returns.push(AbiParam::new(types::I64));
        let mut sig_callable_flag = Signature::new(cc);
        sig_callable_flag.params.push(AbiParam::new(types::I64));
        sig_callable_flag.returns.push(AbiParam::new(types::I8));
        let mut sig_result_new_i8 = Signature::new(cc);
        sig_result_new_i8.params.push(AbiParam::new(types::I8));
        sig_result_new_i8.params.push(AbiParam::new(types::I8));
        sig_result_new_i8.returns.push(AbiParam::new(types::I64));
        let mut sig_result_new_i32 = Signature::new(cc);
        sig_result_new_i32.params.push(AbiParam::new(types::I8));
        sig_result_new_i32.params.push(AbiParam::new(types::I32));
        sig_result_new_i32.returns.push(AbiParam::new(types::I64));
        let mut sig_unit_convert_exact = Signature::new(cc);
        sig_unit_convert_exact.params.push(AbiParam::new(types::F64));
        sig_unit_convert_exact.params.extend([AbiParam::new(types::I64); 4]);
        sig_unit_convert_exact.returns.push(AbiParam::new(types::I64));
        let mut sig_unit_convert_rounded = Signature::new(cc);
        sig_unit_convert_rounded.params.push(AbiParam::new(types::F64));
        sig_unit_convert_rounded.params.extend([AbiParam::new(types::I64); 6]);
        sig_unit_convert_rounded.returns.push(AbiParam::new(types::I64));
        let mut sig_unit_convert_implicit = Signature::new(cc);
        sig_unit_convert_implicit.params.push(AbiParam::new(types::F64));
        sig_unit_convert_implicit.params.extend([AbiParam::new(types::I64); 4]);
        sig_unit_convert_implicit.returns.push(AbiParam::new(types::F64));
        let mut sig_result_query_i8 = Signature::new(cc);
        sig_result_query_i8.params.push(AbiParam::new(types::I64));
        sig_result_query_i8.returns.push(AbiParam::new(types::I8));
        let mut sig_result_query_i64 = Signature::new(cc);
        sig_result_query_i64.params.push(AbiParam::new(types::I64));
        sig_result_query_i64.returns.push(AbiParam::new(types::I64));
        let mut sig_result_query_f64 = Signature::new(cc);
        sig_result_query_f64.params.push(AbiParam::new(types::I64));
        sig_result_query_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_result_query_i32 = Signature::new(cc);
        sig_result_query_i32.params.push(AbiParam::new(types::I64));
        sig_result_query_i32.returns.push(AbiParam::new(types::I32));
        let mut sig_duration_float = Signature::new(cc);
        sig_duration_float.params.push(AbiParam::new(types::F64));
        sig_duration_float.params.push(AbiParam::new(types::I64));
        sig_duration_float.returns.push(AbiParam::new(types::I64));
        let mut sig_duration_int = Signature::new(cc);
        sig_duration_int.params.push(AbiParam::new(types::I64));
        sig_duration_int.params.push(AbiParam::new(types::I64));
        sig_duration_int.returns.push(AbiParam::new(types::I64));
        let mut sig_noarg_f64 = Signature::new(cc);
        sig_noarg_f64.returns.push(AbiParam::new(types::F64));
        let mut sig_perf_override = Signature::new(cc);
        sig_perf_override.params.push(AbiParam::new(types::F64));
        sig_perf_override.returns.push(AbiParam::new(types::I64));
        let mut sig_service_call = Signature::new(cc);
        sig_service_call
            .params
            .extend([AbiParam::new(types::I64); 10]);
        sig_service_call.returns.push(AbiParam::new(types::I64));
        let mut sig_service_call_bool = Signature::new(cc);
        sig_service_call_bool
            .params
            .extend([AbiParam::new(types::I64); 10]);
        sig_service_call_bool.returns.push(AbiParam::new(types::I8));
        let mut sig_deopt = Signature::new(cc);
        // fn_idx, argc, a0..a7
        for _ in 0..10 {
            sig_deopt.params.push(AbiParam::new(types::I64));
        }
        sig_deopt.returns.push(AbiParam::new(types::I64));
        let sig_noarg = Signature::new(cc);
        let mut sig_noarg_i64 = Signature::new(cc);
        sig_noarg_i64.returns.push(AbiParam::new(types::I64));
    }
    #extra {
        coll: Collections::CollectionsHostFns,
        compute: Compute::ComputeHostFns,
        memory: Memory::MemoryHostFns,
        cell: LocalCell::CellHostFns,
        conc: Concurrency::ConcurrencyHostFns,
        core: CoreHost::CoreHostFns,
        encoding: Encoding::EncodingHostFns,
        stream: crate::enc_stream::StreamHostFns,
        fmt: Fmt::FmtHostFns,
        compress: Compress::CompressHostFns,
        archive: Archive::ArchiveHostFns,
        process: Process::ProcessHostFns,
        num: Numeric::NumericHostFns,
        solver: Solver::SolverHostFns,
        random: Random::RandomHostFns,
        text: crate::Text::TextHostFns,
        sketch: crate::Sketch::SketchHostFns,
        args: crate::Args::ArgsHostFns,
        db: crate::DB::DBHostFns,
        crypto: Crypto::CryptoHostFns,
        net: Net::NetHostFns,
        net_http: crate::net_http_rt::NetHttpHostFns,
        game: crate::Game::GameHostFns,
        raylib: crate::Raylib::RaylibHostFns,
        layout: crate::Layout::LayoutHostFns,
        reactive: crate::Reactive::ReactiveHostFns,
        ui: crate::Ui::UiHostFns,
        web: crate::Web::WebHostFns,
        parse: crate::Parse::HostFns,
        data: crate::Data::DataHostFns,
        time: crate::Time::TimeHostFns,
        io: crate::IO::IOHostFns,
        watcher: crate::Watcher::WatcherHostFns,
        math: crate::Math::MathHostFns,
        math_extra: crate::MathExtra::MathExtraHostFns,
        ffi: crate::Ffi::FfiHostFns,
    }
    add_i64: "jet_jit_add_i64" => jet_jit_add_i64: sig_bin_i64;
    sub_i64: "jet_jit_sub_i64" => jet_jit_sub_i64: sig_bin_i64;
    mul_i64: "jet_jit_mul_i64" => jet_jit_mul_i64: sig_bin_i64;
    div_i64: "jet_jit_div_i64" => jet_jit_div_i64: sig_bin_i64;
    rem_i64: "jet_jit_rem_i64" => jet_jit_rem_i64: sig_bin_i64;
    pow_i64: "jet_jit_pow_i64" => jet_jit_pow_i64: sig_bin_i64;
    floordiv_i64: "jet_jit_floordiv_i64" => jet_jit_floordiv_i64: sig_bin_i64;
    mod_i64: "jet_jit_mod_i64" => jet_jit_mod_i64: sig_bin_i64;
    floordiv_f64: "jet_jit_floordiv_f64" => jet_jit_floordiv_f64: sig_pow_f64;
    pow_f64: "jet_jit_pow_f64" => jet_jit_pow_f64: sig_pow_f64;
    intn_binop: "jet_jit_intn_binop" => jet_jit_intn_binop: sig_intn_binop;
    intn_to_string: "jet_jit_intn_to_string" => jet_jit_intn_to_string: sig_i64_i64_i64;
    print_i64: "jet_jit_print_i64" => jet_jit_print_i64: sig_i64;
    print_f64: "jet_jit_print_f64" => jet_jit_print_f64: sig_f64;
    print_bool: "jet_jit_print_bool" => jet_jit_print_bool: sig_i8;
    print_char: "jet_jit_print_char" => jet_jit_print_char: sig_i32;
    print_str: "jet_jit_print_str" => jet_jit_print_str: sig_i64;
    str_begin: "jet_jit_str_begin" => jet_jit_str_begin: sig_str_begin;
    str_push_lit: "jet_jit_str_push_lit" => jet_jit_str_push_lit: sig_str_push_lit;
    str_push_i64: "jet_jit_str_push_i64" => jet_jit_str_push_i64: sig_str_push_i64;
    str_push_f64: "jet_jit_str_push_f64" => jet_jit_str_push_f64: sig_str_push_f64;
    str_push_compact_f64: "jet_jit_str_push_compact_f64" => jet_jit_str_push_compact_f64: sig_str_push_f64;
    str_push_bool: "jet_jit_str_push_bool" => jet_jit_str_push_bool: sig_str_push_bool;
    str_push_char: "jet_jit_str_push_char" => jet_jit_str_push_char: sig_str_push_char;
    str_push_str: "jet_jit_str_push_str" => jet_jit_str_push_str: sig_str_push_lit;
    str_eq: "jet_jit_str_eq" => jet_jit_str_eq: sig_str_eq;
    str_contains: "jet_jit_str_contains" => jet_jit_str_contains: sig_str_eq;
    str_starts_with: "jet_jit_str_starts_with" => jet_jit_str_starts_with: sig_str_eq;
    str_ends_with: "jet_jit_str_ends_with" => jet_jit_str_ends_with: sig_str_eq;
    str_clone: "jet_jit_str_clone" => jet_jit_str_clone: sig_str_unary_i64;
    str_len: "jet_jit_str_len" => jet_jit_str_len: sig_str_unary_i64;
    str_byte_len: "jet_jit_str_byte_len" => jet_jit_str_byte_len: sig_str_unary_i64;
    str_is_ascii: "jet_jit_str_is_ascii" => jet_jit_str_is_ascii: sig_str_unary_i8;
    str_trim: "jet_jit_str_trim" => jet_jit_str_trim: sig_str_unary_i64;
    str_to_upper: "jet_jit_str_to_upper" => jet_jit_str_to_upper: sig_str_unary_i64;
    str_to_lower: "jet_jit_str_to_lower" => jet_jit_str_to_lower: sig_str_unary_i64;
    str_replace: "jet_jit_str_replace" => jet_jit_str_replace: sig_str_replace;
    str_lines: "jet_jit_str_lines" => jet_jit_str_lines: sig_str_unary_i64;
    str_split: "jet_jit_str_split" => jet_jit_str_split: sig_str_binary_i64;
    str_rsplit: "jet_jit_str_rsplit" => jet_jit_str_rsplit: sig_str_binary_i64;
    str_chars: "jet_jit_str_chars" => jet_jit_str_chars: sig_str_unary_i64;
    str_bytes: "jet_jit_str_bytes" => jet_jit_str_bytes: sig_str_unary_i64;
    str_scalar_strings: "jet_jit_str_scalar_strings" => jet_jit_str_scalar_strings: sig_str_unary_i64;
    str_after: "jet_jit_str_after" => jet_jit_str_after: sig_str_binary_i64;
    str_before: "jet_jit_str_before" => jet_jit_str_before: sig_str_binary_i64;
    str_trim_view: "jet_jit_str_trim_view" => jet_jit_str_trim_view: sig_str_unary_i64;
    str_after_view: "jet_jit_str_after_view" => jet_jit_str_after_view: sig_str_binary_i64;
    str_before_view: "jet_jit_str_before_view" => jet_jit_str_before_view: sig_str_binary_i64;
    str_slice: "jet_jit_str_slice" => jet_jit_str_slice: sig_str_replace;
    clock_new: "jet_jit_clock_new" => jet_jit_clock_new: sig_str_unary_i64;
    clock_now: "jet_jit_clock_now" => jet_jit_clock_now: sig_str_unary_i64;
    clock_tick: "jet_jit_clock_tick" => jet_jit_clock_tick: sig_struct_assign;
    clock_advance: "jet_jit_clock_advance" => jet_jit_clock_advance: sig_str_binary_i64;
    clock_wait: "jet_jit_clock_wait" => jet_jit_clock_wait: sig_str_binary_i64;
    parse_i64: "jet_jit_parse_i64" => jet_jit_parse_i64: sig_str_unary_i64;
    parse_f64: "jet_jit_parse_f64" => jet_jit_parse_f64: sig_str_unary_i64;
    numeric_try_i64: "jet_jit_numeric_try_i64" => jet_jit_numeric_try_i64: sig_i64_i64_i64_i64;
    numeric_float_to_int: "jet_jit_numeric_float_to_int" => jet_jit_numeric_float_to_int: sig_f64_i64_i64;
    numeric_float_narrow: "jet_jit_numeric_float_narrow" => jet_jit_numeric_float_narrow: sig_f64_i64;
    numeric_checked_widen: "jet_jit_numeric_checked_widen" => jet_jit_numeric_checked_widen: sig_numeric_checked_widen;
    distinct_range: "jet_jit_distinct_range" => jet_jit_distinct_range: sig_i64_i64_i64_i64;
    distinct_range_result: "jet_jit_distinct_range_result" => jet_jit_distinct_range_result: sig_i64_i64_i64_i64;
    numeric_predicate: "jet_jit_numeric_predicate" => jet_jit_numeric_predicate: sig_f64_i64_i8;
    numeric_bit_count: "jet_jit_numeric_bit_count" => jet_jit_numeric_bit_count: sig_i64_i64_i64_i64;
    struct_new: "jet_jit_struct_new" => jet_jit_struct_new: sig_struct_new;
    struct_assign: "jet_jit_struct_assign" => jet_jit_struct_assign: sig_struct_assign;
    struct_get_i64: "jet_jit_struct_get_i64" => jet_jit_struct_get_i64: sig_struct_get_i64;
    struct_get_f64: "jet_jit_struct_get_f64" => jet_jit_struct_get_f64: sig_struct_get_f64;
    struct_get_bool: "jet_jit_struct_get_bool" => jet_jit_struct_get_bool: sig_struct_get_i8;
    struct_get_char: "jet_jit_struct_get_char" => jet_jit_struct_get_char: sig_struct_get_i32;
    struct_get_str: "jet_jit_struct_get_str" => jet_jit_struct_get_str: sig_struct_get_i64;
    struct_set_i64: "jet_jit_struct_set_i64" => jet_jit_struct_set_i64: sig_struct_set_i64;
    struct_set_f64: "jet_jit_struct_set_f64" => jet_jit_struct_set_f64: sig_struct_set_f64;
    struct_set_bool: "jet_jit_struct_set_bool" => jet_jit_struct_set_bool: sig_struct_set_i8;
    struct_set_char: "jet_jit_struct_set_char" => jet_jit_struct_set_char: sig_struct_set_i32;
    struct_set_str: "jet_jit_struct_set_str" => jet_jit_struct_set_str: sig_struct_set_i64;
    memo_probe: "jet_jit_memo_probe" => jet_jit_memo_probe: sig_memo_probe;
    memo_get: "jet_jit_memo_get" => jet_jit_memo_get: sig_struct_get_i64;
    memo_put: "jet_jit_memo_put" => jet_jit_memo_put: sig_struct_set_i64;
    memo_clear: "jet_jit_memo_clear" => jet_jit_memo_clear: sig_i64;
    memo_clear_slot: "jet_jit_memo_clear_slot" => jet_jit_memo_clear_slot: sig_i64_i64;
    err_new: "jet_jit_err_new" => jet_jit_err_new: sig_i64_i64_i64_i64;
    err_message: "jet_jit_err_message" => jet_jit_err_message: sig_str_unary_i64;
    err_code: "jet_jit_err_code" => jet_jit_err_code: sig_str_unary_i64;
    err_cause: "jet_jit_err_cause" => jet_jit_err_cause: sig_str_unary_i64;
    measurement_new: "jet_jit_measurement_new" => jet_jit_measurement_new: sig_measurement_new;
    measurement_arithmetic: "jet_jit_measurement_arithmetic" => jet_jit_measurement_arithmetic: sig_measurement_arithmetic;
    measurement_get: "jet_jit_measurement_get" => jet_jit_measurement_get: sig_measurement_get;
    measurement_show: "jet_jit_measurement_show" => jet_jit_measurement_show: sig_str_unary_i64;
    result_new_i64: "jet_jit_result_new_i64" => jet_jit_result_new_i64: sig_result_new_i64;
    result_new_f64: "jet_jit_result_new_f64" => jet_jit_result_new_f64: sig_result_new_f64;
    result_new_i8: "jet_jit_result_new_i8" => jet_jit_result_new_i8: sig_result_new_i8;
    result_new_i32: "jet_jit_result_new_i32" => jet_jit_result_new_i32: sig_result_new_i32;
    option_lift2: "jet_jit_option_lift2" => jet_jit_option_lift2: sig_option_lift2;
    callable_bind: "jet_jit_callable_bind" => jet_jit_callable_bind: sig_callable_bind;
    callable_normalize: "jet_jit_callable_normalize" => jet_jit_callable_normalize: sig_callable_word;
    callable_fn: "jet_jit_callable_fn" => jet_jit_callable_fn: sig_callable_word;
    callable_env: "jet_jit_callable_env" => jet_jit_callable_env: sig_callable_word;
    callable_has_env: "jet_jit_callable_has_env" => jet_jit_callable_has_env: sig_callable_flag;
    unit_convert_exact: "jet_jit_unit_convert_exact" => jet_jit_unit_convert_exact: sig_unit_convert_exact;
    unit_convert_rounded: "jet_jit_unit_convert_rounded" => jet_jit_unit_convert_rounded: sig_unit_convert_rounded;
    unit_convert_implicit: "jet_jit_unit_convert_implicit" => jet_jit_unit_convert_implicit: sig_unit_convert_implicit;
    result_is_ok: "jet_jit_result_is_ok" => jet_jit_result_is_ok: sig_result_query_i8;
    result_get_i64: "jet_jit_result_get_i64" => jet_jit_result_get_i64: sig_result_query_i64;
    result_get_f64: "jet_jit_result_get_f64" => jet_jit_result_get_f64: sig_result_query_f64;
    result_get_i8: "jet_jit_result_get_i8" => jet_jit_result_get_i8: sig_result_query_i8;
    result_get_i32: "jet_jit_result_get_i32" => jet_jit_result_get_i32: sig_result_query_i32;
    trap_panic: "jet_jit_trap_panic" => jet_jit_trap_panic: sig_i64;
    rich_panic: "jet_jit_rich_panic" => jet_jit_rich_panic: sig_rich_panic;
    todo_stop: "jet_jit_todo_stop" => jet_jit_todo_stop: sig_todo_stop;
    contract_check: "jet_jit_contract_check" => jet_jit_contract_check: sig_contract_check;
    contract_fail: "jet_jit_contract_fail" => jet_jit_contract_fail: sig_contract_fail;
    trace_err: "jet_jit_trace_err" => jet_jit_trace_err: sig_trace_err;
    trace_err_note: "jet_jit_trace_err_note" => jet_jit_trace_err_note: sig_trace_err_note;
    trace_reset: "jet_jit_trace_reset" => jet_jit_trace_reset: sig_trace_reset;
    duration_from_int: "jet_jit_duration_from_int" => jet_jit_duration_from_int: sig_duration_int;
    duration_from_float: "jet_jit_duration_from_float" => jet_jit_duration_from_float: sig_duration_float;
    duration_in: "jet_jit_duration_in" => jet_jit_duration_in: sig_duration_int;
    duration_in_unit: "jet_jit_duration_in_unit" => jet_jit_duration_in_unit: sig_duration_int;
    duration_is_zero: "jet_jit_duration_is_zero" => jet_jit_duration_is_zero: sig_result_query_i8;
    duration_total_seconds: "jet_jit_duration_total_seconds" => jet_jit_duration_total_seconds: sig_result_query_i64;
    duration_difference: "jet_jit_duration_difference" => jet_jit_duration_difference: sig_duration_int;
    perf_fidelity: "jet_jit_perf_fidelity" => jet_jit_perf_fidelity: sig_noarg_f64;
    perf_default_fidelity: "jet_jit_perf_default_fidelity" => jet_jit_perf_default_fidelity: sig_noarg_f64;
    perf_override_fidelity: "jet_jit_perf_override_fidelity" => jet_jit_perf_override_fidelity: sig_perf_override;
    perf_reset_fidelity: "jet_jit_perf_reset_fidelity" => jet_jit_perf_reset_fidelity: sig_noarg;
    service_call: "jet_jit_service_call" => jet_jit_service_call: sig_service_call;
    service_call_bool: "jet_jit_service_call_bool" => jet_jit_service_call_bool: sig_service_call_bool;
    service_show: "jet_jit_service_show" => jet_jit_service_show: sig_str_unary_i64;
    is_trapped: "jet_jit_is_trapped" => jet_jit_is_trapped: sig_is_trapped;
    stack_enter: "jet_jit_stack_enter" => jet_jit_stack_enter: sig_stack_enter;
    stack_leave: "jet_jit_stack_leave" => jet_jit_stack_leave: sig_noarg;
    deopt_call: "jet_deopt_call" => super::deopt::jet_deopt_call: sig_deopt;
    reflect_of_finish: "jet_jit_reflect_of_finish" => jet_jit_reflect_of_finish: sig_reflect_finish;
    reflect_field_new: "jet_jit_reflect_field_new" => jet_jit_reflect_field_new: sig_reflect_field_new;
    reflect_type_name: "jet_jit_reflect_type_name" => jet_jit_reflect_type_name: sig_str_unary_i64;
    reflect_path: "jet_jit_reflect_path" => jet_jit_reflect_path: sig_str_unary_i64;
    reflect_display: "jet_jit_reflect_display" => jet_jit_reflect_display: sig_str_unary_i64;
    reflect_fields: "jet_jit_reflect_fields" => jet_jit_reflect_fields: sig_str_unary_i64;
    reflect_field_name: "jet_jit_reflect_field_name" => jet_jit_reflect_field_name: sig_str_unary_i64;
    reflect_field_value: "jet_jit_reflect_field_value" => jet_jit_reflect_field_value: sig_str_unary_i64;
    testing_temp_dir: "jet_jit_testing_temp_dir" => jet_jit_testing_temp_dir: sig_str_unary_i64;
    testing_snap: "jet_jit_testing_snap" => jet_jit_testing_snap: sig_str_eq;
    testing_test_suite_new: "jet_jit_testing_test_suite_new" => jet_jit_testing_test_suite_new: sig_str_begin;
    testing_test_suite_run: "jet_jit_testing_test_suite_run" => jet_jit_testing_test_suite_run: sig_str_unary_i64;
    testing_bench_suite_new: "jet_jit_testing_bench_suite_new" => jet_jit_testing_bench_suite_new: sig_str_begin;
    testing_bench_suite_run: "jet_jit_testing_bench_suite_run" => jet_jit_testing_bench_suite_run: sig_str_unary_i64;
    cli_main: "jet_jit_cli_main" => crate::CLI::jet_jit_cli_main: sig_noarg_i64;
}

#[cfg(test)]
mod host_fns_tests {
    use super::new_jit_module;
    use crate::host_fns_audit;

    /// #1633 criterion #3: every `host_fns!`-declared symbol (across every
    /// `host_fns!`-migrated module, `@shared` entries included) must have a
    /// matching `builder.symbol` registration.
    ///
    /// `JITModule::new` does NOT prove this: cranelift-jit 0.112.3's
    /// `declare_function` for `Linkage::Import` does
    /// `lookup_symbol(name).unwrap_or(null)` and installs a null PLT entry on
    /// a miss, returning `Ok` regardless. A prior version of this test only
    /// asserted `new_jit_module()` is `Ok`, which stayed green even with a
    /// missing registration (e.g. deleting `Reactive`'s
    /// `event_scope: "jet_jit_event_scope" => jet_jit_event_scope`
    /// registration while `Watcher`'s `@shared event_scope_cancel` import
    /// kept expecting it) — the JIT would then call a null pointer at run
    /// time. `host_fns!` now records every symbol it declares and registers
    /// into two process-wide sets (`host_fns_audit`); this test compares
    /// them directly instead of trusting `new_jit_module`'s `Ok`.
    #[test]
    fn all_host_symbols_declared_match_all_registered() {
        let (_module, _host) = new_jit_module()
            .expect("every declared JIT host FuncId must resolve to a registered symbol");
        let (registered, declared) = host_fns_audit::take_snapshot();
        assert!(
            !declared.is_empty(),
            "host_fns! declared no symbols — audit hooks did not fire"
        );
        let declared_not_registered: Vec<_> = declared.difference(&registered).collect();
        assert!(
            declared_not_registered.is_empty(),
            "declared JIT host symbols with no matching registration (would resolve to a \
             null PLT entry at run time, not a build error): {declared_not_registered:?}"
        );
        let registered_not_declared: Vec<_> = registered.difference(&declared).collect();
        assert!(
            registered_not_declared.is_empty(),
            "registered JIT host symbols that no `host_fns!` table declares/imports \
             (dead registration, not the single listing #1633 requires): {registered_not_declared:?}"
        );
    }
}
