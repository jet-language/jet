//! Interpreter deopt host shim + whole-program fallback (D-ONECORE1=A / #778).
//!
//! Deopt tier calls the SAME TIR evaluator as #777 (`TirBridge` /
//! `install_comptime_bridge` / `run_named_func`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use cranelift_codegen::ir::{types, InstBuilder};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};
use jet_codegen::Codegen::TIR::{self, JitProgram, TFunc};
use jet_codegen::Comptime::{self, CtReport, CtValue, DevSink};
use jet_foundation::AST::{ProgramBundle, Type};
use jet_foundation::Diagnostics::Diagnostic;
use jet_foundation::JitBackend::RunOutcome;

use super::runtime_host::{HostFns, JitRuntime};
use super::tiers::{deopt_marshallable, record_trace, Tier, TierPlan, TierRow};
use super::types_meta::{func_has_receiver, func_signature, JitMeta};
use super::Concurrency;

thread_local! {
    /// Borrowed for the duration of one resident invoke / compile.
    static DEOPT_PROGRAM: RefCell<Option<*const JitProgram>> = const { RefCell::new(None) };
    static DEOPT_NAMES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static NATIVE_FNS: RefCell<HashMap<String, NativeFn>> = RefCell::new(HashMap::new());
}

struct NativeFn {
    code: *const u8,
    params: Vec<Type>,
    ret: Option<Type>,
}

// SAFETY: pointers live only for the resident module lifetime on this thread.
unsafe impl Send for NativeFn {}
unsafe impl Sync for NativeFn {}

pub(crate) fn clear_deopt_state() {
    DEOPT_PROGRAM.with(|s| *s.borrow_mut() = None);
    DEOPT_NAMES.with(|s| s.borrow_mut().clear());
    NATIVE_FNS.with(|s| s.borrow_mut().clear());
    TIR::set_native_call_hook(None);
}

pub(crate) fn install_deopt_program(program: &JitProgram, deopt_names: &[String]) {
    DEOPT_PROGRAM.with(|s| *s.borrow_mut() = Some(program as *const JitProgram));
    DEOPT_NAMES.with(|s| *s.borrow_mut() = deopt_names.to_vec());
}

pub(crate) fn register_native_fn(name: String, code: *const u8, tir: &TFunc) {
    NATIVE_FNS.with(|s| {
        s.borrow_mut().insert(
            name,
            NativeFn {
                code,
                params: tir.params.iter().map(|(_, ty, _)| ty.clone()).collect(),
                ret: tir.ret.clone(),
            },
        );
    });
}

pub(crate) fn install_native_hook() {
    TIR::set_native_call_hook(Some(native_call_hook));
}

