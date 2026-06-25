//! Taint tracking (D-TAINT1, option A).
//!
//! An *untrusted* value carries a `#Tainted` value-fact tag (D-QUAL1), attached
//! inline at its source. Taint **spreads** along intraprocedural dataflow:
//! anything derived from a tainted value (binding, reassignment, interpolation,
//! field/index read, arithmetic, return) is tainted. A `#Sanitizer fn` is the
//! one blessed way to strip it — its return value is untainted by contract, even
//! when its inputs were tainted (the audited cleaning step). A tainted value
//! reaching a **sink effect** (`Db`/`Exec`/`Net` — a security-sensitive Core
//! operation) without first passing through a sanitizer is **E0721**.
//!
//! This rides D-EFF1's effect classification (a sink is just a Core call whose
//! effect is in the sink set) and is **fully erased in codegen** (I3): the tag
//! is a compile-time proof with no runtime value.
//!
//! The model is intraprocedural plus the explicit `#Sanitizer fn` contract:
//! taint is introduced only by `#Tainted` and cleared only by a sanitizer call.
//! Taint does not silently cross an ordinary call boundary — that would be the
//! research-grade information-flow-control analysis explicitly deferred to D-IFC1
//! (D-TAINT1 option B). What the card lists as propagation (assignment,
//! interpolation, field store, return, arithmetic) is exactly the dataflow this
//! pass tracks.

use crate::AST::{
    ElseBranch, EnumLitArg, Expr, ForKind, IfStmt, Item, LValue, Lambda, LambdaBody, OrFallback,
    Stmt, StrPart,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Effects::{core_effect, Effect};
use std::collections::{HashMap, HashSet};

/// The sink effects (D-TAINT1): a tainted value reaching a Core call carrying one
/// of these without a sanitizer is E0721. `Db` (query injection), `Exec`
/// (command injection), `Net` (SSRF / request smuggling) are the injection-class
/// sinks the card names (`#db`/`#exec`/`#net`).
fn is_sink_effect(e: Effect) -> bool {
    matches!(e, Effect::Db | Effect::Exec | Effect::Net)
}

/// Per-function taint analyzer. Carries the program-level facts (which functions
/// are `#Sanitizer`, how Core aliases resolve to modules) and the running set of
/// tainted locals while it walks one function body.
struct TaintCtx<'a> {
    /// Names of `#Sanitizer fn` functions (bare names + `Type::method` keys). A
    /// call to one yields an untainted value regardless of argument taint.
    sanitizers: &'a HashSet<String>,
    /// Core import aliases in scope for the module owning this body
    /// (alias → resolved module path, e.g. `db` → `jet.db`). Used to classify a
    /// `MethodCall` on a Core alias as a sink.
    core_imports: &'a HashMap<String, String>,
    /// Locals currently holding a tainted value, in this function body.
    tainted: HashSet<String>,
    diags: Vec<Diagnostic>,
}

impl<'a> TaintCtx<'a> {
    fn new(
        sanitizers: &'a HashSet<String>,
        core_imports: &'a HashMap<String, String>,
    ) -> Self {
        TaintCtx {
            sanitizers,
            core_imports,
            tainted: HashSet::new(),
            diags: Vec::new(),
        }
    }

    /// Resolve a `MethodCall` on a bare Core alias to its sink effect, if any.
    /// Returns `Some(effect)` when `receiver` is `Ident(alias)`, the alias is a
    /// Core import, and the resolved Core call carries a sink effect.
    fn call_sink_effect(&self, receiver: &Expr, method: &str) -> Option<Effect> {
        let Expr::Ident(alias, _) = receiver else {
            return None;
        };
        let module = self.core_imports.get(alias)?;
        let e = core_effect(module, method)?;
        if is_sink_effect(e) {
            Some(e)
        } else {
            None
        }
    }

