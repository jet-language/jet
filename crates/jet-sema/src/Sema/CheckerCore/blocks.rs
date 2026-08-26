use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Sema::Captures::stmt_refs_name;
use crate::Sema::Checker;
use crate::Sema::Diagnostics::block_definitely_returns;
use crate::Syntax;
use crate::AST::{Expr, Stmt, Type};
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
        let redundant_tail_span = value_tail
            .and_then(|_| Self::redundant_arm_table_parts(stmts).map(|(_, span, _, _)| span));
        if redundant_tail_span.is_some() {
            self.check_redundant_arm_table_return(stmts);
        }
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

    /// L0513 / D-TAIL-RETURN1=A: a statement arm table immediately followed
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
