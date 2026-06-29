//! c139 (D-JITDEP1 / D-JIT2=A) — Cranelift JIT tier-1 backend.
//!
//! Architecture: CraneliftBackend<F: JitBackend> where F is the tier-0
//! fallback. M0 delegates everything to F; M1 adds jit_covers() and
//! lower_tir_clif() to actually compile + run the covered subset natively.
//!
//! I6: Cranelift crates live here, not in the compiler `jet` crate (`Source/`).
//! The root package depends on jet-jit; jet-jit depends on cranelift-*.
//! D-JITDEP1 approved this as a scoped runtime-side exception.

use jet_foundation::{
    AST::ProgramBundle,
    Diagnostics::Diagnostic,
    JitBackend::{JitBackend, RunOutcome},
};

use cranelift_codegen::ir::{types, AbiParam, InstBuilder, Signature};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_codegen::Codegen::TIR::{self, TExpr, TExprKind, TFunc, TFuncKind, TStmt, TStrPart};
use std::cell::RefCell;

thread_local! {
    static JIT_RUNTIME: RefCell<Option<JitRuntime>> = const { RefCell::new(None) };
}

struct JitRuntime {
    stdout: String,
    stderr: String,
    strings: Vec<String>,
}

fn with_runtime_mut<F: FnOnce(&mut JitRuntime)>(f: F) {
    JIT_RUNTIME.with(|slot| {
        if let Some(rt) = slot.borrow_mut().as_mut() {
            f(rt);
        }
    });
}

fn render_float(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        v.to_string()
    }
}

extern "C" fn jet_jit_print_i64(v: i64) {
    with_runtime_mut(|rt| {
        rt.stdout.push_str(&v.to_string());
        rt.stdout.push('\n');
    });
}

extern "C" fn jet_jit_print_f64(v: f64) {
    with_runtime_mut(|rt| {
        rt.stdout.push_str(&render_float(v));
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
        if let Some(s) = rt.strings.get(id as usize) {
            rt.stdout.push_str(s);
            rt.stdout.push('\n');
        }
    });
}

struct HostFns {
    print_i64: FuncId,
    print_f64: FuncId,
    print_bool: FuncId,
    print_char: FuncId,
    print_str: FuncId,
}

fn declare_host_fns(module: &mut JITModule) -> Result<HostFns, String> {
    let cc = module.target_config().default_call_conv;
    let mut sig_i64 = Signature::new(cc);
    sig_i64.params.push(AbiParam::new(types::I64));
    let mut sig_f64 = Signature::new(cc);
    sig_f64.params.push(AbiParam::new(types::F64));
    let mut sig_i8 = Signature::new(cc);
    sig_i8.params.push(AbiParam::new(types::I8));
    let mut sig_i32 = Signature::new(cc);
    sig_i32.params.push(AbiParam::new(types::I32));

    let print_i64 = module
        .declare_function("jet_jit_print_i64", Linkage::Import, &sig_i64)
        .map_err(|e| e.to_string())?;
    let print_f64 = module
        .declare_function("jet_jit_print_f64", Linkage::Import, &sig_f64)
        .map_err(|e| e.to_string())?;
    let print_bool = module
        .declare_function("jet_jit_print_bool", Linkage::Import, &sig_i8)
        .map_err(|e| e.to_string())?;
    let print_char = module
        .declare_function("jet_jit_print_char", Linkage::Import, &sig_i32)
        .map_err(|e| e.to_string())?;
    let print_str = module
        .declare_function("jet_jit_print_str", Linkage::Import, &sig_i64)
        .map_err(|e| e.to_string())?;
    Ok(HostFns {
        print_i64,
        print_f64,
        print_bool,
        print_char,
        print_str,
    })
}

fn flatten_string(parts: &[TStrPart]) -> Option<String> {
    let mut out = String::new();
    for p in parts {
        match p {
            TStrPart::Lit(s) => out.push_str(s),
            TStrPart::Interp(_) => return None,
        }
    }
    Some(out)
}

fn jit_covers(tir: &TFunc) -> bool {
    if !matches!(tir.kind, TFuncKind::TopLevel) || !tir.is_main || !tir.params.is_empty() || tir.ret.is_some() {
        return false;
    }
    tir.body.iter().all(|s| match s {
        TStmt::ExprStmt(TExpr {
            kind: TExprKind::Print(inner),
            ..
        }) => match &inner.kind {
            TExprKind::IntLit(_, _)
            | TExprKind::FloatLit(_)
            | TExprKind::BoolLit(_)
            | TExprKind::CharLit(_) => true,
            TExprKind::StrLit(parts) => flatten_string(parts).is_some(),
            _ => false,
        },
        TStmt::Return(None) => true,
        _ => false,
    })
}

