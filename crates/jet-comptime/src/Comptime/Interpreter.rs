//! Comptime/dev interpreter host: `Interp` state, fuel, `Flow`, `DevSink`.
//! Expression/statement execution goes through TirBridge → TIR evaluator (#777).
//! Residual `Methods/` helpers (`call_func`/`call_closure`) support specialized
//! host surfaces (typed decode, data pipeline) that still invoke named funcs.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{BindPattern, CtFloat, Expr, Func, Stmt, Type};

use super::Diagnostics::comptime_panic;
use crate::AST::{CtReport, CtValue};

/// Default step budget per binding (S26 rule 3). Compiler-internal, not a
/// user knob (philosophy: minimal configuration).
pub(super) const FUEL_BUDGET: u64 = 10_000_000;

/// Step budget for a whole-program `jet dev` interpretation (E2-M4). Larger
/// than the per-binding comptime budget because a `main()` does real work,
/// but still finite so a runaway loop surfaces as E2202 instead of hanging
/// the watch loop. Compiler-internal, not a user knob.
pub(super) const DEV_FUEL_BUDGET: u64 = 1_000_000_000;

/// Step budget for a single `jet repl` input (D-REPL-FUEL=A). Tighter than
/// `jet dev` because REPL snippets should be short demos; a runaway loop
/// surfaces as E1801 instead of hanging the session. The user can bypass
/// with `:run` (unbounded but still finite — this constant then applies to
/// the `:run`-spawned one-shot compile path, not the interpreter).
pub const REPL_FUEL_BUDGET: u64 = 10_000_000;

/// Control-flow signal threaded through statement execution.
pub(super) enum Flow {
    Normal,
    Break,
    Continue,
    /// D-LOOPLABEL3=A as amended by D-ARROW-CONTROL1=A: `break(name)` bubbles through enclosing loops until the
    /// matching named loop turns it into an ordinary `Break`.
    BreakLabel(String),
    /// D-LOOPLABEL3=A as amended by D-ARROW-CONTROL1=A: `next(name)` follows the same bubbling rule and becomes
    /// an ordinary `Continue` at the matching loop.
    ContinueLabel(String),
    Return(CtValue),
}

/// Where a [`Interp`] running in whole-program "dev" mode sends program
/// output. In pure comptime mode this is `None` and `print`/`eprint` never
/// reach the evaluator (the purity check rejects them as E3401 first).
///
/// The dev interpreter (E2-M4 `jet dev`) buffers stdout/stderr so the
/// watch loop can stream them; the bytes are produced exactly as the
/// compiled program would (`jet_show()` + a trailing newline), which the
/// differential battery enforces.
pub struct DevSink {
    pub stdout: String,
    pub stderr: String,
    /// Soft `core.process.exit` — set instead of killing the host process
    /// when a sink is present (interpreter / deopt / `jet run` in-process).
    pub exit_code: Option<i32>,
}

impl DevSink {
    pub fn new() -> Self {
        DevSink {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
        }
    }
}

impl Default for DevSink {
    fn default() -> Self {
        DevSink::new()
    }
}

/// D-DBG3: a source-level debugger driver. The interpreter calls
/// [`DebugHook::at_stmt`] before executing each statement, passing the
/// statement's source span, the current call depth, and the current local
/// scope. The driver decides whether to pause (and run its `(jet)` prompt);
/// it never touches generated Rust — every value it shows comes through the
/// interpreter's own `CtValue::jet_show()` path (I2). `None` outside `jet debug`.
pub trait DebugHook {
    /// Called before a statement runs. `func` is the executing function name,
    /// `depth` the user-function call depth (0 = `main`), `scope` the live
    /// locals. The driver may print, prompt, and block; it returns when the
    /// user resumes. An `Err` aborts the run (e.g. the user typed `quit`).
    fn at_stmt(
        &mut self,
        func: &str,
        depth: usize,
        span: Span,
        scope: &HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic>;
}

/// Concrete ambient operation reached by a REPL turn after arguments have
/// been evaluated but before host state is touched (D-REPLCOREEFFECT1=A).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReplEffectRequest {
    pub root: String,
    pub operation: String,
    pub resource: String,
}

