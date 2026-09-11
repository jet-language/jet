use super::super::{
    AccessConvention, Call, CallArg, Diagnostic, Expr, Parser, Span, StrPart, TokKind, Token, Type,
};

impl<'a> Parser<'a> {
    pub(super) fn call_after_name(
        &mut self,
        name: String,
        name_span: Span,
        type_args: Vec<Type>,
    ) -> Result<Call, Diagnostic> {
        self.expect(TokKind::LParen, &format!("after `{}` to call it", name))?;
        let mut args = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let compose = self.allow_environment_reads && name == crate::Syntax::SECRET_COMPOSE;
                while compose && matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                args.push(self.call_arg_for_named_call(compose)?);
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                } else if compose && matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                } else {
                    self.expect(TokKind::Comma, "between arguments")?;
                }
            }
        }
        self.expect(TokKind::RParen, "to finish the call")?;
        Ok(Call {
            name,
            name_span,
            type_args,
            args,
            resolved_ret: None,
            range_checked: false,
            widen_approx: false,
        })
    }

    pub(in crate::Parser) fn call_arg(&mut self) -> Result<CallArg, Diagnostic> {
        self.call_arg_with_leading_dot(false, false, false)
    }

    fn call_arg_for_named_call(&mut self, compose: bool) -> Result<CallArg, Diagnostic> {
        self.call_arg_with_leading_dot(false, false, compose)
    }

    /// Marker arguments also accept a leading-dot enum literal for a lowercase
    /// variant, such as `#Kernel(.parallel)`. Effect rows use the same
    /// `-[Effects]>` representation as callable bounds, but stay in the marker
    /// argument carrier until sema validates their slot.
    pub(in crate::Parser) fn marker_call_arg(
        &mut self,
    ) -> Result<(crate::AST::MarkerCallArg, Option<(String, Span)>), Diagnostic> {
        if self.marker_effect_row_starts_here() {
            let (label, label_span) = self.marker_effect_row_label()?;
            let (effects, span) = self.parse_marker_effect_row()?;
            return Ok((
                crate::AST::MarkerCallArg::EffectRow { effects, span },
                label.map(|name| (name, label_span)),
            ));
        }
        let arg = self.call_arg_with_leading_dot(true, true, false)?;
        let label = arg.label.clone();
        Ok((crate::AST::MarkerCallArg::Expr(arg.expr), label))
    }


    fn marker_effect_row_starts_here(&self) -> bool {
        let direct = matches!(self.peek().kind, TokKind::Minus)
            && matches!(self.peek2().kind, TokKind::LBracket);
        let labeled = matches!(
            (
                &self.peek().kind,
                &self.peek2().kind,
                self.toks.get(self.pos + 2).map(|token| &token.kind),
                self.toks.get(self.pos + 3).map(|token| &token.kind),
            ),
            (
                TokKind::Ident(_) | TokKind::KwTag | TokKind::KwUse,
                TokKind::Colon,
                Some(TokKind::Minus),
                Some(TokKind::LBracket),
            )
        );
        direct || labeled
    }

    fn marker_effect_row_label(&mut self) -> Result<(Option<String>, Span), Diagnostic> {
        if !matches!(
            (
                &self.peek().kind,
                &self.peek2().kind,
                self.toks.get(self.pos + 2).map(|token| &token.kind),
                self.toks.get(self.pos + 3).map(|token| &token.kind),
            ),
            (
                TokKind::Ident(_) | TokKind::KwTag | TokKind::KwUse,
                TokKind::Colon,
                Some(TokKind::Minus),
                Some(TokKind::LBracket),
            )
        ) {
            return Ok((None, self.peek().span));
        }
        let token = self.bump();
        let name_span = token.span;
        let name = match token.kind {
            TokKind::Ident(name) => name,
            TokKind::KwTag => "tag".to_string(),
            TokKind::KwUse => crate::Syntax::KW_USE.to_string(),
            _ => unreachable!(),
        };
        self.bump(); // `:`
        Ok((Some(name), name_span))
    }

    fn parse_marker_effect_row(
        &mut self,
    ) -> Result<(Vec<(String, Span)>, Span), Diagnostic> {
        let start = self.peek().span.start;
        self.expect(TokKind::Minus, "to start an effect row")?;
        self.expect(TokKind::LBracket, "to start an effect row")?;
        let mut effects = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBracket) {
            if matches!(self.peek().kind, TokKind::DotDot) {
                self.bump();
                let (name, span) = self.expect_ident("for an open effect-row name")?;
                effects.push((format!("..{name}"), span));
            } else {
                let prohibited = matches!(self.peek().kind, TokKind::Bang);
                if prohibited {
                    self.bump();
                }
                let (name, span) = self.expect_effect_path_name("for an effect name")?;
                let name = if prohibited {
                    format!("!{name}")
                } else {
                    name
                };
                effects.push((name, span));
            }
            if matches!(self.peek().kind, TokKind::RBracket) {
                break;
            }
            self.expect(TokKind::Comma, "between effects in the row")?;
        }
        self.expect(TokKind::RBracket, "to close the effect row")?;
        self.expect(TokKind::Gt, "after the effect row")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok((effects, Span::new(start, end)))
    }

    fn call_arg_with_leading_dot(
        &mut self,
        allow_lowercase_leading_dot: bool,
        allow_marker_labels: bool,
        allow_compose_template: bool,
    ) -> Result<CallArg, Diagnostic> {
        // D-MEM1/S2: an unmarked argument is a plain read at the call site —
        // `parse_access_prefix` already resolves unmarked to `Read` directly.
        let mut convention = self.parse_access_prefix();
        let span = self.peek().span;
        // D-VARIADIC1: `f(...xs)` call spread.
        let spread = if matches!(self.peek().kind, TokKind::DotDotDot) {
            self.bump();
            true
        } else {
            false
        };
        // S61: detect `name: expr` labels — including the registered `use` keyword
        // inside marker arguments — without consuming a plain variable name.
        let label_token = matches!(self.peek().kind, TokKind::Ident(_) | TokKind::KwTag)
            || allow_marker_labels && matches!(self.peek().kind, TokKind::KwUse);
        let label = if label_token && matches!(self.peek2().kind, TokKind::Colon) {
            let lbl_tok = self.bump();
            let lbl_name = match lbl_tok.kind {
                TokKind::Ident(n) => n,
                TokKind::KwTag => "tag".to_string(),
                TokKind::KwUse => crate::Syntax::KW_USE.to_string(),
                _ => unreachable!(),
            };
            self.bump(); // consume `:`
            Some((lbl_name, lbl_tok.span))
        } else {
            None
        };
        // D-MEM1 + D-APILABEL1=A: the capability sigil rides the VALUE, so
        // with a label it lands after the colon — `absorb(payload: ^owned)`.
        // Without this a label-only (`*`) parameter that takes `^` or `&`
        // would be uncallable, since a label is the only way to reach it.
        if label.is_some() {
            let after_label = self.parse_access_prefix();
            if after_label != AccessConvention::Read {
                if convention != AccessConvention::Read {
                    self.diags.push(Diagnostic::error(
                        "E0029",
                        "this argument has two access markers".to_string(),
                        "an argument's access marker is written once, on the value".to_string(),
                        "keep the sigil after the label and remove the other".to_string(),
                        Some(span),
                    ));
                }
                convention = after_label;
            }
        }
        let expr = if allow_compose_template
            && label.as_ref().map(|(name, _)| name.as_str())
                == Some(crate::Syntax::SECRET_COMPOSE_FIELD_TEMPLATE)
            && self.looks_like_unquoted_compose_template()
        {
            self.unquoted_compose_template()
        } else if label.as_ref().map(|(name, _)| name.as_str()) == Some("source")
            && self.looks_like_provider_ref_value()
        {
            self.provider_ref_placeholder()
        } else if allow_lowercase_leading_dot
            && matches!(self.peek().kind, TokKind::Dot)
            && matches!(
                &self.peek2().kind,
                TokKind::Ident(name)
                    if name.chars().next().is_some_and(char::is_lowercase)
            )
        {
            let start = self.bump().span.start;
            let (variant, variant_span) =
                self.expect_ident("after `.` in a leading-dot enum argument")?;
            Expr::EnumLit {
                type_name: String::new(),
                variant,
                variant_span: Some(variant_span),
                args: Vec::new(),
                leading_dot: true,
                span: Span::new(start, variant_span.end),
            }
        } else {
            self.expr()?
        };
        Ok(CallArg {
            convention,
            expr,
            span,
            flags: Default::default(),
            label,
            spread,
        })
    }

    fn looks_like_unquoted_compose_template(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::Ident(_)) {
            return false;
        }
        let mut index = self.pos;
        let mut saw_url_punctuation = false;
        while let Some(token) = self.toks.get(index) {
            match &token.kind {
                TokKind::RParen | TokKind::Comma | TokKind::Semi if index > self.pos => break,
                TokKind::Ident(name)
                    if index > self.pos && name == crate::Syntax::SECRET_COMPOSE_FIELD_FROM =>
                {
                    if matches!(
                        self.toks.get(index + 1).map(|next| &next.kind),
                        Some(TokKind::Colon)
                    ) {
                        break;
                    }
                }
                TokKind::Slash | TokKind::LBrace | TokKind::At => saw_url_punctuation = true,
                TokKind::Str(_) | TokKind::RawStr(_) | TokKind::Eof => break,
                _ => {}
            }
            index += 1;
        }
        saw_url_punctuation
    }

    fn unquoted_compose_template(&mut self) -> Expr {
        let start = self.peek().span.start;
        let mut end = start;
        let mut parts = Vec::new();
        let mut brace_depth = 0usize;
        while let Some(token) = self.toks.get(self.pos) {
            let stop = match &token.kind {
                TokKind::RParen | TokKind::Comma | TokKind::Semi if brace_depth == 0 => true,
                TokKind::Ident(name)
                    if brace_depth == 0
                        && !parts.is_empty()
                        && name == crate::Syntax::SECRET_COMPOSE_FIELD_FROM
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    true
                }
                TokKind::Eof => true,
                _ => false,
            };
            if stop {
                break;
            }
            match token.kind {
                TokKind::LBrace => brace_depth += 1,
                TokKind::RBrace => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
            end = token.span.end;
            parts.push(self.token_spelling(&token));
            self.bump();
        }
        let text = self
            .source
            .as_deref()
            .and_then(|source| source.get(start..end))
            .map(str::to_string)
            .unwrap_or_else(|| parts.concat());
        Expr::Str(vec![StrPart::Lit(text)], Span::new(start, end))
    }

    fn token_spelling(&self, token: &Token) -> String {
        match &token.kind {
            TokKind::Ident(name) => name.clone(),
            TokKind::Int(_, raw) | TokKind::Float(_, raw) => raw.clone(),
            TokKind::UnitNumber { raw, suffix, .. } => format!("{raw}{suffix}"),
            TokKind::LBrace => "{".into(),
            TokKind::RBrace => "}".into(),
            TokKind::Colon => ":".into(),
            TokKind::Slash => "/".into(),
            TokKind::At => "@".into(),
            TokKind::Dot => ".".into(),
            TokKind::Minus => "-".into(),
            TokKind::Plus => "+".into(),
            TokKind::Star => "*".into(),
            TokKind::Percent => "%".into(),
            TokKind::Question => "?".into(),
            TokKind::Eq => "=".into(),
            TokKind::LBracket => "[".into(),
            TokKind::RBracket => "]".into(),
            TokKind::Amp => "&".into(),
            TokKind::Pipe => "|".into(),
            TokKind::Caret => "^".into(),
            _ => String::new(),
        }
    }
}
