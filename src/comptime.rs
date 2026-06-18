//! M9.5 — Comptime v1 (CTFE). A tree-walking interpreter over the typed
//! AST that evaluates a pure, deterministic Jet subset at compile time and
//! bakes the answer into the binary. See docs/plans/m095-comptime.md.
//!
//! One law (S26): comptime computes *values* only — it never creates,
//! parameterizes, or selects a type, and never affects dispatch.
//!
//! Diagnostics: E0951 impurity (with call path) · E0952 fuel exhausted ·
//! E0953 comptime panic (user message verbatim, overflow, divide-by-zero) ·
//! E0955 embed_file errors · E0956 construct not yet supported at comptime.
//!
//! Semantics are bit-for-bit identical to the compiled runtime (the
//! differential battery in tests/comptime_diff.rs is the enforcement):
//! i64 `Int`, IEEE f64 `Float` (S21 display via `{:?}`), char-counted
//! `String` (S41), and `BTreeMap` ordering (S38).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::ast::{BinOp, BindPattern, EnumLitArg, Expr, Func, Stmt, StrPart, Type, UnOp};
use crate::diag::{Diagnostic, Span};

/// Default step budget per binding (S26 rule 3). Compiler-internal, not a
/// user knob (philosophy: minimal configuration).
const FUEL_BUDGET: u64 = 10_000_000;

/// Step budget for a whole-program `jet dev` interpretation (E2-M4). Larger
/// than the per-binding comptime budget because a `main()` does real work,
/// but still finite so a runaway loop surfaces as E2202 instead of hanging
/// the watch loop. Compiler-internal, not a user knob.
const DEV_FUEL_BUDGET: u64 = 1_000_000_000;

/// Step budget for a single `jet repl` input (D-REPL-FUEL=A). Tighter than
/// `jet dev` because REPL snippets should be short demos; a runaway loop
/// surfaces as E1801 instead of hanging the session. The user can bypass
/// with `:run` (unbounded but still finite — this constant then applies to
/// the `:run`-spawned one-shot compile path, not the interpreter).
pub const REPL_FUEL_BUDGET: u64 = 10_000_000;

/// A fully-evaluated compile-time value. Self-describing: the Jet type is
/// recovered by [`CtValue::jet_type`] and the Rust literal by
/// [`CtValue::serialize`].
#[derive(Clone, Debug, PartialEq)]
pub enum CtValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(String),
    List(Vec<CtValue>),
    Map(BTreeMap<CtKey, CtValue>),
    Struct {
        type_name: String,
        fields: Vec<(String, CtValue)>,
    },
    Enum {
        type_name: String,
        variant: String,
        args: Vec<(Option<String>, CtValue)>,
    },
    Some(Box<CtValue>),
    None(Type),
    Unit,
}

/// Orderable map key (S38: maps are `BTreeMap`, so keys must be `Ord`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CtKey {
    Int(i64),
    Str(String),
    Bool(bool),
    Char(char),
}

impl CtKey {
    fn from_value(v: CtValue) -> Option<CtKey> {
        match v {
            CtValue::Int(n) => Some(CtKey::Int(n)),
            CtValue::Str(s) => Some(CtKey::Str(s)),
            CtValue::Bool(b) => Some(CtKey::Bool(b)),
            CtValue::Char(c) => Some(CtKey::Char(c)),
            _ => None,
        }
    }
    fn to_value(&self) -> CtValue {
        match self {
            CtKey::Int(n) => CtValue::Int(*n),
            CtKey::Str(s) => CtValue::Str(s.clone()),
            CtKey::Bool(b) => CtValue::Bool(*b),
            CtKey::Char(c) => CtValue::Char(*c),
        }
    }
    fn jet_type(&self) -> Type {
        match self {
            CtKey::Int(_) => Type::Int,
            CtKey::Str(_) => Type::String,
            CtKey::Bool(_) => Type::Bool,
            CtKey::Char(_) => Type::Char,
        }
    }
    fn jet_show(&self) -> String {
        self.to_value().jet_show()
    }
}

impl CtValue {
    /// The Jet type this value inhabits — used to register the binding so
    /// the rest of the program type-checks references to it.
    pub fn jet_type(&self) -> Type {
        match self {
            CtValue::Int(_) => Type::Int,
            CtValue::Float(_) => Type::Float,
            CtValue::Bool(_) => Type::Bool,
            CtValue::Char(_) => Type::Char,
            CtValue::Str(_) => Type::String,
            CtValue::List(xs) => {
                let inner = xs.first().map(|x| x.jet_type()).unwrap_or(Type::Int);
                Type::List(Box::new(inner))
            }
            CtValue::Map(m) => {
                let (k, v) = m
                    .iter()
                    .next()
                    .map(|(k, v)| (k.jet_type(), v.jet_type()))
                    .unwrap_or((Type::String, Type::Int));
                Type::Map {
                    key: Box::new(k),
                    value: Box::new(v),
                }
            }
            CtValue::Some(inner) => Type::Option(Box::new(inner.jet_type())),
            CtValue::None(t) => Type::Option(Box::new(t.clone())),
            CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => {
                Type::Named(type_name.clone())
            }
            CtValue::Unit => Type::Named(String::new()),
        }
    }

