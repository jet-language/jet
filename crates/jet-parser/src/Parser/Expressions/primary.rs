use super::super::{
    AccessConvention, CallArg, Diagnostic, EnumLitArg, Expr, Lambda, LambdaBody, LambdaMeta,
    Parser, Span, StrPart, StrTokPart, Syntax, TokKind, describe,
};

impl<'a> Parser<'a> {
        pub(super) fn expr_primary(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
            match self.peek().kind.clone() {
                TokKind::KwLoop => self.yielding_loop_expr(),
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
                    Ok(Expr::EnumLit {
                        type_name: String::new(),
                        variant: Syntax::LIT_VALUE.to_string(),
                        args: vec![EnumLitArg::Positional(inner)],
                        leading_dot: false,
                        span: full,
                    })
                }
                TokKind::KwNull => {
                    let span = self.bump().span;
                    return Ok(Expr::EnumLit {
                        type_name: String::new(),
                        variant: Syntax::LIT_NULL.to_string(),
                        args: Vec::new(),
                        leading_dot: false,
                        span,
                    });
                }
                // D-VERDICT-1455-1: one marker read at expression position. The
                // shared reader takes the name — any name, open vocabulary
                // included — and only then does this classify what it means.
                TokKind::Hash
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::Ident(_))
                    ) =>
                {
                    let head = self.read_marker_head()?;
                    let start = head.span.start;
                    return match head.name.as_str() {
                        Syntax::MARKER_META => {
                            Err(self.meta_attr_wrong_place_diag(head.span, "binding or function"))
                        }
                        Syntax::MARKER_OFF | Syntax::MARKER_DEBUG_ONLY => Err(Diagnostic::error(
                            "E0343",
                            format!("`#{}` does not produce a value", head.name),
                            "statement switch attributes control a whole statement; expressions must still produce values in every build".to_string(),
                            format!("put it before the statement: `#{} <statement>`", head.name),
                            Some(head.span),
                        )),
                        // D-TOOL2 (D-CASING1 follow-on): `#Todo` typed hole — valid in any
                        // expression position; sema fills `expected_type`; codegen emits a
                        // panic.
                        Syntax::KW_TODO => Ok(Expr::Todo {
                            span: head.span,
                            expected_type: None,
                        }),
                        // D-TAG-SURFACE1=A: recover retired `#Tainted[(Kind)] value`
                        // as the corresponding ordinary tag application.
                        Syntax::KW_TAINTED => {
                            // Optional `(Kind)` argument after the keyword.
                            let kind: Option<String> =
                                if matches!(self.toks.get(self.pos).map(|t| &t.kind), Some(TokKind::LParen)) {
                                    self.bump(); // `(`
                                    let kind_name = if let Some(tok) = self.toks.get(self.pos) {
                                        if let TokKind::Ident(n) = &tok.kind {
                                            let n = n.clone();
                                            self.bump();
                                            n
                                        } else {
                                            String::new()
                                        }
                                    } else {
                                        String::new()
                                    };
                                    // Consume the closing `)`.
                                    if matches!(self.toks.get(self.pos).map(|t| &t.kind), Some(TokKind::RParen)) {
                                        self.bump();
                                    }
                                    if kind_name.is_empty() { None } else { Some(kind_name) }
                                } else {
                                    None
                                };
                            let tag = kind.unwrap_or_else(|| "Input".to_string());
                            self.diags.push(Diagnostic::error(
                                "E0927",
                                "`#Tainted` is retired".to_string(),
                                "taint kinds are ordinary declared fact tags".to_string(),
                                format!("write `#{tag} value`"),
                                Some(head.span),
                            ));
                            let inner = self.expr_primary(allow_struct_lit)?;
                            let span = Span::new(start, inner.span().end);
                            Ok(Expr::Tainted(Box::new(inner), Some(tag), span))
                        }
                        // The four old SIMD reduce selectors remain parser
                        // recovery nodes until their retirement diagnostic fires.
                        "Add" | "Mul" | "Min" | "Max" => {
                            Ok(Expr::ReduceMarker(head.name.clone(), head.span))
                        }
                        // D-TAG-SURFACE1=A: any declared tag may prefix a value.
                        _ => {
                            let inner = self.expr_primary(allow_struct_lit)?;
                            let span = Span::new(start, inner.span().end);
                            Ok(Expr::Tainted(Box::new(inner), Some(head.name.clone()), span))
                        }
                    };
                }
                TokKind::At => {
                    let span = self.bump().span;
                    return Err(Diagnostic::error(
                        "E0063",
                        "applied rules use `#`, not `@`".to_string(),
                        "`#` marks attributes, instructions, and properties; `@` marks locations, addresses, and sources (D-VERDICT-732-1)".to_string(),
                        "replace the leading `@` with `#`".to_string(),
                        Some(span),
                    ));
                }
                TokKind::Ident(name)
                    if false && name == Syntax::FOREIGN_TODO =>
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
                    if false
                        && matches!(name.as_str(), Syntax::FOREIGN_THROW | Syntax::FOREIGN_RAISE) =>
                {
                    let t = self.bump();
                    let foreign = name.clone();
                    self.diags.push(Diagnostic::error(
                        "E0026",
                        format!("{} doesn't use `{}`", Syntax::LANG_NAME, foreign),
                        "a function that can fail returns `T ? E` and signals failure with `Err(...)`"
                            .to_string(),
                        format!("return `Err(...)` instead of `{}`", foreign),
                        Some(t.span),
                    ));
                    return self.expr_primary(allow_struct_lit);
                }
                TokKind::Ident(name)
                    if matches!(
                        name.as_str(),
                        Syntax::FOREIGN_CATCH | Syntax::FOREIGN_EXCEPT
                    ) && false =>
                {
                    let t = self.bump();
                    let foreign = name.clone();
                    self.diags.push(Diagnostic::error(
                        "E0024",
                        format!("{} doesn't use `{}`", Syntax::LANG_NAME, foreign),
                        "handle a failure with `or` for a fallback, or test with `== .Err(...)`"
                            .to_string(),
                        format!(
                            "write `parse(x) or 0` or `if x == .Err(e) {{ ... }}` instead of `{}`",
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
                    ) && false =>
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
                TokKind::Str(parts) => {
                    let span = self.bump().span;
                    self.str_expr_from_parts(parts, span)
                }
                TokKind::Int(n, raw) => {
                    let span = self.bump().span;
                    Ok(Expr::Int(n, span, None, Some(raw)))
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
                TokKind::Ident(name)
                    if false && name == Syntax::FOREIGN_LAMBDA =>
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
                    if false && name == Syntax::FOREIGN_AS =>
                {
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0030",
                        format!(
                            "{} doesn't use `{}` for conversions",
                            Syntax::LANG_NAME,
                            Syntax::FOREIGN_AS
                        ),
                        "the destination type owns conversion with `Target.from_source(value)`".to_string(),
                        "e.g. `Float.from_int(x)` instead of `x as Float`".to_string(),
                        Some(t.span),
                    ));
                    return self.expr_primary(allow_struct_lit);
                }
                TokKind::Ident(name)
                    if false && name == Syntax::FOREIGN_APPEND =>
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
                // D-META-STAGE1=B: `$limit` reads a compile-time name. The
                // lexer merges the mark into one `Ident` token, and the mark
                // stays on the name here — a marked name and a plain name are
                // two different names. Declaration positions consume the same
                // token through `expect_ident`, so they bind the marked name.
                TokKind::Ident(name) if Syntax::is_comptime_name(name.as_str()) => {
                    let span = self.bump().span;
                    Ok(Expr::ComptimeName {
                        name,
                        span,
                        value: None,
                    })
                }
                TokKind::Ident(name) if name == Syntax::KW_CONC_TASK => self.task_surface_expr(),
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
                        // A lowercase name falls through to a plain `Ident`. A following
                        // `{` after a call is E0335 under D-TRAILBLOCK2 (retired trailing
                        // sugar) — not a desugared lambda argument.
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
                        self.bump();
                        // D-CAP9: postfix `name.*` — dereference a raw pointer.
                        // Returning the `Deref` lets `expr_postfix`'s loop pick up a
                        // following `.field`, giving `name.*.field`.
                        if matches!(self.peek().kind, TokKind::Star) {
                            let star = self.bump().span;
                            let full = Span::new(span.start, star.end);
                            return Ok(Expr::Deref(Box::new(Expr::Ident(type_name, span)), full));
                        }
                        // D-SPREAD1=A: `prefix.[a, b, c]` member spread (primary path —
                        // bare idents consume `.` here before postfix sees them).
                        if matches!(self.peek().kind, TokKind::LBracket) {
                            let base = Expr::Ident(type_name, span);
                            return self.parse_member_spread(base, span.start);
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
                        // D-GENERIC-CALL1=A: optional call-site type arguments on
                        // every qualified call, such as `csv.decode<Order>(raw)`.
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
                        let method_type_args = if self.at_turbofish() {
                            self.parse_turbofish()?
                        } else {
                            Vec::new()
                        };
                        if matches!(self.peek().kind, TokKind::LParen) {
                            // D-JPK-BUILDRECIPE1: the finite build-step values
                            // use lower-case leading-dot names (`.exec(...)`,
                            // `.install(...)`). Keep that spelling scoped to
                            // the ratified Recipe.build call; ordinary value
                            // expressions retain the existing upper-case-only
                            // leading-dot enum grammar.
                            let recipe_build =
                                type_name == Syntax::RECIPE_TYPE
                                    && member == Syntax::RECIPE_BUILD_METHOD;
                            let previous_lowercase = self.allow_lowercase_leading_dot;
                            if recipe_build {
                                self.allow_lowercase_leading_dot = true;
                            }
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
                            let result = Expr::MethodCall {
                                receiver: Box::new(Expr::Ident(type_name, span)),
                                method: member,
                                method_span: member_span,
                                owner_type_args: type_args,
                                type_args: method_type_args,
                                args,
                                recv_type: None,
                                resolved_ret: None,
                                checked_widen: false,
                            };
                            self.allow_lowercase_leading_dot = previous_lowercase;
                            return Ok(result);
                        }
                        return Ok(Expr::Field(
                            Box::new(Expr::Ident(type_name, span)),
                            member,
                            member_span,
                        ));
                    }
                    let call_type_args = if self.at_turbofish() {
                        self.parse_turbofish()?
                    } else {
                        type_args
                    };
                    if matches!(self.peek().kind, TokKind::LParen) {
                        let call = self.call_after_name(type_name, span, call_type_args)?;
                        return Ok(Expr::Call(call));
                    }
                    Ok(Expr::Ident(type_name, span))
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

        /// D-CONC-SPAWN1=D: one `task` word owns single-task spawn and the
        /// nested `all`/`race`/`any` combinators. Lower these forms into the
        /// existing method-call seam; sema and TIR keep one task mechanism.
        fn task_surface_expr(&mut self) -> Result<Expr, Diagnostic> {
            let task_span = self.bump().span;
            if matches!(self.peek().kind, TokKind::Dot) {
                self.bump();
                let (selector, selector_span) = self.expect_ident("after `task.`")?;
                if selector == "group" {
                    return Err(Diagnostic::error(
                        "E0003",
                        "`task.group` is a statement, not a value".to_string(),
                        "a task group owns a block and its children until the closing brace"
                            .to_string(),
                        "write `task.group name { … }`".to_string(),
                        Some(selector_span),
                    ));
                }
                if !matches!(selector.as_str(), "all" | "race" | "any") {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("unknown task selector `{selector}`"),
                        "task combinators use only `all`, `race`, or `any`".to_string(),
                        "write `task.all { … }`, `task.race { … }`, or `task.any { … }`"
                            .to_string(),
                        Some(selector_span),
                    ));
                }
                self.expect(TokKind::LBrace, "after the task selector")?;
                let open = self.toks[self.pos - 1].span;
                let mut branches = Vec::new();
                if !matches!(self.peek().kind, TokKind::RBrace) {
                    loop {
                        let (body, body_span) = if matches!(self.peek().kind, TokKind::LBrace) {
                            let open = self.bump().span;
                            let body = self.block_stmts();
                            let end = self.toks[self.pos - 1].span.end;
                            (LambdaBody::Block(body), Span::new(open.start, end))
                        } else {
                            let body = self.expr()?;
                            let body_span = body.span();
                            (LambdaBody::Expr(Box::new(body)), body_span)
                        };
                        branches.push((body, body_span));
                        if matches!(self.peek().kind, TokKind::RBrace) {
                            break;
                        }
                        self.expect(TokKind::Comma, "between task branches")?;
                    }
                }
                self.expect(TokKind::RBrace, "to close the task combinator")?;
                let close = self.toks[self.pos - 1].span;
                let list_span = Span::new(open.start, close.end);
                let tasks = branches
                    .into_iter()
                    .map(|(body, body_span)| {
                        let lambda = Lambda {
                            take_names: Vec::new(),
                            params: Vec::new(),
                            body,
                            span: body_span,
                            meta: LambdaMeta::default(),
                        };
                        Expr::MethodCall {
                            receiver: Box::new(Expr::Ident(
                                Syntax::INTERNAL_TASK_RECEIVER.to_string(),
                                task_span,
                            )),
                            method: "spawn".to_string(),
                            method_span: task_span,
                            owner_type_args: Vec::new(),
                            type_args: Vec::new(),
                            args: vec![CallArg {
                                convention: AccessConvention::Read,
                                expr: Expr::Lambda(lambda),
                                span: body_span,
                                flags: crate::AST::CallArgFlags::default(),
                                label: None,
                                spread: false,
                            }],
                            recv_type: None,
                            resolved_ret: None,
                            checked_widen: false,
                        }
                    })
                    .collect();
                return Ok(Expr::MethodCall {
                    receiver: Box::new(Expr::Ident(
                        Syntax::INTERNAL_TASK_RECEIVER.to_string(),
                        task_span,
                    )),
                    method: selector,
                    method_span: selector_span,
                    owner_type_args: Vec::new(),
                    type_args: Vec::new(),
                    args: vec![CallArg {
                        convention: AccessConvention::Read,
                        expr: Expr::ListLit(tasks, list_span),
                        span: list_span,
                        flags: crate::AST::CallArgFlags::default(),
                        label: None,
                        spread: false,
                    }],
                    recv_type: None,
                    resolved_ret: None,
                    checked_widen: false,
                });
            }

            let (body, body_span) = if matches!(self.peek().kind, TokKind::LBrace) {
                let open = self.bump().span;
                let body = self.block_stmts();
                let end = self.toks[self.pos - 1].span.end;
                (LambdaBody::Block(body), Span::new(open.start, end))
            } else {
                let body = self.expr()?;
                let body_span = body.span();
                (LambdaBody::Expr(Box::new(body)), body_span)
            };
            let lambda = Lambda {
                take_names: Vec::new(),
                params: Vec::new(),
                body,
                span: body_span,
                meta: LambdaMeta::default(),
            };
            Ok(Expr::MethodCall {
                receiver: Box::new(Expr::Ident(
                    Syntax::INTERNAL_TASK_RECEIVER.to_string(),
                    task_span,
                )),
                method: "spawn".to_string(),
                method_span: task_span,
                owner_type_args: Vec::new(),
                type_args: Vec::new(),
                args: vec![CallArg {
                    convention: AccessConvention::Read,
                    expr: Expr::Lambda(lambda),
                    span: body_span,
                    flags: crate::AST::CallArgFlags::default(),
                    label: None,
                    spread: false,
                }],
                recv_type: None,
                resolved_ret: None,
                checked_widen: false,
            })
        }
    
        pub(super) fn str_expr_from_parts(
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
                            pub_file_default: false,
                            in_layout_body: self.in_layout_body,
                            adjacent_if_body_depth: 0,
                            block_depth: 0,
                            callable_tail_block_depth: None,
                            module_arg_expr_depth: None,
                            allow_lowercase_leading_dot: self.allow_lowercase_leading_dot,
                            policy_declarations: Vec::new(),
                            applied_rules: Vec::new(),
                            rule_facts: Vec::new(),
                            block_spans: Vec::new(),
                        };
                        // D-FMT-INTERP2=A: trailing `=` prints expression source, then " = ", then the value.
                        // Reject empty `{=}` before attempting to parse an expression.
                        if matches!(sub.peek().kind, TokKind::Eq) {
                            return Err(Diagnostic::error(
                                "E0003",
                                "empty debug-label interpolation `{=}`".to_string(),
                                "`{expr=}` needs an expression before `=`".to_string(),
                                "write `{count=}` or `{x + 1=}`".to_string(),
                                Some(sub.peek().span),
                            ));
                        }
                        let e = sub.expr()?;
                        if !sub.diags.is_empty() {
                            let mut ds = sub.diags;
                            let first = ds.remove(0);
                            self.diags.extend(ds);
                            return Err(first);
                        }
                        let mut debug_label: Option<String> = None;
                        if matches!(sub.peek().kind, TokKind::Eq) {
                            let label_end = sub.pos;
                            let mut label = String::new();
                            for tok in &toks[..label_end] {
                                match &tok.kind {
                                    TokKind::Ident(name) => label.push_str(name),
                                    TokKind::Int(n, _) => label.push_str(&n.to_string()),
                                    TokKind::Float(n) => label.push_str(&n.to_string()),
                                    TokKind::Dot => label.push('.'),
                                    TokKind::LParen => label.push('('),
                                    TokKind::RParen => label.push(')'),
                                    TokKind::LBracket => label.push('['),
                                    TokKind::RBracket => label.push(']'),
                                    TokKind::Plus => label.push_str(" + "),
                                    TokKind::Minus => label.push_str(" - "),
                                    TokKind::Star => label.push_str(" * "),
                                    TokKind::Slash => label.push_str(" / "),
                                    TokKind::Percent => label.push_str(" % "),
                                    other => {
                                        return Err(Diagnostic::error(
                                            "E0003",
                                            format!(
                                                "`{{…=}}` debug labels need a simple expression, not {}",
                                                describe(other)
                                            ),
                                            "the label reprints the expression text before `=`".to_string(),
                                            "use a name or a short expression such as `{count=}`".to_string(),
                                            Some(tok.span),
                                        ));
                                    }
                                }
                            }
                            if label.is_empty() {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    "empty debug-label interpolation".to_string(),
                                    "`{expr=}` needs an expression before `=`".to_string(),
                                    "write `{count=}`".to_string(),
                                    Some(sub.peek().span),
                                ));
                            }
                            sub.bump(); // consume `=`
                            debug_label = Some(label);
                        }
                        let mut format = crate::AST::StrFormat::Display;
                        let selector_rail = match sub.peek().kind {
                            TokKind::Colon => {
                                sub.bump();
                                Some(crate::Syntax::INTERPOLATION_SELECTOR_RAIL)
                            }
                            TokKind::Hash => {
                                // D-ONCE-RETIRE1=C: pure renames remain parseable
                                // only as input to the shared fmt/fix rewrite.
                                sub.bump();
                                Some(crate::Syntax::RETIRED_INTERPOLATION_SELECTOR_RAIL)
                            }
                            _ => None,
                        };
                        if let Some(_selector_rail) = selector_rail {
                            let (sel, sel_span) = sub.expect_ident("after `:` in interpolation")?;
                            if let Some(selector) = crate::Syntax::interpolation_selector(&sel) {
                                let selector_head = format!(
                                    "{}{}",
                                    crate::Syntax::INTERPOLATION_SELECTOR_RAIL,
                                    selector.name
                                );
                                match selector.kind {
                                    crate::Syntax::InterpolationSelectorKind::Debug => {
                                        format = crate::AST::StrFormat::Debug;
                                    }
                                    crate::Syntax::InterpolationSelectorKind::Fixed => {
                                        let after_selector =
                                            format!("after `{selector_head}` in interpolation");
                                        sub.expect(
                                            TokKind::LParen,
                                            &after_selector,
                                        )?;
                                        let precision = match sub.bump() {
                                            crate::Lexer::Token {
                                                kind: TokKind::Int(value, _),
                                                ..
                                            } => value,
                                            token => {
                                                return Err(Diagnostic::error(
                                                    "E0003",
                                                    format!(
                                                        "expected decimal places inside `{selector_head}( )`"
                                                    ),
                                                    format!(
                                                        "`{selector_head}(n)` takes one nonnegative integer literal"
                                                    ),
                                                    format!(
                                                        "write a precision such as `{selector_head}(2)`"
                                                    ),
                                                    Some(token.span),
                                                ));
                                            }
                                        };
                                        let after_precision = format!(
                                            "after the decimal places in `{selector_head}(n)`"
                                        );
                                        sub.expect(
                                            TokKind::RParen,
                                            &after_precision,
                                        )?;
                                        format = crate::AST::StrFormat::Fixed(precision);
                                    }
                                    crate::Syntax::InterpolationSelectorKind::Unit => {
                                        let after_selector =
                                            format!("after `{selector_head}` in interpolation");
                                        sub.expect(
                                            TokKind::LParen,
                                            &after_selector,
                                        )?;
                                        let (style, style_span) =
                                            sub.expect_ident(&format!("inside `{selector_head}( )`"))?;
                                        let crate::Syntax::InterpolationSelectorArguments::UnitStyle(
                                            styles,
                                        ) = selector.arguments
                                        else {
                                            unreachable!("the Unit selector declares unit styles")
                                        };
                                        let style_index = styles
                                            .iter()
                                            .position(|candidate| *candidate == style.as_str());
                                        format = match style_index {
                                            Some(0) => crate::AST::StrFormat::Unit(
                                                crate::AST::UnitFormat::Name,
                                            ),
                                            Some(1) => crate::AST::StrFormat::Unit(
                                                crate::AST::UnitFormat::Bare,
                                            ),
                                            _ => {
                                                let accepted = styles
                                                    .iter()
                                                    .map(|style| format!("`{style}`"))
                                                    .collect::<Vec<_>>()
                                                    .join(" or ");
                                                return Err(Diagnostic::error(
                                                    "E0003",
                                                    format!("unknown unit style `{style}`"),
                                                    format!("`{selector_head}` accepts {accepted}"),
                                                    format!(
                                                        "write {}",
                                                        styles
                                                            .iter()
                                                            .map(|style| {
                                                                format!(
                                                                    "`{selector_head}({style})`"
                                                                )
                                                            })
                                                            .collect::<Vec<_>>()
                                                            .join(" or ")
                                                    ),
                                                    Some(style_span),
                                                ));
                                            }
                                        };
                                        let after_style =
                                            format!("after the style in `{selector_head}( )`");
                                        sub.expect(
                                            TokKind::RParen,
                                            &after_style,
                                        )?;
                                    }
                                }
                            } else {
                                self.diags.push(crate::Generics::e0914(&sel, sel_span));
                            }
                        }
                        if !matches!(sub.peek().kind, TokKind::Eof) {
                            let unit_selector = crate::Syntax::interpolation_selector_for_kind(
                                crate::Syntax::InterpolationSelectorKind::Unit,
                            )
                            .name;
                            return Err(Diagnostic::error(
                                "E0003",
                                format!(
                                    "unexpected {} inside this interpolated `{{ }}`",
                                    describe(&sub.peek().kind)
                                ),
                                "the braces hold exactly one value, optional trailing `=`, and one optional format selector"
                                    .to_string(),
                                format!(
                                    "keep one value per `{{ }}`, for example \"{{a}}\", \"{{a=}}\", or \"{{a:{unit_selector}(bare)}}\""
                                ),
                                Some(sub.peek().span),
                            ));
                        }
                        if let Some(label) = debug_label {
                            out.push(StrPart::Lit(label));
                            out.push(StrPart::Lit(" = ".to_string()));
                        }
                        out.push(StrPart::Interp(Box::new(e), format));
                    }
                }
            }
            Ok(Expr::Str(out, span))
        }
    
}
