use super::super::{Diagnostic, Expr, Parser, Span, Syntax, TokKind};

impl<'a> Parser<'a> {
        /// S58 (E2-M13): parse the tail of `alias.Ptr<T>.from_addr(addr)`, with the
        /// cursor at the `<`. The `alias`/`alias_span` are the already-parsed
        /// module alias and `.Ptr` member.
        pub(super) fn ptr_from_addr(&mut self, alias: String, alias_span: Span) -> Result<Expr, Diagnostic> {
            self.expect_type_args_open(Syntax::TYPE_PTR)?;
            let (elem, _) = self.type_()?;
            if matches!(self.peek().kind, TokKind::Comma) {
                return Err(Diagnostic::error(
                    "E0003",
                    format!("`{}<…>` takes exactly one element type", Syntax::TYPE_PTR),
                    "a pointer points at a single element type".to_string(),
                    format!(
                        "write `{}.{}<Int>.{}(addr)`",
                        alias,
                        Syntax::TYPE_PTR,
                        Syntax::MEM_FROM_ADDR
                    ),
                    Some(self.peek().span),
                ));
            }
            self.expect_type_args_close(&format!("after `{}<…>`", Syntax::TYPE_PTR))?;
            self.expect(TokKind::Dot, &format!("after `{}<…>`", Syntax::TYPE_PTR))?;
            let (method, method_span) = self.expect_field_name()?;
            if method != Syntax::MEM_FROM_ADDR {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "`{}<…>` has no static method `{}`",
                        Syntax::TYPE_PTR,
                        method
                    ),
                    "a typed pointer is built from an address".to_string(),
                    format!(
                        "write `{}.{}<Int>.{}(addr)`",
                        alias,
                        Syntax::TYPE_PTR,
                        Syntax::MEM_FROM_ADDR
                    ),
                    Some(method_span),
                ));
            }
            self.expect(
                TokKind::LParen,
                &format!("after `{}`", Syntax::MEM_FROM_ADDR),
            )?;
            let addr = self.expr()?;
            self.expect(TokKind::RParen, "to finish the call")?;
            let end = self.toks[self.pos - 1].span.end;
            Ok(Expr::PtrFromAddr {
                alias,
                alias_span,
                elem,
                addr: Box::new(addr),
                span: Span::new(alias_span.start, end),
            })
        }
    
        /// S73: `( name :: … )` — named tuple literal or type, not grouping.
        /// When `lparen_consumed` is true, `self.pos` is already on the first member name.
        pub(in crate::Parser) fn looks_like_named_tuple(&self, lparen_consumed: bool) -> bool {
            let i = if lparen_consumed {
                self.pos
            } else {
                self.pos + 1
            };
            matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Ident(_)))
                && matches!(self.toks.get(i + 1).map(|t| &t.kind), Some(TokKind::Colon))
        }
    
        fn emit_positional_tuple_error(&mut self, span: Span) {
            self.diags.push(Diagnostic::error(
                "E0048",
                format!(
                    "{} tuples name every member — positional `(1, 2)` isn't allowed (S73)",
                    Syntax::LANG_NAME
                ),
                "named members make field access obvious and avoid `.0`, which collides with decimal numbers"
                    .to_string(),
                "write named members: `(x: 1, y: 2)` and use `p.x`, not `p.0`".to_string(),
                Some(span),
            ));
        }
    
        fn emit_numeric_field_error(&mut self, span: Span) {
            self.diags.push(Diagnostic::error(
                "E0049",
                format!(
                    "{} doesn't use numeric field access like `.0` (S73)",
                    Syntax::LANG_NAME
                ),
                "`.0` looks like the start of a decimal number, so tuple members must have names"
                    .to_string(),
                "name the members when you build the tuple: `(x: 1, y: 2)`, then read `p.x`"
                    .to_string(),
                Some(span),
            ));
        }
    
        /// S73: reject `.0` / `.1` field access before `expect_ident`.
        pub(super) fn expect_field_name(&mut self) -> Result<(String, Span), Diagnostic> {
            if matches!(self.peek().kind, TokKind::Int(_, _) | TokKind::Float(_)) {
                let span = self.peek().span;
                self.bump();
                self.emit_numeric_field_error(span);
                return Ok(("0".to_string(), span));
            }
            // D-LAYOUT-FACTS1=B / D-META-STAGE1=B: a marked member after `.` is
            // a compiler-owned fact, and the registry is closed. The mark is
            // not a general user-member escape hatch.
            if matches!(&self.peek().kind, TokKind::Ident(n) if Syntax::is_comptime_name(n)) {
                let (member, member_span) = self.expect_ident("in a compiler fact")?;
                if Syntax::compiler_fact_member(&member).is_none() {
                    return Err(Diagnostic::error(
                        "E0302",
                        format!("`{member}` is not a compiler-owned fact"),
                        format!(
                            "the compiler-owned facts are {}",
                            Syntax::COMPILER_FACTS
                                .iter()
                                .map(|(name, _)| format!("`{name}`"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        "write `T.@layout`, `T.@name`, or `T.@fields`".to_string(),
                        Some(member_span),
                    ));
                }
                return Ok((member, member_span));
            }
            // D-ITER1: `take` is `KwMove` in the lexer but is valid as a method name
            // in dot position (`xs.take(n)`). Accept it as an identifier here.
            if matches!(self.peek().kind, TokKind::KwMove) {
                let span = self.peek().span;
                self.bump();
                return Ok((Syntax::KW_MOVE.to_string(), span));
            }
            // U20: `Recipe.copy()` uses the ordinary dot-member form, while `copy`
            // is also the retired copy keyword (D-SHAPE-COPY1, now `~x`) in value
            // position. Keep the keyword reserved everywhere else; permit it only
            // after `.`.
            if matches!(self.peek().kind, TokKind::KwCopy) {
                let span = self.peek().span;
                self.bump();
                return Ok((Syntax::KW_COPY.to_string(), span));
            }
            // D-EFFECT-DECL1=A: `effect` is a declaration keyword in item
            // position, but existing APIs such as `reactive.effect()` keep
            // their ordinary qualified member spelling.
            if matches!(self.peek().kind, TokKind::KwEffect) {
                let span = self.peek().span;
                self.bump();
                return Ok((Syntax::KW_EFFECT_DECL.to_string(), span));
            }
            self.expect_ident("after `.`")
        }
    
        fn sync_to_rparen(&mut self) {
            let mut depth = 0;
            while self.pos < self.toks.len() {
                match self.peek().kind {
                    TokKind::LParen => depth += 1,
                    TokKind::RParen if depth == 0 => {
                        self.bump();
                        return;
                    }
                    TokKind::RParen => depth -= 1,
                    TokKind::Semi | TokKind::RBrace if depth == 0 => return,
                    _ => {}
                }
                self.bump();
            }
        }
    
        /// S73: `(x: expr, y: expr)` after the opening `(`.
        fn parse_tuple_lit(&mut self, open: Span) -> Result<Expr, Diagnostic> {
            let mut fields = Vec::new();
            if !matches!(self.peek().kind, TokKind::RParen) {
                loop {
                    let (name, _) = self.expect_ident("for a tuple member name")?;
                    self.expect(TokKind::Colon, "after each tuple member name")?;
                    let value = self.expr()?;
                    fields.push((name, value));
                    if matches!(self.peek().kind, TokKind::Comma) {
                        self.bump();
                        if matches!(self.peek().kind, TokKind::RParen) {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            let close = self.peek().span;
            self.expect(TokKind::RParen, "to close this tuple")?;
            if fields.len() < 2 {
                return Err(Diagnostic::error(
                    "E0003",
                    "a tuple needs at least two named members".to_string(),
                    "a single `(name: value)` would be ambiguous with grouping — use a one-field `struct` instead"
                        .to_string(),
                    "add another member: `(x: 1, y: 2)`".to_string(),
                    Some(Span::new(open.start, close.end)),
                ));
            }
            Ok(Expr::TupleLit(
                fields,
                Span::new(open.start, close.end),
                None,
            ))
        }
    
        pub(super) fn parse_paren_primary(&mut self, _allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
            let open = self.bump().span;
            if self.looks_like_named_tuple(true) {
                return self.parse_tuple_lit(open);
            }
            if self.after_lparen_is_positional_tuple() {
                self.emit_positional_tuple_error(open);
                self.sync_to_rparen();
                return Ok(Expr::Int(0, open, None, None));
            }
            let pos_after_first_open = self.pos;

            // Collect every immediately-consecutive `(` that opens a bare nested
            // group (not a lambda param list or named tuple) so purely redundant
            // nesting (`((((expr))))`) can fold into one `expr()` call below and
            // cost one nesting level, not N (#1319).
            let mut opens = vec![open];
            while matches!(self.peek().kind, TokKind::LParen)
                && !self.after_lparen_is_lambda()
                && !self.looks_like_named_tuple(false)
            {
                let candidate_pos = self.pos;
                let candidate = self.bump().span;
                if self.after_lparen_is_positional_tuple() {
                    self.pos = candidate_pos;
                    break;
                }
                opens.push(candidate);
            }

            if opens.len() > 1 {
                let diags_before = self.diags.len();
                if let Some(folded) = self.try_fold_paren_run(&opens) {
                    return Ok(folded);
                }
                // Not pure nesting after all — e.g. `((a + b) + c)`, where the
                // inner `(a + b)` closes but the group keeps growing before the
                // outer `)`. Discard whatever the failed attempt parsed/recorded
                // and fall through to ordinary one-level-at-a-time recursion,
                // which handles this correctly (still bounded by
                // MAX_SOURCE_NESTING via `expr()`'s `with_nesting`).
                self.diags.truncate(diags_before);
                self.pos = pos_after_first_open;
            }

            let inner = self.expr()?;
            if matches!(self.peek().kind, TokKind::Comma) {
                self.emit_positional_tuple_error(open);
                self.sync_to_rparen();
                return Ok(Expr::Int(0, open, None, None));
            }
            self.expect(TokKind::RParen, "to close this `(`")?;
            let close_span = self.toks[self.pos - 1].span;
            let span = Span::new(open.start, close_span.end);
            // D-FMTPARENS1=A: preserve author parens as a distinct AST node so
            // the formatter can always re-emit them, even when redundant.
            Ok(Expr::Paren(Box::new(inner), span))
        }

        /// Fast path for `opens.len()` immediately-consecutive `(` tokens: parse
        /// exactly one inner expression, then require that many `)` back to
        /// back. Returns `None` the moment a close doesn't immediately follow —
        /// the caller rewinds `self.pos` and falls back to naive recursion.
        fn try_fold_paren_run(&mut self, opens: &[Span]) -> Option<Expr> {
            let mut inner = self.expr().ok()?;
            if matches!(self.peek().kind, TokKind::Comma) {
                return None;
            }
            for &open in opens.iter().rev() {
                if !matches!(self.peek().kind, TokKind::RParen) {
                    return None;
                }
                self.bump();
                let close_span = self.toks[self.pos - 1].span;
                let span = Span::new(open.start, close_span.end);
                // D-FMTPARENS1=A: preserve author parens as distinct AST nodes so
                // the formatter can always re-emit them, even when redundant.
                inner = Expr::Paren(Box::new(inner), span);
            }
            Some(inner)
        }
    
        /// True when `(` starts `( expr , … )` without member names — rejected (S73).
        /// Call with `self.pos` on the first token inside `(`.
        fn after_lparen_is_positional_tuple(&self) -> bool {
            let mut i = self.pos;
            if i >= self.toks.len() {
                return false;
            }
            if matches!(self.toks[i].kind, TokKind::RParen) {
                return false;
            }
            if matches!(self.toks[i].kind, TokKind::Ident(_))
                && self
                    .toks
                    .get(i + 1)
                    .is_some_and(|t| matches!(t.kind, TokKind::Colon))
            {
                return false;
            }
            loop {
                match &self.toks[i].kind {
                    TokKind::RParen => return false,
                    TokKind::Comma => return true,
                    TokKind::Colon => return false,
                    TokKind::LParen | TokKind::LBrace | TokKind::LBracket => {
                        i += 1;
                        let mut depth = 1;
                        while i < self.toks.len() && depth > 0 {
                            match self.toks[i].kind {
                                TokKind::LParen | TokKind::LBrace | TokKind::LBracket => depth += 1,
                                TokKind::RParen | TokKind::RBrace | TokKind::RBracket => depth -= 1,
                                _ => {}
                            }
                            i += 1;
                        }
                    }
                    _ => i += 1,
                }
                if i >= self.toks.len() {
                    return false;
                }
            }
        }
    
        pub(in crate::Parser) fn expr(&mut self) -> Result<Expr, Diagnostic> {
            let span = self.peek().span;
            self.with_nesting(span, |p| {
                if let Some(result) = p.try_primary_expr() {
                    return result;
                }
                p.expr_or_fallback(true)
            })
        }

        /// Most expression values are one primary plus an optional postfix
        /// chain. Sending those values through every precedence layer creates
        /// a large stack frame at each nesting level, even when no operator is
        /// present. Probe that common shape first; an infix token restores the
        /// parser and uses the canonical precedence path below.
        fn try_primary_expr(&mut self) -> Option<Result<Expr, Diagnostic>> {
            if !self.can_start_primary_expr() {
                return None;
            }

            let state = ExprProbeState::save(self);
            match self.expr_postfix(true) {
                Ok(expr) if !self.has_expr_infix_continuation() => Some(Ok(expr)),
                Ok(_) | Err(_) => {
                    state.restore(self);
                    None
                }
            }
        }

        fn can_start_primary_expr(&self) -> bool {
            matches!(
                self.peek().kind,
                TokKind::KwLoop
                    | TokKind::KwIt
                    | TokKind::KwNull
                    | TokKind::Hash
                    | TokKind::Str(_)
                    | TokKind::Int(_, _)
                    | TokKind::Float(_)
                    | TokKind::UnitNumber { .. }
                    | TokKind::Char(_)
                    | TokKind::LBracket
                    | TokKind::KwTrue
                    | TokKind::KwFalse
                    | TokKind::KwSelf
                    | TokKind::KwIf
                    | TokKind::KwMove
                    | TokKind::LParen
                    | TokKind::Ident(_)
            )
        }

        fn has_expr_infix_continuation(&self) -> bool {
            match self.peek().kind {
                TokKind::QuestionQuestion
                | TokKind::DotDot
                | TokKind::DotDotLt
                | TokKind::OrOr
                | TokKind::AndAnd
                | TokKind::EqEq
                | TokKind::NotEq
                | TokKind::Lt
                | TokKind::Le
                | TokKind::Ge
                | TokKind::TildePipe
                | TokKind::Amp
                | TokKind::Shl
                | TokKind::Shr
                | TokKind::Plus
                | TokKind::Minus
                | TokKind::Star
                | TokKind::Slash
                | TokKind::SlashPercent
                | TokKind::Percent
                | TokKind::PercentPercent
                | TokKind::Caret => true,
                TokKind::Gt => self.module_arg_expr_depth != Some(self.depth),
                _ => false,
            }
        }

        pub(in crate::Parser) fn module_arg_expr(&mut self) -> Result<Expr, Diagnostic> {
            let previous = self.module_arg_expr_depth;
            self.module_arg_expr_depth = Some(self.depth + 1);
            let result = self.expr();
            self.module_arg_expr_depth = previous;
            result
        }
    
        pub(in crate::Parser) fn expr_no_struct_lit(&mut self) -> Result<Expr, Diagnostic> {
            let span = self.peek().span;
            self.with_nesting(span, |p| p.expr_or_fallback(false))
        }
    
        /// D-IF3: parse a subject expression that stops *below* comparison
        /// precedence, so a trailing `==` is left for `if_or_dispatch` to detect as
        /// the dispatch marker (`if subject == { … }`). `expr_cmp` would otherwise
        /// consume the `==` (as a comparison or a `PatternTest`) and then choke on
        /// the `{`. Struct literals are disallowed, like `expr_no_struct_lit`, so
        /// `if subject {` never reads `subject { … }` as a struct value.
        pub(in crate::Parser) fn expr_no_struct_lit_no_cmp(&mut self) -> Result<Expr, Diagnostic> {
            let span = self.peek().span;
            self.with_nesting(span, |p| p.expr_bitxor(false))
        }
    
}

#[derive(Clone, Copy)]
struct ExprProbeState {
    pos: usize,
    diags_len: usize,
    pending_type_gt: bool,
    depth: usize,
    type_generic_depth: usize,
    type_generic_chain_len: usize,
    type_generic_truncated: bool,
    in_layout_body: usize,
    adjacent_if_body_depth: usize,
    block_depth: usize,
    callable_tail_block_depth: Option<usize>,
    module_arg_expr_depth: Option<usize>,
    allow_lowercase_leading_dot: bool,
    policy_declarations_len: usize,
    applied_rules_len: usize,
    rule_facts_len: usize,
    block_spans_len: usize,
}

impl ExprProbeState {
    fn save(parser: &Parser<'_>) -> Self {
        Self {
            pos: parser.pos,
            diags_len: parser.diags.len(),
            pending_type_gt: parser.pending_type_gt,
            depth: parser.depth,
            type_generic_depth: parser.type_generic_depth,
            type_generic_chain_len: parser.type_generic_chain.len(),
            type_generic_truncated: parser.type_generic_truncated,
            in_layout_body: parser.in_layout_body,
            adjacent_if_body_depth: parser.adjacent_if_body_depth,
            block_depth: parser.block_depth,
            callable_tail_block_depth: parser.callable_tail_block_depth,
            module_arg_expr_depth: parser.module_arg_expr_depth,
            allow_lowercase_leading_dot: parser.allow_lowercase_leading_dot,
            policy_declarations_len: parser.policy_declarations.len(),
            applied_rules_len: parser.applied_rules.len(),
            rule_facts_len: parser.rule_facts.len(),
            block_spans_len: parser.block_spans.len(),
        }
    }

    fn restore(self, parser: &mut Parser<'_>) {
        parser.pos = self.pos;
        parser.diags.truncate(self.diags_len);
        parser.pending_type_gt = self.pending_type_gt;
        parser.depth = self.depth;
        parser.type_generic_depth = self.type_generic_depth;
        parser.type_generic_chain.truncate(self.type_generic_chain_len);
        parser.type_generic_truncated = self.type_generic_truncated;
        parser.in_layout_body = self.in_layout_body;
        parser.adjacent_if_body_depth = self.adjacent_if_body_depth;
        parser.block_depth = self.block_depth;
        parser.callable_tail_block_depth = self.callable_tail_block_depth;
        parser.module_arg_expr_depth = self.module_arg_expr_depth;
        parser.allow_lowercase_leading_dot = self.allow_lowercase_leading_dot;
        parser
            .policy_declarations
            .truncate(self.policy_declarations_len);
        parser.applied_rules.truncate(self.applied_rules_len);
        parser.rule_facts.truncate(self.rule_facts_len);
        parser.block_spans.truncate(self.block_spans_len);
    }
}
