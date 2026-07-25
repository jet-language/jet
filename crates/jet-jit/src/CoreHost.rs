//! Host shims for `core.os`, `jet.log`, and `core.math` CoreCalls (#729).
//! Behavior mirrors AOT helpers in the CoreLib prelude (`jet_std_os_*`,
//! `jet_ring_log_*`, `jet_std_math_*`) — thin std/libm wrappers, not a third algorithm.

use super::Concurrency;
use std::cell::Cell;

// ── core.os (mirrors jet_std_os_* in FsIoEnvOsTesting.rs) ────────────────────

extern "C" fn jet_jit_os_name() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(std::env::consts::OS.to_string()))
}

extern "C" fn jet_jit_os_family() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(std::env::consts::FAMILY.to_string()))
}

extern "C" fn jet_jit_os_arch() -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(std::env::consts::ARCH.to_string()))
}

extern "C" fn jet_jit_os_cpu_count() -> i64 {
    std::thread::available_parallelism()
        .map(|n| n.get() as i64)
        .unwrap_or(1)
}

extern "C" fn jet_jit_os_temp_dir() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.heap
            .alloc_string(std::env::temp_dir().to_string_lossy().to_string())
    })
}

extern "C" fn jet_jit_os_executable() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        rt.heap.alloc_string(path)
    })
}

extern "C" fn jet_jit_os_pid() -> i64 {
    std::process::id() as i64
}

extern "C" fn jet_jit_os_hostname() -> i64 {
    let host = std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| "localhost".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(host))
}

// ── jet.log (mirrors jet_ring_log_* in RingCsvLogTimeCrypto.rs) ───────────────
// Level: 0=debug, 1=info, 2=warn, 3=error. Format: 0=auto, 1=json, 2=text.

thread_local! {
    static JIT_LOG_LEVEL: Cell<u8> = const { Cell::new(1) };
    static JIT_LOG_FORMAT: Cell<u8> = const { Cell::new(0) };
}

fn jit_log_set_level_str(level: &str) {
    let n: u8 = match level {
        "debug" => 0,
        "info" => 1,
        "warn" => 2,
        "error" => 3,
        _ => 1,
    };
    JIT_LOG_LEVEL.with(|l| l.set(n));
}

fn jit_log_setup_str(format: &str) {
    let n: u8 = match format {
        "json" => 1,
        "text" => 2,
        _ => 0,
    };
    JIT_LOG_FORMAT.with(|f| f.set(n));
}

fn jit_log_format_active() -> u8 {
    let explicit = JIT_LOG_FORMAT.with(|f| f.get());
    if explicit != 0 {
        return explicit;
    }
    use std::io::IsTerminal;
    if std::io::stderr().is_terminal() {
        2
    } else {
        1
    }
}

fn jit_log_json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Mirrors AOT `unix_to_ymdhms` in RingCsvLogTimeCrypto.rs.
fn unix_to_ymdhms(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let mut days = secs / 86400;
    let time_of_day = (secs % 86400).unsigned_abs();
    let h = (time_of_day / 3600) as u32;
    let mi = ((time_of_day % 3600) / 60) as u32;
    let s = (time_of_day % 60) as u32;
    let mut year: i32 = 1970;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    let month_days: [i64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month: u32 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, (days + 1) as u32, h, mi, s)
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn clone_heap_string(id: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(id).unwrap_or_default())
}

fn jit_log_emit(level: &str, msg: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    let line = if jit_log_format_active() == 2 {
        let secs = ts / 1000;
        let (y, mo, d, h, mi, s) = unix_to_ymdhms(secs);
        let level_tag = match level {
            "debug" => "DEBUG",
            "info" => "INFO",
            "warn" => "WARN",
            "error" => "ERROR",
            _ => level,
        };
        format!("[{level_tag}] {y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z | {msg}")
    } else {
        format!(
            "{{\"level\":\"{}\",\"body\":\"{}\",\"ts\":{}}}",
            level,
            jit_log_json_escape(msg),
            ts
        )
    };
    Concurrency::with_runtime_mut(|rt| {
        rt.stderr.push_str(&line);
        rt.stderr.push('\n');
    });
}

