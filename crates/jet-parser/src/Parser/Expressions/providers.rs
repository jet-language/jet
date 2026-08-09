use super::super::{
    AccessConvention, Diagnostic, Expr, Parser, Span, Syntax, TokKind,
};

impl<'a> Parser<'a> {
        pub(super) fn looks_like_provider_ref_value(&self) -> bool {
            matches!(self.peek().kind, TokKind::Ident(_)) && matches!(self.peek2().kind, TokKind::At)
        }
    
        pub(super) fn provider_ref_placeholder(&mut self) -> Expr {
            let start = self.peek().span.start;
            let mut end = self.peek().span.end;
            self.bump(); // provider
            if matches!(self.peek().kind, TokKind::At) {
                end = self.bump().span.end;
            }
            while matches!(
                self.peek().kind,
                TokKind::Ident(_)
                    | TokKind::Int(_, _)
                    | TokKind::Dot
                    | TokKind::Slash
                    | TokKind::Minus
                    | TokKind::Star
                    | TokKind::Question
                    | TokKind::Eq
            ) {
                end = self.bump().span.end;
            }
            Expr::Str(
                vec![crate::AST::StrPart::Lit(String::new())],
                Span { start, end },
            )
        }
    
        /// D-MEM1: consume a leading capability sigil `&`/`^` → Write/Move. `~`
        /// (D-SHAPE-COPY1=A copy sigil) has no arm here — a parameter/argument
        /// convention position doesn't take a copy sigil (D-SHAPE-PLACE1, place
        /// precedence at any position, is a separate pending ballot). Returns
        /// `None` when no sigil is present.
        /// Position-disambiguated: infix `^` (power, D-EXPOP1) and `&` (BitAnd) are
        /// parsed inside expressions and never reach the start of a parameter or
        /// argument, or a type. `*` (raw) is D-CAP9, handled apart.
        pub(in crate::Parser) fn parse_capability_sigil(&mut self) -> Option<AccessConvention> {
            let cap = match self.peek().kind {
                TokKind::Amp => AccessConvention::Write,
                TokKind::Caret => AccessConvention::Move,
                _ => return None,
            };
            self.bump();
            Some(cap)
        }
    
        pub(in crate::Parser) fn parse_access_prefix(&mut self) -> AccessConvention {
            // D-MEM1 sigils take precedence over the retired keyword forms.
            if let Some(cap) = self.parse_capability_sigil() {
                return cap;
            }
            if let TokKind::Ident(name) = self.peek().kind.clone() {
                match name.as_str() {
                    Syntax::FOREIGN_READ if false => {
                        let span = self.peek().span;
                        self.bump();
                        self.diags.push(Diagnostic::error(
                            "E0017",
                            format!(
                                "shared access is written with no word in front — not `{}`",
                                Syntax::FOREIGN_READ
                            ),
                            "Jet has exactly one spelling for each thing, so all code reads the same"
                                .to_string(),
                            format!("remove `{}` and write `name: Type`", Syntax::FOREIGN_READ),
                            Some(span),
                        ));
                        return AccessConvention::Read;
                    }
                    Syntax::FOREIGN_WRITE if false => {
                        let span = self.peek().span;
                        self.bump();
                        self.diags.push(Diagnostic::error(
                            "E0018",
                            format!(
                                "the {} is written `{}`, not `{}`",
                                Syntax::WRITE_CAPABILITY_LABEL,
                                Syntax::SIGIL_WRITE,
                                Syntax::FOREIGN_WRITE
                            ),
                            "Jet has exactly one spelling for each thing, so all code reads the same"
                                .to_string(),
                            format!(
                                "replace `{}` with the {} (`{}`)",
                                Syntax::FOREIGN_WRITE,
                                Syntax::WRITE_CAPABILITY_LABEL,
                                Syntax::SIGIL_WRITE
                            ),
                            Some(span),
                        ));
                        return AccessConvention::Write;
                    }
                    _ => {}
                }
            }
            match self.peek().kind {
                // D-S14-PAUSE: `mut` teaching is paused.
                TokKind::KwMutate if false => {
                    let span = self.bump().span;
                    self.push_cap_keyword_teach("E0056", Syntax::KW_MUTATE, Syntax::SIGIL_WRITE, span);
                    AccessConvention::Write
                }
                TokKind::KwMove if false => {
                    // `take(names) () =>` is a lambda take-prefix, not an arg convention.
                    // Only treat bare `take name` as the retired move keyword.
                    let is_lambda_take = matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::LParen)
                    );
                    if is_lambda_take {
                        // `take(names)` lambda prefix is not a capability marker — the value
                        // here is unmarked, so it's `Read` (D-MEM1/S2).
                        AccessConvention::Read
                    } else {
                        // D-S14-PAUSE: bare `take` teaching is paused.
                        let span = self.bump().span;
                        self.push_cap_keyword_teach("E0057", Syntax::KW_MOVE, Syntax::SIGIL_MOVE, span);
                        AccessConvention::Move
                    }
                }
                // D-MEM1/S2 ("signatures can't lie"): an unmarked parameter/argument
                // is `Read`, decided here at parse time — no body-usage inference.
                _ => AccessConvention::Read,
            }
        }
    
        /// D-S14-PAUSE keeps retired capability-word teaching disabled. This helper
        /// remains for any future explicit teaching mode.
        pub(super) fn push_cap_keyword_teach(
            &mut self,
            code: &'static str,
            keyword: &str,
            sigil: &str,
            span: Span,
        ) {
            self.diags.push(Diagnostic::error(
                code,
                format!("`{}` is now {} (`{}`)", keyword, Syntax::capability_label(sigil), sigil),
                "capability is written with a sigil on the type, not a word — so all \
                 code reads the same"
                    .to_string(),
                format!("write {} (`{}`) in place of `{}`", Syntax::capability_label(sigil), sigil, keyword),
                Some(span),
            ));
        }
    
        pub(super) fn starts_expr(&self, kind: &TokKind) -> bool {
            matches!(
                kind,
                TokKind::Ident(_)
                    | TokKind::Int(_, _)
                    | TokKind::Float(_)
                    | TokKind::Str(_)
                    | TokKind::KwTrue
                    | TokKind::KwFalse
                    | TokKind::KwNull
                    | TokKind::KwIt
                    | TokKind::LParen
                    | TokKind::Minus
                    | TokKind::Bang
                    | TokKind::Tilde
                    | TokKind::KwCopy
            )
        }
    
        pub(super) fn foreign_logic_error(&mut self, foreign: &str, canonical: &str) {
            self.diags.push(Diagnostic::error(
                "E0012",
                format!(
                    "{} writes \"{}\" as `{}`",
                    Syntax::LANG_NAME,
                    foreign,
                    canonical
                ),
                format!(
                    "logic uses the symbols `{}`, `{}`, and `{}`",
                    Syntax::OP_AND,
                    Syntax::OP_OR,
                    Syntax::OP_NOT
                ),
                format!("replace `{}` with `{}`", foreign, canonical),
                Some(self.peek().span),
            ));
    }
}
