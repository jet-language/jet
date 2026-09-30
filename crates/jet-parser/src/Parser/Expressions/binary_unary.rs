use super::super::{
    leading_dot_variant, pat_span, BinOp, Diagnostic, EnumLitArg, Expr, OrFallback, Parser, Span,
    Stmt, StrTokPart, Syntax, TokKind, UnOp,
};

fn is_fallback_exit(stmt: &Stmt) -> bool {
    crate::Parser::statement_definitely_exits_value_block(stmt)
}

/// D-SHAPE-PLACE1=A / D-CAP-RECEIVER1=D: a `&` or `^` mark binds the maximal
/// place after it — one name plus fields, indexes, or ranges — and never a
/// call result. Before a place-rooted method chain it binds the place the
/// first call receives (`&buf.grow().len()` marks `buf` for `grow`); on a
/// bare place it is the whole place (`&values[0..1]`).
pub(super) fn mark_at_maximal_place(expr: Expr, access: crate::AST::PlaceAccess, start: usize) -> Expr {
    match expr {
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            owner_type_args,
            type_args,
            args,
            recv_type,
            resolved_ret,
            operator_rhs,
            checked_widen,
        } => Expr::MethodCall {
            receiver: Box::new(mark_at_maximal_place(*receiver, access, start)),
            method,
            method_span,
            owner_type_args,
            type_args,
            args,
            recv_type,
            resolved_ret,
            operator_rhs,
            checked_widen,
        },
        Expr::Field(base, member, span) if contains_method_call(&base) => {
            Expr::Field(Box::new(mark_at_maximal_place(*base, access, start)), member, span)
        }
        Expr::Index {
            base,
            index,
            span,
            kind,
        } if contains_method_call(&base) => Expr::Index {
            base: Box::new(mark_at_maximal_place(*base, access, start)),
            index,
            span,
            kind,
        },
        Expr::Slice {
            base,
            start: slice_start,
            end,
            range,
            span,
        } if contains_method_call(&base) => Expr::Slice {
            base: Box::new(mark_at_maximal_place(*base, access, start)),
            start: slice_start,
            end,
            range,
            span,
        },
        place => {
            let span = Span::new(start, place.span().end);
            Expr::Place(Box::new(place), access, span)
        }
    }
}

/// True when a projection chain passes through a method call, so a leading
/// mark belongs to the place that call receives rather than to the chain.
pub(super) fn contains_method_call(expr: &Expr) -> bool {
    match expr {
        Expr::MethodCall { .. } => true,
        Expr::Field(base, _, _) | Expr::Index { base, .. } | Expr::Slice { base, .. } => {
            contains_method_call(base)
        }
        _ => false,
    }
}

/// D-CONC-SHARE1=A: decide whether `shared` opens a shared-cell construction
/// (`shared expr`) or is an ordinary identifier. The keyword is contextual, so
/// this is a closed allow-list of tokens that begin the constructed value —
/// a name, a literal, a list/typed literal, or an inferred brace literal. Anything
/// else (a binding sigil, an operator, a `.`, a separator, a call `(`) leaves
/// `shared` as a plain name, which keeps existing bindings and arguments
/// spelled `shared` legal.
pub(super) fn starts_shared_operand(kind: &TokKind) -> bool {
    matches!(
        kind,
        TokKind::Ident(_)
            | TokKind::Int(..)
            | TokKind::Float(..)
            | TokKind::UnitNumber { .. }
            | TokKind::RawStr(_)
            | TokKind::Str(_)
            | TokKind::Char(_)
            | TokKind::KwTrue
            | TokKind::KwFalse
            | TokKind::KwSelf
            | TokKind::LBracket
            | TokKind::LBrace
    )
}