extern "C" fn jet_jit_log_set_level(msg: i64) {
    jit_log_set_level_str(&clone_heap_string(msg));
}

extern "C" fn jet_jit_log_setup(msg: i64) {
    jit_log_setup_str(&clone_heap_string(msg));
}

extern "C" fn jet_jit_log_debug(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 0 {
        jit_log_emit("debug", &clone_heap_string(msg));
    }
}

extern "C" fn jet_jit_log_info(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 1 {
        jit_log_emit("info", &clone_heap_string(msg));
    }
}

extern "C" fn jet_jit_log_warn(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 2 {
        jit_log_emit("warn", &clone_heap_string(msg));
    }
}

extern "C" fn jet_jit_log_error(msg: i64) {
    if JIT_LOG_LEVEL.with(|l| l.get()) <= 3 {
        jit_log_emit("error", &clone_heap_string(msg));
    }
}

// ── core.math (mirrors jet_std_math_* / f64 methods in Process.rs emit) ───────

extern "C" fn jet_jit_math_sin(x: f64) -> f64 {
    x.sin()
}
extern "C" fn jet_jit_math_cos(x: f64) -> f64 {
    x.cos()
}
extern "C" fn jet_jit_math_exp(x: f64) -> f64 {
    x.exp()
}
extern "C" fn jet_jit_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}
extern "C" fn jet_jit_math_hypot(a: f64, b: f64) -> f64 {
    a.hypot(b)
}
extern "C" fn jet_jit_math_lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}
extern "C" fn jet_jit_math_degrees(x: f64) -> f64 {
    x.to_degrees()
}
extern "C" fn jet_jit_math_radians(x: f64) -> f64 {
    x.to_radians()
}
extern "C" fn jet_jit_math_is_finite(x: f64) -> i8 {
    i8::from(x.is_finite())
}
extern "C" fn jet_jit_math_sign(x: f64) -> i64 {
    if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }
}

/// Packed Option<i64> ABI: `0` = None, else `bits.wrapping_add(1)`.
extern "C" fn jet_jit_math_checked_add(a: i64, b: i64) -> i64 {
    match a.checked_add(b) {
        Some(v) => v.wrapping_add(1),
        None => 0,
    }
}

extern "C" fn jet_jit_math_saturating_add(a: i64, b: i64) -> i64 {
    a.saturating_add(b)
}
extern "C" fn jet_jit_math_wrapping_add(a: i64, b: i64) -> i64 {
    a.wrapping_add(b)
}

/// Mirrors `jet_std_math_int_pow`.
extern "C" fn jet_jit_math_int_pow(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0;
    }
    base.saturating_pow(exp as u32)
}

/// Mirrors `jet_std_math_gcd`.
extern "C" fn jet_jit_math_gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

/// Mirrors `jet_std_math_lcm`.
extern "C" fn jet_jit_math_lcm(a: i64, b: i64) -> i64 {
    if a == 0 || b == 0 {
        0
    } else {
        (a / jet_jit_math_gcd(a, b)).saturating_mul(b).abs()
    }
}

pub(crate) struct CoreHostFns {
    pub os_name: cranelift_module::FuncId,
    pub os_family: cranelift_module::FuncId,
    pub os_arch: cranelift_module::FuncId,
    pub os_cpu_count: cranelift_module::FuncId,
    pub os_temp_dir: cranelift_module::FuncId,
    pub os_executable: cranelift_module::FuncId,
    pub os_pid: cranelift_module::FuncId,
    pub os_hostname: cranelift_module::FuncId,
    pub log_set_level: cranelift_module::FuncId,
    pub log_setup: cranelift_module::FuncId,
    pub log_debug: cranelift_module::FuncId,
    pub log_info: cranelift_module::FuncId,
    pub log_warn: cranelift_module::FuncId,
    pub log_error: cranelift_module::FuncId,
    pub math_sin: cranelift_module::FuncId,
    pub math_cos: cranelift_module::FuncId,
    pub math_exp: cranelift_module::FuncId,
    pub math_atan2: cranelift_module::FuncId,
    pub math_hypot: cranelift_module::FuncId,
    pub math_lerp: cranelift_module::FuncId,
    pub math_degrees: cranelift_module::FuncId,
    pub math_radians: cranelift_module::FuncId,
    pub math_is_finite: cranelift_module::FuncId,
    pub math_sign: cranelift_module::FuncId,
    pub math_checked_add: cranelift_module::FuncId,
    pub math_saturating_add: cranelift_module::FuncId,
    pub math_wrapping_add: cranelift_module::FuncId,
    pub math_int_pow: cranelift_module::FuncId,
    pub math_gcd: cranelift_module::FuncId,
    pub math_lcm: cranelift_module::FuncId,
}

