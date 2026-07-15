//! The tree-walking interpreter: `Interp` struct, fuel, control-flow `Flow`,
//! the `DevSink` output buffer, and the statement/expression evaluation spine.
//! Method dispatch (`eval_call`/`eval_method`/…) continues in `methods.rs`.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{
    BinOp, BindPattern, EnumLitArg, Expr, Func, IncDecOp, OrFallback as FallbackKind, PatSlot,
    Pattern, Stmt, StrFormat, StrPart, Type, UnOp,
};

use super::Builtins::{as_bool, as_int, eval_binop};
use super::Diagnostics::{
    comptime_panic, early_return_sentinel, err_propagate_sentinel, index_oob, map_missing,
    overflow, slice_oob, unsupported, unsupported_expr,
};
use super::Value::{CtKey, CtValue};

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
    /// D-LABEL1: `break @name` — bubbles up through enclosing loops until one
    /// whose own `@name` label matches, where it becomes an ordinary `Break`.
    BreakLabel(String),
    /// D-LABEL1: `continue @name` — same bubbling as `BreakLabel`, becomes an
    /// ordinary `Continue` at the matching loop.
    ContinueLabel(String),
    Return(CtValue),
}

/// What a loop body's [`Flow`] result means to the loop wrapping it, given
/// that loop's own `@name` label (if any). D-LABEL1: an unlabeled `break`/
/// `continue` always applies to its innermost loop (`Break`/`Continue`); a
/// labeled one only stops here if the name matches this loop's label —
/// otherwise it keeps bubbling outward unchanged (`Bubble`).
enum LoopStep {
    Break,
    Continue,
    Return(CtValue),
    Bubble(Flow),
}

fn loop_step(flow: Flow, label: Option<&str>) -> LoopStep {
    match flow {
        Flow::Break => LoopStep::Break,
        Flow::Continue | Flow::Normal => LoopStep::Continue,
        Flow::Return(v) => LoopStep::Return(v),
        Flow::BreakLabel(name) => {
            if label == Some(name.as_str()) {
                LoopStep::Break
            } else {
                LoopStep::Bubble(Flow::BreakLabel(name))
            }
        }
        Flow::ContinueLabel(name) => {
            if label == Some(name.as_str()) {
                LoopStep::Continue
            } else {
                LoopStep::Bubble(Flow::ContinueLabel(name))
            }
        }
    }
}

/// Where a [`Interp`] running in whole-program "dev" mode sends program
/// output. In pure comptime mode this is `None` and `print`/`eprint` never
/// reach the evaluator (the purity check rejects them as E0951 first).
///
/// The dev interpreter (E2-M4 `jet dev`) buffers stdout/stderr so the
/// watch loop can stream them; the bytes are produced exactly as the
/// compiled program would (`jet_show()` + a trailing newline), which the
/// differential battery enforces.
pub struct DevSink {
    pub stdout: String,
    pub stderr: String,
}

