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
    Archive, Cell as LocalCell, Collections, Compress, Concurrency, CoreHost, Crypto, Encoding, Fmt,
    JitResultValue, Memory, Net, Numeric, Process, Random, Solver, Text, Time,
    TRY_COMPILE_PANIC_HOOK_LOCK,
};

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
    pub type_name: String,
    pub display: String,
    pub fields: Vec<(String, String)>,
}

pub(crate) struct JitRuntime {
    pub(crate) source_file: String,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) heap: jet_rt::JetArena,
    /// Compile-time string handles baked into Cranelift as `iconst` ids.
    /// `reset_run_heap` and the run-cache artifact must preserve these — clearing
    /// them leaves warm `jet run` hits with empty panic/require text (I9).
    pub(crate) compile_strings: Vec<(usize, String)>,
    pub(crate) invocations: u64,
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
    pub(crate) tasks: Vec<Option<JetSchedulerJoin<i64>>>,
    pub(crate) task_controls: Vec<std::sync::Arc<JetTaskControl>>,
    pub(crate) task_groups:
        Vec<Option<jet_codegen::task_group::JetTaskGroupRuntime<i64>>>,
    /// D-LOCALCELL1=A: one-thread canonical Cell values and guards.
    pub(crate) cells: LocalCell::CellState,
    /// General `Result<T, E>` ABI arena. Handles are one-based indices; payload
    /// bits are interpreted from checked TIR types, never dynamically guessed.
    pub(crate) results: Vec<JitResultValue>,
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
    pub(crate) expirings: Vec<Memory::ExpiringState>,
    pub(crate) secrets: Vec<Option<Memory::SecretState>>,
    pub(crate) crypto_values: Vec<Option<Crypto::CryptoValue>>,
    /// `core.url` / `core.mime` / net handles (#1221).
    pub(crate) net_values: Vec<Option<Net::NetValue>>,
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
    /// Regex / Match handles for jet.regex (#1219).
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

impl JitRuntime {
    /// Snapshot string handles allocated during lowering (baked into code).
    pub(crate) fn snapshot_compile_strings(&mut self) {
        self.compile_strings = self.heap.string_slots();
    }

    /// Record a runtime panic. Keeps the first message (the unwind branch may
    /// re-enter trap sites with dummy values before the epilogue is reached).
    pub(crate) fn set_trap(&mut self, msg: &str) {
        if self.trapped.is_none() {
            self.trapped = Some(msg.to_string());
        }
    }

    pub(crate) fn set_deadline(&mut self, rendered: String) {
        if self.deadline_exceeded.is_none() {
            self.deadline_exceeded = Some(rendered);
        }
    }
}

pub(crate) struct ResidentModule {
    pub(crate) module: JITModule,
    pub(crate) host: HostFns,
    pub(crate) main_id: FuncId,
    pub(crate) main_returns_result: bool,
}

fn with_runtime_mut<F: FnOnce(&mut JitRuntime)>(f: F) {
    Concurrency::with_runtime_mut(f);
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
fn jet_trap_overflow(op: &str) {
    use jet_codegen::Comptime::MathLayout;
    let msg = match op {
        "add" => "this addition overflows the value's type (the result is outside its range)",
        "sub" => "this subtraction overflows the value's type (the result is outside its range)",
        "mul" => "this multiplication overflows the value's type (the result is outside its range)",
        "div" => "this division can't be done (dividing by zero, or overflow)",
        "pow" => MathLayout::INTEGER_POWER_OVERFLOW,
        _ => "this operation overflows the value's type (the result is outside its range)",
    };
    with_runtime_mut(|rt| rt.set_trap(msg));
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
    Concurrency::with_runtime_mut(|rt| i64::from(rt.trapped.is_some()))
}

extern "C" fn jet_jit_add_i64(a: i64, b: i64, _line: u32) -> i64 {
    match a.checked_add(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("add");
            0
        }
    }
}

extern "C" fn jet_jit_sub_i64(a: i64, b: i64, _line: u32) -> i64 {
    match a.checked_sub(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("sub");
            0
        }
    }
}

