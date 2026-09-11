use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::Checker;
use crate::Sema::Captures::expr_refs_name;
use crate::AST::{AccessConvention, CtValue, Expr, LValue, LambdaBody, PlaceAccess, Stmt};
use jet_foundation::MIR::{
    MirDecisionDisposition, MirDecisionKind, MirDecisionRow,
};

const LINT_UNREACHABLE_AFTER_LOOP: &str = "unreachable_after_loop";

impl<'a> Checker<'a> {
    fn attach_decision_row(
        &self,
        diagnostic: Diagnostic,
        kind: MirDecisionKind,
        disposition: MirDecisionDisposition,
        span: Span,
        rule: &str,
        reason: &str,
    ) -> Diagnostic {
        diagnostic.with_decision_row(MirDecisionRow::new(
            kind,
            disposition,
            None,
            self.fn_name.clone(),
            span,
            rule,
            reason,
            "sema.loop-liveness",
            "static-flow-proof",
        ))
    }

    /// Emit statement-local loop/liveness guidance after the statement has been
    /// type-checked. The caller supplies the reachability before the statement
    /// so a constant-true loop can still report its own constant condition.
    pub(crate) fn emit_loop_liveness_lints_for_stmt(
        &mut self,
        stmt: &Stmt,
        reachable_before: bool,
        effect_free: bool,
    ) {
        if !reachable_before {
            return;
        }
        self.emit_constant_loop_condition_lint(stmt);
        self.emit_discarded_expression_lint(stmt, effect_free);
        self.emit_mutable_never_reassigned_lint(stmt);
        self.emit_state_loop_unchanged_lint(stmt);
    }

    /// Report the first statement after an unconditional loop. This runs from
    /// the block walker before the statement consumes its marker facts, so an
    /// adjacent `#allow(unreachable_after_loop)` can suppress just this report.
    pub(crate) fn emit_unreachable_after_loop(&mut self, previous: &Stmt, current: &Stmt) {
        if self.flow.reachable
            || !self.is_unconditional_loop(previous)
            || self.statement_lint_is_allowed(current.span(), LINT_UNREACHABLE_AFTER_LOOP)
        {
            return;
        }
        let diagnostic = self.attach_decision_row(
            Diagnostic::from_row("L0525", &[], Some(current.span())).with_edit(TextEdit {
                span: current.span(),
                new_text: String::new(),
            }),
            MirDecisionKind::Unreachable,
            MirDecisionDisposition::Selected,
            current.span(),
            "unreachable-after-loop",
            "the unconditional loop has no reachable break before this statement",
        );
        self.diags.push(diagnostic);
    }

    fn emit_constant_loop_condition_lint(&mut self, stmt: &Stmt) {
        let (condition, value, removable) = match stmt {
            Stmt::While { cond, .. } => {
                let Some(value) = self.constant_bool(cond) else {
                    return;
                };
                (cond, value, true)
            }
            Stmt::CountedLoop { cond, .. } => {
                let Some(value) = self.constant_bool(cond) else {
                    return;
                };
                // A counted/state loop cannot become `loop { … }` by deleting
                // its condition: its initializer is part of the loop header.
                (cond, value, false)
            }
            _ => return,
        };
        let span = condition.span();
        if span.start >= span.end {
            return;
        }
        // Only the literal `true` in a plain conditional loop has a
        // behavior-preserving header rewrite. Folded expressions still
        // receive the useful lint, but no automatic edit is guessed for them.
        let mut diagnostic = Diagnostic::from_row("L0526", &[], Some(span));
        if removable
            && value
            && matches!(condition.without_parens(), Expr::Bool(true, _))
        {
            if let Some(header_span) = self.loop_condition_header_span(stmt, span) {
                diagnostic = diagnostic.with_edit(TextEdit {
                    span: header_span,
                    new_text: "loop".to_string(),
                });
            }
        }
        let diagnostic = self.attach_decision_row(
            diagnostic,
            MirDecisionKind::LoopInvariant,
            MirDecisionDisposition::Rejected,
            span,
            "constant-loop-condition",
            "the loop condition is constant under the checked flow facts",
        );
        self.diags.push(diagnostic);
    }