    /// Runtime display, identical to the generated `JetShow` impls (codegen
    /// PRELUDE). This is what string interpolation produces.
    pub fn jet_show(&self) -> String {
        match self {
            CtValue::Int(n) => n.to_string(),
            CtValue::Float(f) => format!("{:?}", f),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => c.to_string(),
            CtValue::Str(s) => s.clone(),
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.jet_show()).collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::Map(m) => {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k.jet_show(), v.jet_show()))
                    .collect();
                format!("[{}]", parts.join(", "))
            }
            CtValue::Some(v) => v.jet_show(),
            CtValue::None(_) => "null".to_string(),
            CtValue::Struct { type_name, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", n, v.jet_show()))
                    .collect();
                format!("{}({})", type_name, parts.join(", "))
            }
            CtValue::Enum { variant, .. } => variant.clone(),
            CtValue::Unit => String::new(),
        }
    }

    /// A Rust expression that reconstructs this value, matching codegen's
    /// `emit_expr` representations exactly (Vec, BTreeMap, Option, owned
    /// String). Inlined at each use site (codegen stays dumb, I3).
    pub fn serialize(&self) -> String {
        match self {
            CtValue::Int(n) => format!("{}i64", n),
            CtValue::Float(f) => format!("{:?}f64", f),
            CtValue::Bool(b) => b.to_string(),
            CtValue::Char(c) => format!("{:?}", c),
            CtValue::Str(s) => format!("{:?}.to_string()", s),
            CtValue::List(xs) => {
                let parts: Vec<String> = xs.iter().map(|x| x.serialize()).collect();
                format!("vec![{}]", parts.join(", "))
            }
            CtValue::Map(m) => {
                if m.is_empty() {
                    "std::collections::BTreeMap::new()".to_string()
                } else {
                    let mut s = String::from("{ let mut _m = std::collections::BTreeMap::new(); ");
                    for (k, v) in m {
                        s.push_str(&format!(
                            "_m.insert(({}), {}); ",
                            k.to_value().serialize(),
                            v.serialize()
                        ));
                    }
                    s.push_str("_m }");
                    s
                }
            }
            CtValue::Some(v) => format!("Some({})", v.serialize()),
            CtValue::None(_) => "None".to_string(),
            CtValue::Struct { type_name, fields } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, v)| format!("{}: {}", mangle(n), v.serialize()))
                    .collect();
                format!("user_{} {{ {} }}", type_name, parts.join(", "))
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } => {
                let prefix = format!("user_{}::{}", type_name, mangle(variant));
                if args.is_empty() {
                    prefix
                } else if args.iter().all(|(label, _)| label.is_none()) {
                    let parts: Vec<String> = args.iter().map(|(_, v)| v.serialize()).collect();
                    format!("{}({})", prefix, parts.join(", "))
                } else {
                    let parts: Vec<String> = args
                        .iter()
                        .filter_map(|(label, v)| {
                            label
                                .as_ref()
                                .map(|name| format!("{}: {}", mangle(name), v.serialize()))
                        })
                        .collect();
                    format!("{} {{ {} }}", prefix, parts.join(", "))
                }
            }
            CtValue::Unit => "()".to_string(),
        }
    }
}

fn mangle(name: &str) -> String {
    if name == "main" {
        "main".to_string()
    } else {
        format!("user_{}", name)
    }
}

/// Control-flow signal threaded through statement execution.
enum Flow {
    Normal,
    Break,
    Continue,
    Return(CtValue),
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

struct Interp<'a> {
    funcs: &'a HashMap<String, &'a Func>,
    base_dir: &'a Path,
    fuel: u64,
    /// `Some` in whole-program dev mode (E2-M4): `print`/`eprint` write here
    /// instead of being rejected. `None` in pure comptime mode (M9.5).
    sink: Option<&'a mut DevSink>,
}