extern "C" fn jet_jit_mul_i64(a: i64, b: i64, _line: u32) -> i64 {
    match a.checked_mul(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("mul");
            0
        }
    }
}

extern "C" fn jet_jit_div_i64(a: i64, b: i64, _line: u32) -> i64 {
    match a.checked_div(b) {
        Some(v) => v,
        None => {
            jet_trap_overflow("div");
            0
        }
    }
}

/// D-EXPSEM1=A: the same exact, trapping whole-number power the Prelude runs
/// (`Prelude/Core/Power.rs`). A negative exponent has no whole-number result.
extern "C" fn jet_jit_pow_i64(a: i64, b: i64, _line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if b < 0 {
        with_runtime_mut(|rt| rt.set_trap(MathLayout::INTEGER_POWER_NEGATIVE));
        return 0;
    }
    match u32::try_from(b).ok().and_then(|e| a.checked_pow(e)) {
        Some(value) => value,
        None => {
            jet_trap_overflow("pow");
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
extern "C" fn jet_jit_floordiv_i64(a: i64, b: i64, _line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if b == 0 {
        with_runtime_mut(|rt| rt.set_trap(MathLayout::INTEGER_DIVIDE_ZERO));
        return 0;
    }
    match MathLayout::floor_div(a as i128, b as i128).and_then(|v| i64::try_from(v).ok()) {
        Some(value) => value,
        None => {
            with_runtime_mut(|rt| rt.set_trap(MathLayout::INTEGER_DIVIDE_OVERFLOW));
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
extern "C" fn jet_jit_mod_i64(a: i64, b: i64, _line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if b == 0 {
        with_runtime_mut(|rt| rt.set_trap(MathLayout::INTEGER_DIVIDE_ZERO));
        return 0;
    }
    match MathLayout::floored_mod(a as i128, b as i128).and_then(|v| i64::try_from(v).ok()) {
        Some(value) => value,
        None => {
            with_runtime_mut(|rt| rt.set_trap(MathLayout::INTEGER_DIVIDE_OVERFLOW));
            0
        }
    }
}

extern "C" fn jet_jit_rem_i64(a: i64, b: i64, _line: u32) -> i64 {
    use jet_codegen::Comptime::MathLayout;
    if let Some(message) = MathLayout::integer_remainder_trap(b) {
        with_runtime_mut(|rt| rt.set_trap(message));
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
            with_runtime_mut(|rt| rt.set_trap("unknown fixed-width integer operation"));
            return 0;
        }
    };
    let signed = signed != 0;
    let bits = bits as u8;
    let right_signed = right_signed != 0;
    let shift_count = MathLayout::integer_widen(right, right_signed);
    if let Some(message) = MathLayout::integer_shift_trap(op, shift_count, bits) {
        with_runtime_mut(|rt| rt.set_trap(&message));
        return 0;
    }
    // D-FLOORDIV1=A: `/%` names a zero divisor exactly, rather than falling
    // into the shared "this division can't be done" wording below.
    if mode == INTN_MODE_TRAP && matches!(op, BinOp::FloorDiv | BinOp::Mod) && right == 0 {
        with_runtime_mut(|rt| rt.set_trap(MathLayout::INTEGER_DIVIDE_ZERO));
        return 0;
    }
    if mode == INTN_MODE_TRAP && op == BinOp::Rem {
        if let Some(message) = MathLayout::integer_remainder_trap(right) {
            with_runtime_mut(|rt| rt.set_trap(message));
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
                    with_runtime_mut(|rt| rt.set_trap(MathLayout::INTEGER_DIVIDE_OVERFLOW));
                }
                BinOp::Pow => {
                    with_runtime_mut(|rt| rt.set_trap(MathLayout::INTEGER_POWER_OVERFLOW));
                }
                _ => {
                    let name = match op {
                        BinOp::Add => "add",
                        BinOp::Sub => "sub",
                        BinOp::Mul => "mul",
                        BinOp::Div => "div",
                        _ => "shift",
                    };
                    jet_trap_overflow(name);
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
    with_runtime_mut(|rt| {
        rt.stdout.push_str(&v.to_string());
        rt.stdout.push('\n');
    });
}

extern "C" fn jet_jit_print_f64(v: f64) {
    with_runtime_trap(|rt| {
        rt.stdout.push_str(&jet_rt::display_f64(v));
        rt.stdout.push('\n');
    });
}

extern "C" fn jet_jit_print_bool(v: i8) {
    with_runtime_mut(|rt| {
        rt.stdout.push_str(if v == 0 { "false" } else { "true" });
        rt.stdout.push('\n');
    });
}

extern "C" fn jet_jit_print_char(v: i32) {
    with_runtime_mut(|rt| {
        match char::from_u32(v as u32) {
            Some(ch) => rt.stdout.push(ch),
            None => rt.stdout.push('?'),
        }
        rt.stdout.push('\n');
    });
}

extern "C" fn jet_jit_print_str(id: i64) {
    with_runtime_mut(|rt| {
        if let Some(s) = rt.heap.get_string(id) {
            rt.stdout.push_str(s);
            rt.stdout.push('\n');
        }
    });
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
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        let src_line = rt.heap.clone_string(src_line).unwrap_or_default();
        let msg = rt.heap.clone_string(msg).unwrap_or_default();
        let locals = rt.heap.clone_string(locals).unwrap_or_default();
        let line_s = line.to_string();
        let margin = line_s.len();
        let pad = " ".repeat(margin);
        let col_offset = (col as u64).saturating_sub(1) as usize;
        let caret = "^".repeat((caret as usize).max(1));
        let mut out = String::new();
        out.push_str(&format!("panic: {msg}\n"));
        out.push_str(&format!("  --> {file}:{line} in {fn_name}\n"));
        out.push_str(&format!("   {pad}|\n"));
        out.push_str(&format!("{line_s} | {src_line}\n"));
        out.push_str(&format!("   {pad}| {}{caret}\n", " ".repeat(col_offset)));
        if !locals.is_empty() {
            out.push_str(&format!("locals: {locals}\n"));
        }
        rt.stderr.push_str(&out);
        rt.exit_code = Some(70);
        rt.set_trap("__jet_rich_panic__");
        0
    })
}


extern "C" fn jet_jit_trap_panic(_unused: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap("panic");
        0
    })
}

extern "C" fn jet_jit_trace_err(file: i64, line: i64, fn_name: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let file = rt.heap.clone_string(file).unwrap_or_default();
        let fn_name = rt.heap.clone_string(fn_name).unwrap_or_default();
        let line = format!(
            "error propagated from: {fn_name} ({file}:{line}) via ?\n"
        );
        rt.stderr.push_str(&line);
    });
}

extern "C" fn jet_jit_result_context(handle: i64, msg: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some(result) = jit_result(rt, handle) else {
            return 0;
        };
        if result.ok {
            return handle;
        }
        let err = rt
            .heap
            .clone_string(result.bits as i64)
            .unwrap_or_default();
        let msg = rt.heap.clone_string(msg).unwrap_or_default();
        let combined = rt.heap.alloc_string(format!("{msg}: {err}"));
        alloc_jit_result(rt, false, combined as u64)
    })
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