/// Invocation policy seam owned by the REPL frontend. Returning an error
/// aborts before the Core operation executes.
pub trait ReplAuthorizer {
    fn preflight(&mut self, request: &ReplEffectRequest, span: Span) -> Result<(), Diagnostic>;
    fn authorize(&mut self, request: &ReplEffectRequest, span: Span) -> Result<(), Diagnostic>;
    fn fs_read(&mut self, path: &str) -> std::io::Result<Vec<u8>>;
    fn fs_write(&mut self, path: &str, bytes: &[u8], append: bool) -> std::io::Result<()>;
    fn fs_exists(&mut self, path: &str) -> std::io::Result<bool>;
    fn fs_is_dir(&mut self, path: &str) -> std::io::Result<bool>;
    fn fs_create_dir(&mut self, path: &str) -> std::io::Result<()>;
    fn fs_remove(&mut self, path: &str) -> std::io::Result<()>;
    fn verified_root(&mut self) -> std::io::Result<std::fs::File>;
    fn read_input(&mut self, prompt: &str) -> std::io::Result<String> {
        use std::io::{self, Write};
        if !prompt.is_empty() {
            print!("{prompt}");
            io::stdout().flush()?;
        }
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        Ok(line)
    }
    fn reset_session(&mut self) {}
}

pub(super) fn reborrow_repl_authorizer<'short, 'long: 'short>(
    authorizer: &'short mut Option<&'long mut dyn ReplAuthorizer>,
) -> Option<&'short mut (dyn ReplAuthorizer + 'short)> {
    match authorizer {
        Some(authorizer) => Some(&mut **authorizer),
        None => None,
    }
}

