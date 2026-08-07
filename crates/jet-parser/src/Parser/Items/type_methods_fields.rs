use super::super::{
    ConstAttr, ConstDef, Diagnostic, Field, Func, Parser, Span, Syntax, TokKind, TraitMethodSig,
};

impl<'a> Parser<'a> {
        pub(super) fn trait_method_sig(&mut self, is_pure: bool) -> Result<TraitMethodSig, Diagnostic> {
            let start = self.peek().span;
            self.expect_kw(TokKind::KwFn, "to start a trait method signature")?;
            let (name, name_span) = self.expect_ident("after `fn`")?;
            self.expect(TokKind::LParen, "after the method name")?;
            let params = self.parse_param_list()?;
            self.validate_variadic_params(&params);
            self.validate_param_labels(&params);
            self.reject_root_method_params(&params);
            // D-EFF3 / D-SHAPE8 / D-ARROW-CONTROL1: optional `=[GPU]=>`
            // effect bound.
            let declared_effects = self.parse_opt_effect_annotation()?;
            let decorated_arrow = declared_effects.is_some();
            let is_pure = is_pure
                || declared_effects.as_ref().is_some_and(|effects| effects.is_empty());
            let mut return_type = None;
            if decorated_arrow
                || matches!(self.peek().kind, TokKind::LambdaArrow | TokKind::Arrow)
            {
                if !decorated_arrow {
                    let arrow = self.bump();
                    if matches!(arrow.kind, TokKind::Arrow) {
                        self.diags.push(Self::retired_callable_arrow(arrow.span));
                    }
                }
                if self.type_starts_here() {
                    let (ty, _) = self.return_type()?;
                    return_type = Some(ty);
                }
            }
            let declared_return_view_provenance =
                self.parse_opt_declared_view_from(&params);
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
                return_view_provenance: crate::AST::ViewProvenanceCell::new(),
                declared_return_view_provenance,
            })
        }
    
        /// S27: method inside a type body or `impl` block.
        pub(super) fn method_in_type(&mut self) -> Result<Func, Diagnostic> {
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
                false,
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
            self.reject_root_method_params(&function.params);
            self.apply_method_markers(function, markers)
        }
    
        pub(super) fn field(&mut self) -> Result<Field, Diagnostic> {
            let (is_pub, is_package_pub) = self.parse_pub_qualifier();
            let (name, name_span) = self.expect_ident("for a field name")?;
            // D-META-STAGE1=B: the compile-time mark rides the name, so `$word`
            // now lexes as one identifier. A field never carries it — the
            // `$`-marked members belong to the compiler (`T.$layout`).
            if Syntax::is_comptime_name(&name) {
                return Err(Diagnostic::error(
                    "E0003",
                    format!("`{name}` is not a field name"),
                    "the compile-time mark `$` belongs to the compiler's own members and to compile-time bindings, never to a declared field"
                        .to_string(),
                    format!("drop the `$`, or read the compiler fact as `T.{name}`"),
                    Some(name_span),
                ));
            }
            self.expect(TokKind::Colon, "after a field name")?;
            let (ty, ty_span) = self.type_()?;
            // D-FIELDPOL1: `name: T => expr` — a computed field. `expr` is a
            // single expression (no block); sibling field names inside it are
            // still bare `Ident`s here — `Sema::CheckerFieldPolicy` rewrites them
            // to `self.<field>` once every field of the struct is known.
            // D-FIELDDEF1=C: `name: T = expr` — absence / construction default.
            let (computed, default) = if matches!(self.peek().kind, TokKind::LambdaArrow) {
                self.bump();
                (Some(Box::new(self.expr()?)), None)
            } else if matches!(self.peek().kind, TokKind::Eq) {
                self.bump();
                (None, Some(Box::new(self.expr()?)))
            } else {
                (None, None)
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
                default,
                default_ct: None,
            })
        }
    
        /// D-PERSIST1: true at `#Persist` (module top level only — this
        /// predicate is never consulted by the statement parser, so a local
        /// binding's `#Persist` falls through to the E0145 teaching diagnostic
        /// in `Statements.rs` instead).
        pub(in crate::Parser) fn at_persist_binding(&self) -> bool {
            matches!(&self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_PERSIST)
        }

        /// D-CONSTMARK1: true at `#Static` / `#Inline` (or retired lowercase)
        /// immediately before a comptime binding.
        pub(in crate::Parser) fn at_comptime_marker(&self) -> bool {
            matches!(&self.peek().kind, TokKind::Hash)
                && matches!(
                    &self.peek2().kind,
                    TokKind::Ident(n)
                        if matches!(n.as_str(), "Static" | "Inline" | "static" | "inline")
                )
        }

        /// D-CONST-RETIRE1 (E0146): `const` is retired — teach `comptime`, then
        /// recover by parsing the rest as a comptime binding.
        pub(in crate::Parser) fn retired_const_def(&mut self) -> Result<ConstDef, Diagnostic> {
            self.comptime_def()
        }

        /// D-PERSIST1: `#Persist name (:: | :=) expr` — module-level bare
        /// binding that survives a `jet dev` hot reload. Not `#Persist comptime`
        /// and not `#Persist const`.
        pub(in crate::Parser) fn persist_def(&mut self) -> Result<ConstDef, Diagnostic> {
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
            let sigil = self.bump(); // `#`
            let name_tok = self.bump(); // `Persist`
            let persist_span = Span::new(sigil.span.start, name_tok.span.end);
            if matches!(self.peek().kind, TokKind::KwConst | TokKind::KwComptime) {
                let bad = self.bump();
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "`#{}` marks a bare binding, not `{}`",
                        Syntax::MARKER_PERSIST,
                        match bad.kind {
                            TokKind::KwConst => Syntax::KW_CONST,
                            _ => Syntax::KW_COMPTIME,
                        }
                    ),
                    format!(
                        "`#{}` attaches to `name :: …` or `name := …` (D-PERSIST1 / D-BIND-BARE1)",
                        Syntax::MARKER_PERSIST
                    ),
                    format!(
                        "write `#{} name := …` (or `::` for an immutable bare bind)",
                        Syntax::MARKER_PERSIST
                    ),
                    Some(Span::new(persist_span.start, bad.span.end)),
                ));
            }
            let (name, name_span) = self.expect_ident("after `#Persist`")?;
            let mutable = match self.peek().kind {
                TokKind::ColonColon => {
                    self.bump();
                    false
                }
                TokKind::ColonEq => {
                    self.bump();
                    true
                }
                _ => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}` or `{}` after the `#{}` name",
                            Syntax::SIGIL_BIND_IMMUT,
                            Syntax::SIGIL_BIND_MUT,
                            Syntax::MARKER_PERSIST
                        ),
                        format!(
                            "`#{}` marks a bare binding (D-PERSIST1 / D-BIND-BARE1)",
                            Syntax::MARKER_PERSIST
                        ),
                        format!(
                            "write `#{} {name} := …` (or `{name} :: …`)",
                            Syntax::MARKER_PERSIST
                        ),
                        Some(self.peek().span),
                    ));
                }
            };
            let value = self.expr()?;
            self.expect(TokKind::Semi, "after a `#Persist` value")?;
            Ok(ConstDef {
                span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                name,
                name_span,
                value,
                meta,
                attrs: Vec::new(),
                rust_kind: crate::AST::RustConstKind::Const,
                is_comptime: false,
                ct: None,
                ty: None,
                is_persist: true,
                persist_span: Some(persist_span),
                mutable,
                resolved_output: None,
            })
        }

        fn parse_comptime_attrs(&mut self) -> Result<Vec<ConstAttr>, Diagnostic> {
            let mut attrs = Vec::new();
            while matches!(self.peek().kind, TokKind::Hash)
                && matches!(
                    &self.peek2().kind,
                    TokKind::Ident(n)
                        if matches!(n.as_str(), "Static" | "Inline" | "static" | "inline")
                )
            {
                self.bump();
                let (attr_name, _) = self.expect_ident("after `#`")?;
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
                            "comptime markers use the same PascalCase marker plane as other declarations"
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
                            format!("`#{}` isn't a known rule on a comptime binding", other),
                            "only `#Static` and `#Inline` are supported on comptime declarations"
                                .to_string(),
                            "remove the rule or use `#Static` or `#Inline`".to_string(),
                            Some(self.peek().span),
                        ));
                    }
                }
            }
            Ok(attrs)
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
                mutable: false,
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
                mutable: false,
                resolved_output: None,
            })
        }
    
        /// S57 (M9.5): `#Known name :: expr;` — a compile-time constant binding.
        /// D-CONSTMARK1: optional `#Static` / `#Inline` precede `comptime`.
        /// D-CONST-RETIRE1: bare/`#Static`/`#Inline` `const` teaches E0146 and recovers.
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
            let attrs = self.parse_comptime_attrs()?;
            let known = self.at_known_lead();
            match self.peek().kind {
                TokKind::KwComptime => {
                    let span = self.bump().span;
                    self.diags.push(Diagnostic::error(
                        "E0374",
                        "`comptime` is retired".to_string(),
                        "Jet folds ordinary foldable expressions automatically; explicit compile-time demand lives on the marker plane"
                            .to_string(),
                        "remove the keyword for ordinary code, or replace it with `#Known` when failure to compute now must stop the build"
                            .to_string(),
                        Some(span),
                    ));
                }
                TokKind::KwConst => {
                    let kw = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0146",
                        format!("`{}` is retired — write `#Known`", Syntax::KW_CONST),
                        "explicit compile-time demand is a marker on an immutable binding"
                            .to_string(),
                        "write `#Known name :: …` (or `#Persist name := …` for hot-reload state)"
                            .to_string(),
                        Some(kw.span),
                    ));
                }
                // D-META-STAGE1=B: `#Known` is retired. Recover it so the rest
                // of the file still parses, and teach the `$` form once.
                TokKind::Hash if known => {
                    let head = self.read_marker_head()?;
                    let fix = match &self.peek().kind {
                        TokKind::Ident(name) => format!("write `${name} :: …`"),
                        _ => "write the mark on the name: `$name :: …`".to_string(),
                    };
                    self.diags.push(self.retired_known_error(head.span, fix));
                }
                // D-META-STAGE1=B: `$name :: expr` — the mark rides the name.
                // Additive new spelling from #1537's checkpoint; kept, not part
                // of the B5 revert (it doesn't hard-error anything in the
                // existing `#Known` corpus).
                TokKind::Ident(ref n) if Syntax::is_comptime_name(n) => {}
                _ => {
                    self.expect_kw(TokKind::KwComptime, "to start a comptime binding")?;
                }
            }
            let marked = matches!(&self.peek().kind, TokKind::Ident(n) if Syntax::is_comptime_name(n));
            let (name, name_span) = self.expect_ident(if known || marked {
                "after `#Known`"
            } else {
                "for the compile-time binding name"
            })?;
            if known || marked {
                self.expect(TokKind::ColonColon, "after the `#Known` name")?;
            } else {
                self.expect(TokKind::Eq, "after the retired comptime name")?;
            }
            let value = self.expr()?;
            self.expect(TokKind::Semi, "after a comptime value")?;
            Ok(ConstDef {
                span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                name,
                name_span,
                value,
                meta,
                attrs,
                rust_kind: crate::AST::RustConstKind::Const,
                is_comptime: true,
                ct: None,
                ty: None,
                is_persist: false,
                persist_span: None,
                mutable: false,
                resolved_output: None,
            })
        }
    
}