fn measurement_ct(value: f64, uncertainty: f64) -> Option<jet_codegen::AST::CtValue> {
    use jet_codegen::AST::{CtFloat, CtValue};
    jet_codegen::Comptime::apply_core_call(
        "core.science.measurement",
        "from",
        vec![
            CtValue::Float(CtFloat::f64(value)),
            CtValue::Float(CtFloat::f64(uncertainty)),
        ],
        jet_codegen::Diagnostics::Span::new(0, 0),
        false,
    )
    .ok()
}

fn measurement_parts(value: &jet_codegen::AST::CtValue) -> Option<(f64, f64)> {
    use jet_codegen::AST::CtValue;
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "Measurement" {
        return None;
    }
    let field = |name: &str| {
        fields.iter().find_map(|(field, value)| {
            (field == name).then_some(value).and_then(|value| match value {
                CtValue::Float(value) => Some(value.as_f64()),
                _ => None,
            })
        })
    };
    Some((field("value")?, field("uncertainty")?))
}

fn alloc_measurement(rt: &mut JitRuntime, value: &jet_codegen::AST::CtValue) -> i64 {
    let Some((measured, uncertainty)) = measurement_parts(value) else {
        rt.set_trap("the canonical Measurement operation returned a malformed value");
        return 0;
    };
    let handle = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_float(handle, 0, measured);
    let _ = rt.heap.record_set_float(handle, 1, uncertainty);
    handle
}

