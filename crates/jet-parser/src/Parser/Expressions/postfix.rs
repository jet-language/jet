use super::super::{
    AccessConvention, CallArg, Diagnostic, Expr, LValue, Parser, Span, Syntax, TokKind, TryConvert,
};

impl<'a> Parser<'a> {
        pub(super) fn expr_postfix(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
            let mut expr = self.expr_primary(allow_struct_lit)?;
            loop {
                match &self.peek().kind {
                    TokKind::Dot => {
                        self.bump();
                        // D-CAP9: postfix `p.*` — dereference a raw pointer. The `.`
                        // followed by `*` reads as deref (it composes with a further
                        // `.field`, giving `p.*.field`). Gated to `#Unsafe` in sema.
                        if matches!(self.peek().kind, TokKind::Star) {
                            let star = self.bump().span;
                            let full = Span::new(expr.span().start, star.end);
                            expr = Expr::Deref(Box::new(expr), full);
                            continue;
                        }
                        // D-SPREAD1=A: `prefix.[a, b, c]` member spread.
                        if matches!(self.peek().kind, TokKind::LBracket) {
                            let start = expr.span().start;
                            expr = self.parse_member_spread(expr, start)?;
                            continue;
                        }
                        // D-DOTCTOR1: `alias.Type.{ … }` — named construction through
                        // an import namespace, or `Protocol.Client.{ … }` for a dotted
                        // local type when the base name is PascalCase (D-PROTO1/D-PROTO2).
                        if allow_struct_lit && matches!(self.peek().kind, TokKind::LBrace) {
                            let start = expr.span().start;
                            if let Expr::Field(inner, type_name, _) = &expr {
                                if let Expr::Ident(alias, _) = inner.as_ref() {
                                    let alias = alias.clone();
                                    let type_name = type_name.clone();
                                    if alias.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                                        let full = format!("{alias}.{type_name}");
                                        let span = expr.span();
                                        expr = self.struct_lit_after_name(full, Vec::new(), span)?;
                                    } else {
                                        expr = self.struct_lit_after_import(alias, type_name, start)?;
                                    }
                                    continue;
                                }
                            }
                            // Non-import chain (e.g. `x.Foo.{`): not valid — fall
                            // through to break so the `{` opens a block.
                            break;
                        }
                        let (member, member_span) = self.expect_field_name()?;
                        // S58 (E2-M13): `alias.Ptr<T>.from_addr(addr)` — a typed
                        // pointer constructor through a `core.mem` alias. Recognise
                        // the `<…>` here (postfix position) so `<` is read as a
                        // type-arg list, not a comparison.
                        if member == Syntax::TYPE_PTR && matches!(self.peek().kind, TokKind::Lt) {
                            if let Expr::Ident(alias, alias_span) = &expr {
                                let alias = alias.clone();
                                let alias_span = *alias_span;
                                expr = self.ptr_from_addr(alias, alias_span)?;
                                continue;
                            }
                        }
                        // D-GENERIC-CALL1=A: optional call-site type arguments on
                        // every method call, such as `decode<Order>(…)`.
                        let type_args = if self.at_turbofish() {
                            self.parse_turbofish()?
                        } else {
                            Vec::new()
                        };
                        // D-TASKSCOPE1=A / D-ARROW-CONTROL1: `g.task => body`
                        // desugars to a zero-parameter lambda.
                        if member == Syntax::TASKGROUP_SPAWN_METHOD
                            && matches!(self.peek().kind, TokKind::LambdaArrow)
                        {
                            let lam = self.parse_task_body_lambda()?;
                            let lam_span = lam.span;
                            expr = Expr::MethodCall {
                                receiver: Box::new(expr),
                                method: member.clone(),
                                method_span: member_span,
                                owner_type_args: Vec::new(),
                                type_args,
                                args: vec![CallArg {
                                    convention: AccessConvention::Read,
                                    expr: Expr::Lambda(lam),
                                    span: lam_span,
                                    flags: crate::AST::CallArgFlags::default(),
                                    label: None,
                                    spread: false,
                                }],
                                recv_type: None,
                                resolved_ret: None,
                            };
                            continue;
                        } else if matches!(self.peek().kind, TokKind::LParen) {
                            self.bump();
                            let mut args = Vec::new();
                            if member == Syntax::METHOD_TAKE_PATTERN {
                                // D-SHIFT1: `cursor.take_pattern("…")` — the sole
                                // legal shape is one pattern-literal argument
                                // (I8: not a second call-argument grammar, just
                                // like `.view(a..b)` above).
                                let pat = self.parse_take_pattern_literal()?;
                                let pat_span = pat.span();
                                args.push(CallArg {
                                    convention: AccessConvention::Read,
                                    expr: pat,
                                    span: pat_span,
                                    flags: crate::AST::CallArgFlags::default(),
                                    label: None,
                                    spread: false,
                                });
                            } else if member == Syntax::METHOD_VIEW {
                                // D-DYNARRAY1: `.view(a..b)` is the ONLY legal shape —
                                // a comma arg list is not a second spelling (I8). Parse
                                // `expr .. expr` directly; the two ends become the
                                // constructor's two Int arguments, exactly like a
                                // bracket slice's `start`/`end`.
                                let range = self.expr()?;
                                let Expr::Range { start, end, .. } = range else {
                                    return Err(Diagnostic::error(
                                        "E0003",
                                        "expected a `..` between the view's start and end"
                                            .to_string(),
                                        "the structure here isn't what the compiler expected"
                                            .to_string(),
                                        "use `..` between the view's start and end".to_string(),
                                        Some(self.peek().span),
                                    ));
                                };
                                let start = *start;
                                let end = *end;
                                let start_span = start.span();
                                let end_span = end.span();
                                args.push(CallArg {
                                    convention: AccessConvention::Read,
                                    expr: start,
                                    span: start_span,
                                    flags: crate::AST::CallArgFlags::default(),
                                    label: None,
                                    spread: false,
                                });
                                args.push(CallArg {
                                    convention: AccessConvention::Read,
                                    expr: end,
                                    span: end_span,
                                    flags: crate::AST::CallArgFlags::default(),
                                    label: None,
                                    spread: false,
                                });
                            } else if !matches!(self.peek().kind, TokKind::RParen) {
                                loop {
                                    args.push(self.call_arg()?);
                                    if matches!(self.peek().kind, TokKind::RParen) {
                                        break;
                                    }
                                    self.expect(TokKind::Comma, "between arguments")?;
                                }
                            }
                            self.expect(TokKind::RParen, "to finish the call")?;
                            expr = Expr::MethodCall {
                                receiver: Box::new(expr),
                                method: member,
                                method_span: member_span,
                                owner_type_args: Vec::new(),
                                type_args,
                                args,
                                recv_type: None,
                                resolved_ret: None,
                            };
                        } else {
                            expr = Expr::Field(Box::new(expr), member, member_span);
                        }
                    }
                    TokKind::Question => {
                        let qspan = self.bump().span;
                        let full = Span::new(expr.span().start, qspan.end);
                        expr = Expr::Try(Box::new(expr), full, TryConvert::None);
                    }
                    // S71 (D-SG6): `base?.field` optional chaining.
                    TokKind::QuestionDot => {
                        self.bump();
                        let (member, member_span) = self.expect_field_name()?;
                        if matches!(self.peek().kind, TokKind::LParen) {
                            return Err(Diagnostic::error(
                                "E0046",
                                "optional chaining `?.` only reaches fields, not methods".to_string(),
                                "`a?.b` short-circuits a `T?` to absent; calling through `?.` isn't in yet"
                                    .to_string(),
                                "unwrap first, e.g. `(a ?? return).method()`, or test with `== present`"
                                    .to_string(),
                                Some(member_span),
                            ));
                        }
                        let span = Span::new(expr.span().start, member_span.end);
                        expr = Expr::OptField {
                            base: Box::new(expr),
                            member,
                            member_span,
                            flatten: false,
                            span,
                        };
                    }
                    TokKind::LParen => {
                        let open = self.bump().span;
                        let mut args = Vec::new();
                        if !matches!(self.peek().kind, TokKind::RParen) {
                            loop {
                                args.push(self.call_arg()?);
                                if matches!(self.peek().kind, TokKind::RParen) {
                                    break;
                                }
                                self.expect(TokKind::Comma, "between arguments")?;
                            }
                        }
                        self.expect(TokKind::RParen, "to finish the call")?;
                        let close = self.toks[self.pos - 1].span;
                        let span = Span::new(open.start, close.end);
                        expr = Expr::CallValue {
                            callee: Box::new(expr),
                            args,
                            span,
                        };
                    }
                    TokKind::LambdaArrow => {
                        // D-TASKSCOPE1=A / D-ARROW-CONTROL1: `g.task => body`
                        // after `.task` was parsed as a field.
                        if let Expr::Field(base, member, member_span) = &expr {
                            if member == Syntax::TASKGROUP_SPAWN_METHOD {
                                let lam = self.parse_task_body_lambda()?;
                                let lam_span = lam.span;
                                expr = Expr::MethodCall {
                                    receiver: base.clone(),
                                    method: member.clone(),
                                    method_span: *member_span,
                                    owner_type_args: Vec::new(),
                                    type_args: Vec::new(),
                                    args: vec![CallArg {
                                        convention: AccessConvention::Read,
                                        expr: Expr::Lambda(lam),
                                        span: lam_span,
                                        flags: crate::AST::CallArgFlags::default(),
                                        label: None,
                                        spread: false,
                                    }],
                                    recv_type: None,
                                    resolved_ret: None,
                                };
                                continue;
                            }
                        }
                        break;
                    }
                    TokKind::LBrace => {
                        // In a control-flow header (`for … in expr {`, `if cond {`, …)
                        // the `{` opens the body, never a struct literal — even after a
                        // field chain like `recv.field`. Only treat `expr.Type { … }` as
                        // an old-form import-namespace struct literal (E0320 recovery)
                        // when struct literals are allowed in this position.
                        let import_lit = if !allow_struct_lit {
                            None
                        } else if let Expr::Field(inner, type_name, _) = &expr {
                            if let Expr::Ident(alias, _) = inner.as_ref() {
                                Some((alias.clone(), type_name.clone()))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if let Some((alias, type_name)) = import_lit {
                            // D-DOTCTOR2: old dotless `alias.Type { … }` — E0320 recovery.
                            let brace_span = self.peek().span;
                            self.diags.push(Diagnostic::error(
                                "E0320",
                                format!(
                                    "struct construction uses `{}.{}.{{…}}`, not `{}.{} {{…}}`",
                                    alias, type_name, alias, type_name
                                ),
                                "named construction has a dot before the brace (D-DOTCTOR1)"
                                    .to_string(),
                                format!("write `{}.{}.{{…}}` instead", alias, type_name),
                                Some(brace_span),
                            ));
                            let start = expr.span().start;
                            expr = self.struct_lit_after_import(alias, type_name, start)?;
                        } else if allow_struct_lit
                            && matches!(
                                expr,
                                Expr::Ident(..)
                                    | Expr::Call(_)
                                    | Expr::MethodCall { .. }
                                    | Expr::CallValue { .. }
                            )
                        {
                            // D-TRAILBLOCK2=A: trailing `{ }` after a call is retired.
                            // Pass code as an ordinary `() => { … }` argument inside the
                            // parentheses (multiline bodies and multiple code args allowed).
                            let bad_span = self.peek().span;
                            let fix = match &expr {
                                Expr::Ident(name, _) => format!(
                                    "write `{name}(() => {{ … }})` — a multiline code argument uses `() => {{ … }}` inside the call"
                                ),
                                Expr::Call(c) => format!(
                                    "write `{}(…, () => {{ … }})` — put the block inside the parentheses as `() => {{ … }}`",
                                    c.name
                                ),
                                Expr::MethodCall { method, .. } => format!(
                                    "write `….{method}(…, () => {{ … }})` — put the block inside the parentheses as `() => {{ … }}`"
                                ),
                                _ => "write `callee(…, () => { … })` — put the block inside the parentheses as `() => { … }`".to_string(),
                            };
                            return Err(Diagnostic::error(
                                "E0335",
                                "trailing blocks are gone — pass code with `() =>`".to_string(),
                                "a bare `{ }` after a call used to fill one last zero-parameter function argument; that sugar is retired (D-TRAILBLOCK2)"
                                    .to_string(),
                                fix,
                                Some(bad_span),
                            ));
                        } else if allow_struct_lit
                            && matches!(
                                expr,
                                Expr::Field(..) | Expr::Index { .. } | Expr::Slice { .. }
                            )
                        {
                            let bad_span = self.peek().span;
                            return Err(Diagnostic::error(
                                "E0335",
                                "trailing blocks are gone — pass code with `() =>`".to_string(),
                                "a bare `{ }` here is not a call argument; code arguments use `() => { … }` inside a call's parentheses (D-TRAILBLOCK2)"
                                    .to_string(),
                                "write `callee(() => { … })` on a call — this expression is not a call"
                                    .to_string(),
                                Some(bad_span),
                            ));
                        } else {
                            break;
                        }
                    }
                    TokKind::LBracket => {
                        let open = self.bump().span;
                        // D-LAYOUT-FACTS1=B: `[.field]` is a typed selector,
                        // not a leading-dot enum literal. Store it in the
                        // existing identifier node with an internal sentinel;
                        // formatter and TIR unwrap it at their boundaries.
                        let start = if matches!(self.peek().kind, TokKind::Dot)
                            && matches!(&self.peek2().kind, TokKind::Ident(_))
                            && matches!(
                                self.peek3().kind,
                                TokKind::RBracket | TokKind::DotDot
                            )
                        {
                            let dot = self.bump().span;
                            let (name, name_span) = self.expect_ident(
                                "after `.` in a layout field selector",
                            )?;
                            Expr::Ident(
                                Syntax::layout_selector(&name),
                                Span::new(dot.start, name_span.end),
                            )
                        } else {
                            self.expr()?
                        };
                        if matches!(self.peek().kind, TokKind::DotDot) {
                            self.bump();
                            let end = self.expr()?;
                            self.expect(TokKind::RBracket, "after a slice range")?;
                            let close = self.toks[self.pos - 1].span;
                            let span = Span::new(open.start, close.end);
                            expr = Expr::Slice {
                                base: Box::new(expr),
                                start: Box::new(start),
                                end: Box::new(end),
                                range: None,
                                span,
                            };
                        } else {
                            self.expect(TokKind::RBracket, "after an index")?;
                            let close = self.toks[self.pos - 1].span;
                            let span = Span::new(open.start, close.end);
                            expr = Expr::Index {
                                base: Box::new(expr),
                                index: Box::new(start),
                                span,
                                kind: crate::AST::IndexKind::Unknown,
                            };
                        }
                    }
                    // D-INCR1: postfix `++` / `--` on the postfix chain.
                    TokKind::PlusPlus | TokKind::MinusMinus => {
                        let op_tok = self.bump();
                        let op = match op_tok.kind {
                            TokKind::PlusPlus => crate::AST::IncDecOp::Inc,
                            TokKind::MinusMinus => crate::AST::IncDecOp::Dec,
                            _ => unreachable!(),
                        };
                        let full = Span::new(expr.span().start, op_tok.span.end);
                        expr = Expr::IncDec {
                            op,
                            operand: Box::new(expr),
                            postfix: true,
                            span: full,
                        };
                    }
                    _ => break,
                }
            }
            Ok(expr)
        }

        /// D-SPREAD1=A: after consuming `.`, parse `[a, b, c]` into `MemberSpread`.
        /// `spread_start` is the base expression's start (or the leading ident span).
        pub(super) fn parse_member_spread(
            &mut self,
            base: Expr,
            spread_start: usize,
        ) -> Result<Expr, Diagnostic> {
            self.bump(); // `[`
            let mut members = Vec::new();
            if !matches!(self.peek().kind, TokKind::RBracket) {
                loop {
                    // S84: package/member names may be dashed (`util-linux`).
                    if matches!(self.peek().kind, TokKind::Ident(_)) {
                        match self.expect_dashed_name("in a member spread `.[…]`") {
                            Ok((name, span)) => {
                                if !matches!(
                                    self.peek().kind,
                                    TokKind::Comma | TokKind::RBracket
                                ) {
                                    let bad = self.peek().span;
                                    self.diags.push(Diagnostic::error(
                                        "E0961",
                                        "`.[ ]` lists member names, not calls or expressions"
                                            .to_string(),
                                        "member spread names fields or package names that hang off the prefix"
                                            .to_string(),
                                        "write bare names like `default.[cargo, ripgrep]`"
                                            .to_string(),
                                        Some(bad),
                                    ));
                                    while !matches!(
                                        self.peek().kind,
                                        TokKind::Comma | TokKind::RBracket | TokKind::Eof
                                    ) {
                                        self.bump();
                                    }
                                } else {
                                    members.push((name, span));
                                }
                            }
                            Err(d) => {
                                self.diags.push(d);
                                while !matches!(
                                    self.peek().kind,
                                    TokKind::Comma | TokKind::RBracket | TokKind::Eof
                                ) {
                                    self.bump();
                                }
                            }
                        }
                    } else {
                        let bad = self.peek().span;
                        self.diags.push(Diagnostic::error(
                            "E0961",
                            "`.[ ]` lists member names, not calls or expressions"
                                .to_string(),
                            "member spread names fields or package names that hang off the prefix"
                                .to_string(),
                            "write bare names like `default.[cargo, ripgrep]`"
                                .to_string(),
                            Some(bad),
                        ));
                        while !matches!(
                            self.peek().kind,
                            TokKind::Comma | TokKind::RBracket | TokKind::Eof
                        ) {
                            self.bump();
                        }
                    }
                    if matches!(self.peek().kind, TokKind::Comma) {
                        self.bump();
                        if matches!(self.peek().kind, TokKind::RBracket) {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            self.expect(TokKind::RBracket, "to close a member spread `.[…]`")?;
            let end = self.toks[self.pos - 1].span.end;
            Ok(Expr::MemberSpread {
                base: Box::new(base),
                members,
                span: Span::new(spread_start, end),
            })
        }
    
        /// Reinterpret an already-parsed expression as an assignment target.
        pub(in crate::Parser) fn expr_to_lvalue(&mut self, expr: Expr) -> Result<LValue, Diagnostic> {
            match expr {
                Expr::Ident(name, name_span) => Ok(LValue::Local { name, name_span }),
                Expr::Index {
                    base, index, span, ..
                } => Ok(LValue::Index {
                    base,
                    index,
                    span,
                    kind: crate::AST::IndexKind::Unknown,
                }),
                // D-MUTSELF1: a field-access target `place.field = v` (the headline being
                // `self.field = v` in a `mut self` method). Sema gates whether the root is
                // mutable (E0205); the parser only records the place.
                Expr::Field(base, field, span) => Ok(LValue::Field { base, field, span }),
                other => Err(Diagnostic::error(
                    "E0003",
                    "this value can't be assigned to".to_string(),
                    "only a name or an indexed slot like `items[0]` can appear on the left of `=`"
                        .to_string(),
                    format!(
                        "use `name {} ...` or `map[key] = ...`",
                        Syntax::SIGIL_BIND_MUT
                    ),
                    Some(other.span()),
                )),
            }
        }
    
}