    /// True when `e` evaluates to a tainted value, given the current tainted-local
    /// set. Taint flows out of `#Tainted`, out of tainted locals, and through any
    /// derivation (arithmetic, field/index read, interpolation, optional/result
    /// wrappers, …). A `#Sanitizer fn` call is the cut point: its result is clean.
    fn is_tainted(&self, e: &Expr) -> bool {
        match e {
            // The source of taint, and a tainted local reference.
            Expr::Tainted(_, _) => true,
            Expr::Ident(name, _) => self.tainted.contains(name),

            // A free-function call's result is untainted: a `#Sanitizer fn`
            // clears taint by contract, and an ordinary call doesn't propagate
            // taint across the boundary in this intraprocedural model (that is
            // the deferred D-IFC1 analysis). Either way the result is clean.
            Expr::Call(_) => false,
            Expr::MethodCall { receiver, method, recv_type, args, .. } => {
                // `value.method(…)` where the method is a `#Sanitizer fn` clears
                // taint. Otherwise taint flows from the receiver (a tainted
                // string's `.trim()` is still tainted) and from any argument.
                if let Some(ty) = recv_type {
                    if self.sanitizers.contains(&format!("{ty}::{method}")) {
                        return false;
                    }
                }
                self.is_tainted(receiver)
                    || args.iter().any(|a| self.is_tainted(&a.expr))
            }
            Expr::CallValue { .. } => false,

            // Derivations — taint flows through if any operand is tainted.
            Expr::Binary(_, l, r, _) => self.is_tainted(l) || self.is_tainted(r),
            Expr::Unary(_, inner, _)
            | Expr::Deref(inner, _)
            | Expr::RawOf(inner, _)
            | Expr::Field(inner, _, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _)
            | Expr::Try(inner, _, _) => self.is_tainted(inner),
            Expr::OptField { base, .. } => self.is_tainted(base),
            Expr::Index { base, index, .. } => {
                self.is_tainted(base) || self.is_tainted(index)
            }
            Expr::Slice { base, start, end, .. } => {
                self.is_tainted(base) || self.is_tainted(start) || self.is_tainted(end)
            }
            Expr::ListLit(elems, _) => elems.iter().any(|el| self.is_tainted(el)),
            Expr::MapLit(entries, _) => entries
                .iter()
                .any(|(k, v)| self.is_tainted(k) || self.is_tainted(v)),
            Expr::TupleLit(fields, _, _) => fields.iter().any(|(_, e)| self.is_tainted(e)),
            Expr::StructLit { fields, .. } => fields.iter().any(|(_, _, f)| self.is_tainted(f)),
            Expr::EnumLit { args, .. } => args.iter().any(|a| match a {
                EnumLitArg::Positional(e) => self.is_tainted(e),
                EnumLitArg::Named { expr, .. } => self.is_tainted(expr),
            }),
            // Interpolation: a tainted value spliced into a string taints it.
            Expr::Str(parts, _) => parts.iter().any(|p| match p {
                StrPart::Interp(e) => self.is_tainted(e),
                _ => false,
            }),
            Expr::OrFallback { value, fallback, .. } => {
                self.is_tainted(value)
                    || match fallback {
                        OrFallback::Value(e) => self.is_tainted(e),
                        OrFallback::Return(Some(e), _) => self.is_tainted(e),
                        _ => false,
                    }
            }
            Expr::PatternTest { subject, .. } => self.is_tainted(subject),
            Expr::If { then_value, else_value, .. } => {
                self.is_tainted(then_value) || self.is_tainted(else_value)
            }
            Expr::FanOut { items, .. } => items.iter().any(|e| self.is_tainted(e)),
            Expr::PtrFromAddr { addr, .. } => self.is_tainted(addr),

            // Literals, holes, lambdas, absent — never tainted on their own.
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Char(..)
            | Expr::Absent(_)
            | Expr::ReduceMarker(_, _)
            | Expr::Todo { .. }
            | Expr::Lambda(_) => false,
        }
    }