fn read_measurement(rt: &mut JitRuntime, handle: i64) -> Option<jet_codegen::AST::CtValue> {
    measurement_ct(
        rt.heap.record_get_float(handle, 0)?,
        rt.heap.record_get_float(handle, 1)?,
    )
}

extern "C" fn jet_jit_measurement_new(value: f64, uncertainty: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match measurement_ct(value, uncertainty) {
        Some(value) => alloc_measurement(rt, &value),
        None => {
            rt.set_trap("the canonical Measurement constructor rejected Float inputs");
            0
        }
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
        let method = match op {
            0 => "add",
            1 => "sub",
            2 => "mul",
            3 => "div",
            _ => {
                rt.set_trap("the JIT received an invalid Measurement operation");
                return 0;
            }
        };
        match jet_codegen::Comptime::Builtins::apply_method(
            &left,
            method,
            vec![right],
            jet_codegen::Diagnostics::Span::new(0, 0),
        ) {
            Ok(value) => alloc_measurement(rt, &value),
            Err(_) => {
                rt.set_trap("the canonical Measurement operation rejected valid operands");
                0
            }
        }
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
        let Some(rendered) = jet_codegen::Comptime::display_core_pure_value(&value) else {
            rt.set_trap("the canonical Measurement display rejected a valid value");
            return 0;
        };
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
    Concurrency::with_runtime_mut(|rt| match value.checked_mul(scale) {
        Some(ms) => alloc_jit_result(rt, true, ms as u64),
        None => alloc_jit_result(rt, false, 0),
    })
}

extern "C" fn jet_jit_duration_from_float(value: f64, scale: i64) -> i64 {
    let ms = value * scale as f64;
    Concurrency::with_runtime_mut(|rt| {
        if ms.is_finite() && ms >= i64::MIN as f64 && ms < 9_223_372_036_854_775_808.0 {
            alloc_jit_result(rt, true, ms.trunc() as i64 as u64)
        } else {
            alloc_jit_result(rt, false, 0)
        }
    })
}

extern "C" fn jet_jit_duration_in(value: i64, scale: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, true, (value / scale) as u64))
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
    i8::from(value == 0)
}

extern "C" fn jet_jit_duration_total_seconds(value: i64) -> i64 {
    value / 1_000_000_000
}

extern "C" fn jet_jit_duration_difference(a: i64, b: i64) -> i64 {
    a.saturating_sub(b)
}

extern "C" fn jet_jit_result_new_f64(ok: i8, value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value.to_bits()))
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
    display: i64,
    fields: i64,
) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let type_name = rt.heap.clone_string(type_name).unwrap_or_default();
        let display = rt.heap.clone_string(display).unwrap_or_default();
        let field_len = rt.heap.list_len(fields).unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..field_len {
            let fh = rt.heap.list_get_int(fields, i).unwrap_or(0);
            let idx = (fh as usize).wrapping_sub(1);
            if let Some(slot) = rt.reflect_values.get(idx) {
                // field slots store single field in type_name/display misuse:
                // we store Field as ReflectSlot { type_name=name, display=value, fields=[] }
                out.push((slot.type_name.clone(), slot.display.clone()));
            }
        }
        rt.reflect_values.push(ReflectSlot {
            type_name,
            display,
            fields: out,
        });
        rt.reflect_values.len() as i64
    })
}

extern "C" fn jet_jit_reflect_field_new(name: i64, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let name = rt.heap.clone_string(name).unwrap_or_default();
        let value = rt.heap.clone_string(value).unwrap_or_default();
        rt.reflect_values.push(ReflectSlot {
            type_name: name,
            display: value,
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
            rt.reflect_values.push(ReflectSlot {
                type_name: name,
                display: value,
                fields: Vec::new(),
            });
            ids.push(rt.reflect_values.len() as i64);
        }
        rt.heap.alloc_int_list(ids)
    })
}

