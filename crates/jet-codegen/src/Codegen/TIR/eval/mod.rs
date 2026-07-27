//! Canonical TIR evaluator — reference semantics (D-ONECORE1=A / #777).

mod builtins;
mod browser;
mod closure_ops;
mod data_calls;
mod exprs;
mod handles;
mod stmts;

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::AST::{Expr, ProgramBundle, Stmt};
use super::Cx;
use crate::Codegen::TIR::{self, JitProgram, LowerEnv, TExpr, TFunc, TLocal, TStmt};
use super::build_cx_items;
use crate::Comptime::{self, CtValue, DevSink};
use crate::Diagnostics::{Diagnostic, Span};

/// Cross-tier hook: Cranelift-native functions callable from the TIR evaluator (#778).
pub type NativeCallHook = fn(&str, &[CtValue]) -> Option<Result<CtValue, Diagnostic>>;

thread_local! {
    static NATIVE_CALL_HOOK: Cell<Option<NativeCallHook>> = const { Cell::new(None) };
}

pub fn set_native_call_hook(hook: Option<NativeCallHook>) {
    NATIVE_CALL_HOOK.with(|slot| slot.set(hook));
}

pub(super) fn native_call_hook() -> Option<NativeCallHook> {
    NATIVE_CALL_HOOK.with(Cell::get)
}

pub(super) fn unsupported(what: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        format!("{what} can't run at compile time yet"),
        "the canonical TIR evaluator doesn't cover this construct yet".to_string(),
        "use a simpler form, or run via `jet build` / `jet run`".to_string(),
        Some(span),
    )
}

/// Resolve a `__JetViewMut { base, start, end }` handle to the inclusive window List.
pub(super) fn materialize_view_mut_window(
    fields: &[(String, CtValue)],
    scope: &HashMap<String, CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut base = None;
    let mut start = None;
    let mut end = None;
    for (name, value) in fields {
        match (name.as_str(), value) {
            ("base", CtValue::Str(s)) => base = Some(s.clone()),
            ("start", CtValue::Int(n)) => start = Some(*n),
            ("end", CtValue::Int(n)) => end = Some(*n),
            _ => {}
        }
    }
    let (base, start, end) = match (base, start, end) {
        (Some(b), Some(s), Some(e)) => (b, s, e),
        _ => return Err(unsupported("view-mut fields", span)),
    };
    let Some(CtValue::List(items)) = scope.get(&base) else {
        return Err(unsupported("view-mut owner", span));
    };
    if start < 0 || end < start || end as usize >= items.len() {
        return Err(unsupported("view-mut bounds", span));
    }
    Ok(CtValue::List(items[start as usize..=end as usize].to_vec()))
}

#[derive(Debug)]
pub(super) enum Flow {
    Normal,
    Break,
    Continue,
    BreakLabel(String),
    BreakValue(Option<String>, CtValue),
    ContinueLabel(String),
    Return(CtValue),
}

const DEV_FUEL: u64 = 1_000_000_000;
#[allow(dead_code)]
const CT_FUEL: u64 = 10_000_000;

