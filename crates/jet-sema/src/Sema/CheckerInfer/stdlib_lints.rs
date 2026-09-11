//! Semantic guidance for the two block-shaped standard-library replacements.
//!
//! Expression-shaped guidance lives beside the expression inference that
//! proves its types. These rules need the surrounding statement sequence, so
//! they use the same sema checker but keep their structural probes here.

mod display_lint;
mod loop_lints;

pub(crate) use display_lint::{
    debug_interpolation_edit, explicit_copy_edit, is_display_migration_candidate,
};

use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::Checker;
use std::collections::HashSet;
use crate::AST::{Expr, ForKind, LValue, Stmt, StrPart, Type};

impl<'a> Checker<'a> {
    /// Precompute a complete ASCII ladder before checking its first statement.
    /// The candidate is keyed by the binding statement so a statement-local
    /// `#allow(complete_ascii_case_ladder)` is already active when it emits.
    pub(crate) fn prepare_stdlib_lint_block(&mut self, stmts: &[Stmt]) {
        let mut ascii_lower_landed = None;
        let mut ascii_upper_landed = None;
        let mut replace_landed = None;
        for index in 0..stmts.len() {
            let Some((binding, lower, last_end)) = ascii_ladder(stmts, index) else {
                continue;
            };
            let direction_landed = if lower {
                *ascii_lower_landed.get_or_insert_with(|| {
                    crate::Collections::builtin_method_return(
                        &Type::String,
                        "to_ascii_lower",
                        0,
                        false,
                    ) == Some(Some(Type::String))
                })
            } else {
                *ascii_upper_landed.get_or_insert_with(|| {
                    crate::Collections::builtin_method_return(
                        &Type::String,
                        "to_ascii_upper",
                        0,
                        false,
                    ) == Some(Some(Type::String))
                })
            };
            if !direction_landed {
                continue;
            }
            if !*replace_landed.get_or_insert_with(|| {
                crate::Collections::builtin_method_return(&Type::String, "replace", 2, false)
                    == Some(Some(Type::String))
            }) {
                continue;
            }
            let direction = if lower {
                "to_ascii_lower"
            } else {
                "to_ascii_upper"
            };
            let Some(prefix) = self
                .source
                .get(binding.name_span.start..binding.init.span().start)
            else {
                continue;
            };
            let Some(init) = self
                .source
                .get(binding.init.span().start..binding.init.span().end)
            else {
                continue;
            };
            let edit = TextEdit {
                span: Span::new(binding.name_span.start, last_end),
                new_text: format!("{prefix}{init}.{direction}()"),
            };
            self.stdlib_lint_candidates
                .insert(binding.name_span.start, (binding.name_span, edit));
        }
    }

    /// Emit block-shaped guidance after marker facts have been consumed. This
    /// ordering preserves the existing statement-scoped `#allow` behavior.
    pub(crate) fn emit_stdlib_lints_for_stmt(&mut self, stmt: &Stmt) {
        let typed_mutable_string = matches!(
            stmt,
            Stmt::Val(binding)
                if binding.mutable
                    && self
                        .lookup(&binding.name)
                        .is_some_and(|info| info.ty == Type::String)
        );
        if typed_mutable_string && self.flow.reachable {
            if let Some((span, edit)) = self.stdlib_lint_candidates.remove(&stmt.span().start) {
                self.diags
                    .push(Diagnostic::from_row("L0521", &[], Some(span)).with_edit(edit));
            }
        }

        if let Stmt::For {
            var,
            var2: None,
            kind: ForKind::In { collection, .. },
            body,
            ..
        } = stmt
        {
            self.check_stdlib_walk_filter(var, collection, body);
        }
    }