extern "C" fn jet_jit_reflect_field_name(handle: i64) -> i64 {
    jet_jit_reflect_type_name(handle)
}

extern "C" fn jet_jit_reflect_field_value(handle: i64) -> i64 {
    jet_jit_reflect_display(handle)
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
        sig_intn_binop.returns.push(AbiParam::new(types::I64));

        let mut sig_i64 = Signature::new(cc);
        sig_i64.params.push(AbiParam::new(types::I64));
        let mut sig_reflect_finish = Signature::new(cc);
        for _ in 0..3 {
            sig_reflect_finish.params.push(AbiParam::new(types::I64));
        }
        sig_reflect_finish.returns.push(AbiParam::new(types::I64));
        let mut sig_rich_panic = Signature::new(cc);
        for _ in 0..8 {
            sig_rich_panic.params.push(AbiParam::new(types::I64));
        }
        sig_rich_panic.returns.push(AbiParam::new(types::I64));
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
        let mut sig_deopt = Signature::new(cc);
        // fn_idx, argc, a0..a7
        for _ in 0..10 {
            sig_deopt.params.push(AbiParam::new(types::I64));
        }
        sig_deopt.returns.push(AbiParam::new(types::I64));
        let sig_noarg = Signature::new(cc);
    }
    #extra {
        coll: Collections::CollectionsHostFns,
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
    measurement_new: "jet_jit_measurement_new" => jet_jit_measurement_new: sig_measurement_new;
    measurement_arithmetic: "jet_jit_measurement_arithmetic" => jet_jit_measurement_arithmetic: sig_measurement_arithmetic;
    measurement_get: "jet_jit_measurement_get" => jet_jit_measurement_get: sig_measurement_get;
    measurement_show: "jet_jit_measurement_show" => jet_jit_measurement_show: sig_str_unary_i64;
    result_new_i64: "jet_jit_result_new_i64" => jet_jit_result_new_i64: sig_result_new_i64;
    result_new_f64: "jet_jit_result_new_f64" => jet_jit_result_new_f64: sig_result_new_f64;
    result_new_i8: "jet_jit_result_new_i8" => jet_jit_result_new_i8: sig_result_new_i8;
    result_new_i32: "jet_jit_result_new_i32" => jet_jit_result_new_i32: sig_result_new_i32;
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
    trace_err: "jet_jit_trace_err" => jet_jit_trace_err: sig_trace_err;
    result_context: "jet_jit_result_context" => jet_jit_result_context: sig_str_binary_i64;
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
    is_trapped: "jet_jit_is_trapped" => jet_jit_is_trapped: sig_is_trapped;
    deopt_call: "jet_deopt_call" => super::deopt::jet_deopt_call: sig_deopt;
    reflect_of_finish: "jet_jit_reflect_of_finish" => jet_jit_reflect_of_finish: sig_reflect_finish;
    reflect_field_new: "jet_jit_reflect_field_new" => jet_jit_reflect_field_new: sig_str_binary_i64;
    reflect_type_name: "jet_jit_reflect_type_name" => jet_jit_reflect_type_name: sig_str_unary_i64;
    reflect_display: "jet_jit_reflect_display" => jet_jit_reflect_display: sig_str_unary_i64;
    reflect_fields: "jet_jit_reflect_fields" => jet_jit_reflect_fields: sig_str_unary_i64;
    reflect_field_name: "jet_jit_reflect_field_name" => jet_jit_reflect_field_name: sig_str_unary_i64;
    reflect_field_value: "jet_jit_reflect_field_value" => jet_jit_reflect_field_value: sig_str_unary_i64;
    testing_temp_dir: "jet_jit_testing_temp_dir" => jet_jit_testing_temp_dir: sig_str_unary_i64;
    testing_snap: "jet_jit_testing_snap" => jet_jit_testing_snap: sig_str_eq;
    cli_main: "jet_jit_cli_main" => crate::CLI::jet_jit_cli_main: sig_noarg;
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
