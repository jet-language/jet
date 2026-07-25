use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_codegen::scheduler::{
    JetSchedulerChannel, JetSchedulerJoin, JetSchedulerSender, JetTaskControl,
};
use std::panic::{catch_unwind, AssertUnwindSafe};

use super::resident::resident_teardown;
use super::{
    Archive, Collections, Compress, Concurrency, CoreHost, Encoding, Fmt, JitResultValue, Numeric,
    Process, Random, Sketch, Solver, Text, TRY_COMPILE_PANIC_HOOK_LOCK,
};

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
pub(crate) struct JitRuntime {
    pub(crate) source_file: String,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) heap: jet_rt::JetArena,
    pub(crate) invocations: u64,
    pub(crate) channels: Vec<JetSchedulerChannel<i64>>,
    pub(crate) senders: Vec<JetSchedulerSender<i64>>,
    pub(crate) tasks: Vec<Option<JetSchedulerJoin<i64>>>,
    pub(crate) task_controls: Vec<std::sync::Arc<JetTaskControl>>,
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
    pub(crate) json_readers: Vec<crate::enc_stream::JsonReaderSlot>,
    pub(crate) json_writers: Vec<crate::enc_stream::JsonWriterSlot>,
    pub(crate) jsonl_readers: Vec<crate::enc_stream::JsonlReaderSlot>,
    pub(crate) jsonl_writers: Vec<crate::enc_stream::JsonlWriterSlot>,
    pub(crate) csv_readers: Vec<crate::enc_stream::CsvReaderSlot>,
    pub(crate) csv_writers: Vec<crate::enc_stream::CsvWriterSlot>,
    pub(crate) xml_readers: Vec<crate::enc_stream::XmlReaderSlot>,
    pub(crate) xml_writers: Vec<crate::enc_stream::XmlWriterSlot>,
    pub(crate) cbor_readers: Vec<crate::enc_stream::CborReaderSlot>,
    pub(crate) cbor_writers: Vec<crate::enc_stream::CborWriterSlot>,
    /// `Set<T>` handles — 1-based indices (#729 collections/set). Int elems only.
    pub(crate) sets: Vec<std::collections::HashSet<i64>>,
    /// `Deque<T>` handles — 1-based indices (#729 collections/deque). Int elems only.
    pub(crate) deques: Vec<std::collections::VecDeque<i64>>,
    /// Set by a host shim when the user program hits a runtime panic (overflow,
    /// list index/slice OOB, a couple of concurrency panics). Non-`None` makes
    /// JIT-generated code branch to its epilogue on the next `emit_trap_check`,
    /// so the trap unwinds through pure Cranelift control flow (never a Rust
    /// panic through a JIT frame — I1). `resident_invoke` turns it into an
    /// `E0953` diagnostic, exactly as the tier-0 interpreter reports the same
    /// panic. Keeps the FIRST message; later traps on the unwind path are noise.
    pub(crate) trapped: Option<String>,
    /// Compiler-owned E3003 rendered after native code returns; never unwinds
    /// through a Cranelift frame.
    pub(crate) deadline_exceeded: Option<String>,
}

impl JitRuntime {
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
    let msg = match op {
        "add" => "this addition overflows the value's type (the result is outside its range)",
        "sub" => "this subtraction overflows the value's type (the result is outside its range)",
        "mul" => "this multiplication overflows the value's type (the result is outside its range)",
        "div" => "this division can't be done (dividing by zero, or overflow)",
        _ => "this operation overflows the value's type (the result is outside its range)",
    };
    with_runtime_mut(|rt| rt.set_trap(msg));
}

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

