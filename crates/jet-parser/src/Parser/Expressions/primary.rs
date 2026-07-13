use super::super::{
    AccessConvention, Call, CallArg, Diagnostic, Expr, Parser, Span, StrPart, StrTokPart, Syntax,
    TokKind, describe, retired_s14_teaching_enabled,
};

impl<'a> Parser<'a> {
        pub(super) fn expr_primary(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
            match self.peek().kind.clone() {
                TokKind::KwOk => {
                    let span = self.bump().span;
                    self.expect(TokKind::LParen, "after `ok`")?;
                    let inner = self.expr()?;
                    self.expect(TokKind::RParen, "after the value inside `ok(...)`")?;
                    let full = Span::new(span.start, inner.span().end);
                    Ok(Expr::Ok(Box::new(inner), full))
                }
                TokKind::KwErr => {
                    let span = self.bump().span;
                    self.expect(TokKind::LParen, "after `err`")?;
                    let inner = self.expr()?;
                    self.expect(TokKind::RParen, "after the value inside `err(...)`")?;
                    let full = Span::new(span.start, inner.span().end);
                    Ok(Expr::Err(Box::new(inner), full))
                }
                TokKind::KwIt => {
                    let span = self.bump().span;
                    Ok(Expr::Ident(Syntax::KW_IT.to_string(), span))
                }
                TokKind::Ident(name)
                    if name == Syntax::LIT_VALUE
                        && matches!(
                            self.toks.get(self.pos + 1).map(|t| &t.kind),
                            Some(TokKind::LParen)
                        ) =>
                {
                    let span = self.bump().span;
                    self.expect(TokKind::LParen, &format!("after `{}`", Syntax::LIT_VALUE))?;
                    let inner = self.expr()?;
                    self.expect(
                        TokKind::RParen,
                        &format!("after the value inside `{}(...)`", Syntax::LIT_VALUE),
                    )?;
                    let full = Span::new(span.start, inner.span().end);
                    Ok(Expr::Present(Box::new(inner), full))
                }
                TokKind::Ident(name)
                    if matches!(name.as_str(), "sql" | "html")
                        && matches!(
                            self.toks.get(self.pos + 1).map(|t| &t.kind),
                            Some(TokKind::Str(_))
                        )
                        && self
                            .toks
                            .get(self.pos + 1)
                            .is_some_and(|next| self.peek().span.end == next.span.start) =>
                {
                    let prefix = self.bump();
                    let str_tok = self.bump();
                    let TokKind::Str(parts) = str_tok.kind else {
                        unreachable!()
                    };
                    let str_expr = self.str_expr_from_parts(parts, str_tok.span)?;
                    let name = if name == "sql" {
                        Syntax::TYPED_TEXT_SQL_PREFIX_CALL
                    } else {
                        Syntax::TYPED_TEXT_HTML_PREFIX_CALL
                    };
                    Ok(Expr::Call(Call {
                        name: name.to_string(),
                        name_span: prefix.span,
                        args: vec![CallArg {
                            convention: AccessConvention::Read,
                            expr: str_expr,
                            span: str_tok.span,
                            flags: crate::AST::CallArgFlags::default(),
                            label: None,
                            spread: false,
                        }],
                        range_checked: false,
                    }))
                }
                TokKind::KwNull => {
                    let span = self.bump().span;
                    return Ok(Expr::Absent(span));
                }
                TokKind::Hash
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::Ident(n)) if n == Syntax::ATTR_META
                    ) =>
                {
                    let hash = self.bump().span;
                    let name_tok = self.bump();
                    return Err(self.meta_attr_wrong_place_diag(
                        Span::new(hash.start, name_tok.span.end),
                        "binding, const, or function",
                    ));
                }
                TokKind::Hash
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::Ident(n))
                            if n == Syntax::ATTR_OFF || n == Syntax::ATTR_DEBUG_ONLY
                    ) =>
                {
                    let hash = self.bump().span;
                    let name_tok = self.bump();
                    let name = match &name_tok.kind {
                        TokKind::Ident(n) => n.clone(),
                        _ => String::new(),
                    };
                    return Err(Diagnostic::error(
                        "E0343",
                        format!("`#{}` does not produce a value", name),
                        "statement switch attributes control a whole statement; expressions must still produce values in every build".to_string(),
                        format!("put it before the statement: `#{} <statement>`", name),
                        Some(Span::new(hash.start, name_tok.span.end)),
                    ));
                }
                TokKind::Hash
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::Ident(n)) if n == Syntax::KW_TODO
                    ) =>
                {
                    // D-TOOL2 (D-CASING1 follow-on): `#Todo` typed hole — valid in any
                    // expression position; sema fills `expected_type`; codegen emits a
                    // panic.
                    let start = self.bump().span.start; // `#`
                    let span = Span::new(start, self.bump().span.end); // `Todo`
                    return Ok(Expr::Todo {
                        span,
                        expected_type: None,
                    });
                }
                TokKind::Hash
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::Ident(n)) if n == Syntax::KW_TAINTED
                    ) =>
                {
                    // D-TAINT1: `#Tainted expr` — a value-fact tag marking the value
                    // as untrusted at its source. The tag binds tightly to the
                    // following primary expression. Taint propagation + the E0721
                    // sink check run in the sema taint pass; codegen erases the tag
                    // (I3), emitting the inner expression unchanged.
                    let start = self.bump().span.start; // `#`
                    self.bump(); // `Tainted`
                    let inner = self.expr_primary(allow_struct_lit)?;
                    let span = Span::new(start, inner.span().end);
                    return Ok(Expr::Tainted(Box::new(inner), span));
                }
                TokKind::Hash
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        // Any `#Ident` reaching here is past the `#Todo`/`#Tainted`
                        // expression markers handled above, so in expression position it
                        // is a SIMD reduce-op marker `#Add`/`#Mul`/`#Min`/`#Max`
                        // (D-SIMD2). Parse it generically; sema validates the closed set
                        // and the `.reduce(…)` position (E2510), giving a teaching error
                        // for a typo like `#Avg` instead of a bare parse error.
                        Some(TokKind::Ident(n))
                            if n != Syntax::KW_TODO && n != Syntax::KW_TAINTED
                    ) =>
                {
                    let start = self.bump().span.start; // `#`
                    let tok = self.bump(); // the op name
                    let name = if let TokKind::Ident(n) = &tok.kind {
                        n.clone()
                    } else {
                        String::new()
                    };
                    let span = Span::new(start, tok.span.end);
                    return Ok(Expr::ReduceMarker(name, span));
                }
                TokKind::Ident(name)
                    if retired_s14_teaching_enabled() && name == Syntax::FOREIGN_TODO =>
                {
                    // S14: bare lowercase `todo` is the retired spelling (E0054).
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0054",
                        format!(
                            "the typed hole is written `#{}`, not bare `{}`",
                            Syntax::KW_TODO,
                            Syntax::FOREIGN_TODO
                        ),
                        format!(
                            "`#{}` is a marker, like every other `#`-tag, so an unfinished spot draws the eye",
                            Syntax::KW_TODO
                        ),
                        format!("write: #{}", Syntax::KW_TODO),
                        Some(t.span),
                    ));
                    return Ok(Expr::Todo {
                        span: t.span,
                        expected_type: None,
                    });
                }
                TokKind::Ident(name)
                    if retired_s14_teaching_enabled()
                        && matches!(name.as_str(), Syntax::FOREIGN_THROW | Syntax::FOREIGN_RAISE) =>
                {
                    let t = self.bump();
                    let foreign = name.clone();
                    self.diags.push(Diagnostic::error(
                        "E0026",
                        format!("{} doesn't use `{}`", Syntax::LANG_NAME, foreign),
                        "a function that can fail returns `T ? E` and signals failure with `err(...)`"
                            .to_string(),
                        format!("return `err(...)` instead of `{}`", foreign),
                        Some(t.span),
                    ));
                    return self.expr_primary(allow_struct_lit);
                }
                TokKind::Ident(name)
                    if matches!(
                        name.as_str(),
                        Syntax::FOREIGN_CATCH | Syntax::FOREIGN_EXCEPT
                    ) && retired_s14_teaching_enabled() =>
                {
                    let t = self.bump();
                    let foreign = name.clone();
                    self.diags.push(Diagnostic::error(
                        "E0024",
                        format!("{} doesn't use `{}`", Syntax::LANG_NAME, foreign),
                        "handle a failure with `or` for a fallback, or test with `== err(...)`"
                            .to_string(),
                        format!(
                            "write `parse(x) or 0` or `if x == err(e) {{ ... }}` instead of `{}`",
                            foreign
                        ),
                        Some(t.span),
                    ));
                    return self.expr_primary(allow_struct_lit);
                }
                TokKind::Ident(name)
                    if matches!(
                        name.as_str(),
                        Syntax::FOREIGN_UNWRAP | Syntax::FOREIGN_EXPECT
                    ) && retired_s14_teaching_enabled() =>
                {
                    let t = self.bump();
                    let foreign = name.clone();
                    self.diags.push(Diagnostic::error(
                        "E0025",
                        format!("{} doesn't use `{}`", Syntax::LANG_NAME, foreign),
                        "when failure should stop the program, use `or panic(\"…\")`".to_string(),
                        format!(
                            "write `parse(x) or panic(\"…\")` instead of `.{}()`",
                            foreign
                        ),
                        Some(t.span),
                    ));
                    return self.expr_primary(allow_struct_lit);
                }
                // D-S14-PAUSE: optional spelling teaching is paused.
                TokKind::Ident(name)
                    if matches!(
                        name.as_str(),
                        Syntax::FOREIGN_SOME
                            | Syntax::FOREIGN_NIL
                            | Syntax::FOREIGN_NONE_LOWER
                            | Syntax::FOREIGN_SOME_LOWER
                    ) && retired_s14_teaching_enabled() =>
                {
                    let t = self.bump();
                    let foreign = if let TokKind::Ident(n) = &t.kind {
                        n.clone()
                    } else {
                        unreachable!()
                    };
                    let (canonical, fix) = match foreign.as_str() {
                        Syntax::FOREIGN_NONE_LOWER | Syntax::FOREIGN_NIL => {
                            (Syntax::LIT_NULL, Syntax::LIT_NULL)
                        }
                        _ => (Syntax::LIT_VALUE, Syntax::LIT_VALUE),
                    };
                    self.diags.push(Diagnostic::error(
                        "E0020",
                        format!(
                            "optional values use `{}` and `{}`, not `{}`",
                            Syntax::LIT_VALUE,
                            Syntax::LIT_NULL,
                            foreign
                        ),
                        format!(
                            "{} uses exactly one spelling for each thing, so all code reads the same",
                            Syntax::LANG_NAME
                        ),
                        format!("replace `{}` with `{}`", foreign, fix),
                        Some(t.span),
                    ));
                    if canonical == Syntax::LIT_NULL {
                        Ok(Expr::Absent(t.span))
                    } else {
                        self.expect(TokKind::LParen, &format!("after `{}`", Syntax::LIT_VALUE))?;
                        let inner = self.expr()?;
                        self.expect(
                            TokKind::RParen,
                            &format!("after the value inside `{}(...)`", Syntax::LIT_VALUE),
                        )?;
                        let full = Span::new(t.span.start, inner.span().end);
                        Ok(Expr::Present(Box::new(inner), full))
                    }
                }
                TokKind::Str(parts) => {
                    let span = self.bump().span;
                    self.str_expr_from_parts(parts, span)
                }
                TokKind::Int(n) => {
                    let span = self.bump().span;
                    Ok(Expr::Int(n, span, None))
                }
                TokKind::Float(v) => {
                    let span = self.bump().span;
                    Ok(Expr::Float(v, span, false))
                }
                // D-UNITLIT1: `500ms`, `12.50usd` — a numeric literal with a unit
                // suffix. The lexer already separated the exponent form; anything
                // reaching here as `UnitNumber` genuinely carries a suffix.
                TokKind::UnitNumber { .. } => {
                    let tok = self.bump();
                    let TokKind::UnitNumber { raw, int, float, suffix } = tok.kind else {
                        unreachable!("guarded by the outer match above")
                    };
                    let span = tok.span;
                    // The suffix sits at the tail of the token's span (no space
                    // between the digits and the suffix — the lexer requires that).
                    let suffix_span = Span::new(span.end - suffix.len(), span.end);
                    Ok(Expr::UnitLit {
                        raw,
                        int,
                        float,
                        suffix,
                        suffix_span,
                        span,
                    })
                }
                TokKind::Char(ch) => {
                    let span = self.bump().span;
                    Ok(Expr::Char(ch, span))
                }
                TokKind::LBracket => self.list_or_map_lit(),
                TokKind::KwTrue => {
                    let span = self.bump().span;
                    Ok(Expr::Bool(true, span))
                }
                TokKind::KwFalse => {
                    let span = self.bump().span;
                    Ok(Expr::Bool(false, span))
                }
                TokKind::KwSelf => {
                    let span = self.bump().span;
                    Ok(Expr::Ident(Syntax::KW_SELF.to_string(), span))
                }
                // S68 (D-SG2): `if` used as a value. Statement-position `if` is
                // handled earlier in `stmt`, so reaching here means expression use.
                TokKind::KwIf => self.parse_if_expr(),
                TokKind::KwMove
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::LParen)
                    ) =>
                {
                    let takes = self.parse_lambda_takes()?;
                    Ok(Expr::Lambda(self.parse_lambda(takes)?))
                }
                TokKind::LParen if self.after_lparen_is_lambda() => {
                    Ok(Expr::Lambda(self.parse_lambda(vec![])?))
                }
                // D-LAMBDA-INFER1 (ratified 2026-07-04): a bare single-param
                // lambda with no parens — `m => m.hp > 0`. Sema accepts it only
                // where the expected type fixes the param type (E0801 elsewhere).
                TokKind::Ident(_) if matches!(self.peek2().kind, TokKind::LambdaArrow) => {
                    Ok(Expr::Lambda(self.parse_bare_lambda()?))
                }
                TokKind::LParen => self.parse_paren_primary(allow_struct_lit),
                TokKind::Pipe if retired_s14_teaching_enabled() => {
                    let span = self.bump().span;
                    self.diags.push(Diagnostic::error(
                        "E0033",
                        format!("{} doesn't use `|` pipes for lambdas", Syntax::LANG_NAME),
                        "a short function is written with parentheses and `=>`".to_string(),
                        "write `(x) => x + 1` instead of `|x| x + 1`".to_string(),
                        Some(span),
                    ));
                    while !matches!(self.peek().kind, TokKind::Pipe | TokKind::Eof) {
                        self.bump();
                    }
                    if matches!(self.peek().kind, TokKind::Pipe) {
                        self.bump();
                    }
                    return self.expr_primary(allow_struct_lit);
                }
                TokKind::Ident(name)
                    if retired_s14_teaching_enabled() && name == Syntax::FOREIGN_LAMBDA =>
                {
                    let span = self.bump().span;
                    self.diags.push(Diagnostic::error(
                        "E0032",
                        format!(
                            "{} doesn't use the `{}` keyword for short functions",
                            Syntax::LANG_NAME,
                            Syntax::FOREIGN_LAMBDA
                        ),
                        "write a lambda with parentheses and `=>` instead".to_string(),
                        "e.g. `(x) => x + 1` instead of `lambda x { ... }`".to_string(),
                        Some(span),
                    ));
                    return self.expr_primary(allow_struct_lit);
                }
                TokKind::Ident(name)
                    if retired_s14_teaching_enabled() && name == Syntax::FOREIGN_AS =>
                {
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0030",
                        format!(
                            "{} doesn't use `{}` for conversions",
                            Syntax::LANG_NAME,
                            Syntax::FOREIGN_AS
                        ),
                        "convert with methods like `.to_float()` or `.to_string()`".to_string(),
                        "e.g. `x.to_float()` instead of `x as Float`".to_string(),
                        Some(t.span),
                    ));
                    return self.expr_primary(allow_struct_lit);
                }
                TokKind::Ident(name)
                    if retired_s14_teaching_enabled() && name == Syntax::FOREIGN_APPEND =>
                {
                    let span = self.bump().span;
                    self.diags.push(Diagnostic::error(
                        "E0027",
                        format!("lists use `{}`, not `{}`", "push", Syntax::FOREIGN_APPEND),
                        "add an item to the end of a list with `.push(value)`".to_string(),
                        "e.g. `items.push(x)`".to_string(),
                        Some(span),
                    ));
                    if matches!(self.peek().kind, TokKind::LParen) {
                        self.bump();
                        let _ = self.expr();
                        let _ = self.expect(TokKind::RParen, "after append args");
                    }
                    return self.expr_primary(allow_struct_lit);
                }
                TokKind::Ident(name) => {
                    let span = self.bump().span;
                    let type_name = name.clone();
                    let mut type_args = Vec::new();
                    if allow_struct_lit
                        && matches!(self.peek().kind, TokKind::Lt)
                        && type_name.chars().next().is_some_and(|c| c.is_uppercase())
                    {
                        self.expect_type_args_open(&type_name)?;
                        loop {
                            let (arg, _) = self.type_()?;
                            type_args.push(arg);
                            if matches!(self.peek().kind, TokKind::Comma) {
                                self.bump();
                                continue;
                            }
                            break;
                        }
                        self.expect_type_args_close(&format!("after `{type_name}<…>`"))?;
                    }
                    if allow_struct_lit
                        && matches!(self.peek().kind, TokKind::LBrace)
                        && type_name.chars().next().is_some_and(|c| c.is_uppercase())
                    {
                        // D-DOTCTOR2: old dotless `Type { … }` form — teaching error E0320.
                        // Recover: parse the fields as if the user had written `Type.{ … }`.
                        // A lowercase name falls through to a plain `Ident`, letting
                        // `expr_postfix` read a following `{` as a D-TRAILBLOCK1 trailing
                        // block (`callee { … }`) instead — same case-based split as the
                        // turbofish check above.
                        let brace_span = self.peek().span;
                        self.diags.push(Diagnostic::error(
                            "E0320",
                            format!(
                                "struct construction uses `{}.{{…}}`, not `{} {{…}}`",
                                type_name, type_name
                            ),
                            "named construction has a dot before the brace (D-DOTCTOR1)".to_string(),
                            format!("write `{}.{{…}}` instead", type_name),
                            Some(brace_span),
                        ));
                        return self.struct_lit_after_name(type_name, type_args, span);
                    }
                    if matches!(self.peek().kind, TokKind::Dot) {
                        let dot_span = self.bump().span;
                        // S75 (2026-06-16): `ident.[a, b, c]` fan-out
                        if matches!(self.peek().kind, TokKind::LBracket) {
                            let callee = Box::new(Expr::Ident(type_name, span));
                            return self.parse_fan_out_bracket(callee, dot_span);
                        }
                        // D-CAP9: postfix `name.*` — dereference a raw pointer.
                        // Returning the `Deref` lets `expr_postfix`'s loop pick up a
                        // following `.field`, giving `name.*.field`.
                        if matches!(self.peek().kind, TokKind::Star) {
                            let star = self.bump().span;
                            let full = Span::new(span.start, star.end);
                            return Ok(Expr::Deref(Box::new(Expr::Ident(type_name, span)), full));
                        }
                        // D-DOTCTOR1: `Type.{ … }` named construction.
                        if allow_struct_lit && matches!(self.peek().kind, TokKind::LBrace) {
                            return self.struct_lit_after_name(type_name, type_args, span);
                        }
                        let (member, member_span) = self.expect_field_name()?;
                        // S58 (E2-M13): `alias.Ptr<T>.from_addr(addr)` — a typed
                        // pointer constructor through a `core.mem` alias. Recognise
                        // the `<…>` here (primary position, where `alias.Member` is
                        // consumed) so `<` is read as a type-arg list, not a
                        // comparison. Mirrors the postfix-position trigger.
                        if member == Syntax::TYPE_PTR && matches!(self.peek().kind, TokKind::Lt) {
                            return self.ptr_from_addr(type_name, span);
                        }
                        // D-SERDE6: optional call-site turbofish `csv.decode<Order>(raw)`.
                        // D-MEM1 S6 fix: don't blindly overwrite `type_args` — a
                        // capitalized receiver's OWN turbofish (`Pool<Player>.new()`,
                        // parsed above at the type-name position) has no call-site
                        // turbofish of its own (`self.at_turbofish()` is false right
                        // after `.new`), so this used to silently clobber it with an
                        // empty `Vec`, losing `Player` entirely. The two positions are
                        // mutually exclusive in practice (a lowercase alias like `csv`
                        // never reaches the type-name turbofish parse above), so
                        // falling back to the outer `type_args` when there's no
                        // call-site turbofish is a pure bug fix, not a behavior change.
                        let type_args = if self.at_turbofish() {
                            self.parse_turbofish()?
                        } else {
                            type_args
                        };
                        if matches!(self.peek().kind, TokKind::LParen) {
                            self.bump();
                            let mut args = Vec::new();
                            if member == Syntax::METHOD_TAKE_PATTERN {
                                // D-SHIFT1: `cursor.take_pattern("…")` — mirrors the
                                // identical carve-out in `expr_postfix` above, for
                                // the bare-leading-ident receiver fast path.
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
                                // D-DYNARRAY1: `.view(a..b)` — mirrors the identical
                                // carve-out in `expr_postfix` above; this is the
                                // separate fast-path `expr_primary` takes when the
                                // receiver is a bare leading identifier (`incidents.
                                // view(0..2)`), not a chained postfix expression.
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
                            return Ok(Expr::MethodCall {
                                receiver: Box::new(Expr::Ident(type_name, span)),
                                method: member,
                                method_span: member_span,
                                type_args,
                                args,
                                recv_type: None,
                                resolved_ret: None,
                            });
                        }
                        return Ok(Expr::Field(
                            Box::new(Expr::Ident(type_name, span)),
                            member,
                            member_span,
                        ));
                    }
                    if matches!(self.peek().kind, TokKind::LParen) {
                        let call = self.call_after_name(type_name, span)?;
                        return Ok(Expr::Call(call));
                    }
                    Ok(Expr::Ident(type_name, span))
                }
                // D-CTMARKER1=C: `$name` — comptime splice expression.
                TokKind::Dollar => {
                    let dollar_span = self.bump().span;
                    let (name, name_span) = self.expect_ident("after `$` in a comptime splice")?;
                    let full = Span::new(dollar_span.start, name_span.end);
                    Ok(Expr::ComptimeSplice {
                        name,
                        span: full,
                        value: None,
                    })
                }
                other => Err(Diagnostic::error(
                    "E0003",
                    format!("expected a value, found {}", describe(&other)),
                    "a value can be a name, a number, quoted text, `true`/`false`, or a call"
                        .to_string(),
                    "e.g. `x`, `42`, `3.5`, or `\"hello\"`".to_string(),
                    Some(self.peek().span),
                )),
            }
        }
    
        fn str_expr_from_parts(
            &mut self,
            parts: Vec<StrTokPart>,
            span: Span,
        ) -> Result<Expr, Diagnostic> {
            let mut out = Vec::new();
            for part in parts {
                match part {
                    StrTokPart::Lit(s) => out.push(StrPart::Lit(s)),
                    StrTokPart::Interp(toks) => {
                        let mut sub = Parser {
                            toks: &toks,
                            pos: 0,
                            diags: Vec::new(),
                            pending_type_gt: false,
                            depth: self.depth,
                            type_generic_depth: 0,
                            type_generic_chain: Vec::new(),
                            type_generic_truncated: false,
                            arm_head_term: false,
                            pub_file_default: false,
                            in_layout_body: self.in_layout_body,
                            module_arg_expr_depth: None,
                        };
                        let e = sub.expr()?;
                        if !sub.diags.is_empty() {
                            let mut ds = sub.diags;
                            let first = ds.remove(0);
                            self.diags.extend(ds);
                            return Err(first);
                        }
                        let mut format = crate::AST::StrFormat::Display;
                        if matches!(sub.peek().kind, TokKind::At) {
                            sub.bump();
                            let (sel, sel_span) = sub.expect_ident("after `@` in interpolation")?;
                            if sel == crate::Syntax::INTERP_SELECTOR_DEBUG {
                                format = crate::AST::StrFormat::Debug;
                            } else {
                                self.diags.push(crate::Generics::e0914(&sel, sel_span));
                            }
                        }
                        if !matches!(sub.peek().kind, TokKind::Eof) {
                            return Err(Diagnostic::error(
                                "E0003",
                                format!(
                                    "unexpected {} inside this interpolated `{{ }}`",
                                    describe(&sub.peek().kind)
                                ),
                                "the braces hold exactly one value (and an optional `@Debug` selector)"
                                    .to_string(),
                                "keep one value per `{ }`, e.g. \"{a}\" or \"{a@Debug}\"".to_string(),
                                Some(sub.peek().span),
                            ));
                        }
                        out.push(StrPart::Interp(Box::new(e), format));
                    }
                }
            }
            Ok(Expr::Str(out, span))
        }
    
}