fn lower_tir_clif(module: &mut JITModule, tir: &TFunc) -> Result<(FuncId, Vec<String>), String> {
    let mut strings: Vec<String> = Vec::new();
    let host = declare_host_fns(module)?;
    let mut ctx = module.make_context();
    ctx.func.signature = module.make_signature();
    let mut fbcx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
        let entry = b.create_block();
        b.switch_to_block(entry);
        b.seal_block(entry);

        for stmt in &tir.body {
            match stmt {
                TStmt::ExprStmt(TExpr {
                    kind: TExprKind::Print(inner),
                    ..
                }) => {
                    let (host_id, arg) = match &inner.kind {
                        TExprKind::IntLit(v, _) => (host.print_i64, b.ins().iconst(types::I64, *v)),
                        TExprKind::FloatLit(v) => (host.print_f64, b.ins().f64const(*v)),
                        TExprKind::BoolLit(v) => (
                            host.print_bool,
                            b.ins().iconst(types::I8, if *v { 1 } else { 0 }),
                        ),
                        TExprKind::CharLit(v) => (host.print_char, b.ins().iconst(types::I32, *v as i64)),
                        TExprKind::StrLit(parts) => {
                            let text = flatten_string(parts)
                                .ok_or_else(|| "jit string interpolation unsupported".to_string())?;
                            let id = strings.len() as i64;
                            strings.push(text);
                            (host.print_str, b.ins().iconst(types::I64, id))
                        }
                        _ => return Err("jit print expression unsupported".to_string()),
                    };
                    let host_ref = module.declare_func_in_func(host_id, b.func);
                    b.ins().call(host_ref, &[arg]);
                }
                TStmt::Return(None) => {
                    b.ins().return_(&[]);
                }
                _ => return Err("jit statement unsupported".to_string()),
            }
        }
        b.ins().return_(&[]);
        b.finalize();
    }
    let id = module
        .declare_function("jet_jit_main", Linkage::Export, &ctx.func.signature)
        .map_err(|e| e.to_string())?;
    module.define_function(id, &mut ctx).map_err(|e| e.to_string())?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().map_err(|e| e.to_string())?;
    Ok((id, strings))
}

fn run_jit(tir: &TFunc) -> Result<RunOutcome, String> {
    let mut builder =
        JITBuilder::new(cranelift_module::default_libcall_names()).map_err(|e| e.to_string())?;
    builder.symbol("jet_jit_print_i64", jet_jit_print_i64 as *const u8);
    builder.symbol("jet_jit_print_f64", jet_jit_print_f64 as *const u8);
    builder.symbol("jet_jit_print_bool", jet_jit_print_bool as *const u8);
    builder.symbol("jet_jit_print_char", jet_jit_print_char as *const u8);
    builder.symbol("jet_jit_print_str", jet_jit_print_str as *const u8);
    let mut module = JITModule::new(builder);
    let (id, strings) = lower_tir_clif(&mut module, tir)?;
    let code = module.get_finalized_function(id);
    let entry: extern "C" fn() = unsafe { std::mem::transmute(code) };
    JIT_RUNTIME.with(|slot| {
        *slot.borrow_mut() = Some(JitRuntime {
            stdout: String::new(),
            stderr: String::new(),
            strings,
        })
    });
    entry();
    let rt = JIT_RUNTIME.with(|slot| slot.borrow_mut().take());
    match rt {
        Some(out) => Ok(RunOutcome::Ran {
            stdout: out.stdout,
            stderr: out.stderr,
        }),
        None => Err("jit runtime capture missing".to_string()),
    }
}

/// c139 tier-1 JIT backend over the `JitBackend` seam.
///
/// `F` is the tier-0 fallback (always `InterpreterBackend` in practice).
/// M0: every method delegates to `fallback`.
/// M1: `run` and `hot_swap` will JIT-compile functions inside `jit_covers()`
///     and delegate only the uncovered remainder to `fallback`.
pub struct CraneliftBackend<F: JitBackend> {
    fallback: F,
}

impl<F: JitBackend> CraneliftBackend<F> {
    /// Construct a CraneliftBackend wrapping `fallback` for tier-0 coverage.
    pub fn new(fallback: F) -> Self {
        CraneliftBackend { fallback }
    }
}

impl<F: JitBackend> JitBackend for CraneliftBackend<F> {
    fn run(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        let Some(tir) = TIR::lower_entry_main_for_jit(bundle) else {
            return self.fallback.run(bundle, try_anyway);
        };
        if !jit_covers(&tir) {
            return self.fallback.run(bundle, try_anyway);
        }
        match run_jit(&tir) {
            Ok(out) => out,
            Err(_) => self.fallback.run(bundle, try_anyway),
        }
    }

    fn hot_swap(
        &mut self,
        module_name: &str,
        bundle: &ProgramBundle,
        try_anyway: bool,
    ) -> Result<RunOutcome, Vec<Diagnostic>> {
        // M0: delegate; M2 will re-link the module in the resident process.
        self.fallback.hot_swap(module_name, bundle, try_anyway)
    }

    fn restart(&mut self, bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
        // M0: delegate; M2 will tear down and rebuild the resident JIT process.
        self.fallback.restart(bundle, try_anyway)
    }
}
