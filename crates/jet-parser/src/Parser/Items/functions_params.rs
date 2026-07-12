use super::super::{
    AccessConvention, Diagnostic, EnumDef, Func, MetaAttr, Param, Parser, Span, StructDef, Syntax,
    TokKind, Type,
};

impl<'a> Parser<'a> {
        #[allow(clippy::too_many_arguments)]
        pub(super) fn func_after_fn(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
            is_unsafe: bool,
            unsafe_reason: Option<String>,
            unsafe_span: Option<Span>,
            is_pure: bool,
            is_sanitizer: bool,
            meta: Option<MetaAttr>,
            state_requires: Option<(String, Span)>,
            state_transition: Option<crate::AST::StateTransition>,
            is_reactive: bool,
            web_marker: Option<crate::Syntax::WebPartitionMarker>,
            is_must_use: bool,
            must_use_span: Option<Span>,
            maturity: Option<crate::AST::MaturityTag>,
            maturity_span: Option<Span>,
            is_inline: bool,
            is_inline_always: bool,
            inline_span: Option<Span>,
            is_replayable: bool,
            replayable_span: Option<Span>,
        ) -> Result<Func, Diagnostic> {
            let declaration_start = self.toks[self.pos.saturating_sub(1)].span.start;
            let (mut name, mut name_span) = self.expect_ident("after `fn`")?;
            let external_type = if (matches!(self.peek().kind, TokKind::Dot)
                || matches!(self.peek().kind, TokKind::TildeTilde))
                && matches!(self.peek2().kind, TokKind::Ident(_))
                && matches!(self.peek3().kind, TokKind::LParen)
            {
                let type_name = name;
                let type_span = name_span;
                if matches!(self.peek().kind, TokKind::TildeTilde) {
                    let sep_span = self.peek().span;
                    self.diags.push(Diagnostic::error(
                        "E0325",
                        format!(
                            "external methods attach with `{}`, not `{}`",
                            Syntax::EXTERNAL_METHOD_CONNECTOR,
                            Syntax::EXTERNAL_METHOD_CONNECTOR_RETIRED
                        ),
                        "the dot connector matches trait impls and ordinary member access".to_string(),
                        format!("write `fn {}.method(...)`", type_name),
                        Some(sep_span),
                    ));
                }
                self.bump(); // `.` or retired `~~`
                let (method_name, method_span) =
                    self.expect_ident("after the connector in an external method definition")?;
                name = method_name;
                name_span = method_span;
                Some((type_name, type_span))
            } else {
                None
            };
            let type_params = self.parse_opt_type_params()?;
            self.expect(TokKind::LParen, "after the function name")?;
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
    
            // D-EFF1 / D-QUAL1: an optional `#(Net, Db)` effect bound, between the
            // parameter list and the return arrow. Effect names are validated in
            // sema, not here. D-EFF2: the same slot also admits a `#(via f)` tight
            // pass-through.
            let (declared_effects, effect_via) = self.parse_opt_func_effects()?;
    
            let mut return_type = None;
            let mut return_type_span = None;
            if matches!(self.peek().kind, TokKind::Arrow) {
                self.bump();
                let (ty, span) = self.return_type()?;
                return_type = Some(ty);
                return_type_span = Some(span);
            }
    
            // Single-expression body: `fn name(...) -> T = expr;`
            // Desugars to `return expr;` so the "path must return" check is satisfied.
            if matches!(self.peek().kind, TokKind::Eq) {
                let start = self.bump().span.start;
                let expr = self.expr_no_struct_lit()?;
                let expr_end = expr.span().end;
                self.expect(TokKind::Semi, "after the single-expression function body")?;
                let end = if self.pos > 0 {
                    self.toks[self.pos - 1].span.end
                } else {
                    expr_end
                };
                let ret_span = Span::new(start, end);
                let body = vec![crate::AST::Stmt::Return(Some(expr), ret_span)];
                return Ok(Func {
                    span: Span::new(declaration_start, end),
                    is_pub,
                    is_package_pub,
                    external_type,
                    name,
                    name_span,
                    meta,
                    type_params,
                    params,
                    return_type,
                    return_type_span,
                    is_unsafe,
                    unsafe_reason,
                    unsafe_span,
                    is_pure,
                    is_sanitizer,
                    is_reactive,
                    is_replayable,
                    replayable_span,
                    is_task: false,
                    task_span: None,
                    every: None,
                    declared_effects,
                    effect_via,
                    state_requires,
                    state_transition,
                    web_marker,
                    is_must_use,
                    must_use_span,
                    maturity,
                    maturity_span,
                    is_inline,
                    is_inline_always,
                    inline_span,
                    pre: Vec::new(),
                    post: Vec::new(),
                    inline_foreign: None,
                    body,
                });
            }
            self.expect(TokKind::LBrace, "to open the function body")?;
            let body = self.block_stmts();
            let declaration_end = self.toks[self.pos.saturating_sub(1)].span.end;
            Ok(Func {
                span: Span::new(declaration_start, declaration_end),
                is_pub,
                is_package_pub,
                external_type,
                name,
                name_span,
                meta,
                type_params,
                params,
                return_type,
                return_type_span,
                is_unsafe,
                unsafe_reason,
                unsafe_span,
                is_pure,
                is_sanitizer,
                is_reactive,
                is_replayable,
                replayable_span,
                is_task: false,
                task_span: None,
                every: None,
                declared_effects,
                effect_via,
                state_requires,
                state_transition,
                web_marker,
                is_must_use,
                must_use_span,
                maturity,
                maturity_span,
                is_inline,
                is_inline_always,
                inline_span,
                pre: Vec::new(),
                post: Vec::new(),
                inline_foreign: None,
                body,
            })
        }
    
