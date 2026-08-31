use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::Captures::stmt_refs_name;
use crate::Sema::Checker;
use crate::Sema::Diagnostics::block_definitely_returns;
use crate::Syntax;
use crate::AST::{AccessConvention, BinOp, Expr, LValue, Stmt, StrPart, Type};
impl<'a> Checker<'a> {
    // --- statements -----------------------------------------------------

    pub(crate) fn check_block(&mut self, stmts: &mut [Stmt], new_scope: bool) {
        self.check_block_inner(stmts, new_scope, None);
    }

    /// Check a block whose final unadorned expression is its value. The AST
    /// keeps that expression as `Stmt::Expr`; the shared return-value checker
    /// validates its ownership, views, fallibility, and expected-type
    /// conversions without changing the source node's statement kind.
    pub(crate) fn check_value_block(
        &mut self,
        stmts: &mut [Stmt],
        expected: &Type,
        new_scope: bool,
        block_span: Span,
    ) {
        self.check_block_inner(stmts, new_scope, Some((expected, block_span)));
    }

    fn check_block_inner(
        &mut self,
        stmts: &mut [Stmt],
        new_scope: bool,
        value_tail: Option<(&Type, Span)>,
    ) {
        let redundant_tail_span =
            value_tail.and_then(|_| Self::redundant_arm_table_parts(stmts).map(|(_, span, _, _)| span));
        if redundant_tail_span.is_some() {
            self.check_redundant_arm_table_return(stmts);
        }
        self.check_adjacent_subject_dispatch_lint(stmts);
        self.prepare_stdlib_lint_block(stmts);
        if new_scope {
            self.push_scope();
        }
        // E0209 liveness gate: before checking each statement,
        // record the tail of the current block (statements that follow it).
        // The helper `is_name_live_after` reads this to word the E0209 fix
        // menu (move vs. copy/reorder). We push the previous frame onto the
        // liveness_frames stack so `is_name_live_after` can walk enclosing
        // scopes; on exit we pop and restore.
        let saved_ptr = self.stmt_tail_ptr;
        let saved_len = self.stmt_tail_len;
        // Push the caller's frame as an enclosing scope (non-null only).
        let pushed_frame = !saved_ptr.is_null();
        if pushed_frame {
            self.liveness_frames.push((saved_ptr, saved_len));
        }
        for i in 0..stmts.len() {
            // tail = stmts[i+1..], i.e. the statements after index i.
            let tail = &stmts[i + 1..];
            self.stmt_tail_ptr = tail.as_ptr();
            self.stmt_tail_len = tail.len();
            self.views_used_in_stmt.clear();
            self.scoped_loan_read_reported = false;
            if i + 1 == stmts.len() {
                if let Some((expected, _block_span)) = value_tail {
                    self.check_value_tail(&mut stmts[i], expected);
                } else {
                    self.check_stmt(&mut stmts[i]);
                }
            } else {
                let diagnostics_start = self.diags.len();
                self.check_stmt(&mut stmts[i]);
                if i + 2 == stmts.len() {
                    if let Some(span) = redundant_tail_span {
                        let checked = self.diags.split_off(diagnostics_start);
                        self.diags.extend(checked.into_iter().filter(|diagnostic| {
                            !(diagnostic.code == "E0003" && diagnostic.span == Some(span))
                        }));
                    }
                }
            }
        }
        if stmts.is_empty() {
            if let Some((expected, block_span)) = value_tail {
                self.report_missing_block_value(expected, block_span);
            }
        }
        if pushed_frame {
            self.liveness_frames.pop();
        }
        self.stmt_tail_ptr = saved_ptr;
        self.stmt_tail_len = saved_len;
        if new_scope {
            self.pop_scope();
        }
    }

    fn check_value_tail(&mut self, stmt: &mut Stmt, expected: &Type) {
        if !self.flow.reachable {
            // Source after an earlier exit is still checked for its own
            // diagnostics, but cannot be the block's reachable value tail.
            self.check_stmt(stmt);
            return;
        }
        match stmt {
            Stmt::Expr(expr)
                if !self.tail_has_authored_semicolon(expr.span())
                    && !Self::is_diverging_tail(expr) =>
            {
                let span = expr.span();
                let mut value = Some(std::mem::replace(expr, Expr::Absent(span)));
                self.check_return_expr(&mut value, &span, Some(expected.clone()));
                if let Some(value) = value.as_mut() {
                    self.lift_value_tail_into_return_carrier(value, expected);
                }
                if let Some(value) = value {
                    *expr = value;
                }
            }
            Stmt::Expr(expr) if Self::is_diverging_tail(expr) => {
                // Diverging expressions such as `panic(...)` and `todo` do
                // not need to produce the promised value.
                self.check_stmt(stmt);
            }
            Stmt::Expr(expr) => {
                let span = expr.span();
                self.check_stmt(stmt);
                self.report_missing_block_value(expected, span);
            }
            Stmt::Return(..) => {
                // An explicit return is an early exit. Its existing checker
                // owns both its value type and its divergence semantics.
                self.check_stmt(stmt);
            }
            _ => {
                let span = stmt.span();
                self.check_stmt(stmt);
                if self.flow.reachable && !block_definitely_returns(std::slice::from_ref(stmt)) {
                    self.report_missing_block_value(expected, span);
                }
            }
        }
    }

