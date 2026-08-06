use super::super::*;

fn leading_dot_variant(kind: &TokKind) -> Option<String> {
    match kind {
        TokKind::Ident(name) if name.chars().next().is_some_and(char::is_uppercase) => {
            Some(name.clone())
        }
        TokKind::KwNull => Some(Syntax::LIT_NULL.to_string()),
        _ => None,
    }
}

impl<'a> Parser<'a> {
    /// D-DOTSCOPE1: parse a scope-member statement `.name { … }` /
    /// `.name(args) { … }`. Purely structural — whether `name` is a valid member
    /// of the enclosing marker (E0614), legal here at all (E0615), correctly
    /// positioned (E0616), correctly argued (E0617), or un-nested (E0618) is all
    /// a sema concern. The leading `.` + ident + block/paren shape is guaranteed
    /// by the caller's guard in `stmt()`.
    pub(super) fn scope_member_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let dot_span = self.peek().span;
        self.bump(); // `.`
        let (name, name_span) = self.expect_ident("for the scope-member name")?;
        let mut args = Vec::new();
        let mut args_span = None;
        if matches!(self.peek().kind, TokKind::LParen) {
            let lp = self.peek().span;
            self.bump(); // `(`
            if !matches!(self.peek().kind, TokKind::RParen) {
                loop {
                    args.push(self.expr()?);
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    self.expect(TokKind::Comma, "between scope-member arguments")?;
                }
            }
            self.expect(TokKind::RParen, "to close the scope-member arguments")?;
            let rp_end = self.toks[self.pos - 1].span.end;
            args_span = Some(Span::new(lp.start, rp_end));
        }
        self.expect(TokKind::LBrace, "to open the scope-member block")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ScopeMember {
            name,
            name_span,
            args,
            args_span,
            body,
            dot_span,
            span: Span::new(dot_span.start, end),
        })
    }

    /// D-IF3 / D-IFDIST1: `if subject OP { … }` is multi-arm value/pattern
    /// dispatch (`OP` is any comparison); a plain `if cond { … }` is a
    /// conventional boolean test. The comparison marker between the subject and
    /// `{` is *required* to enter dispatch; an old-style `if subject { head ->
    /// body }` with no marker is teaching error E0992, which recovers by parsing
    /// the body as arms so the rest of the file still checks. Multi-arm `if`
    /// lowers to the same `Stmt::Switch` IR the former `when` used.
    pub(super) fn if_or_dispatch(&mut self) -> Result<Stmt, Diagnostic> {
        let branch_start = self.pos;
        let span = self.bump().span; // `if`

        // D-IFGUARD1=A: a leading `{` selects ordered Boolean guards. Reuse the
        // dispatch AST with a compiler-private `true` subject whose span is the
        // `if` keyword; source-authored `true` can never carry that span.
        if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            return self.guard_arms(span);
        }

        // Parse the subject below comparison precedence so a trailing compare
        // marker is left in the token stream (`expr_cmp` would eat it). If
        // `OP {` follows, this is explicit dispatch (D-IFDIST1).
        let probe = self.pos;
        let probe_diags = self.diags.len();
        if let Ok(subject) = self.expr_no_struct_lit_no_cmp() {
            if let Some(op) = self.peek_dispatch_op() {
                self.bump(); // comparison marker
                self.expect(TokKind::LBrace, "to open the `if` dispatch body")?;
                return self.if_arms(subject, span, op);
            }
        }
        // Not dispatch — rewind and parse the full boolean condition.
        self.pos = probe;
        self.diags.truncate(probe_diags);

        let cond = self.expr_no_struct_lit()?;
        // D-ASSIGNCOND1 / E0322: `if x = 5` — bare `=` is assignment, not equality.
        if matches!(self.peek().kind, TokKind::Eq) {
            let eq_span = self.peek().span;
            return Err(Diagnostic::error(
                "E0322",
                "assignment `=` cannot appear in an `if` condition".to_string(),
                "`=` binds a name; use `==` to compare two values".to_string(),
                "replace `=` with `==` to compare, or move the assignment before the `if`"
                    .to_string(),
                Some(eq_span),
            ));
        }
        // D-ARROW-CONTROL1: accept the retired result arrow and adjacent body
        // only to teach both removals while preserving a formatter-recoverable AST.
        if matches!(self.peek().kind, TokKind::Arrow) {
            let arrow = self.bump();
            self.diags.push(Diagnostic::error(
                "E0071",
                "this effect-only `if` uses a result arrow".to_string(),
                "an arrow says that control selects a value; this body only performs work"
                    .to_string(),
                "remove `->` and wrap the body in `{ ... }`".to_string(),
                Some(arrow.span),
            ));
            self.teach_control_braces("if", self.peek().span);
            let then_body = self.adjacent_effect_body()?;
            let else_branch = self.adjacent_effect_else()?;
            if matches!(else_branch, Some(ElseBranch::ElseIf(_))) {
                self.prefer_arm_table_lint(span);
            }
            return Ok(Self::classic_if_switch(IfStmt {
                cond,
                then_body,
                else_branch,
                span,
            }));
        }
        if !matches!(self.peek().kind, TokKind::LBrace | TokKind::Semi) {
            self.teach_control_braces("if", self.peek().span);
            let then_body = self.adjacent_effect_body()?;
            let else_branch = self.adjacent_effect_else()?;
            if matches!(else_branch, Some(ElseBranch::ElseIf(_))) {
                self.prefer_arm_table_lint(span);
            }
            return Ok(Self::classic_if_switch(IfStmt {
                cond,
                then_body,
                else_branch,
                span,
            }));
        }
        self.expect(TokKind::LBrace, "to open the `if` body")?;

        // D-IF3 / E0992: an old implicit-dispatch body (first item is `head ->`)
        // with no comparison marker. Teach the explicit form, then recover by
        // parsing the body as arms so the rest of the file's errors still surface (S14).
        if self.if_body_is_arms() {
            self.diags.push(Diagnostic::error(
                "E0992",
                format!(
                    "a multi-arm `{}` needs a comparison between the subject and `{{`",
                    Syntax::KW_IF
                ),
                format!(
                    "`{} subject == {{ … }}` (or `<`, `>`, `!=`, `<=`, `>=`) marks value dispatch explicitly, so a plain `{} cond {{ … }}` body is always statements",
                    Syntax::KW_IF,
                    Syntax::KW_IF
                ),
                format!("write `{} subject == {{ … }}`", Syntax::KW_IF),
                Some(span),
            ));
            return self.if_arms(cond, span, BinOp::Eq);
        }

        // Conventional `if`: the condition gates a statement body.
        let then_body = self.block_stmts();
        let mut else_branch = None;
        let mut chained = false;
        if matches!(self.peek().kind, TokKind::KwElse) {
            self.bump();
            if matches!(self.peek().kind, TokKind::KwIf) {
                chained = true;
                else_branch = Some(ElseBranch::ElseIf(Box::new(self.if_stmt()?)));
            } else {
                self.expect(TokKind::LBrace, "to open the `else` body")?;
                else_branch = Some(ElseBranch::Else(self.block_stmts()));
            }
        }
        if else_branch.is_some()
            && (chained || self.span_has_authored_line_break(branch_start, self.pos))
        {
            self.prefer_arm_table_lint(span);
        }
        Ok(Self::classic_if_switch(IfStmt {
            cond,
            then_body,
            else_branch,
            span,
        }))
    }

    /// D-BRANCH-AST1=C: classic effect `if` syntax enters the same arm-table
    /// node as every other branch directly from the parser.
    fn classic_if_switch(branch: IfStmt) -> Stmt {
        let IfStmt {
            cond,
            then_body,
            else_branch,
            span,
        } = branch;
        let else_body = match else_branch {
            None => None,
            Some(ElseBranch::Else(body)) => Some(body),
            Some(ElseBranch::ElseIf(next)) => Some(vec![Self::classic_if_switch(*next)]),
        };
        Stmt::Switch {
            subject: Expr::Bool(true, span),
            arms: vec![SwitchArm {
                span: cond.span(),
                cond,
                body: then_body,
            }],
            else_body,
            span,
        }
    }

    fn adjacent_effect_body(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        if matches!(self.peek().kind, TokKind::KwIf) {
            return Err(Diagnostic::error(
                "E0329",
                "a nested one-line `if` needs braces".to_string(),
                "an adjacent body owns one non-`if` statement, so direct nesting is ambiguous"
                    .to_string(),
                "wrap the nested `if` in `{ ... }`".to_string(),
                Some(self.peek().span),
            ));
        }
        self.adjacent_if_body_depth += 1;
        let statement = self.stmt();
        self.adjacent_if_body_depth -= 1;
        Ok(vec![statement?])
    }

    fn adjacent_effect_else(&mut self) -> Result<Option<ElseBranch>, Diagnostic> {
        if !matches!(self.peek().kind, TokKind::KwElse) {
            return Ok(None);
        }
        self.bump();
        if matches!(self.peek().kind, TokKind::KwIf) {
            return Ok(Some(ElseBranch::ElseIf(Box::new(self.if_stmt()?))));
        }
        if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            return Ok(Some(ElseBranch::Else(self.block_stmts())));
        }
        self.teach_control_braces("else", self.peek().span);
        Ok(Some(ElseBranch::Else(self.adjacent_effect_body()?)))
    }

    pub(super) fn teach_control_braces(&mut self, body: &str, span: Span) {
        self.diags.push(Diagnostic::error(
            "E0372",
            format!("this `{body}` body needs braces"),
            "braces make the body's boundary visible to readers, editors, and the compiler"
                .to_string(),
            format!("wrap the body in `{{ ... }}`; `jet fmt` applies this fix"),
            Some(span),
        ));
    }

    /// D-IFGUARD1=A: `if { Bool -> body ... [else -> body] }` statement guards.
    /// The existing switch node preserves first-match dispatch and checked TIR
    /// lowering; `true` at the keyword span is the compiler-private subject.
    fn guard_arms(&mut self, span: Span) -> Result<Stmt, Diagnostic> {
        let mut arms = Vec::new();
        let mut else_body = None;
        loop {
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            match self.peek().kind {
                TokKind::RBrace => {
                    if arms.is_empty() {
                        return Err(Diagnostic::error(
                            "E0003",
                            "a statement guard table needs at least one condition".to_string(),
                            "an empty guard table can never perform an action".to_string(),
                            "add a `condition -> statement` arm".to_string(),
                            Some(self.peek().span),
                        ));
                    }
                    self.bump();
                    break;
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this guard table, found the end of the file".to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                TokKind::KwElse => {
                    self.bump();
                    self.expect(TokKind::Arrow, "after `else` in a guard table")?;
                    else_body = Some(self.guard_arm_body()?);
                    while matches!(self.peek().kind, TokKind::Semi) {
                        self.bump();
                    }
                    self.expect(TokKind::RBrace, "after the final `else` guard arm")?;
                    break;
                }
                _ => {
                    let arm_start = self.peek().span;
                    let cond = self.expr_no_struct_lit()?;
                    self.expect(TokKind::Arrow, "after a guard condition")?;
                    let body = self.guard_arm_body()?;
                    let end = body.last().map(Stmt::span).map_or(cond.span().end, |s| s.end);
                    arms.push(SwitchArm {
                        cond,
                        body,
                        span: Span::new(arm_start.start, end),
                    });
                }
            }
        }
        Ok(Stmt::Switch {
            subject: Expr::Bool(true, span),
            arms,
            else_body,
            span,
        })
    }

    fn guard_arm_body(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            return Ok(self.block_stmts());
        }
        if matches!(self.peek().kind, TokKind::KwIf) {
            return Err(Diagnostic::error(
                "E0329",
                "a nested guard body needs braces".to_string(),
                "a braceless guard owns exactly one non-`if` statement, so direct nesting would be ambiguous".to_string(),
                "wrap the nested `if` in `{ ... }`".to_string(),
                Some(self.peek().span),
            ));
        }
        Ok(vec![self.stmt()?])
    }

    /// D-IF1: is the `if` body (cursor just past `{`) a multi-arm dispatch?
    /// True when the body opens with `else ->`, or with an expression arm head
    /// immediately followed by `->`. Pure lookahead — restores the cursor.
    pub(super) fn if_body_is_arms(&mut self) -> bool {
        // `else ->` catch-all as the first (only) arm.
        if matches!(self.peek().kind, TokKind::KwElse)
            && matches!(self.peek2().kind, TokKind::Arrow)
        {
            return true;
        }
        // An empty body `{}` is a conventional (empty) if, not arm mode.
        if matches!(self.peek().kind, TokKind::RBrace) {
            return false;
        }
        // D-PATR: `Int .. Int ->` is a range arm — detect without full expr parse.
        // Also detect `Int ..= Int ->` (E0318) and `Int .. Int step Int ->` (E0319)
        // porting hazards so we can emit teaching errors rather than confusing parse failures.
        if let TokKind::Int(_, _) = &self.peek().kind {
            if matches!(
                self.toks.get(self.pos + 1).map(|t| &t.kind),
                Some(TokKind::DotDot)
            ) {
                // `lo .. hi ->` (4 tokens: lo, .., hi, ->)
                if let Some(tok_after_hi) = self.toks.get(self.pos + 3) {
                    if matches!(tok_after_hi.kind, TokKind::Arrow) {
                        return true;
                    }
                }
                // `lo .. hi step n ->` (6 tokens: lo, .., hi, step, n, ->)  — E0319 porting hazard
                if matches!(self.toks.get(self.pos + 3).map(|t| &t.kind), Some(TokKind::Ident(s)) if s == Syntax::RETIRED_LOOP_STEP)
                {
                    if let Some(tok_after_step_n) = self.toks.get(self.pos + 5) {
                        if matches!(tok_after_step_n.kind, TokKind::Arrow) {
                            return true;
                        }
                    }
                }
                // `lo ..= hi ->` (5 tokens: lo, .., =, hi, ->)  — E0318 porting hazard
                if matches!(
                    self.toks.get(self.pos + 2).map(|t| &t.kind),
                    Some(TokKind::Eq)
                ) {
                    if let Some(tok_after_eq_hi) = self.toks.get(self.pos + 4) {
                        if matches!(tok_after_eq_hi.kind, TokKind::Arrow) {
                            return true;
                        }
                    }
                }
                return false;
            }
        }
        let save = self.pos;
        let saved_diags = self.diags.len();
        let is_arm = matches!(self.expr_no_struct_lit(), Ok(_))
            && matches!(self.peek().kind, TokKind::Arrow);
        self.pos = save;
        self.diags.truncate(saved_diags);
        is_arm
    }

    /// D-IF3 / D-IFDIST1: parse the arms of an `if subject OP { … }` dispatch
    /// (cursor just past `{`), lowering to `Stmt::Switch`. Each arm is
    /// `head -> body` where `head` is a bare value compared with `OP`, a bare
    /// range (`400..499`), a bare pattern (`Active(id)`, … — `==` only), or a
    /// Boolean expression evaluated as written. The leading `subject OP` is
    /// dropped and re-bound here. A leftover `subject ==` prefix gets E0994.
    /// `body` is a braceless single statement or a `{ … }` block (D-IF2 Q2).
    /// `else -> body` is the catch-all.
    pub(super) fn if_arms(
        &mut self,
        subject: Expr,
        span: Span,
        op: BinOp,
    ) -> Result<Stmt, Diagnostic> {
        let mut arms: Vec<SwitchArm> = Vec::new();
        let mut else_body: Option<Vec<Stmt>> = None;
        // The scrutinee a pattern arm binds to: a simple ident subject is named
        // directly; a complex subject (call/field) is the synthesised `it` that
        // sema declares for fallible/dispatch subjects.
        let pat_subject = match &subject {
            Expr::Ident(..) => subject.clone(),
            _ => Expr::Ident(Syntax::KW_IT.to_string(), subject.span()),
        };
        loop {
            // Skip synthetic terminators between arms (S6-R).
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            match &self.peek().kind {
                TokKind::RBrace => {
                    self.bump();
                    break;
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this `if`, found the end of the file".to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                TokKind::KwElse => {
                    let arm_start = self.bump().span; // `else`
                    self.expect(TokKind::Arrow, "after `else` in an `if`")?;
                    let body = self.arm_body()?;
                    let _ = arm_start;
                    else_body = Some(body);
                }
                _ => {
                    let arm_start = self.peek().span;
                    // D-PATR / C25: range heads live in `if_arm_head` (shared with
                    // expression-position value dispatch).
                    let raw_head = self.if_arm_head(&subject, &pat_subject, op)?;
                    self.expect(TokKind::Arrow, "after an `if` arm value or condition")?;
                    let body = self.arm_body()?;
                    let end = self
                        .toks
                        .get(self.pos.saturating_sub(1))
                        .map(|t| t.span.end)
                        .unwrap_or(arm_start.end);
                    arms.push(SwitchArm {
                        cond: raw_head,
                        body,
                        span: Span::new(arm_start.start, end),
                    });
                }
            }
        }
        Ok(Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        })
    }

    /// D-PATR / C25: parse `lo..hi` (plus E0318 `..=` / E0319 `step` recovery) as a
    /// range arm head attached to `subject`. `Ok(None)` when the cursor is not a
    /// range head — caller falls through to the ordinary arm-head grammar.
    pub(super) fn try_range_arm_head(
        &mut self,
        subject: &Expr,
    ) -> Result<Option<Expr>, Diagnostic> {
        let TokKind::Int(lo_val, _) = &self.peek().kind.clone() else {
            return Ok(None);
        };
        if !matches!(
            self.toks.get(self.pos + 1).map(|t| &t.kind),
            Some(TokKind::DotDot)
        ) {
            return Ok(None);
        }
        let lo = *lo_val;
        let range_start = self.bump().span; // consume lo
        self.bump(); // consume `..`
                     // C25/E0318: `..=` is Rust's inclusive range — Jet's `..` is already inclusive.
                     // Push the error, then recover by consuming hi and building a valid range arm.
        if matches!(self.peek().kind, TokKind::Eq) {
            self.bump(); // consume `=`
            if let TokKind::Int(hi_val, _) = &self.peek().kind.clone() {
                let hi = *hi_val;
                let range_end = self.bump().span; // consume hi
                let pat_span = Span::new(range_start.start, range_end.end);
                self.diags.push(Diagnostic::error(
                    "E0318",
                    "`..=` is not a Jet operator — Jet's `..` is already inclusive".to_string(),
                    "in Rust, `..` is exclusive and `..=` is inclusive; in Jet, `..` includes both ends".to_string(),
                    format!("write `{}..{}` — that already includes `{}`", lo, hi, hi),
                    Some(pat_span),
                ));
                return Ok(Some(Expr::PatternTest {
                    subject: Box::new(subject.clone()),
                    pattern: Pattern::Range {
                        lo,
                        hi,
                        span: pat_span,
                    },
                    span: pat_span,
                }));
            }
            return Err(Diagnostic::error(
                "E0318",
                "`..=` is not a Jet operator — Jet's `..` is already inclusive".to_string(),
                "in Rust, `..` is exclusive and `..=` is inclusive; in Jet, `..` includes both ends".to_string(),
                "write `lo..hi` — that already includes `hi`".to_string(),
                Some(self.peek().span),
            ));
        }
        if let TokKind::Int(hi_val, _) = &self.peek().kind.clone() {
            let hi = *hi_val;
            let range_end = self.bump().span; // consume hi
            let pat_span = Span::new(range_start.start, range_end.end);
            // C25/E0319: `step` after a range arm is a loop modifier, not an arm construct.
            // Push the error and skip `step N` so the arm can still be parsed.
            if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::RETIRED_LOOP_STEP)
            {
                self.diags.push(Diagnostic::error(
                    "E0319",
                    "`step` is not allowed in a range arm — range arms test a band, not a sequence".to_string(),
                    "`step` is a retired loop spelling; a range arm just checks if the subject falls between the two ends".to_string(),
                    format!("remove `step …`, or use a full condition: `subject >= {} && subject <= {} && subject % n == 0 ->`", lo, hi),
                    Some(pat_span),
                ));
                self.bump(); // consume `step`
                if matches!(self.peek().kind, TokKind::Int(_, _)) {
                    self.bump(); // consume step value
                }
            }
            return Ok(Some(Expr::PatternTest {
                subject: Box::new(subject.clone()),
                pattern: Pattern::Range {
                    lo,
                    hi,
                    span: pat_span,
                },
                span: pat_span,
            }));
        }
        Err(Diagnostic::error(
            "E0003",
            "expected an integer after `..` in a range arm".to_string(),
            "range arms need both ends: `lo..hi -> body`".to_string(),
            "write `0..59 -> { body }` for an inclusive range arm".to_string(),
            Some(self.peek().span),
        ))
    }

    /// D-IF3 / D-MATCHARM1 / D-IFDIST1: parse one bare arm head (no leading
    /// `subject OP`) and bind it to the subject with the table's comparison.
    /// - A pattern head (`.Active(id)`, `A(x) | B(x)`) becomes a `PatternTest`
    ///   (`==` marker only).
    /// - Values use the D-MATCHARM1 grammar: `|` unions distributed atoms,
    ///   `&&`/`||` combine booleans, parens group.
    /// - A leftover redundant `subject ==` prefix emits E0994 and recovers.
    pub(super) fn if_arm_head(
        &mut self,
        subject: &Expr,
        pat_subject: &Expr,
        op: BinOp,
    ) -> Result<Expr, Diagnostic> {
        // D-PATR: `Int .. Int ->` range heads — shared with statement/value dispatch.
        // Attach `pat_subject` (same as other pattern heads; `it` for non-Ident).
        if let Some(range) = self.try_range_arm_head(pat_subject)? {
            return Ok(range);
        }
        // A bare pattern head: parse it standalone and attach the subject.
        let save = self.pos;
        let save_diags = self.diags.len();
        if let Some(pattern) = self.try_pattern_rhs()? {
            // Only a pattern if it consumed up to the `->`; otherwise it was an
            // ordinary value — rewind and re-parse as the value grammar.
            if matches!(self.peek().kind, TokKind::Arrow) {
                let span = pat_span(&pattern);
                if op != BinOp::Eq {
                    self.diags.push(Diagnostic::error(
                        "E0366",
                        format!(
                            "pattern arms need `==` — this table uses `{}`",
                            op.spell()
                        ),
                        "structural patterns compare by shape under `if subject == { … }` only"
                            .to_string(),
                        format!(
                            "write `{} subject == {{ … }}` for pattern arms, or use a Bool head",
                            Syntax::KW_IF
                        ),
                        Some(span),
                    ));
                    // Recover with a Bool false head so the rest of the table
                    // (and a later `else`) can still parse.
                    return Ok(Expr::Bool(false, span));
                }
                return Ok(Expr::PatternTest {
                    subject: Box::new(pat_subject.clone()),
                    pattern,
                    span,
                });
            }
            self.pos = save;
            self.diags.truncate(save_diags);
        }
        // E0994: peek for a redundant `subject ==` prefix before entering the
        // new grammar (parse-and-rewind so diagnostics don't leak).
        let save_pos = self.pos;
        let save_diags = self.diags.len();
        if let Ok(raw) = self.expr_no_struct_lit() {
            match &raw {
                Expr::PatternTest {
                    subject: lhs, span, ..
                } if Self::same_subject(lhs, subject) => {
                    self.diags.push(Self::redundant_subject_diag(*span));
                    return Ok(raw);
                }
                Expr::Binary(binop, lhs, _, span)
                    if *binop == op && Self::same_subject(lhs, subject) =>
                {
                    self.diags.push(Self::redundant_subject_diag(*span));
                    return Ok(raw);
                }
                _ => {}
            }
        }
        // Rewind — the new arm-head grammar re-parses from the saved position.
        self.pos = save_pos;
        self.diags.truncate(save_diags);
        // D-MATCHARM1: value-alternates grammar.
        self.parse_arm_value_cond(subject, op)
    }

    pub(super) fn redundant_subject_diag(span: Span) -> Diagnostic {
        Diagnostic::error(
            "E0994",
            format!(
                "drop the `subject OP` — the comparison on the `{}` already applies it to every arm",
                Syntax::KW_IF
            ),
            format!(
                "`{} subject OP {{ … }}` matches each arm head against the subject, so repeating `subject OP` on an arm is redundant",
                Syntax::KW_IF
            ),
            "delete the `subject OP` prefix; write just the value or pattern".to_string(),
            Some(span),
        )
    }

    // ── D-MATCHARM1 arm-head grammar ─────────────────────────────────────────

    /// `arm_and_expr = arm_alternates ("&&" arm_bool_operand)*`
    /// D-IFDIST1: after `&&`, a single Ident/field/call is a Bool predicate
    /// (not `subject OP ident`); `|` unions still distribute.
    pub(super) fn parse_arm_and_cond(
        &mut self,
        subject: &Expr,
        op: BinOp,
    ) -> Result<Expr, Diagnostic> {
        let (lhs_cond, _) = self.parse_arm_alternates_cond(subject, op, false)?;
        if !matches!(self.peek().kind, TokKind::AndAnd) {
            return Ok(lhs_cond);
        }
        let mut parts = vec![lhs_cond];
        while matches!(self.peek().kind, TokKind::AndAnd) {
            self.bump();
            parts.push(self.parse_arm_bool_operand(subject, op)?);
        }
        Ok(parts
            .into_iter()
            .reduce(|a, b| {
                let span = Span::new(a.span().start, b.span().end);
                Expr::Binary(BinOp::And, Box::new(a), Box::new(b), span)
            })
            .unwrap())
    }

    /// Entry point: `arm_bool_expr = arm_and_expr ("||" arm_bool_operand)*`
    pub(super) fn parse_arm_value_cond(
        &mut self,
        subject: &Expr,
        op: BinOp,
    ) -> Result<Expr, Diagnostic> {
        let lhs = self.parse_arm_and_cond(subject, op)?;
        if !matches!(self.peek().kind, TokKind::OrOr) {
            return Ok(lhs);
        }
        let mut parts = vec![lhs];
        while matches!(self.peek().kind, TokKind::OrOr) {
            self.bump();
            parts.push(self.parse_arm_bool_operand(subject, op)?);
        }
        Ok(parts
            .into_iter()
            .reduce(|a, b| {
                let span = Span::new(a.span().start, b.span().end);
                Expr::Binary(BinOp::Or, Box::new(a), Box::new(b), span)
            })
            .unwrap())
    }

    /// Operand after `&&` / `||`: a `|` union still distributes; a lone
    /// predicate (Ident/field/call/…) is left as a Bool expression.
    pub(super) fn parse_arm_bool_operand(
        &mut self,
        subject: &Expr,
        op: BinOp,
    ) -> Result<Expr, Diagnostic> {
        if matches!(self.peek().kind, TokKind::LParen) {
            self.bump();
            let inner = self.parse_arm_value_cond(subject, op)?;
            self.expect(TokKind::RParen, "to close the arm head group")?;
            return Ok(inner);
        }
        let (cond, _) = self.parse_arm_alternates_cond(subject, op, true)?;
        Ok(cond)
    }

    /// `arm_alternates = arm_atom ("|" arm_atom)*`
    /// Returns `(condition_expr, alternate_count)`.
    /// `prefer_predicate` is true for a single atom after `&&`/`||`.
    pub(super) fn parse_arm_alternates_cond(
        &mut self,
        subject: &Expr,
        op: BinOp,
        prefer_predicate: bool,
    ) -> Result<(Expr, usize), Diagnostic> {
        let first = self.parse_arm_atom_cond(subject, op, prefer_predicate)?;
        let mut alts = vec![first];
        // Consume single `|` but not `||` (peek at the token after `|`).
        while matches!(self.peek().kind, TokKind::Pipe)
            && !matches!(
                self.toks.get(self.pos + 1).map(|t| &t.kind),
                Some(TokKind::Pipe)
            )
        {
            self.bump(); // consume `|`
            // Values in a `|` union always distribute.
            alts.push(self.parse_arm_atom_cond(subject, op, false)?);
        }
        let n = alts.len();
        if n == 1 {
            return Ok((alts.into_iter().next().unwrap(), 1));
        }
        let combined = alts
            .into_iter()
            .reduce(|a, b| {
                let span = Span::new(a.span().start, b.span().end);
                Expr::Binary(BinOp::Or, Box::new(a), Box::new(b), span)
            })
            .unwrap();
        Ok((combined, n))
    }

    /// `arm_atom = "(" arm_bool_expr ")" | single_value`
    pub(super) fn parse_arm_atom_cond(
        &mut self,
        subject: &Expr,
        op: BinOp,
        prefer_predicate: bool,
    ) -> Result<Expr, Diagnostic> {
        if matches!(self.peek().kind, TokKind::LParen) {
            self.bump(); // consume `(`
            let inner = self.parse_arm_value_cond(subject, op)?;
            self.expect(TokKind::RParen, "to close the arm head group")?;
            return Ok(inner);
        }
        let raw = self.expr_cmp(false)?;
        Ok(Self::arm_atom_to_cond(subject.clone(), raw, op, prefer_predicate))
    }

    /// Wrap a single value as a condition. Comparisons / PatternTest / Bool are
    /// kept as-is; plain values get `subject OP value` wrapping (D-IFDIST1).
    /// After `&&`/`||`, Ident/field/call predicates stay unwrapped.
    pub(super) fn arm_atom_to_cond(
        subject: Expr,
        value: Expr,
        op: BinOp,
        prefer_predicate: bool,
    ) -> Expr {
        match &value {
            Expr::Binary(binop, ..)
                if binop.is_comparison() || matches!(binop, BinOp::And | BinOp::Or) =>
            {
                value
            }
            Expr::PatternTest { .. } | Expr::Bool(_, _) => value,
            Expr::Unary(crate::AST::UnOp::Not, ..) => value,
            Expr::Ident(..) | Expr::Field { .. } | Expr::Call { .. } | Expr::MethodCall { .. }
                if prefer_predicate =>
            {
                value
            }
            _ => {
                let span = Span::new(subject.span().start, value.span().end);
                Expr::Binary(op, Box::new(subject), Box::new(value), span)
            }
        }
    }

    /// D-IFDIST1: a comparison token followed by `{` is the dispatch marker.
    pub(super) fn peek_dispatch_op(&self) -> Option<BinOp> {
        if !matches!(self.peek2().kind, TokKind::LBrace) {
            return None;
        }
        match self.peek().kind {
            TokKind::EqEq => Some(BinOp::Eq),
            TokKind::NotEq => Some(BinOp::Ne),
            TokKind::Lt => Some(BinOp::Lt),
            TokKind::Gt => Some(BinOp::Gt),
            TokKind::Le => Some(BinOp::Le),
            TokKind::Ge => Some(BinOp::Ge),
            _ => None,
        }
    }

    /// True when `a` denotes the dispatch subject (a byte-equal ident, or the
    /// synthesised `it`). Mirrors the formatter's `same_subject`.
    pub(super) fn same_subject(a: &Expr, subject: &Expr) -> bool {
        match (a, subject) {
            (Expr::Ident(n, _), _) if n == Syntax::KW_IT => true,
            (Expr::Ident(n, _), Expr::Ident(s, _)) => n == s,
            _ => false,
        }
    }

    /// D-IF2 Q2: an arm body is a `{ … }` block or a single braceless statement.
    pub(super) fn arm_body(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            Ok(self.block_stmts())
        } else {
            // Braceless single statement (a call, binding, return, etc.).
            let stmt = self.stmt()?;
            Ok(vec![stmt])
        }
    }

    pub(super) fn if_stmt(&mut self) -> Result<IfStmt, Diagnostic> {
        let span = self.bump().span; // `if`
        let cond = self.expr_no_struct_lit()?;
        self.expect(TokKind::LBrace, "to open the `if` body")?;
        let then_body = self.block_stmts();
        let mut else_branch = None;
        if matches!(self.peek().kind, TokKind::KwElse) {
            self.bump();
            if matches!(self.peek().kind, TokKind::KwIf) {
                else_branch = Some(ElseBranch::ElseIf(Box::new(self.if_stmt()?)));
            } else {
                self.expect(TokKind::LBrace, "to open the `else` body")?;
                else_branch = Some(ElseBranch::Else(self.block_stmts()));
            }
        }
        Ok(IfStmt {
            cond,
            then_body,
            else_branch,
            span,
        })
    }

    /// `switch` body, after the keyword (S24): either legacy condition arms
    /// with `->`, or pipe arms where bare terms mean `subject == term`.
    /// S68 / D-ARROW-CONTROL1: parse an `if` expression —
    /// `if cond -> value else -> value`.
    /// Each branch is a value block; `else` is required (an `if` with no value
    /// is a statement, parsed elsewhere).
    pub(in super::super) fn parse_if_expr(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_if_expr_inner(true)
    }

    fn parse_if_expr_inner(&mut self, lint_style: bool) -> Result<Expr, Diagnostic> {
        let branch_start = self.pos;
        let start = self.bump().span; // `if`
        if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            return self.parse_guard_expr(start);
        }
        // D-IFDIST1: `if subject OP { value-arms }` in expression position.
        let probe = self.pos;
        let probe_diags = self.diags.len();
        if let Ok(subject) = self.expr_no_struct_lit_no_cmp() {
            if let Some(op) = self.peek_dispatch_op() {
                self.bump(); // comparison marker
                self.expect(TokKind::LBrace, "to open the `if` dispatch body")?;
                return self.parse_dispatch_expr(subject, op, start);
            }
        }
        self.pos = probe;
        self.diags.truncate(probe_diags);

        let cond = self.expr_no_struct_lit()?;
        self.expect(TokKind::Arrow, "after the condition of a value-producing `if`")?;
        let (then_body, then_value) = self.parse_selected_value()?;
        if !matches!(self.peek().kind, TokKind::KwElse) {
            return Err(Diagnostic::error(
                "E0003",
                "an `if` used as a value needs an `else` branch".to_string(),
                "when `if` produces a value, both branches must produce one (S68)".to_string(),
                "add `else -> value` so every path has a value".to_string(),
                Some(self.peek().span),
            ));
        }
        self.bump(); // `else`
        // `else if …` nests directly; a final selected value uses `else ->`.
        let chained = matches!(self.peek().kind, TokKind::KwIf);
        let (else_body, else_value) = if chained {
            (Vec::new(), self.parse_if_expr_inner(false)?)
        } else {
            self.expect(TokKind::Arrow, "after `else` in a value-producing `if`")?;
            self.parse_selected_value()?
        };
        let span = Span::new(start.start, else_value.span().end);
        if lint_style
            && (chained || self.span_has_authored_line_break(branch_start, self.pos))
        {
            self.prefer_arm_table_lint(start);
        }
        Ok(Expr::If {
            cond: Box::new(cond),
            then_body,
            then_value: Box::new(then_value),
            else_body,
            else_value: Box::new(else_value),
            span,
        })
    }

    /// D-IFDIST1: value-producing `if subject OP { head -> value … else -> value }`.
    /// Desugars to a nested `Expr::If` chain (same shape as subjectless value guards).
    fn parse_dispatch_expr(
        &mut self,
        subject: Expr,
        op: BinOp,
        start: Span,
    ) -> Result<Expr, Diagnostic> {
        let pat_subject = match &subject {
            Expr::Ident(..) => subject.clone(),
            _ => Expr::Ident(Syntax::KW_IT.to_string(), subject.span()),
        };
        let mut arms: Vec<(Expr, Vec<Stmt>, Expr)> = Vec::new();
        let (else_body, else_value, end) = loop {
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            match self.peek().kind {
                TokKind::KwElse => {
                    self.bump();
                    self.expect(TokKind::Arrow, "after `else` in a value dispatch")?;
                    let (body, value) = self.guard_value_body()?;
                    while matches!(self.peek().kind, TokKind::Semi) {
                        self.bump();
                    }
                    let close = self.peek().span;
                    self.expect(TokKind::RBrace, "after the final `else` value dispatch arm")?;
                    break (body, value, close.end);
                }
                TokKind::RBrace => {
                    let close = self.bump().span;
                    // Card #1440: an all-pattern table may omit `else` — sema
                    // proves the arms cover the subject's whole type (E0307).
                    let all_pattern = !arms.is_empty()
                        && arms
                            .iter()
                            .all(|(cond, _, _)| matches!(cond, Expr::PatternTest { .. }));
                    if all_pattern {
                        break (Vec::new(), Expr::NoElse(close), close.end);
                    }
                    return Err(Diagnostic::error(
                        "E0003",
                        "a value dispatch needs a final `else` arm".to_string(),
                        "only a table whose every arm is a pattern can prove one arm always matches; this one has a comparison or Bool arm"
                            .to_string(),
                        "add `else -> value` so every path produces a value".to_string(),
                        Some(close),
                    ));
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected a final `else` arm and `}` for this value dispatch".to_string(),
                        "a value dispatch must produce a value on every path".to_string(),
                        "add `else -> value` and a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                _ => {
                    let cond = self.if_arm_head(&subject, &pat_subject, op)?;
                    self.expect(TokKind::Arrow, "after a value dispatch arm head")?;
                    let (body, value) = self.guard_value_body()?;
                    arms.push((cond, body, value));
                }
            }
        };
        if arms.is_empty() {
            return Err(Diagnostic::error(
                "E0003",
                "a value dispatch needs at least one arm before `else`".to_string(),
                "`else` is a fallback, not a condition".to_string(),
                "add a `value -> result` arm before `else`".to_string(),
                Some(start),
            ));
        }
        let span = Span::new(start.start, end);
        let mut fallback_body = else_body;
        let mut fallback_value = else_value;
        for (cond, then_body, then_value) in arms.into_iter().rev() {
            fallback_value = Expr::If {
                cond: Box::new(cond),
                then_body,
                then_value: Box::new(then_value),
                else_body: fallback_body,
                else_value: Box::new(fallback_value),
                span,
            };
            fallback_body = Vec::new();
        }
        Ok(fallback_value)
    }

    fn parse_selected_value(&mut self) -> Result<(Vec<Stmt>, Expr), Diagnostic> {
        if matches!(self.peek().kind, TokKind::LBrace) {
            self.parse_value_block()
        } else {
            let value = self.expr()?;
            Ok((Vec::new(), value))
        }
    }

    /// D-IFGUARD1=A: a value guard is a nested ordinary if-expression chain.
    /// The final `else` is syntactically mandatory because arbitrary Boolean
    /// heads are not solver-proved exhaustive.
    fn parse_guard_expr(&mut self, start: Span) -> Result<Expr, Diagnostic> {
        let mut arms: Vec<(Expr, Vec<Stmt>, Expr)> = Vec::new();
        let (else_body, else_value, end) = loop {
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            match self.peek().kind {
                TokKind::KwElse => {
                    self.bump();
                    self.expect(TokKind::Arrow, "after `else` in a value guard")?;
                    let (body, value) = self.guard_value_body()?;
                    while matches!(self.peek().kind, TokKind::Semi) {
                        self.bump();
                    }
                    let close = self.peek().span;
                    self.expect(TokKind::RBrace, "after the final `else` value guard arm")?;
                    break (body, value, close.end);
                }
                TokKind::RBrace => {
                    let close = self.bump().span;
                    return Err(Diagnostic::error(
                        "E0003",
                        "a guard table used as a value needs a final `else` arm".to_string(),
                        "arbitrary Boolean guards cannot prove that one arm always matches".to_string(),
                        "add `else -> value` so every path produces a value".to_string(),
                        Some(close),
                    ));
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected a final `else` arm and `}` for this value guard".to_string(),
                        "a value guard must produce a value on every path".to_string(),
                        "add `else -> value` and a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                _ => {
                    let cond = self.expr_no_struct_lit()?;
                    self.expect(TokKind::Arrow, "after a value guard condition")?;
                    let (body, value) = self.guard_value_body()?;
                    arms.push((cond, body, value));
                }
            }
        };
        if arms.is_empty() {
            return Err(Diagnostic::error(
                "E0003",
                "a value guard needs at least one condition before `else`".to_string(),
                "`else` is a fallback, not a condition".to_string(),
                "add a `condition -> value` arm before `else`".to_string(),
                Some(start),
            ));
        }
        let span = Span::new(start.start, end);
        let mut fallback_body = else_body;
        let mut fallback_value = else_value;
        for (cond, then_body, then_value) in arms.into_iter().rev() {
            fallback_value = Expr::If {
                cond: Box::new(cond),
                then_body,
                then_value: Box::new(then_value),
                else_body: fallback_body,
                else_value: Box::new(fallback_value),
                span,
            };
            fallback_body = Vec::new();
        }
        Ok(fallback_value)
    }

    fn guard_value_body(&mut self) -> Result<(Vec<Stmt>, Expr), Diagnostic> {
        if matches!(self.peek().kind, TokKind::LBrace) {
            return self.parse_value_block();
        }
        let value = self.expr()?;
        Ok((Vec::new(), value))
    }

    /// S68 (D-SG2): parse `{ stmt* tail-expr }` where the trailing expression
    /// (no `;`) is the block's value. Leading statements use the ordinary
    /// statement grammar; the tail is detected by speculatively parsing an
    /// expression and checking for the closing `}`.
    pub(super) fn parse_value_block(&mut self) -> Result<(Vec<Stmt>, Expr), Diagnostic> {
        self.expect(TokKind::LBrace, "to open this `if` branch")?;
        let mut stmts = Vec::new();
        loop {
            match &self.peek().kind {
                TokKind::RBrace => {
                    let span = self.peek().span;
                    self.bump();
                    return Err(Diagnostic::error(
                        "E0003",
                        "this `if` branch is empty but is used as a value".to_string(),
                        "when `if` produces a value, each branch must end with one (S68)"
                            .to_string(),
                        "put a value as the last line, like `{ x }`".to_string(),
                        Some(span),
                    ));
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this `if` branch, found the end of the file"
                            .to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                _ => {}
            }
            // Try the current position as the trailing value expression.
            let save = self.pos;
            let saved_diags = self.diags.len();
            if let Ok(e) = self.expr() {
                // S6-R: the lexer inserts a synthetic terminator after the tail
                // value too (it ends a line before `}`); accept `expr }` or
                // `expr ; }` as the block's value.
                if matches!(self.peek().kind, TokKind::Semi)
                    && matches!(self.peek2().kind, TokKind::RBrace)
                {
                    self.bump(); // synthetic `;`
                }
                if matches!(self.peek().kind, TokKind::RBrace) {
                    self.bump();
                    return Ok((stmts, e));
                }
            }
            // Not the tail value — rewind and parse an ordinary statement.
            self.pos = save;
            self.diags.truncate(saved_diags);
            match self.stmt() {
                Ok(s) => stmts.push(s),
                Err(d) => {
                    self.diags.push(d);
                    self.sync_stmt();
                }
            }
        }
    }

    pub(super) fn switch_after_kw(&mut self, span: Span) -> Result<Stmt, Diagnostic> {
        let subject = self.expr_no_struct_lit()?;
        self.expect(TokKind::LBrace, "to open the `switch` body")?;
        let mut arms = Vec::new();
        let mut else_body: Option<Vec<Stmt>> = None;
        loop {
            match &self.peek().kind {
                TokKind::RBrace => {
                    self.bump();
                    break;
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this `switch`, found the end of the file"
                            .to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                TokKind::Pipe => {
                    let arm_start = self.bump().span;
                    if matches!(self.peek().kind, TokKind::KwElse) {
                        self.bump();
                        self.expect(TokKind::LBrace, "to open the `else` arm")?;
                        let body = self.block_stmts();
                        if matches!(self.peek().kind, TokKind::Semi) {
                            self.bump();
                        }
                        else_body = Some(body);
                    } else {
                        let cond = self.parse_arm_value_cond(&subject, BinOp::Eq)?;
                        self.expect(TokKind::LBrace, "to open the arm's body")?;
                        let body = self.block_stmts();
                        let end = self.peek().span.end;
                        if matches!(self.peek().kind, TokKind::Semi) {
                            self.bump();
                        }
                        arms.push(SwitchArm {
                            cond,
                            body,
                            span: Span::new(arm_start.start, end),
                        });
                    }
                }
                TokKind::Ident(name)
                    if false
                        && (name == Syntax::FOREIGN_CASE || name == Syntax::FOREIGN_DEFAULT) =>
                {
                    let t = self.bump();
                    let foreign = if let TokKind::Ident(n) = &t.kind {
                        n.clone()
                    } else {
                        unreachable!()
                    };
                    self.diags.push(Diagnostic::error(
                        "E0023",
                        format!(
                            "`{}` arms are written `value {} body`, not `{}`",
                            Syntax::KW_IF,
                            Syntax::OP_ARM_ARROW,
                            foreign
                        ),
                        format!(
                            "choosing one branch from many uses `{}` with `{}` arms (D-IF1)",
                            Syntax::KW_IF,
                            Syntax::OP_ARM_ARROW
                        ),
                        format!(
                            "replace `{}` with a value or condition and `{}`, like `1 {} body`",
                            foreign,
                            Syntax::OP_ARM_ARROW,
                            Syntax::OP_ARM_ARROW
                        ),
                        Some(t.span),
                    ));
                    self.sync_stmt();
                    continue;
                }
                TokKind::KwElse => {
                    self.bump();
                    self.expect(TokKind::Arrow, "after `else` in a `switch`")?;
                    self.expect(TokKind::LBrace, "to open the `else` arm")?;
                    let body = self.block_stmts();
                    self.expect(TokKind::Semi, "after a `switch` arm's closing `}`")?;
                    else_body = Some(body);
                }
                _ => {
                    let arm_start = self.peek().span;
                    // D-ENUMDOT1: `.Variant` or `.Variant(binding)` as a switch arm head.
                    // D-PATR: detect `Int .. Int ->` as a range-pattern arm head.
                    // C25: also detect `Int ..= Int ->` (E0318) and `Int .. Int step N ->` (E0319).
                    let cond = if matches!(&self.peek().kind, TokKind::Dot)
                        && self.toks.get(self.pos + 1)
                            .and_then(|token| leading_dot_variant(&token.kind))
                            .is_some() {
                        let dot_span = self.bump().span; // consume `.`
                        let variant_token = self.bump();
                        let variant = leading_dot_variant(&variant_token.kind)
                            .expect("leading-dot variant guard and token must agree");
                        let variant_span = variant_token.span;
                        let (bindings, end) = if matches!(self.peek().kind, TokKind::LParen) {
                            self.bump(); // consume `(`
                            let mut bindings: Vec<crate::AST::PatSlot> = Vec::new();
                            if !matches!(self.peek().kind, TokKind::RParen) {
                                loop {
                                    let slot = if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::PAT_WILDCARD_SLOT)
                                    {
                                        self.bump();
                                        crate::AST::PatSlot::Wildcard
                                    } else if let TokKind::Int(lo_val, _) = &self.peek().kind.clone() {
                                        let lo = *lo_val;
                                        self.bump();
                                        if matches!(self.peek().kind, TokKind::DotDot) {
                                            self.bump();
                                            if let TokKind::Int(hi_val, _) = &self.peek().kind.clone()
                                            {
                                                let hi = *hi_val;
                                                self.bump();
                                                crate::AST::PatSlot::Range { lo, hi }
                                            } else {
                                                return Err(Diagnostic::error(
                                                    "E0003",
                                                    "expected an integer after `..` in a range pattern".to_string(),
                                                    "range patterns need both ends: `lo..hi`".to_string(),
                                                    "write `0..100` for an inclusive range".to_string(),
                                                    Some(self.peek().span),
                                                ));
                                            }
                                        } else {
                                            return Err(Diagnostic::error(
                                                "E0003",
                                                "expected `..` after the lower bound of a range pattern".to_string(),
                                                "range patterns need `lo..hi` syntax".to_string(),
                                                "write `0..100` for an inclusive range".to_string(),
                                                Some(self.peek().span),
                                            ));
                                        }
                                    } else {
                                        let (name, span) =
                                            self.expect_ident("for a pattern binding")?;
                                        crate::AST::PatSlot::Bind { name, span }
                                    };
                                    bindings.push(slot);
                                    if matches!(self.peek().kind, TokKind::RParen) {
                                        break;
                                    }
                                    self.expect(TokKind::Comma, "between pattern bindings")?;
                                }
                            }
                            self.expect(TokKind::RParen, "after pattern bindings")?;
                            let end = self.toks[self.pos.saturating_sub(1)].span.end;
                            (bindings, end)
                        } else {
                            (vec![], variant_span.end)
                        };
                        let pat_span = Span::new(dot_span.start, end);
                        let pattern = Pattern::Variant { variant, bindings, span: pat_span };
                        Expr::PatternTest {
                            subject: Box::new(subject.clone()),
                            pattern,
                            span: pat_span,
                        }
                    } else if let Some(range) = self.try_range_arm_head(&subject)? {
                        range
                    } else {
                        self.expr_no_struct_lit()?
                    };
                    self.expect(TokKind::Arrow, "after a `switch` arm's condition")?;
                    self.expect(TokKind::LBrace, "to open the arm's body")?;
                    let body = self.block_stmts();
                    // Capture the `;` end so SwitchArm.span covers the full arm.
                    let semi_end = self.peek().span.end;
                    self.expect(TokKind::Semi, "after a `switch` arm's closing `}`")?;
                    arms.push(SwitchArm {
                        cond,
                        body,
                        span: Span::new(arm_start.start, semi_end),
                    });
                }
            }
        }
        Ok(Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        })
    }

}