impl DevSink {
    pub fn new() -> Self {
        DevSink {
            stdout: String::new(),
            stderr: String::new(),
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
    fn reset_session(&mut self) {}
}

pub(super) struct Interp<'a> {
    pub(super) funcs: &'a HashMap<String, &'a Func>,
    pub(super) base_dir: &'a Path,
    pub(super) fuel: u64,
    /// `Some` in whole-program dev mode (E2-M4): `print`/`eprint` write here
    /// instead of being rejected. `None` in pure comptime mode (M9.5).
    pub(super) sink: Option<&'a mut DevSink>,
    /// D-CTCORE1: module alias → Core module path (e.g. `"math"` → `"core.math"`).
    /// Enables the comptime interpreter to evaluate whitelisted pure Core calls.
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
    /// D-CTEFFECT1: nesting depth of active `#Impure("reason") { … }` blocks.
    /// Tier-2 comptime effect calls (core.files/env/exec/io) are allowed only
    /// while this is `> 0` AND `allow_impure` is true.
    pub(super) impure_depth: usize,
    /// D-CTEFFECT1: true when the caller compiled with `--allow-impure`.
    /// Without this, `#Impure` blocks are syntactically valid but Tier-2
    /// effect calls inside them still fail with E3411.
    pub(super) allow_impure: bool,
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
    /// D-METADERIVE1=A: source fragments emitted by `emit(…)` calls inside
    /// a user-authored `derive` body. Drained by `evaluate_derive_body`.
    pub(super) emitted_fragments: Vec<String>,
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
    /// mirror codegen's `JetDebug` field order and `@[Redact]` handling.
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
    /// Card #392 pass 5: `TypeName -> migration { }` blocks declared for that
    /// `@PublishedSchema` type, source order (the migration chain, oldest step
    /// first — mirrors `Codegen/Items.rs::migration_blocks`'s per-type list).
    /// Empty for a type with no migrations (the common case), which keeps
    /// `decode_traced<T>`'s fast path — try the current shape, done — the same
    /// zero-cost identity codegen's trait default gives every other type.
    pub(super) migrations: &'a HashMap<String, Vec<&'a crate::AST::MigrationDecl>>,
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
        for stmt in stmts {
            match self.exec_stmt(stmt, scope)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
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
        // D-DBG3: notify the source-level debugger before each statement runs.
        // The driver may pause, run its `(jet)` prompt, and block here; it sees
        // only Jet line/scope (I2). Temporarily detach the hook so a re-entrant
        // call (none today) can't alias the borrow.
        if let Some(dbg) = self.debugger.take() {
            let span = stmt.span();
            let res = dbg.at_stmt(&self.cur_func, self.depth, span, scope);
            self.debugger = Some(dbg);
            res?;
        }
        match stmt {
            Stmt::Expr(e) => {
                self.eval(e, scope)?;
                Ok(Flow::Normal)
            }
            Stmt::Val(b) => {
                let v = self.eval(&b.init, scope)?;
                // S74: a destructuring target binds each field/element.
                if let Some(pat) = &b.pattern {
                    self.bind_pattern(pat, v, scope)?;
                } else {
                    scope.insert(b.name.clone(), v);
                }
                Ok(Flow::Normal)
            }
            Stmt::Assign {
                target,
                op,
                value,
                op_span,
            } => {
                self.exec_assign(target, *op, value, *op_span, scope)?;
                Ok(Flow::Normal)
            }
            Stmt::Return(e, _) => {
                let v = match e {
                    Some(e) => self.eval(e, scope)?,
                    None => CtValue::Unit,
                };
                Ok(Flow::Return(v))
            }
            Stmt::If(ifs) => self.exec_if(ifs, scope),
            Stmt::While {
                cond,
                body,
                span,
                label,
            } => {
                let label = label.as_ref().map(|(n, _)| n.as_str());
                loop {
                    self.burn(*span)?;
                    let c = self.eval(cond, scope)?;
                    if !as_bool(&c, cond.span())? {
                        break;
                    }
                    match loop_step(self.exec_block(body, scope)?, label) {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                        LoopStep::Return(v) => return Ok(Flow::Return(v)),
                        LoopStep::Bubble(f) => return Ok(f),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::For {
                var,
                var2,
                kind,
                body,
                span,
                label,
                ..
            } => self.exec_for(
                var,
                var2.as_ref(),
                kind,
                body,
                *span,
                label.as_ref().map(|(n, _)| n.as_str()),
                scope,
            ),
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                // Sema rewrites enum/equality arms into ordinary Bool
                // conditions, so a switch is a first-true-arm chain. D-IF3: a
                // complex subject (call/field, not a bare ident) is bound to
                // the synthesized name `it` so arm patterns (`PatternTest`)
                // have something to match against — mirrors what `if_arms`
                // does at parse time (Statements.rs).
                let subj_val = self.eval(subject, scope)?;
                scope.insert(crate::Syntax::KW_IT.to_string(), subj_val);
                for arm in arms {
                    let c = self.eval(&arm.cond, scope)?;
                    if as_bool(&c, arm.cond.span())? {
                        return self.exec_block(&arm.body, scope);
                    }
                }
                if let Some(body) = else_body {
                    return self.exec_block(body, scope);
                }
                Ok(Flow::Normal)
            }
            Stmt::Break(_) => Ok(Flow::Break),
            Stmt::Continue(_) => Ok(Flow::Continue),
            Stmt::Loop { body, span, label } => {
                let label = label.as_ref().map(|(n, _)| n.as_str());
                loop {
                    self.burn(*span)?;
                    match loop_step(self.exec_block(body, scope)?, label) {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                        LoopStep::Return(v) => return Ok(Flow::Return(v)),
                        LoopStep::Bubble(f) => return Ok(f),
                    }
                }
                Ok(Flow::Normal)
            }
            // D-LOOP-SEMICOLON1=A: counted loop — evaluate init, then run as while.
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                span,
                label,
            } => {
                let label = label.as_ref().map(|(n, _)| n.as_str());
                let v = self.eval(&init.init, scope)?;
                scope.insert(init.name.clone(), v);
                loop {
                    self.burn(*span)?;
                    let c = self.eval(cond, scope)?;
                    if !as_bool(&c, cond.span())? {
                        break;
                    }
                    match loop_step(self.exec_block(body, scope)?, label) {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                        LoopStep::Return(v) => return Ok(Flow::Return(v)),
                        LoopStep::Bubble(f) => return Ok(f),
                    }
                    self.exec_stmt(step, scope)?;
                }
                Ok(Flow::Normal)
            }
            Stmt::Unsafe { span, .. } => Err(unsupported("an `#Unsafe` block", *span)),
            Stmt::Reactive { span, .. } => Err(unsupported("a `#Reactive` block", *span)),
            // D-CTEFFECT1: `#Impure("reason") { … }` — gate for Tier-2 ambient
            // comptime effects. Increments impure_depth around the body so that
            // `apply_core_call` knows we're inside a gate. The E3411 (gate but no
            // flag) check is in `apply_impure_core_call`; the body always runs so
            // the interpreter can reach the impure call and produce a good span.
            Stmt::Impure { body, .. } => {
                // c139 JIT/interpreter-parity fix: entering the gate is always
                // fine — it just marks Tier-2 comptime effects as *reachable*.
                // The actual `--allow-impure` requirement is enforced per call
                // in `eval_method` (E3411 there names the specific call, e.g.
                // `core.files.read()`), which also correctly leaves a gate whose
                // body never attempts a Tier-2 effect (e.g. only `print`s, or
                // a nested `#Impure` demo block) needing no flag at all —
                // matching the doc comment on D-CTEFFECT1's own example.
                self.impure_depth += 1;
                let result = self.exec_block(body, scope);
                self.impure_depth -= 1;
                result
            }
            // D-SHIELDNAME1=A: `#Shield { … }` is a runtime scheduler region; at
            // comptime there are no tasks or deadlines, so it is a transparent no-op
            // wrapper — execute the body directly.
            Stmt::Shield { body, .. } => self.exec_block(body, scope),
            // D-CANVASSTATE1=D: `#Off` is real checked code but never executes.
            Stmt::Off { .. } => Ok(Flow::Normal),
            // D-CANVASSTATE1=D: comptime execution is a dev/debug tier.
            Stmt::DebugOnly { body, .. } => self.exec_block(body, scope),
            // D-REGION1: allocation regions are a runtime/codegen construct; the
            // comptime interpreter has no arenas, so a `region` block is declined.
            Stmt::Region { span, .. } => Err(unsupported("a `region` block", *span)),
            Stmt::TaskGroup { span, .. } => Err(unsupported("a `taskgroup` block", *span)),
            // D-LAYOUT1: the constraint solver is a runtime construct (real
            // `Rc<RefCell<..>>` handle state); the comptime interpreter has
            // no solver, so a `layout` block is declined, same as `region`/
            // `taskgroup`.
            Stmt::Layout { span, .. } => Err(unsupported("a `layout` block", *span)),
            Stmt::Caps { span, .. } => Err(unsupported("a `#Caps` block", *span)),
            Stmt::Grant { caps, body, .. } if self.repl_mode => {
                let old_len = self.repl_grants.len();
                self.repl_grants.extend(caps.iter().map(|(name, _)| name.clone()));
                self.impure_depth += 1;
                let result = self.exec_block(body, scope);
                self.impure_depth -= 1;
                self.repl_grants.truncate(old_len);
                result
            }
            Stmt::Grant { span, .. } => Err(unsupported("a `#Grant` block", *span)),
            // D-TXN1–D-TXN4: a transaction block is a runtime/codegen construct; the
            // comptime interpreter has no transactions, so `#Transact` is declined.
            Stmt::Transact { span, .. } => Err(unsupported("a `#Transact` block", *span)),
            // D-STREAMYIELD1: a generator suspends on a real thread/channel at
            // runtime; the comptime interpreter has no such thing.
            Stmt::Yield(_, span) => Err(unsupported("a `yield`", *span)),
            // D-CTX1: the smart-context block is a runtime/codegen construct; the
            // comptime interpreter declines it (no thread-local context at compile time).
            Stmt::ContextBlock { span, .. } => Err(unsupported("a `#Context` block", *span)),
            // D-TERM1 (ratified 2026-06-22): `live { … }` is a runtime/codegen
            // construct; the comptime interpreter has no terminal at compile time.
            Stmt::Live { span, .. } => Err(unsupported("a `live` block", *span)),
            // D-DOTSCOPE1: `.setup`/`.expect_fail`/`.timeout`/`.skip` are `jet test`
            // harness constructs — the comptime interpreter never runs them.
            Stmt::ScopeMember { span, .. } => Err(unsupported("a scope-member block", *span)),
            // D-DET1: `assume_deterministic { … }` is semantically transparent — it
            // only suspends the sema determinism check. The interpreter just runs
            // its body (the suspension is a no-op at comptime, which is already pure).
            Stmt::AssumeDet { body, .. } => self.exec_block(body, scope),
            // D-LABEL1: labeled `break @name`/`continue @name` — bubble a
            // named Flow signal outward; the matching labeled loop (found by
            // `loop_step`) turns it into an ordinary Break/Continue.
            Stmt::BreakLabel(name, _) => Ok(Flow::BreakLabel(name.clone())),
            Stmt::ContinueLabel(name, _) => Ok(Flow::ContinueLabel(name.clone())),
            // D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }`
            // already ran (and was purity/fuel-checked) at sema time; it is
            // build-time-only and erases from the compiled program (I3). The
            // dev interpreter re-runs it — same pure body, same guaranteed
            // termination — directly in the *current* scope (not a child
            // scope) so a binding inside the block "leaks into the
            // surrounding comptime scope" exactly as the doc comment above
            // the example describes, and a later `$name` splice resolves it.
            Stmt::ComptimeBlock { body, .. } => self.exec_block(body, scope),
            // D-WHEN1 (ratified 2026-06-19): sema already selected the arm
            // (selected_then = Some(…)); execute only that arm. If sema didn't
            // run (None), skip both — matching codegen's dumb-emit behaviour.
            Stmt::ComptimeIf {
                then_body,
                else_body,
                selected_then,
                ..
            } => match selected_then {
                Some(true) => self.exec_block(then_body, scope),
                Some(false) => {
                    if let Some(eb) = else_body {
                        self.exec_block(eb, scope)
                    } else {
                        Ok(Flow::Normal)
                    }
                }
                None => Ok(Flow::Normal),
            },
            // D-OSTARGET2=B: `comptime if build.os == { … }` is desugared into a
            // `comptime if` chain in sema before the interpreter ever runs, so
            // this is unreachable; erase like a comptime block for safety.
            Stmt::ComptimeSwitch { .. } => Ok(Flow::Normal),
        }
    }

    pub(super) fn exec_if(
        &mut self,
        ifs: &crate::AST::IfStmt,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        let c = self.eval(&ifs.cond, scope)?;
        if as_bool(&c, ifs.cond.span())? {
            return self.exec_block(&ifs.then_body, scope);
        }
        match &ifs.else_branch {
            Some(crate::AST::ElseBranch::ElseIf(inner)) => self.exec_if(inner, scope),
            Some(crate::AST::ElseBranch::Else(body)) => self.exec_block(body, scope),
            None => Ok(Flow::Normal),
        }
    }

    fn exec_for(
        &mut self,
        var: &str,
        var2: Option<&(String, Span)>,
        kind: &crate::AST::ForKind,
        body: &[Stmt],
        span: Span,
        label: Option<&str>,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        match kind {
            crate::AST::ForKind::Range { start, end, step } => {
                let a = as_int(&self.eval(start, scope)?, start.span())?;
                let b = as_int(&self.eval(end, scope)?, end.span())?;
                // S22 (D-SG8): `a..b` is inclusive; `step n` strides by n
                // (sema guarantees a positive Int).
                let stride = match step {
                    Some(step) => as_int(&self.eval(step, scope)?, step.span())?.max(1),
                    None => 1,
                };
                let mut i = a;
                while i <= b {
                    self.burn(span)?;
                    scope.insert(var.to_string(), CtValue::Int(i));
                    match loop_step(self.exec_block(body, scope)?, label) {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                        LoopStep::Return(v) => return Ok(Flow::Return(v)),
                        LoopStep::Bubble(f) => return Ok(f),
                    }
                    i += stride;
                }
                Ok(Flow::Normal)
            }
            crate::AST::ForKind::In { collection } => {
                let c = self.eval(collection, scope)?;
                match c {
                    CtValue::List(items) => {
                        for item in items {
                            self.burn(span)?;
                            scope.insert(var.to_string(), item);
                            match loop_step(self.exec_block(body, scope)?, label) {
                                LoopStep::Break => break,
                                LoopStep::Continue => {}
                                LoopStep::Return(v) => return Ok(Flow::Return(v)),
                                LoopStep::Bubble(f) => return Ok(f),
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    CtValue::Bytes(bs) => {
                        for byte in bs {
                            self.burn(span)?;
                            scope.insert(var.to_string(), CtValue::Int(byte as i64));
                            match loop_step(self.exec_block(body, scope)?, label) {
                                LoopStep::Break => break,
                                LoopStep::Continue => {}
                                LoopStep::Return(v) => return Ok(Flow::Return(v)),
                                LoopStep::Bubble(f) => return Ok(f),
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    CtValue::Map(m) => {
                        for (k, v) in m {
                            self.burn(span)?;
                            if let Some((v2name, _)) = var2 {
                                scope.insert(var.to_string(), k.to_value());
                                scope.insert(v2name.clone(), v);
                            } else {
                                scope.insert(var.to_string(), k.to_value());
                            }
                            match loop_step(self.exec_block(body, scope)?, label) {
                                LoopStep::Break => break,
                                LoopStep::Continue => {}
                                LoopStep::Return(v) => return Ok(Flow::Return(v)),
                                LoopStep::Bubble(f) => return Ok(f),
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    CtValue::Str(s) => {
                        // `for c in s.chars()` lowers the receiver to the string.
                        for ch in s.chars() {
                            self.burn(span)?;
                            scope.insert(var.to_string(), CtValue::Char(ch));
                            match loop_step(self.exec_block(body, scope)?, label) {
                                LoopStep::Break => break,
                                LoopStep::Continue => {}
                                LoopStep::Return(v) => return Ok(Flow::Return(v)),
                                LoopStep::Bubble(f) => return Ok(f),
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    _ => Err(unsupported("looping over this value", span)),
                }
            }
        }
    }

    fn eval_incdec(
        &mut self,
        op: IncDecOp,
        operand: &Expr,
        postfix: bool,
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let delta = match op {
            IncDecOp::Inc => 1,
            IncDecOp::Dec => -1,
        };
        match operand {
            Expr::Ident(name, name_span) => {
                let CtValue::Int(n) = scope
                    .get(name)
                    .cloned()
                    .ok_or_else(|| unsupported("this update", *name_span))?
                else {
                    let old = scope.get(name).cloned().unwrap();
                    return Err(unsupported(
                        &format!("`++`/`--` on {}", old.jet_show()),
                        span,
                    ));
                };
                let new_n = n
                    .checked_add(delta)
                    .ok_or_else(|| overflow("increment/decrement", span))?;
                let ret = if postfix {
                    CtValue::Int(n)
                } else {
                    CtValue::Int(new_n)
                };
                scope.insert(name.clone(), CtValue::Int(new_n));
                Ok(ret)
            }
            _ => Err(unsupported("this increment/decrement", span)),
        }
    }

    fn exec_assign(
        &mut self,
        target: &crate::AST::LValue,
        op: Option<BinOp>,
        value: &Expr,
        op_span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let rhs = self.eval(value, scope)?;
        match target {
            crate::AST::LValue::Local { name, name_span } => {
                let new = match op {
                    None => rhs,
                    Some(op) => {
                        let cur = scope
                            .get(name)
                            .cloned()
                            .ok_or_else(|| unsupported("this assignment", *name_span))?;
                        eval_binop(op, cur, rhs, op_span)?
                    }
                };
                scope.insert(name.clone(), new);
                Ok(())
            }
            crate::AST::LValue::Index {
                base, index, span, ..
            } => {
                // Only `name[key] = v` is supported (the common case).
                let Expr::Ident(bname, _) = base.as_ref() else {
                    return Err(unsupported("this indexed assignment", *span));
                };
                let key = self.eval(index, scope)?;
                let mut container = scope
                    .get(bname)
                    .cloned()
                    .ok_or_else(|| unsupported("this indexed assignment", *span))?;
                match &mut container {
                    CtValue::List(xs) => {
                        let i = as_int(&key, index.span())?;
                        let new = match op {
                            None => rhs,
                            Some(op) => {
                                let cur = list_get(xs, i, *span)?;
                                eval_binop(op, cur, rhs, op_span)?
                            }
                        };
                        if i < 0 || i as usize >= xs.len() {
                            return Err(index_oob(xs.len(), i, *span));
                        }
                        xs[i as usize] = new;
                    }
                    CtValue::Map(m) => {
                        let k = CtKey::from_value(key)
                            .ok_or_else(|| unsupported("this map key type", index.span()))?;
                        let new = match op {
                            None => rhs,
                            Some(op) => {
                                let cur = m.get(&k).cloned().unwrap_or(CtValue::Int(0));
                                eval_binop(op, cur, rhs, op_span)?
                            }
                        };
                        m.insert(k, new);
                    }
                    _ => return Err(unsupported("this indexed assignment", *span)),
                }
                scope.insert(bname.clone(), container);
                Ok(())
            }
            // D-MUTSELF1: a field-assignment `place.field = v`. The comptime
            // interpreter has no struct-field mutation model — report it unsupported
            // rather than silently dropping the write.
            crate::AST::LValue::Field { span, .. } => {
                Err(unsupported("this field assignment", *span))
            }
        }
    }

    pub(super) fn eval(
        &mut self,
        e: &Expr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        self.burn(e.span())?;
        match e {
            Expr::Int(n, _, _, _) => Ok(CtValue::Int(*n)),
            Expr::Float(f, _, _) => Ok(CtValue::Float(*f)),
            Expr::Bool(b, _) => Ok(CtValue::Bool(*b)),
            Expr::Char(c, _) => Ok(CtValue::Char(*c)),
            Expr::Str(parts, _) => {
                let mut s = String::new();
                for part in parts {
                    match part {
                        StrPart::Lit(t) => s.push_str(t),
                        StrPart::Interp(e, fmt) => {
                            let v = self.eval(e, scope)?;
                            match fmt {
                                // D-DISPLAY-SHAPE: bare `{value}` — a user
                                // `impl Type.Display` (if any) wins over the
                                // built-in rendering (D-DISPLAYDBG1/2).
                                StrFormat::Display => {
                                    s.push_str(&self.show_value(&v, e.span())?);
                                }
                                // `{value@Debug}` always uses the built-in
                                // (auto-derived-Debug-shaped) rendering, never
                                // a user `Display` impl.
                                StrFormat::Debug => s.push_str(&self.debug_value(&v)),
                            }
                        }
                    }
                }
                Ok(CtValue::Str(s))
            }
            Expr::ListLit(elems, _) => {
                let mut xs = Vec::with_capacity(elems.len());
                for el in elems {
                    xs.push(self.eval(el, scope)?);
                }
                Ok(CtValue::List(xs))
            }
            Expr::MapLit(entries, _) => {
                let mut m = BTreeMap::new();
                for (k, v) in entries {
                    let key = CtKey::from_value(self.eval(k, scope)?)
                        .ok_or_else(|| unsupported("this map key type", k.span()))?;
                    let val = self.eval(v, scope)?;
                    m.insert(key, val);
                }
                Ok(CtValue::Map(m))
            }
            Expr::Ident(name, span) => scope
                .get(name)
                .or_else(|| self.globals.get(name))
                .cloned()
                .or_else(|| {
                    self.funcs
                        .get(name.as_str())
                        .map(|f| fn_value(name, f, *span))
                })
                .ok_or_else(|| unsupported(&format!("the name `{}`", name), *span)),
            Expr::Unary(op, inner, span) => {
                let v = self.eval(inner, scope)?;
                match (op, v) {
                    (UnOp::Neg, CtValue::Int(n)) => n
                        .checked_neg()
                        .map(CtValue::Int)
                        .ok_or_else(|| overflow("negate", *span)),
                    (UnOp::Neg, CtValue::Float(f)) => Ok(CtValue::Float(-f)),
                    (UnOp::Neg, CtValue::BigInt(b)) => Ok(CtValue::BigInt(b.neg())),
                    (UnOp::Not, CtValue::Bool(b)) => Ok(CtValue::Bool(!b)),
                    _ => Err(unsupported("this operation", *span)),
                }
            }
            Expr::IncDec {
                op,
                operand,
                postfix,
                span,
            } => self.eval_incdec(*op, operand, *postfix, *span, scope),
            Expr::Binary(op, l, r, span) => {
                // Short-circuit && / || to match runtime.
                if matches!(op, BinOp::And | BinOp::Or) {
                    let lv = as_bool(&self.eval(l, scope)?, l.span())?;
                    if matches!(op, BinOp::And) && !lv {
                        return Ok(CtValue::Bool(false));
                    }
                    if matches!(op, BinOp::Or) && lv {
                        return Ok(CtValue::Bool(true));
                    }
                    let rv = as_bool(&self.eval(r, scope)?, r.span())?;
                    return Ok(CtValue::Bool(rv));
                }
                let lv = self.eval(l, scope)?;
                let rv = self.eval(r, scope)?;
                eval_binop(*op, lv, rv, *span)
            }
            Expr::Index {
                base, index, span, ..
            } => {
                let b = self.eval(base, scope)?;
                let i = self.eval(index, scope)?;
                match b {
                    CtValue::List(xs) => list_get(&xs, as_int(&i, index.span())?, *span),
                    CtValue::Bytes(bs) => {
                        let xs: Vec<CtValue> =
                            bs.iter().map(|byte| CtValue::Int(*byte as i64)).collect();
                        list_get(&xs, as_int(&i, index.span())?, *span)
                    }
                    CtValue::Map(m) => {
                        let k = CtKey::from_value(i)
                            .ok_or_else(|| unsupported("this map key type", index.span()))?;
                        m.get(&k).cloned().ok_or_else(|| map_missing(*span))
                    }
                    _ => Err(unsupported("indexing this value", *span)),
                }
            }
            Expr::Field(inner, member, span) => {
                if let Expr::Ident(type_name, _) = inner.as_ref() {
                    // If the name is in scope as a variable, do field access on
                    // the value (e.g. TypeInfo struct from a derive body).
                    // Otherwise it is an enum-variant literal (Color.Red).
                    if !scope.contains_key(type_name.as_str()) {
                        return Ok(CtValue::Enum {
                            type_name: type_name.clone(),
                            variant: member.clone(),
                            args: Vec::new(),
                        });
                    }
                }
                let v = self.eval(inner, scope)?;
                match &v {
                    CtValue::Struct { type_name, fields } => {
                        if let Some((_, value)) = fields.iter().find(|(name, _)| name == member) {
                            return Ok(value.clone());
                        }
                        // D-FIELDPOL1: not a stored field — try a computed
                        // field (`name: T => expr`, sibling names already
                        // rewritten to `self.<field>` by sema).
                        if let Some(expr) = self
                            .computed_fields
                            .get(&(type_name.clone(), member.clone()))
                            .copied()
                        {
                            let mut self_scope = HashMap::new();
                            self_scope.insert("self".to_string(), v.clone());
                            return self.eval(expr, &mut self_scope);
                        }
                        Err(unsupported(&format!("the field `.{}`", member), *span))
                    }
                    _ => Err(unsupported("field access on this value", *span)),
                }
            }
            Expr::StructLit {
                type_name, fields, ..
            } => {
                let mut out = Vec::with_capacity(fields.len());
                for (name, _, expr) in fields {
                    out.push((name.clone(), self.eval(expr, scope)?));
                }
                Ok(CtValue::Struct {
                    type_name: type_name.clone(),
                    fields: out,
                })
            }
            // S73/D-SG7: a named tuple literal. Evaluate each member in
            // written order (side effects, if any, run left to right), then
            // canonicalize into sorted-by-name order — the same order sema
            // gives `Type::Tuple` (`canonicalize_tuple_fields`) and codegen's
            // `JetTup_*` struct — so field access, equality, and `(a, b) ::`
            // destructuring all agree with the compiled build regardless of
            // the order the literal was written in.
            Expr::TupleLit(fields, _, _) => {
                let mut out = Vec::with_capacity(fields.len());
                for (name, expr) in fields {
                    out.push((name.clone(), self.eval(expr, scope)?));
                }
                let canonical = crate::AST::canonicalize_tuple_fields(out);
                let type_name = format!(
                    "({})",
                    canonical
                        .iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                Ok(CtValue::Struct {
                    type_name,
                    fields: canonical,
                })
            }
            Expr::EnumLit {
                type_name,
                variant,
                args,
                ..
            } => {
                let mut out = Vec::with_capacity(args.len());
                for arg in args {
                    match arg {
                        EnumLitArg::Positional(expr) => out.push((None, self.eval(expr, scope)?)),
                        EnumLitArg::Named { label, expr } => {
                            out.push((Some(label.clone()), self.eval(expr, scope)?))
                        }
                    }
                }
                Ok(CtValue::Enum {
                    type_name: type_name.clone(),
                    variant: variant.clone(),
                    args: out,
                })
            }
            Expr::Slice {
                base,
                start,
                end,
                span,
            } => {
                let b = self.eval(base, scope)?;
                let a = as_int(&self.eval(start, scope)?, start.span())?;
                let z = as_int(&self.eval(end, scope)?, end.span())?;
                match b {
                    CtValue::List(xs) => slice_inclusive(&xs, a, z, *span).map(CtValue::List),
                    CtValue::Str(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        let n = chars.len() as i64;
                        if a < 0 || z < 0 || a > z || z >= n {
                            return Err(slice_oob(n, a, z, *span));
                        }
                        Ok(CtValue::Str(
                            chars[a as usize..=z as usize].iter().collect(),
                        ))
                    }
                    _ => Err(unsupported("slicing this value", *span)),
                }
            }
            // D-TAINT1: the value-fact tag is erased; evaluate the inner value.
            Expr::Tainted(inner, _, _) => self.eval(inner, scope),
            // `~expr`: a value-semantics marker sema/codegen use to make
            // an explicit clone visible at the call site. The tree-walker
            // already hands every value around as an owned `CtValue` clone
            // (no aliasing), so `~` is a plain pass-through here too.
            Expr::Copy(inner, _) => self.eval(inner, scope),
            Expr::Present(inner, _) => Ok(CtValue::Some(Box::new(self.eval(inner, scope)?))),
            Expr::Absent(_) => Ok(CtValue::None(Type::Int)),
            Expr::Call(call) => self.eval_call(&call.name, call.name_span, &call.args, scope),
            // c139/HOF: calling an already-evaluated value — `f(x)(y)`, or a
            // callee produced by anything other than a bare name (a stored
            // closure reached through a bare `Ident` still parses as
            // `Expr::Call`, handled above; this is the postfix-chain shape).
            Expr::CallValue { callee, args, span } => {
                let f = self.eval(callee, scope)?;
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval(&a.expr, scope)?);
                }
                self.call_closure(&f, vals, *span)
            }
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                type_args,
                args,
                ..
            } => self.eval_method(receiver, method, *method_span, type_args, args, scope),
            Expr::If {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                ..
            } => {
                let c = self.eval(cond, scope)?;
                if as_bool(&c, cond.span())? {
                    match self.exec_block(then_body, scope)? {
                        Flow::Return(v) => Ok(v),
                        _ => self.eval(then_value, scope),
                    }
                } else {
                    match self.exec_block(else_body, scope)? {
                        Flow::Return(v) => Ok(v),
                        _ => self.eval(else_value, scope),
                    }
                }
            }
            Expr::FanOut {
                callee,
                items,
                span,
            } => self.eval_fan_out(callee, items, *span, scope),
            // D-CTMARKER1=C: `$name` outside an emit() string — look up in scope like Ident.
            Expr::ComptimeSplice { name, span, .. } => scope
                .get(name)
                .cloned()
                .ok_or_else(|| unsupported(&format!("the name `{}`", name), *span)),
            // c97/D-STRPARSE1: `ok(expr)` — wraps a value in the success arm.
            Expr::Ok(inner, _) => {
                let v = self.eval(inner, scope)?;
                Ok(CtValue::ResOk(Box::new(v)))
            }
            // c97/D-STRPARSE1: `err(expr)` — wraps a value in the failure arm.
            Expr::Err(inner, _) => {
                let v = self.eval(inner, scope)?;
                Ok(CtValue::ResErr(Box::new(v)))
            }
            // c97/D-STRPARSE1: `expr?` — unwrap `ok(v)` to `v`, propagate `err(e)`.
            // For `T?` (Option): unwrap `Some(v)` to `v`, propagate `None` as an
            // empty-error sentinel.
            Expr::Try(inner, span, _convert) => {
                let v = self.eval(inner, scope)?;
                match v {
                    CtValue::ResOk(inner) => Ok(*inner),
                    CtValue::ResErr(e) => {
                        // Propagate the error through the call stack via sentinel.
                        Err(err_propagate_sentinel(&e.jet_show(), *span))
                    }
                    CtValue::Some(inner) => Ok(*inner),
                    CtValue::None(_) => Err(err_propagate_sentinel("null propagated via ?", *span)),
                    other => Err(unsupported(
                        &format!(
                            "using `?` on a value that isn't a result or option (got {})",
                            other.jet_show()
                        ),
                        *span,
                    )),
                }
            }
            // c97/D-STRPARSE1: `value ?? fallback` — use fallback on failure/absence.
            Expr::OrFallback {
                value,
                fallback,
                is_option: _,
                span: _,
            } => {
                let v = self.eval(value, scope)?;
                // Runtime variant is authoritative here. REPL statements are
                // interpreted from the accepted AST while sema annotates a
                // checked clone, so relying on sema's `is_option` bit would
                // make a cross-turn `None ?? fallback` silently stay `None`.
                let is_absent = matches!(v, CtValue::None(_) | CtValue::ResErr(_));
                if is_absent {
                    // Evaluate the fallback.
                    match fallback {
                        FallbackKind::Value(fb) => self.eval(fb, scope),
                        FallbackKind::Return(ret_expr, ret_span) => {
                            let rv = match ret_expr {
                                Some(e) => self.eval(e, scope)?,
                                None => CtValue::Unit,
                            };
                            // Signal an early return via the sentinel — eval_call
                            // intercepts EARLY_RETURN_CODE and converts it to a
                            // `CtValue::Unit` / value return from the callee.
                            Err(early_return_sentinel(&rv.jet_show(), *ret_span))
                        }
                        FallbackKind::Panic { name_span, args } => {
                            let msg = match args.first() {
                                Some(a) => self.eval(&a.expr, scope)?.jet_show(),
                                None => "?? fallback panic".to_string(),
                            };
                            Err(comptime_panic(&msg, *name_span))
                        }
                        // `?? break` / `?? continue` are runtime loop controls and
                        // are not evaluable at comptime.
                        FallbackKind::Break(span) | FallbackKind::Continue(span) => {
                            Err(unsupported("loop control in `??`", *span))
                        }
                    }
                } else {
                    // Value is present/ok — unwrap it.
                    match v {
                        CtValue::Some(inner) | CtValue::ResOk(inner) => Ok(*inner),
                        other => Ok(other),
                    }
                }
            }
            Expr::Paren(inner, _) => self.eval(inner, scope),
            // S31: `subject == pattern` — evaluate the subject once, test it
            // against the pattern, and bind any names the pattern captures
            // into the current scope (so they're visible in the rest of the
            // condition / the `if`'s then-body, matching the switch-arm
            // semantics `Stmt::Switch` already relies on).
            Expr::PatternTest {
                subject, pattern, ..
            } => {
                let v = self.eval(subject, scope)?;
                let matched = self.match_pattern(&v, pattern, scope)?;
                Ok(CtValue::Bool(matched))
            }
            // D-CHAINCMP1: `a <= b < c` — each shared middle operand is
            // evaluated exactly once, then every adjacent pair is compared
            // and the results AND-ed (short-circuiting on the first false,
            // matching what the pairs would do written out by hand).
            Expr::CompareChain { operands, ops, .. } => {
                let mut vals = Vec::with_capacity(operands.len());
                for o in operands {
                    vals.push(self.eval(o, scope)?);
                }
                for (i, op) in ops.iter().enumerate() {
                    let r = eval_binop(*op, vals[i].clone(), vals[i + 1].clone(), e.span())?;
                    if !as_bool(&r, e.span())? {
                        return Ok(CtValue::Bool(false));
                    }
                }
                Ok(CtValue::Bool(true))
            }
            // c139: a lambda literal — capture the *current* scope by value
            // (a tree-walker over-captures rather than tracking free
            // variables) so the closure still sees its bindings after this
            // `eval` call returns, e.g. once it's handed to `.filter(…)`.
            Expr::Lambda(l) => Ok(CtValue::Closure(std::sync::Arc::new(
                crate::AST::ClosureData {
                    lambda: l.clone(),
                    captured: scope.clone(),
                },
            ))),
            other => Err(unsupported_expr(other)),
        }
    }

    /// S31/S32/S34/D-PATW/D-PATR/D-PATO/D-DESTRUCT1: match `v` against
    /// `pattern`, binding any names the pattern captures into `scope` when it
    /// matches. Mirrors the shapes `CtValue` construction already produces
    /// (`Expr::EnumLit`/`Expr::Present`/`Expr::Ok`/`Expr::Err` above) — every
    /// pattern kind here has a matching value-construction arm elsewhere in
    /// this file.
    fn match_pattern(
        &mut self,
        v: &CtValue,
        pattern: &Pattern,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<bool, Diagnostic> {
        match pattern {
            Pattern::Present { binding, .. } => match v {
                CtValue::Some(inner) => {
                    scope.insert(binding.clone(), (**inner).clone());
                    Ok(true)
                }
                CtValue::None(_) => Ok(false),
                other => Err(unsupported(
                    &format!(
                        "matching `value(..)` against a value that isn't an option (got {})",
                        other.jet_show()
                    ),
                    pattern.span(),
                )),
            },
            Pattern::Absent(_) => match v {
                CtValue::None(_) => Ok(true),
                CtValue::Some(_) => Ok(false),
                other => Err(unsupported(
                    &format!(
                        "matching `null` against a value that isn't an option (got {})",
                        other.jet_show()
                    ),
                    pattern.span(),
                )),
            },
            Pattern::Ok { binding, .. } => match v {
                CtValue::ResOk(inner) => {
                    scope.insert(binding.clone(), (**inner).clone());
                    Ok(true)
                }
                CtValue::ResErr(_) => Ok(false),
                other => Err(unsupported(
                    &format!(
                        "matching `ok(..)` against a value that isn't a result (got {})",
                        other.jet_show()
                    ),
                    pattern.span(),
                )),
            },
            Pattern::Err { binding, .. } => match v {
                CtValue::ResErr(inner) => {
                    scope.insert(binding.clone(), (**inner).clone());
                    Ok(true)
                }
                CtValue::ResOk(_) => Ok(false),
                other => Err(unsupported(
                    &format!(
                        "matching `err(..)` against a value that isn't a result (got {})",
                        other.jet_show()
                    ),
                    pattern.span(),
                )),
            },
            Pattern::Variant {
                variant, bindings, ..
            } => match v {
                CtValue::Enum {
                    variant: vname,
                    args,
                    ..
                } => {
                    if vname != variant {
                        return Ok(false);
                    }
                    for (slot, (_, arg_val)) in bindings.iter().zip(args.iter()) {
                        match slot {
                            PatSlot::Wildcard => {}
                            PatSlot::Bind(name) => {
                                scope.insert(name.clone(), arg_val.clone());
                            }
                            PatSlot::Range { lo, hi } => {
                                let n = as_int(arg_val, pattern.span())?;
                                if n < *lo || n > *hi {
                                    return Ok(false);
                                }
                            }
                        }
                    }
                    Ok(true)
                }
                other => Err(unsupported(
                    &format!(
                        "matching an enum-variant pattern against a value that isn't an enum (got {})",
                        other.jet_show()
                    ),
                    pattern.span(),
                )),
            },
            Pattern::Range { lo, hi, .. } => match v {
                CtValue::Int(n) => Ok(*n >= *lo && *n <= *hi),
                CtValue::Char(c) => {
                    let n = *c as i64;
                    Ok(n >= *lo && n <= *hi)
                }
                other => Err(unsupported(
                    &format!(
                        "matching a range pattern against a value that isn't Int/Char (got {})",
                        other.jet_show()
                    ),
                    pattern.span(),
                )),
            },
            Pattern::Or(alts, _) => {
                for alt in alts {
                    if self.match_pattern(v, alt, scope)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Pattern::Struct { fields, .. } => match v {
                CtValue::Struct { fields: sfields, .. } => {
                    for f in fields {
                        match f {
                            crate::AST::StructPatField::Bind { field, local, .. } => {
                                let val = sfields
                                    .iter()
                                    .find(|(n, _)| n == field)
                                    .map(|(_, v)| v.clone())
                                    .ok_or_else(|| {
                                        unsupported(&format!("the field `.{}`", field), pattern.span())
                                    })?;
                                scope.insert(local.clone(), val);
                            }
                            crate::AST::StructPatField::Value { field, value, .. } => {
                                let expected = self.eval(value, scope)?;
                                let actual = sfields.iter().find(|(n, _)| n == field).map(|(_, v)| v);
                                if actual != Some(&expected) {
                                    return Ok(false);
                                }
                            }
                        }
                    }
                    Ok(true)
                }
                other => Err(unsupported(
                    &format!(
                        "matching a struct pattern against a value that isn't a struct (got {})",
                        other.jet_show()
                    ),
                    pattern.span(),
                )),
            },
            // D-PARSESTR1: an interpolation literal in pattern position —
            // fixed text anchors the match, each `{hole}` binds the substring
            // between its anchors (non-greedy: up to the next literal, or to
            // the end of the subject for a trailing hole), typed holes
            // additionally requiring that substring to parse as `ty` (a
            // failed parse is a non-match, not an error — E0148 needs an
            // `else` for exactly this). The whole subject must be consumed.
            Pattern::StrMatch { parts, span } => match v {
                CtValue::Str(s) => {
                    let mut pos = 0usize;
                    for (i, part) in parts.iter().enumerate() {
                        match part {
                            crate::AST::StrMatchPart::Lit(lit) => {
                                if !s[pos..].starts_with(lit.as_str()) {
                                    return Ok(false);
                                }
                                pos += lit.len();
                            }
                            crate::AST::StrMatchPart::Hole { name, ty, span } => {
                                let end = match parts.get(i + 1) {
                                    Some(crate::AST::StrMatchPart::Lit(next_lit)) => {
                                        match s[pos..].find(next_lit.as_str()) {
                                            Some(off) => pos + off,
                                            None => return Ok(false),
                                        }
                                    }
                                    _ => s.len(),
                                };
                                let captured = &s[pos..end];
                                pos = end;
                                let val = match ty {
                                    None => CtValue::Str(captured.to_string()),
                                    Some(Type::Int) => match captured.parse::<i64>() {
                                        Ok(n) => CtValue::Int(n),
                                        Err(_) => return Ok(false),
                                    },
                                    Some(Type::Float) => match captured.parse::<f64>() {
                                        Ok(f) => CtValue::Float(f),
                                        Err(_) => return Ok(false),
                                    },
                                    Some(_) => {
                                        return Err(unsupported(
                                            "this hole type in a string pattern",
                                            *span,
                                        ))
                                    }
                                };
                                scope.insert(name.clone(), val);
                            }
                        }
                    }
                    Ok(pos == s.len())
                }
                other => Err(unsupported(
                    &format!(
                        "matching a string pattern against a value that isn't a string (got {})",
                        other.jet_show()
                    ),
                    *span,
                )),
            },
            // D-BINPAT1 (card #506): a `b"…"` binary pattern — the byte-mode
            // sibling of `StrMatch`. Reads the `[U8]` subject with a sequential
            // MSB-first bit cursor; each fixed-width hole binds an unsigned
            // integer, a `{rest:...}` hole binds the remaining bytes as `[U8]`.
            // Any short read, byte mismatch, or misaligned literal/rest is a
            // non-match (not an error — E0148 requires an `else`). Tier-0 must
            // read bit-for-bit the same as the AOT `bin_match_scan_closure_ex`
            // (R12 parity).
            Pattern::BinMatch { parts, span } => {
                let bytes: Vec<u8> = match v {
                    CtValue::Bytes(b) => b.clone(),
                    CtValue::List(items) => {
                        let mut out = Vec::with_capacity(items.len());
                        for it in items {
                            match it {
                                CtValue::Int(n) if (0..=255).contains(n) => out.push(*n as u8),
                                _ => {
                                    return Err(unsupported(
                                        "matching a binary pattern against a list whose elements aren't bytes",
                                        *span,
                                    ))
                                }
                            }
                        }
                        out
                    }
                    other => {
                        return Err(unsupported(
                            &format!(
                                "matching a binary pattern against a value that isn't `[U8]` (got {})",
                                other.jet_show()
                            ),
                            *span,
                        ))
                    }
                };
                let total_bits = bytes.len() * 8;
                let mut bit_pos = 0usize;
                let mut pending: Vec<(String, CtValue)> = Vec::new();
                for part in parts {
                    match part {
                        crate::AST::BinMatchPart::Lit(lit) => {
                            if bit_pos % 8 != 0 {
                                return Ok(false);
                            }
                            let start = bit_pos / 8;
                            if start + lit.len() > bytes.len()
                                || &bytes[start..start + lit.len()] != lit.as_slice()
                            {
                                return Ok(false);
                            }
                            bit_pos += lit.len() * 8;
                        }
                        crate::AST::BinMatchPart::Hole { name, spec, .. } => match spec {
                            crate::AST::BinSpec::Rest => {
                                if bit_pos % 8 != 0 {
                                    return Ok(false);
                                }
                                let start = bit_pos / 8;
                                let rest: Vec<CtValue> =
                                    bytes[start..].iter().map(|b| CtValue::Int(*b as i64)).collect();
                                pending.push((name.clone(), CtValue::List(rest)));
                                bit_pos = total_bits;
                            }
                            crate::AST::BinSpec::Bits { width, endian } => {
                                let w = *width as usize;
                                if bit_pos + w > total_bits {
                                    return Ok(false);
                                }
                                let mut val: u64 = 0;
                                for k in 0..w {
                                    let p = bit_pos + k;
                                    let bit = (bytes[p / 8] >> (7 - (p % 8))) & 1;
                                    val = (val << 1) | bit as u64;
                                }
                                bit_pos += w;
                                if matches!(endian, crate::AST::BinEndian::Little) {
                                    // width is a multiple of 8 (sema guarantees).
                                    let nbytes = w / 8;
                                    let mut swapped: u64 = 0;
                                    for i in 0..nbytes {
                                        let b = (val >> (8 * i)) & 0xff;
                                        swapped |= b << (8 * (nbytes - 1 - i));
                                    }
                                    val = swapped;
                                }
                                pending.push((name.clone(), CtValue::Int(val as i64)));
                            }
                        },
                    }
                }
                // A pattern with no trailing rest must consume the whole subject.
                let ends_in_rest = matches!(
                    parts.last(),
                    Some(crate::AST::BinMatchPart::Hole {
                        spec: crate::AST::BinSpec::Rest,
                        ..
                    })
                );
                if !ends_in_rest && bit_pos != total_bits {
                    return Ok(false);
                }
                for (name, val) in pending {
                    scope.insert(name, val);
                }
                Ok(true)
            }
        }
    }
}

// --- value helpers --------------------------------------------------------

/// c139/HOF bare-fn-value: a top-level function name used as a value (bound
/// to a variable, passed to a higher-order method, called through a stored
/// variable) has no dedicated `CtValue` — wrap it in a synthetic
/// single-expression closure that just forwards to the real function by
/// name. Every existing closure-calling path (`call_closure`,
/// `.map`/`.filter`/`.each`/…, `Expr::CallValue`) then picks it up unchanged,
/// with no new `CtValue` variant needed.
fn fn_value(name: &str, func: &Func, span: Span) -> CtValue {
    let params: Vec<crate::AST::LambdaParam> = func
        .params
        .iter()
        .map(|p| crate::AST::LambdaParam {
            name: p.name.clone(),
            name_span: p.name_span,
            ty: None,
            ty_span: None,
        })
        .collect();
    let args: Vec<crate::AST::CallArg> = func
        .params
        .iter()
        .map(|p| crate::AST::CallArg {
            convention: crate::AST::AccessConvention::Read,
            expr: Expr::Ident(p.name.clone(), p.name_span),
            span: p.name_span,
            flags: crate::AST::CallArgFlags::default(),
            label: None,
            spread: false,
        })
        .collect();
    let body = crate::AST::LambdaBody::Expr(Box::new(Expr::Call(crate::AST::Call {
        name: name.to_string(),
        name_span: span,
        args,
        range_checked: false,
    })));
    let lambda = crate::AST::Lambda {
        take_names: Vec::new(),
        params,
        body,
        span,
        meta: crate::AST::LambdaMeta::default(),
    };
    CtValue::Closure(std::sync::Arc::new(crate::AST::ClosureData {
        lambda,
        captured: HashMap::new(),
    }))
}

fn list_get(xs: &[CtValue], i: i64, span: Span) -> Result<CtValue, Diagnostic> {
    if i < 0 || i as usize >= xs.len() {
        return Err(index_oob(xs.len(), i, span));
    }
    Ok(xs[i as usize].clone())
}

fn slice_inclusive(xs: &[CtValue], a: i64, b: i64, span: Span) -> Result<Vec<CtValue>, Diagnostic> {
    let n = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= n {
        return Err(slice_oob(n, a, b, span));
    }
    Ok(xs[a as usize..=b as usize].to_vec())
}