    /// Walk an expression for sink violations: a Core sink call whose argument is
    /// tainted is E0721. Recurses into every sub-expression so a sink nested in a
    /// larger expression is still checked.
    fn check_expr(&mut self, e: &Expr) {
        match e {
            Expr::MethodCall { receiver, method, method_span, args, .. } => {
                if let Some(effect) = self.call_sink_effect(receiver, method) {
                    for a in args {
                        if self.is_tainted(&a.expr) {
                            let alias = match receiver.as_ref() {
                                Expr::Ident(n, _) => n.clone(),
                                _ => String::new(),
                            };
                            self.diags.push(e0721(&alias, method, effect, *method_span));
                            break;
                        }
                    }
                }
                self.check_expr(receiver);
                for a in args {
                    self.check_expr(&a.expr);
                }
            }
            Expr::Tainted(inner, _) => self.check_expr(inner),
            Expr::Call(c) => {
                for a in &c.args {
                    self.check_expr(&a.expr);
                }
            }
            Expr::CallValue { callee, args, .. } => {
                self.check_expr(callee);
                for a in args {
                    self.check_expr(&a.expr);
                }
            }
            Expr::Binary(_, l, r, _) => {
                self.check_expr(l);
                self.check_expr(r);
            }
            Expr::Unary(_, inner, _)
            | Expr::Deref(inner, _)
            | Expr::RawOf(inner, _)
            | Expr::Field(inner, _, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _)
            | Expr::Try(inner, _, _) => self.check_expr(inner),
            Expr::OptField { base, .. } => self.check_expr(base),
            Expr::Index { base, index, .. } => {
                self.check_expr(base);
                self.check_expr(index);
            }
            Expr::Slice { base, start, end, .. } => {
                self.check_expr(base);
                self.check_expr(start);
                self.check_expr(end);
            }
            Expr::ListLit(elems, _) => elems.iter().for_each(|el| self.check_expr(el)),
            Expr::MapLit(entries, _) => entries.iter().for_each(|(k, v)| {
                self.check_expr(k);
                self.check_expr(v);
            }),
            Expr::TupleLit(fields, _, _) => fields.iter().for_each(|(_, e)| self.check_expr(e)),
            Expr::StructLit { fields, .. } => {
                fields.iter().for_each(|(_, _, f)| self.check_expr(f))
            }
            Expr::EnumLit { args, .. } => args.iter().for_each(|a| match a {
                EnumLitArg::Positional(e) => self.check_expr(e),
                EnumLitArg::Named { expr, .. } => self.check_expr(expr),
            }),
            Expr::Str(parts, _) => parts.iter().for_each(|p| {
                if let StrPart::Interp(e) = p {
                    self.check_expr(e);
                }
            }),
            Expr::OrFallback { value, fallback, .. } => {
                self.check_expr(value);
                match fallback {
                    OrFallback::Value(e) => self.check_expr(e),
                    OrFallback::Return(Some(e), _) => self.check_expr(e),
                    _ => {}
                }
            }
            Expr::PatternTest { subject, .. } => self.check_expr(subject),
            Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
                self.check_expr(cond);
                self.check_block(then_body);
                self.check_expr(then_value);
                self.check_block(else_body);
                self.check_expr(else_value);
            }
            Expr::FanOut { callee, items, .. } => {
                self.check_expr(callee);
                items.iter().for_each(|e| self.check_expr(e));
            }
            Expr::PtrFromAddr { addr, .. } => self.check_expr(addr),
            Expr::Lambda(l) => self.check_lambda(l),
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Char(..)
            | Expr::Ident(..)
            | Expr::Absent(_)
            | Expr::ReduceMarker(_, _)
            | Expr::Todo { .. } => {}
        }
    }

    fn check_lambda(&mut self, l: &Lambda) {
        match &l.body {
            LambdaBody::Expr(e) => self.check_expr(e),
            LambdaBody::Block(b) => self.check_block(b),
        }
    }

    fn check_block(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.check_stmt(s);
        }
    }

    fn check_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) => self.check_expr(e),
            Stmt::Val(b) => {
                self.check_expr(&b.init);
                // A binding takes on its initializer's taint. A destructuring
                // pattern spreads taint to every bound name (conservative).
                let init_tainted = self.is_tainted(&b.init);
                if let Some(pat) = &b.pattern {
                    for name in pattern_names(pat) {
                        self.set_taint(name, init_tainted);
                    }
                } else if !b.name.is_empty() {
                    self.set_taint(b.name.clone(), init_tainted);
                }
            }
            Stmt::Assign { target, op, value, .. } => {
                self.check_expr(value);
                if let LValue::Local { name, .. } = target {
                    // A plain `x = v` resets taint to v's; a compound `x += v`
                    // keeps x's taint if either side is tainted.
                    let v_tainted = self.is_tainted(value);
                    let new = if op.is_some() {
                        self.tainted.contains(name) || v_tainted
                    } else {
                        v_tainted
                    };
                    self.set_taint(name.clone(), new);
                } else {
                    // Field/index assign targets are also walked for nested sinks.
                    if let LValue::Index { base, index, .. } = target {
                        self.check_expr(base);
                        self.check_expr(index);
                    }
                    if let LValue::Field { base, .. } = target {
                        self.check_expr(base);
                    }
                }
            }
            Stmt::Return(Some(e), _) => self.check_expr(e),
            Stmt::Return(None, _) => {}
            Stmt::If(ifs) => self.check_if(ifs),
            Stmt::While { cond, body, .. } => {
                self.check_expr(cond);
                self.check_block(body);
            }
            Stmt::For { kind, body, var, var2, .. } => {
                let coll_tainted = match kind {
                    ForKind::Range { start, end, step } => {
                        self.check_expr(start);
                        self.check_expr(end);
                        if let Some(s) = step {
                            self.check_expr(s);
                        }
                        false
                    }
                    ForKind::In { collection } => {
                        self.check_expr(collection);
                        // Iterating a tainted collection yields tainted elements.
                        self.is_tainted(collection)
                    }
                };
                self.set_taint(var.clone(), coll_tainted);
                if let Some((v2, _)) = var2 {
                    self.set_taint(v2.clone(), coll_tainted);
                }
                self.check_block(body);
            }
            Stmt::Switch { subject, arms, else_body, .. } => {
                self.check_expr(subject);
                for a in arms {
                    self.check_expr(&a.cond);
                    self.check_block(&a.body);
                }
                if let Some(b) = else_body {
                    self.check_block(b);
                }
            }
            Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::Live { body, .. } => self.check_block(body),
            Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
                self.check_expr(cond);
                self.check_block(then_body);
                if let Some(b) = else_body {
                    self.check_block(b);
                }
            }
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields {
                    self.check_expr(e);
                }
                self.check_block(body);
            }
            Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(..)
            | Stmt::ContinueLabel(..) => {}
        }
    }

    fn check_if(&mut self, ifs: &IfStmt) {
        self.check_expr(&ifs.cond);
        self.check_block(&ifs.then_body);
        if let Some(e) = &ifs.else_branch {
            self.check_else(e);
        }
    }

    fn check_else(&mut self, e: &ElseBranch) {
        match e {
            ElseBranch::Else(stmts) => self.check_block(stmts),
            ElseBranch::ElseIf(ifs) => self.check_if(ifs),
        }
    }

    /// Set or clear a local's taint. A `false` value removes it from the set so a
    /// reassignment to a clean value un-taints the binding.
    fn set_taint(&mut self, name: String, tainted: bool) {
        if tainted {
            self.tainted.insert(name);
        } else {
            self.tainted.remove(&name);
        }
    }
}

