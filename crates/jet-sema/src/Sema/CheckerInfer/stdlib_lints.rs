//! Semantic guidance for the two block-shaped standard-library replacements.
//!
//! Expression-shaped guidance lives beside the expression inference that
//! proves its types. These rules need the surrounding statement sequence, so
//! they use the same sema checker but keep their structural probes here.

use crate::AST::{Expr, ForKind, LValue, Stmt, StrPart, Type};
use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::Checker;
use std::collections::HashSet;

impl<'a> Checker<'a> {
    /// Precompute a complete ASCII ladder before checking its first statement.
    /// The candidate is keyed by the binding statement so a statement-local
    /// `#allow(complete_ascii_case_ladder)` is already active when it emits.
    pub(crate) fn prepare_stdlib_lint_block(&mut self, stmts: &[Stmt]) {
        for index in 0..stmts.len() {
            let Some((binding, lower, last_end)) = ascii_ladder(stmts, index) else {
                continue;
            };
            let direction = if lower { "to_ascii_lower" } else { "to_ascii_upper" };
            if crate::Collections::builtin_method_return(&Type::String, direction, 0, false)
                != Some(Some(Type::String))
                || crate::Collections::builtin_method_return(&Type::String, "replace", 2, false)
                    != Some(Some(Type::String))
            {
                continue;
            }
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
        let Some(filter_index) = body
            .iter()
            .enumerate()
            .find_map(|(index, stmt)| is_directory_filter(stmt, var).then_some(index))
        else {
            return;
        };
        // The filter must run before any other loop-visible work. Replacing
        // the source walk with a file-only walk would otherwise suppress that
        // earlier work for directory entries.
        if filter_index != 0 {
            return;
        }
        if body
            .iter()
            .enumerate()
            .any(|(index, stmt)| index != filter_index && stmt_has_control_flow(stmt))
        {
            return;
        }
        let mut path_uses = 0usize;
        if body.iter().enumerate().any(|(index, stmt)| {
            index != filter_index && {
                let (ok, uses) = stmt_uses_only_path(stmt, var);
                path_uses += uses;
                !ok
            }
        }) || path_uses == 0
        {
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
        && args.len() == 1
        && args[0].label.is_none()
        && !args[0].spread
        && checker
            .core_module_path_from_receiver(receiver)
            .is_some_and(|(module, alias, _)| module == "core.files" && alias == "fs")
        && super::expr::landed_core_call("core.files", "walk", 1)
        && super::expr::landed_core_call("core.files", "walk_files", 1))
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

fn stmt_uses_only_path(stmt: &Stmt, var: &str) -> (bool, usize) {
    let mut allowed = HashSet::new();
    let mut path_uses = 0usize;
    stmt.for_each_expr(|expr| {
        expr.for_each_expr(|nested| {
            if let Expr::Field(base, field, _) = nested.without_parens() {
                if field == "path"
                    && matches!(
                        base.without_parens(),
                        Expr::Ident(name, span) if name == var && { allowed.insert(*span); true }
                    )
                {
                    path_uses += 1;
                }
            }
        });
    });
    let mut ok = true;
    stmt.for_each_expr(|expr| {
        expr.for_each_expr(|nested| {
            if let Expr::Ident(name, span) = nested.without_parens() {
                if name == var && !allowed.contains(span) {
                    ok = false;
                }
            }
        });
    });
    (ok, path_uses)
}

fn stmt_has_early_exit(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(..)
        | Stmt::Break(..)
        | Stmt::BreakValue(..)
        | Stmt::Continue(..)
        | Stmt::BreakLabel(..)
        | Stmt::BreakLabelValue(..)
        | Stmt::ContinueLabel(..)
        | Stmt::Yield(..) => true,
        Stmt::Expr(expr) => expr_has_fallback_exit(expr),
        Stmt::Val(binding) => expr_has_fallback_exit(&binding.init),
        Stmt::Assign { value, .. } => expr_has_fallback_exit(value),
        Stmt::While { .. }
        | Stmt::For { .. }
        | Stmt::Loop { .. }
        | Stmt::CountedLoop { .. } => true,
        Stmt::Switch {
            arms, else_body, ..
        }
        | Stmt::ComptimeSwitch {
            arms, else_body, ..
        } => {
            arms.iter().any(|arm| {
                expr_has_fallback_exit(&arm.cond) || body_has_early_exit(&arm.body)
            }) || else_body
                .as_deref()
                .is_some_and(body_has_early_exit)
        }
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            expr_has_fallback_exit(cond)
                || body_has_early_exit(then_body)
                || else_body
                    .as_deref()
                    .is_some_and(body_has_early_exit)
        }
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
        | Stmt::ScopeMember { body, .. } => body_has_early_exit(body),
        Stmt::TaskGroup { body, limit, .. } => {
            limit.as_ref().is_some_and(expr_has_fallback_exit) || body_has_early_exit(body)
        }
        Stmt::Layout { body, .. } => body_has_early_exit(body),
        Stmt::DeferClose { close, .. } => expr_has_fallback_exit(close),
    }
}

fn stmt_has_control_flow(stmt: &Stmt) -> bool {
    if stmt_has_early_exit(stmt) {
        return true;
    }
    match stmt {
        Stmt::While { .. }
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
        | Stmt::Layout { body, .. } => body.iter().any(stmt_has_control_flow),
        _ => false,
    }
}

fn body_has_early_exit(body: &[Stmt]) -> bool {
    body.iter().any(stmt_has_early_exit)
}

fn expr_has_fallback_exit(expr: &Expr) -> bool {
    let mut found = false;
    expr.for_each_expr(|nested| {
        if matches!(nested, Expr::OrFallback { .. } | Expr::Try(..) | Expr::Todo { .. })
            || matches!(
                nested,
                Expr::Call(crate::AST::Call { name, .. })
                    if name == crate::Syntax::BUILTIN_PANIC
            )
        {
            found = true;
        }
    });
    found
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
    Some((single_literal(&args[0].expr)?, single_literal(&args[1].expr)?))
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