impl<'a> Interp<'a> {
    fn burn(&mut self, span: Span) -> Result<(), Diagnostic> {
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

    fn exec_block(
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
    fn bind_pattern(
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
                            comptime_panic(
                                &format!("this value has no field `{}`", f.name),
                                f.span,
                            )
                        })?;
                    scope.insert(f.name.clone(), v);
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
            BindPattern::Tuple { span, .. } => {
                return Err(comptime_panic(
                    "tuple destructuring isn't supported in comptime yet",
                    *span,
                ));
            }
        }
        Ok(())
    }

    fn exec_stmt(
        &mut self,
        stmt: &Stmt,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
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
            Stmt::While { cond, body, span } => {
                loop {
                    self.burn(*span)?;
                    let c = self.eval(cond, scope)?;
                    if !as_bool(&c, cond.span())? {
                        break;
                    }
                    match self.exec_block(body, scope)? {
                        Flow::Break => break,
                        Flow::Continue | Flow::Normal => {}
                        ret @ Flow::Return(_) => return Ok(ret),
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
                ..
            } => self.exec_for(var, var2.as_ref(), kind, body, *span, scope),
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                // Sema rewrites enum/equality arms into ordinary Bool
                // conditions, so a switch is a first-true-arm chain.
                let _ = self.eval(subject, scope)?;
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
            Stmt::Loop(body, span) => {
                loop {
                    self.burn(*span)?;
                    match self.exec_block(body, scope)? {
                        Flow::Break => break,
                        Flow::Continue | Flow::Normal => {}
                        ret @ Flow::Return(_) => return Ok(ret),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Unsafe { span, .. } => Err(unsupported("an `@unsafe` block", *span)),
        }
    }

    fn exec_if(
        &mut self,
        ifs: &crate::ast::IfStmt,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        let c = self.eval(&ifs.cond, scope)?;
        if as_bool(&c, ifs.cond.span())? {
            return self.exec_block(&ifs.then_body, scope);
        }
        match &ifs.else_branch {
            Some(crate::ast::ElseBranch::ElseIf(inner)) => self.exec_if(inner, scope),
            Some(crate::ast::ElseBranch::Else(body)) => self.exec_block(body, scope),
            None => Ok(Flow::Normal),
        }
    }

    fn exec_for(
        &mut self,
        var: &str,
        var2: Option<&(String, Span)>,
        kind: &crate::ast::ForKind,
        body: &[Stmt],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<Flow, Diagnostic> {
        match kind {
            crate::ast::ForKind::Range { start, end, step } => {
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
                    match self.exec_block(body, scope)? {
                        Flow::Break => break,
                        Flow::Continue | Flow::Normal => {}
                        ret @ Flow::Return(_) => return Ok(ret),
                    }
                    i += stride;
                }
                Ok(Flow::Normal)
            }
            crate::ast::ForKind::In { collection } => {
                let c = self.eval(collection, scope)?;
                match c {
                    CtValue::List(items) => {
                        for item in items {
                            self.burn(span)?;
                            scope.insert(var.to_string(), item);
                            match self.exec_block(body, scope)? {
                                Flow::Break => break,
                                Flow::Continue | Flow::Normal => {}
                                ret @ Flow::Return(_) => return Ok(ret),
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
                            match self.exec_block(body, scope)? {
                                Flow::Break => break,
                                Flow::Continue | Flow::Normal => {}
                                ret @ Flow::Return(_) => return Ok(ret),
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    CtValue::Str(s) => {
                        // `for c in s.chars()` lowers the receiver to the string.
                        for ch in s.chars() {
                            self.burn(span)?;
                            scope.insert(var.to_string(), CtValue::Char(ch));
                            match self.exec_block(body, scope)? {
                                Flow::Break => break,
                                Flow::Continue | Flow::Normal => {}
                                ret @ Flow::Return(_) => return Ok(ret),
                            }
                        }
                        Ok(Flow::Normal)
                    }
                    _ => Err(unsupported("looping over this value", span)),
                }
            }
        }
    }

    fn exec_assign(
        &mut self,
        target: &crate::ast::LValue,
        op: Option<BinOp>,
        value: &Expr,
        op_span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        let rhs = self.eval(value, scope)?;
        match target {
            crate::ast::LValue::Local { name, name_span } => {
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
            crate::ast::LValue::Index {
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
        }
    }

    fn eval(
        &mut self,
        e: &Expr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        self.burn(e.span())?;
        match e {
            Expr::Int(n, _) => Ok(CtValue::Int(*n)),
            Expr::Float(f, _) => Ok(CtValue::Float(*f)),
            Expr::Bool(b, _) => Ok(CtValue::Bool(*b)),
            Expr::Char(c, _) => Ok(CtValue::Char(*c)),
            Expr::Str(parts, _) => {
                let mut s = String::new();
                for part in parts {
                    match part {
                        StrPart::Lit(t) => s.push_str(t),
                        StrPart::Interp(e) => s.push_str(&self.eval(e, scope)?.jet_show()),
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
                .cloned()
                .ok_or_else(|| unsupported(&format!("the name `{}`", name), *span)),
            Expr::Unary(op, inner, span) => {
                let v = self.eval(inner, scope)?;
                match (op, v) {
                    (UnOp::Neg, CtValue::Int(n)) => n
                        .checked_neg()
                        .map(CtValue::Int)
                        .ok_or_else(|| overflow("negate", *span)),
                    (UnOp::Neg, CtValue::Float(f)) => Ok(CtValue::Float(-f)),
                    (UnOp::Not, CtValue::Bool(b)) => Ok(CtValue::Bool(!b)),
                    _ => Err(unsupported("this operation", *span)),
                }
            }
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
                    return Ok(CtValue::Enum {
                        type_name: type_name.clone(),
                        variant: member.clone(),
                        args: Vec::new(),
                    });
                }
                let v = self.eval(inner, scope)?;
                match v {
                    CtValue::Struct { fields, .. } => fields
                        .into_iter()
                        .find(|(name, _)| name == member)
                        .map(|(_, value)| value)
                        .ok_or_else(|| unsupported(&format!("the field `.{}`", member), *span)),
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
            Expr::TupleLit(_, span, _) => Err(unsupported("tuple literals in comptime", *span)),
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
            Expr::Present(inner, _) => Ok(CtValue::Some(Box::new(self.eval(inner, scope)?))),
            Expr::Absent(_) => Ok(CtValue::None(Type::Int)),
            Expr::Call(call) => self.eval_call(&call.name, call.name_span, &call.args, scope),
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                args,
                ..
            } => self.eval_method(receiver, method, *method_span, args, scope),
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
            Expr::FanOut { callee, items, span } => self.eval_fan_out(callee, items, *span, scope),
            other => Err(unsupported_expr(other)),
        }
    }

    /// `f.[a, b, c]` → `[f(a), f(b), f(c)]` (fan-out, ratified in
    /// docs/spec/spec.md (S75 fan-out). Comptime only supports
    /// the named-one-arg-function callee case; sources/type-constructor
    /// callees are jetpack-module-specific sugar handled structurally by the
    /// jetpack module evaluator (src/jetpack/modeval.rs), not here.
    fn eval_fan_out(
        &mut self,
        callee: &Expr,
        items: &[Expr],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let Expr::Ident(name, _) = callee else {
            return Err(unsupported("this fan-out callee", callee.span()));
        };
        let func = self
            .funcs
            .get(name.as_str())
            .copied()
            .ok_or_else(|| unsupported(&format!("calling `{}`", name), span))?;
        if func.params.len() != 1 {
            return Err(unsupported(
                &format!("`{}` (fan-out needs a one-argument function)", name),
                span,
            ));
        }
        let param_name = func.params[0].name.clone();
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let arg = self.eval(item, scope)?;
            let mut frame = HashMap::new();
            frame.insert(param_name.clone(), arg);
            let v = match self.exec_block(&func.body, &mut frame)? {
                Flow::Return(v) => v,
                _ => CtValue::Unit,
            };
            out.push(v);
        }
        Ok(CtValue::List(out))
    }

    fn eval_call(
        &mut self,
        name: &str,
        span: Span,
        args: &[crate::ast::CallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        // Whole-program dev mode (E2-M4): the two IO builtins write to the
        // buffered sink, producing bytes identical to the compiled program
        // (`jet_show()` + `\n`). In pure comptime mode the sink is `None` and
        // these are unreachable (the purity check rejects them first, E0951).
        if name == "print" || name == "eprint" {
            if self.sink.is_some() {
                let text = match args.first() {
                    Some(a) => self.eval(&a.expr, scope)?.jet_show(),
                    None => String::new(),
                };
                let sink = self.sink.as_mut().expect("dev-mode sink");
                if name == "print" {
                    sink.stdout.push_str(&text);
                    sink.stdout.push('\n');
                } else {
                    sink.stderr.push_str(&text);
                    sink.stderr.push('\n');
                }
                return Ok(CtValue::Unit);
            }
            // Pure comptime mode: unreachable in practice, but stay honest.
            return Err(unsupported(&format!("`{}`", name), span));
        }
        // The two sanctioned comptime builtins.
        if name == "embed_file" {
            return self.eval_embed_file(args, span, scope);
        }
        if name == "panic" {
            let msg = match args.first() {
                Some(a) => self.eval(&a.expr, scope)?.jet_show(),
                None => "comptime panic".to_string(),
            };
            return Err(comptime_panic(&msg, span));
        }
        if name == "require" || name == "require_eq" {
            return self.eval_require(name, args, span, scope);
        }
        // A user function: bind params, run the body in a fresh frame.
        let func = self
            .funcs
            .get(name)
            .copied()
            .ok_or_else(|| unsupported(&format!("calling `{}`", name), span))?;
        if func.params.len() != args.len() {
            return Err(unsupported(
                &format!("`{}` (wrong number of arguments)", name),
                span,
            ));
        }
        let mut frame = HashMap::new();
        for (p, a) in func.params.iter().zip(args) {
            let v = self.eval(&a.expr, scope)?;
            frame.insert(p.name.clone(), v);
        }
        match self.exec_block(&func.body, &mut frame)? {
            Flow::Return(v) => Ok(v),
            _ => Ok(CtValue::Unit),
        }
    }

    fn eval_require(
        &mut self,
        name: &str,
        args: &[crate::ast::CallArg],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if name == "require_eq" {
            let a = self.eval(&args[0].expr, scope)?;
            let b = self.eval(&args[1].expr, scope)?;
            if a != b {
                let msg = args
                    .get(2)
                    .map(|a| self.eval(&a.expr, scope))
                    .transpose()?
                    .map(|v| v.jet_show())
                    .unwrap_or_else(|| format!("{} != {}", a.jet_show(), b.jet_show()));
                return Err(comptime_panic(&msg, span));
            }
            return Ok(CtValue::Unit);
        }
        let cond = as_bool(&self.eval(&args[0].expr, scope)?, span)?;
        if !cond {
            let msg = args
                .get(1)
                .map(|a| self.eval(&a.expr, scope))
                .transpose()?
                .map(|v| v.jet_show())
                .unwrap_or_else(|| "a requirement was not met".to_string());
            return Err(comptime_panic(&msg, span));
        }
        Ok(CtValue::Unit)
    }

    fn eval_embed_file(
        &mut self,
        args: &[crate::ast::CallArg],
        span: Span,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let arg = args
            .first()
            .ok_or_else(|| unsupported("embed_file with no path", span))?;
        let path_val = self.eval(&arg.expr, scope)?;
        let CtValue::Str(rel) = path_val else {
            return Err(unsupported("embed_file with a non-text path", span));
        };
        let full = self.base_dir.join(&rel);
        match std::fs::read(&full) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Ok(CtValue::Str(text)),
                Err(_) => Err(Diagnostic::error(
                    "E0955",
                    format!("`embed_file` can't read `{}` as text", rel),
                    "the file isn't valid UTF-8; comptime `embed_file` returns a String in v1"
                        .to_string(),
                    "embed a text file, or wait for the byte-buffer version (M10)".to_string(),
                    Some(span),
                )),
            },
            Err(e) => Err(Diagnostic::error(
                "E0955",
                format!("`embed_file` can't open `{}`", rel),
                format!("{} (looked next to the file doing the embedding)", e),
                "check the path — it is relative to the file's own directory".to_string(),
                Some(span),
            )),
        }
    }

    fn eval_method(
        &mut self,
        receiver: &Expr,
        method: &str,
        span: Span,
        args: &[crate::ast::CallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        // Mutating list/map methods on a named variable write back in place.
        const MUTATING: &[&str] = &[
            "push", "pop", "insert", "remove", "clear", "reverse", "sort",
        ];
        if MUTATING.contains(&method) {
            if let Expr::Ident(bname, _) = receiver {
                let mut container = scope
                    .get(bname)
                    .cloned()
                    .ok_or_else(|| unsupported(&format!("the name `{}`", bname), span))?;
                let mut argv = Vec::new();
                for a in args {
                    argv.push(self.eval(&a.expr, scope)?);
                }
                let ret = apply_mutating(&mut container, method, argv, span)?;
                scope.insert(bname.clone(), container);
                return Ok(ret);
            }
        }
        let recv = self.eval(receiver, scope)?;
        let mut argv = Vec::new();
        for a in args {
            argv.push(self.eval(&a.expr, scope)?);
        }
        apply_method(&recv, method, argv, span)
    }
}

// --- value helpers --------------------------------------------------------

fn as_bool(v: &CtValue, span: Span) -> Result<bool, Diagnostic> {
    match v {
        CtValue::Bool(b) => Ok(*b),
        _ => Err(unsupported("a non-Bool used as a condition", span)),
    }
}

fn as_int(v: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    match v {
        CtValue::Int(n) => Ok(*n),
        _ => Err(unsupported("a non-Int used as a number", span)),
    }
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

/// Binary operators with runtime-identical semantics (i64 wrapping is
/// rejected: debug-profile rustc panics on overflow, so comptime does too).
fn eval_binop(op: BinOp, l: CtValue, r: CtValue, span: Span) -> Result<CtValue, Diagnostic> {
    use CtValue::*;
    match (op, l, r) {
        (BinOp::Add, Int(a), Int(b)) => a
            .checked_add(b)
            .map(Int)
            .ok_or_else(|| overflow("add", span)),
        (BinOp::Sub, Int(a), Int(b)) => a
            .checked_sub(b)
            .map(Int)
            .ok_or_else(|| overflow("subtract", span)),
        (BinOp::Mul, Int(a), Int(b)) => a
            .checked_mul(b)
            .map(Int)
            .ok_or_else(|| overflow("multiply", span)),
        (BinOp::Div, Int(_), Int(0)) => Err(divide_by_zero(span)),
        (BinOp::Div, Int(a), Int(b)) => a
            .checked_div(b)
            .map(Int)
            .ok_or_else(|| overflow("divide", span)),
        (BinOp::Rem, Int(_), Int(0)) => Err(divide_by_zero(span)),
        (BinOp::Rem, Int(a), Int(b)) => a
            .checked_rem(b)
            .map(Int)
            .ok_or_else(|| overflow("take the remainder of", span)),
        (BinOp::BitAnd, Int(a), Int(b)) => Ok(Int(a & b)),
        (BinOp::BitOr, Int(a), Int(b)) => Ok(Int(a | b)),
        (BinOp::BitXor, Int(a), Int(b)) => Ok(Int(a ^ b)),
        (BinOp::Shl, Int(a), Int(b)) => Ok(Int(a.wrapping_shl(b as u32))),
        (BinOp::Shr, Int(a), Int(b)) => Ok(Int(a.wrapping_shr(b as u32))),
        (BinOp::Add, Float(a), Float(b)) => Ok(Float(a + b)),
        (BinOp::Sub, Float(a), Float(b)) => Ok(Float(a - b)),
        (BinOp::Mul, Float(a), Float(b)) => Ok(Float(a * b)),
        (BinOp::Div, Float(a), Float(b)) => Ok(Float(a / b)),
        (BinOp::Eq, a, b) => Ok(Bool(a == b)),
        (BinOp::Ne, a, b) => Ok(Bool(a != b)),
        (BinOp::Lt, a, b) => cmp(a, b, span).map(|o| Bool(o == std::cmp::Ordering::Less)),
        (BinOp::Gt, a, b) => cmp(a, b, span).map(|o| Bool(o == std::cmp::Ordering::Greater)),
        (BinOp::Le, a, b) => cmp(a, b, span).map(|o| Bool(o != std::cmp::Ordering::Greater)),
        (BinOp::Ge, a, b) => cmp(a, b, span).map(|o| Bool(o != std::cmp::Ordering::Less)),
        _ => Err(unsupported("this operation", span)),
    }
}

fn cmp(a: CtValue, b: CtValue, span: Span) -> Result<std::cmp::Ordering, Diagnostic> {
    use CtValue::*;
    match (a, b) {
        (Int(a), Int(b)) => Ok(a.cmp(&b)),
        (Float(a), Float(b)) => a
            .partial_cmp(&b)
            .ok_or_else(|| unsupported("comparing NaN", span)),
        (Char(a), Char(b)) => Ok(a.cmp(&b)),
        (Str(a), Str(b)) => Ok(a.cmp(&b)),
        _ => Err(unsupported("comparing these values", span)),
    }
}

/// Mutating list/map methods (`push`, `pop`, …). Returns the method's value.
fn apply_mutating(
    recv: &mut CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match (recv, method) {
        (CtValue::List(xs), "push") => {
            xs.push(args.into_iter().next().unwrap_or(CtValue::Unit));
            Ok(CtValue::Unit)
        }
        (CtValue::List(xs), "pop") => Ok(match xs.pop() {
            Some(v) => CtValue::Some(Box::new(v)),
            None => CtValue::None(Type::Int),
        }),
        (CtValue::List(xs), "reverse") => {
            xs.reverse();
            Ok(CtValue::Unit)
        }
        (CtValue::List(xs), "sort") => {
            xs.sort_by(|a, b| cmp(a.clone(), b.clone(), span).unwrap_or(std::cmp::Ordering::Equal));
            Ok(CtValue::Unit)
        }
        (CtValue::List(xs), "clear") => {
            xs.clear();
            Ok(CtValue::Unit)
        }
        (CtValue::List(xs), "remove") => {
            let i = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            if i < 0 || i as usize >= xs.len() {
                return Err(index_oob(xs.len(), i, span));
            }
            Ok(xs.remove(i as usize))
        }
        (CtValue::Map(m), "insert") => {
            let mut it = args.into_iter();
            let k = CtKey::from_value(it.next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            let v = it.next().unwrap_or(CtValue::Unit);
            m.insert(k, v);
            Ok(CtValue::Unit)
        }
        (CtValue::Map(m), "remove") => {
            let k = CtKey::from_value(args.into_iter().next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            m.remove(&k);
            Ok(CtValue::Unit)
        }
        _ => Err(unsupported(
            &format!("the method `.{}` at compile time", method),
            span,
        )),
    }
}

/// Non-mutating methods on values.
fn apply_method(
    recv: &CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match (recv, method) {
        // Universal
        (v, "to_string") => Ok(CtValue::Str(v.jet_show())),
        // Int / Float conversions
        (CtValue::Int(n), "to_float") => Ok(CtValue::Float(*n as f64)),
        (CtValue::Int(n), "abs") => n
            .checked_abs()
            .map(CtValue::Int)
            .ok_or_else(|| overflow("take the absolute value of", span)),
        (CtValue::Float(f), "to_int") => Ok(CtValue::Int(*f as i64)),
        (CtValue::Float(f), "abs") => Ok(CtValue::Float(f.abs())),
        // List
        (CtValue::List(xs), "len") => Ok(CtValue::Int(xs.len() as i64)),
        (CtValue::List(xs), "is_empty") => Ok(CtValue::Bool(xs.is_empty())),
        (CtValue::List(xs), "get") => {
            let i = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            Ok(if i < 0 || i as usize >= xs.len() {
                CtValue::None(xs.first().map(|v| v.jet_type()).unwrap_or(Type::Int))
            } else {
                CtValue::Some(Box::new(xs[i as usize].clone()))
            })
        }
        (CtValue::List(xs), "contains") => {
            let needle = args.into_iter().next().unwrap_or(CtValue::Unit);
            Ok(CtValue::Bool(xs.iter().any(|x| *x == needle)))
        }
        (CtValue::List(xs), "join") => {
            let sep = match args.into_iter().next() {
                Some(CtValue::Str(s)) => s,
                _ => String::new(),
            };
            let parts: Vec<String> = xs.iter().map(|x| x.jet_show()).collect();
            Ok(CtValue::Str(parts.join(&sep)))
        }
        // Map
        (CtValue::Map(m), "len") => Ok(CtValue::Int(m.len() as i64)),
        (CtValue::Map(m), "is_empty") => Ok(CtValue::Bool(m.is_empty())),
        (CtValue::Map(m), "contains_key") => {
            let k = CtKey::from_value(args.into_iter().next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            Ok(CtValue::Bool(m.contains_key(&k)))
        }
        (CtValue::Map(m), "get") => {
            let k = CtKey::from_value(args.into_iter().next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            Ok(match m.get(&k) {
                Some(v) => CtValue::Some(Box::new(v.clone())),
                None => CtValue::None(Type::Int),
            })
        }
        (CtValue::Map(m), "keys") => Ok(CtValue::List(m.keys().map(|k| k.to_value()).collect())),
        (CtValue::Map(m), "values") => Ok(CtValue::List(m.values().cloned().collect())),
        // String (char-counted per S41)
        (CtValue::Str(s), "len") => Ok(CtValue::Int(s.chars().count() as i64)),
        (CtValue::Str(s), "is_empty") => Ok(CtValue::Bool(s.is_empty())),
        (CtValue::Str(s), "to_upper") => Ok(CtValue::Str(s.to_uppercase())),
        (CtValue::Str(s), "to_lower") => Ok(CtValue::Str(s.to_lowercase())),
        (CtValue::Str(s), "trim") => Ok(CtValue::Str(s.trim().to_string())),
        (CtValue::Str(s), "contains") => match args.into_iter().next() {
            Some(CtValue::Str(n)) => Ok(CtValue::Bool(s.contains(&n))),
            _ => Err(unsupported("contains with a non-text argument", span)),
        },
        (CtValue::Str(s), "starts_with") => match args.into_iter().next() {
            Some(CtValue::Str(n)) => Ok(CtValue::Bool(s.starts_with(&n))),
            _ => Err(unsupported("starts_with with a non-text argument", span)),
        },
        (CtValue::Str(s), "ends_with") => match args.into_iter().next() {
            Some(CtValue::Str(n)) => Ok(CtValue::Bool(s.ends_with(&n))),
            _ => Err(unsupported("ends_with with a non-text argument", span)),
        },
        (CtValue::Str(s), "repeat") => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            Ok(CtValue::Str(s.repeat(n.max(0) as usize)))
        }
        (CtValue::Str(s), "replace") => {
            let mut it = args.into_iter();
            match (it.next(), it.next()) {
                (Some(CtValue::Str(from)), Some(CtValue::Str(to))) => {
                    Ok(CtValue::Str(s.replace(&from, &to)))
                }
                _ => Err(unsupported("replace with non-text arguments", span)),
            }
        }
        (CtValue::Str(s), "split") => {
            let sep = match args.into_iter().next() {
                Some(CtValue::Str(s)) => s,
                _ => String::new(),
            };
            Ok(CtValue::List(
                s.split(&sep).map(|p| CtValue::Str(p.to_string())).collect(),
            ))
        }
        (CtValue::Str(s), "chars") => Ok(CtValue::List(s.chars().map(CtValue::Char).collect())),
        _ => Err(unsupported(
            &format!("the method `.{}` at compile time", method),
            span,
        )),
    }
}

// --- diagnostics ----------------------------------------------------------

fn comptime_panic(msg: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0953",
        "your comptime code stopped the build".to_string(),
        format!(
            "while computing this value at compile time, the program panicked: {}",
            msg
        ),
        "this is the sanctioned way to validate at compile time — fix the input the check rejects"
            .to_string(),
        Some(span),
    )
}

fn overflow(verb: &str, span: Span) -> Diagnostic {
    comptime_panic(
        &format!(
            "tried to {} two numbers and the result was too big for an Int",
            verb
        ),
        span,
    )
}

fn divide_by_zero(span: Span) -> Diagnostic {
    comptime_panic("divided by zero", span)
}

fn index_oob(len: usize, i: i64, span: Span) -> Diagnostic {
    comptime_panic(
        &format!(
            "the list has {} items, so position {} doesn't exist",
            len, i
        ),
        span,
    )
}

fn slice_oob(len: i64, a: i64, b: i64, span: Span) -> Diagnostic {
    comptime_panic(
        &format!("can't slice {} items from {} to {} (inclusive)", len, a, b),
        span,
    )
}

fn map_missing(span: Span) -> Diagnostic {
    comptime_panic("the map has no entry for this key", span)
}

fn unsupported(what: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        format!("{} can't run at compile time yet", what),
        "comptime evaluates a pure subset of Jet; this construct isn't supported there yet"
            .to_string(),
        "compute this value at runtime, or use a simpler comptime expression".to_string(),
        Some(span),
    )
}

fn unsupported_expr(e: &Expr) -> Diagnostic {
    unsupported("this expression", e.span())
}

// --- purity check ---------------------------------------------------------

/// Walk the call graph reachable from `init`; reject the first impure call
/// (IO, FFI) with the path that reached it (E0951). `embed_file`, `panic`,
/// and `require` are allowed.
fn check_purity(
    init: &Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
) -> Result<(), Diagnostic> {
    let mut visited = HashSet::new();
    let mut path = Vec::new();
    check_purity_expr(init, funcs, extern_names, &mut visited, &mut path)
}

fn impure_builtin(name: &str) -> bool {
    matches!(name, "print" | "eprint" | "input" | "read_all_input")
}

fn check_purity_expr(
    e: &Expr,
    funcs: &HashMap<String, &Func>,
    externs: &HashSet<String>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Result<(), Diagnostic> {
    let mut result = Ok(());
    crate::comptime::walk_calls(e, &mut |name, span| {
        if result.is_err() {
            return;
        }
        if impure_builtin(name) || externs.contains(name) {
            result = Err(impurity_diag(name, path, span));
        } else if let Some(f) = funcs.get(name) {
            if visited.insert(name.to_string()) {
                path.push(name.to_string());
                for stmt in &f.body {
                    if result.is_err() {
                        break;
                    }
                    result = check_purity_stmt(stmt, funcs, externs, visited, path);
                }
                path.pop();
            }
        }
    });
    result
}

fn check_purity_stmt(
    s: &Stmt,
    funcs: &HashMap<String, &Func>,
    externs: &HashSet<String>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Result<(), Diagnostic> {
    let mut result = Ok(());
    walk_stmt_exprs(s, &mut |e| {
        if result.is_ok() {
            result = check_purity_expr(e, funcs, externs, visited, path);
        }
    });
    result
}

fn impurity_diag(name: &str, path: &[String], span: Span) -> Diagnostic {
    let why = if path.is_empty() {
        format!(
            "`{}` touches the outside world, so it can't run while compiling",
            name
        )
    } else {
        format!(
            "{} calls `{}`, which touches the outside world — comptime must give the same answer on every machine",
            path.join(" calls "),
            name
        )
    };
    Diagnostic::error(
        "E0951",
        format!("`{}` is not allowed in comptime code", name),
        why,
        "compute this at runtime instead; the one exception is `embed_file(\"path\")`".to_string(),
        Some(span),
    )
}

/// Visit every direct `Call` name in an expression tree (shallow over
/// nested functions — recursion is driven by the purity walker).
pub fn walk_calls(e: &Expr, f: &mut impl FnMut(&str, Span)) {
    match e {
        Expr::Call(c) => {
            f(&c.name, c.name_span);
            for a in &c.args {
                walk_calls(&a.expr, f);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            walk_calls(receiver, f);
            for a in args {
                walk_calls(&a.expr, f);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            walk_calls(callee, f);
            for a in args {
                walk_calls(&a.expr, f);
            }
        }
        Expr::Binary(_, l, r, _) => {
            walk_calls(l, f);
            walk_calls(r, f);
        }
        Expr::Unary(_, x, _) | Expr::Present(x, _) | Expr::Try(x, _, _) | Expr::Deref(x, _) => {
            walk_calls(x, f)
        }
        Expr::Index { base, index, .. } => {
            walk_calls(base, f);
            walk_calls(index, f);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            walk_calls(base, f);
            walk_calls(start, f);
            walk_calls(end, f);
        }
        Expr::ListLit(xs, _) => xs.iter().for_each(|x| walk_calls(x, f)),
        Expr::MapLit(es, _) => es.iter().for_each(|(k, v)| {
            walk_calls(k, f);
            walk_calls(v, f);
        }),
        Expr::Str(parts, _) => parts.iter().for_each(|p| {
            if let StrPart::Interp(e) = p {
                walk_calls(e, f)
            }
        }),
        Expr::Ok(x, _) | Expr::Err(x, _) => walk_calls(x, f),
        Expr::Field(x, _, _) => walk_calls(x, f),
        Expr::OrFallback { value, .. } => walk_calls(value, f),
        Expr::EnumLit { args, .. } => args.iter().for_each(|a| match a {
            EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => walk_calls(e, f),
        }),
        _ => {}
    }
}

fn walk_stmt_exprs(s: &Stmt, f: &mut impl FnMut(&Expr)) {
    match s {
        Stmt::Expr(e) | Stmt::Val(crate::ast::Binding { init: e, .. }) => f(e),
        Stmt::Assign { value, .. } => f(value),
        Stmt::Return(Some(e), _) => f(e),
        Stmt::Return(None, _) => {}
        Stmt::If(ifs) => walk_if_exprs(ifs, f),
        Stmt::While { cond, body, .. } => {
            f(cond);
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                crate::ast::ForKind::Range { start, end, step } => {
                    f(start);
                    f(end);
                    if let Some(step) = step {
                        f(step);
                    }
                }
                crate::ast::ForKind::In { collection } => f(collection),
            }
            body.iter().for_each(|s| walk_stmt_exprs(s, f));
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            f(subject);
            for a in arms {
                f(&a.cond);
                a.body.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
            if let Some(b) = else_body {
                b.iter().for_each(|s| walk_stmt_exprs(s, f));
            }
        }
        Stmt::Loop(body, _) | Stmt::Unsafe { body, .. } => {
            body.iter().for_each(|s| walk_stmt_exprs(s, f))
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
    }
}

fn walk_if_exprs(ifs: &crate::ast::IfStmt, f: &mut impl FnMut(&Expr)) {
    f(&ifs.cond);
    ifs.then_body.iter().for_each(|s| walk_stmt_exprs(s, f));
    match &ifs.else_branch {
        Some(crate::ast::ElseBranch::ElseIf(inner)) => walk_if_exprs(inner, f),
        Some(crate::ast::ElseBranch::Else(body)) => body.iter().for_each(|s| walk_stmt_exprs(s, f)),
        None => {}
    }
}

// --- public entry ---------------------------------------------------------

/// Type-check happens elsewhere (every function body goes through sema);
/// this checks purity then evaluates `init` to a constant value.
pub fn evaluate(
    init: &Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
) -> Result<CtValue, Diagnostic> {
    check_purity(init, funcs, extern_names)?;
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: FUEL_BUDGET,
        sink: None,
    };
    let mut scope = globals.clone();
    interp.eval(init, &mut scope)
}

/// Whole-program dev interpretation (E2-M4 `jet dev`). Runs `main`'s body
/// with a buffered stdout/stderr sink, reusing the exact same evaluator the
/// M9.5 comptime path uses — there is no second interpreter. Output bytes are
/// produced via `CtValue::jet_show()` + `\n`, identical to the compiled
/// program (the differential battery in `tests/dev.rs` enforces this, I2).
///
/// The caller (src/interp.rs) is responsible for the E2201 boundary scan
/// (FFI/tasks/`@unsafe`); this function simply runs and may itself return
/// E0956 (`unsupported`) when it reaches a construct the evaluator can't run,
/// or E2202 when the fuel budget is exhausted.
pub fn run_main(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel: DEV_FUEL_BUDGET,
        sink: Some(sink),
    };
    let mut scope = HashMap::new();
    interp.exec_block(&main.body, &mut scope)?;
    Ok(())
}

/// REPL variant of `run_main`: uses a caller-supplied fuel cap so the REPL
/// can enforce D-REPL-FUEL without patching DEV_FUEL_BUDGET. Returns the
/// same E2202 (dev fuel stop) or E0956 (unsupported) errors; the REPL
/// intercepts E2202 and upgrades it to E1801 with REPL-specific wording.
pub fn run_main_with_fuel(
    main: &Func,
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    fuel: u64,
) -> Result<(), Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel,
        sink: Some(sink),
    };
    let mut scope = HashMap::new();
    interp.exec_block(&main.body, &mut scope)?;
    Ok(())
}

/// REPL per-input step (E2-M18). Executes `stmts` inside a running REPL
/// session. Differs from `run_main_with_fuel` in two ways:
///
/// 1. The scope is passed in *and mutated* — accumulated bindings survive
///    across inputs (D-REPL7: one accumulating module).
/// 2. If the last statement is a bare `Stmt::Expr` (not `Stmt::Val`), the
///    evaluated value is returned so the caller can display
///    `x: T = v` (D-REPL16=B).
///
/// The `suppress` flag implements `;` at end of input — the caller strips the
/// trailing `;` to detect a bare expression and passes `suppress = false`; a
/// statement ending in `;` passes `suppress = true`.
pub fn run_repl_step(
    stmts: &[crate::ast::Stmt],
    funcs: &HashMap<String, &Func>,
    base_dir: &Path,
    sink: &mut DevSink,
    scope: &mut HashMap<String, CtValue>,
    fuel: u64,
    suppress: bool,
) -> Result<Option<CtValue>, Diagnostic> {
    let mut interp = Interp {
        funcs,
        base_dir,
        fuel,
        sink: Some(sink),
    };
    // Split: run all statements except the last; then handle the last specially
    // if it is a bare expression (for display) and not suppressed.
    let (last, head) = match stmts.split_last() {
        Some(pair) => pair,
        None => return Ok(None),
    };
    interp.exec_block(head, scope)?;
    // Determine if the last statement should produce an echo value.
    // Case 1: `Stmt::Val` named `__repl_echo__` — the sentinel that `classify`
    //   injects for bare-expression inputs (e.g. `1 + 2` → `val __repl_echo__ = 1 + 2`).
    //   Evaluate but don't add to the persistent scope.
    // Case 2: bare `Stmt::Expr` — retained for forward-compat.
    let echo_bare = !suppress && matches!(last, crate::ast::Stmt::Expr(_));
    match last {
        crate::ast::Stmt::Val(b) if !suppress && b.name == "__repl_echo__" => {
            let v = interp.eval(&b.init, scope)?;
            Ok(Some(v))
        }
        crate::ast::Stmt::Val(b) => {
            let v = interp.eval(&b.init, scope)?;
            if let Some(pat) = &b.pattern {
                interp.bind_pattern(pat, v, scope)?;
            } else {
                scope.insert(b.name.clone(), v);
            }
            Ok(None)
        }
        crate::ast::Stmt::Expr(e) if echo_bare => {
            let v = interp.eval(e, scope)?;
            Ok(Some(v))
        }
        other => {
            interp.exec_stmt(other, scope)?;
            Ok(None)
        }
    }
}

/// Owned-function variant used while sema is mutating function bodies for
/// local `comptime` bindings. The cloned function map is a snapshot of the
/// already-parsed program; the interpreter still sees ordinary Jet AST.
pub fn evaluate_owned(
    init: &Expr,
    funcs: &HashMap<String, Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    globals: &HashMap<String, CtValue>,
) -> Result<CtValue, Diagnostic> {
    let refs: HashMap<String, &Func> = funcs.iter().map(|(n, f)| (n.clone(), f)).collect();
    evaluate(init, &refs, extern_names, base_dir, globals)
}