    fn emit_discarded_expression_lint(&mut self, stmt: &Stmt, effect_free: bool) {
        if self.arrow_loop_body || !effect_free {
            return;
        }
        let Stmt::Expr(expr) = stmt else {
            return;
        };
        let expression = expr.without_parens();
        let discarded = match expression {
            Expr::Ident(name, _) => !name.starts_with('\0') && name != "_",
            Expr::Unary(..) | Expr::Binary(..) | Expr::CompareChain { .. } => true,
            _ => false,
        };
        if !discarded {
            return;
        }
        let span = expr.span();
        if span.start >= span.end {
            return;
        }
        // `effect_free` is the post-check delta of the canonical effect
        // accumulators, so removing this expression cannot drop a call or
        // other recorded ambient effect.
        let diagnostic = self.attach_decision_row(
            Diagnostic::from_row("L0527", &[], Some(span)).with_edit(TextEdit {
                span,
                new_text: String::new(),
            }),
            MirDecisionKind::LoopInvariant,
            MirDecisionDisposition::Rejected,
            span,
            "discarded-expression",
            "the expression has no checked effect and its value is discarded",
        );
        self.diags.push(diagnostic);

    }

    fn emit_mutable_never_reassigned_lint(&mut self, stmt: &Stmt) {
        let Stmt::Val(binding) = stmt else {
            return;
        };
        if !binding.mutable || binding.name.is_empty() {
            return;
        }
        let Some((info_mutable, info_sigil_span)) = self
            .lookup(&binding.name)
            .map(|info| (info.mutable, info.binding_sigil_span))
        else {
            return;
        };
        if !info_mutable {
            return;
        }
        let name = binding.name.as_str();
        let tail_has_write = {
            let tail = self.current_stmt_tail();
            tail.iter().any(|stmt| statement_writes_name(stmt, name))
        };
        if tail_has_write {
            return;
        }
        let sigil_span = binding.sigil_span.or(info_sigil_span);
        let mut diagnostic =
            Diagnostic::from_row("L0528", &[], Some(binding.name_span));
        if let Some(sigil_span) = sigil_span {
            diagnostic = diagnostic.with_edit(TextEdit {
                span: sigil_span,
                new_text: "::".to_string(),
            });
        }
        let diagnostic = self.attach_decision_row(
            diagnostic,
            MirDecisionKind::LoopInvariant,
            MirDecisionDisposition::Rejected,
            binding.name_span,
            "mutable-never-reassigned",
            "the mutable binding has no checked write in the remaining block",
        );
        self.diags.push(diagnostic);
    }