    fn check_stdlib_walk_filter(&mut self, var: &str, collection: &Expr, body: &[Stmt]) {
        if !self.flow.reachable {
            return;
        }
        let Some((receiver, method_span)) = direct_fs_walk(self, collection) else {
            return;
        };
        let Some(filter) = body.first() else {
            return;
        };
        // A matching filter after the first statement was already rejected;
        // checking only the first statement avoids a second body walk.
        if !is_directory_filter(filter, var) {
            return;
        }
        let mut path_uses = 0usize;
        for stmt in body.iter().skip(1) {
            let facts = stmt_lint_facts(stmt, var);
            if facts.has_control_flow(stmt) || !facts.uses_only_path() {
                return;
            }
            path_uses += facts.path_uses;
        }
        if path_uses == 0 {
            return;
        }
        let Some(edit) = walk_files_edit(self, receiver, method_span) else {
            return;
        };
        self.diags
            .push(Diagnostic::from_row("L0522", &[], Some(edit.span)).with_edit(edit));
    }
}
fn direct_fs_walk<'a>(checker: &Checker<'_>, expr: &'a Expr) -> Option<(&'a Expr, Span)> {
    match expr.without_parens() {
        Expr::OrFallback { value, .. } | Expr::Try(value, ..) => {
            return direct_fs_walk(checker, value);
        }
        _ => {}
    }
    let Expr::MethodCall {
        receiver,
        method,
        method_span,
        args,
        ..
    } = expr.without_parens()
    else {
        return None;
    };
    (method == "walk"
        && matches!(args.len(), 1 | 2)
        && args.iter().enumerate().all(|(index, arg)| {
            let label_ok = if index == 1 {
                matches!(
                    arg.label.as_ref().map(|(label, _)| label.as_str()),
                    None | Some("ignore")
                )
            } else {
                arg.label.is_none()
            };
            !arg.spread && label_ok
        })
        && checker
            .core_module_path_from_receiver(receiver)
            .is_some_and(|(module, alias, _)| module == "core.files" && alias == "fs")
        && super::expr::landed_core_call("core.files", "walk", 2)
        && super::expr::landed_core_call("core.files", "walk_files", 2))
    .then_some((receiver.as_ref(), *method_span))
}

fn walk_files_edit(checker: &Checker<'_>, receiver: &Expr, method_span: Span) -> Option<TextEdit> {
    let start = crate::Sema::source_expr_start(receiver);
    let end = crate::Sema::source_call_end(checker.source, method_span)?;
    let prefix = checker.source.get(start..method_span.start)?;
    let suffix = checker.source.get(method_span.end..end)?;
    Some(TextEdit {
        span: Span::new(start, end),
        new_text: format!("{prefix}walk_files{suffix}"),
    })
}

fn is_directory_filter(stmt: &Stmt, var: &str) -> bool {
    let Stmt::Switch {
        subject,
        arms,
        else_body: None,
        span,
    } = stmt
    else {
        return false;
    };
    if !crate::AST::is_subjectless_guard(subject, *span) || arms.len() != 1 {
        return false;
    }
    let arm = &arms[0];
    matches!(
        arm.cond.without_parens(),
        Expr::Field(base, field, _)
            if field == "is_dir"
                && matches!(base.without_parens(), Expr::Ident(name, _) if name == var)
    ) && arm.body.len() == 1
        && matches!(arm.body[0], Stmt::Continue(_))
}

#[derive(Default)]
struct StmtLintFacts {
    path_uses: usize,
    allowed_path_bases: HashSet<Span>,
    variable_uses: Vec<Span>,
    has_fallback_exit: bool,
}

impl StmtLintFacts {
    fn uses_only_path(&self) -> bool {
        self.variable_uses
            .iter()
            .all(|span| self.allowed_path_bases.contains(span))
    }

    fn has_control_flow(&self, stmt: &Stmt) -> bool {
        self.has_fallback_exit || stmt_has_structural_control_flow(stmt)
    }
}

fn stmt_lint_facts(stmt: &Stmt, var: &str) -> StmtLintFacts {
    let mut copy = stmt.clone();
    let mut facts = StmtLintFacts::default();
    scan_stmt_exprs(&mut copy, var, &mut facts);
    facts
}

fn scan_expr(expr: &mut Expr, var: &str, facts: &mut StmtLintFacts, check_fallback: bool) {
    expr.for_each_expr_mut(|nested| {
        let shape = nested.without_parens();
        if let Expr::Field(base, field, _) = shape {
            if field == "path" {
                if let Expr::Ident(name, span) = base.without_parens() {
                    if name == var {
                        facts.allowed_path_bases.insert(*span);
                        facts.path_uses += 1;
                    }
                }
            }
        }
        if let Expr::Ident(name, span) = shape {
            if name == var {
                facts.variable_uses.push(*span);
            }
        }
        if check_fallback && is_fallback_exit_expr(nested) {
            facts.has_fallback_exit = true;
        }
    });
}

