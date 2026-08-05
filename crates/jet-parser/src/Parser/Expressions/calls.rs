use super::super::{Call, CallArg, Diagnostic, Expr, Parser, Span, TokKind, Type};

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
                    args.push(self.call_arg()?);
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    self.expect(TokKind::Comma, "between arguments")?;
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
                arg_source_order: None,
            })
        }
    
    pub(in crate::Parser) fn call_arg(&mut self) -> Result<CallArg, Diagnostic> {
        self.call_arg_with_leading_dot(false)
    }

    /// Marker arguments also accept a leading-dot enum literal for a lowercase
    /// variant, such as `#Kernel(.parallel)`. Ordinary expression parsing keeps
    /// the existing uppercase-only leading-dot value grammar.
    pub(in crate::Parser) fn marker_call_arg(&mut self) -> Result<CallArg, Diagnostic> {
        self.call_arg_with_leading_dot(true)
    }

    fn call_arg_with_leading_dot(
        &mut self,
        allow_lowercase_leading_dot: bool,
    ) -> Result<CallArg, Diagnostic> {
        // D-MEM1/S2: an unmarked argument is a plain read at the call site —
        // `parse_access_prefix` already resolves unmarked to `Read` directly.
        let convention = self.parse_access_prefix();
            let span = self.peek().span;
            // D-VARIADIC1: `f(...xs)` call spread.
            let spread = if matches!(self.peek().kind, TokKind::DotDotDot) {
                self.bump();
                true
            } else {
                false
            };
            // S61: detect `name: expr` label at call site — an ident followed by `:` that is
            // NOT `::` (a Rust path). We must not consume it yet if it is just a variable name.
            let label = if matches!(self.peek().kind, TokKind::Ident(_) | TokKind::KwTag)
                && matches!(self.peek2().kind, TokKind::Colon)
            {
                let lbl_tok = self.bump();
                let lbl_name = match lbl_tok.kind {
                    TokKind::Ident(n) => n,
                    TokKind::KwTag => "tag".to_string(),
                    _ => unreachable!(),
                };
                self.bump(); // consume `:`
                Some((lbl_name, lbl_tok.span))
            } else {
                None
            };
            let expr = if label.as_ref().map(|(name, _)| name.as_str()) == Some("source")
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
                    self.expect_ident("after `.` in a marker enum argument")?;
                Expr::EnumLit {
                    type_name: String::new(),
                    variant,
                    args: Vec::new(),
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
}
