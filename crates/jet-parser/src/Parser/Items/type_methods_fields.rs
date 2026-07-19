use super::super::{
    ConstAttr, ConstDef, Diagnostic, Field, Func, Parser, Span, Syntax, TokKind, TraitMethodSig,
    retired_s14_teaching_enabled,
};

impl<'a> Parser<'a> {
        pub(super) fn trait_method_sig(&mut self, is_pure: bool) -> Result<TraitMethodSig, Diagnostic> {
            let start = self.peek().span;
            self.expect_kw(TokKind::KwFn, "to start a trait method signature")?;
            let (name, name_span) = self.expect_ident("after `fn`")?;
            self.expect(TokKind::LParen, "after the method name")?;
            let mut params = Vec::new();
            if !matches!(self.peek().kind, TokKind::RParen) {
                loop {
                    params.push(self.param()?);
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    self.expect(TokKind::Comma, "between parameters")?;
                }
            }
            self.expect(TokKind::RParen, "to close the parameter list")?;
            self.validate_variadic_params(&params);
            // D-EFF3 / D-SHAPE8: optional `--[Gpu]->` effect bound.
            let declared_effects = self.parse_opt_effect_annotation()?;
            let decorated_arrow = declared_effects.is_some();
            let is_pure = is_pure
                || declared_effects.as_ref().is_some_and(|effects| effects.is_empty());
            let mut return_type = None;
            if decorated_arrow || matches!(self.peek().kind, TokKind::Arrow) {
                if !decorated_arrow {
                    self.bump();
                }
                if self.type_starts_here() {
                    let (ty, _) = self.return_type()?;
                    return_type = Some(ty);
                }
            }
            // D-LIB2: optional default body `{ … }` instead of `;`.
            let default_body = if matches!(self.peek().kind, TokKind::LBrace) {
                self.bump();
                let stmts = self.block_stmts();
                Some(stmts)
            } else {
                let end = self.peek().span.end;
                self.finish_stmt()?;
                let _ = end;
                None
            };
            let end = self.peek().span.end;
            Ok(TraitMethodSig {
                name,
                name_span,
                params,
                return_type,
                span: Span::new(start.start, end),
                default_body,
                is_pure,
                declared_effects,
                return_view_provenance: std::sync::Arc::new(std::sync::OnceLock::new()),
            })
        }
    