pub(super) struct Interp<'a> {
    pub(super) funcs: &'a HashMap<String, &'a Func>,
    pub(super) base_dir: &'a Path,
    pub(super) fuel: u64,
    /// `Some` in whole-program dev mode (E2-M4): `print`/`eprint` write here
    /// instead of being rejected. `None` in pure comptime mode (M9.5).
    pub(super) sink: Option<&'a mut DevSink>,
    /// D-META-EFFECT1: module alias → Core module path (e.g. `"math"` → `"core.math"`).
    /// Enables the comptime interpreter to evaluate effect-approved Core calls.
    /// Empty for contexts that have no `use` declarations (e.g. module-level consts).
    pub(super) core_imports: &'a HashMap<String, String>,
    /// D-DBG3: `Some` under `jet debug` — the source-level debugger driver,
    /// notified before every statement. `None` for every non-debug path
    /// (comptime, `jet dev`, `jet repl`), so those keep their exact behavior.
    pub(super) debugger: Option<&'a mut dyn DebugHook>,
    /// D-DBG3: user-function call depth (0 = `main`/top level), threaded so the
    /// debugger can implement `next` (step over a call) and `finish` (run to the
    /// caller). Incremented around a user-function body in `eval_call`.
    pub(super) depth: usize,
    /// D-DBG3: the name of the function whose body is currently executing, for
    /// the breakpoint banner (`in main()`). Top level / `main` until a call
    /// swaps it.
    pub(super) cur_func: String,
    /// D-CTEFFECT1: execution context for active `#Impure("reason") { … }`
    /// blocks. The source statement is the writer; sema's GateLedger reads
    /// that same statement. This depth only marshals the checked context to
    /// comptime evaluation and must not become a second audit record.
    pub(super) impure_depth: usize,
    /// D-CTEFFECT1: true when the caller compiled with `--gate impure=allow`.
    /// Without this, `#Impure` blocks are syntactically valid but Tier-2
    /// effect calls inside them still fail with E3411.
    pub(super) gates: jet_foundation::Policy::GateSet,
    /// E2-M18 / c133: true only in `run_repl_step` — enables REPL-specific
    /// diagnostics for native-only Core modules (E1802-style wording).
    pub(super) repl_mode: bool,
    /// Active lexical `#Grant` effect names in REPL mode. Sema proves the
    /// region statically; this copy gates host authorization dynamically.
    pub(super) repl_grants: Vec<String>,
    /// Host invocation policy callback. Called after concrete arguments are
    /// known and before any ambient operation executes.
    pub(super) repl_authorizer: Option<&'a mut dyn ReplAuthorizer>,
    /// True only for a live raw-REPL turn. Cancellation uses an internal
    /// unwind caught at the REPL boundary, never a user diagnostic.
    pub(super) repl_interruptible: bool,
    /// D-CTEFFECT1 Tier-1: embed_file/embed_bytes inputs accumulated during
    /// this evaluation. Each entry records the relative path and the sha256
    /// of the bytes read, for recording in `.jet/lock`. Drained by the
    /// `evaluate_*_collecting` variants after evaluation.
    pub(super) embed_inputs: Vec<crate::AST::ComptimeInput>,
    /// Static types for bindings in the active interpreter frame. An empty
    /// CtValue::List has no element to sample, so sequence identities use this
    /// declared fact instead of guessing Int.
    pub(super) binding_types: HashMap<String, Type>,
    /// c139 JIT/interpreter-parity: top-level `const`/`comptime` bindings,
    /// pre-evaluated (in declaration order, each seeing the ones before it)
    /// so any function body — not just the initializer that names them
    /// directly — can read them. Checked as a fallback under a local-scope
    /// miss (locals always shadow). Empty for contexts with no such bindings.
    pub(super) globals: &'a HashMap<String, CtValue>,
    /// c139: `(TypeName, method) -> Func` for user-written instance/associated
    /// methods — `impl Type { fn … }` / `impl Type.Trait { … }`, in-struct
    /// `fn`/`impl Trait { … }` blocks, and D-MOD2 code-module namespaced calls
    /// (registered as `(module_alias, fn_name)`). `eval_method` consults this
    /// before falling back to the built-in `apply_method` dispatch.
    pub(super) methods: &'a HashMap<(String, String), &'a Func>,
    /// c139/D-DISPLAYDBG: struct definitions by Jet type name, used only to
    /// mirror codegen's `JetDebug` field order and `#[Redact]` handling.
    pub(super) structs: &'a HashMap<String, &'a crate::AST::StructDef>,
    /// c139: `(TypeName, field) -> expr` for D-FIELDPOL1 computed fields
    /// (`name: T => expr`). Sema has already rewritten sibling names to
    /// `self.<field>` inside `expr`, so evaluating it just needs `self` bound
    /// to the struct value.
    pub(super) computed_fields: &'a HashMap<(String, String), &'a Expr>,
    /// c139: `TypeName -> Some((lo, hi))` for a `distinct Base(lo..hi)` range
    /// constraint (D-RANGETYPE1), `TypeName -> None` for every other distinct
    /// type / `#UnitFamily` member (D-DIST1/D-QUAL3). `Name(expr)` is the only
    /// call-syntax construct capitalized-name calls are used for in Jet (struct
    /// literals use `.{ }`, enum variants use `.Variant`), so an unresolved
    /// call to a name in this map is a distinct-type constructor: identity
    /// for an unranged type, a proven-in-range literal folds to a direct
    /// value, anything else is the fallible `Result`-wrapped form.
    pub(super) distinct_ranges: &'a HashMap<String, Option<(i64, i64)>>,
    /// Numeric base type for each distinct/unit target.
    pub(super) distinct_bases: &'a HashMap<String, Type>,
    /// Card #392 pass 5: `TypeName -> migration { }` blocks declared for that
    /// `#PublishedSchema` type, source order (the migration chain, oldest step
    /// first — mirrors `Codegen/Items.rs::migration_blocks`'s per-type list).
    /// Empty for a type with no migrations (the common case), which keeps
    /// `decode_traced<T>`'s fast path — try the current shape, done — the same
    /// zero-cost identity codegen's trait default gives every other type.
    pub(super) migrations: &'a HashMap<String, Vec<&'a crate::AST::MigrationDecl>>,
    pub(super) list_write_windows: HashMap<String, (String, i64)>,
}