impl<'a> Parser<'a> {
    /// S35/S71: the `??` fallback binds looser than `&&` / `||`.
    pub(super) fn expr_or_fallback(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_range(allow_struct_lit)?;
        loop {
            match &self.peek().kind {
                TokKind::QuestionQuestion => {}
                // S71 (D-SG6): the retired word `or` — teach `??`, then recover.
                TokKind::Ident(n) if false && n == Syntax::FOREIGN_OR_FALLBACK => {
                    let span = self.peek().span;
                    self.diags.push(Diagnostic::error(
                            "E0045",
                            "Jet writes the fallback as `??`, not `or`".to_string(),
                            "`??` supplies a value when a `T?` is absent or a `T E!` failed — `count ?? 0`, `read() ?? return`"
                                .to_string(),
                            "replace `or` with `??`".to_string(),
                            Some(span),
                        ));
                }
                _ => break,
            }
            let op_span = self.bump().span;
            let fallback = self.parse_or_fallback(allow_struct_lit)?;
            let end = match &fallback {
                OrFallback::Value(e) => e.span().end,
                OrFallback::Block { span, .. } => span.end,
                OrFallback::Return(_, s) => s.end,
                OrFallback::Panic { name_span, .. } => name_span.end,
                OrFallback::Break(s)
                | OrFallback::Continue(s)
                | OrFallback::BreakLabel(_, s)
                | OrFallback::ContinueLabel(_, s) => s.end,
            };
            let span = Span::new(lhs.span().start, end.max(op_span.end));
            lhs = Expr::OrFallback {
                value: Box::new(lhs),
                fallback,
                is_option: false,
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_or_fallback(&mut self, allow_struct_lit: bool) -> Result<OrFallback, Diagnostic> {
        // D-LIT-DOT1: a field-led bare brace is a record literal. Keep
        // statement-shaped braces as fallback blocks.
        if matches!(self.peek().kind, TokKind::LBrace)
            && (!self.brace_starts_record() || matches!(self.peek2().kind, TokKind::RBrace))
        {
            return self.parse_or_fallback_block();
        }
        if matches!(self.peek().kind, TokKind::KwReturn) {
            let span = self.bump().span;
            if self.starts_expr(&self.peek().kind) {
                let e = self.expr_or(allow_struct_lit)?;
                return Ok(OrFallback::Return(Some(Box::new(e)), span));
            }
            return Ok(OrFallback::Return(None, span));
        }
        // D-LOOP-CONTROLWORD1=B: bare `?? next` is contextual control;
        // `?? (next)` remains an ordinary value fallback.
        if matches!(self.peek().kind, TokKind::KwBreak) {
            if matches!(self.peek2().kind, TokKind::LParen) {
                let start = self.bump().span;
                self.bump(); // `(`
                let (name, _) = self.expect_ident("as the loop name in `break(name)`")?;
                let end = self.peek().span.end;
                self.expect(TokKind::RParen, "after the named break target")?;
                return Ok(OrFallback::BreakLabel(name, Span::new(start.start, end)));
            }
            return Ok(OrFallback::Break(self.bump().span));
        }
        if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_NEXT)
            && (!matches!(self.peek2().kind, TokKind::LParen)
                || matches!(self.peek3().kind, TokKind::Ident(_))
                    && matches!(self.peek4().kind, TokKind::RParen))
        {
            if matches!(self.peek2().kind, TokKind::LParen) {
                let start = self.bump().span;
                self.bump(); // `(`
                let (name, _) = self.expect_ident("as the loop name in `next(name)`")?;
                let end = self.peek().span.end;
                self.expect(TokKind::RParen, "after the named next target")?;
                return Ok(OrFallback::ContinueLabel(name, Span::new(start.start, end)));
            }
            return Ok(OrFallback::Continue(self.bump().span));
        }
        let e = self.expr_range(allow_struct_lit)?;
        if let Expr::Call(call) = &e {
            if call.name == Syntax::BUILTIN_PANIC {
                return Ok(OrFallback::Panic {
                    name_span: call.name_span,
                    args: call.args.clone(),
                });
            }
        }
        Ok(OrFallback::Value(Box::new(e)))
    }

    /// D-FAIL-BIND1=A: a fallback block has ordinary statements followed
    /// by one bare value or one real exit. The old `return value` block
    /// value reading is retired; `return value` now returns from the fn.
    fn parse_or_fallback_block(&mut self) -> Result<OrFallback, Diagnostic> {
        let start = self.peek().span.start;
        self.expect(TokKind::LBrace, "to open the `??` fallback block")?;
        let mut body = Vec::new();
        loop {
            // S6-R: a nested block statement ends at `}`, so consume the
            // lexer-inserted zero-width terminator before the next fallback
            // item. Authored semicolons remain visible to the diagnostics.
            if matches!(self.peek().kind, TokKind::Semi)
                && self.peek().span.start == self.peek().span.end
            {
                self.bump();
                continue;
            }
            match &self.peek().kind {
                TokKind::RBrace => {
                    let span = self.peek().span;
                    self.bump();
                    if let Some(stmt) = body.last() {
                        if is_fallback_exit(stmt) {
                            let end = span.end;
                            return Ok(OrFallback::Block {
                                body,
                                value: None,
                                span: Span::new(start, end),
                            });
                        }
                        return Err(Diagnostic::error(
                            "E0114",
                            "this fallback ends with a statement instead of a value".to_string(),
                            "a value-expected fallback block must end with one unadorned expression; statements yield unit"
                                .to_string(),
                            "put one unadorned expression last, or use `return ...` for an early exit"
                                .to_string(),
                            Some(stmt.span()),
                        ));
                    }
                    return Err(Diagnostic::error(
                            "E0003",
                            "a `??` fallback block needs a final value or exit".to_string(),
                            "a fallback block runs its statements and then produces one value or leaves control"
                                .to_string(),
                            "add a final value or exit, such as `{ log(err); default_value }` or `{ log(err); return }`"
                                .to_string(),
                            Some(span),
                        ));
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close the `??` fallback block, found the end of the file"
                            .to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                _ => {}
            }

            // A control statement is a real exit only when it is the tail.
            // If more source follows, restore and parse it as an ordinary
            // body statement so the normal block diagnostic remains local.
            if matches!(self.peek().kind, TokKind::KwReturn | TokKind::KwBreak)
                || matches!(&self.peek().kind, TokKind::Ident(name) if name == Syntax::KW_NEXT)
            {
                let save = self.pos;
                let saved_diags = self.diags.len();
                if let Ok(stmt) = self.stmt() {
                    if is_fallback_exit(&stmt) && matches!(self.peek().kind, TokKind::RBrace) {
                        let end = self.bump().span.end;
                        let mut body = body;
                        body.push(stmt);
                        return Ok(OrFallback::Block {
                            body,
                            value: None,
                            span: Span::new(start, end),
                        });
                    }
                }
                self.pos = save;
                self.diags.truncate(saved_diags);
            }

            // As with value-if blocks, speculate on a trailing ordinary
            // expression before parsing a statement.
            let save = self.pos;
            let saved_diags = self.diags.len();
            if let Ok(value) = self.expr() {
                if matches!(self.peek().kind, TokKind::Semi)
                    && matches!(self.peek2().kind, TokKind::RBrace)
                {
                    let semi = self.peek().span;
                    if semi.start != semi.end {
                        let diverges = matches!(value.without_parens(), Expr::Todo { .. })
                            || matches!(
                                value.without_parens(),
                                Expr::Call(call) if call.name == Syntax::BUILTIN_PANIC
                            );
                        if !diverges {
                            return Err(Diagnostic::error(
                                "E0114",
                                "this fallback ends with a semicolon-terminated expression"
                                    .to_string(),
                                "a value-expected fallback block must end with one unadorned expression; a semicolon-terminated expression yields unit"
                                    .to_string(),
                                "remove the semicolon, or use `return ...` for an early exit"
                                    .to_string(),
                                Some(value.span()),
                            ));
                        }
                    }
                    self.bump();
                }
                if matches!(self.peek().kind, TokKind::RBrace) {
                    let end = self.bump().span.end;
                    let is_exit = matches!(value.without_parens(), Expr::Todo { .. })
                        || matches!(
                            value.without_parens(),
                            Expr::Call(call) if call.name == Syntax::BUILTIN_PANIC
                        );
                    if is_exit {
                        let mut body = body;
                        body.push(Stmt::Expr(value));
                        return Ok(OrFallback::Block {
                            body,
                            value: None,
                            span: Span::new(start, end),
                        });
                    }
                    return Ok(OrFallback::Block {
                        body,
                        value: Some(Box::new(value)),
                        span: Span::new(start, end),
                    });
                }
            }
            self.pos = save;
            self.diags.truncate(saved_diags);
            match self.stmt() {
                Ok(stmt) => body.push(stmt),
                Err(diag) => {
                    self.diags.push(diag);
                    self.sync_stmt();
                }
            }
        }
    }

    /// D-RANGE-VALUE1=A: ranges are ordinary values and bind looser than
    /// Boolean operators. Arm heads and distinct constraints keep their
    /// dedicated literal-only parsers.
    fn expr_range(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let start = self.expr_or(allow_struct_lit)?;
        let exclusive = match self.peek().kind {
            TokKind::DotDot => false,
            TokKind::DotDotLt => true,
            _ => return Ok(start),
        };
        self.bump();
        let end = self.expr_or(allow_struct_lit)?;
        let span = Span::new(start.span().start, end.span().end);
        Ok(Expr::Range {
            start: Box::new(start),
            end: Box::new(end),
            exclusive,
            span,
        })
    }

    fn expr_or(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_and(allow_struct_lit)?;
        // D-ARMHEAD-PAREN1 (2026-09-28 amendment): an unparenthesized `&&`
        // operand of `||` hides the precedence; report each one after the
        // whole chain is known.
        let mut and_operands = Vec::new();
        if matches!(lhs, Expr::Binary(BinOp::And, ..)) {
            and_operands.push(lhs.span());
        }
        let mut saw_or = false;
        loop {
            let is_or = matches!(self.peek().kind, TokKind::OrOr);
            if !is_or {
                break;
            }
            saw_or = true;
            let op_span = self.bump().span;
            let rhs = self.expr_and(allow_struct_lit)?;
            if matches!(rhs, Expr::Binary(BinOp::And, ..)) {
                and_operands.push(rhs.span());
            }
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs), span);
        }
        if saw_or {
            for operand in and_operands {
                self.mixed_logic_error(operand);
            }
        }
        Ok(lhs)
    }

    /// E0082: `a || b && c` must spell the grouping `a || (b && c)`. The Safe
    /// edit inserts exactly the parentheses precedence already implies.
    fn mixed_logic_error(&mut self, operand: Span) {
        let grouped = self
            .source
            .as_deref()
            .and_then(|source| source.get(operand.start..operand.end))
            .map(|text| format!("({text})"));
        let diagnostic = Diagnostic::from_row(
            "E0082",
            &[("grouped", grouped.as_deref().unwrap_or("(… && …)"))],
            Some(operand),
        );
        let diagnostic = match grouped {
            Some(new_text) => diagnostic.with_edit(crate::Diagnostics::TextEdit {
                span: operand,
                new_text,
            }),
            None => diagnostic,
        };
        self.diags.push(diagnostic);
    }

    fn expr_and(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_cmp(allow_struct_lit)?;
        loop {
            let is_and = match &self.peek().kind {
                TokKind::AndAnd => true,
                TokKind::Ident(n) if false && n == Syntax::FOREIGN_AND => {
                    self.foreign_logic_error(Syntax::FOREIGN_AND, Syntax::OP_AND);
                    true
                }
                _ => false,
            };
            if !is_and {
                break;
            }
            let op_span = self.bump().span;
            let rhs = self.expr_cmp(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::And, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    /// D-CHAINCMP1: a run of same-direction relational operators (`<`/`<=`/
    /// `>`/`>=`) chains into `Expr::CompareChain`, any length. `==`/`!=` never
    /// chain (with each other or with a relational op) — `a == b == c` and
    /// `a < b == c` both stay the pre-existing "comparisons can't be chained"
    /// error (E0003). A mixed-direction relational chain (`a < b > c`) is
    /// E0333, naming the direction break.
    pub(in crate::Parser) fn expr_cmp(
        &mut self,
        allow_struct_lit: bool,
    ) -> Result<Expr, Diagnostic> {
        let lhs = self.expr_bitor(allow_struct_lit)?;
        if matches!(self.peek().kind, TokKind::KwIn) {
            let span = self.bump().span;
            return Err(Diagnostic::from_row("E0384", &[], Some(span)));
        }
        let op = match &self.peek().kind {
            TokKind::EqEq => Some(BinOp::Eq),
            TokKind::NotEq => Some(BinOp::Ne),
            TokKind::Lt => Some(BinOp::Lt),
            TokKind::Gt if self.module_arg_expr_depth != Some(self.depth) => Some(BinOp::Gt),
            TokKind::Le => Some(BinOp::Le),
            TokKind::Ge => Some(BinOp::Ge),
            TokKind::Compare => Some(BinOp::Compare),
            _ => None,
        };
        let Some(op) = op else { return Ok(lhs) };
        let op_span = self.bump().span;
        if op == BinOp::Eq {
            if let Some(pat) = self.try_pattern_rhs(allow_struct_lit)? {
                let span = Span::new(lhs.span().start, pat_span(&pat).end.max(op_span.end));
                return Ok(Expr::PatternTest {
                    subject: Box::new(lhs),
                    pattern: pat,
                    span,
                });
            }
        }
        let rhs = self.expr_bitor(allow_struct_lit)?;

        // D-CMP3WAY1=B: spaceship comparison is one binary operation, not
        // a relational chain. User-defined values are rewritten by sema
        // to their `compare` hook; built-ins retain this node for lowering.
        if op == BinOp::Compare {
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            let cmp = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
            if let Some(second) = self.peek_cmp_span() {
                return Err(self.chained_eq_error(second));
            }
            return Ok(cmp);
        }

        // `==`/`!=` never chain — D-CHAINCMP1 excludes them. Reproduce the
        // pre-existing behavior exactly: any further relational/equality
        // token after an `==`/`!=` pair is E0003.
        if op == BinOp::Eq || op == BinOp::Ne {
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            let cmp = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
            if let Some(second) = self.peek_cmp_span() {
                return Err(self.chained_eq_error(second));
            }
            return Ok(cmp);
        }

        // Relational op: collect a same-direction chain.
        let ascending = matches!(op, BinOp::Lt | BinOp::Le);
        let mut operands = vec![lhs, rhs];
        let mut ops = vec![op];
        loop {
            let next = match &self.peek().kind {
                TokKind::Lt => Some(BinOp::Lt),
                TokKind::Le => Some(BinOp::Le),
                TokKind::Gt => Some(BinOp::Gt),
                TokKind::Ge => Some(BinOp::Ge),
                TokKind::EqEq | TokKind::NotEq => {
                    // `==`/`!=` can't extend a relational chain either.
                    let bad_span = self.peek().span;
                    return Err(self.chained_eq_error(bad_span));
                }
                _ => None,
            };
            let Some(next_op) = next else { break };
            let next_ascending = matches!(next_op, BinOp::Lt | BinOp::Le);
            if next_ascending != ascending {
                let bad_span = self.peek().span;
                return Err(Diagnostic::error(
                        "E0333",
                        "this comparison chain changes direction".to_string(),
                        "a chain like `0 <= sev < 10` reads in one direction; mixing `<` and `>` in one chain is almost always a mistake and has no single meaning".to_string(),
                        "split it into two comparisons joined with `&&`".to_string(),
                        Some(bad_span),
                    ));
            }
            self.bump();
            let next_rhs = self.expr_bitor(allow_struct_lit)?;
            operands.push(next_rhs);
            ops.push(next_op);
        }

        let span = Span::new(
            operands.first().unwrap().span().start,
            operands.last().unwrap().span().end,
        );
        if ops.len() == 1 {
            let rhs = operands.pop().unwrap();
            let lhs = operands.pop().unwrap();
            return Ok(Expr::Binary(ops[0], Box::new(lhs), Box::new(rhs), span));
        }
        Ok(Expr::CompareChain {
            hooks: vec![false; ops.len()],
            operands,
            ops,
            span,
        })
    }

    fn peek_cmp_span(&self) -> Option<Span> {
        match &self.peek().kind {
            TokKind::EqEq
            | TokKind::NotEq
            | TokKind::Lt
            | TokKind::Gt
            | TokKind::Le
            | TokKind::Ge
            | TokKind::Compare => Some(self.peek().span),
            _ => None,
        }
    }

    fn chained_eq_error(&self, second: Span) -> Diagnostic {
        Diagnostic::error(
            "E0003",
            "comparisons can't be chained".to_string(),
            format!(
                "`a < b < c` doesn't compare all three; check each pair and join with `{}`",
                Syntax::OP_AND
            ),
            format!("write `a < b {} b < c`", Syntax::OP_AND),
            Some(second),
        )
    }

    /// D-BITOREXPR1=A: `|` is bitwise OR in value position. Keep it above
    /// comparisons; type position has its own `|` union grammar.
    pub(in crate::Parser) fn expr_bitor(
        &mut self,
        allow_struct_lit: bool,
    ) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_bitxor(allow_struct_lit)?;
        while matches!(self.peek().kind, TokKind::Pipe)
            && !matches!(self.peek2().kind, TokKind::Gt)
        {
            let op_span = self.bump().span;
            let rhs = self.expr_bitxor(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::BitOr, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    /// D-XORSPELL1=A: `~|` is bitwise exclusive-or, in the precedence slot
    /// the old `^` spelling held.
    pub(in crate::Parser) fn expr_bitxor(
        &mut self,
        allow_struct_lit: bool,
    ) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_bitand(allow_struct_lit)?;
        while matches!(self.peek().kind, TokKind::TildePipe) {
            let op_span = self.bump().span;
            let rhs = self.expr_bitand(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::BitXor, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_bitand(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_shift(allow_struct_lit)?;
        while matches!(self.peek().kind, TokKind::Amp) {
            let op_span = self.bump().span;
            let rhs = self.expr_shift(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::BitAnd, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_shift(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_add(allow_struct_lit)?;
        loop {
            let op = match &self.peek().kind {
                TokKind::Shl => BinOp::Shl,
                TokKind::Shr => BinOp::Shr,
                _ => break,
            };
            let op_span = self.bump().span;
            let rhs = self.expr_add(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_add(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_mul(allow_struct_lit)?;
        loop {
            let op = match &self.peek().kind {
                TokKind::Plus => BinOp::Add,
                TokKind::Minus => BinOp::Sub,
                _ => break,
            };
            let op_span = self.bump().span;
            let rhs = self.expr_mul(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_mul(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_unary(allow_struct_lit)?;
        loop {
            let op = match &self.peek().kind {
                TokKind::Star => BinOp::Mul,
                TokKind::Slash => BinOp::Div,
                // D-FLOORDIV1=A: `/%` sits with the other division-family
                // operators, so `a /% b * c` groups left to right.
                TokKind::SlashPercent => BinOp::FloorDiv,
                // D-MODSEM1=A: `%` is the floored modulo, `%%` the
                // truncated remainder. Both sit at the division level.
                TokKind::Percent => BinOp::Mod,
                TokKind::PercentPercent => BinOp::Rem,
                _ => break,
            };
            let op_span = self.bump().span;
            let rhs = self.expr_unary(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    pub(super) fn expr_unary(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let span = self.peek().span;
        self.with_nesting(span, |p| p.expr_unary_inner(allow_struct_lit))
    }

    fn expr_unary_inner(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        match &self.peek().kind {
            // D-INCR1 is retired: prefix `++x` / `--x` teaches E0160 and
            // keeps the operand so `--x` never parses as `-(-x)`.
            TokKind::PlusPlus | TokKind::MinusMinus => {
                let op_tok = self.bump();
                let increment = matches!(op_tok.kind, TokKind::PlusPlus);
                let inner = self.expr_unary(allow_struct_lit)?;
                let whole = Span::new(op_tok.span.start, inner.span().end);
                self.teach_retired_step(increment, &inner, whole, true);
                Ok(inner)
            }
            TokKind::Minus => {
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Unary(UnOp::Neg, Box::new(inner), full))
            }
            TokKind::Bang => {
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Unary(UnOp::Not, Box::new(inner), full))
            }
            TokKind::Ident(n)
                if false && n == Syntax::FOREIGN_NOT && self.starts_expr(&self.peek2().kind) =>
            {
                self.foreign_logic_error(Syntax::FOREIGN_NOT, Syntax::OP_NOT);
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Unary(UnOp::Not, Box::new(inner), full))
            }
            TokKind::Ident(n)
                if false && n == Syntax::FOREIGN_TRY && self.starts_expr(&self.peek2().kind) =>
            {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0014",
                    format!(
                        "{} does not use `{}`",
                        Syntax::LANG_NAME,
                        Syntax::FOREIGN_TRY
                    ),
                    format!(
                        "a call that can fail is marked with `{}` after it, like `parse(x){}`",
                        Syntax::OP_TRY_SUFFIX,
                        Syntax::OP_TRY_SUFFIX
                    ),
                    format!("write `parse(x){}` instead", Syntax::OP_TRY_SUFFIX),
                    Some(t.span),
                ));
                self.expr_unary(allow_struct_lit)
            }
            TokKind::Star => {
                // D-CAP9: prefix `*x` is raw-pointer-of (take a raw pointer to
                // `x`), gated to `#Unsafe`. Dereference moved to postfix `p.*`.
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::RawOf(Box::new(inner), full))
            }
            // D-SHAPE-PLACE1=A: `&place` is an expression-level exclusive
            // write window. It is distinct from call-argument convention;
            // sema proves the operand is a maximal place.
            // D-CAP-RECEIVER1=D: `&` before a place-rooted method chain marks
            // the place the first call receives (`&buf.append(x)`).
            TokKind::Amp => {
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                Ok(mark_at_maximal_place(inner, crate::AST::PlaceAccess::Write, span.start))
            }
            // D-CAP-RECEIVER1=D: `^` marks the place a method call takes
            // (`^buf.seal()`). D-COPY-DEFAULT1=A (#3714): before a bare place
            // (a name plus fields or indexes) in any value position it is the
            // programmer's exact move (`ys := ^xs`, `Box{xs: ^xs}`, `^xs` as
            // a result); sema checks the place and records the move. Only a
            // fresh value (a literal or call result) has nothing to move.
            TokKind::Caret => {
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                if contains_method_call(&inner) {
                    return Ok(mark_at_maximal_place(
                        inner,
                        crate::AST::PlaceAccess::Take,
                        span.start,
                    ));
                }
                if matches!(inner, Expr::Ident(..) | Expr::Field(..) | Expr::Index { .. }) {
                    let full = Span::new(span.start, inner.span().end);
                    return Ok(Expr::Place(Box::new(inner), crate::AST::PlaceAccess::Take, full));
                }
                self.diags.push(Diagnostic::error(
                    "E0225",
                    format!(
                        "the move marker `{}` here marks a fresh value, not a named place",
                        Syntax::SIGIL_MOVE
                    ),
                    format!(
                        "`{}` moves a named place (`{}xs`, `{}buf.seal()`); a literal or call result is already new, so it needs no mark",
                        Syntax::SIGIL_MOVE,
                        Syntax::SIGIL_MOVE,
                        Syntax::SIGIL_MOVE
                    ),
                    format!("remove `{}`", Syntax::SIGIL_MOVE),
                    Some(Span::new(span.start, span.end)),
                ));
                Ok(inner)
            }
            // D-CONC-SHARE1=A: `shared expr` is the one shared-cell
            // construction form (it replaced the retired `Shared.new(x)`
            // call). It is contextual: `shared` stays an ordinary
            // identifier unless the next token opens a value, so a
            // binding or argument still reads as a name. Desugared here
            // to the `Shared`-receiver constructor shape every tier
            // already lowers, so no execution tier grows a second
            // sharing mechanism (I9). The cell owns its payload, so the
            // value is consumed the way a struct literal's field value is
            // — construction, not a call argument, so no `^` is written.
            TokKind::Ident(word)
                if word == Syntax::KW_SHARED && starts_shared_operand(&self.peek2().kind) =>
            {
                let span = self.bump().span; // `shared`
                let inner = self.expr_unary(allow_struct_lit)?;
                Ok(Expr::MethodCall {
                    receiver: Box::new(Expr::Ident(Syntax::TYPE_SHARED.to_string(), span)),
                    method: "new".to_string(),
                    method_span: span,
                    owner_type_args: Vec::new(),
                    type_args: Vec::new(),
                    args: vec![crate::AST::CallArg {
                        convention: crate::AST::AccessConvention::Move,
                        span: inner.span(),
                        expr: inner,
                        flags: Default::default(),
                        label: None,
                        spread: false,
                    }],
                    recv_type: None,
                    resolved_ret: None,
                    operator_rhs: None,
                    checked_widen: false,
                })
            }
            // D-SHAPE-COPY1=A: `~x` — the one copy sigil, a prefix-verb
            // expression form. Legal on any expression; most useful on a named
            // binding. `.clone()` is not user-typable Jet syntax (I8).
            TokKind::Tilde => {
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Copy(Box::new(inner), full))
            }
            // D-SHAPE-COPY1=A: the `copy` word is retired — copy is now the
            // `~` sigil (was D-CAP2/S4). Teach, then recover as Expr::Copy so
            // parsing continues.
            TokKind::KwCopy => {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0991",
                    format!(
                        "`{}` is now the `{}` sigil",
                        Syntax::KW_COPY,
                        Syntax::SIGIL_COPY
                    ),
                    format!(
                        "Jet has exactly one spelling for a copy — the `{}` sigil \
                             (D-SHAPE-COPY1) — so all code reads the same",
                        Syntax::SIGIL_COPY
                    ),
                    format!(
                        "write `{}name` in place of `{} name`",
                        Syntax::SIGIL_COPY,
                        Syntax::KW_COPY
                    ),
                    Some(span),
                ));
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Copy(Box::new(inner), full))
            }
            // D-LIT-DOT1: a non-empty `{ … }` in value position is an
            // inferred literal. `allow_struct_lit` is the existing
            // value-vs-block seam.
            TokKind::LBrace if allow_struct_lit && self.brace_starts_inferred_literal() => {
                let start = self.peek().span.start;
                self.struct_lit_inferred(start)
            }
            // D-LIT-DOT1: `.{ … }` is retired. `jet fmt` still rewrites it;
            // compile (`jet` / `jetpack`) does not accept it.
            TokKind::Dot if matches!(self.peek2().kind, TokKind::LBrace) => {
                let dot = self.bump();
                let diagnostic = Diagnostic::error(
                    "E0320",
                    "inferred record construction uses `{…}`, not `.{…}`".to_string(),
                    "D-LIT-DOT1 drops the constructor dot from every literal head".to_string(),
                    "write `{…}`".to_string(),
                    Some(dot.span),
                );
                if !self.migration_mode {
                    return Err(diagnostic);
                }
                self.diags.push(diagnostic);
                self.struct_lit_inferred(dot.span.start)
            }
            // D-SHAPE3a=A: `.new(...)` leaves only the static receiver implicit.
            // Sema fills the empty, unspellable identifier from the ordinary
            // expected type; downstream lowering then sees the same MethodCall as
            // an explicit `Type.new(...)`.
            TokKind::Dot if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MEM_ALLOC_NEW) =>
            {
                let dot = self.bump().span;
                let (method, method_span) = self.expect_field_name()?;
                self.expect(TokKind::LParen, "after `.new`")?;
                let mut args = Vec::new();
                if !matches!(self.peek().kind, TokKind::RParen) {
                    loop {
                        args.push(self.call_arg()?);
                        if matches!(self.peek().kind, TokKind::RParen) {
                            break;
                        }
                        self.expect_list_separator("between arguments", "call")?;
                        if matches!(self.peek().kind, TokKind::RParen) {
                            break;
                        }
                    }
                }
                self.expect_closer(TokKind::RParen, "to finish the call", "call")?;
                Ok(Expr::MethodCall {
                    receiver: Box::new(Expr::Ident(String::new(), dot)),
                    method,
                    method_span,
                    owner_type_args: Vec::new(),
                    type_args: Vec::new(),
                    args,
                    recv_type: None,
                    resolved_ret: None,
                    operator_rhs: None,
                    checked_widen: false,
                })
            }
            // D-ENUMDOT2=A: `.Variant`, `.Variant(arg)`, or `.Variant.{ field: val }` in
            // value position. An uppercase ident after `.` with no receiver is a
            // leading-dot enum literal. type_name="" is the unresolved sentinel;
            // sema fills it in via expected_type.
            TokKind::Dot if leading_dot_variant(&self.peek2().kind).is_some() => {
                let dot_start = self.bump().span.start; // consume `.`
                let variant_token = self.bump();
                let mut variant = leading_dot_variant(&variant_token.kind)
                    .expect("leading-dot variant guard and token must agree");
                let mut variant_span = variant_token.span;
                // D-TAG1: a dotted leaf path (`.Fire.Burn`) — further uppercase
                // segments extend the variant path. `.{` never matches (its second
                // token is `{`, not an ident), so named-payload construction below
                // still sees its `.` intact.
                while matches!(self.peek().kind, TokKind::Dot)
                    && matches!(&self.peek2().kind, TokKind::Ident(n) if n.chars().next().map_or(false, |c| c.is_uppercase()))
                {
                    self.bump(); // consume `.`
                    let (seg, seg_span) =
                        self.expect_ident("after `.` in a leading-dot enum variant")?;
                    variant = format!("{variant}.{seg}");
                    variant_span = Span::new(variant_span.start, seg_span.end);
                }
                // D-LIT-DOT1: enum payload fields touch the variant name.
                let (args, end) = if allow_struct_lit && matches!(self.peek().kind, TokKind::LBrace)
                {
                    self.enum_lit_named_fields()?
                } else if matches!(self.peek().kind, TokKind::Dot)
                    && matches!(self.peek2().kind, TokKind::LBrace)
                {
                    // Retired `.Variant.{ … }`. Fmt rewrites; compile rejects.
                    let dot = self.bump();
                    let diagnostic = Diagnostic::error(
                        "E0320",
                        format!(
                            "enum payload uses `.{}{{…}}`, not `.{}.{{…}}`",
                            variant, variant
                        ),
                        "D-LIT-DOT1 drops the constructor dot before payload fields".to_string(),
                        format!("write `.{}{{…}}`", variant),
                        Some(dot.span),
                    );
                    if !self.migration_mode {
                        return Err(diagnostic);
                    }
                    self.diags.push(diagnostic);
                    self.enum_lit_named_fields()?
                } else if matches!(self.peek().kind, TokKind::LParen) {
                    self.bump(); // consume `(`
                    let mut args = Vec::new();
                    if !matches!(self.peek().kind, TokKind::RParen) {
                        loop {
                            let e = self.expr()?;
                            args.push(EnumLitArg::Positional(e));
                            if matches!(self.peek().kind, TokKind::RParen) {
                                break;
                            }
                            self.expect(TokKind::Comma, "between enum variant arguments")?;
                            if matches!(self.peek().kind, TokKind::RParen) {
                                break;
                            }
                        }
                    }
                    self.expect(TokKind::RParen, "to close the enum variant arguments")?;
                    let close_end = self.toks[self.pos - 1].span.end;
                    (args, close_end)
                } else {
                    (Vec::new(), variant_span.end)
                };
                Ok(Expr::EnumLit {
                    type_name: String::new(),
                    variant,
                    variant_span: Some(variant_span),
                    args,
                    leading_dot: true,
                    span: Span::new(dot_start, end),
                })
            }
            // D-JPK-BUILDRECIPE1: executable adapter steps are the one
            // lower-case leading-dot value surface. The parser keeps them
            // as the ordinary inferred enum-literal node so module
            // consumers and formatters see one value shape.
            TokKind::Dot
                if self.allow_lowercase_leading_dot
                    && matches!(&self.peek2().kind, TokKind::Ident(name) if name.chars().next().is_some_and(char::is_lowercase)) =>
            {
                let dot_start = self.bump().span.start;
                let (variant, variant_span) = self.expect_ident("after `.` in a build step")?;
                let (args, end) = if matches!(self.peek().kind, TokKind::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek().kind, TokKind::RParen) {
                        loop {
                            let arg = if matches!(self.peek().kind, TokKind::Ident(_))
                                && matches!(self.peek2().kind, TokKind::Colon)
                            {
                                let (label, _) =
                                    self.expect_ident("for a named build-step argument")?;
                                self.bump();
                                let expr = if label == Syntax::RECIPE_STEP_FIELD_ARGS {
                                    self.build_step_string_list()?
                                } else {
                                    self.build_step_string()?
                                };
                                EnumLitArg::Named { label, expr }
                            } else {
                                return Err(Diagnostic::error(
                                        "E0003",
                                        "build-step arguments need field labels".to_string(),
                                        "finite build actions name every tool, path, URL, hash, and argument list".to_string(),
                                        "write `field: value`, for example `tool: \"cc\"`".to_string(),
                                        Some(self.peek().span),
                                    ));
                            };
                            args.push(arg);
                            if matches!(self.peek().kind, TokKind::RParen) {
                                break;
                            }
                            self.expect(TokKind::Comma, "between build-step arguments")?;
                            if matches!(self.peek().kind, TokKind::RParen) {
                                break;
                            }
                        }
                    }
                    self.expect(TokKind::RParen, "to close the build step")?;
                    (args, self.toks[self.pos - 1].span.end)
                } else {
                    (Vec::new(), variant_span.end)
                };
                Ok(Expr::EnumLit {
                    type_name: String::new(),
                    variant,
                    variant_span: Some(variant_span),
                    args,
                    leading_dot: true,
                    span: Span::new(dot_start, end),
                })
            }
            _ => self.expr_pow(allow_struct_lit),
        }
    }

    /// D-EXPSEM1=A: infix `^` raises to a power. It binds tighter than
    /// every other binary operator and tighter than unary minus, so
    /// `-3 ^ 2` is `-(3 ^ 2)`. It is right-associative and its exponent
    /// runs back through the unary level, so `2 ^ 3 ^ 2` is `2 ^ (3 ^ 2)`
    /// and `2 ^ -1` reads the negative exponent.
    fn expr_pow(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let base = self.expr_postfix(allow_struct_lit)?;
        if !matches!(self.peek().kind, TokKind::Caret) {
            return Ok(base);
        }
        let op_span = self.bump().span;
        let exponent = self.expr_unary(allow_struct_lit)?;
        let span = Span::new(base.span().start, exponent.span().end.max(op_span.end));
        Ok(Expr::Binary(
            BinOp::Pow,
            Box::new(base),
            Box::new(exponent),
            span,
        ))
    }

    /// D-JPK-BUILDRECIPE1: build-step fields are deliberately finite
    /// strings or lists of strings. Parse these directly instead of
    /// recursing through the full expression grammar at an already deep
    /// nested call site.
    fn build_step_string(&mut self) -> Result<Expr, Diagnostic> {
        let token = self.peek().clone();
        let parts = match token.kind {
            TokKind::Str(parts) => parts,
            TokKind::RawStr(text) => vec![StrTokPart::Lit(text)],
            _ => {
                return Err(Diagnostic::error(
                    "E0003",
                    "a build-step field must be quoted text".to_string(),
                    "build actions use fixed strings for tools, paths, URLs, and hashes".to_string(),
                    "write a quoted string such as `\"src/file\"`".to_string(),
                    Some(token.span),
                ));
            }
        };
        if parts
            .iter()
            .any(|part| matches!(part, StrTokPart::Interp(_)))
        {
            return Err(Diagnostic::error(
                "E0003",
                "build-step fields must be literal quoted text".to_string(),
                "build action identity includes fixed tool, path, URL, and hash facts".to_string(),
                "replace interpolation with a quoted literal".to_string(),
                Some(token.span),
            ));
        }
        self.bump();
        self.str_expr_from_parts(parts, token.span)
    }

    fn build_step_string_list(&mut self) -> Result<Expr, Diagnostic> {
        let open = self.peek().span;
        self.expect(TokKind::LBracket, "before build-step string arguments")?;
        let mut values = Vec::new();
        if !matches!(self.peek().kind, TokKind::RBracket) {
            loop {
                values.push(self.build_step_string()?);
                if matches!(self.peek().kind, TokKind::RBracket) {
                    break;
                }
                self.expect(TokKind::Comma, "between build-step string arguments")?;
                if matches!(self.peek().kind, TokKind::RBracket) {
                    break;
                }
            }
        }
        let close = self.peek().span;
        self.expect(TokKind::RBracket, "after build-step string arguments")?;
        Ok(Expr::ListLit(values, Span::new(open.start, close.end)))
    }
}