pub(super) struct EvalCtx<'a> {
    pub(super) funcs: HashMap<String, &'a TFunc>,
    #[allow(dead_code)]
    pub(super) base_dir: PathBuf,
    pub(super) fuel: u64,
    pub(super) sink: Option<&'a mut DevSink>,
    #[allow(dead_code)]
    pub(super) core_imports: &'a HashMap<String, String>,
    pub(super) globals: HashMap<String, CtValue>,
    #[allow(dead_code)]
    pub(super) allow_impure: bool,
    #[allow(dead_code)]
    pub(super) impure_depth: usize,
    /// True only for an actual dev/JIT runtime execution. Comptime may permit
    /// explicit I/O, but it must never open a Browser session while compiling.
    pub(super) runtime_execution: bool,
    pub(super) repl_mode: bool,
    pub(super) pending_return: Option<CtValue>,
    /// Control emitted by an inline loop expression that targets an enclosing
    /// loop. The containing statement list consumes and propagates it.
    pub(super) pending_flow: Option<Flow>,
    /// Compiler-private eager List sinks for raw comptime yielding-loop
    /// fragments. Fully checked programs rewrite these sends to `List.push`.
    pub(super) collecting_items: Vec<Vec<CtValue>>,
    pub(super) call_depth: usize,
    pub(super) emitted_fragments: Option<&'a mut Vec<String>>,
    pub(super) embed_inputs: Option<&'a mut Vec<crate::AST::ComptimeInput>>,
    /// `TypeName -> [(field, redact)]` for JetDebug formatting (D-DISPLAYDBG).
    pub(super) struct_fields: HashMap<String, Vec<(String, bool)>>,
    /// `TypeName -> [(field, Type)]` for `core.data.csv` / decode on deopt.
    pub(super) struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
}

impl EvalCtx<'_> {
    pub(crate) fn span(&self) -> Span {
        Span::new(0, 0)
    }

    pub(super) fn burn(&mut self) -> Result<(), Diagnostic> {
        if self.fuel == 0 {
            // Dev/REPL fragments use E2202; pure comptime uses E0952.
            let (code, what, why, fix) = if self.sink.is_some() || self.repl_mode {
                (
                    "E2202",
                    "this program ran too many steps without finishing".to_string(),
                    format!(
                        "`jet dev` interprets your program to give instant feedback, but it ran out of steps without finishing — this usually means a loop that never ends"
                    ),
                    "check the loop near here for a condition that never becomes false; run `jet run` to execute the real build with no step limit"
                        .to_string(),
                )
            } else {
                (
                    "E0952",
                    "compile-time evaluation ran out of steps".to_string(),
                    "a loop or recursion in this expression doesn't finish".to_string(),
                    "simplify the expression, or move the work to runtime".to_string(),
                )
            };
            return Err(Diagnostic::error(code, what, why, fix, Some(self.span())));
        }
        self.fuel -= 1;
        Ok(())
    }

    pub(crate) fn run_func(
        &mut self,
        func: &TFunc,
        args: Vec<CtValue>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if self.call_depth > 64 {
            self.fuel = 0;
            self.burn()?;
            unreachable!("burn with fuel 0 always errors");
        }
        self.call_depth += 1;
        for (i, (name, _, _)) in func.params.iter().enumerate() {
            let jet = name.strip_prefix("user_").unwrap_or(name.as_str());
            scope.insert(
                jet.to_string(),
                args.get(i).cloned().unwrap_or(CtValue::Unit),
            );
        }
        let result = match self.exec_stmts(&func.body, scope)? {
            Flow::Return(v) => Ok(v),
            Flow::Normal => Ok(CtValue::Unit),
            other => Err(unsupported(
                &format!("control flow {other:?} escaping function"),
                self.span(),
            )),
        };
        self.call_depth -= 1;
        result
    }
}

fn empty_cx() -> Cx {
    build_cx_items(&[], "", "<eval>", None, &HashMap::new())
}

fn seed_fragment_distinct_types(
    cx: &mut Cx,
    ranges: &HashMap<String, Option<(i64, i64)>>,
    bases: &HashMap<String, crate::AST::Type>,
) {
    for (name, base) in bases {
        cx.distinct_types
            .insert(name.clone(), (base.clone(), base.is_numeric()));
    }
    for (name, range) in ranges {
        if let Some(bounds) = range {
            cx.distinct_ranges.insert(name.clone(), *bounds);
        }
    }
}