fn native_call_hook(name: &str, args: &[CtValue]) -> Option<Result<CtValue, Diagnostic>> {
    let native =
        NATIVE_FNS.with(|s| s.borrow().get(name).map(|n| (n.code, n.params.clone(), n.ret.clone())))?;
    let (code, params, ret) = native;
    if args.len() != params.len() {
        return Some(Err(Diagnostic::error(
            "E0956",
            format!("native call `{name}` arity mismatch"),
            "cross-tier call argument count does not match the Cranelift signature".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )));
    }
    if args.len() > 8 {
        return Some(Err(Diagnostic::error(
            "E0956",
            format!("native call `{name}` has too many arguments"),
            "cross-tier host shim supports at most 8 parameters".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )));
    }
    let mut bits = [0i64; 8];
    let converted: Option<Result<(), Diagnostic>> = Concurrency::with_runtime_mut(|rt| {
        for (i, (arg, ty)) in args.iter().zip(params.iter()).enumerate() {
            match ct_to_bits(rt, ty, arg) {
                Ok(b) => bits[i] = b,
                Err(d) => return Some(Err(d)),
            }
        }
        Some(Ok(()))
    });
    match converted {
        Some(Err(d)) => return Some(Err(d)),
        None => {
            return Some(Err(Diagnostic::error(
                "E0956",
                "native call with no active JIT runtime".to_string(),
                "cross-tier native dispatch needs the resident runtime".to_string(),
                "report this as a compiler bug".to_string(),
                None,
            )));
        }
        Some(Ok(())) => {}
    }
    let result_bits = unsafe {
        match params.len() {
            0 => {
                let f: extern "C" fn() -> i64 = std::mem::transmute(code);
                f()
            }
            1 => {
                let f: extern "C" fn(i64) -> i64 = std::mem::transmute(code);
                f(bits[0])
            }
            2 => {
                let f: extern "C" fn(i64, i64) -> i64 = std::mem::transmute(code);
                f(bits[0], bits[1])
            }
            3 => {
                let f: extern "C" fn(i64, i64, i64) -> i64 = std::mem::transmute(code);
                f(bits[0], bits[1], bits[2])
            }
            4 => {
                let f: extern "C" fn(i64, i64, i64, i64) -> i64 = std::mem::transmute(code);
                f(bits[0], bits[1], bits[2], bits[3])
            }
            n => {
                let f: extern "C" fn(i64, i64, i64, i64, i64, i64, i64, i64) -> i64 =
                    std::mem::transmute(code);
                let mut wide = [0i64; 8];
                wide[..n].copy_from_slice(&bits[..n]);
                f(
                    wide[0], wide[1], wide[2], wide[3], wide[4], wide[5], wide[6], wide[7],
                )
            }
        }
    };
    Some(
        Concurrency::with_runtime_mut(|rt| match &ret {
            None => Some(Ok(CtValue::Unit)),
            Some(Type::Named(n)) if n == "Unit" => Some(Ok(CtValue::Unit)),
            Some(ty) => Some(bits_to_ct(rt, ty, result_bits)),
        })
        .unwrap_or_else(|| {
            Err(Diagnostic::error(
                "E0956",
                "native call with no active JIT runtime".to_string(),
                "cross-tier native dispatch needs the resident runtime".to_string(),
                "report this as a compiler bug".to_string(),
                None,
            ))
        }),
    )
}

/// Strip call-site "at comptime" framing baked into evaluator `what` strings
/// before surfacing them as a runtime-tier diagnostic (D-VERDICT-1254-1).
fn strip_comptime_framing(what: &str) -> String {
    let mut s = what
        .strip_suffix(" can't run at compile time yet")
        .unwrap_or(what)
        .to_string();
    if let Some(rest) = s.strip_suffix(" (impure tier)") {
        s = rest.to_string();
    }
    s = s.replace(" at comptime", "");
    s = s.replace(" at compile time", "");
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s.trim().to_string()
}

/// Rewrite shared-evaluator diagnostics that leaked comptime voice into the
/// default `jet run` / whole-program deopt path. Keeps the code; only what /
/// why / fix change. Comptime callers never hit this (D-VERDICT-1254-1).
///
/// D-META-EFFECT1 c3: E0951 retired into E3401 (the comptime purity gate and
/// the run-time `=[]=>` check share one code now). This is still safe to
/// rewrite unconditionally: a genuine run-time E3401 (a `=[]=>`-declared
/// function's own body, or the whole-program effect fixpoint) is a sema-time
/// diagnostic that fails the build before the deopt path ever runs, so any
/// E3401 seen here can only be the shared evaluator's own purity gate firing
/// during interpretation.
fn rewrite_runtime_tier_diag(d: Diagnostic) -> Diagnostic {
    let construct = match d.code.as_str() {
        "E0956" => strip_comptime_framing(&d.what),
        "E3401" => d
            .what
            .strip_suffix(" is not allowed in comptime code")
            .unwrap_or(&d.what)
            .to_string(),
        "E3412" => d
            .what
            .strip_suffix(" is not available at comptime")
            .unwrap_or(&d.what)
            .to_string(),
        "E3410" => d
            .what
            .split(" is a Tier-2")
            .next()
            .unwrap_or(&d.what)
            .to_string(),
        _ => return d,
    };
    Diagnostic::error(
        d.code.clone(),
        format!("{construct} isn't supported in Jet's quick-run mode yet"),
        "Jet's quick-run mode doesn't cover this yet — that's a gap in Jet, not a mistake in your program"
            .to_string(),
        "run with `jet run --release <file>` (full build), which supports everything".to_string(),
        d.span,
    )
}

