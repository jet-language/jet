fn catch_jit_panic<R>(context: &str, f: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
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
        Err(_) => {
            resident_teardown();
            Err(format!(
                "jit {context} panicked before returning an unsupported reason"
            ))
        }
    }
}

/// Live heap carried across type-stable hot_swap (M2). `invocations` counts
/// how many times `main` ran without a clean restart — preserved on swap,
/// reset on restart.
pub(crate) struct JitRuntime {
    source_file: String,
    stdout: String,
    stderr: String,
    heap: jet_rt::JetArena,
    invocations: u64,
    channels: Vec<JetSchedulerChannel<i64>>,
    senders: Vec<JetSchedulerSender<i64>>,
    tasks: Vec<Option<JetSchedulerJoin<i64>>>,
    task_controls: Vec<std::sync::Arc<JetTaskControl>>,
    /// General `Result<T, E>` ABI arena. Handles are one-based indices; payload
    /// bits are interpreted from checked TIR types, never dynamically guessed.
    results: Vec<JitResultValue>,
    /// Set by a host shim when the user program hits a runtime panic (overflow,
    /// list index/slice OOB, a couple of concurrency panics). Non-`None` makes
    /// JIT-generated code branch to its epilogue on the next `emit_trap_check`,
    /// so the trap unwinds through pure Cranelift control flow (never a Rust
    /// panic through a JIT frame — I1). `resident_invoke` turns it into an
    /// `E0953` diagnostic, exactly as the tier-0 interpreter reports the same
    /// panic. Keeps the FIRST message; later traps on the unwind path are noise.
    trapped: Option<String>,
}

impl JitRuntime {
    /// Record a runtime panic. Keeps the first message (the unwind branch may
    /// re-enter trap sites with dummy values before the epilogue is reached).
    fn set_trap(&mut self, msg: &str) {
        if self.trapped.is_none() {
            self.trapped = Some(msg.to_string());
        }
    }
}

struct ResidentModule {
    module: JITModule,
    host: HostFns,
    main_id: FuncId,
    main_returns_result: bool,
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

extern "C" fn jet_jit_str_len(id: i64) -> i64 {
    with_runtime_result(0, |rt| {
        rt.heap
            .get_string(id)
            .map(jet_rt::string_len_chars)
            .unwrap_or(0)
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

extern "C" fn jet_jit_struct_new(n: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_record(n as usize))
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
static JIT_PERF_FIDELITY: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(JIT_PERF_DEFAULT_FIDELITY_BITS);

fn alloc_jit_result(rt: &mut JitRuntime, ok: bool, bits: u64) -> i64 {
    rt.results.push(JitResultValue { ok, bits });
    rt.results.len() as i64
}

fn jit_result(rt: &JitRuntime, handle: i64) -> Option<JitResultValue> {
    usize::try_from(handle)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| rt.results.get(index).copied())
}

extern "C" fn jet_jit_result_new_i64(ok: i8, value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value as u64))
}

extern "C" fn jet_jit_result_new_f64(ok: i8, value: f64) -> i64 {
    Concurrency::with_runtime_mut(|rt| alloc_jit_result(rt, ok != 0, value.to_bits()))
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
    f32::from_bits(JIT_PERF_FIDELITY.load(std::sync::atomic::Ordering::SeqCst)) as f64
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
        JIT_PERF_FIDELITY.store((value as f32).to_bits(), std::sync::atomic::Ordering::SeqCst);
        alloc_jit_result(rt, true, 0)
    })
}

extern "C" fn jet_jit_perf_reset_fidelity() {
    JIT_PERF_FIDELITY.store(
        JIT_PERF_DEFAULT_FIDELITY_BITS,
        std::sync::atomic::Ordering::SeqCst,
    );
}

struct HostFns {
    add_i64: FuncId,
    sub_i64: FuncId,
    mul_i64: FuncId,
    div_i64: FuncId,
    print_i64: FuncId,
    print_f64: FuncId,
    print_bool: FuncId,
    print_char: FuncId,
    print_str: FuncId,
    str_begin: FuncId,
    str_push_lit: FuncId,
    str_push_i64: FuncId,
    str_push_f64: FuncId,
    str_push_bool: FuncId,
    str_push_char: FuncId,
    str_push_str: FuncId,
    str_eq: FuncId,
    str_len: FuncId,
    str_trim: FuncId,
    str_to_upper: FuncId,
    str_to_lower: FuncId,
    str_replace: FuncId,
    struct_new: FuncId,
    struct_get_i64: FuncId,
    struct_get_f64: FuncId,
    struct_get_bool: FuncId,
    struct_get_char: FuncId,
    struct_get_str: FuncId,
    struct_set_i64: FuncId,
    struct_set_f64: FuncId,
    struct_set_bool: FuncId,
    struct_set_char: FuncId,
    struct_set_str: FuncId,
    result_new_i64: FuncId,
    result_new_f64: FuncId,
    result_new_i8: FuncId,
    result_new_i32: FuncId,
    result_is_ok: FuncId,
    result_get_i64: FuncId,
    result_get_f64: FuncId,
    result_get_i8: FuncId,
    result_get_i32: FuncId,
    perf_fidelity: FuncId,
    perf_default_fidelity: FuncId,
    perf_override_fidelity: FuncId,
    perf_reset_fidelity: FuncId,
    is_trapped: FuncId,
    coll: Collections::CollectionsHostFns,
    conc: Concurrency::ConcurrencyHostFns,
    num: Numeric::NumericHostFns,
}