/// Lower one expression for the evaluator (comptime / REPL fragments).
pub fn lower_expr_for_eval(
    expr: &Expr,
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    distinct_ranges: &HashMap<String, Option<(i64, i64)>>,
    distinct_bases: &HashMap<String, crate::AST::Type>,
) -> Result<TExpr, Diagnostic> {
    let mut cx = empty_cx();
    seed_fragment_distinct_types(&mut cx, distinct_ranges, distinct_bases);
    cx.const_values = globals.clone();
    for (name, value) in globals {
        cx.consts.insert(name.clone(), String::new());
        let _ = value;
    }
    cx.core_imports = core_imports.clone();
    let mut env = LowerEnv::new("__ct".into());
    for name in globals.keys() {
        env.bind(name, TLocal::user(name), None);
    }
    // Fragment eval: sema facts may still be incomplete (e.g. IndexKind::Unknown).
    // Try lower under catch_unwind; refuse with E0956 only when lower can't.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::Codegen::TIR::with_eval_fragment(|| TIR::lower_expr(expr, &cx, &mut env))
    })) {
        Ok(tir) => Ok(tir),
        Err(_) => Err(unsupported("this expression", Span::new(0, 0))),
    }
}

/// Lower a statement list for the evaluator.
pub fn lower_stmts_for_eval(
    stmts: &[Stmt],
    globals: &HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    distinct_ranges: &HashMap<String, Option<(i64, i64)>>,
    distinct_bases: &HashMap<String, crate::AST::Type>,
) -> Result<Vec<TStmt>, Diagnostic> {
    let mut cx = empty_cx();
    seed_fragment_distinct_types(&mut cx, distinct_ranges, distinct_bases);
    cx.const_values = globals.clone();
    for name in globals.keys() {
        cx.consts.insert(name.clone(), String::new());
    }
    cx.core_imports = core_imports.clone();
    let mut env = LowerEnv::new("__ct_block".into());
    for name in globals.keys() {
        env.bind(name, TLocal::user(name), None);
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::Codegen::TIR::with_eval_fragment(|| TIR::lower_stmts(stmts, &cx, &mut env))
    })) {
        Ok(tir) => Ok(tir),
        Err(_) => Err(unsupported("this statement", Span::new(0, 0))),
    }
}

pub fn lower_interp_program(bundle: &ProgramBundle) -> Option<JitProgram> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| TIR::lower_jit_program(bundle)))
    {
        Ok(program) => program,
        Err(_) => None,
    }
}

fn program_funcs(program: &JitProgram) -> HashMap<String, &TFunc> {
    program.funcs.iter().map(|f| (f.name.clone(), f)).collect()
}

fn collect_struct_fields(bundle: &ProgramBundle) -> HashMap<String, Vec<(String, bool)>> {
    let mut out = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Struct(s) = item {
                out.insert(
                    s.name.clone(),
                    s.fields
                        .iter()
                        .map(|f| (f.name.clone(), f.redact))
                        .collect(),
                );
            }
        }
    }
    out
}

fn collect_struct_field_types(
    bundle: &ProgramBundle,
) -> HashMap<String, Vec<(String, crate::AST::Type)>> {
    let mut out = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Struct(s) = item {
                out.insert(
                    s.name.clone(),
                    s.fields
                        .iter()
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                );
            }
        }
    }
    out
}

pub fn run_program(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
) -> Result<CtValue, Diagnostic> {
    run_program_with_structs(
        program,
        base_dir,
        sink,
        globals,
        core_imports,
        allow_impure,
        HashMap::new(),
        HashMap::new(),
    )
}