fn scan_lvalue(lvalue: &mut LValue, var: &str, facts: &mut StmtLintFacts) {
    match lvalue {
        LValue::Local { .. } => {}
        LValue::Index { base, index, .. } => {
            scan_expr(base, var, facts, false);
            scan_expr(index, var, facts, false);
        }
        LValue::Field { base, .. } => scan_expr(base, var, facts, false),
    }
}

fn scan_for_kind(kind: &mut ForKind, var: &str, facts: &mut StmtLintFacts) {
    match kind {
        ForKind::Range {
            start, end, step, ..
        } => {
            scan_expr(start, var, facts, true);
            scan_expr(end, var, facts, true);
            if let Some(step) = step {
                scan_expr(step, var, facts, true);
            }
        }
        ForKind::In { collection, step } => {
            scan_expr(collection, var, facts, true);
            if let Some(step) = step {
                scan_expr(step, var, facts, true);
            }
        }
    }
}

fn scan_body(body: &mut [Stmt], var: &str, facts: &mut StmtLintFacts) {
    for stmt in body {
        scan_stmt_exprs(stmt, var, facts);
    }
}

fn scan_stmt_exprs(stmt: &mut Stmt, var: &str, facts: &mut StmtLintFacts) {
    match stmt {
        Stmt::Expr(expr) => scan_expr(expr, var, facts, true),
        Stmt::Val(binding) => scan_expr(&mut binding.init, var, facts, true),
        Stmt::Assign { target, value, .. } => {
            scan_lvalue(target, var, facts);
            scan_expr(value, var, facts, true);
        }
        Stmt::Return(value, ..) => {
            if let Some(value) = value {
                scan_expr(value, var, facts, true);
            }
        }
        Stmt::While { cond, body, .. } => {
            scan_expr(cond, var, facts, true);
            scan_body(body, var, facts);
        }
        Stmt::For { kind, body, .. } => {
            scan_for_kind(kind, var, facts);
            scan_body(body, var, facts);
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
            scan_expr(subject, var, facts, false);
            for arm in arms {
                scan_expr(&mut arm.cond, var, facts, true);
                scan_body(&mut arm.body, var, facts);
            }
            if let Some(body) = else_body {
                scan_body(body, var, facts);
            }
        }
        Stmt::BreakValue(value, ..) | Stmt::Yield(value, ..) => scan_expr(value, var, facts, true),
        Stmt::BreakLabelValue(_, _, value, ..) => scan_expr(value, var, facts, true),
        Stmt::Loop { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::Transact { body, .. } => scan_body(body, var, facts),
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            scan_expr(&mut init.init, var, facts, true);
            scan_expr(cond, var, facts, true);
            if let Some(step) = step {
                scan_stmt_exprs(step, var, facts);
            }
            scan_body(body, var, facts);
        }
        Stmt::Unsafe {
            audit_expr, body, ..
        } => {
            if let Some(audit_expr) = audit_expr {
                scan_expr(audit_expr, var, facts, false);
            }
            scan_body(body, var, facts);
        }
        Stmt::Impure {
            reason_expr, body, ..
        } => {
            if let Some(reason_expr) = reason_expr {
                scan_expr(reason_expr, var, facts, false);
            }
            scan_body(body, var, facts);
        }
        Stmt::TaskGroup { limit, body, .. } => {
            if let Some(limit) = limit {
                scan_expr(limit, var, facts, true);
            }
            scan_body(body, var, facts);
        }
        Stmt::Layout { body, .. } => scan_body(body, var, facts),
        Stmt::AuthorityScope { body, .. } => scan_body(body, var, facts),
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            scan_expr(cond, var, facts, true);
            scan_body(then_body, var, facts);
            if let Some(body) = else_body {
                scan_body(body, var, facts);
            }
        }
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, value, _) in fields {
                scan_expr(value, var, facts, false);
            }
            scan_body(body, var, facts);
        }
        Stmt::AssumeDet {
            reason_expr, body, ..
        } => {
            scan_expr(reason_expr, var, facts, false);
            scan_body(body, var, facts);
        }
        Stmt::ScopeMember { args, body, .. } => {
            for arg in args {
                scan_expr(arg, var, facts, false);
            }
            scan_body(body, var, facts);
        }
        Stmt::DeferClose { close, .. } => scan_expr(close, var, facts, true),
        Stmt::Break(..) | Stmt::Continue(..) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
    }
}

