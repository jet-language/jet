use super::super::{
    AccessConvention, Call, CallArg, Diagnostic, Expr, LValue, Parser, Span, Syntax, TokKind,
    TryConvert,
};

impl<'a> Parser<'a> {
        pub(super) fn expr_postfix(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
            let mut expr = self.expr_primary(allow_struct_lit)?;
            // D-TRAILBLOCK1: a call takes at most one trailing `{ }` block.
            let mut trailing_block_attached = false;
            loop {
                match &self.peek().kind {
                    TokKind::Dot => {
                        let dot = self.bump().span;
                        // S75 (2026-06-16): `f.[a, b, c]` fan-out — `.` immediately followed by `[`
                        if matches!(self.peek().kind, TokKind::LBracket) {
                            expr = self.parse_fan_out_bracket(Box::new(expr), dot)?;
                            continue;
                        }
                        // D-CAP9: postfix `p.*` — dereference a raw pointer. The `.`
                        // followed by `*` reads as deref (it composes with a further
                        // `.field`, giving `p.*.field`). Gated to `#Unsafe` in sema.
                        if matches!(self.peek().kind, TokKind::Star) {
                            let star = self.bump().span;
                            let full = Span::new(expr.span().start, star.end);
                            expr = Expr::Deref(Box::new(expr), full);
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
                        // D-SERDE6: optional call-site turbofish `decode<Order>(…)`.
                        let type_args = if self.at_turbofish() {
                            self.parse_turbofish()?
                        } else {
                            Vec::new()
                        };
                        // D-TASKSCOPE1=A: `g.task { … }` — block body desugars to a
                        // zero-parameter lambda (same closure shape as `tasks.spawn`).
                        if member == Syntax::TASKGROUP_SPAWN_METHOD
                            && matches!(self.peek().kind, TokKind::LBrace)
                        {
                            let lam = self.parse_task_body_lambda()?;
                            let lam_span = lam.span;
                            expr = Expr::MethodCall {
                                receiver: Box::new(expr),
                                method: member.clone(),
                                method_span: member_span,
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
                                let start = self.expr()?;
                                self.expect(
                                    TokKind::DotDot,
                                    "a `..` between the view's start and end",
                                )?;
                                let end = self.expr()?;
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
                        let (member, member_span) = self.expect_ident("after `?.`")?;
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
                    TokKind::LBrace => {
                        // D-TASKSCOPE1=A: `g.task { … }` after `.task` was parsed as a field.
                        if let Expr::Field(base, member, member_span) = &expr {
                            if member == Syntax::TASKGROUP_SPAWN_METHOD {
                                let lam = self.parse_task_body_lambda()?;
                                let lam_span = lam.span;
                                expr = Expr::MethodCall {
                                    receiver: base.clone(),
                                    method: member.clone(),
                                    method_span: *member_span,
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
                            // D-TRAILBLOCK1: `callee(args) { … }` — a bare `{` directly
                            // after a call's `)` (or after a callable name with no `()`
                            // at all, `callee { … }`) is a trailing ZERO-PARAMETER
                            // lambda filling the call's last argument. Jet has no
                            // bare-block statements, so this slot was free; a PascalCase
                            // name still takes the E0320 dotless-struct-literal recovery
                            // above (mirrors the turbofish case rule).
                            if trailing_block_attached {
                                let bad_span = self.peek().span;
                                return Err(Diagnostic::error(
                                    "E0335",
                                    "a call takes only one trailing block".to_string(),
                                    "a trailing `{ }` fills exactly one last argument; a second `{ }` has nothing to fill".to_string(),
                                    "pass the extra block as an ordinary argument inside the parentheses, or remove it".to_string(),
                                    Some(bad_span),
                                ));
                            }
                            let lam = self.parse_task_body_lambda()?;
                            let lam_span = lam.span;
                            let arg = CallArg {
                                convention: AccessConvention::Read,
                                expr: Expr::Lambda(lam),
                                span: lam_span,
                                flags: crate::AST::CallArgFlags {
                                    is_trailing_block: true,
                                    ..Default::default()
                                },
                                label: None,
                                spread: false,
                            };
                            expr = match expr {
                                Expr::Ident(name, name_span) => Expr::Call(Call {
                                    name,
                                    name_span,
                                    args: vec![arg],
                                    range_checked: false,
                                }),
                                Expr::Call(mut c) => {
                                    c.args.push(arg);
                                    Expr::Call(c)
                                }
                                Expr::MethodCall {
                                    receiver,
                                    method,
                                    method_span,
                                    type_args,
                                    mut args,
                                    recv_type,
                                    resolved_ret,
                                } => {
                                    args.push(arg);
                                    Expr::MethodCall {
                                        receiver,
                                        method,
                                        method_span,
                                        type_args,
                                        args,
                                        recv_type,
                                        resolved_ret,
                                    }
                                }
                                Expr::CallValue {
                                    callee,
                                    mut args,
                                    span,
                                } => {
                                    args.push(arg);
                                    Expr::CallValue {
                                        callee,
                                        args,
                                        span: Span::new(span.start, lam_span.end),
                                    }
                                }
                                _ => unreachable!("guarded by the outer match above"),
                            };
                            trailing_block_attached = true;
                            continue;
                        } else if allow_struct_lit
                            && matches!(
                                expr,
                                Expr::Field(..) | Expr::Index { .. } | Expr::Slice { .. }
                            )
                        {
                            // D-TRAILBLOCK1: a trailing `{ }` on something that isn't a
                            // call at all (a plain field/index read) — teach the shape
                            // rather than falling through to a generic parse error.
                            let bad_span = self.peek().span;
                            return Err(Diagnostic::error(
                                "E0335",
                                "a trailing block only follows a call".to_string(),
                                "a trailing `{ }` fills a call's last argument; this isn't a call"
                                    .to_string(),
                                "call it first, e.g. `name(){ … }`, or remove the block".to_string(),
                                Some(bad_span),
                            ));
                        } else {
                            break;
                        }
                    }
                    TokKind::LBracket => {
                        let open = self.bump().span;
                        let start = self.expr()?;
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
    
        /// S75 (2026-06-16): parse `.[item, …]` after the `.` has already been consumed.
        /// `dot_span` is the span of the consumed `.`. Called from both `expr_primary`
        /// (for `ident.[…]`) and `expr_postfix` (for chained `expr.[…]`).
        pub(super) fn parse_fan_out_bracket(
            &mut self,
            callee: Box<Expr>,
            dot_span: Span,
        ) -> Result<Expr, Diagnostic> {
            self.bump(); // consume `[`
            let mut items = Vec::new();
            if !matches!(self.peek().kind, TokKind::RBracket) {
                loop {
                    items.push(self.expr()?);
                    if matches!(self.peek().kind, TokKind::RBracket) {
                        break;
                    }
                    self.expect(TokKind::Comma, "between fan-out items")?;
                    if matches!(self.peek().kind, TokKind::RBracket) {
                        break; // trailing comma
                    }
                }
            }
            self.expect(TokKind::RBracket, "to close the fan-out `.[`")?;
            let close = self.toks[self.pos - 1].span;
            let span = Span::new(dot_span.start, close.end);
            Ok(Expr::FanOut {
                callee,
                items,
                span,
            })
        }
    
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