pub(crate) fn register_core_host_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_os_name", jet_jit_os_name as *const u8);
    builder.symbol("jet_jit_os_family", jet_jit_os_family as *const u8);
    builder.symbol("jet_jit_os_arch", jet_jit_os_arch as *const u8);
    builder.symbol("jet_jit_os_cpu_count", jet_jit_os_cpu_count as *const u8);
    builder.symbol("jet_jit_os_temp_dir", jet_jit_os_temp_dir as *const u8);
    builder.symbol("jet_jit_os_executable", jet_jit_os_executable as *const u8);
    builder.symbol("jet_jit_os_pid", jet_jit_os_pid as *const u8);
    builder.symbol("jet_jit_os_hostname", jet_jit_os_hostname as *const u8);
    builder.symbol("jet_jit_log_set_level", jet_jit_log_set_level as *const u8);
    builder.symbol("jet_jit_log_setup", jet_jit_log_setup as *const u8);
    builder.symbol("jet_jit_log_debug", jet_jit_log_debug as *const u8);
    builder.symbol("jet_jit_log_info", jet_jit_log_info as *const u8);
    builder.symbol("jet_jit_log_warn", jet_jit_log_warn as *const u8);
    builder.symbol("jet_jit_log_error", jet_jit_log_error as *const u8);
    builder.symbol("jet_jit_math_sin", jet_jit_math_sin as *const u8);
    builder.symbol("jet_jit_math_cos", jet_jit_math_cos as *const u8);
    builder.symbol("jet_jit_math_exp", jet_jit_math_exp as *const u8);
    builder.symbol("jet_jit_math_atan2", jet_jit_math_atan2 as *const u8);
    builder.symbol("jet_jit_math_hypot", jet_jit_math_hypot as *const u8);
    builder.symbol("jet_jit_math_lerp", jet_jit_math_lerp as *const u8);
    builder.symbol("jet_jit_math_degrees", jet_jit_math_degrees as *const u8);
    builder.symbol("jet_jit_math_radians", jet_jit_math_radians as *const u8);
    builder.symbol("jet_jit_math_is_finite", jet_jit_math_is_finite as *const u8);
    builder.symbol("jet_jit_math_sign", jet_jit_math_sign as *const u8);
    builder.symbol("jet_jit_math_checked_add", jet_jit_math_checked_add as *const u8);
    builder.symbol(
        "jet_jit_math_saturating_add",
        jet_jit_math_saturating_add as *const u8,
    );
    builder.symbol(
        "jet_jit_math_wrapping_add",
        jet_jit_math_wrapping_add as *const u8,
    );
    builder.symbol("jet_jit_math_int_pow", jet_jit_math_int_pow as *const u8);
    builder.symbol("jet_jit_math_gcd", jet_jit_math_gcd as *const u8);
    builder.symbol("jet_jit_math_lcm", jet_jit_math_lcm as *const u8);
}

