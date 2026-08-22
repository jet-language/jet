use super::super::{
    Diagnostic, Lambda, LambdaBody, LambdaMeta, LambdaParam, Parser, Span, Stmt, TokKind,
};

impl<'a> Parser<'a> {
        /// D-LAMBDA-IFACE1=A: a lambda may carry the same result, error, and
        /// effect suffixes as a named callable. Look past `)` without parsing
        /// the type; the real parser below remains the single source of truth.
        fn lambda_tail_starts_at(&self, index: usize) -> bool {
            let Some(kind) = self.toks.get(index).map(|token| &token.kind) else {
                return false;
            };
            if Self::at_unified_arrow_token(kind)
                || matches!(kind, TokKind::Bang | TokKind::Question)
            {
                return true;
            }
            if matches!(kind, TokKind::Minus | TokKind::Colon | TokKind::Eq)
                && matches!(self.toks.get(index + 1).map(|token| &token.kind), Some(TokKind::LBracket))
            {
                return true;
            }
            if matches!(kind, TokKind::MinusMinus) {
                return true;
            }
            if matches!(kind, TokKind::Hash) {
                let next = self.toks.get(index + 1).map(|token| &token.kind);
                if matches!(next, Some(TokKind::LParen)) {
                    return true;
                }
            }
            matches!(
                kind,
                TokKind::KwFn
                    | TokKind::Ident(_)
                    | TokKind::LParen
                    | TokKind::LBracket
                    | TokKind::Star
            )
        }