fn stmt_has_structural_control_flow(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(..)
        | Stmt::Break(..)
        | Stmt::BreakValue(..)
        | Stmt::Continue(..)
        | Stmt::BreakLabel(..)
        | Stmt::BreakLabelValue(..)
        | Stmt::ContinueLabel(..)
        | Stmt::Yield(..)
        | Stmt::While { .. }
        | Stmt::For { .. }
        | Stmt::Loop { .. }
        | Stmt::CountedLoop { .. }
        | Stmt::Switch { .. }
        | Stmt::ComptimeSwitch { .. }
        | Stmt::ComptimeIf { .. }
        | Stmt::TaskGroup { .. } => true,
        Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::ComptimeBlock { body, .. }
        | Stmt::ContextBlock { body, .. }
        | Stmt::AuthorityScope { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::ScopeMember { body, .. }
        | Stmt::Layout { body, .. } => body.iter().any(stmt_has_structural_control_flow),
        Stmt::Expr(..) | Stmt::Val(..) | Stmt::Assign { .. } | Stmt::DeferClose { .. } => false,
    }
}

fn is_fallback_exit_expr(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::OrFallback { .. } | Expr::Try(..) | Expr::Todo { .. }
    ) || matches!(
        expr,
        Expr::Call(crate::AST::Call { name, .. })
            if name == crate::Syntax::BUILTIN_PANIC
    )
}

fn ascii_ladder(stmts: &[Stmt], index: usize) -> Option<(&crate::AST::Binding, bool, usize)> {
    let Stmt::Val(binding) = stmts.get(index)? else {
        return None;
    };
    if binding.name.is_empty() || binding.pattern.is_some() || binding.is_comptime {
        return None;
    }
    let mut lower = None;
    let mut last_end = binding.init.span().end;
    for offset in 0..26usize {
        let Stmt::Assign {
            target: LValue::Local { name, .. },
            op: None,
            value,
            ..
        } = stmts.get(index + offset + 1)?
        else {
            return None;
        };
        if name != &binding.name {
            return None;
        }
        let (from, to) = ascii_replace(value, &binding.name)?;
        let upper = char::from(b'A' + offset as u8);
        let lower_char = char::from(b'a' + offset as u8);
        let this_lower = if from == upper && to == lower_char {
            true
        } else if from == lower_char && to == upper {
            false
        } else {
            return None;
        };
        if let Some(previous) = lower {
            if previous != this_lower {
                return None;
            }
        } else {
            lower = Some(this_lower);
        }
        last_end = value.span().end;
    }
    Some((binding, lower?, last_end))
}

fn ascii_replace(expr: &Expr, binding: &str) -> Option<(char, char)> {
    let Expr::MethodCall {
        receiver,
        method,
        args,
        ..
    } = expr.without_parens()
    else {
        return None;
    };
    if method != "replace"
        || !matches!(receiver.without_parens(), Expr::Ident(name, _) if name == binding)
        || args.len() != 2
        || args.iter().any(|arg| arg.label.is_some() || arg.spread)
    {
        return None;
    }
    Some((
        single_literal(&args[0].expr)?,
        single_literal(&args[1].expr)?,
    ))
}

fn single_literal(expr: &Expr) -> Option<char> {
    let Expr::Str(parts, _) = expr.without_parens() else {
        return None;
    };
    let [StrPart::Lit(text)] = parts.as_slice() else {
        return None;
    };
    let mut chars = text.chars();
    let character = chars.next()?;
    chars.next().is_none().then_some(character)
}