pub(crate) fn declare_core_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<CoreHostFns, String> {
    use cranelift_codegen::ir::{types, AbiParam, Signature};
    use cranelift_module::{Linkage, Module};

    let cc = module.target_config().default_call_conv;
    let mut sig_str = Signature::new(cc);
    sig_str.returns.push(AbiParam::new(types::I64));
    let mut sig_i64 = Signature::new(cc);
    sig_i64.returns.push(AbiParam::new(types::I64));
    let mut sig_void_str = Signature::new(cc);
    sig_void_str.params.push(AbiParam::new(types::I64));
    let mut sig_f64_f64 = Signature::new(cc);
    sig_f64_f64.params.push(AbiParam::new(types::F64));
    sig_f64_f64.returns.push(AbiParam::new(types::F64));
    let mut sig_f64_f64_f64 = Signature::new(cc);
    sig_f64_f64_f64.params.push(AbiParam::new(types::F64));
    sig_f64_f64_f64.params.push(AbiParam::new(types::F64));
    sig_f64_f64_f64.returns.push(AbiParam::new(types::F64));
    let mut sig_lerp = Signature::new(cc);
    sig_lerp.params.push(AbiParam::new(types::F64));
    sig_lerp.params.push(AbiParam::new(types::F64));
    sig_lerp.params.push(AbiParam::new(types::F64));
    sig_lerp.returns.push(AbiParam::new(types::F64));
    let mut sig_f64_i8 = Signature::new(cc);
    sig_f64_i8.params.push(AbiParam::new(types::F64));
    sig_f64_i8.returns.push(AbiParam::new(types::I8));
    let mut sig_f64_i64 = Signature::new(cc);
    sig_f64_i64.params.push(AbiParam::new(types::F64));
    sig_f64_i64.returns.push(AbiParam::new(types::I64));
    let mut sig_i64_i64_i64 = Signature::new(cc);
    sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
    sig_i64_i64_i64.params.push(AbiParam::new(types::I64));
    sig_i64_i64_i64.returns.push(AbiParam::new(types::I64));

    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

    Ok(CoreHostFns {
        os_name: import("jet_jit_os_name", &sig_str)?,
        os_family: import("jet_jit_os_family", &sig_str)?,
        os_arch: import("jet_jit_os_arch", &sig_str)?,
        os_cpu_count: import("jet_jit_os_cpu_count", &sig_i64)?,
        os_temp_dir: import("jet_jit_os_temp_dir", &sig_str)?,
        os_executable: import("jet_jit_os_executable", &sig_str)?,
        os_pid: import("jet_jit_os_pid", &sig_i64)?,
        os_hostname: import("jet_jit_os_hostname", &sig_str)?,
        log_set_level: import("jet_jit_log_set_level", &sig_void_str)?,
        log_setup: import("jet_jit_log_setup", &sig_void_str)?,
        log_debug: import("jet_jit_log_debug", &sig_void_str)?,
        log_info: import("jet_jit_log_info", &sig_void_str)?,
        log_warn: import("jet_jit_log_warn", &sig_void_str)?,
        log_error: import("jet_jit_log_error", &sig_void_str)?,
        math_sin: import("jet_jit_math_sin", &sig_f64_f64)?,
        math_cos: import("jet_jit_math_cos", &sig_f64_f64)?,
        math_exp: import("jet_jit_math_exp", &sig_f64_f64)?,
        math_atan2: import("jet_jit_math_atan2", &sig_f64_f64_f64)?,
        math_hypot: import("jet_jit_math_hypot", &sig_f64_f64_f64)?,
        math_lerp: import("jet_jit_math_lerp", &sig_lerp)?,
        math_degrees: import("jet_jit_math_degrees", &sig_f64_f64)?,
        math_radians: import("jet_jit_math_radians", &sig_f64_f64)?,
        math_is_finite: import("jet_jit_math_is_finite", &sig_f64_i8)?,
        math_sign: import("jet_jit_math_sign", &sig_f64_i64)?,
        math_checked_add: import("jet_jit_math_checked_add", &sig_i64_i64_i64)?,
        math_saturating_add: import("jet_jit_math_saturating_add", &sig_i64_i64_i64)?,
        math_wrapping_add: import("jet_jit_math_wrapping_add", &sig_i64_i64_i64)?,
        math_int_pow: import("jet_jit_math_int_pow", &sig_i64_i64_i64)?,
        math_gcd: import("jet_jit_math_gcd", &sig_i64_i64_i64)?,
        math_lcm: import("jet_jit_math_lcm", &sig_i64_i64_i64)?,
    })
}