#[cfg(test)]
mod rewrite_tests {
    use super::*;

    fn assert_quick_run_voice(d: &Diagnostic) {
        assert!(
            d.what.contains("quick-run"),
            "what must name quick-run, got: {:?}",
            d.what
        );
        assert!(
            d.why.contains("gap in Jet") && d.why.contains("not a mistake"),
            "why must blame Jet's gap, got: {:?}",
            d.why
        );
        assert!(
            d.fix.contains("jet run --release"),
            "fix must point at jet run --release, got: {:?}",
            d.fix
        );
        for (label, text) in [("what", d.what.as_str()), ("why", d.why.as_str()), ("fix", d.fix.as_str())]
        {
            let lower = text.to_ascii_lowercase();
            assert!(
                !lower.contains("comptime") && !lower.contains("compile time"),
                "runtime-tier {label} must not mention comptime/compile time, got: {text:?}"
            );
        }
    }

    #[test]
    fn rewrite_e3412_uses_quick_run_voice() {
        let d = Diagnostic::error(
            "E3412",
            "core.net.listen() is not available at comptime".to_string(),
            "only fetch is Tier-1".to_string(),
            "use fetch".to_string(),
            None,
        );
        let out = rewrite_runtime_tier_diag(d);
        assert_eq!(out.code, "E3412");
        assert!(
            out.what.contains("core.net.listen()"),
            "construct must survive rewrite, got: {:?}",
            out.what
        );
        assert_quick_run_voice(&out);
    }

    #[test]
    fn rewrite_e3401_comptime_leak_uses_quick_run_voice() {
        let d = Diagnostic::error(
            "E3401",
            "core.files.read is not allowed in comptime code".to_string(),
            "impure".to_string(),
            "gate it".to_string(),
            None,
        );
        let out = rewrite_runtime_tier_diag(d);
        assert_eq!(out.code, "E3401");
        assert!(
            out.what.contains("core.files.read"),
            "construct must survive rewrite, got: {:?}",
            out.what
        );
        assert_quick_run_voice(&out);
    }
}

/// Whole-program interpreter deopt — same evaluator as `--interpret` / comptime.
pub(crate) fn run_whole_interp(bundle: &ProgramBundle, plan: &TierPlan) -> RunOutcome {
    TIR::install_comptime_bridge();
    let started = Instant::now();
    let mut sink = DevSink::new();
    let outcome = jet_codegen::Comptime::with_ambient(
        Some(crate::ambient_interp::ambient_core_call),
        Some(crate::ambient_interp::ambient_handle),
        || match Comptime::TirBridge::run_bundle(bundle, &mut sink, true) {
            Ok(CtValue::Failed(CtReport::Told(error))) => {
                sink.stderr.push_str(&error.jet_show());
                sink.stderr.push('\n');
                RunOutcome::Ran {
                    stdout: sink.stdout,
                    stderr: sink.stderr,
                    exit_code: 1,
                }
            }
            Ok(_) => RunOutcome::Ran {
                stdout: sink.stdout,
                stderr: sink.stderr,
                exit_code: sink.exit_code.unwrap_or(0),
            },
            Err(d) if sink.exit_code.is_some() || d.code == "SOFT_EXIT" => RunOutcome::Ran {
                stdout: sink.stdout,
                stderr: sink.stderr,
                exit_code: sink
                    .exit_code
                    .unwrap_or_else(|| d.what.parse().unwrap_or(0)),
            },
            // Evaluator traps under whole-program deopt are live-program panics
            // (I9), not comptime build failures — match AOT exit 70 + wording.
            Err(d) if d.code == "E0953" => {
                let msg = d
                    .why
                    .strip_prefix(
                        "while computing this value at compile time, the program panicked: ",
                    )
                    .unwrap_or(d.why.as_str());
                if !sink.stderr.is_empty() && !sink.stderr.ends_with('\n') {
                    sink.stderr.push('\n');
                }
                sink.stderr.push_str("panic: ");
                sink.stderr.push_str(msg);
                sink.stderr.push('\n');
                RunOutcome::Ran {
                    stdout: sink.stdout,
                    stderr: sink.stderr,
                    exit_code: 70,
                }
            }
            Err(d) => RunOutcome::Problems(vec![rewrite_runtime_tier_diag(d)]),
        },
    );
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut rows = plan.rows.clone();
    for row in &mut rows {
        row.tier = Tier::Interp;
        if row.reason.is_empty() {
            row.reason = "whole-program deopt".into();
        }
        row.millis = ms;
    }
    if rows.is_empty() {
        rows.push(TierRow {
            function: plan
                .gap
                .as_ref()
                .map(|g| g.function.clone())
                .unwrap_or_else(|| "run".into()),
            tier: Tier::Interp,
            reason: plan
                .gap
                .as_ref()
                .map(|g| g.reason.clone())
                .unwrap_or_else(|| "whole-program deopt".into()),
            millis: ms,
        });
    }
    record_trace(rows);
    outcome
}