pub(super) fn coerce_value_to_type(value: CtValue, ty: &Type) -> CtValue {
    match (value, ty) {
        (CtValue::Float(value), Type::Float32) => CtValue::Float(CtFloat::f32(value.as_f32())),
        (CtValue::Float(value), Type::Float) => CtValue::Float(CtFloat::f64(value.as_f64())),
        (CtValue::List(values), Type::List(elem) | Type::FixedList { elem, .. }) => CtValue::List(
            values
                .into_iter()
                .map(|value| coerce_value_to_type(value, elem))
                .collect(),
        ),
        (CtValue::Map(values), Type::Map { value, .. }) => CtValue::Map(
            values
                .into_iter()
                .map(|(key, item)| (key, coerce_value_to_type(item, value)))
                .collect(),
        ),
        (CtValue::Present(value), Type::Option(inner)) => {
            CtValue::Present(Box::new(coerce_value_to_type(*value, inner)))
        }
        (CtValue::Present(value), Type::Result { ok, .. }) => {
            CtValue::Present(Box::new(coerce_value_to_type(*value, ok)))
        }
        (CtValue::Failed(CtReport::Told(value)), Type::Result { err, .. }) => {
            CtValue::failed(Box::new(coerce_value_to_type(*value, err)))
        }
        (CtValue::Struct { type_name, fields }, Type::Tuple(types)) => CtValue::Struct {
            type_name,
            fields: fields
                .into_iter()
                .map(|(name, value)| {
                    let value = types
                        .iter()
                        .find(|(field, _)| field == &name)
                        .map_or(value.clone(), |(_, ty)| coerce_value_to_type(value, ty));
                    (name, value)
                })
                .collect(),
        },
        (CtValue::Closure(data), Type::Fn { ret, .. }) => {
            let mut data = (*data).clone();
            data.return_type = ret.as_deref().cloned();
            CtValue::Closure(std::sync::Arc::new(data))
        }
        (value, Type::Shared(inner) | Type::Tagged { inner, .. }) => {
            coerce_value_to_type(value, inner)
        }
        (value, _) => value,
    }
}

impl<'a> Interp<'a> {
    pub(super) fn poll_repl_interrupt(&self) {
        if self.repl_interruptible && super::repl_interrupt_count() > 0 {
            std::panic::resume_unwind(Box::new(super::ReplInterrupted));
        }
    }

    pub(super) fn burn(&mut self, span: Span) -> Result<(), Diagnostic> {
        self.poll_repl_interrupt();
        if self.fuel == 0 {
            // E2202 in whole-program dev mode (E2-M4): the interpreter ran out
            // of fuel, which almost always means an unbounded loop. E0952 in
            // pure comptime mode (M9.5).
            if self.sink.is_some() {
                return Err(Diagnostic::error(
                    "E2202",
                    "this program ran too long for `jet dev` to keep interpreting".to_string(),
                    format!(
                        "`jet dev` interprets your program to give instant feedback, but it ran more than {} steps without finishing — this usually means a loop that never ends",
                        DEV_FUEL_BUDGET
                    ),
                    "check the loop near here for a condition that never becomes false; run `jet run` to execute the real build with no step limit"
                        .to_string(),
                    Some(span),
                ));
            }
            return Err(Diagnostic::error(
                "E0952",
                "comptime evaluation used up its budget".to_string(),
                format!(
                    "this `comptime` value ran more than {} steps before compilation gave up — it may loop forever",
                    FUEL_BUDGET
                ),
                "make the computation finish in fewer steps, or compute it at runtime instead"
                    .to_string(),
                Some(span),
            ));
        }
        self.fuel -= 1;
        Ok(())
    }