fn new_jit_module() -> Result<(JITModule, HostFns), String> {
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
    builder.symbol("jet_jit_str_len", jet_jit_str_len as *const u8);
    builder.symbol("jet_jit_str_trim", jet_jit_str_trim as *const u8);
    builder.symbol("jet_jit_str_to_upper", jet_jit_str_to_upper as *const u8);
    builder.symbol("jet_jit_str_to_lower", jet_jit_str_to_lower as *const u8);
    builder.symbol("jet_jit_str_replace", jet_jit_str_replace as *const u8);
    builder.symbol("jet_jit_struct_new", jet_jit_struct_new as *const u8);
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
    builder.symbol("jet_jit_result_is_ok", jet_jit_result_is_ok as *const u8);
    builder.symbol("jet_jit_result_get_i64", jet_jit_result_get_i64 as *const u8);
    builder.symbol("jet_jit_result_get_f64", jet_jit_result_get_f64 as *const u8);
    builder.symbol("jet_jit_result_get_i8", jet_jit_result_get_i8 as *const u8);
    builder.symbol("jet_jit_result_get_i32", jet_jit_result_get_i32 as *const u8);
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
    Collections::register_collections_symbols(&mut builder);
    Concurrency::register_concurrency_symbols(&mut builder);
    Numeric::register_numeric_symbols(&mut builder);
    let mut module = JITModule::new(builder);
    let coll = Collections::declare_collections_host_fns(&mut module)?;
    let conc = Concurrency::declare_concurrency_host_fns(&mut module)?;
    let num = Numeric::declare_numeric_host_fns(&mut module)?;
    let host = declare_host_fns(&mut module, coll, conc, num)?;
    Ok((module, host))
}

fn declare_host_fns(
    module: &mut JITModule,
    coll: Collections::CollectionsHostFns,
    conc: Concurrency::ConcurrencyHostFns,
    num: Numeric::NumericHostFns,
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
    let mut sig_str_replace = Signature::new(cc);
    sig_str_replace.params.push(AbiParam::new(types::I64));
    sig_str_replace.params.push(AbiParam::new(types::I64));
    sig_str_replace.params.push(AbiParam::new(types::I64));
    sig_str_replace.returns.push(AbiParam::new(types::I64));
    let mut sig_str_begin = Signature::new(cc);
    sig_str_begin.returns.push(AbiParam::new(types::I64));
    let mut sig_struct_new = Signature::new(cc);
    sig_struct_new.params.push(AbiParam::new(types::I64));
    sig_struct_new.returns.push(AbiParam::new(types::I64));
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
    let mut sig_noarg_f64 = Signature::new(cc);
    sig_noarg_f64.returns.push(AbiParam::new(types::F64));
    let mut sig_perf_override = Signature::new(cc);
    sig_perf_override.params.push(AbiParam::new(types::F64));
    sig_perf_override.returns.push(AbiParam::new(types::I64));
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
        str_len: import("jet_jit_str_len", &sig_str_unary_i64)?,
        str_trim: import("jet_jit_str_trim", &sig_str_unary_i64)?,
        str_to_upper: import("jet_jit_str_to_upper", &sig_str_unary_i64)?,
        str_to_lower: import("jet_jit_str_to_lower", &sig_str_unary_i64)?,
        str_replace: import("jet_jit_str_replace", &sig_str_replace)?,
        struct_new: import("jet_jit_struct_new", &sig_struct_new)?,
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
        result_is_ok: import("jet_jit_result_is_ok", &sig_result_query_i8)?,
        result_get_i64: import("jet_jit_result_get_i64", &sig_result_query_i64)?,
        result_get_f64: import("jet_jit_result_get_f64", &sig_result_query_f64)?,
        result_get_i8: import("jet_jit_result_get_i8", &sig_result_query_i8)?,
        result_get_i32: import("jet_jit_result_get_i32", &sig_result_query_i32)?,
        perf_fidelity: import("jet_jit_perf_fidelity", &sig_noarg_f64)?,
        perf_default_fidelity: import("jet_jit_perf_default_fidelity", &sig_noarg_f64)?,
        perf_override_fidelity: import("jet_jit_perf_override_fidelity", &sig_perf_override)?,
        perf_reset_fidelity: import("jet_jit_perf_reset_fidelity", &sig_noarg)?,
        is_trapped: import("jet_jit_is_trapped", &sig_is_trapped)?,
        coll,
        conc,
        num,
    })
}