        #[allow(clippy::too_many_arguments)]
        pub(super) fn bump_then_func_after_fn(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
            is_unsafe: bool,
            is_pure: bool,
            is_sanitizer: bool,
            meta: Option<MetaAttr>,
            state_requires: Option<(String, Span)>,
            state_transition: Option<crate::AST::StateTransition>,
            web_marker: Option<crate::Syntax::WebPartitionMarker>,
            is_must_use: bool,
            must_use_span: Option<Span>,
            maturity: Option<crate::AST::MaturityTag>,
            maturity_span: Option<Span>,
        ) -> Result<Func, Diagnostic> {
            self.expect_kw(TokKind::KwFn, "to start a function definition")?;
            self.func_after_fn(
                is_pub,
                is_package_pub,
                is_unsafe,
                None,
                None,
                is_pure,
                is_sanitizer,
                meta,
                state_requires,
                state_transition,
                false,
                web_marker,
                is_must_use,
                must_use_span,
                maturity,
                maturity_span,
                false,
                false,
                None,
                false,
                None,
            )
        }
    
        /// D-EFF1 / D-QUAL1: parse an optional `#(Net, Db)` effect bound. Returns
        /// `None` when the cursor is not at `#(`. D-EFFTREE1: an entry may be a
        /// dotted effect path (`Fs.Read`); sema validates the root against the
        /// known effect vocabulary.
        pub(super) fn parse_opt_effect_annotation(&mut self) -> Result<Option<Vec<(String, Span)>>, Diagnostic> {
            // Trait methods (and any caller that can't host a `#(via f)` pass-through)
            // route through here: a `via` clause is parsed and discarded as a list,
            // so it surfaces as an unknown-effect E0119 in sema rather than silently
            // working. The two `Func` sites use `parse_opt_func_effects` instead.
            Ok(self.parse_opt_func_effects()?.0)
        }
    