    pub(super) fn exec_block(
        &mut self,
        stmts: &[Stmt],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        if stmts.is_empty() {
            return Ok(Flow::Normal);
        }
        // TIR intentionally erases `#Grant` to a plain Region. Keep REPL
        // grant scope in this host frame before the bridge evaluates a body.
        if self.repl_mode
            && stmts
                .iter()
                .any(|stmt| matches!(stmt, Stmt::Grant { .. }))
        {
            for stmt in stmts {
                match self.exec_stmt(stmt, scope)? {
                    Flow::Normal => {}
                    flow => return Ok(flow),
                }
            }
            return Ok(Flow::Normal);
        }
        let mut globals = self.globals.clone();
        for (k, v) in scope.iter() {
            globals.insert(k.clone(), v.clone());
        }
        let extern_names = HashSet::new();
        let fuel = self.fuel;
        let gates = self.gates;
        let impure_depth = self.impure_depth;
        let repl_mode = self.repl_mode;
        let base_dir = self.base_dir;
        let funcs = self.funcs;
        let core_imports = self.core_imports;
        let structs = self.structs;
        let sink = self.sink.as_deref_mut();
        let repl_grants = &self.repl_grants;
        let repl_authorizer = reborrow_repl_authorizer(&mut self.repl_authorizer);
        let embed_inputs = Some(&mut self.embed_inputs);
        let mut req = super::TirBridge::BlockEvalRequest {
            stmts,
            funcs,
            methods: self.methods,
            extern_names: &extern_names,
            base_dir,
            globals: &globals,
            core_imports,
            structs,
            computed_fields: self.computed_fields,
            distinct_ranges: self.distinct_ranges,
            distinct_bases: self.distinct_bases,
            fuel,
            sink,
            repl_mode,
            repl_grants,
            repl_authorizer,
            gates,
            impure_depth,
            embed_inputs,
        };
        match super::TirBridge::eval_block(&mut req)? {
            super::TirBridge::StmtOutcome::Done(new_scope) => {
                *scope = new_scope;
                Ok(Flow::Normal)
            }
            super::TirBridge::StmtOutcome::Returned { value, scope: new_scope } => {
                *scope = new_scope;
                Ok(Flow::Return(value))
            }
        }
    }

    /// S74: bind the names of a destructuring target from a comptime value.
    pub(super) fn bind_pattern(
        &mut self,
        pat: &BindPattern,
        value: CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        match pat {
            BindPattern::Struct { fields, .. } => {
                let CtValue::Struct { fields: vals, .. } = value else {
                    return Err(comptime_panic(
                        "this value isn't a struct, so it can't be destructured with `{ }`",
                        pat.span(),
                    ));
                };
                for f in fields {
                    let v = vals
                        .iter()
                        .find(|(n, _)| n == &f.name)
                        .map(|(_, v)| v.clone())
                        .ok_or_else(|| {
                            comptime_panic(&format!("this value has no field `{}`", f.name), f.span)
                        })?;
                    // D-DESTRUCT1: `field: local` binds under the rename, not
                    // the struct's own field name.
                    scope.insert(f.local_name().to_string(), v);
                }
            }
            BindPattern::List { elems, span } => {
                let CtValue::List(xs) = value else {
                    return Err(comptime_panic(
                        "this value isn't a list, so it can't be destructured with `[ ]`",
                        *span,
                    ));
                };
                if xs.len() != elems.len() {
                    return Err(comptime_panic(
                        &format!(
                            "this pattern needs exactly {} item{}, but the list has {}",
                            elems.len(),
                            if elems.len() == 1 { "" } else { "s" },
                            xs.len()
                        ),
                        *span,
                    ));
                }
                for (e, v) in elems.iter().zip(xs) {
                    scope.insert(e.name.clone(), v);
                }
            }
            // S73/D-SG7: `(a, b) :: p` binds named tuple fields in
            // canonical (sorted-by-name) order — a tuple value's fields are
            // always stored in that order (see `Expr::TupleLit` in `eval`),
            // so a straight positional zip lines up correctly.
            BindPattern::Tuple { elems, span } => {
                let CtValue::Struct { fields: vals, .. } = value else {
                    return Err(comptime_panic(
                        "this value isn't a tuple, so it can't be destructured with `( )`",
                        *span,
                    ));
                };
                if vals.len() != elems.len() {
                    return Err(comptime_panic(
                        &format!(
                            "this pattern needs exactly {} member{}, but the tuple has {}",
                            elems.len(),
                            if elems.len() == 1 { "" } else { "s" },
                            vals.len()
                        ),
                        *span,
                    ));
                }
                for (e, (_, v)) in elems.iter().zip(vals) {
                    scope.insert(e.name.clone(), v);
                }
            }
        }
        Ok(())
    }