extern "C" fn jet_jit_trap_panic(_unused: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_trap("panic");
        0
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

extern "C" fn jet_jit_numeric_bit_count(value: i64, op: i64) -> i64 {
    match op {
        0 => value.count_ones() as i64,
        1 => value.count_zeros() as i64,
        2 => value.leading_zeros() as i64,
        _ => value.trailing_zeros() as i64,
    }
}

extern "C" fn jet_jit_struct_new(n: i64) -> i64 {
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

fn alloc_jit_result(rt: &mut JitRuntime, ok: bool, bits: u64) -> i64 {
    rt.results.push(JitResultValue { ok, bits });
    rt.results.len() as i64
}

pub(crate) fn jit_result(rt: &JitRuntime, handle: i64) -> Option<JitResultValue> {
    usize::try_from(handle)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| rt.results.get(index).copied())
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

pub(crate) struct HostFns {
    pub(crate) add_i64: FuncId,
    pub(crate) sub_i64: FuncId,
    pub(crate) mul_i64: FuncId,
    pub(crate) div_i64: FuncId,
    pub(crate) print_i64: FuncId,
    pub(crate) print_f64: FuncId,
    pub(crate) print_bool: FuncId,
    pub(crate) print_char: FuncId,
    pub(crate) print_str: FuncId,
    pub(crate) str_begin: FuncId,
    pub(crate) str_push_lit: FuncId,
    pub(crate) str_push_i64: FuncId,
    pub(crate) str_push_f64: FuncId,
    pub(crate) str_push_bool: FuncId,
    pub(crate) str_push_char: FuncId,
    pub(crate) str_push_str: FuncId,
    pub(crate) str_eq: FuncId,
    pub(crate) str_contains: FuncId,
    pub(crate) str_clone: FuncId,
    pub(crate) str_len: FuncId,
    pub(crate) str_byte_len: FuncId,
    pub(crate) str_is_ascii: FuncId,
    pub(crate) str_trim: FuncId,
    pub(crate) str_to_upper: FuncId,
    pub(crate) str_to_lower: FuncId,
    pub(crate) str_replace: FuncId,
    pub(crate) str_lines: FuncId,
    pub(crate) str_split: FuncId,
    pub(crate) str_chars: FuncId,
    pub(crate) str_scalar_strings: FuncId,
    pub(crate) str_after: FuncId,
    pub(crate) str_before: FuncId,
    pub(crate) str_slice: FuncId,
    pub(crate) clock_new: FuncId,
    pub(crate) parse_i64: FuncId,
    pub(crate) parse_f64: FuncId,
    pub(crate) numeric_try_i64: FuncId,
    pub(crate) numeric_float_to_int: FuncId,
    pub(crate) numeric_float_narrow: FuncId,
    pub(crate) distinct_range: FuncId,
    pub(crate) distinct_range_result: FuncId,
    pub(crate) numeric_predicate: FuncId,
    pub(crate) numeric_bit_count: FuncId,
    pub(crate) struct_new: FuncId,
    pub(crate) struct_assign: FuncId,
    pub(crate) struct_get_i64: FuncId,
    pub(crate) struct_get_f64: FuncId,
    pub(crate) struct_get_bool: FuncId,
    pub(crate) struct_get_char: FuncId,
    pub(crate) struct_get_str: FuncId,
    pub(crate) struct_set_i64: FuncId,
    pub(crate) struct_set_f64: FuncId,
    pub(crate) struct_set_bool: FuncId,
    pub(crate) struct_set_char: FuncId,
    pub(crate) struct_set_str: FuncId,
    pub(crate) result_new_i64: FuncId,
    pub(crate) result_new_f64: FuncId,
    pub(crate) result_new_i8: FuncId,
    pub(crate) result_new_i32: FuncId,
    pub(crate) unit_convert_exact: FuncId,
    pub(crate) unit_convert_rounded: FuncId,
    pub(crate) unit_convert_implicit: FuncId,
    pub(crate) result_is_ok: FuncId,
    pub(crate) result_get_i64: FuncId,
    pub(crate) result_get_f64: FuncId,
    pub(crate) result_get_i8: FuncId,
    pub(crate) result_get_i32: FuncId,
    pub(crate) trap_panic: FuncId,
    pub(crate) duration_from_int: FuncId,
    pub(crate) duration_from_float: FuncId,
    pub(crate) duration_in: FuncId,
    pub(crate) perf_fidelity: FuncId,
    pub(crate) perf_default_fidelity: FuncId,
    pub(crate) perf_override_fidelity: FuncId,
    pub(crate) perf_reset_fidelity: FuncId,
    pub(crate) is_trapped: FuncId,
    pub(crate) deopt_call: FuncId,
    pub(crate) coll: Collections::CollectionsHostFns,
    pub(crate) conc: Concurrency::ConcurrencyHostFns,
    pub(crate) core: CoreHost::CoreHostFns,
    pub(crate) encoding: Encoding::EncodingHostFns,
    pub(crate) stream: crate::enc_stream::StreamHostFns,
    pub(crate) fmt: Fmt::FmtHostFns,
    pub(crate) compress: Compress::CompressHostFns,
    pub(crate) archive: Archive::ArchiveHostFns,
    pub(crate) process: Process::ProcessHostFns,
    pub(crate) num: Numeric::NumericHostFns,
    pub(crate) solver: Solver::SolverHostFns,
    pub(crate) random: Random::RandomHostFns,
    pub(crate) text: crate::Text::TextHostFns,
    pub(crate) sketch: crate::Sketch::SketchHostFns,
    pub(crate) args: crate::Args::ArgsHostFns,
}

pub(crate) fn new_jit_module() -> Result<(JITModule, HostFns), String> {
    let mut builder =
        JITBuilder::new(cranelift_module::default_libcall_names()).map_err(|e| e.to_string())?;
    builder.symbol("jet_jit_add_i64", jet_jit_add_i64 as *const u8);
    builder.symbol("jet_jit_sub_i64", jet_jit_sub_i64 as *const u8);
    builder.symbol("jet_jit_mul_i64", jet_jit_mul_i64 as *const u8);
    builder.symbol("jet_jit_div_i64", jet_jit_div_i64 as *const u8);
    builder.symbol("jet_jit_print_i64", jet_jit_print_i64 as *const u8);
    builder.symbol("jet_jit_print_f64", jet_jit_print_f64 as *const u8);
    builder.symbol("jet_jit_print_bool", jet_jit_print_bool as *const u8);
    builder.symbol("jet_jit_print_char", jet_jit_print_char as *const u8);
    builder.symbol("jet_jit_print_str", jet_jit_print_str as *const u8);
    builder.symbol("jet_jit_str_begin", jet_jit_str_begin as *const u8);
    builder.symbol("jet_jit_str_push_lit", jet_jit_str_push_lit as *const u8);
    builder.symbol("jet_jit_str_push_i64", jet_jit_str_push_i64 as *const u8);
    builder.symbol("jet_jit_str_push_f64", jet_jit_str_push_f64 as *const u8);
    builder.symbol("jet_jit_str_push_bool", jet_jit_str_push_bool as *const u8);
    builder.symbol("jet_jit_str_push_char", jet_jit_str_push_char as *const u8);
    builder.symbol("jet_jit_str_push_str", jet_jit_str_push_str as *const u8);
    builder.symbol("jet_jit_str_eq", jet_jit_str_eq as *const u8);
    builder.symbol("jet_jit_str_contains", jet_jit_str_contains as *const u8);
    builder.symbol("jet_jit_str_clone", jet_jit_str_clone as *const u8);
    builder.symbol("jet_jit_str_len", jet_jit_str_len as *const u8);
    builder.symbol("jet_jit_str_byte_len", jet_jit_str_byte_len as *const u8);
    builder.symbol("jet_jit_str_is_ascii", jet_jit_str_is_ascii as *const u8);
    builder.symbol("jet_jit_str_trim", jet_jit_str_trim as *const u8);
    builder.symbol("jet_jit_str_to_upper", jet_jit_str_to_upper as *const u8);
    builder.symbol("jet_jit_str_to_lower", jet_jit_str_to_lower as *const u8);
    builder.symbol("jet_jit_str_replace", jet_jit_str_replace as *const u8);
    builder.symbol("jet_jit_str_lines", jet_jit_str_lines as *const u8);
    builder.symbol("jet_jit_str_split", jet_jit_str_split as *const u8);
    builder.symbol("jet_jit_str_chars", jet_jit_str_chars as *const u8);
    builder.symbol("jet_jit_str_scalar_strings", jet_jit_str_scalar_strings as *const u8);
    builder.symbol("jet_jit_str_after", jet_jit_str_after as *const u8);
    builder.symbol("jet_jit_str_before", jet_jit_str_before as *const u8);
    builder.symbol("jet_jit_str_slice", jet_jit_str_slice as *const u8);
    builder.symbol("jet_jit_clock_new", jet_jit_clock_new as *const u8);
    builder.symbol("jet_jit_trap_panic", jet_jit_trap_panic as *const u8);
    builder.symbol("jet_jit_parse_i64", jet_jit_parse_i64 as *const u8);
    builder.symbol("jet_jit_parse_f64", jet_jit_parse_f64 as *const u8);
    builder.symbol("jet_jit_numeric_try_i64", jet_jit_numeric_try_i64 as *const u8);
    builder.symbol("jet_jit_numeric_float_to_int", jet_jit_numeric_float_to_int as *const u8);
    builder.symbol("jet_jit_numeric_float_narrow", jet_jit_numeric_float_narrow as *const u8);
    builder.symbol("jet_jit_distinct_range", jet_jit_distinct_range as *const u8);
    builder.symbol("jet_jit_distinct_range_result", jet_jit_distinct_range_result as *const u8);
    builder.symbol("jet_jit_numeric_predicate", jet_jit_numeric_predicate as *const u8);
    builder.symbol("jet_jit_numeric_bit_count", jet_jit_numeric_bit_count as *const u8);
    builder.symbol("jet_jit_struct_new", jet_jit_struct_new as *const u8);
    builder.symbol("jet_jit_struct_assign", jet_jit_struct_assign as *const u8);
    builder.symbol(
        "jet_jit_struct_get_i64",
        jet_jit_struct_get_i64 as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_get_f64",
        jet_jit_struct_get_f64 as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_get_bool",
        jet_jit_struct_get_bool as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_get_char",
        jet_jit_struct_get_char as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_get_str",
        jet_jit_struct_get_str as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_set_i64",
        jet_jit_struct_set_i64 as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_set_f64",
        jet_jit_struct_set_f64 as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_set_bool",
        jet_jit_struct_set_bool as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_set_char",
        jet_jit_struct_set_char as *const u8,
    );
    builder.symbol(
        "jet_jit_struct_set_str",
        jet_jit_struct_set_str as *const u8,
    );
    builder.symbol("jet_jit_result_new_i64", jet_jit_result_new_i64 as *const u8);
    builder.symbol("jet_jit_result_new_f64", jet_jit_result_new_f64 as *const u8);
    builder.symbol("jet_jit_result_new_i8", jet_jit_result_new_i8 as *const u8);
    builder.symbol("jet_jit_result_new_i32", jet_jit_result_new_i32 as *const u8);
    builder.symbol("jet_jit_unit_convert_exact", jet_jit_unit_convert_exact as *const u8);
    builder.symbol("jet_jit_unit_convert_rounded", jet_jit_unit_convert_rounded as *const u8);
    builder.symbol("jet_jit_unit_convert_implicit", jet_jit_unit_convert_implicit as *const u8);
    builder.symbol("jet_jit_result_is_ok", jet_jit_result_is_ok as *const u8);
    builder.symbol("jet_jit_result_get_i64", jet_jit_result_get_i64 as *const u8);
    builder.symbol("jet_jit_result_get_f64", jet_jit_result_get_f64 as *const u8);
    builder.symbol("jet_jit_result_get_i8", jet_jit_result_get_i8 as *const u8);
    builder.symbol("jet_jit_result_get_i32", jet_jit_result_get_i32 as *const u8);
    builder.symbol("jet_jit_duration_from_int", jet_jit_duration_from_int as *const u8);
    builder.symbol("jet_jit_duration_from_float", jet_jit_duration_from_float as *const u8);
    builder.symbol("jet_jit_duration_in", jet_jit_duration_in as *const u8);
    builder.symbol("jet_jit_perf_fidelity", jet_jit_perf_fidelity as *const u8);
    builder.symbol(
        "jet_jit_perf_default_fidelity",
        jet_jit_perf_default_fidelity as *const u8,
    );
    builder.symbol(
        "jet_jit_perf_override_fidelity",
        jet_jit_perf_override_fidelity as *const u8,
    );
    builder.symbol(
        "jet_jit_perf_reset_fidelity",
        jet_jit_perf_reset_fidelity as *const u8,
    );
    builder.symbol("jet_jit_is_trapped", jet_jit_is_trapped as *const u8);
    builder.symbol("jet_deopt_call", super::deopt::jet_deopt_call as *const u8);
    Collections::register_collections_symbols(&mut builder);
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
    let mut module = JITModule::new(builder);
    let coll = Collections::declare_collections_host_fns(&mut module)?;
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
    let host = declare_host_fns(
        &mut module,
        coll,
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
    )?;
    Ok((module, host))
}

fn declare_host_fns(
    module: &mut JITModule,
    coll: Collections::CollectionsHostFns,
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
) -> Result<HostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut sig_bin_i64 = Signature::new(cc);
    sig_bin_i64.params.push(AbiParam::new(types::I64));
    sig_bin_i64.params.push(AbiParam::new(types::I64));
    sig_bin_i64.params.push(AbiParam::new(types::I32));
    sig_bin_i64.returns.push(AbiParam::new(types::I64));

    let mut sig_i64 = Signature::new(cc);
    sig_i64.params.push(AbiParam::new(types::I64));
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
    let mut sig_is_trapped = Signature::new(cc);
    sig_is_trapped.returns.push(AbiParam::new(types::I64));
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

    let mut import = |name: &str, sig: &Signature| -> Result<FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

    Ok(HostFns {
        add_i64: import("jet_jit_add_i64", &sig_bin_i64)?,
        sub_i64: import("jet_jit_sub_i64", &sig_bin_i64)?,
        mul_i64: import("jet_jit_mul_i64", &sig_bin_i64)?,
        div_i64: import("jet_jit_div_i64", &sig_bin_i64)?,
        print_i64: import("jet_jit_print_i64", &sig_i64)?,
        print_f64: import("jet_jit_print_f64", &sig_f64)?,
        print_bool: import("jet_jit_print_bool", &sig_i8)?,
        print_char: import("jet_jit_print_char", &sig_i32)?,
        print_str: import("jet_jit_print_str", &sig_i64)?,
        str_begin: import("jet_jit_str_begin", &sig_str_begin)?,
        str_push_lit: import("jet_jit_str_push_lit", &sig_str_push_lit)?,
        str_push_i64: import("jet_jit_str_push_i64", &sig_str_push_i64)?,
        str_push_f64: import("jet_jit_str_push_f64", &sig_str_push_f64)?,
        str_push_bool: import("jet_jit_str_push_bool", &sig_str_push_bool)?,
        str_push_char: import("jet_jit_str_push_char", &sig_str_push_char)?,
        str_push_str: import("jet_jit_str_push_str", &sig_str_push_lit)?,
        str_eq: import("jet_jit_str_eq", &sig_str_eq)?,
        str_contains: import("jet_jit_str_contains", &sig_str_eq)?,
        str_clone: import("jet_jit_str_clone", &sig_str_unary_i64)?,
        str_len: import("jet_jit_str_len", &sig_str_unary_i64)?,
        str_byte_len: import("jet_jit_str_byte_len", &sig_str_unary_i64)?,
        str_is_ascii: import("jet_jit_str_is_ascii", &sig_str_unary_i8)?,
        str_trim: import("jet_jit_str_trim", &sig_str_unary_i64)?,
        str_to_upper: import("jet_jit_str_to_upper", &sig_str_unary_i64)?,
        str_to_lower: import("jet_jit_str_to_lower", &sig_str_unary_i64)?,
        str_replace: import("jet_jit_str_replace", &sig_str_replace)?,
        str_lines: import("jet_jit_str_lines", &sig_str_unary_i64)?,
        str_split: import("jet_jit_str_split", &sig_str_binary_i64)?,
        str_chars: import("jet_jit_str_chars", &sig_str_unary_i64)?,
        str_scalar_strings: import("jet_jit_str_scalar_strings", &sig_str_unary_i64)?,
        str_after: import("jet_jit_str_after", &sig_str_binary_i64)?,
        str_before: import("jet_jit_str_before", &sig_str_binary_i64)?,
        str_slice: import("jet_jit_str_slice", &sig_str_replace)?,
        clock_new: import("jet_jit_clock_new", &sig_str_unary_i64)?,
        parse_i64: import("jet_jit_parse_i64", &sig_str_unary_i64)?,
        parse_f64: import("jet_jit_parse_f64", &sig_str_unary_i64)?,
        numeric_try_i64: import("jet_jit_numeric_try_i64", &sig_i64_i64_i64_i64)?,
        numeric_float_to_int: import("jet_jit_numeric_float_to_int", &sig_f64_i64_i64)?,
        numeric_float_narrow: import("jet_jit_numeric_float_narrow", &sig_f64_i64)?,
        distinct_range: import("jet_jit_distinct_range", &sig_i64_i64_i64_i64)?,
        distinct_range_result: import("jet_jit_distinct_range_result", &sig_i64_i64_i64_i64)?,
        numeric_predicate: import("jet_jit_numeric_predicate", &sig_f64_i64_i8)?,
        numeric_bit_count: import("jet_jit_numeric_bit_count", &sig_i64_i64_i64)?,
        struct_new: import("jet_jit_struct_new", &sig_struct_new)?,
        struct_assign: import("jet_jit_struct_assign", &sig_struct_assign)?,
        struct_get_i64: import("jet_jit_struct_get_i64", &sig_struct_get_i64)?,
        struct_get_f64: import("jet_jit_struct_get_f64", &sig_struct_get_f64)?,
        struct_get_bool: import("jet_jit_struct_get_bool", &sig_struct_get_i8)?,
        struct_get_char: import("jet_jit_struct_get_char", &sig_struct_get_i32)?,
        struct_get_str: import("jet_jit_struct_get_str", &sig_struct_get_i64)?,
        struct_set_i64: import("jet_jit_struct_set_i64", &sig_struct_set_i64)?,
        struct_set_f64: import("jet_jit_struct_set_f64", &sig_struct_set_f64)?,
        struct_set_bool: import("jet_jit_struct_set_bool", &sig_struct_set_i8)?,
        struct_set_char: import("jet_jit_struct_set_char", &sig_struct_set_i32)?,
        struct_set_str: import("jet_jit_struct_set_str", &sig_struct_set_i64)?,
        result_new_i64: import("jet_jit_result_new_i64", &sig_result_new_i64)?,
        result_new_f64: import("jet_jit_result_new_f64", &sig_result_new_f64)?,
        result_new_i8: import("jet_jit_result_new_i8", &sig_result_new_i8)?,
        result_new_i32: import("jet_jit_result_new_i32", &sig_result_new_i32)?,
        unit_convert_exact: import("jet_jit_unit_convert_exact", &sig_unit_convert_exact)?,
        unit_convert_rounded: import("jet_jit_unit_convert_rounded", &sig_unit_convert_rounded)?,
        unit_convert_implicit: import("jet_jit_unit_convert_implicit", &sig_unit_convert_implicit)?,
        result_is_ok: import("jet_jit_result_is_ok", &sig_result_query_i8)?,
        result_get_i64: import("jet_jit_result_get_i64", &sig_result_query_i64)?,
        result_get_f64: import("jet_jit_result_get_f64", &sig_result_query_f64)?,
        result_get_i8: import("jet_jit_result_get_i8", &sig_result_query_i8)?,
        result_get_i32: import("jet_jit_result_get_i32", &sig_result_query_i32)?,
        trap_panic: import("jet_jit_trap_panic", &sig_i64)?,
        duration_from_int: import("jet_jit_duration_from_int", &sig_duration_int)?,
        duration_from_float: import("jet_jit_duration_from_float", &sig_duration_float)?,
        duration_in: import("jet_jit_duration_in", &sig_duration_int)?,
        perf_fidelity: import("jet_jit_perf_fidelity", &sig_noarg_f64)?,
        perf_default_fidelity: import("jet_jit_perf_default_fidelity", &sig_noarg_f64)?,
        perf_override_fidelity: import("jet_jit_perf_override_fidelity", &sig_perf_override)?,
        perf_reset_fidelity: import("jet_jit_perf_reset_fidelity", &sig_noarg)?,
        is_trapped: import("jet_jit_is_trapped", &sig_is_trapped)?,
        deopt_call: import("jet_deopt_call", &sig_deopt)?,
        coll,
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
    })
}