    fn report_missing_block_value(&mut self, expected: &Type, span: Span) {
        self.diags.push(Diagnostic::error(
            "E0114",
            format!(
                "this block promises to produce {}, but its final statement produces no value",
                expected.show()
            ),
            "a value-expected block must end with one unadorned expression; statements and semicolon-terminated expressions yield unit".to_string(),
            "move the expression to the final line without `;`, or add an explicit `return ...` for an early exit".to_string(),
            Some(span),
        ));
    }

    /// D-FAILURE-FOUNDATION1=A: named callable bodies are checked against
    /// their source success type, while the callable itself carries the
    /// shared `Result` rail. Lift that checked tail through the same `Ok`
    /// constructor used by an explicit `return` before lowering.
    fn lift_value_tail_into_return_carrier(&mut self, value: &mut Expr, expected: &Type) {
        if matches!(expected, Type::Result { .. })
            || !matches!(self.ret.as_ref(), Some(Type::Result { .. }))
            || matches!(value.without_parens(), Expr::Ok(..) | Expr::Err(..))
        {
            return;
        }
        let span = value.span();
        let inner = std::mem::replace(value, Expr::Absent(span));
        *value = Expr::Ok(Box::new(inner), span);
        let saved_expected = self.expected_type.clone();
        self.expected_type = self.ret.clone();
        self.infer(value);
        self.expected_type = saved_expected;
    }

    fn tail_has_authored_semicolon(&self, span: Span) -> bool {
        find_authored_semicolon(self.source, span.end).is_some()
    }

    fn is_diverging_tail(expr: &Expr) -> bool {
        matches!(expr.without_parens(), Expr::Todo { .. })
            || matches!(
                expr.without_parens(),
                Expr::Call(call) if call.name == Syntax::BUILTIN_PANIC
            )
    }

    /// L0514 / D-BRANCH-LINT1: adjacent classic guards over one stable
    /// categorical subject are one ordered table in disguise. The parser has
    /// already normalized each classic `if` to a subjectless `Stmt::Switch`;
    /// this pass only joins that existing AST shape and never scans source
    /// text to rediscover branch conditions.
    fn check_adjacent_subject_dispatch_lint(&mut self, stmts: &[Stmt]) {
        let mut index = 0;
        while index < stmts.len() {
            let Some(first) = Self::adjacent_dispatch_guard(&stmts[index], self.source) else {
                index += 1;
                continue;
            };
            let mut run = vec![first];
            let mut end = index + 1;
            while end < stmts.len() {
                let Some(next) = Self::adjacent_dispatch_guard(&stmts[end], self.source) else {
                    break;
                };
                if next.subject_key != run[0].subject_key {
                    break;
                }
                run.push(next);
                end += 1;
            }
            if run.len() >= 3
                && Self::dispatch_values_are_exclusive(&run)
                && !run
                    .iter()
                    .any(|guard| {
                        Self::statements_write_subject(guard.body, &guard.subject_dependencies)
                    })
            {
                let span = run[0].span;
                let subject = self
                    .source
                    .get(run[0].subject_span.start..run[0].subject_span.end)
                    .map(str::trim)
                    .filter(|subject| !subject.is_empty())
                    .map_or_else(|| run[0].subject.join("."), str::to_string);
                let mut diagnostic =
                    Diagnostic::from_row("L0514", &[("subject", subject.as_str())], Some(span));
                if let Some(edit) = self.adjacent_dispatch_edit(&run) {
                    diagnostic = diagnostic.with_edit(edit);
                }
                self.diags.push(diagnostic);
                index = end;
            } else {
                index += 1;
            }
        }
    }