    pub(super) fn exec_stmt(
        &mut self,
        stmt: &Stmt,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        if let Some(dbg) = self.debugger.take() {
            let span = stmt.span();
            let res = dbg.at_stmt(&self.cur_func, self.depth, span, scope);
            self.debugger = Some(dbg);
            res?;
        }
        if self.repl_mode {
            if let Stmt::Grant { caps, body, .. } = stmt {
                let old_len = self.repl_grants.len();
                self.repl_grants
                    .extend(caps.iter().map(|(name, _)| name.clone()));
                self.impure_depth += 1;
                let result = self.exec_block(body, scope);
                self.impure_depth -= 1;
                self.repl_grants.truncate(old_len);
                return result;
            }
        }
        self.exec_block(std::slice::from_ref(stmt), scope)
    }

    /// Canonical evaluator entry for expressions (#777): lower to TIR, then eval.
    pub(super) fn eval(
        &mut self,
        e: &Expr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        self.burn(e.span())?;
        let mut globals = self.globals.clone();
        for (k, v) in scope.iter() {
            globals.insert(k.clone(), v.clone());
        }
        let extern_names = HashSet::new();
        let fuel = self.fuel;
        let gates = self.gates;
        let impure_depth = self.impure_depth;
        let repl_mode = self.repl_mode;
        let base_dir = self.base_dir;
        let funcs = self.funcs;
        let core_imports = self.core_imports;
        let structs = self.structs;
        let sink = self.sink.as_deref_mut();
        let repl_grants = &self.repl_grants;
        let repl_authorizer = reborrow_repl_authorizer(&mut self.repl_authorizer);
        let embed_inputs = Some(&mut self.embed_inputs);
        let mut mutated = HashMap::new();
        let mut req = super::TirBridge::ExprEvalRequest {
            expr: e,
            funcs,
            methods: self.methods,
            extern_names: &extern_names,
            base_dir,
            globals: &globals,
            core_imports,
            gates,
            initial_impure_depth: impure_depth,
            structs,
            computed_fields: self.computed_fields,
            distinct_ranges: self.distinct_ranges,
            distinct_bases: self.distinct_bases,
            fuel,
            sink,
            repl_mode,
            repl_grants,
            repl_authorizer,
            embed_inputs,
            mutated: Some(&mut mutated),
        };
        let result = super::TirBridge::eval_expr(&mut req);
        drop(req);
        // Keep whatever the expression changed about bindings this scope owns.
        // Anything else in `mutated` came from `globals` and is not ours.
        for (name, value) in mutated {
            if let Some(slot) = scope.get_mut(&name) {
                *slot = value;
            }
        }
        result
    }
}



thread_local! {
    static RUNTIME_ARGV: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

struct RuntimeArgvGuard(Option<Vec<String>>);

impl Drop for RuntimeArgvGuard {
    fn drop(&mut self) {
        RUNTIME_ARGV.with(|slot| {
            *slot.borrow_mut() = std::mem::take(&mut self.0);
        });
    }
}

/// Install argv for one interpreted / deopt run (`argv[0]` = entry path).
/// When set, impure `core.io.args` uses this instead of the host process argv
/// (so `cargo test` flags never leak into example output).
pub fn with_runtime_argv<R>(args: &[String], run: impl FnOnce() -> R) -> R {
    let previous = RUNTIME_ARGV.with(|slot| {
        std::mem::replace(&mut *slot.borrow_mut(), Some(args.to_vec()))
    });
    let _guard = RuntimeArgvGuard(previous);
    run()
}

pub fn runtime_argv() -> Option<Vec<String>> {
    RUNTIME_ARGV.with(|slot| slot.borrow().clone())
}