/// Emit a Cranelift trampoline that packs args and calls `jet_deopt_call`.
pub(crate) fn lower_deopt_stub(
    module: &mut JITModule,
    host: &HostFns,
    meta: &JitMeta<'_>,
    tir: &TFunc,
    func_id: FuncId,
    deopt_idx: i64,
) -> Result<(), String> {
    if !deopt_marshallable(tir) {
        return Err(format!("{}: deopt ABI not marshallable", tir.name));
    }
    if func_has_receiver(tir) {
        return Err(format!("{}: method deopt not supported", tir.name));
    }
    let mut ctx = module.make_context();
    ctx.func.signature = func_signature(module, tir, meta)?;
    let mut fbcx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let params = b.block_params(entry).to_vec();
        if params.len() > 8 {
            return Err(format!("{}: deopt stub supports at most 8 params", tir.name));
        }
        let mut args = Vec::with_capacity(10);
        args.push(b.ins().iconst(types::I64, deopt_idx));
        args.push(b.ins().iconst(types::I64, params.len() as i64));
        for i in 0..8 {
            if i < params.len() {
                let p = params[i];
                let wide = match b.func.dfg.value_type(p) {
                    types::I64 => p,
                    types::I8 | types::I32 => b.ins().uextend(types::I64, p),
                    other => {
                        return Err(format!("{}: unexpected param clif type {other}", tir.name))
                    }
                };
                args.push(wide);
            } else {
                args.push(b.ins().iconst(types::I64, 0));
            }
        }
        let host_ref = module.declare_func_in_func(host.deopt_call, b.func);
        let call = b.ins().call(host_ref, &args);
        if let Some(ret) = &tir.ret {
            if let Some(ct) = meta.clif_ty(ret) {
                let raw = b.inst_results(call)[0];
                let out = match ct {
                    types::I64 => raw,
                    types::I8 => b.ins().ireduce(types::I8, raw),
                    types::I32 => b.ins().ireduce(types::I32, raw),
                    _ => return Err(format!("{}: unexpected return clif type", tir.name)),
                };
                b.ins().return_(&[out]);
            } else {
                b.ins().return_(&[]);
            }
        } else {
            b.ins().return_(&[]);
        }
        b.finalize();
    }
    module
        .define_function(func_id, &mut ctx)
        .map_err(|e| e.to_string())?;
    module.clear_context(&mut ctx);
    Ok(())
}

