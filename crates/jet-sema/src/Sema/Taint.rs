//! Taint tracking (D-TAINT1, option A; D-TAINT2, option A).
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
//! **D-TAINT2 (option A)**: the kind is named in parens — `#Tainted(Credential)
//! value`. Bare `#Tainted` defaults to `.Input`. The `Credential` kind extends
//! D-TAINT1 with log/print/serialize **credential sinks** (E0722) alongside the
//! existing injection sinks (E0721). Only `Credential` is gated on additional
//! sinks; other kinds (`.Input`/`.PII`/`.Secret`) use the injection sink set.
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

use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Effects::{core_effect, Effect};
use crate::AST::{
    ElseBranch, EnumLitArg, Expr, ForKind, IfStmt, Item, LValue, Lambda, LambdaBody, OrFallback,
    Stmt, StrPart,
};
use std::collections::{HashMap, HashSet};

/// The injection sink effects (D-TAINT1): a tainted value reaching a Core call
/// carrying one of these without a sanitizer is E0721. `Db` (query injection),
/// `Exec` (command injection), `Net` (SSRF / request smuggling).
fn is_sink_effect(e: Effect) -> bool {
    matches!(e, Effect::Db | Effect::Exec | Effect::Net)
}

/// D-TAINT2: the credential log/print/serialize sinks. A `#Tainted(Credential)`
/// value reaching `core.io.print`, `core.io.eprint`, `jet.log.*`, or
/// `core.encoding.*.to_string*` is E0722.
fn is_credential_sink(module: &str, method: &str) -> bool {
    match module {
        "core.io" => matches!(method, "print" | "eprint"),
        "jet.log" | "core.log" => true, // all log methods are credential sinks
        "core.encoding.json" | "core.encoding.csv" | "core.encoding.toml"
        | "core.encoding.yaml" | "core.encoding.cbor" | "core.encoding.xml" => {
            matches!(method, "to_string" | "to_string_pretty" | "to_bytes" | "to_bytes_canonical")
        }
        _ => false,
    }
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
    /// Locals currently holding a tainted value (any kind), in this function body.
    tainted: HashSet<String>,
    /// D-TAINT2: locals whose taint kind is `Credential`. A strict subset of
    /// `tainted` — every credential-tainted local is also in the general set.
    credential_tainted: HashSet<String>,
    diags: Vec<Diagnostic>,
}

