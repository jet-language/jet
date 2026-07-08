impl<'a> Parser<'a> {
        /// S46: `(` … `) =>` without scanning nested `(` for the `=>` probe.
        fn after_lparen_is_lambda(&self) -> bool {
            let mut i = self.pos + 1;
            let mut depth = 1usize;
            while i < self.toks.len() {
                match &self.toks[i].kind {
                    TokKind::LParen => depth += 1,
                    TokKind::RParen => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return matches!(
                                self.toks.get(i + 1).map(|t| &t.kind),
                                Some(TokKind::LambdaArrow)
                            );
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            false
        }
    
        /// S47: `take(a, b)` prefix on a lambda.
        fn parse_lambda_takes(&mut self) -> Result<Vec<(String, Span)>, Diagnostic> {
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
            Ok(names)
        }
    
        fn parse_lambda(&mut self, take_names: Vec<(String, Span)>) -> Result<Lambda, Diagnostic> {
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
            self.expect(TokKind::LambdaArrow, "after `)` in a lambda")?;
            let (body, end) = self.lambda_arrow_body(close_paren.end)?;
            Ok(Lambda {
                take_names,
                params,
                body,
                span: Span::new(open.start, end),
                meta: LambdaMeta::default(),
            })
        }
    
        /// D-LAMBDA-INFER1 (ratified 2026-07-04): a bare single-param lambda with
        /// no parens and no type — `m => m.hp > 0`. Legal wherever the expected
        /// closure/fn type fixes the param type (sema rejects it elsewhere, same
        /// as the existing omitted-type `(m) => …` form under S46/D-LAMBDAINFER1).
        /// No `take` prefix on the bare form — write `(take x) (x) => …` when a
        /// capture list is needed.
        fn parse_bare_lambda(&mut self) -> Result<Lambda, Diagnostic> {
            let (name, name_span) = self.expect_ident("as a lambda parameter")?;
            self.expect(TokKind::LambdaArrow, "after a bare lambda parameter")?;
            let (body, end) = self.lambda_arrow_body(name_span.end)?;
            Ok(Lambda {
                take_names: vec![],
                params: vec![LambdaParam {
                    name,
                    name_span,
                    ty: None,
                    ty_span: None,
                }],
                body,
                span: Span::new(name_span.start, end),
                meta: LambdaMeta::default(),
            })
        }
    
        /// Shared by `parse_lambda`/`parse_bare_lambda`: the body after `=>` and
        /// the lambda's overall end offset. `fallback_end` is used only for an
        /// empty block body (no statements to read an end span from).
        fn lambda_arrow_body(
            &mut self,
            fallback_end: usize,
        ) -> Result<(LambdaBody, usize), Diagnostic> {
            let body = if matches!(self.peek().kind, TokKind::LBrace) {
                self.expect(TokKind::LBrace, "to open the lambda body")?;
                LambdaBody::Block(self.block_stmts())
            } else {
                LambdaBody::Expr(Box::new(self.expr()?))
            };
            let end = match &body {
                LambdaBody::Expr(e) => e.span().end,
                LambdaBody::Block(stmts) => {
                    if let Some(last) = stmts.last() {
                        match last {
                            Stmt::Expr(e) => e.span().end,
                            Stmt::Return(_, s) => s.end,
                            Stmt::Break(s)
                            | Stmt::Continue(s)
                            | Stmt::BreakLabel(_, s)
                            | Stmt::ContinueLabel(_, s) => s.end,
                            Stmt::If(i) => i.span.end,
                            Stmt::While { span, .. }
                            | Stmt::For { span, .. }
                            | Stmt::Switch { span, .. } => span.end,
                            Stmt::Val(b) => b.init.span().end,
                            Stmt::Assign { value, .. } => value.span().end,
                            Stmt::Loop { span: s, .. } | Stmt::CountedLoop { span: s, .. } => s.end,
                            Stmt::Unsafe { span, .. } => span.end,
                            Stmt::Impure { span, .. } => span.end,
                            Stmt::Reactive { span, .. } => span.end,
                            Stmt::SuppressMustUse { span, .. } => span.end,
                            Stmt::Region { span, .. } => span.end,
                            Stmt::TaskGroup { span, .. } => span.end,
                            Stmt::Layout { span, .. } => span.end,
                            Stmt::Caps { span, .. } => span.end,
                            Stmt::Grant { span, .. } => span.end,
                            Stmt::ComptimeBlock { span, .. } => span.end,
                            Stmt::ComptimeIf { span, .. } => span.end,
                            Stmt::ComptimeSwitch { span, .. } => span.end,
                            Stmt::ContextBlock { span, .. } => span.end,
                            // D-TERM1 (ratified 2026-06-22): live block span end.
                            Stmt::Live { span, .. } => span.end,
                            // D-DOTSCOPE1: `.name { … }` scope member span end.
                            Stmt::ScopeMember { span, .. } => span.end,
                            // D-DET1: assume_deterministic block span end.
                            Stmt::AssumeDet { span, .. } => span.end,
                            // D-TXN1–D-TXN4: transaction block span end.
                            Stmt::Transact { span, .. } => span.end,
                            Stmt::Yield(e, _) => e.span().end,
                        }
                    } else {
                        fallback_end
                    }
                }
            };
            Ok((body, end))
        }
    
        /// D-TASKSCOPE1=A: `{ stmts }` after `.task` → `() => { stmts }`.
        fn parse_task_body_lambda(&mut self) -> Result<Lambda, Diagnostic> {
            let open = self.peek().span;
            self.expect(TokKind::LBrace, "to open the task body")?;
            let stmts = self.block_stmts();
            let end = self.toks[self.pos - 1].span.end;
            Ok(Lambda {
                take_names: Vec::new(),
                params: Vec::new(),
                body: LambdaBody::Block(stmts),
                span: Span::new(open.start, end),
                meta: LambdaMeta::default(),
            })
        }
    
}