pub fn run_program_with_structs(
    program: &JitProgram,
    base_dir: &Path,
    sink: &mut DevSink,
    globals: HashMap<String, CtValue>,
    core_imports: &HashMap<String, String>,
    allow_impure: bool,
    struct_fields: HashMap<String, Vec<(String, bool)>>,
    struct_field_types: HashMap<String, Vec<(String, crate::AST::Type)>>,
) -> Result<CtValue, Diagnostic> {
    let _browser_session = browser::SessionGuard::new();
    let funcs = program_funcs(program);
    let entry = funcs.get(&program.entry).copied().ok_or_else(|| {
        Diagnostic::error(
            "E2201",
            format!("entry `{}` missing from lowered TIR", program.entry),
            "the interpreter needs the selected entry function in the TIR program".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )
    })?;
    let mut ctx = EvalCtx {
        funcs,
        base_dir: base_dir.to_path_buf(),
        fuel: DEV_FUEL,
        sink: Some(sink),
        core_imports,
        globals,
        allow_impure,
        // Runtime `run_bundle` / deopt: ambient Tier-2 I/O matches AOT `jet run`.
        // Comptime purity still uses eval_expr/eval_block with explicit depths.
        impure_depth: if allow_impure { 1 } else { 0 },
        runtime_execution: true,
        repl_mode: false,
        pending_return: None,
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        emitted_fragments: None,
        embed_inputs: None,
        struct_fields,
        struct_field_types,
    };
    let mut scope = HashMap::new();
    ctx.run_func(entry, Vec::new(), &mut scope)
}

/// Run one named function through the canonical TIR evaluator (#778 deopt).
pub fn run_named_func(
    program: &JitProgram,
    name: &str,
    args: Vec<CtValue>,
    sink: &mut DevSink,
) -> Result<CtValue, Diagnostic> {
    let _browser_session = browser::SessionGuard::new();
    let funcs = program_funcs(program);
    let func = funcs.get(name).copied().ok_or_else(|| {
        Diagnostic::error(
            "E2201",
            format!("function `{name}` missing from lowered TIR"),
            "the deopt tier needs the named function in the TIR program".to_string(),
            "report this as a compiler bug".to_string(),
            None,
        )
    })?;
    let core_imports = HashMap::new();
    let mut ctx = EvalCtx {
        funcs,
        base_dir: PathBuf::from("."),
        fuel: DEV_FUEL,
        sink: Some(sink),
        core_imports: &core_imports,
        globals: HashMap::new(),
        allow_impure: true,
        // Runtime deopt is not comptime: open Tier-2 ambient I/O so `jet run`
        // matches AOT for env/fs/process (D-LENS-RUN2 / #778).
        impure_depth: 1,
        runtime_execution: true,
        repl_mode: false,
        pending_return: None,
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        emitted_fragments: None,
        embed_inputs: None,
        struct_fields: HashMap::new(),
        struct_field_types: HashMap::new(),
    };
    let mut scope = HashMap::new();
    ctx.run_func(func, args, &mut scope)
}

static INSTALLED: OnceLock<()> = OnceLock::new();

pub fn install_comptime_bridge() {
    INSTALLED.get_or_init(|| {
        Comptime::TirBridge::install(Comptime::TirBridge::Hooks {
            run_bundle,
            eval_expr: eval_expr_hook,
            eval_block: eval_block_hook,
        });
    });
}

fn run_bundle(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    allow_impure: bool,
) -> Result<CtValue, Diagnostic> {
    let program = lower_interp_program(bundle).ok_or_else(|| {
        Diagnostic::error(
            "E2201",
            "`jet dev` needs a `run` function to run".to_string(),
            "`jet dev` runs a program; a library with no `run` has nothing to execute".to_string(),
            "add `fn run() { … }`, or use `jet check <file>`".to_string(),
            None,
        )
    })?;
    let mut globals = HashMap::new();
    let mut core_imports = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let crate::AST::Item::Const(c) = item {
                if let Some(v) = &c.ct {
                    globals.entry(c.name.clone()).or_insert_with(|| v.clone());
                }
            }
        }
        for imp in &module.imports {
            if let Some(core_module) = imp.core_module_path() {
                core_imports
                    .entry(imp.import_alias())
                    .or_insert(core_module);
            }
        }
    }
    run_program_with_structs(
        &program,
        &bundle.project_root,
        sink,
        globals,
        &core_imports,
        allow_impure,
        collect_struct_fields(bundle),
        collect_struct_field_types(bundle),
    )
}