    fn emit_state_loop_unchanged_lint(&mut self, stmt: &Stmt) {
        let Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            label,
            ..
        } = stmt
        else {
            return;
        };
        if !init.mutable || init.name.is_empty() || !expr_refs_name(cond, &init.name) {
            return;
        }
        let name = init.name.as_str();
        if body.iter().any(|statement| statement_writes_name(statement, name))
            || step
                .as_deref()
                .is_some_and(|statement| statement_writes_name(statement, name))
        {
            return;
        }
        let loop_label = label.as_ref().map(|(name, _)| name.as_str());
        if !state_loop_can_reach_next_turn(body, loop_label) {
            return;
        }
        let diagnostic = self.attach_decision_row(
            Diagnostic::from_row("L0529", &[], Some(init.name_span)),
            MirDecisionKind::LoopInvariant,
            MirDecisionDisposition::Rejected,
            init.name_span,
            "state-loop-unchanged",
            "the state-loop condition reads unchanged header state before another turn",
        );
        self.diags.push(diagnostic);
    }

    fn is_unconditional_loop(&self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Loop { .. } => {
                crate::Sema::Diagnostics::block_definitely_exits(std::slice::from_ref(stmt))
            }
            Stmt::While { cond, .. } | Stmt::CountedLoop { cond, .. }
                if self.constant_bool(cond) == Some(true) =>
            {
                crate::Sema::Diagnostics::block_definitely_exits(std::slice::from_ref(stmt))
            }
            _ => false,
        }
    }

    fn constant_bool(&self, expr: &Expr) -> Option<bool> {
        if let Expr::Bool(value, _) = expr.without_parens() {
            return Some(*value);
        }
        match self.evaluate_constant(expr) {
            Some(CtValue::Bool(value)) => Some(value),
            _ => None,
        }
    }
    fn loop_condition_header_span(&self, stmt: &Stmt, condition: Span) -> Option<Span> {
        let statement = stmt.span();
        let prefix = self.source.get(statement.start..condition.start)?;
        let loop_keyword = prefix.rfind("loop");
        let while_keyword = prefix.rfind("while");
        let keyword = match (loop_keyword, while_keyword) {
            (Some(loop_keyword), Some(while_keyword)) => loop_keyword.max(while_keyword),
            (Some(keyword), None) | (None, Some(keyword)) => keyword,
            (None, None) => return None,
        };
        Some(Span::new(statement.start + keyword, condition.end))
    }

    fn current_stmt_tail(&self) -> &[Stmt] {
        if self.stmt_tail_ptr.is_null() || self.stmt_tail_len == 0 {
            return &[];
        }
        // SAFETY: `check_block_inner` sets this pointer and length from the
        // current block's `stmts[i + 1..]`; the AST outlives this checker and
        // this helper only reads it while that statement is being checked.
        unsafe { std::slice::from_raw_parts(self.stmt_tail_ptr, self.stmt_tail_len) }
    }

    fn statement_lint_is_allowed(&self, target: Span, lint_name: &str) -> bool {
        self.rule_facts.iter().any(|application| {
            if application.marker.name != crate::Syntax::MARKER_ALLOW
                || !matches!(
                    application.site,
                    Some(crate::Policy::RuleSite::Block | crate::Policy::RuleSite::Statement)
                )
            {
                return false;
            }
            let target_matches = application.target == Some(target)
                || (application.target.is_none()
                    && application.marker.span.start <= target.start.saturating_add(1)
                    && target.start <= application.marker.span.start);
            target_matches
                && application
                    .marker
                    .args
                    .iter()
                    .filter_map(crate::AST::MarkerCallArg::as_expr)
                    .any(|argument| {
                        matches!(argument, Expr::Ident(name, _) if name == lint_name)
                    })
        })
    }
}

fn state_loop_can_reach_next_turn(body: &[Stmt], label: Option<&str>) -> bool {
    if !crate::Sema::Diagnostics::block_definitely_exits(body) {
        return true;
    }
    // `continue` is an exit from the current body, but it still reaches the
    // next turn. Keep it distinct from an unconditional `break`/`return`.
    for statement in body {
        if statement_contains_current_continue(statement, label) {
            return true;
        }
        if crate::Sema::Diagnostics::block_definitely_exits(std::slice::from_ref(statement)) {
            return false;
        }
    }
    false
}

fn body_contains_current_continue(body: &[Stmt], label: Option<&str>) -> bool {
    body.iter()
        .any(|stmt| statement_contains_current_continue(stmt, label))
}

fn statement_contains_current_continue(stmt: &Stmt, label: Option<&str>) -> bool {
    match stmt {
        Stmt::Continue(_) => true,
        Stmt::ContinueLabel(name, _) => label == Some(name.as_str()),
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            arms.iter()
                .any(|arm| body_contains_current_continue(&arm.body, label))
                || else_body
                    .as_deref()
                    .is_some_and(|body| body_contains_current_continue(body, label))
        }
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            body_contains_current_continue(then_body, label)
                || else_body
                    .as_deref()
                    .is_some_and(|body| body_contains_current_continue(body, label))
        }
        Stmt::Unsafe { body, .. }
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
        | Stmt::Transact { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::ScopeMember { body, .. } => body_contains_current_continue(body, label),
        // A nested loop owns its unlabelled `next`; do not mistake it for an
        // iteration of this state loop. Other statements cannot contain the
        // current loop's control transfer.
        _ => false,
    }
}

fn statement_writes_name(stmt: &Stmt, name: &str) -> bool {
    let direct = match stmt {
        Stmt::Assign { target, .. } => lvalue_writes_name(target, name),
        _ => false,
    };
    if direct || nested_direct_assignment(stmt, name) {
        return true;
    }
    let mut writes = false;
    stmt.for_each_expr(|expr| {
        writes |= expression_writes_name(expr, name);
    });
    writes
}

