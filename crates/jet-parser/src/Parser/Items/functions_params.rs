use super::super::{
    AccessConvention, Diagnostic, EnumDef, Func, MetaAttr, Param, Parser, Span, StructDef, Syntax,
    TokKind, Type,
};
use crate::AST::ParamZone;

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
            let params = self.parse_param_list()?;
            self.validate_variadic_params(&params);
            self.validate_param_labels(&params);
            if external_type.is_some() {
                self.reject_root_method_params(&params);
            }
    
            // D-ARROW-CONTROL1=A: an optional `=[Net, DB]=>` callable effect
            // arrow. Effect names are validated in sema. D-EFF2 keeps `via f`
            // in the same row.
            let (declared_effects, effect_via) = self.parse_opt_func_effects()?;
            let decorated_arrow = declared_effects.is_some() || effect_via.is_some();
            let is_pure = is_pure
                || declared_effects.as_ref().is_some_and(|effects| effects.is_empty());
    
            let mut return_type = None;
            let mut return_type_span = None;
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
                    let (ty, span) = self.return_type()?;
                    return_type = Some(ty);
                    return_type_span = Some(span);
                }
            }
            let declared_return_view_provenance =
                self.parse_opt_declared_view_from(&params);
    
            // Single-expression body: `fn name(...) => T = expr;`
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
                    return_view_provenance: None,
                    declared_return_view_provenance,
            gc_return: false,
            gc_scope: false,
                    is_unsafe,
                    unsafe_reason,
                    unsafe_span,
                    is_pure,
                    is_sanitizer,
                    scrub_tag: None,
                    is_reactive,
                    reactive_upgrades: Vec::new(),
                    is_replayable,
                    replayable_span,
                    is_task: false,
                    task_span: None,
                    every: None,
                    task_metadata: None,
                    declared_effects,
                    effect_via,
                    state_requires,
                    state_transition,
                    web_marker,
                    is_must_use,
                    must_use_span,
                    maturity,
                    maturity_span,
                    kernel: None,
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
            let previous_tail_depth = self.callable_tail_block_depth;
            if return_type.is_some() {
                self.callable_tail_block_depth = Some(self.block_depth + 1);
            }
            let body = self.block_stmts();
            self.callable_tail_block_depth = previous_tail_depth;
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
                return_view_provenance: None,
                declared_return_view_provenance,
            gc_return: false,
            gc_scope: false,
                is_unsafe,
                unsafe_reason,
                unsafe_span,
                is_pure,
                is_sanitizer,
                scrub_tag: None,
                is_reactive,
                reactive_upgrades: Vec::new(),
                is_replayable,
                replayable_span,
                is_task: false,
                task_span: None,
                every: None,
                task_metadata: None,
                declared_effects,
                effect_via,
                state_requires,
                state_transition,
                web_marker,
                is_must_use,
                must_use_span,
                maturity,
                maturity_span,
                kernel: None,
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
    
        /// D-EFF1 / D-SHAPE8 / D-ARROW-CONTROL1: parse an optional
        /// `=[Net, DB]=>` effect bound.
        /// Returns `None` when the cursor is not at the decorated arrow. D-EFFTREE1: an entry may be a
        /// dotted effect path (`FS.Read`); sema validates the root against the
        /// known effect vocabulary.
        pub(super) fn parse_opt_effect_annotation(&mut self) -> Result<Option<Vec<(String, Span)>>, Diagnostic> {
            // Trait methods (and any caller that can't host a `#(via f)` pass-through)
            // route through here: a `via` clause is parsed and discarded as a list,
            // so it surfaces as an unknown-effect E0119 in sema rather than silently
            // working. The two `Func` sites use `parse_opt_func_effects` instead.
            Ok(self.parse_opt_func_effects()?.0)
        }
    
        /// D-EFF1 / D-EFF2 / D-SHAPE8 / D-ARROW-CONTROL1: parse the decorated
        /// effect arrow, either a declared bound (`=[Net, DB]=>`) or an
        /// `=[via f]=>` pass-through. Returns
        /// `(declared_effects, effect_via)` — at most one is `Some`. `None`/`None` when
        /// the cursor is not at `=[`.
        pub(super) fn parse_opt_func_effects(
            &mut self,
        ) -> Result<(Option<Vec<(String, Span)>>, Option<(String, Span)>), Diagnostic> {
            let canonical = matches!(self.peek().kind, TokKind::Eq)
                && matches!(self.peek2().kind, TokKind::LBracket);
            let retired_hash = matches!(self.peek().kind, TokKind::Hash)
                && matches!(self.peek2().kind, TokKind::LParen);
            let retired_ballot = matches!(self.peek().kind, TokKind::Minus)
                && matches!(self.peek2().kind, TokKind::LBracket);
            let retired_double = matches!(self.peek().kind, TokKind::MinusMinus);
            if !canonical
                && !retired_double
                && !retired_hash
                && !retired_ballot
            {
                return Ok((None, None));
            }
            let start = self.peek().span;
            let (open, close, close_arrow) = if canonical {
                self.bump(); // `=`
                (TokKind::LBracket, TokKind::RBracket, TokKind::LambdaArrow)
            } else if retired_hash {
                self.bump();
                self.diags.push(Self::retired_effect_syntax(start));
                (TokKind::LParen, TokKind::RParen, TokKind::Arrow)
            } else if retired_ballot {
                self.bump();
                self.diags.push(Self::retired_effect_syntax(start));
                (TokKind::LBracket, TokKind::RBracket, TokKind::Arrow)
            } else {
                self.bump(); // `--`
                self.diags.push(Self::retired_effect_syntax(start));
                (TokKind::LBracket, TokKind::RBracket, TokKind::Arrow)
            };
            self.expect(open, "to start an effect row")?;
            // D-EFF2 `=[via f]=>`: tight pass-through publishing param `f`'s effects.
            if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_VIA) {
                self.bump(); // `via`
                let (param, span) = self.expect_ident("for the callback parameter name after `via`")?;
                self.expect(close.clone(), "to close the effect row")?;
                if retired_hash {
                    if matches!(self.peek().kind, TokKind::Arrow) {
                        self.bump();
                    }
                } else {
                    self.expect(close_arrow.clone(), "after the effect row")?;
                }
                return Ok((None, Some((param, span))));
            }
            let mut effects = Vec::new();
            if self.peek().kind != close {
                loop {
                    if matches!(self.peek().kind, TokKind::DotDot) {
                        self.bump();
                        let (name, span) = self.expect_ident("for an open effect-row name")?;
                        effects.push((format!("..{name}"), span));
                        if self.peek().kind == close {
                            break;
                        }
                        self.expect(TokKind::Comma, "between effects in the row")?;
                        continue;
                    }
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
                    if self.peek().kind == close {
                        break;
                    }
                    self.expect(TokKind::Comma, "between effects in the list")?;
                }
            }
            self.expect(close, "to close the effect row")?;
            if retired_hash {
                if matches!(self.peek().kind, TokKind::Arrow) {
                    self.bump();
                }
            } else {
                self.expect(close_arrow, "after the effect row")?;
            }
            Ok((Some(effects), None))
        }

        pub(in crate::Parser) fn retired_effect_syntax(span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0066",
                "this function uses the retired effect-arrow spelling".to_string(),
                "callable results use `=>`, and an explicit effect ceiling belongs inside that callable arrow".to_string(),
                "write `=[Effects]=>`; use `=[]=>` for an explicit purity bound".to_string(),
                Some(span),
            )
        }

        pub(in crate::Parser) fn retired_callable_arrow(span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0070",
                "this callable result uses `->`".to_string(),
                "`=>` defines callable results; `->` is reserved for selected or yielded control values".to_string(),
                "replace `->` with `=>`".to_string(),
                Some(span),
            )
        }
    
        /// D-APILABEL1=A: parse `(` … `)` parameters including the two zone
        /// separators. `/` closes the positional-only zone — every parameter
        /// written before it forbids a label. `*` opens the label-only zone —
        /// every parameter after it requires one. Unmarked parameters between
        /// them accept either call form.
        ///
        /// The caller has already consumed the `(`; this consumes through `)`.
        pub(super) fn parse_param_list(&mut self) -> Result<Vec<Param>, Diagnostic> {
            let mut params: Vec<Param> = Vec::new();
            let mut zone = ParamZone::Either;
            let mut slash: Option<Span> = None;
            let mut star: Option<Span> = None;
            if !matches!(self.peek().kind, TokKind::RParen) {
                loop {
                    match self.peek().kind {
                        TokKind::Slash => {
                            let span = self.bump().span;
                            if let Some(first) = slash {
                                self.diags.push(Self::repeated_param_zone(Syntax::PARAM_ZONE_POSITIONAL_ONLY, span, first));
                            } else if let Some(star_span) = star {
                                self.diags.push(Self::zone_out_of_order(span, star_span));
                            } else if params.is_empty() {
                                self.diags.push(Self::empty_param_zone(
                                    Syntax::PARAM_ZONE_POSITIONAL_ONLY,
                                    "a positional-only zone needs at least one parameter before the `/`",
                                    "write the positional-only parameters before `/`, or remove the `/`",
                                    span,
                                ));
                            } else {
                                slash = Some(span);
                                for param in params.iter_mut() {
                                    param.zone = ParamZone::PositionalOnly;
                                }
                            }
                        }
                        TokKind::Star => {
                            let span = self.bump().span;
                            if let Some(first) = star {
                                self.diags.push(Self::repeated_param_zone(Syntax::PARAM_ZONE_LABEL_ONLY, span, first));
                            } else {
                                star = Some(span);
                                zone = ParamZone::LabelOnly;
                            }
                        }
                        _ => {
                            let mut param = self.param()?;
                            param.zone = zone;
                            params.push(param);
                        }
                    }
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    // No trailing comma: a parameter (or a zone separator) has
                    // to follow. D-APILABEL1 changed nothing here, and lambda
                    // and call-argument lists reject one too.
                    self.expect(TokKind::Comma, "between parameters")?;
                }
            }
            self.expect(TokKind::RParen, "to close the parameter list")?;
            if let Some(span) = star {
                if !params.iter().any(|p| p.zone == ParamZone::LabelOnly) {
                    self.diags.push(Self::empty_param_zone(
                        Syntax::PARAM_ZONE_LABEL_ONLY,
                        "a label-only zone needs at least one parameter after the `*`",
                        "write the label-only parameters after `*`, or remove the `*`",
                        span,
                    ));
                }
            }
            Ok(params)
        }

        fn repeated_param_zone(sigil: &str, span: Span, first: Span) -> Diagnostic {
            let _ = first;
            Diagnostic::error(
                "E0763",
                format!("`{sigil}` appears twice in this parameter list"),
                format!(
                    "each parameter list has at most one positional-only `{}` and one label-only `{}`",
                    Syntax::PARAM_ZONE_POSITIONAL_ONLY, Syntax::PARAM_ZONE_LABEL_ONLY
                ),
                format!("remove the extra `{sigil}`"),
                Some(span),
            )
        }

        fn zone_out_of_order(slash: Span, star: Span) -> Diagnostic {
            let _ = star;
            Diagnostic::error(
                "E0763",
                format!(
                    "`{}` comes after `{}` in this parameter list",
                    Syntax::PARAM_ZONE_POSITIONAL_ONLY, Syntax::PARAM_ZONE_LABEL_ONLY
                ),
                "the zones read left to right: positional-only, then either, then label-only"
                    .to_string(),
                format!(
                    "move `{}` before `{}`",
                    Syntax::PARAM_ZONE_POSITIONAL_ONLY, Syntax::PARAM_ZONE_LABEL_ONLY
                ),
                Some(slash),
            )
        }

        fn empty_param_zone(
            sigil: &str,
            why: &str,
            fix: &str,
            span: Span,
        ) -> Diagnostic {
            Diagnostic::error(
                "E0763",
                format!("`{sigil}` marks an empty parameter zone"),
                why.to_string(),
                fix.to_string(),
                Some(span),
            )
        }

        pub(super) fn param(&mut self) -> Result<Param, Diagnostic> {
            let root = if matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(name) if name == Syntax::CONTRACT_ROOT)
            {
                let marker = self.parse_rule_marker()?;
                self.bind_rule_fact(
                    marker.name_span,
                    None,
                    crate::Policy::RuleSite::Parameter,
                );
                true
            } else {
                false
            };
            let mut convention = self.parse_access_prefix();
            let (mut name, mut name_span) = if matches!(self.peek().kind, TokKind::KwSelf) {
                let span = self.bump().span;
                (Syntax::KW_SELF.to_string(), span)
            } else {
                self.expect_ident("for a parameter name")?
            };
            // D-APILABEL1=A: `timeout seconds: Int` — two adjacent identifiers
            // split the public call label from the local parameter name. The
            // first one parsed is the label; the second is what the body reads.
            let public_label = if name != Syntax::KW_SELF
                && matches!(self.peek().kind, TokKind::Ident(_))
            {
                let (local, local_span) = self.expect_ident("for the local parameter name")?;
                let label = (name, name_span);
                name = local;
                name_span = local_span;
                Some(label)
            } else {
                None
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
            // D-MEMPROVENANCE3=A: optional `from src (| src)*` after the parameter type.
            let declared_view_from_names = self.parse_opt_param_view_from_names();
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
                root,
                name,
                name_span,
                public_label,
                // `parse_param_list` assigns the real zone; a parameter parsed
                // outside a zoned list keeps the unmarked default.
                zone: ParamZone::default(),
                ty,
                ty_span,
                default,
                variadic,
                variadic_bound_list,
                declared_view_from_names,
            })
        }
    
        /// D-VARIADIC1: a variadic `...` parameter must be the last one in the list.
        /// D-APILABEL1=A: two parameters may not publish the same call label.
        /// A label binds by name, so a repeat makes the second parameter
        /// unreachable and turns every call into nonsense — the binder would
        /// report the first one twice and the second one missing.
        pub(super) fn validate_param_labels(&mut self, params: &[Param]) {
            for (index, param) in params.iter().enumerate() {
                if param.name == Syntax::KW_SELF {
                    continue;
                }
                let label = param.call_label();
                let clash = params
                    .iter()
                    .take(index)
                    .find(|earlier| earlier.name != Syntax::KW_SELF && earlier.call_label() == label);
                if let Some(earlier) = clash {
                    let _ = earlier;
                    self.diags.push(Diagnostic::error(
                        "E0770",
                        format!("two parameters both publish the label `{label}`"),
                        "a label binds an argument by name, so a repeated one leaves the second parameter with no way to be called"
                            .to_string(),
                        format!("give one of them a different label, as in `{label}_2 {}: …`", param.name),
                        Some(param.call_label_span()),
                    ));
                }
            }
        }

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
                if p.root && i != 0 {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!("`#{}` must mark the first parameter", Syntax::CONTRACT_ROOT),
                        "a reversible dot call has one receiver, and it is always the first value parameter"
                            .to_string(),
                        format!("move `#{}` to the first parameter", Syntax::CONTRACT_ROOT),
                        Some(p.name_span),
                    ));
                }
                if p.root && p.convention != AccessConvention::Read {
                    self.diags.push(Diagnostic::error(
                        "E0103",
                        format!("`#{}` must mark a bare-read parameter", Syntax::CONTRACT_ROOT),
                        "dot-call syntax never hides a write or move capability behind the receiver"
                            .to_string(),
                        format!("remove `&` or `^` from the `#{}` parameter", Syntax::CONTRACT_ROOT),
                        Some(p.name_span),
                    ));
                }
            }
        }

        pub(super) fn reject_root_method_params(&mut self, params: &[Param]) {
            for param in params.iter().filter(|param| param.root) {
                self.diags.push(Diagnostic::error(
                    "E0103",
                    format!("`#{}` is only valid on a top-level function", Syntax::CONTRACT_ROOT),
                    "a method already owns its receiver after the dot; marking another receiver would make dispatch ambiguous".to_string(),
                    format!(
                        "remove `#{}`, or move the function to module scope",
                        Syntax::CONTRACT_ROOT
                    ),
                    Some(param.name_span),
                ));
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
            let mut validate_block = Vec::new();
            let mut validate_span = None;
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                if self.method_starts_here() {
                    methods.push(self.method_in_type()?);
                    continue;
                }
                // D-MARK-STACK1: one field rule is bare; two or more share `#[…]`.
                if self.at_marker_list() || matches!(self.peek().kind, TokKind::Hash) {
                    let field_markers =
                        self.parse_field_markers(crate::Policy::RuleSite::Field)?;
                    let mut f = self.field()?;
                    for marker in &field_markers {
                        self.bind_rule_fact(
                            marker.name_span,
                            Some(f.name_span),
                            crate::Policy::RuleSite::Field,
                        );
                    }
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
                } else if self.at_validate_block() {
                    let (stmts, span) = self.validate_block()?;
                    validate_block = stmts;
                    validate_span = Some(span);
                } else {
                    fields.push(self.field()?);
                    if matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                        self.bump();
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
                auto_derive_default: true,
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
                validate_block,
                validate_span,
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