/// Names bound by a destructuring pattern (S74), flattened.
fn pattern_names(pat: &crate::AST::BindPattern) -> Vec<String> {
    use crate::AST::BindPattern;
    match pat {
        BindPattern::Struct { fields, .. } => fields.iter().map(|b| b.name.clone()).collect(),
        BindPattern::List { elems, .. } | BindPattern::Tuple { elems, .. } => {
            elems.iter().map(|b| b.name.clone()).collect()
        }
    }
}

/// D-TAINT1: run the taint pass over one function body. `params` taints any
/// parameter declared `#Tainted` (reserved — params carry no tag today, so this
/// starts empty), `core_imports` resolves Core aliases, `sanitizers` is the set
/// of `#Sanitizer` function/method keys. Returns the E0721 diagnostics found.
pub fn check_func_taint(
    body: &[Stmt],
    sanitizers: &HashSet<String>,
    core_imports: &HashMap<String, String>,
) -> Vec<Diagnostic> {
    let mut ctx = TaintCtx::new(sanitizers, core_imports);
    ctx.check_block(body);
    ctx.diags
}

/// Collect the set of `#Sanitizer fn` keys across a program's items: bare names
/// for top-level functions, `Type::method` for methods. A call to one of these
/// strips taint from its result.
pub fn collect_sanitizers(items: &[Item], out: &mut HashSet<String>) {
    for item in items {
        match item {
            Item::Func(f) if f.is_sanitizer => {
                out.insert(f.name.clone());
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    if m.is_sanitizer {
                        out.insert(format!("{}::{}", i.type_name, m.name));
                    }
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    if m.is_sanitizer {
                        out.insert(format!("{}::{}", s.name, m.name));
                    }
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        if m.is_sanitizer {
                            out.insert(format!("{}::{}", s.name, m.name));
                        }
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    if m.is_sanitizer {
                        out.insert(format!("{}::{}", e.name, m.name));
                    }
                }
            }
            _ => {}
        }
    }
}

/// E0721 (D-TAINT1): a tainted (untrusted) value reaches a sink effect without
/// passing through a `#Sanitizer fn`. Names the sink and offers the sanitizer
/// fix-it — the one blessed way to clear taint before a security-sensitive call.
pub fn e0721(alias: &str, method: &str, effect: Effect, span: Span) -> Diagnostic {
    let api = if alias.is_empty() {
        method.to_string()
    } else {
        format!("{alias}.{method}")
    };
    let kind = match effect {
        Effect::Db => "a database query",
        Effect::Exec => "a subprocess command",
        Effect::Net => "a network request",
        _ => "a sink",
    };
    Diagnostic::error(
        "E0721",
        format!("untrusted (`#{}`) data reaches `{}` without being sanitized", crate::Syntax::KW_TAINTED, api),
        format!(
            "`{}` runs {} — a `{}` sink; a `#{}` value used there unchecked is the classic injection bug",
            api, kind, effect.name(), crate::Syntax::KW_TAINTED
        ),
        format!(
            "pass the value through a `#{} fn` first — its result is trusted, so it may reach the sink",
            crate::Syntax::KW_SANITIZER
        ),
        Some(span),
    )
}
