//! D-CONC-SHARE1=A (ratified 2026-08-06, card #1561): a `Shared<T>` value
//! reads and writes with ordinary value syntax.
//!
//! `shared expr` builds the cell (the parser's one construction shape).
//! `handle.field` reads it, `handle.field = v` and `handle.field += v` write
//! it, and each statement is one atomic locked step. Several statements commit
//! together under `#Transact`. The compiler's internal read/edit seam is not a
//! source spelling; expert guards and `Condition` are unchanged.
//!
//! One desugar owns the whole surface, and it targets the shape every
//! execution tier already lowers:
//!
//! ```text
//! label :: config.name      →  internal locked-read call
//! config.hits += 1          →  internal locked-edit call
//! ```
//!
//! That keeps the semantics in the Prelude with engines only marshalling (I9).
//! The one seam is `JetShared::read`/`edit`/`edit_txn` in
//! `Prelude/CoreLib/JetStd/MathTaskMem.rs`, which acquires through
//! `Prelude/SharedProtocol.rs`. The AOT emitter, the Cranelift host, the TIR
//! evaluator, and the comptime evaluator all already route to it, so this card
//! adds no per-engine locking policy and no second sharing mechanism.
//!
//! A read-modify-write (`+=`) becomes ONE edit, so the read and the write
//! share one lock and no update is lost.
//!
//! A statement that also reads another shared cell would otherwise nest one
//! cell's lock inside another's, which is the only way plain access could
//! deadlock. Such a statement is wrapped in a synthesized one-statement
//! transaction: the other cell's read registers its participant and runs in
//! the body, the write defers, and the commit takes every participant's write
//! lock through `jet_shared_acquire_ordered` — stable address order, one
//! engine, no nesting. The synthesized transaction carries the commit plane only; the
//! D-TXN2 effect wall stays with transactions the author wrote.

use super::*;
use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::AST::{
    AccessConvention, BinOp, CallArg, CallArgFlags, Expr, LValue, Lambda, LambdaBody, LambdaMeta,
    LambdaParam, Stmt, Type,
};

/// The synthesized lambda parameter standing for the locked payload, and the
/// synthesized name holding a hoisted right-hand side. Neither is spellable
/// Jet, so they can never shadow or be shadowed by user code.
pub(crate) const SHARED_PAYLOAD_PARAM: &str = "__jet_shared_value";
const SHARED_HOISTED_RHS: &str = "__jet_shared_rhs";

/// `Some(inner)` when `ty` is a `Shared<inner>` handle.
pub(crate) fn shared_inner(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Shared(inner) => Some(inner),
        _ => None,
    }
}

fn payload(span: Span) -> Expr {
    Expr::Ident(SHARED_PAYLOAD_PARAM.to_string(), span)
}

/// Build `handle.<method>(<payload> -> <body>)`, tagging the synthesized
/// argument so the retired-spelling check can tell this desugar apart from a
/// user-typed closure.
fn shared_access_call(handle: Expr, method: &str, body: LambdaBody, span: Span) -> Expr {
    let lambda = Lambda {
        take_names: Vec::new(),
        params: vec![LambdaParam {
            name: SHARED_PAYLOAD_PARAM.to_string(),
            name_span: span,
            ty: None,
            ty_span: None,
        }],
        result_type: None,
        error_type: None,
        effects: None,
        body,
        span,
        meta: LambdaMeta::default(),
    };
    Expr::MethodCall {
        receiver: Box::new(handle),
        method: method.to_string(),
        method_span: span,
        owner_type_args: Vec::new(),
        type_args: Vec::new(),
        args: vec![CallArg {
            convention: AccessConvention::Read,
            expr: Expr::Lambda(Box::new(lambda)),
            span,
            flags: CallArgFlags {
                shared_access_desugar: true,
                ..CallArgFlags::default()
            },
            label: None,
            spread: false,
        }],
        recv_type: None,
        resolved_ret: None,
        operator_rhs: None,
        checked_widen: false,
    }
}

/// True when this method call is the compiler's own use of the locked
/// read/edit seam rather than a retired user-typed closure.
pub(crate) fn is_shared_access_desugar(args: &[CallArg]) -> bool {
    args.len() == 1 && args[0].flags.shared_access_desugar
}