fn expression_writes_name(expr: &Expr, name: &str) -> bool {
    match expr.without_parens() {
        Expr::IncDec { operand, .. } => expr_root_name(operand) == Some(name),
        Expr::Place(inner, PlaceAccess::Write, _) => expr_root_name(inner) == Some(name),
        Expr::Call(call) => call.args.iter().any(|arg| {
            arg.convention == AccessConvention::Write
                && expr_root_name(&arg.expr) == Some(name)
        }),
        Expr::CallValue { args, .. } => args.iter().any(|arg| {
            arg.convention == AccessConvention::Write
                && expr_root_name(&arg.expr) == Some(name)
        }),
        Expr::MethodCall {
            receiver, args, ..
        } => {
            expr_root_name(receiver) == Some(name)
                || args.iter().any(|arg| {
                    arg.convention == AccessConvention::Write
                        && expr_root_name(&arg.expr) == Some(name)
                })
        }
        Expr::Lambda(lambda) => match &lambda.body {
            LambdaBody::Expr(body) => expression_writes_name(body, name),
            LambdaBody::Block(body) => body.iter().any(|stmt| statement_writes_name(stmt, name)),
        },
        Expr::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.iter().any(|stmt| statement_writes_name(stmt, name))
                || else_body.iter().any(|stmt| statement_writes_name(stmt, name))
        }
        Expr::OrFallback {
            value, fallback, ..
        } => expression_writes_name(value, name) || fallback_writes_name(fallback, name),
        _ => false,
    }
}

fn fallback_writes_name(fallback: &crate::AST::OrFallback, name: &str) -> bool {
    match fallback {
        crate::AST::OrFallback::Value(value) => expression_writes_name(value, name),
        crate::AST::OrFallback::Block { body, value, .. } => {
            body.iter().any(|stmt| statement_writes_name(stmt, name))
                || value
                    .as_deref()
                    .is_some_and(|value| expression_writes_name(value, name))
        }
        crate::AST::OrFallback::Return(Some(value), _) => expression_writes_name(value, name),
        crate::AST::OrFallback::Panic { args, .. } => args.iter().any(|arg| {
            arg.convention == AccessConvention::Write
                && expr_root_name(&arg.expr) == Some(name)
        }),
        crate::AST::OrFallback::Return(None, _)
        | crate::AST::OrFallback::Break(_)
        | crate::AST::OrFallback::Continue(_)
        | crate::AST::OrFallback::BreakLabel(..)
        | crate::AST::OrFallback::ContinueLabel(..) => false,
    }
}

fn nested_direct_assignment(stmt: &Stmt, name: &str) -> bool {
    fn body_writes(body: &[Stmt], name: &str) -> bool {
        body.iter().any(|stmt| statement_writes_name(stmt, name))
    }
    match stmt {
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
        | Stmt::AuthorityScope { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::Layout { body, .. } => body_writes(body, name),
        Stmt::CountedLoop { step, body, .. } => {
            step.as_deref()
                .is_some_and(|step| statement_writes_name(step, name))
                || body_writes(body, name)
        }
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            arms.iter().any(|arm| body_writes(&arm.body, name))
                || else_body
                    .as_deref()
                    .is_some_and(|body| body_writes(body, name))
        }
        Stmt::TaskGroup { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::ScopeMember { body, .. } => body_writes(body, name),
        Stmt::ComptimeIf {
            then_body,
            else_body,
            ..
        } => {
            body_writes(then_body, name)
                || else_body
                    .as_deref()
                    .is_some_and(|body| body_writes(body, name))
        }
        _ => false,
    }
}


fn lvalue_writes_name(target: &LValue, name: &str) -> bool {
    match target {
        LValue::Local { name: candidate, .. } => candidate == name,
        LValue::Field { base, .. } | LValue::Index { base, .. } => {
            expr_root_name(base) == Some(name)
        }
    }
}

fn expr_root_name(expr: &Expr) -> Option<&str> {
    crate::Sema::Diagnostics::expr_root_ident(expr.without_parens())
}