/// Host: interpret one deopted function with packed i64 args.
pub(crate) extern "C" fn jet_deopt_call(
    fn_idx: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
    a7: i64,
) -> i64 {
    let packed = [a0, a1, a2, a3, a4, a5, a6, a7];
    let argc = argc.clamp(0, 8) as usize;
    TIR::install_comptime_bridge();
    let name = DEOPT_NAMES.with(|s| s.borrow().get(fn_idx as usize).cloned());
    let Some(name) = name else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("deopt call: unknown function index");
        });
        return 0;
    };
    let program_ptr = DEOPT_PROGRAM.with(|s| *s.borrow());
    let Some(program_ptr) = program_ptr else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("deopt call: no program");
        });
        return 0;
    };
    // SAFETY: install_deopt_program keeps this pointer valid for the invoke.
    let program = unsafe { &*program_ptr };
    let Some(func) = program.funcs.iter().find(|f| f.name == name) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(&format!("deopt call: missing `{name}`"));
        });
        return 0;
    };
    let func_name = func.name.clone();
    let param_tys: Vec<Type> = func.params.iter().map(|(_, ty, _)| ty.clone()).collect();
    let ret_ty = func.ret.clone();

    let result: Option<Result<i64, String>> = Concurrency::with_runtime_mut(|rt| {
        let mut args = Vec::with_capacity(argc);
        for i in 0..argc {
            let ty = match param_tys.get(i) {
                Some(ty) => ty,
                None => return Some(Err(format!("deopt `{func_name}` missing param {i}"))),
            };
            match bits_to_ct(rt, ty, packed[i]) {
                Ok(v) => args.push(v),
                Err(d) => return Some(Err(d.what)),
            }
        }
        let mut sink = DevSink::new();
        let value = match jet_codegen::Comptime::with_ambient(
            Some(crate::ambient_interp::ambient_core_call),
            Some(crate::ambient_interp::ambient_handle),
            || TIR::run_named_func(program, &func_name, args, &mut sink),
        ) {
            Ok(v) => v,
            Err(d) => return Some(Err(d.what)),
        };
        rt.stdout.push_str(&sink.stdout);
        rt.stderr.push_str(&sink.stderr);
        match &ret_ty {
            None => Some(Ok(0)),
            Some(Type::Named(n)) if n == "Unit" => Some(Ok(0)),
            Some(ty) => Some(ct_to_bits(rt, ty, &value).map_err(|d| d.what)),
        }
    });
    match result {
        Some(Ok(bits)) => bits,
        Some(Err(msg)) => {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(&msg);
            });
            0
        }
        None => {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_trap("deopt call: no active runtime");
            });
            0
        }
    }
}

fn bits_to_ct(rt: &JitRuntime, ty: &Type, bits: i64) -> Result<CtValue, Diagnostic> {
    match ty {
        Type::Int | Type::IntN { .. } => Ok(CtValue::Int(bits)),
        Type::Bool => Ok(CtValue::Bool(bits != 0)),
        Type::Char => Ok(CtValue::Char(char::from_u32(bits as u32).unwrap_or('\0'))),
        Type::String => Ok(CtValue::Str(
            rt.heap.clone_string(bits).unwrap_or_default(),
        )),
        Type::Named(n) if n == "Int" => Ok(CtValue::Int(bits)),
        Type::Named(n) if n == "Bool" => Ok(CtValue::Bool(bits != 0)),
        Type::Named(n) if n == "Char" => {
            Ok(CtValue::Char(char::from_u32(bits as u32).unwrap_or('\0')))
        }
        Type::Named(n) if n == "String" => Ok(CtValue::Str(
            rt.heap.clone_string(bits).unwrap_or_default(),
        )),
        Type::Named(n) if n == "Unit" => Ok(CtValue::Unit),
        _ => Err(Diagnostic::error(
            "E0956",
            format!("deopt cannot marshall type `{ty:?}`"),
            "cross-tier host shim only moves Int/Bool/Char/String/Unit".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )),
    }
}

fn ct_to_bits(rt: &mut JitRuntime, ty: &Type, value: &CtValue) -> Result<i64, Diagnostic> {
    match value {
        CtValue::Int(n) => Ok(*n),
        CtValue::Bool(b) => Ok(i64::from(*b)),
        CtValue::Char(c) => Ok(u32::from(*c) as i64),
        CtValue::Str(s) => Ok(rt.heap.alloc_string(s.clone())),
        CtValue::Unit => Ok(0),
        _ => Err(Diagnostic::error(
            "E0956",
            format!("deopt cannot marshall value for `{ty:?}`"),
            "cross-tier host shim only moves Int/Bool/Char/String/Unit".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )),
    }
}