impl<'a> TaintCtx<'a> {
    fn new(sanitizers: &'a HashSet<String>, core_imports: &'a HashMap<String, String>) -> Self {
        TaintCtx {
            sanitizers,
            core_imports,
            tainted: HashSet::new(),
            credential_tainted: HashSet::new(),
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

    /// D-TAINT2: true when the Core call `receiver.method(…)` is a credential
    /// sink (print/log/serialize). Used alongside `is_credential_tainted` to
    /// emit E0722.
    fn call_is_credential_sink(&self, receiver: &Expr, method: &str) -> bool {
        let Expr::Ident(alias, _) = receiver else {
            return false;
        };
        let Some(module) = self.core_imports.get(alias) else {
            return false;
        };
        is_credential_sink(module, method)
    }

    /// D-TAINT2: true when `e` evaluates to a `#Tainted(Credential)` value.
    fn is_credential_tainted(&self, e: &Expr) -> bool {
        match e {
            Expr::Tainted(_, kind, _) => {
                kind.as_deref() == Some(crate::Syntax::KW_CREDENTIAL)
            }
            Expr::Ident(name, _) => self.credential_tainted.contains(name),
            // Derivations — credential taint flows through like general taint.
            Expr::Binary(_, l, r, _) => {
                self.is_credential_tainted(l) || self.is_credential_tainted(r)
            }
            Expr::Unary(_, inner, _)
            | Expr::Deref(inner, _)
            | Expr::Field(inner, _, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _)
            | Expr::Try(inner, _, _) => self.is_credential_tainted(inner),
            Expr::Str(parts, _) => parts.iter().any(|p| match p {
                StrPart::Interp(e, _) => self.is_credential_tainted(e),
                _ => false,
            }),
            Expr::StructLit { fields, .. } => {
                fields.iter().any(|(_, _, f)| self.is_credential_tainted(f))
            }
            Expr::TypedLit { body, .. } => {
                let mut hit = false;
                body.for_each_expr(|f| {
                    if self.is_credential_tainted(f) {
                        hit = true;
                    }
                });
                hit
            }
            Expr::MethodCall { receiver, args, recv_type, method, .. } => {
                if let Some(ty) = recv_type {
                    if self.sanitizers.contains(&format!("{ty}::{method}")) {
                        return false;
                    }
                }
                self.is_credential_tainted(receiver)
                    || args.iter().any(|a| self.is_credential_tainted(&a.expr))
            }
            _ => false,
        }
    }

    /// True when `e` evaluates to a tainted value (any kind), given the current
    /// tainted-local set. Taint flows out of `#Tainted`, out of tainted locals,
    /// and through any derivation (arithmetic, field/index read, interpolation,
    /// optional/result wrappers, …). A `#Sanitizer fn` call is the cut point.
    fn is_tainted(&self, e: &Expr) -> bool {
        match e {
            // The source of taint (any kind), and a tainted local reference.
            Expr::Tainted(_, _, _) => true,
            Expr::Ident(name, _) => self.tainted.contains(name),

            // A free-function call's result is untainted: a `#Sanitizer fn`
            // clears taint by contract, and an ordinary call doesn't propagate
            // taint across the boundary in this intraprocedural model (that is
            // the deferred D-IFC1 analysis). Either way the result is clean.
            Expr::Call(_) => false,
            Expr::MethodCall {
                receiver,
                method,
                recv_type,
                args,
                ..
            } => {
                // `value.method(…)` where the method is a `#Sanitizer fn` clears
                // taint. Otherwise taint flows from the receiver (a tainted
                // string's `.trim()` is still tainted) and from any argument.
                if let Some(ty) = recv_type {
                    if self.sanitizers.contains(&format!("{ty}::{method}")) {
                        return false;
                    }
                }
                self.is_tainted(receiver) || args.iter().any(|a| self.is_tainted(&a.expr))
            }
            Expr::CallValue { .. } => false,

            // Derivations — taint flows through if any operand is tainted.
            Expr::Binary(_, l, r, _) => self.is_tainted(l) || self.is_tainted(r),
            Expr::CompareChain { operands, .. } => operands.iter().any(|e| self.is_tainted(e)),
            Expr::Unary(_, inner, _)
            | Expr::IncDec { operand: inner, .. }
            | Expr::Deref(inner, _)
            | Expr::RawOf(inner, _)
            | Expr::Copy(inner, _)
            | Expr::Place(inner, _, _)
            | Expr::Field(inner, _, _)
            | Expr::Present(inner, _)
            | Expr::Ok(inner, _)
            | Expr::Err(inner, _)
            | Expr::Try(inner, _, _) => self.is_tainted(inner),
            Expr::OptField { base, .. } => self.is_tainted(base),
            Expr::Index { base, index, .. } => self.is_tainted(base) || self.is_tainted(index),
            Expr::Slice {
                base, start, end, ..
            } => self.is_tainted(base) || self.is_tainted(start) || self.is_tainted(end),
            Expr::ListLit(elems, _) => elems.iter().any(|el| self.is_tainted(el)),
            Expr::MapLit(entries, _) => entries
                .iter()
                .any(|(k, v)| self.is_tainted(k) || self.is_tainted(v)),
            Expr::TupleLit(fields, _, _) => fields.iter().any(|(_, e)| self.is_tainted(e)),
            Expr::StructLit { fields, .. } => fields.iter().any(|(_, _, f)| self.is_tainted(f)),
            Expr::TypedLit { body, .. } => {
                let mut hit = false;
                body.for_each_expr(|f| {
                    if self.is_tainted(f) {
                        hit = true;
                    }
                });
                hit
            },
            Expr::EnumLit { args, .. } => args.iter().any(|a| match a {
                EnumLitArg::Positional(e) => self.is_tainted(e),
                EnumLitArg::Named { expr, .. } => self.is_tainted(expr),
            }),
            // Interpolation: a tainted value spliced into a string taints it.
            Expr::Str(parts, _) => parts.iter().any(|p| match p {
                StrPart::Interp(e, _) => self.is_tainted(e),
                _ => false,
            }),
            Expr::OrFallback {
                value, fallback, ..
            } => {
                self.is_tainted(value)
                    || match fallback {
                        OrFallback::Value(e) => self.is_tainted(e),
                        OrFallback::Return(Some(e), _) => self.is_tainted(e),
                        _ => false,
                    }
            }
            Expr::PatternTest { subject, .. } => self.is_tainted(subject),
            Expr::If {
                then_value,
                else_value,
                ..
            } => self.is_tainted(then_value) || self.is_tainted(else_value),
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
            | Expr::Lambda(_)
            | Expr::UnitLit { .. }
            | Expr::ComptimeSplice { .. }
            // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
            // literal, no nested `Expr` to recurse into.
            | Expr::StrMatchLit(_, _)
            | Expr::BinMatchLit(_, _) => false,
            Expr::Paren(inner, _) => self.is_tainted(inner),
            Expr::Spread(inner, _) => self.is_tainted(inner),
        }
    }

    /// Walk an expression for sink violations:
    /// - E0721: a tainted (any kind) value reaches an injection sink (Db/Exec/Net).
    /// - E0722: a `#Tainted(Credential)` value reaches a log/print/serialize sink.
    /// Recurses into every sub-expression so a nested sink is still checked.
    fn check_expr(&mut self, e: &Expr) {
        match e {
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                args,
                ..
            } => {
                // E0721: injection sinks (any taint kind).
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
                // D-TAINT2 / E0722: credential sinks (print/log/serialize).
                if self.call_is_credential_sink(receiver, method) {
                    for a in args {
                        if self.is_credential_tainted(&a.expr) {
                            let alias = match receiver.as_ref() {
                                Expr::Ident(n, _) => n.clone(),
                                _ => String::new(),
                            };
                            self.diags
                                .push(e0722(&alias, method, *method_span));
                            break;
                        }
                    }
                    // Also check the format string argument itself for credential
                    // interpolation (`print("password: {cred}")` → the string
                    // literal is the first arg and is_credential_tainted catches it).
                }
                self.check_expr(receiver);
                for a in args {
                    self.check_expr(&a.expr);
                }
            }
            Expr::Tainted(inner, _, _) => self.check_expr(inner),
            Expr::Call(c) => {
                // D-TAINT2 / E0722: bare `print`/`eprint` are credential sinks too.
                if matches!(c.name.as_str(), "print" | "eprint") {
                    for a in &c.args {
                        if self.is_credential_tainted(&a.expr) {
                            self.diags
                                .push(e0722("", &c.name, c.name_span));
                            break;
                        }
                    }
                }
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
            Expr::CompareChain { operands, .. } => {
                for e in operands {
                    self.check_expr(e);
                }
            }
            Expr::Unary(_, inner, _)
            | Expr::IncDec { operand: inner, .. }
            | Expr::Deref(inner, _)
            | Expr::RawOf(inner, _)
            | Expr::Copy(inner, _)
            | Expr::Place(inner, _, _)
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
            Expr::Slice {
                base, start, end, ..
            } => {
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
            Expr::TypedLit { body, .. } => {
                body.for_each_expr(|f| self.check_expr(f))
            }
            Expr::EnumLit { args, .. } => args.iter().for_each(|a| match a {
                EnumLitArg::Positional(e) => self.check_expr(e),
                EnumLitArg::Named { expr, .. } => self.check_expr(expr),
            }),
            Expr::Str(parts, _) => parts.iter().for_each(|p| {
                if let StrPart::Interp(e, _) = p {
                    self.check_expr(e);
                }
            }),
            Expr::OrFallback {
                value, fallback, ..
            } => {
                self.check_expr(value);
                match fallback {
                    OrFallback::Value(e) => self.check_expr(e),
                    OrFallback::Return(Some(e), _) => self.check_expr(e),
                    _ => {}
                }
            }
            Expr::PatternTest { subject, .. } => self.check_expr(subject),
            Expr::If {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                ..
            } => {
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
            | Expr::Todo { .. }
            | Expr::UnitLit { .. }
            | Expr::ComptimeSplice { .. }
            // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
            // literal, no nested `Expr` to recurse into.
            | Expr::StrMatchLit(_, _)
            | Expr::BinMatchLit(_, _) => {}
            Expr::Paren(inner, _) => self.check_expr(inner),
            Expr::Spread(inner, _) => self.check_expr(inner),
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
            Stmt::Expr(e) | Stmt::Yield(e, _) => self.check_expr(e),
            Stmt::Val(b) => {
                self.check_expr(&b.init);
                // A binding takes on its initializer's taint. A destructuring
                // pattern spreads taint to every bound name (conservative).
                let init_tainted = self.is_tainted(&b.init);
                let init_cred = self.is_credential_tainted(&b.init);
                if let Some(pat) = &b.pattern {
                    for name in pattern_names(pat) {
                        self.set_taint(name.clone(), init_tainted);
                        self.set_credential_taint(name, init_cred);
                    }
                } else if !b.name.is_empty() {
                    self.set_taint(b.name.clone(), init_tainted);
                    self.set_credential_taint(b.name.clone(), init_cred);
                }
            }
            Stmt::Assign {
                target, op, value, ..
            } => {
                self.check_expr(value);
                if let LValue::Local { name, .. } = target {
                    // A plain `x = v` resets taint to v's; a compound `x += v`
                    // keeps x's taint if either side is tainted.
                    let v_tainted = self.is_tainted(value);
                    let v_cred = self.is_credential_tainted(value);
                    let new = if op.is_some() {
                        self.tainted.contains(name) || v_tainted
                    } else {
                        v_tainted
                    };
                    let new_cred = if op.is_some() {
                        self.credential_tainted.contains(name) || v_cred
                    } else {
                        v_cred
                    };
                    self.set_taint(name.clone(), new);
                    self.set_credential_taint(name.clone(), new_cred);
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
            Stmt::For {
                kind,
                body,
                var,
                var2,
                ..
            } => {
                let (coll_tainted, coll_cred) = match kind {
                    ForKind::Range { start, end, step, exclusive: _ } => {
                        self.check_expr(start);
                        self.check_expr(end);
                        if let Some(s) = step {
                            self.check_expr(s);
                        }
                        (false, false)
                    }
                    ForKind::In { collection, step } => {
                        self.check_expr(collection);
                        if let Some(step) = step { self.check_expr(step); }
                        (self.is_tainted(collection), self.is_credential_tainted(collection))
                    }
                };
                self.set_taint(var.clone(), coll_tainted);
                self.set_credential_taint(var.clone(), coll_cred);
                if let Some((v2, _)) = var2 {
                    self.set_taint(v2.clone(), coll_tainted);
                    self.set_credential_taint(v2.clone(), coll_cred);
                }
                self.check_block(body);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            }
            | Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                self.check_expr(subject);
                for a in arms {
                    self.check_expr(&a.cond);
                    self.check_block(&a.body);
                }
                if let Some(b) = else_body {
                    self.check_block(b);
                }
            }
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.check_expr(&init.init);
                self.check_expr(cond);
                self.check_block(body);
                if let Some(step) = step { self.check_stmt(step); }
            }
            Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Off { body, .. }
            | Stmt::DebugOnly { body, .. }
            | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::ScopeMember { body, .. }
            | Stmt::Live { body, .. } => self.check_block(body),
            // D-CTMARKER1: comptime block erases; walk body conservatively.
            Stmt::ComptimeBlock { body, .. } => self.check_block(body),
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
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
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {
            }
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

    /// D-TAINT2: set or clear a local's credential taint. Mirrors `set_taint`.
    fn set_credential_taint(&mut self, name: String, tainted: bool) {
        if tainted {
            self.tainted.insert(name.clone()); // credential → also general taint
            self.credential_tainted.insert(name);
        } else {
            self.credential_tainted.remove(&name);
        }
    }
}

/// Names bound by a destructuring pattern (S74), flattened.
fn pattern_names(pat: &crate::AST::BindPattern) -> Vec<String> {
    use crate::AST::BindPattern;
    match pat {
        BindPattern::Struct { fields, .. } => {
            fields.iter().map(|b| b.local_name().to_string()).collect()
        }
        BindPattern::List { elems, .. } | BindPattern::Tuple { elems, .. } => {
            elems.iter().map(|b| b.name.clone()).collect()
        }
    }
}

/// D-TAINT1/TAINT2: run the taint pass over one function body. Returns all
/// taint diagnostics found (E0721 injection sinks and E0722 credential sinks).
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

/// E0722 (D-TAINT2): a `#Tainted(Credential)` value reaches a log/print/serialize
/// sink. Credentials must never appear in log files, stdout, or serialized output.
pub fn e0722(alias: &str, method: &str, span: Span) -> Diagnostic {
    let api = if alias.is_empty() {
        method.to_string()
    } else {
        format!("{alias}.{method}")
    };
    Diagnostic::error(
        "E0722",
        format!(
            "a credential value reaches `{}`, a logging sink",
            api
        ),
        format!(
            "`{}` writes to a log, terminal, or serialized output — a `#{}({})` value there leaks the secret",
            api,
            crate::Syntax::KW_TAINTED,
            crate::Syntax::KW_CREDENTIAL,
        ),
        "log a non-secret field, or strip the credential with a `#Sanitizer fn` first".to_string(),
        Some(span),
    )
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