fn eval_expr_hook(
    req: &mut Comptime::TirBridge::ExprEvalRequest<'_>,
) -> Result<CtValue, Diagnostic> {
    let tir = lower_expr_for_eval(
        req.expr,
        req.globals,
        req.core_imports,
        req.distinct_ranges,
        req.distinct_bases,
    )?;
    let mut cx = empty_cx();
    seed_fragment_distinct_types(&mut cx, req.distinct_ranges, req.distinct_bases);
    cx.core_imports = req.core_imports.clone();
    let lowered: Vec<TFunc> = req
        .funcs
        .values()
        .filter_map(|f| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::Codegen::TIR::with_eval_fragment(|| TIR::lower_func(f, &cx))
            }))
            .ok()
        })
        .collect();
    let funcs: HashMap<String, &TFunc> = lowered.iter().map(|f| (f.name.clone(), f)).collect();
    let base_dir = req.base_dir.to_path_buf();
    let fuel = req.fuel;
    let core_imports = req.core_imports;
    let globals = req.globals.clone();
    let allow_impure = req.allow_impure;
    let impure_depth = req.initial_impure_depth;
    let repl_mode = req.repl_mode;
    let sink = req.sink.take();
    let emitted_fragments = req.emitted_fragments.take();
    let embed_inputs = req.embed_inputs.take();
    let mut ctx = EvalCtx {
        funcs,
        base_dir,
        fuel,
        sink,
        core_imports,
        globals: globals.clone(),
        allow_impure,
        impure_depth,
        runtime_execution: false,
        repl_mode,
        pending_return: None,
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        emitted_fragments,
        embed_inputs,
        struct_fields: HashMap::new(),
        struct_field_types: HashMap::new(),
    };
    let mut scope = globals;
    ctx.eval_expr(&tir, &mut scope)
}

fn eval_block_hook(
    req: &mut Comptime::TirBridge::BlockEvalRequest<'_>,
) -> Result<Comptime::TirBridge::StmtOutcome, Diagnostic> {
    let tir = lower_stmts_for_eval(
        req.stmts,
        req.globals,
        req.core_imports,
        req.distinct_ranges,
        req.distinct_bases,
    )?;
    let mut cx = empty_cx();
    seed_fragment_distinct_types(&mut cx, req.distinct_ranges, req.distinct_bases);
    cx.core_imports = req.core_imports.clone();
    let lowered: Vec<TFunc> = req
        .funcs
        .values()
        .filter_map(|f| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::Codegen::TIR::with_eval_fragment(|| TIR::lower_func(f, &cx))
            }))
            .ok()
        })
        .collect();
    let funcs: HashMap<String, &TFunc> = lowered.iter().map(|f| (f.name.clone(), f)).collect();
    let base_dir = req.base_dir.to_path_buf();
    let fuel = req.fuel;
    let core_imports = req.core_imports;
    let globals = req.globals.clone();
    let allow_impure = req.allow_impure;
    let impure_depth = req.impure_depth;
    let repl_mode = req.repl_mode;
    let sink = req.sink.take();
    let emitted_fragments = req.emitted_fragments.take();
    let embed_inputs = req.embed_inputs.take();
    let mut ctx = EvalCtx {
        funcs,
        base_dir,
        fuel,
        sink,
        core_imports,
        globals: globals.clone(),
        allow_impure,
        impure_depth,
        runtime_execution: false,
        repl_mode,
        pending_return: None,
        pending_flow: None,
        collecting_items: Vec::new(),
        call_depth: 0,
        emitted_fragments,
        embed_inputs,
        struct_fields: HashMap::new(),
        struct_field_types: HashMap::new(),
    };
    let mut scope = globals;
    match ctx.exec_stmts(&tir, &mut scope)? {
        Flow::Normal => Ok(Comptime::TirBridge::StmtOutcome::Done(scope)),
        Flow::Return(value) => Ok(Comptime::TirBridge::StmtOutcome::Returned { value, scope }),
        other => Err(unsupported(
            &format!("control flow {other:?} in statement fragment"),
            ctx.span(),
        )),
    }
}