        /// S27: method inside a type body or `impl` block.
        pub(super) fn method_in_type(&mut self) -> Result<Func, Diagnostic> {
            let (is_must_use, must_use_span) = if self.at_must_use_fn() {
                (true, Some(self.bump_must_use_marker()?))
            } else {
                (false, None)
            };
            // S60 (D-CASING1 follow-on) / D-MARKERMOVE2: allow `@Pure fn` on methods
            // too; the marker precedes `pub`.
            let is_pure = if self.at_pure_fn() {
                self.bump_pure_marker();
                true
            } else if retired_s14_teaching_enabled()
                && matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::FOREIGN_PURE)
                && self.foreign_pure_follows()
            {
                let t = self.bump();
                self.diags.push(self.foreign_pure_diag(t.span));
                true
            } else {
                false
            };
            // D-TAINT1: `@Sanitizer fn` is valid on methods too.
            let is_sanitizer = if self.at_sanitizer_fn() {
                self.bump(); // `#`
                self.bump(); // `Sanitizer`
                true
            } else {
                false
            };
            let mut is_replayable = false;
            let mut replayable_span = None;
            if self.at_replayable_fn() {
                let start = self.bump().span.start; // `#`
                let end = self.bump().span.end; // `Replayable`
                is_replayable = true;
                replayable_span = Some(Span::new(start, end));
            }
            // D-METHODMACRO1=A: `@Inline`/`@InlineAlways` on methods too.
            let (is_inline, is_inline_always, inline_span) = self.parse_inline_marker()?;
            // D-STATE1: `@State(S)` / `@Transition(From -> To)` typestate markers on
            // methods — the common case (typestate methods carry `self`).
            let mut state_requires = None;
            let mut state_transition = None;
            loop {
                // D-SCHEDULE1 (card #505): a marker on its own line before `fn`
                // gets a lexer-inserted `;` — skip it, matching `func()`'s own
                // stacked-marker loop, so `@Task`/`@Every(…)` (wrong here, but
                // still stacked with `@State`/`@Transition`) don't cascade into
                // a spurious "expected `fn`" parse error after the E0925 push.
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                if state_requires.is_none() && self.at_state_fn() {
                    state_requires = Some(self.parse_state_require_marker()?);
                } else if state_transition.is_none() && self.at_transition_fn() {
                    state_transition = Some(self.parse_transition_marker()?);
                } else if self.at_task_fn() {
                    // D-SCHEDULE1 (card #505): `@Task` only marks a top-level
                    // function (D-JPK-TASKRUN1) — recoverable diagnostic, same
                    // shape as the E0062 plane-teaching pushes elsewhere, then
                    // keep parsing the method normally.
                    let start = self.bump().span.start; // `#`
                    let end = self.bump().span.end; // `Task`
                    self.diags
                        .push(Self::e0925_task_not_toplevel(Span::new(start, end)));
                } else if self.at_every_fn() {
                    let m = self.parse_every_marker()?;
                    self.diags.push(Self::e0925_task_not_toplevel(m.span));
                } else {
                    break;
                }
            }
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let (is_pub, is_package_pub) = self.parse_pub_qualifier();
            self.expect_kw(TokKind::KwFn, "to start a method")?;
            self.func_after_fn(
                is_pub,
                is_package_pub,
                false,
                None,
                None,
                is_pure,
                is_sanitizer,
                None,
                state_requires,
                state_transition,
                false,
                None,
                is_must_use,
                must_use_span,
                None,
                None,
                is_inline,
                is_inline_always,
                inline_span,
                is_replayable,
                replayable_span,
            )
        }
    
        pub(super) fn field(&mut self) -> Result<Field, Diagnostic> {
            let (is_pub, is_package_pub) = self.parse_pub_qualifier();
            let (name, name_span) = self.expect_ident("for a field name")?;
            self.expect(TokKind::Colon, "after a field name")?;
            let (ty, ty_span) = self.type_()?;
            // D-FIELDPOL1: `name: T => expr` — a computed field. `expr` is a
            // single expression (no block); sibling field names inside it are
            // still bare `Ident`s here — `Sema::CheckerFieldPolicy` rewrites them
            // to `self.<field>` once every field of the struct is known.
            let computed = if matches!(self.peek().kind, TokKind::LambdaArrow) {
                self.bump();
                Some(Box::new(self.expr()?))
            } else {
                None
            };
            Ok(Field {
                is_pub,
                is_package_pub,
                name,
                name_span,
                ty,
                ty_span,
                serde_markers: Vec::new(),
                redact: false,
                computed,
            })
        }
    
        /// D-PERSIST1: true at `@Persist` immediately before `const`/`#` (module
        /// top level only — this predicate is never consulted by the statement
        /// parser, so a local binding's `@Persist` falls through to the E0145
        /// teaching diagnostic in `Statements.rs` instead).
        pub(in crate::Parser) fn at_persist_const(&self) -> bool {
            matches!(&self.peek().kind, TokKind::At)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::CONTRACT_PERSIST)
        }
    
        pub(in crate::Parser) fn const_def(&mut self) -> Result<ConstDef, Diagnostic> {
            let item_start = self.peek().span.start;
            let meta = if self.at_meta_attr() {
                let meta = self.parse_meta_attr()?;
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                Some(meta)
            } else {
                None
            };
            // D-PERSIST1: optional `@Persist`.
            let (is_persist, persist_span) = if matches!(&self.peek().kind, TokKind::At)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::CONTRACT_PERSIST)
            {
                let sigil = self.bump(); // `@`
                let name_tok = self.bump(); // `Persist`
                let span = Span::new(sigil.span.start, name_tok.span.end);
                (true, Some(span))
            } else {
                (false, None)
            };
            let mut attrs = Vec::new();
            while matches!(self.peek().kind, TokKind::At) {
                self.bump();
                let (attr_name, _) = self.expect_ident("after `@`")?;
                match attr_name.as_str() {
                    "static" => attrs.push(ConstAttr::ForceStatic),
                    "inline" => attrs.push(ConstAttr::ForceInline),
                    other => {
                        return Err(Diagnostic::error(
                            "E0003",
                            format!("`@{}` isn't a known rule on a const", other),
                            "only `@static` and `@inline` are supported on const declarations"
                                .to_string(),
                            "remove the rule or use `@static` or `@inline`".to_string(),
                            Some(self.peek().span),
                        ));
                    }
                }
            }
            self.expect_kw(TokKind::KwConst, "to start a const declaration")?;
            let (name, name_span) = self.expect_ident("after `const`")?;
            self.expect(TokKind::Eq, "after the const name")?;
            let value = self.expr()?;
            self.expect(TokKind::Semi, "after a const value")?;
            Ok(ConstDef {
                span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                name,
                name_span,
                value,
                meta,
                attrs,
                rust_kind: crate::AST::RustConstKind::Const,
                is_comptime: false,
                ct: None,
                ty: None,
                is_persist,
                persist_span,
                resolved_output: None,
            })
        }

        /// D-SHAPE-OUTPUT-CALLABLE1: an Output is an ordinary typed immutable
        /// top-level value; no wrapper declaration or string symbol path.
        pub(in crate::Parser) fn output_def(&mut self) -> Result<ConstDef, Diagnostic> {
            let start = self.peek().span.start;
            let (name, name_span) = self.expect_ident("for the Output name")?;
            self.expect(TokKind::Colon, "after the Output name")?;
            let (ty, _) = self.type_()?;
            self.expect(TokKind::ColonColon, "after `Output`")?;
            let value = self.expr()?;
            self.expect(TokKind::Semi, "after an Output value")?;
            Ok(ConstDef {
                span: Span::new(start, self.prev_end()),
                name,
                name_span,
                value,
                meta: None,
                attrs: Vec::new(),
                rust_kind: crate::AST::RustConstKind::Const,
                is_comptime: false,
                ct: None,
                ty: Some(ty),
                is_persist: false,
                persist_span: None,
                resolved_output: None,
            })
        }

        /// D-ECO-OUTPUT-DEFAULT1=A: checked Output defaults keep the ratified
        /// `defaults: .{ run: address, ... }` source spelling.
        pub(in crate::Parser) fn output_defaults_def(&mut self) -> Result<ConstDef, Diagnostic> {
            let start = self.peek().span.start;
            let (name, name_span) = self.expect_ident("for Output defaults")?;
            self.expect(TokKind::Colon, "after `defaults`")?;
            let value = self.expr()?;
            self.expect(TokKind::Semi, "after Output defaults")?;
            Ok(ConstDef {
                span: Span::new(start, self.prev_end()),
                name,
                name_span,
                value,
                meta: None,
                attrs: Vec::new(),
                rust_kind: crate::AST::RustConstKind::Const,
                is_comptime: false,
                ct: None,
                ty: Some(crate::AST::Type::Named(Syntax::TYPE_OUTPUT_DEFAULTS.to_string())),
                is_persist: false,
                persist_span: None,
                resolved_output: None,
            })
        }
    
        /// S57 (M9.5): `comptime name = expr;` — a compile-time constant binding.
        pub(in crate::Parser) fn comptime_def(&mut self) -> Result<ConstDef, Diagnostic> {
            let item_start = self.peek().span.start;
            let meta = if self.at_meta_attr() {
                let meta = self.parse_meta_attr()?;
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                Some(meta)
            } else {
                None
            };
            self.expect_kw(TokKind::KwComptime, "to start a comptime binding")?;
            let (name, name_span) = self.expect_ident("after `comptime`")?;
            self.expect(TokKind::Eq, "after the comptime name")?;
            let value = self.expr()?;
            self.expect(TokKind::Semi, "after a comptime value")?;
            Ok(ConstDef {
                span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                name,
                name_span,
                value,
                meta,
                attrs: Vec::new(),
                rust_kind: crate::AST::RustConstKind::Const,
                is_comptime: true,
                ct: None,
                ty: None,
                is_persist: false,
                persist_span: None,
                resolved_output: None,
            })
        }
    
}