fn hoisted_rhs_binding(init: Expr, span: Span) -> Stmt {
    Stmt::Val(crate::AST::Binding {
        mutable: false,
        markers: Vec::new(),
        reactive_upgrade: false,
        meta: None,
        name: SHARED_HOISTED_RHS.to_string(),
        name_span: span,
        sigil_span: None,
        pattern: None,
        ty: None,
        ty_span: None,
        init,
        is_comptime: false,
        ct: None,
        uninit: false,
        arena_view: false,
        string_view: false,
        gc_promotion: None,
        gc_transferred: false,
    })
}

fn edit_field_stmt(field: &str, op: Option<BinOp>, op_span: Span, value: Expr, span: Span) -> Stmt {
    Stmt::Assign {
        target: LValue::Field {
            base: Box::new(payload(span)),
            field: field.to_string(),
            span,
        },
        op,
        op_span,
        value,
    }
}

#[derive(Debug)]
struct SharedPoll {
    handle: String,
    field: String,
}

fn expr_root_is(expr: &Expr, name: &str) -> bool {
    match expr.without_parens() {
        Expr::Ident(candidate, _) => candidate == name,
        Expr::Field(base, ..)
        | Expr::Index { base, .. }
        | Expr::Slice { base, .. }
        | Expr::OptField { base, .. } => expr_root_is(base, name),
        _ => false,
    }
}

fn body_has_blocking_call(body: &[Stmt]) -> bool {
    body.iter().any(|stmt| {
        let mut found = false;
        stmt.for_each_expr(|expr| {
            if found {
                return;
            }
            found = match expr.without_parens() {
                Expr::MethodCall { method, .. } => matches!(
                    method.as_str(),
                    "join"
                        | "read"
                        | "read_line"
                        | "receive"
                        | "recv"
                        | "sleep"
                        | "sleep_until"
                        | "wait"
                        | "yield_now"
                ),
                Expr::Call(call) => matches!(
                    call.name.as_str(),
                    "join"
                        | "read"
                        | "read_line"
                        | "receive"
                        | "recv"
                        | "sleep"
                        | "sleep_until"
                        | "wait"
                        | "yield_now"
                ),
                _ => false,
            };
        });
        found
    })
}

fn body_writes_shared_field(body: &[Stmt], poll: &SharedPoll) -> bool {
    body.iter()
        .any(|stmt| stmt_writes_shared_field(stmt, poll))
}

fn expr_writes_shared_field(expr: &Expr, poll: &SharedPoll) -> bool {
    match expr.without_parens() {
        Expr::Field(base, field, _) => {
            expr_root_is(base, &poll.handle) && field == &poll.field
        }
        Expr::Index { base, .. } => expr_root_is(base, &poll.handle),
        _ => false,
    }
}

fn stmt_writes_shared_field(stmt: &Stmt, poll: &SharedPoll) -> bool {
    let mut writes_incdec = false;
    stmt.for_each_expr(|expr| {
        if let Expr::IncDec { operand, .. } = expr.without_parens() {
            writes_incdec |= expr_writes_shared_field(operand, poll);
        }
    });
    if writes_incdec {
        return true;
    }
    match stmt {
        Stmt::Assign { target, .. } => match target {
            LValue::Local { .. } => false,
            LValue::Index { base, .. } => expr_root_is(base, &poll.handle),
            LValue::Field { base, field, .. } => {
                expr_root_is(base, &poll.handle) && field == &poll.field
            }
        },
        Stmt::While { body, .. }
        | Stmt::For { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. } => body_writes_shared_field(body, poll),
        Stmt::CountedLoop { step, body, .. } => {
            step.as_deref()
                .is_some_and(|step| stmt_writes_shared_field(step, poll))
                || body_writes_shared_field(body, poll)
        }
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .any(|arm| body_writes_shared_field(&arm.body, poll))
                || else_body
                    .as_deref()
                    .is_some_and(|body| body_writes_shared_field(body, poll))
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            body_writes_shared_field(then_body, poll)
                || else_body
                    .as_deref()
                    .is_some_and(|body| body_writes_shared_field(body, poll))
        }
        Stmt::Expr(_)
        | Stmt::Val(_)
        | Stmt::Return(_, _)
        | Stmt::Break(_)
        | Stmt::BreakValue(_, _)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(_, _)
        | Stmt::BreakLabelValue(_, _, _, _)
        | Stmt::ContinueLabel(_, _)
        | Stmt::Yield(_, _)
        | Stmt::DeferClose { .. } => false,
    }
}

