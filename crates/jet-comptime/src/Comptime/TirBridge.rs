//! Dependency-inversion seam: jet-codegen installs the TIR evaluator so
//! comptime/REPL/dev entry points share one engine without a crate cycle.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use crate::AST::{ComptimeInput, Expr, Func, ProgramBundle, Stmt, StructDef, Type};
use crate::Diagnostics::Diagnostic;
use crate::Comptime::CtValue;
use crate::Comptime::DevSink;

pub struct ExprEvalRequest<'a> {
    pub expr: &'a Expr,
    pub funcs: &'a HashMap<String, &'a Func>,
    /// Instance/associated methods are kept in their semantic owner/name table
    /// by comptime. The TIR fragment host needs the same table to lower
    /// computed-field getters and to call user methods.
    pub methods: &'a HashMap<(String, String), &'a Func>,
    pub extern_names: &'a HashSet<String>,
    pub base_dir: &'a Path,
    pub globals: &'a HashMap<String, CtValue>,
    pub core_imports: &'a HashMap<String, String>,
    pub allow_impure: bool,
    pub initial_impure_depth: usize,
    pub structs: &'a HashMap<String, &'a StructDef>,
    pub computed_fields: &'a HashMap<(String, String), &'a Expr>,
    pub distinct_ranges: &'a HashMap<String, Option<(i64, i64)>>,
    pub distinct_bases: &'a HashMap<String, Type>,
    pub fuel: u64,
    pub sink: Option<&'a mut DevSink>,
    pub repl_mode: bool,
    /// D-METADERIVE1: `emit(…)` fragments (usually unused for single exprs).
    pub emitted_fragments: Option<&'a mut Vec<String>>,
    /// D-CTEFFECT1 Tier-1 inputs recorded by the canonical host surface.
    pub embed_inputs: Option<&'a mut Vec<ComptimeInput>>,
    /// Bindings as the fragment left them. An expression can mutate a binding
    /// it reads — `reader.read_u32_le()` advances the reader — and a statement
    /// driver has to see the advance, or the next expression starts over.
    /// `None` for pure const evaluation, which has no caller scope to update.
    pub mutated: Option<&'a mut HashMap<String, CtValue>>,
}

pub struct BlockEvalRequest<'a> {
    pub stmts: &'a [Stmt],
    pub funcs: &'a HashMap<String, &'a Func>,
    pub methods: &'a HashMap<(String, String), &'a Func>,
    pub extern_names: &'a HashSet<String>,
    pub base_dir: &'a Path,
    pub globals: &'a HashMap<String, CtValue>,
    pub core_imports: &'a HashMap<String, String>,
    pub structs: &'a HashMap<String, &'a StructDef>,
    pub distinct_ranges: &'a HashMap<String, Option<(i64, i64)>>,
    pub distinct_bases: &'a HashMap<String, Type>,
    pub fuel: u64,
    pub sink: Option<&'a mut DevSink>,
    pub repl_mode: bool,
    pub allow_impure: bool,
    pub impure_depth: usize,
    pub computed_fields: &'a HashMap<(String, String), &'a Expr>,
    /// D-METADERIVE1: `emit(…)` fragments from a derive body.
    pub emitted_fragments: Option<&'a mut Vec<String>>,
    /// D-CTEFFECT1 Tier-1 inputs recorded by the canonical host surface.
    pub embed_inputs: Option<&'a mut Vec<ComptimeInput>>,
}

/// Outcome of evaluating a statement list through the canonical TIR evaluator.
pub enum StmtOutcome {
    /// Finished normally; scope holds bindings.
    Done(HashMap<String, CtValue>),
    /// `return` escaped the fragment.
    Returned {
        value: CtValue,
        scope: HashMap<String, CtValue>,
    },
}

pub struct Hooks {
    pub run_bundle: fn(&ProgramBundle, &mut DevSink, bool) -> Result<CtValue, Diagnostic>,
    pub eval_expr: fn(&mut ExprEvalRequest<'_>) -> Result<CtValue, Diagnostic>,
    pub eval_block: fn(&mut BlockEvalRequest<'_>) -> Result<StmtOutcome, Diagnostic>,
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

fn hooks() -> &'static Hooks {
    HOOKS
        .get()
        .expect("TIR eval bridge not installed — call Codegen::TIR::eval::install_comptime_bridge()")
}

pub fn run_bundle(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    allow_impure: bool,
) -> Result<CtValue, Diagnostic> {
    (hooks().run_bundle)(bundle, sink, allow_impure)
}

pub fn eval_expr(req: &mut ExprEvalRequest<'_>) -> Result<CtValue, Diagnostic> {
    (hooks().eval_expr)(req)
}

pub fn eval_block(req: &mut BlockEvalRequest<'_>) -> Result<StmtOutcome, Diagnostic> {
    (hooks().eval_block)(req)
}