    fn adjacent_dispatch_guard<'b>(
        stmt: &'b Stmt,
        source: &str,
    ) -> Option<AdjacentDispatchGuard<'b>> {
        let Stmt::Switch {
            subject,
            arms,
            else_body: None,
            span,
            ..
        } = stmt
        else {
            return None;
        };
        if !crate::AST::is_subjectless_guard(subject, *span)
            || arms.len() != 1
            || !crate::AST::uses_classic_if_spelling(source, *span, arms[0].cond.span())
        {
            return None;
        }
        let condition_span = arms[0].cond.span();
        let (
            subject_path,
            subject_span,
            subject_key,
            subject_dependencies,
            values,
        ) = Self::dispatch_condition_values(&arms[0].cond, source)?;
        Some(AdjacentDispatchGuard {
            subject: subject_path,
            subject_key,
            subject_dependencies,
            subject_span,
            values,
            body: &arms[0].body,
            condition_span,
            span: *span,
        })
    }

    fn dispatch_condition_values(
        condition: &Expr,
        source: &str,
    ) -> Option<(
        Vec<String>,
        Span,
        String,
        Vec<Vec<String>>,
        Vec<DispatchValue>,
    )> {
        fn collect(
            condition: &Expr,
            source: &str,
            subject: &mut Option<DispatchSubject>,
            values: &mut Vec<DispatchValue>,
        ) -> bool {
            match condition.without_parens() {
                Expr::Binary(BinOp::Or, left, right, _) => {
                    collect(left, source, subject, values)
                        && collect(right, source, subject, values)
                }
                Expr::Binary(BinOp::Eq, left, right, _) => {
                    let left_subject = Checker::dispatch_subject(left, source);
                    let right_subject = Checker::dispatch_subject(right, source);
                    let (subject_expr, value_expr) = match (
                        left_subject.is_some(),
                        right_subject.is_some(),
                    ) {
                        (true, false) => (left, right),
                        (false, true) => (right, left),
                        _ => return false,
                    };
                    let Some(current) = Checker::dispatch_subject(subject_expr, source) else {
                        return false;
                    };
                    let Some(value) = Checker::dispatch_literal(value_expr) else {
                        return false;
                    };
                    if let Some(existing) = subject.as_ref() {
                        if existing.key != current.key {
                            return false;
                        }
                    } else {
                        *subject = Some(current);
                    }
                    values.push(value);
                    true
                }
                _ => false,
            }
        }

        let mut subject = None;
        let mut values = Vec::new();
        if !collect(condition, source, &mut subject, &mut values) || values.is_empty() {
            return None;
        }
        let subject = subject?;
        if values
            .iter()
            .any(|value| value.kind != values[0].kind)
        {
            return None;
        }
        Some((
            subject.path,
            subject.span,
            subject.key,
            subject.dependencies,
            values,
        ))
    }

    fn dispatch_subject(expr: &Expr, source: &str) -> Option<DispatchSubject> {
        let source_span = expr.span();
        let expr = expr.without_parens();
        if !Self::dispatch_subject_is_call_free(expr) {
            return None;
        }
        let span = expr.span();
        let path = Self::dispatch_subject_path(expr).unwrap_or_default();
        let key = if path.is_empty() {
            Self::dispatch_subject_key(source, span)?
        } else {
            path.join(".")
        };
        let dependencies = Self::dispatch_subject_dependencies(expr);
        if dependencies.is_empty() {
            return None;
        }
        Some(DispatchSubject {
            path,
            // Keep authored parentheses in the replacement. The normalized
            // span above is only for subject identity; using it as source
            // text could change precedence for expressions such as
            // `(left && right)`.
            span: source_span,
            key,
            dependencies,
        })
    }

    fn dispatch_subject_is_call_free(expr: &Expr) -> bool {
        let mut call_free = true;
        expr.for_each_expr(|nested| {
            if matches!(
                nested.without_parens(),
                Expr::Call(_)
                    | Expr::MethodCall { .. }
                    | Expr::CallValue { .. }
                    | Expr::Lambda(_)
                    | Expr::Try(..)
                    | Expr::OrFallback { .. }
                    | Expr::If { .. }
                    | Expr::IncDec { .. }
                    | Expr::Place(..)
                    | Expr::Deref(..)
                    | Expr::RawOf(..)
                    | Expr::PtrFromAddr { .. }
            ) {
                call_free = false;
            }
        });
        call_free
    }

    fn dispatch_subject_key(source: &str, span: Span) -> Option<String> {
        let (tokens, _) = crate::Lexer::lex(source);
        let mut key = String::new();
        for token in tokens {
            if token.span.start < span.start || token.span.end > span.end {
                continue;
            }
            if matches!(
                &token.kind,
                crate::Lexer::TokKind::LineComment(_)
                    | crate::Lexer::TokKind::BlockComment(_)
                    | crate::Lexer::TokKind::Eof
            ) {
                continue;
            }
            key.push_str(&format!("{:?};", token.kind));
        }
        (!key.is_empty()).then_some(key)
    }

    fn dispatch_subject_dependencies(expr: &Expr) -> Vec<Vec<String>> {
        let mut paths = Vec::new();
        expr.for_each_expr(|nested| {
            if let Some(path) = Self::dispatch_subject_path(nested) {
                paths.push(path);
            }
        });
        paths.sort_by_key(|path| std::cmp::Reverse(path.len()));
        paths.dedup();
        let longest_paths = paths.clone();
        paths.retain(|path| {
            !longest_paths
                .iter()
                .any(|other| other.len() > path.len() && other.starts_with(path))
        });
        paths
    }

    fn dispatch_subject_path(expr: &Expr) -> Option<Vec<String>> {
        match expr.without_parens() {
            Expr::Ident(name, _) if !name.starts_with('\0') => Some(vec![name.clone()]),
            Expr::Field(base, field, _) => {
                let mut path = Self::dispatch_subject_path(base)?;
                path.push(field.clone());
                Some(path)
            }
            _ => None,
        }
    }

    fn dispatch_literal(expr: &Expr) -> Option<DispatchValue> {
        let expr = expr.without_parens();
        let (kind, key, span) = match expr {
            Expr::Str(parts, span) => {
                let [StrPart::Lit(value)] = parts.as_slice() else {
                    return None;
                };
                (DispatchLiteralKind::String, value.clone(), *span)
            }
            Expr::Int(value, span, ..) => (DispatchLiteralKind::Int, value.to_string(), *span),
            Expr::Bool(value, span) => (DispatchLiteralKind::Bool, value.to_string(), *span),
            Expr::Char(value, span) => (DispatchLiteralKind::Char, value.to_string(), *span),
            Expr::EnumLit {
                type_name,
                variant,
                args,
                span,
                ..
            } if args.is_empty() => (
                DispatchLiteralKind::Enum,
                format!("{type_name}.{variant}"),
                *span,
            ),
            _ => return None,
        };
        Some(DispatchValue { kind, key, span })
    }

    fn dispatch_values_are_exclusive(run: &[AdjacentDispatchGuard<'_>]) -> bool {
        let mut seen = Vec::new();
        for guard in run {
            for value in &guard.values {
                if seen
                    .iter()
                    .any(|existing: &DispatchValue| existing.kind == value.kind && existing.key == value.key)
                {
                    return false;
                }
                seen.push(value.clone());
            }
        }
        true
    }

    fn statements_write_subject(stmts: &[Stmt], subject: &[Vec<String>]) -> bool {
        stmts
            .iter()
            .any(|stmt| Self::statement_writes_subject(stmt, subject))
    }

    fn statement_writes_subject(stmt: &Stmt, subject: &[Vec<String>]) -> bool {
        match stmt {
            Stmt::Assign { target, value, .. } => {
                Self::lvalue_writes_subject(target, subject)
                    || Self::expression_writes_subject(value, subject)
            }
            Stmt::Expr(expr)
            | Stmt::Return(Some(expr), _)
            | Stmt::BreakValue(expr, _)
            | Stmt::Yield(expr, _)
            | Stmt::BreakLabelValue(_, _, expr, _) => {
                Self::expression_writes_subject(expr, subject)
            }
            Stmt::Val(binding) => {
                Self::name_writes_subject(&binding.name, subject)
                    || Self::expression_writes_subject(&binding.init, subject)
            }
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
            | Stmt::Layout { body, .. }
            | Stmt::AuthorityScope { body, .. }
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::ScopeMember { body, .. } => Self::statements_write_subject(body, subject),
            Stmt::Switch {
                arms, else_body, ..
            }
            | Stmt::ComptimeSwitch {
                arms, else_body, ..
            } => {
                arms.iter()
                    .any(|arm| Self::statements_write_subject(&arm.body, subject))
                    || else_body
                        .as_ref()
                        .is_some_and(|body| Self::statements_write_subject(body, subject))
            }
            Stmt::CountedLoop {
                init, step, body, ..
            } => {
                Self::name_writes_subject(&init.name, subject)
                    || step
                        .as_deref()
                        .is_some_and(|stmt| Self::statement_writes_subject(stmt, subject))
                    || Self::statements_write_subject(body, subject)
            }
            Stmt::TaskGroup { limit, body, .. } => {
                limit
                    .as_ref()
                    .is_some_and(|expr| Self::expression_writes_subject(expr, subject))
                    || Self::statements_write_subject(body, subject)
            }
            Stmt::ContextBlock { fields, body, .. } => {
                fields.iter().any(|(_, value, _)| {
                    Self::expression_writes_subject(value, subject)
                }) || Self::statements_write_subject(body, subject)
            }
            Stmt::ComptimeIf {
                then_body,
                else_body,
                ..
            } => {
                Self::statements_write_subject(then_body, subject)
                    || else_body
                        .as_ref()
                        .is_some_and(|body| Self::statements_write_subject(body, subject))
            }
            Stmt::AssumeDet { body, .. } => Self::statements_write_subject(body, subject),
            Stmt::DeferClose { close, .. } => Self::expression_writes_subject(close, subject),
            Stmt::Return(None, _)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(..)
            | Stmt::ContinueLabel(..) => false,
        }
    }

    fn expression_writes_subject(expr: &Expr, subject: &[Vec<String>]) -> bool {
        let mut writes = false;
        expr.for_each_expr(|nested| match nested.without_parens() {
            Expr::IncDec { operand, .. } => {
                writes |= Self::expr_path(operand)
                    .is_some_and(|path| Self::path_writes_subject(&path, subject));
            }
            Expr::Place(inner, access, _) if *access == crate::AST::PlaceAccess::Write => {
                writes |= Self::expr_path(inner)
                    .is_some_and(|path| Self::path_writes_subject(&path, subject));
            }
            Expr::MethodCall {
                receiver, args, ..
            } => {
                writes |= Self::expr_path(receiver)
                    .is_some_and(|path| Self::path_writes_subject(&path, subject));
                writes |= Self::call_args_write_subject(args, subject);
            }
            Expr::Call(call) => {
                writes |= Self::call_args_write_subject(&call.args, subject);
            }
            Expr::CallValue { callee, args, .. } => {
                writes |= Self::expr_path(callee)
                    .is_some_and(|path| Self::path_writes_subject(&path, subject));
                writes |= Self::call_args_write_subject(args, subject);
            }
            Expr::OrFallback { fallback, .. } => {
                if let crate::AST::OrFallback::Block { body, .. } = fallback {
                    writes |= Self::statements_write_subject(body, subject);
                }
            }
            Expr::If {
                then_body,
                else_body,
                ..
            } => {
                writes |= Self::statements_write_subject(then_body, subject);
                writes |= Self::statements_write_subject(else_body, subject);
            }
            Expr::Lambda(lambda) => {
                if let crate::AST::LambdaBody::Block(body) = &lambda.body {
                    writes |= Self::statements_write_subject(body, subject);
                }
            }
            _ => {}
        });
        writes
    }

    fn call_args_write_subject(
        args: &[crate::AST::CallArg],
        subject: &[Vec<String>],
    ) -> bool {
        args.iter().any(|arg| {
            matches!(arg.convention, AccessConvention::Write | AccessConvention::Move)
                && Self::argument_path(&arg.expr)
                    .is_some_and(|path| Self::path_writes_subject(&path, subject))
        })
    }

    fn argument_path(expr: &Expr) -> Option<Vec<String>> {
        match expr.without_parens() {
            Expr::Place(inner, ..) => Self::expr_path(inner),
            _ => Self::expr_path(expr),
        }
    }

    fn expr_path(expr: &Expr) -> Option<Vec<String>> {
        Self::dispatch_subject_path(expr)
    }

    fn lvalue_writes_subject(target: &LValue, subject: &[Vec<String>]) -> bool {
        let path = match target {
            LValue::Local { name, .. } => vec![name.clone()],
            LValue::Field { base, field, .. } => {
                if Self::expression_writes_subject(base, subject) {
                    return true;
                }
                let Some(mut path) = Self::expr_path(base) else {
                    return false;
                };
                path.push(field.clone());
                path
            }
            LValue::Index { base, index, .. } => {
                if Self::expression_writes_subject(base, subject)
                    || Self::expression_writes_subject(index, subject)
                {
                    return true;
                }
                let Some(path) = Self::expr_path(base) else {
                    return false;
                };
                path
            }
        };
        Self::path_writes_subject(&path, subject)
    }

    fn name_writes_subject(name: &str, subject: &[Vec<String>]) -> bool {
        subject
            .iter()
            .any(|path| path.first().is_some_and(|root| root == name))
    }

    fn path_writes_subject(path: &[String], subject: &[Vec<String>]) -> bool {
        subject
            .iter()
            .any(|candidate| Self::paths_overlap(path, candidate))
    }

    fn paths_overlap(left: &[String], right: &[String]) -> bool {
        left.starts_with(right) || right.starts_with(left)
    }

    fn adjacent_dispatch_edit(
        &self,
        run: &[AdjacentDispatchGuard<'_>],
    ) -> Option<TextEdit> {
        let extents = run
            .iter()
            .map(|guard| {
                source_if_extent(self.source, guard.span, guard.condition_span, guard.body)
            })
            .collect::<Option<Vec<_>>>()?;
        let start = extents.first()?.start;
        let end = extents.last()?.end;
        if extents
            .windows(2)
            .any(|pair| pair[0].end > pair[1].start)
            || source_has_comment(self.source, start, end)
        {
            return None;
        }
        if extents.iter().zip(run).any(|(extent, guard)| {
            extent.braced && statements_contain_multiline_string(guard.body)
        }) {
            // The braced-body reindent below operates on source lines. A raw
            // multiline string is source data, not code indentation; decline
            // the edit rather than changing that data while still reporting
            // the advisory lint.
            return None;
        }
        let indent = source_line_indent(self.source, start);
        let subject = self
            .source
            .get(run[0].subject_span.start..run[0].subject_span.end)?
            .trim();
        if subject.is_empty() || subject.contains('\n') {
            return None;
        }
        // The statement span starts at `if`, after its existing line indent.
        // Keep that prefix outside the replacement so applying the edit does
        // not double-indent the whole rewritten table.
        let mut replacement = format!("if {subject} == {{");
        for (guard, extent) in run.iter().zip(extents.iter()) {
            replacement.push('\n');
            replacement.push_str(&indent);
            replacement.push_str("    ");
            for (index, value) in guard.values.iter().enumerate() {
                if index > 0 {
                    replacement.push_str(" | ");
                }
                replacement.push_str(
                    self.source
                        .get(value.span.start..value.span.end)?
                        .trim(),
                );
            }
            replacement.push_str(" ->");
            let body = self.source.get(extent.body_start..extent.body_end)?.trim();
            if body.is_empty() {
                replacement.push_str(" {}");
            } else if extent.braced {
                replacement.push_str(" {");
                for line in body.lines() {
                    if !line.trim().is_empty() {
                        replacement.push('\n');
                        replacement.push_str(&indent);
                        replacement.push_str("        ");
                        replacement.push_str(line.trim());
                    }
                }
                replacement.push('\n');
                replacement.push_str(&indent);
                replacement.push_str("    }");
            } else {
                replacement.push(' ');
                replacement.push_str(body);
            }
        }
        replacement.push('\n');
        replacement.push_str(&indent);
        replacement.push_str("    else -> {}");
        replacement.push('\n');
        replacement.push('}');
        let formatted = crate::Formatter::format_source(&replacement).ok()?;
        let mut indented = String::new();
        for (line_index, line) in formatted.trim_end_matches('\n').lines().enumerate() {
            if line_index > 0 {
                indented.push('\n');
                indented.push_str(&indent);
            }
            indented.push_str(line);
        }
        Some(TextEdit {
            span: Span::new(start, end),
            new_text: indented,
        })
    }

    /// by a default return is the old spelling of one value table. The probe
    /// is deliberately strict: every arm must be a direct return, the table
    /// must have no explicit fallback, and the default must be the block tail.
    /// The shared probe behind both L0513 and its E0003 suppression: the last
    /// two statements must be a fallback-free statement arm table whose arms
    /// all return directly, followed by a default return. Both callers must
    /// agree on what counts, so neither re-derives it.
    fn redundant_arm_table_parts(
        stmts: &[Stmt],
    ) -> Option<(&[crate::AST::SwitchArm], Span, &Expr, Span)> {
        let [
            ..,
            Stmt::Switch {
                subject,
                arms,
                else_body: None,
                span,
                ..
            },
            Stmt::Return(Some(default), default_return_span),
        ] = stmts
        else {
            return None;
        };
        // Subjectless guards (including readiness tables) are effect/control
        // tables, not value dispatch. Adding an `else` body would change the
        // statement shape instead of merely moving the fallback value into
        // the table, so the rewrite is not equivalent.
        if crate::AST::is_subjectless_guard(subject, *span)
            || arms
                .iter()
                .any(|arm| crate::AST::readiness_head(&arm.cond).is_some())
        {
            return None;
        }
        if arms.is_empty()
            || !arms
                .iter()
                .all(|arm| matches!(arm.body.as_slice(), [Stmt::Return(Some(_), _)]))
        {
            return None;
        }
        Some((arms, *span, default, *default_return_span))
    }

    fn check_redundant_arm_table_return(&mut self, stmts: &[Stmt]) {
        let Some((arms, span, default, default_return_span)) =
            Self::redundant_arm_table_parts(stmts)
        else {
            return;
        };
        let Some(edit) = self.redundant_arm_table_edit(arms, span, default, default_return_span)
        else {
            return;
        };
        let mut diagnostic = Diagnostic::from_row("L0513", &[], Some(span));
        diagnostic.set_structured_edit(edit);
        self.diags.push(diagnostic);
    }

    fn redundant_arm_table_edit(
        &self,
        arms: &[crate::AST::SwitchArm],
        switch_span: Span,
        default: &Expr,
        default_return_span: Span,
    ) -> Option<TextEdit> {
        let source = self.source;
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for arm in arms {
            let [Stmt::Return(Some(value), return_span)] = arm.body.as_slice() else {
                return None;
            };
            if return_span.start > return_span.end {
                return None;
            }
            // Remove only the keyword and an authored terminator. Any comment
            // or spacing around the value remains in the source edit.
            edits.push((return_span.start, return_span.end, String::new()));
            if let Some(semicolon) = find_authored_semicolon(source, value.span().end) {
                edits.push((semicolon, semicolon + 1, String::new()));
            }
        }

        let close = find_switch_close(source, switch_span.start)?;
        let default_text = source.get(default.span().start..default.span().end)?;
        let prefix =
            if close > switch_span.start && !source.as_bytes()[close - 1].is_ascii_whitespace() {
                " "
            } else {
                ""
            };
        edits.push((
            close,
            close,
            format!("{prefix}else -> {{ {default_text} }}"),
        ));

        edits.push((
            default_return_span.start,
            default_return_span.end,
            String::new(),
        ));
        edits.push((default.span().start, default.span().end, String::new()));

        let mut end = default.span().end;
        if let Some(semicolon) = find_authored_semicolon(source, end) {
            // The semicolon belongs to the removed fallback statement. Keep
            // any whitespace/comments around it, but do not leave a stray
            // statement terminator after the new value table.
            end = semicolon + 1;
            edits.push((semicolon, semicolon + 1, String::new()));
        }
        let start = switch_span.start;
        let mut replacement = source.get(start..end)?.to_string();
        edits.sort_by_key(|(edit_start, _, _)| *edit_start);
        for (edit_start, edit_end, new_text) in edits.into_iter().rev() {
            if edit_start < start || edit_end > end || edit_start > edit_end {
                return None;
            }
            replacement.replace_range(edit_start - start..edit_end - start, &new_text);
        }
        Some(TextEdit {
            span: Span::new(start, end),
            new_text: replacement,
        })
    }

    /// Check a body that may not run at all: a lambda or `task { … }` body,
    /// or a `??` fallback block. A `return` inside such a body still returns
    /// the enclosing function (that targeting is correct and the corpus
    /// depends on it), but only when the body runs — so it is a CONDITIONAL
    /// return, not an unconditional one. Statements after the construct stay
    /// reachable.
    ///
    /// Without this, `check_stmt` sees `flow.reachable == false` for every
    /// following statement and restores the pre-statement facts, discarding
    /// each later declaration and reporting it as `nothing named X exists
    /// here`. One `return` inside a task body buried its own real error under
    /// eleven phantom E0107/E0003 reports (card #2006).
    ///
    /// Inline bodies (`#Unsafe`, `region`, `policy`, comptime blocks) run
    /// unconditionally and deliberately keep `check_block`; loop bodies and
    /// switch arms already isolate reachability through their own joins
    /// (`FlowFacts::after_loop`, `merge_states`).
    pub(crate) fn check_conditional_block(&mut self, stmts: &mut [Stmt], new_scope: bool) {
        let reachable = self.flow.reachable;
        self.check_block(stmts, new_scope);
        self.flow.reachable = reachable;
    }

    /// E0209 liveness gate: returns `true` when `name` is
    /// referenced in any statement that follows the current statement in the
    /// innermost block. E0209 fires either way now (no clone is ever silent),
    /// but this decides its fix menu: live-after means `^` would break that
    /// later use, so the menu offers copy/reorder; dead-after means `^` is
    /// safe (this is the value's last use).
    ///
    /// Checks the current block's tail AND all enclosing block tails pushed
    /// by `check_block`, so a clone inside a nested `if` body is not flagged
    /// when the value is used again in the enclosing block after the `if`.
    pub(crate) fn is_name_live_after(&self, name: &str) -> bool {
        // Check the innermost block's tail first.
        if !self.stmt_tail_ptr.is_null() && self.stmt_tail_len > 0 {
            // SAFETY: stmt_tail_ptr + stmt_tail_len describe a valid slice that was
            // set from `&stmts[i+1..]` just before the current check_stmt call.
            // The slice's data lives in the Program AST, which is `&mut Program`
            // at the call site and outlives the Checker.  We only read (no writes)
            // and only during `check_stmt`, so no aliasing issues.
            let tail =
                unsafe { std::slice::from_raw_parts(self.stmt_tail_ptr, self.stmt_tail_len) };
            if tail.iter().any(|s| stmt_refs_name(s, name)) {
                return true;
            }
        }
        // Walk enclosing frames (innermost pushed last) — if the name appears
        // in any enclosing block after the point this nested block was entered,
        // the clone is necessary.
        for &(ptr, len) in self.liveness_frames.iter().rev() {
            if !ptr.is_null() && len > 0 {
                // SAFETY: same as above — each frame was set from a block slice
                // in the Program AST that outlives the Checker.
                let frame = unsafe { std::slice::from_raw_parts(ptr, len) };
                if frame.iter().any(|s| stmt_refs_name(s, name)) {
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn lexical_tail_len(&self) -> usize {
        self.stmt_tail_len
            + self
                .liveness_frames
                .iter()
                .map(|(_, len)| *len)
                .sum::<usize>()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DispatchLiteralKind {
    String,
    Int,
    Bool,
    Char,
    Enum,
}

struct DispatchSubject {
    path: Vec<String>,
    span: Span,
    key: String,
    dependencies: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DispatchValue {
    kind: DispatchLiteralKind,
    key: String,
    span: Span,
}

struct AdjacentDispatchGuard<'a> {
    subject: Vec<String>,
    subject_key: String,
    subject_dependencies: Vec<Vec<String>>,
    subject_span: Span,
    values: Vec<DispatchValue>,
    body: &'a [Stmt],
    condition_span: Span,
    span: Span,
}

struct SourceIfExtent {
    start: usize,
    end: usize,
    body_start: usize,
    body_end: usize,
    braced: bool,
}

fn source_if_extent(
    source: &str,
    span: Span,
    condition_span: Span,
    body: &[Stmt],
) -> Option<SourceIfExtent> {
    let first_open = find_source_char(source, condition_span.end, b'{');
    let arrow = find_source_sequence(source, condition_span.end, b"->");
    // A lambda or struct literal in an arrow body may contain braces. Treat
    // the first brace as the guard body only when it is the authored body
    // opener: before an arrow for classic syntax, or immediately after one
    // for the braced arrow spelling.
    let open = match (first_open, arrow) {
        (Some(open), Some(arrow)) if open < arrow => Some(open),
        (Some(open), None) => Some(open),
        (Some(open), Some(arrow)) => source
            .get(arrow + 2..open)
            .is_some_and(|between| between.trim().is_empty())
            .then_some(open),
        (None, _) => None,
    };
    if let Some(open) = open {
        let close = find_switch_close(source, open)?;
        let mut end = close + 1;
        if let Some(semicolon) = find_authored_semicolon(source, end) {
            end = semicolon + 1;
        }
        return Some(SourceIfExtent {
            start: span.start,
            end,
            body_start: open + 1,
            body_end: close,
            braced: true,
        });
    }
    let arrow = arrow?;
    let body_start = arrow + 2;
    let body_end = body
        .last()
        .and_then(|stmt| statement_source_end(source, body_start, stmt.span().end))?;
    let mut end = body_end;
    if let Some(semicolon) = find_authored_semicolon(source, end) {
        end = semicolon + 1;
    }
    Some(SourceIfExtent {
        start: span.start,
        end,
        body_start,
        body_end,
        braced: false,
    })
}

fn statement_source_end(source: &str, start: usize, fallback: usize) -> Option<usize> {
    let (tokens, _) = crate::Lexer::lex(source);
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut saw_body = false;

    for token in tokens {
        if token.span.end <= start {
            continue;
        }
        match token.kind {
            crate::Lexer::TokKind::LineComment(_)
            | crate::Lexer::TokKind::BlockComment(_) => {}
            crate::Lexer::TokKind::Semi
                if saw_body && parens == 0 && brackets == 0 && braces == 0 =>
            {
                return Some(token.span.start);
            }
            crate::Lexer::TokKind::LParen => {
                parens += 1;
                saw_body = true;
            }
            crate::Lexer::TokKind::RParen if parens > 0 => parens -= 1,
            crate::Lexer::TokKind::LBracket => {
                brackets += 1;
                saw_body = true;
            }
            crate::Lexer::TokKind::RBracket if brackets > 0 => brackets -= 1,
            crate::Lexer::TokKind::LBrace => {
                braces += 1;
                saw_body = true;
            }
            crate::Lexer::TokKind::RBrace if braces > 0 => braces -= 1,
            crate::Lexer::TokKind::Eof => break,
            _ => saw_body = true,
        }
    }
    Some(fallback.max(start))
}

fn source_line_indent(source: &str, start: usize) -> String {
    let line_start = source[..start.min(source.len())]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    source[line_start..start.min(source.len())]
        .chars()
        .take_while(|character| *character == ' ' || *character == '\t')
        .collect()
}

fn source_has_comment(source: &str, start: usize, end: usize) -> bool {
    let bytes = source.as_bytes();
    let mut index = start.min(bytes.len());
    let end = end.min(bytes.len());
    while index < end {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => return true,
            b'/' if bytes.get(index + 1) == Some(&b'*') => return true,
            b'"' => {
                let triple = bytes.get(index..index + 3) == Some(b"\"\"\"");
                index += if triple { 3 } else { 1 };
                while index < end {
                    if triple && bytes.get(index..index + 3) == Some(b"\"\"\"") {
                        index += 3;
                        break;
                    }
                    if !triple && bytes[index] == b'"' {
                        index += 1;
                        break;
                    }
                    if !triple && bytes[index] == b'\\' {
                        index = (index + 2).min(end);
                    } else {
                        index += 1;
                    }
                }
            }
            b'\'' => {
                index += 1;
                while index < end {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(end);
                    } else if bytes[index] == b'\'' {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            _ => index += 1,
        }
    }
    false
}

fn statements_contain_multiline_string(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| {
        let mut found = false;
        stmt.for_each_expr(|expr| {
            if let Expr::Str(parts, _) = expr.without_parens() {
                found |= parts.iter().any(|part| {
                    matches!(part, StrPart::Lit(text) if text.contains('\n'))
                });
            }
        });
        found
    })
}

fn find_source_char(source: &str, start: usize, wanted: u8) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut index = start.min(bytes.len());
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len()
                    && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            b'"' => {
                let triple = bytes.get(index..index + 3) == Some(b"\"\"\"");
                index += if triple { 3 } else { 1 };
                while index < bytes.len() {
                    if triple && bytes.get(index..index + 3) == Some(b"\"\"\"") {
                        index += 3;
                        break;
                    }
                    if !triple && bytes[index] == b'"' {
                        index += 1;
                        break;
                    }
                    if !triple && bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else {
                        index += 1;
                    }
                }
            }
            b'\'' => {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == b'\'' {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            byte if byte == wanted => return Some(index),
            _ => index += 1,
        }
    }
    None
}

fn find_source_sequence(source: &str, start: usize, wanted: &[u8]) -> Option<usize> {
    if wanted.is_empty() {
        return None;
    }
    let bytes = source.as_bytes();
    let mut index = start.min(bytes.len());
    while index + wanted.len() <= bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len()
                    && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            b'"' | b'\'' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            _ if bytes[index..].starts_with(wanted) => return Some(index),
            _ => index += 1,
        }
    }
    None
}

fn find_switch_close(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len()
                    && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            b'"' => {
                let triple = bytes.get(index..index + 3) == Some(b"\"\"\"");
                index += if triple { 3 } else { 1 };
                while index < bytes.len() {
                    if triple && bytes.get(index..index + 3) == Some(b"\"\"\"") {
                        index += 3;
                        break;
                    }
                    if !triple && bytes[index] == b'"' {
                        index += 1;
                        break;
                    }
                    if bytes[index] == b'\\' && !triple {
                        index = (index + 2).min(bytes.len());
                    } else {
                        index += 1;
                    }
                }
            }
            b'\'' => {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == b'\'' {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            b'{' => {
                depth += 1;
                index += 1;
            }
            b'}' if depth == 1 => return Some(index),
            b'}' if depth > 1 => {
                depth -= 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

fn find_authored_semicolon(source: &str, mut at: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    at = at.min(bytes.len());
    loop {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if bytes.get(at..).is_some_and(|tail| tail.starts_with(b"//")) {
            at += 2;
            while at < bytes.len() && bytes[at] != b'\n' {
                at += 1;
            }
            continue;
        }
        if bytes.get(at..).is_some_and(|tail| tail.starts_with(b"/*")) {
            at += 2;
            while at + 1 < bytes.len() && &bytes[at..at + 2] != b"*/" {
                at += 1;
            }
            at = (at + 2).min(bytes.len());
            continue;
        }
        return (bytes.get(at) == Some(&b';')).then_some(at);
    }
}
