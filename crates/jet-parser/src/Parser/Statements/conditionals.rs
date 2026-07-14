use super::super::*;

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

    /// D-IF3: `if subject == { … }` is multi-arm value/pattern dispatch; a plain
    /// `if cond { … }` is a conventional boolean test. The `==` marker between
    /// the subject and `{` is *required* to enter dispatch (Q2); an old-style
    /// `if subject { head -> body }` with no `==` is teaching error E0992, which
    /// recovers by parsing the body as arms so the rest of the file still checks.
    /// Multi-arm `if` lowers to the same `Stmt::Switch` IR the former `when` used.
    pub(super) fn if_or_dispatch(&mut self) -> Result<Stmt, Diagnostic> {
        let span = self.bump().span; // `if`

        // Parse the subject below comparison precedence so a trailing `==` marker
        // is left in the token stream (`expr_cmp` would eat it). If `== {`
        // follows, this is explicit dispatch.
        let probe = self.pos;
        let probe_diags = self.diags.len();
        if let Ok(subject) = self.expr_no_struct_lit_no_cmp() {
            if matches!(self.peek().kind, TokKind::EqEq)
                && matches!(self.peek2().kind, TokKind::LBrace)
            {
                self.bump(); // `==`
                self.expect(TokKind::LBrace, "to open the `if` dispatch body")?;
                return self.if_arms(subject, span);
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
        self.expect(TokKind::LBrace, "to open the `if` body")?;

        // D-IF3 / E0992: an old implicit-dispatch body (first item is `head ->`)
        // with no `==` marker. Teach the explicit form, then recover by parsing
        // the body as arms so the rest of the file's errors still surface (S14).
        if self.if_body_is_arms() {
            self.diags.push(Diagnostic::error(
                "E0992",
                format!(
                    "a multi-arm `{}` needs `==` between the subject and `{{`",
                    Syntax::KW_IF
                ),
                format!(
                    "`{} subject == {{ … }}` marks value dispatch explicitly, so a plain `{} cond {{ … }}` body is always statements",
                    Syntax::KW_IF,
                    Syntax::KW_IF
                ),
                format!("write `{} subject == {{ … }}`", Syntax::KW_IF),
                Some(span),
            ));
            return self.if_arms(cond, span);
        }

        // Conventional `if`: the condition gates a statement body.
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
        Ok(Stmt::If(IfStmt {
            cond,
            then_body,
            else_branch,
            span,
        }))
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
                if matches!(self.toks.get(self.pos + 3).map(|t| &t.kind), Some(TokKind::Ident(s)) if s == Syntax::KW_RANGE_STEP)
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

    /// D-IF3: parse the arms of an `if subject == { … }` dispatch (cursor just
    /// past `{`), lowering to `Stmt::Switch`. Each arm is `head -> body` where
    /// `head` is a bare value (`200`, `"sat" || "sun"`) compared against the
    /// subject, a bare range (`400..499`), or a bare pattern (`Active(id)`,
    /// `value(n)`, `ok(n)`, `null`) — the leading `subject ==` is dropped (Q4)
    /// and re-bound here. A predicate/Bool head is rejected with E0993; a leftover
    /// `subject ==` prefix with E0994. `body` is a braceless single statement or a
    /// `{ … }` block (D-IF2 Q2). `else -> body` is the catch-all (D-IF2 Q1).
    pub(super) fn if_arms(&mut self, subject: Expr, span: Span) -> Result<Stmt, Diagnostic> {
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
                    // D-PATR: detect `Int .. Int ->` as a range-pattern arm head.
                    // C25: also detect `Int ..= Int ->` (E0318) and `Int .. Int step N ->` (E0319).
                    let raw_head = if let TokKind::Int(lo_val, _) = &self.peek().kind.clone() {
                        if matches!(
                            self.toks.get(self.pos + 1).map(|t| &t.kind),
                            Some(TokKind::DotDot)
                        ) {
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
                                    Expr::PatternTest {
                                        subject: Box::new(pat_subject.clone()),
                                        pattern: Pattern::Range {
                                            lo,
                                            hi,
                                            span: pat_span,
                                        },
                                        span: pat_span,
                                    }
                                } else {
                                    return Err(Diagnostic::error(
                                        "E0318",
                                        "`..=` is not a Jet operator — Jet's `..` is already inclusive".to_string(),
                                        "in Rust, `..` is exclusive and `..=` is inclusive; in Jet, `..` includes both ends".to_string(),
                                        "write `lo..hi` — that already includes `hi`".to_string(),
                                        Some(self.peek().span),
                                    ));
                                }
                            } else if let TokKind::Int(hi_val, _) = &self.peek().kind.clone() {
                                let hi = *hi_val;
                                let range_end = self.bump().span; // consume hi
                                let pat_span = Span::new(range_start.start, range_end.end);
                                // C25/E0319: `step` after a range arm is a loop modifier, not an arm construct.
                                // Push the error and skip `step N` so the arm can still be parsed.
                                if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_RANGE_STEP)
                                {
                                    self.diags.push(Diagnostic::error(
                                        "E0319",
                                        "`step` is not allowed in a range arm — range arms test a band, not a sequence".to_string(),
                                        "`step` belongs in a loop (`loop i in lo..hi step n`); a range arm just checks if the subject falls between the two ends".to_string(),
                                        format!("remove `step …`, or use a full condition: `subject >= {} && subject <= {} && subject % n == 0 ->`", lo, hi),
                                        Some(pat_span),
                                    ));
                                    self.bump(); // consume `step`
                                    if matches!(self.peek().kind, TokKind::Int(_, _)) {
                                        self.bump(); // consume step value
                                    }
                                }
                                Expr::PatternTest {
                                    subject: Box::new(pat_subject.clone()),
                                    pattern: Pattern::Range {
                                        lo,
                                        hi,
                                        span: pat_span,
                                    },
                                    span: pat_span,
                                }
                            } else {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    "expected an integer after `..` in a range arm".to_string(),
                                    "range arms need both ends: `lo..hi -> body`".to_string(),
                                    "write `0..59 -> { body }` for an inclusive range arm"
                                        .to_string(),
                                    Some(self.peek().span),
                                ));
                            }
                        } else {
                            self.if_arm_head(&subject, &pat_subject)?
                        }
                    } else {
                        self.if_arm_head(&subject, &pat_subject)?
                    };
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

    /// D-IF3 / D-MATCHARM1: parse one bare arm head (no leading `subject ==`) and
    /// bind it to the subject.
    /// - A pattern head (`.Active(id)`, `A(x) | B(x)`) becomes a `PatternTest`.
    /// - Values use the D-MATCHARM1 grammar: `|` alternates values, `&&`/`||`
    ///   combine booleans, parens group. E0328 (D-MATCHARM2=B) fires for
    ///   unparenthesized mixing of `|` with `&&`/`||`.
    /// - A leftover redundant `subject ==` prefix emits E0994 and recovers.
    pub(super) fn if_arm_head(&mut self, subject: &Expr, pat_subject: &Expr) -> Result<Expr, Diagnostic> {
        // A bare pattern head: parse it standalone and attach the subject.
        let save = self.pos;
        if let Some(pattern) = self.try_pattern_rhs()? {
            // Only a pattern if it consumed up to the `->`; otherwise it was an
            // ordinary value — rewind and re-parse as the value grammar.
            if matches!(self.peek().kind, TokKind::Arrow) {
                let span = pat_span(&pattern);
                return Ok(Expr::PatternTest {
                    subject: Box::new(pat_subject.clone()),
                    pattern,
                    span,
                });
            }
            self.pos = save;
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
                Expr::Binary(BinOp::Eq, lhs, _, span) if Self::same_subject(lhs, subject) => {
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
        self.parse_arm_value_cond(subject)
    }

    pub(super) fn redundant_subject_diag(span: Span) -> Diagnostic {
        Diagnostic::error(
            "E0994",
            format!(
                "drop the `subject ==` — the `==` on the `{}` already applies it to every arm",
                Syntax::KW_IF
            ),
            format!(
                "`{} subject == {{ … }}` matches each arm head against the subject, so repeating `subject ==` on an arm is redundant",
                Syntax::KW_IF
            ),
            "delete the `subject ==` prefix; write just the value or pattern".to_string(),
            Some(span),
        )
    }

    // ── D-MATCHARM1 arm-head grammar ─────────────────────────────────────────

    /// Entry point: `arm_bool_expr = arm_and_expr ("||" arm_and_expr)*`
    pub(super) fn parse_arm_value_cond(&mut self, subject: &Expr) -> Result<Expr, Diagnostic> {
        let lhs = self.parse_arm_and_cond(subject)?;
        if !matches!(self.peek().kind, TokKind::OrOr) {
            return Ok(lhs);
        }
        let mut parts = vec![lhs];
        while matches!(self.peek().kind, TokKind::OrOr) {
            self.bump();
            parts.push(self.parse_arm_and_cond(subject)?);
        }
        Ok(parts
            .into_iter()
            .reduce(|a, b| {
                let span = Span::new(a.span().start, b.span().end);
                Expr::Binary(BinOp::Or, Box::new(a), Box::new(b), span)
            })
            .unwrap())
    }

    /// `arm_and_expr = arm_alternates ("&&" arm_alternates)*`
    /// D-MATCHARM2=B: if LHS has >1 alternates and `&&` follows → E0328.
    pub(super) fn parse_arm_and_cond(&mut self, subject: &Expr) -> Result<Expr, Diagnostic> {
        let (lhs_cond, lhs_count) = self.parse_arm_alternates_cond(subject)?;
        if !matches!(self.peek().kind, TokKind::AndAnd) {
            return Ok(lhs_cond);
        }
        if lhs_count > 1 {
            let span = self.peek().span;
            self.diags.push(Diagnostic::error(
                "E0328",
                "value alternates mixed with `&&` without grouping parentheses".to_string(),
                "`|` and `&&` at the same arm-head level are ambiguous to read (D-MATCHARM2)".to_string(),
                "group the alternates: `(400 | 404) && condition` instead of `400 | 404 && condition`".to_string(),
                Some(span),
            ));
            // Sync to arm body delimiter for recovery.
            while !matches!(
                self.peek().kind,
                TokKind::Arrow | TokKind::LBrace | TokKind::RBrace | TokKind::Eof
            ) {
                self.bump();
            }
            return Ok(lhs_cond);
        }
        let mut parts = vec![lhs_cond];
        while matches!(self.peek().kind, TokKind::AndAnd) {
            self.bump();
            let (rhs_cond, _) = self.parse_arm_alternates_cond(subject)?;
            parts.push(rhs_cond);
        }
        Ok(parts
            .into_iter()
            .reduce(|a, b| {
                let span = Span::new(a.span().start, b.span().end);
                Expr::Binary(BinOp::And, Box::new(a), Box::new(b), span)
            })
            .unwrap())
    }

    /// `arm_alternates = arm_atom ("|" arm_atom)*`
    /// Returns `(condition_expr, alternate_count)`.
    pub(super) fn parse_arm_alternates_cond(&mut self, subject: &Expr) -> Result<(Expr, usize), Diagnostic> {
        let first = self.parse_arm_atom_cond(subject)?;
        let mut alts = vec![first];
        // Consume single `|` but not `||` (peek at the token after `|`).
        while matches!(self.peek().kind, TokKind::Pipe)
            && !matches!(
                self.toks.get(self.pos + 1).map(|t| &t.kind),
                Some(TokKind::Pipe)
            )
        {
            self.bump(); // consume `|`
            alts.push(self.parse_arm_atom_cond(subject)?);
        }
        let n = alts.len();
        if n == 1 {
            return Ok((alts.into_iter().next().unwrap(), 1));
        }
        // Each alt is already a condition (Eq(subject, value) or a comparison
        // from `parse_arm_atom_cond`). Just Or them together.
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
    /// Paren groups recurse with full arm semantics (inner `|` = alternation).
    pub(super) fn parse_arm_atom_cond(&mut self, subject: &Expr) -> Result<Expr, Diagnostic> {
        if matches!(self.peek().kind, TokKind::LParen) {
            self.bump(); // consume `(`
            let inner = self.parse_arm_value_cond(subject)?;
            self.expect(TokKind::RParen, "to close the arm head group")?;
            return Ok(inner);
        }
        // Single value: parse at comparison level so `&&`/`||` are left for the
        // outer arm-head grammar (`parse_arm_and_cond`/`parse_arm_value_cond`).
        // Setting `arm_head_term = true` stops `expr_bitor` before top-level `|`
        // so the caller (`parse_arm_alternates_cond`) handles alternation itself.
        let old = self.arm_head_term;
        self.arm_head_term = true;
        let raw = self.expr_cmp(false);
        self.arm_head_term = old;
        let raw = raw?;
        Ok(Self::arm_atom_to_cond(subject.clone(), raw))
    }

    /// Wrap a single value as a condition. Comparisons / PatternTest / Bool are
    /// kept as-is; plain values get `subject == value` wrapping.
    pub(super) fn arm_atom_to_cond(subject: Expr, value: Expr) -> Expr {
        match &value {
            Expr::Binary(op, ..) if op.is_comparison() => value,
            Expr::PatternTest { .. } | Expr::Bool(_, _) => value,
            _ => {
                let span = Span::new(subject.span().start, value.span().end);
                Expr::Binary(BinOp::Eq, Box::new(subject), Box::new(value), span)
            }
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
    /// S68 (D-SG2): parse an `if` expression — `if cond { … value } else { … }`.
    /// Each branch is a value block; `else` is required (an `if` with no value
    /// is a statement, parsed elsewhere).
    pub(in super::super) fn parse_if_expr(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.bump().span; // `if`
        let cond = self.expr_no_struct_lit()?;
        let (then_body, then_value) = self.parse_value_block()?;
        if !matches!(self.peek().kind, TokKind::KwElse) {
            return Err(Diagnostic::error(
                "E0003",
                "an `if` used as a value needs an `else` branch".to_string(),
                "when `if` produces a value, both branches must produce one (S68)".to_string(),
                "add `else { … }` so every path has a value".to_string(),
                Some(self.peek().span),
            ));
        }
        self.bump(); // `else`
                     // `else if …` nests as the else branch's value.
        let (else_body, else_value) = if matches!(self.peek().kind, TokKind::KwIf) {
            let e = self.parse_if_expr()?;
            (Vec::new(), e)
        } else {
            self.parse_value_block()?
        };
        let span = Span::new(start.start, else_value.span().end);
        Ok(Expr::If {
            cond: Box::new(cond),
            then_body,
            then_value: Box::new(then_value),
            else_body,
            else_value: Box::new(else_value),
            span,
        })
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
                        let cond = self.parse_arm_value_cond(&subject)?;
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
                    if retired_s14_teaching_enabled()
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
                        && matches!(
                            self.toks.get(self.pos + 1).map(|t| &t.kind),
                            Some(TokKind::Ident(_))
                        ) {
                        let dot_span = self.bump().span; // consume `.`
                        let (variant, variant_span) =
                            self.expect_ident("after `.` in a variant pattern")?;
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
                                        let (b, _) = self.expect_ident("for a pattern binding")?;
                                        crate::AST::PatSlot::Bind(b)
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
                        Expr::PatternTest {
                            subject: Box::new(subject.clone()),
                            pattern: Pattern::Variant {
                                variant,
                                bindings,
                                span: pat_span,
                            },
                            span: pat_span,
                        }
                    } else if let TokKind::Int(lo_val, _) = &self.peek().kind.clone() {
                        if matches!(
                            self.toks.get(self.pos + 1).map(|t| &t.kind),
                            Some(TokKind::DotDot)
                        ) {
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
                                    Expr::PatternTest {
                                        subject: Box::new(subject.clone()),
                                        pattern: Pattern::Range {
                                            lo,
                                            hi,
                                            span: pat_span,
                                        },
                                        span: pat_span,
                                    }
                                } else {
                                    return Err(Diagnostic::error(
                                        "E0318",
                                        "`..=` is not a Jet operator — Jet's `..` is already inclusive".to_string(),
                                        "in Rust, `..` is exclusive and `..=` is inclusive; in Jet, `..` includes both ends".to_string(),
                                        "write `lo..hi` — that already includes `hi`".to_string(),
                                        Some(self.peek().span),
                                    ));
                                }
                            } else if let TokKind::Int(hi_val, _) = &self.peek().kind.clone() {
                                let hi = *hi_val;
                                let range_end = self.bump().span; // consume hi
                                let pat_span = Span::new(range_start.start, range_end.end);
                                // C25/E0319: `step` after a range arm is a loop modifier, not an arm construct.
                                // Push the error and skip `step N` so the arm can still be parsed.
                                if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_RANGE_STEP)
                                {
                                    self.diags.push(Diagnostic::error(
                                        "E0319",
                                        "`step` is not allowed in a range arm — range arms test a band, not a sequence".to_string(),
                                        "`step` belongs in a loop (`loop i in lo..hi step n`); a range arm just checks if the subject falls between the two ends".to_string(),
                                        format!("remove `step …`, or use a full condition: `subject >= {} && subject <= {} && subject % n == 0 ->`", lo, hi),
                                        Some(pat_span),
                                    ));
                                    self.bump(); // consume `step`
                                    if matches!(self.peek().kind, TokKind::Int(_, _)) {
                                        self.bump(); // consume step value
                                    }
                                }
                                // Wrap as PatternTest so sema/codegen treat it uniformly.
                                Expr::PatternTest {
                                    subject: Box::new(subject.clone()),
                                    pattern: Pattern::Range {
                                        lo,
                                        hi,
                                        span: pat_span,
                                    },
                                    span: pat_span,
                                }
                            } else {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    "expected an integer after `..` in a range arm".to_string(),
                                    "range arms need both ends: `lo..hi -> body`".to_string(),
                                    "write `0..59 -> { body }` for an inclusive range arm"
                                        .to_string(),
                                    Some(self.peek().span),
                                ));
                            }
                        } else {
                            self.expr_no_struct_lit()?
                        }
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