        /// D-EFF1 / D-EFF2: parse the `#(…)` signature annotation, which is either a
        /// declared effect bound (`#(Net, Db)`) or a `#(via f)` pass-through. Returns
        /// `(declared_effects, effect_via)` — at most one is `Some`. `None`/`None` when
        /// the cursor is not at `#(`.
        pub(super) fn parse_opt_func_effects(
            &mut self,
        ) -> Result<(Option<Vec<(String, Span)>>, Option<(String, Span)>), Diagnostic> {
            if !(matches!(self.peek().kind, TokKind::Hash)
                && matches!(self.peek2().kind, TokKind::LParen))
            {
                return Ok((None, None));
            }
            self.bump(); // `#`
            self.expect(TokKind::LParen, "after `#` to start an effect list")?;
            // D-EFF2 `#(via f)`: a tight pass-through publishing param `f`'s effects.
            if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_VIA) {
                self.bump(); // `via`
                let (param, span) = self.expect_ident("for the callback parameter name after `via`")?;
                self.expect(TokKind::RParen, "to close the `#(via …)` annotation")?;
                return Ok((None, Some((param, span))));
            }
            let mut effects = Vec::new();
            if !matches!(self.peek().kind, TokKind::RParen) {
                loop {
                    // D-PROP2=A: `!Effect` is a prohibition — the function (and its
                    // whole reachable call graph) must not use that effect.
                    let prohibited = matches!(self.peek().kind, TokKind::Bang);
                    if prohibited {
                        self.bump(); // consume `!`
                    }
                    let (name, span) = self.expect_effect_path_name("for an effect name")?;
                    let entry = if prohibited {
                        format!("!{}", name)
                    } else {
                        name
                    };
                    effects.push((entry, span));
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    self.expect(TokKind::Comma, "between effects in the list")?;
                }
            }
            self.expect(TokKind::RParen, "to close the effect list")?;
            Ok((Some(effects), None))
        }
    
        pub(super) fn param(&mut self) -> Result<Param, Diagnostic> {
            let mut convention = self.parse_access_prefix();
            let (name, name_span) = if matches!(self.peek().kind, TokKind::KwSelf) {
                let span = self.bump().span;
                (Syntax::KW_SELF.to_string(), span)
            } else {
                self.expect_ident("for a parameter name")?
            };
            let (ty, ty_span, variadic, variadic_bound_list) =
                if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    // D-MEM1: the capability sigil rides the type side — `name: &T`/`^T`.
                    // (Receivers carry it on `self` instead: `&self`, parsed above.)
                    if let Some(type_cap) = self.parse_capability_sigil() {
                        // A real pre-name marker (not the unmarked `Read` default) plus a
                        // type-side sigil is two markers (E0029).
                        if convention != AccessConvention::Read {
                            self.diags.push(Diagnostic::error(
                                "E0029",
                                format!("`{}` has two capability markers", name),
                                "a parameter's access capability is written once — on the type \
                             (`name: &Type`), or on `self` for a receiver"
                                    .to_string(),
                                "keep the sigil on the type and remove the other".to_string(),
                                Some(name_span),
                            ));
                        }
                        convention = type_cap;
                    }
                    // D-VARIADIC1: `name: ...T` — variadic rest parameter (last position only).
                    if matches!(self.peek().kind, TokKind::DotDotDot) {
                        self.bump();
                        // D-ANY-JAI1/D-VARARGBOUND1: `...[TraitA, TraitB]` — an explicit
                        // trait-bound list. `[` never starts a legal *concrete* variadic
                        // element type here (list `[T]` and map `[K: V]` types don't make
                        // sense as a rest-parameter's own element type spelled this way),
                        // so this position always means a bound list. Bare `...Trait` /
                        // `...T` both go through `type_()` unchanged — sema tells a
                        // trait name from a concrete type the same way `resolve_type_name`
                        // already does elsewhere.
                        if matches!(self.peek().kind, TokKind::LBracket) {
                            let (bounds, bracket_span) = self.parse_bracket_trait_bound_list()?;
                            (Type::Named(String::new()), bracket_span, true, Some(bounds))
                        } else {
                            let (t, ts) = self.type_()?;
                            (t, ts, true, None)
                        }
                    } else {
                        let (t, ts) = self.type_()?;
                        (t, ts, false, None)
                    }
                } else if name == Syntax::KW_SELF {
                    // S27: receiver type is the owning struct/enum; sema fills it in.
                    (Type::Named(String::new()), name_span, false, None)
                } else {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("expected `:` after the parameter `{}`", name),
                        "every parameter except `self` needs a type after its name".to_string(),
                        format!("write `{}: Type`", name),
                        Some(name_span),
                    ));
                };
            // S61: optional trailing `= expr` default value.
            let default = if matches!(self.peek().kind, TokKind::Eq) {
                self.bump();
                Some(Box::new(self.expr_no_struct_lit()?))
            } else {
                None
            };
            if variadic && default.is_some() {
                self.diags.push(Diagnostic::error(
                    "E1310",
                    format!("variadic parameter `{}` can't have a default value", name),
                    "a `...` rest parameter collects trailing arguments — it can't also be optional"
                        .to_string(),
                    "remove the `= …` default from the variadic parameter".to_string(),
                    Some(name_span),
                ));
            }
            Ok(Param {
                convention,
                name,
                name_span,
                ty,
                ty_span,
                default,
                variadic,
                variadic_bound_list,
            })
        }
    
        /// D-VARIADIC1: a variadic `...` parameter must be the last one in the list.
        pub(super) fn validate_variadic_params(&mut self, params: &[Param]) {
            for (i, p) in params.iter().enumerate() {
                if p.variadic && i + 1 != params.len() {
                    self.diags.push(Diagnostic::error(
                        "E1310",
                        format!("variadic parameter `{}` must be last", p.name),
                        "a `name: ...T` rest parameter collects every trailing argument, so nothing may follow it".to_string(),
                        "move `{}` to the end of the parameter list, or remove the `...`".to_string(),
                        Some(p.name_span),
                    ));
                }
            }
        }
    
        pub(in crate::Parser) fn struct_def(&mut self, nested: bool) -> Result<StructDef, Diagnostic> {
            let (is_pub, is_package_pub) = if nested {
                (false, false)
            } else {
                self.parse_item_visibility()
            };
            let item_start = self.peek().span.start;
            self.expect_kw(TokKind::KwStruct, "to start a struct definition")?;
            let (name, name_span) = self.parse_dotted_type_name("after `struct`")?;
            let type_params = self.parse_opt_type_params()?;
            self.expect(TokKind::LBrace, "to open the struct body")?;
            let mut fields = Vec::new();
            let mut methods = Vec::new();
            let mut trait_impls = Vec::new();
            let mut derives = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                // D-SERDE5: `#[Rename("x")] who: String` — field-level serde markers.
                // D-DEBUG-REDACT/D-MARKERMOVE1: `@[Redact] who: String` — contract
                // plane; stackable with a `#[…]` serde group on the same field.
                if self.at_marker_list() || self.at_contract_marker_list() {
                    let field_markers = self.parse_field_markers()?;
                    let mut f = self.field()?;
                    let mut redact = false;
                    let mut serde_markers = Vec::new();
                    for m in field_markers {
                        if m.name == crate::Syntax::ATTR_REDACT {
                            redact = true;
                        } else {
                            serde_markers.push(m);
                        }
                    }
                    f.serde_markers = serde_markers;
                    f.redact = redact;
                    fields.push(f);
                    if matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                        self.bump();
                    }
                    continue;
                }
                if matches!(self.peek().kind, TokKind::KwDerive) {
                    derives.push(self.derive_line()?);
                } else if matches!(self.peek().kind, TokKind::KwImpl) {
                    trait_impls.push(self.trait_impl_block()?);
                } else {
                    let is_method = matches!(self.peek().kind, TokKind::KwFn)
                        || (matches!(self.peek().kind, TokKind::KwPub)
                            && matches!(self.peek2().kind, TokKind::KwFn))
                        || self.at_pure_fn()
                        || self.at_sanitizer_fn()
                        || self.at_inline_fn()
                        || self.at_state_fn()
                        || self.at_transition_fn();
                    if is_method {
                        methods.push(self.method_in_type()?);
                    } else {
                        fields.push(self.field()?);
                        if matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                            self.bump();
                        }
                    }
                }
            }
            let item_end = self.bump().span.end; // }
            Ok(StructDef {
                span: Span::new(item_start, item_end),
                is_pub,
                is_package_pub,
                name,
                name_span,
                type_params,
                fields,
                methods,
                trait_impls,
                derives,
                is_published_schema: false,
                published_schema_span: None,
                is_single_use: false,
                single_use_span: None,
                is_must_use: false,
                must_use_span: None,
                layout: None,
                layout_span: None,
                serde_markers: Vec::new(),
                type_markers: Vec::new(),
            })
        }
    
        pub(in crate::Parser) fn enum_def(&mut self, nested: bool) -> Result<EnumDef, Diagnostic> {
            let (is_pub, is_package_pub) = if nested {
                (false, false)
            } else {
                self.parse_item_visibility()
            };
            self.enum_def_after_pub(is_pub, is_package_pub)
        }
    
}