fn source_loop_open(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len()
                    && bytes.get(index..index + 2) != Some(b"*/")
                {
                    index += 1;
                }
                if index + 1 >= bytes.len() {
                    return None;
                }
                index += 2;
            }
            b'{' => return Some(index),
            _ => return None,
        }
    }
    None
}

fn source_matching_brace(source: &str, open: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = open;
    let mut string = false;
    let mut line_comment = false;
    let mut block_comment = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
            index += 1;
            continue;
        }
        if block_comment {
            if bytes.get(index..index + 2) == Some(b"*/") {
                block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if string {
            if byte == b'\\' {
                index += 2;
            } else {
                string = byte != b'"';
                index += 1;
            }
            continue;
        }
        if bytes.get(index..index + 2) == Some(b"//") {
            line_comment = true;
            index += 2;
        } else if bytes.get(index..index + 2) == Some(b"/*") {
            block_comment = true;
            index += 2;
        } else if byte == b'"' {
            string = true;
            index += 1;
        } else if byte == b'{' {
            depth += 1;
            index += 1;
        } else if byte == b'}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(index);
            }
            index += 1;
        } else {
            index += 1;
        }
    }
    None
}

impl<'a> Checker<'a> {
    fn shared_poll(&self, expr: &Expr) -> Option<SharedPoll> {
        let Expr::Field(base, field, _) = expr.without_parens() else {
            return None;
        };
        let Expr::Ident(handle, _) = base.without_parens() else {
            return None;
        };
        self.place_type_is_shared(base).then(|| SharedPoll {
            handle: handle.clone(),
            field: field.clone(),
        })
    }

    fn find_shared_poll(&self, cond: &Expr) -> Option<SharedPoll> {
        let mut found = None;
        cond.for_each_expr(|expr| {
            if found.is_none() {
                found = self.shared_poll(expr);
            }
        });
        found
    }

    fn unique_condition_name(&self) -> Option<String> {
        let mut names = self
            .visible_names()
            .into_iter()
            .filter(|name| {
                self.lookup(name).is_some_and(|info| {
                    matches!(&info.ty, Type::Named(type_name) if type_name == crate::Syntax::TYPE_CONDITION)
                })
            })
            .collect::<Vec<_>>();
        (names.len() == 1).then(|| names.pop().expect("one Condition name"))
    }

    fn shared_busy_wait_edit(
        &self,
        loop_span: Span,
        cond: &Expr,
        body: &[Stmt],
        poll: &SharedPoll,
    ) -> Option<TextEdit> {
        if !body.is_empty() {
            return None;
        }
        let Expr::Binary(BinOp::Eq, lhs, rhs, _) = cond.without_parens() else {
            return None;
        };
        let Expr::Field(base, field, _) = lhs.without_parens() else {
            return None;
        };
        let Expr::Ident(handle, _) = base.without_parens() else {
            return None;
        };
        if handle != &poll.handle
            || field != &poll.field
            || !matches!(rhs.without_parens(), Expr::Bool(false, _))
        {
            return None;
        }
        let condition = self.unique_condition_name()?;
        let close = source_loop_open(self.source, cond.span().end)
            .and_then(|open| source_matching_brace(self.source, open))?;
        Some(TextEdit {
            span: Span::new(loop_span.start, close + 1),
            new_text: format!(
                "{handle}.guard_edit().wait({condition}, value -> value.{field}) ?? panic(\"wait failed\")"
            ),
        })
    }

    /// D-CONC-SHARE2: warn when an unbounded conditional loop repeatedly polls
    /// a shared field without a wait point, a sleep, a channel receive, or a
    /// write that can change the polled state.
    pub(crate) fn lint_shared_busy_wait(
        &mut self,
        cond: &Expr,
        body: &[Stmt],
        loop_span: Span,
        allow_auto_fix: bool,
    ) {
        if !self.flow.reachable {
            return;
        }
        let Some(poll) = self.find_shared_poll(cond) else {
            return;
        };
        if body_has_blocking_call(body) || body_writes_shared_field(body, &poll) {
            return;
        }
        let mut diagnostic = Diagnostic::from_row("L0207", &[], Some(cond.span()));
        if allow_auto_fix {
            if let Some(edit) = self.shared_busy_wait_edit(loop_span, cond, body, &poll) {
                diagnostic = diagnostic.with_edit(edit);
            }
        }
        self.diags.push(diagnostic);
    }
}

