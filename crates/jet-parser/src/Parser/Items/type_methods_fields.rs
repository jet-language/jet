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
            let is_pure = if retired_s14_teaching_enabled()
                && matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::FOREIGN_PURE)
                && self.foreign_pure_follows()
            {
                let t = self.bump();
                self.diags.push(self.foreign_pure_diag(t.span));
                true
            } else {
                false
            };
            let markers = if matches!(self.peek().kind, TokKind::Hash) {
                self.parse_method_marker_sequence()?
            } else {
                Vec::new()
            };
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let (is_pub, is_package_pub) = self.parse_pub_qualifier();
            self.expect_kw(TokKind::KwFn, "to start a method")?;
            let function = self.func_after_fn(
                is_pub,
                is_package_pub,
                false,
                None,
                None,
                is_pure,
                false,
                None,
                None,
                None,
                false,
                None,
                false,
                None,
                None,
                None,
                false,
                false,
                None,
                false,
                None,
            )?;
            self.apply_method_markers(function, markers)
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
    
        /// D-PERSIST1: true at `#Persist` immediately before `const`/`#` (module
        /// top level only — this predicate is never consulted by the statement
        /// parser, so a local binding's `#Persist` falls through to the E0145
        /// teaching diagnostic in `Statements.rs` instead).
        pub(in crate::Parser) fn at_persist_const(&self) -> bool {
            matches!(&self.peek().kind, TokKind::Hash)
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
            // D-PERSIST1: optional `#Persist`.
            let (is_persist, persist_span) = if matches!(&self.peek().kind, TokKind::Hash)
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
            while matches!(self.peek().kind, TokKind::Hash) {
                self.bump();
                let (attr_name, _) = self.expect_ident("after `@`")?;
                match attr_name.as_str() {
                    "Static" => attrs.push(ConstAttr::ForceStatic),
                    "Inline" => attrs.push(ConstAttr::ForceInline),
                    "static" | "inline" => {
                        let replacement = crate::Policy::applied_rule(&attr_name)
                            .and_then(|row| match row.status {
                                crate::Policy::RuleStatus::Retired { replacement } => {
                                    Some(replacement)
                                }
                                crate::Policy::RuleStatus::Active => None,
                            })
                            .unwrap_or("#Static");
                        self.diags.push(Diagnostic::error(
                            "E0927",
                            format!("`#{attr_name}` is retired"),
                            "const markers use the same PascalCase marker plane as other declarations"
                                .to_string(),
                            format!("write `{replacement}`"),
                            Some(self.toks[self.pos.saturating_sub(1)].span),
                        ));
                        attrs.push(if attr_name == "static" {
                            ConstAttr::ForceStatic
                        } else {
                            ConstAttr::ForceInline
                        });
                    }
                    other => {
                        return Err(Diagnostic::error(
                            "E0003",
                            format!("`#{}` isn't a known rule on a const", other),
                            "only `#Static` and `#Inline` are supported on const declarations"
                                .to_string(),
                            "remove the rule or use `#Static` or `#Inline`".to_string(),
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
