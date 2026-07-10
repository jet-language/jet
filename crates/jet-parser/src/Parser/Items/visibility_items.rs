impl<'a> Parser<'a> {
        /// D-VISDEFAULT2=A: parse one top-level item after `priv` / `private`.
        fn item_after_visibility(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
        ) -> Result<Item, Diagnostic> {
            match self.peek().kind.clone() {
                TokKind::KwStruct => self
                    .struct_def_after_pub_pkg(is_pub, is_package_pub)
                    .map(Item::Struct),
                TokKind::KwEnum => self
                    .enum_def_after_pub(is_pub, is_package_pub)
                    .map(Item::Enum),
                TokKind::KwTrait => self.trait_def(false).map(|mut td| {
                    td.is_pub = is_pub;
                    td.is_package_pub = is_package_pub;
                    Item::Trait(td)
                }),
                TokKind::KwTag => self.tag_def(false).map(|mut td| {
                    td.is_pub = is_pub;
                    td.is_package_pub = is_package_pub;
                    Item::Tag(td)
                }),
                TokKind::KwFn => self
                    .bump_then_func_after_fn(
                        is_pub,
                        is_package_pub,
                        false,
                        false,
                        false,
                        None,
                        None,
                        None,
                        None,
                        false,
                        None,
                        None,
                        None,
                    )
                    .map(Item::Func),
                TokKind::KwModule if self.is_code_module_at(1) => {
                    self.code_module_with_pkg(is_pub, is_package_pub)
                }
                TokKind::Ident(ref n) if n.as_str() == Syntax::KW_STATE_DECL => self
                    .state_decl_with_pkg(is_pub, is_package_pub)
                    .map(Item::StateDecl),
                TokKind::Ident(ref n) if n.as_str() == Syntax::KW_PROTOCOL => self
                    .protocol_decl_with_pkg(is_pub, is_package_pub)
                    .map(Item::ProtocolDecl),
                TokKind::Ident(ref n) if n.as_str() == Syntax::KW_ALIAS => self
                    .type_alias_def(is_pub, is_package_pub)
                    .map(Item::TypeAlias),
                TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_UNIT_FAMILY) => {
                    self.unit_family_def(is_pub, is_package_pub)
                        .map(Item::UnitFamily)
                }
                _ => {
                    let d = Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}`, `{}`, `{}`, or `{}` after `{}`",
                            Syntax::KW_FN,
                            Syntax::KW_STRUCT,
                            Syntax::KW_ENUM,
                            Syntax::KW_ALIAS,
                            Syntax::KW_PRIV
                        ),
                        format!(
                            "`{}` marks one top-level item as private in a `#{}` file",
                            Syntax::KW_PRIV,
                            Syntax::MARKER_PUB_FILE
                        ),
                        format!(
                            "write `{} fn …`, `{} struct …`, or `{} alias …`",
                            Syntax::KW_PRIV,
                            Syntax::KW_PRIV,
                            Syntax::KW_PRIV
                        ),
                        Some(self.peek().span),
                    );
                    Err(d)
                }
            }
        }
    
        /// D-VISDEFAULT2=A: parse top-level item visibility (`priv`, `pub`, defaults).
        fn parse_item_visibility(&mut self) -> (bool, bool) {
            if matches!(self.peek().kind, TokKind::KwPub | TokKind::KwPriv)
                && matches!(self.peek2().kind, TokKind::Colon)
            {
                let span = Span::new(self.peek().span.start, self.peek2().span.end);
                self.bump();
                self.bump();
                self.diags.push(Diagnostic::error(
                    "E0415",
                    "section visibility labels like `pub:` / `priv:` are not supported".to_string(),
                    "moving an item above or below a label would silently change whether it exports"
                        .to_string(),
                    format!(
                        "write `#{}` once at the top of the file, then mark exceptions with `{}`",
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PRIV
                    ),
                    Some(span),
                ));
            }
            if let TokKind::Ident(ref n) = self.peek().kind {
                if (n == Syntax::KW_PUB || n == Syntax::KW_PRIV || n == Syntax::FOREIGN_PRIVATE)
                    && matches!(self.peek2().kind, TokKind::Colon)
                {
                    let span = Span::new(self.peek().span.start, self.peek2().span.end);
                    self.bump();
                    self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0415",
                        "section visibility labels like `pub:` / `priv:` are not supported".to_string(),
                        "moving an item above or below a label would silently change whether it exports"
                            .to_string(),
                        format!(
                            "write `#{}` once at the top of the file, then mark exceptions with `{}`",
                            Syntax::MARKER_PUB_FILE,
                            Syntax::KW_PRIV
                        ),
                        Some(span),
                    ));
                }
            }
            if matches!(
                &self.peek().kind,
                TokKind::Ident(n) if n == Syntax::FOREIGN_PRIVATE
            ) {
                let span = self.peek().span;
                self.bump();
                self.diags.push(Diagnostic::error(
                    "E0412",
                    format!(
                        "write `{}`, not `{}`",
                        Syntax::KW_PRIV,
                        Syntax::FOREIGN_PRIVATE
                    ),
                    format!(
                        "inside a `#{}` file, `{}` marks an item that stays private to this file",
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PRIV
                    ),
                    format!(
                        "write `{} fn …` instead of `{} fn …`",
                        Syntax::KW_PRIV,
                        Syntax::FOREIGN_PRIVATE
                    ),
                    Some(span),
                ));
                if matches!(self.peek().kind, TokKind::KwPub) {
                    let span = Span::new(span.start, self.peek().span.end);
                    self.bump();
                    let _ = self.try_parse_pub_package_suffix();
                    self.diags.push(Diagnostic::error(
                        "E0417",
                        format!(
                            "`{}` and `{}` can't both apply to one item",
                            Syntax::KW_PRIV,
                            Syntax::KW_PUB
                        ),
                        "an item is either public or private — pick one qualifier".to_string(),
                        format!(
                            "drop `{}` (already public in a `#{}` file) or remove `{}`",
                            Syntax::KW_PUB,
                            Syntax::MARKER_PUB_FILE,
                            Syntax::KW_PRIV
                        ),
                        Some(span),
                    ));
                }
                if !self.pub_file_default {
                    self.diags.push(Diagnostic::error(
                        "E0413",
                        format!(
                            "`{}` only applies inside a `#{}` file",
                            Syntax::KW_PRIV,
                            Syntax::MARKER_PUB_FILE
                        ),
                        format!(
                            "without `#{}`, items are private by default and export with `{}`",
                            Syntax::MARKER_PUB_FILE,
                            Syntax::KW_PUB
                        ),
                        format!(
                            "add `#{}` at the top of the file, or write `{}` instead of `{}`",
                            Syntax::MARKER_PUB_FILE,
                            Syntax::KW_PUB,
                            Syntax::KW_PRIV
                        ),
                        Some(span),
                    ));
                }
                return (false, false);
            }
            if matches!(self.peek().kind, TokKind::KwPriv) {
                let span = self.peek().span;
                self.bump();
                if matches!(self.peek().kind, TokKind::KwPub) {
                    let span = Span::new(span.start, self.peek().span.end);
                    self.bump();
                    let _ = self.try_parse_pub_package_suffix();
                    self.diags.push(Diagnostic::error(
                        "E0417",
                        format!(
                            "`{}` and `{}` can't both apply to one item",
                            Syntax::KW_PRIV,
                            Syntax::KW_PUB
                        ),
                        "an item is either public or private — pick one qualifier".to_string(),
                        format!(
                            "drop `{}` (already public in a `#{}` file) or remove `{}`",
                            Syntax::KW_PUB,
                            Syntax::MARKER_PUB_FILE,
                            Syntax::KW_PRIV
                        ),
                        Some(span),
                    ));
                }
                if !self.pub_file_default {
                    self.diags.push(Diagnostic::error(
                        "E0413",
                        format!(
                            "`{}` only applies inside a `#{}` file",
                            Syntax::KW_PRIV,
                            Syntax::MARKER_PUB_FILE
                        ),
                        format!(
                            "without `#{}`, items are private by default and export with `{}`",
                            Syntax::MARKER_PUB_FILE,
                            Syntax::KW_PUB
                        ),
                        format!(
                            "add `#{}` at the top of the file, or write `{}` instead of `{}`",
                            Syntax::MARKER_PUB_FILE,
                            Syntax::KW_PUB,
                            Syntax::KW_PRIV
                        ),
                        Some(span),
                    ));
                }
                return (false, false);
            }
            if matches!(self.peek().kind, TokKind::KwPub) {
                let (is_pub, is_package_pub) = self.parse_pub_qualifier();
                if self.pub_file_default && is_pub && !is_package_pub {
                    self.diags.push(Diagnostic::error(
                        "E0414",
                        format!(
                            "`{}` is redundant in a `#{}` file",
                            Syntax::KW_PUB,
                            Syntax::MARKER_PUB_FILE
                        ),
                        format!(
                            "after `#{}`, top-level items are already public unless marked `{}`",
                            Syntax::MARKER_PUB_FILE,
                            Syntax::KW_PRIV
                        ),
                        format!(
                            "drop `{}` or mark exceptions with `{}`",
                            Syntax::KW_PUB,
                            Syntax::KW_PRIV
                        ),
                        Some(self.toks[self.pos.saturating_sub(1)].span),
                    ));
                }
                return (is_pub, is_package_pub);
            }
            if self.pub_file_default {
                (true, false)
            } else {
                (false, false)
            }
        }
    
        /// D-PUBPKG1=A: parse an optional `pub` or `pub(package)` qualifier.
        /// Returns `(is_pub, is_package_pub)`. On `pub(other)` pushes E0411 and returns `(true, false)`.
        /// Non-failing: never returns Err.
        fn parse_pub_qualifier(&mut self) -> (bool, bool) {
            if !matches!(self.peek().kind, TokKind::KwPub) {
                return (false, false);
            }
            self.bump(); // consume `pub`
            self.try_parse_pub_package_suffix()
        }
    
        /// Consume optional `(package)` after `pub` was already eaten.
        fn try_parse_pub_package_suffix(&mut self) -> (bool, bool) {
            if !matches!(self.peek().kind, TokKind::LParen) {
                return (true, false);
            }
            // pub(…)
            self.bump(); // consume `(`
            match self.peek().kind.clone() {
                TokKind::Ident(ref n) if n == Syntax::PUB_PACKAGE_QUALIFIER => {
                    self.bump(); // consume `package`
                                 // consume `)` — push error if missing but don't abort
                    if matches!(self.peek().kind, TokKind::RParen) {
                        self.bump();
                    } else {
                        let sp = self.peek().span;
                        self.diags.push(Diagnostic::error(
                            "E0003",
                            "expected `)` to close `pub(package)`".to_string(),
                            "write `pub(package)` with no extra content inside the parentheses".to_string(),
                            "use `pub(package)` to restrict access to sibling packages in the same payload".to_string(),
                            Some(sp),
                        ));
                    }
                    (true, true)
                }
                _ => {
                    // pub(something_else) — reject
                    let sp = self.peek().span;
                    self.diags.push(Diagnostic::error(
                        "E0411",
                        format!("unknown `pub(…)` qualifier — only `pub(package)` is supported"),
                        "`pub(package)` restricts access to sibling packages in the same payload"
                            .to_string(),
                        "write `pub` (public to all) or `pub(package)` (package-scoped)".to_string(),
                        Some(sp),
                    ));
                    // skip to `)`
                    while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
                        self.bump();
                    }
                    if matches!(self.peek().kind, TokKind::RParen) {
                        self.bump();
                    }
                    (true, false)
                }
            }
        }
    
        /// Parse a function whose purity is already known (the bare-`pure` teaching
        /// path enters here after emitting E0053 and consuming the `pure` word).
        pub(super) fn func_with_purity(&mut self, is_pure: bool) -> Result<Func, Diagnostic> {
            self.func_with_modifiers(is_pure, false)
        }
    
        /// Parse a function whose `@Pure`/`#Sanitizer` modifiers are already known.
        pub(super) fn func_with_modifiers(
            &mut self,
            is_pure: bool,
            is_sanitizer: bool,
        ) -> Result<Func, Diagnostic> {
            self.func_with_modifiers_full(
                is_pure,
                is_sanitizer,
                None,
                None,
                None,
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
            )
        }
    
        /// Parse a function whose `@Pure`/`#Sanitizer` and D-STATE1 typestate markers
        /// are already known.
        #[allow(clippy::too_many_arguments)]
        pub(super) fn func_with_modifiers_full(
            &mut self,
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
            is_inline: bool,
            is_inline_always: bool,
            inline_span: Option<Span>,
            is_replayable: bool,
            replayable_span: Option<Span>,
        ) -> Result<Func, Diagnostic> {
            let (is_pub, is_package_pub) = self.parse_item_visibility();
            self.expect_kw(TokKind::KwFn, "to start a function definition")?;
            self.func_after_fn(
                is_pub,
                is_package_pub,
                false,
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
                is_inline,
                is_inline_always,
                inline_span,
                is_replayable,
                replayable_span,
            )
        }
    
}