        /// S46: recognize `(` … `) ->` only when the contents have lambda
        /// parameter shape. A condition such as `(a > b) ->` is an
        /// if-expression condition, not a lambda parameter list.
        pub(super) fn after_lparen_is_lambda(&self) -> bool {
            let mut i = self.pos + 1;
            if matches!(self.toks.get(i).map(|token| &token.kind), Some(TokKind::RParen)) {
                return self.lambda_tail_starts_at(i + 1);
            }
            loop {
                if !matches!(self.toks.get(i).map(|token| &token.kind), Some(TokKind::Ident(_))) {
                    return false;
                }
                i += 1;
                if matches!(self.toks.get(i).map(|token| &token.kind), Some(TokKind::Colon)) {
                    i += 1;
                    let type_start = i;
                    let mut paren_depth = 0usize;
                    let mut bracket_depth = 0usize;
                    let mut brace_depth = 0usize;
                    let mut angle_depth = 0usize;
                    while let Some(token) = self.toks.get(i) {
                        match &token.kind {
                            TokKind::LParen => paren_depth += 1,
                            TokKind::RParen if paren_depth > 0 => paren_depth -= 1,
                            TokKind::LBracket => bracket_depth += 1,
                            TokKind::RBracket if bracket_depth > 0 => bracket_depth -= 1,
                            TokKind::LBrace => brace_depth += 1,
                            TokKind::RBrace if brace_depth > 0 => brace_depth -= 1,
                            TokKind::Lt => angle_depth += 1,
                            TokKind::Gt if angle_depth > 0 => angle_depth -= 1,
                            TokKind::Shr if angle_depth > 0 => angle_depth = angle_depth.saturating_sub(2),
                            TokKind::Comma
                                if paren_depth == 0
                                    && bracket_depth == 0
                                    && brace_depth == 0
                                    && angle_depth == 0 =>
                            {
                                break;
                            }
                            TokKind::RParen
                                if paren_depth == 0
                                    && bracket_depth == 0
                                    && brace_depth == 0
                                    && angle_depth == 0 =>
                            {
                                break;
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    if i == type_start {
                        return false;
                    }
                }
                match self.toks.get(i).map(|token| &token.kind) {
                    Some(TokKind::RParen) => {
                        return self.lambda_tail_starts_at(i + 1);
                    }
                    Some(TokKind::Comma) => {
                        i += 1;
                    }
                    _ => return false,
                }
            }
        }
    
        /// Retired S47 spelling. Consume `take(a, b)` so the parser can teach
        /// the implicit-capture law without cascading errors.
        pub(super) fn parse_lambda_takes(&mut self) -> Result<Vec<(String, Span)>, Diagnostic> {
            let start = self.peek().span;
            self.expect(TokKind::KwMove, "before the capture list")?;
            self.expect(TokKind::LParen, "after `take` in a capture list")?;
            let mut names = Vec::new();
            if !matches!(self.peek().kind, TokKind::RParen) {
                loop {
                    let (name, span) = self.expect_ident("in the capture list")?;
                    names.push((name, span));
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    self.expect(TokKind::Comma, "between captured names")?;
                }
            }
            self.expect(TokKind::RParen, "after the capture list")?;
            self.diags.push(Diagnostic::error(
                "E0057",
                "this closure uses the retired `take(...)` capture prefix".to_string(),
                "closures infer captures: Copy values copy at creation and owned non-Copy values move"
                    .to_string(),
                "remove `take(...)` and use the captured names directly".to_string(),
                Some(start),
            ));
            Ok(names)
        }
    
        pub(super) fn parse_lambda(&mut self, take_names: Vec<(String, Span)>) -> Result<Lambda, Diagnostic> {
            let open = self.peek().span;
            self.expect(TokKind::LParen, "before lambda parameters")?;
            let mut params = Vec::new();
            if !matches!(self.peek().kind, TokKind::RParen) {
                loop {
                    let (name, name_span) = self.expect_ident("as a lambda parameter")?;
                    let (ty, ty_span) = if matches!(self.peek().kind, TokKind::Colon) {
                        self.bump();
                        let (t, ts) = self.type_()?;
                        (Some(t), Some(ts))
                    } else {
                        (None, None)
                    };
                    params.push(LambdaParam {
                        name,
                        name_span,
                        ty,
                        ty_span,
                    });
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    self.expect(TokKind::Comma, "between lambda parameters")?;
                }
            }
            let close_paren = self.peek().span;
            self.expect(TokKind::RParen, "after lambda parameters")?;
            let (result_type, error_type) = self.parse_lambda_return_interface()?;
            let (effects, effect_via) = self.parse_opt_func_effects()?;
            if let Some((_, span)) = effect_via {
                return Err(Diagnostic::error(
                    "E0119",
                    "a lambda effect row cannot publish `via`".to_string(),
                    "a lambda stores a concrete effect row as part of its callable interface".to_string(),
                    "write the effects directly, for example `-[Net]>`".to_string(),
                    Some(span),
                ));
            }
            if effects.is_none() {
                self.expect_unified_arrow("after the lambda interface")?;
            }
            let (body, end) = self.lambda_arrow_body(close_paren.end)?;
            Ok(Lambda {
                take_names,
                params,
                result_type,
                error_type,
                effects,
                body,
                span: Span::new(open.start, end),
                meta: LambdaMeta::default(),
            })
        }

        fn parse_lambda_return_interface(
            &mut self,
        ) -> Result<(Option<super::super::Type>, Option<super::super::Type>), Diagnostic> {
            let return_type = if !self.func_effect_starts_here()
                && (self.type_starts_here() || matches!(self.peek().kind, TokKind::Star))
            {
                Some(self.type_()?.0)
            } else {
                self.parse_unit_fallible_return()?.map(|(ty, _)| ty)
            };
            Ok(match return_type {
                Some(super::super::Type::Result { ok, err }) => (Some(*ok), Some(*err)),
                Some(ty) => (Some(ty), None),
                None => (None, None),
            })
        }
    
        /// D-LAMBDA-INFER1 (ratified 2026-07-04): a bare single-param lambda with
        /// no parens and no type — `m -> m.hp > 0`. Legal wherever the expected
        /// closure/fn type fixes the param type (sema rejects it elsewhere, same
        /// as the existing omitted-type `(m) -> …` form under S46/D-LAMBDAINFER1).
        /// D-ARROW-CONTROL1: captures are always inferred.
        pub(super) fn parse_bare_lambda(&mut self) -> Result<Lambda, Diagnostic> {
            let (name, name_span) = self.expect_ident("as a lambda parameter")?;
            self.expect_unified_arrow("after a bare lambda parameter")?;
            let (body, end) = self.lambda_arrow_body(name_span.end)?;
            Ok(Lambda {
                take_names: vec![],
                params: vec![LambdaParam {
                    name,
                    name_span,
                    ty: None,
                    ty_span: None,
                }],
                result_type: None,
                error_type: None,
                effects: None,
                body,
                span: Span::new(name_span.start, end),
                meta: LambdaMeta::default(),
            })
        }
    
        /// Shared by `parse_lambda`/`parse_bare_lambda`: the body after `->` and
        /// the lambda's overall end offset. `fallback_end` is used only for an
        /// empty block body (no statements to read an end span from).
        ///
        /// S46: `-> expr` or `-> { … }`. A single assignment after `->` needs no
        /// braces — `a -> a.balance -= n` is the one-statement form of
        /// `a -> { a.balance -= n }` (braces stay for multi-statement bodies).
        fn lambda_arrow_body(
            &mut self,
            fallback_end: usize,
        ) -> Result<(LambdaBody, usize), Diagnostic> {
            if matches!(self.peek().kind, TokKind::LBrace) {
                self.expect(TokKind::LBrace, "to open the lambda body")?;
                let statements = self.block_stmts();
                // `block_stmts` consumes the closing brace. Keep it in the
                // lambda span: formatter comment ownership must distinguish a
                // comment inside the lambda from one trailing the enclosing
                // call on the same source line.
                let end = self.toks[self.pos - 1].span.end;
                Ok((LambdaBody::Block(statements), end))
            } else {
                let expression = self.expr()?;
                if matches!(self.peek().kind, TokKind::Eq) || self.peek().kind.compound_op().is_some()
                {
                    let op_tok = self.bump();
                    let op = op_tok.kind.compound_op();
                    let value = self.expr()?;
                    let end = value.span().end.max(fallback_end);
                    let target = self.expr_to_lvalue(expression, op)?;
                    Ok((
                        LambdaBody::Block(vec![Stmt::Assign {
                            target,
                            op,
                            op_span: op_tok.span,
                            value,
                        }]),
                        end,
                    ))
                } else {
                    let end = expression.span().end.max(fallback_end);
                    Ok((LambdaBody::Expr(Box::new(expression)), end))
                }
            }
        }
    
}