impl<'a> Checker<'a> {
    /// D-CONC-SHARE1=A: rewrite `handle.field` on a `Shared<T>` handle into the
    /// locked read the Prelude ships. Returns `true` when `e` was rewritten and
    /// the caller must re-infer it.
    pub(crate) fn desugar_shared_field_read(&mut self, e: &mut Expr) -> bool {
        let Expr::Field(inner, member, span) = e else {
            return false;
        };
        let span = *span;
        let member = member.clone();
        if !self.place_type_is_shared(inner) {
            return false;
        }
        let handle = std::mem::replace(&mut **inner, Expr::Absent(span));
        let projection = Expr::Field(Box::new(payload(span)), member, span);
        *e = shared_access_call(handle, "read", LambdaBody::Expr(Box::new(projection)), span);
        true
    }

    /// D-CONC-SHARE1=A: rewrite `handle.field <op>= value` on a `Shared<T>`
    /// handle into the locked edit. Returns `true` when `stmt` was rewritten.
    ///
    /// When the value also reads another shared cell, the write goes through a
    /// synthesized one-statement transaction outside an explicit transaction,
    /// so the commit acquires in address order instead of nesting one cell's
    /// lock inside another's. Inside `#Transact`, it joins that outer commit.
    pub(crate) fn desugar_shared_field_write(&mut self, stmt: &mut Stmt) -> bool {
        let Stmt::Assign {
            target: LValue::Field { base, field, span },
            op,
            op_span,
            value,
        } = stmt
        else {
            return false;
        };
        if !self.place_type_is_shared(base) {
            return false;
        }
        let span = *span;
        let op = *op;
        let op_span = *op_span;
        let field = field.clone();
        let other_cells = self.value_touches_another_shared_cell(value);
        // A deferred edit runs at commit, while every participant already has
        // its write lock. Evaluate shared RHS reads in the transaction body so
        // they cannot re-enter a lock held by that edit. The wrapper is a real
        // implicit transaction outside an authored block; inside one, lowering
        // erases the nested wrapper to a plain joined block.
        let needs_implicit_transaction = other_cells;
        let handle = std::mem::replace(&mut **base, Expr::Absent(span));
        let value = std::mem::replace(value, Expr::Absent(span));
        *stmt = if needs_implicit_transaction {
            let body = vec![
                hoisted_rhs_binding(value, span),
                Stmt::Expr(shared_access_call(
                    handle,
                    "edit",
                    LambdaBody::Block(vec![edit_field_stmt(
                        &field,
                        op,
                        op_span,
                        Expr::Ident(SHARED_HOISTED_RHS.to_string(), span),
                        span,
                    )]),
                    span,
                )),
            ];
            Stmt::Transact {
                name: None,
                name_span: None,
                body,
                implicit: true,
                span,
            }
        } else {
            Stmt::Expr(shared_access_call(
                handle,
                "edit",
                LambdaBody::Block(vec![edit_field_stmt(&field, op, op_span, value, span)]),
                span,
            ))
        };
        true
    }

    /// Does this value expression read a shared cell anywhere inside it? The
    /// probe runs on a clone so the original statement is untouched.
    fn value_touches_another_shared_cell(&self, value: &Expr) -> bool {
        let mut bases: Vec<Expr> = Vec::new();
        let mut probe = value.clone();
        probe.for_each_expr_mut(|node| {
            if let Expr::Field(base, _, _) = node {
                bases.push((**base).clone());
            }
        });
        bases.iter().any(|base| self.place_type_is_shared(base))
    }

    /// Cheap place-type probe. Only a named place or a struct field can hold a
    /// shared handle in the ratified surface, and both types are already
    /// recorded, so this never re-runs inference or emits a diagnostic.
    fn place_type_is_shared(&self, expr: &Expr) -> bool {
        self.compound_expr_type(expr.without_parens())
            .is_some_and(|ty| shared_inner(&ty).is_some())
    }
}
