use super::super::{Call, CallArg, Diagnostic, Parser, Span, TokKind};

impl<'a> Parser<'a> {
        pub(super) fn call_after_name(&mut self, name: String, name_span: Span) -> Result<Call, Diagnostic> {
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
                args,
                range_checked: false,
            })
        }
    
        pub(in crate::Parser) fn call_arg(&mut self) -> Result<CallArg, Diagnostic> {
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
