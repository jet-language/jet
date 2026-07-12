impl<'a> Parser<'a> {
        /// Parse `enum Name { … }` given that pub/is_pub was already handled. Factors
        /// out the body of `enum_def` (mirrors `struct_def_after_pub`) so the
        /// `#SingleUse enum` path can reuse it.
        fn enum_def_after_pub(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
        ) -> Result<EnumDef, Diagnostic> {
            let item_start = self.peek().span.start;
            self.expect_kw(TokKind::KwEnum, "to start an enum definition")?;
            let (name, name_span) = self.expect_ident("after `enum`")?;
            let type_params = self.parse_opt_type_params()?;
            self.expect(TokKind::LBrace, "to open the enum body")?;
            let mut variants = Vec::new();
            let mut groups = Vec::new();
            let mut methods = Vec::new();
            let mut trait_impls = Vec::new();
            let mut derives = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                // D-SERDE5/7: `#[Rename("x")]` on a variant — variant-level serde markers.
                if self.at_marker_list() {
                    let variant_markers = self.parse_marker_groups()?;
                    self.variant_entry("", &mut variants, &mut groups, variant_markers)?;
                    if matches!(self.peek().kind, TokKind::Semi) {
                        self.bump();
                    }
                    continue;
                }
                if matches!(self.peek().kind, TokKind::KwDerive) {
                    derives.push(self.derive_line()?);
                } else if matches!(self.peek().kind, TokKind::KwImpl) {
                    trait_impls.push(self.trait_impl_block()?);
                } else if matches!(self.peek().kind, TokKind::KwFn | TokKind::KwPub) {
                    methods.push(self.method_in_type()?);
                } else {
                    self.variant_entry("", &mut variants, &mut groups, Vec::new())?;
                    if matches!(self.peek().kind, TokKind::Semi) {
                        self.bump();
                    }
                }
            }
            let item_end = self.bump().span.end;
            Ok(EnumDef {
                span: Span::new(item_start, item_end),
                is_pub,
                is_package_pub,
                name,
                name_span,
                type_params,
                variants,
                methods,
                trait_impls,
                derives,
                is_single_use: false,
                single_use_span: None,
                is_must_use: false,
                must_use_span: None,
                serde_markers: Vec::new(),
                type_markers: Vec::new(),
                groups,
            })
        }
    
        /// D-TAG1 (ratified 2026-07-03): parse one enum-body entry — a leaf variant
        /// or a variant group `Name { … }`. Leaves are recorded flat with their full
        /// dotted path (`prefix.Name`); each group path is recorded in `groups` so
        /// sema and the formatter see the tree. Groups nest to any depth. Payloads
        /// live on leaves only (E0331).
        fn variant_entry(
            &mut self,
            prefix: &str,
            variants: &mut Vec<crate::AST::Variant>,
            groups: &mut Vec<crate::AST::EnumGroup>,
            serde_markers: Vec<crate::AST::Marker>,
        ) -> Result<(), Diagnostic> {
            let (name, name_span) = self.expect_ident("for a variant name")?;
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}.{name}")
            };
            let payload = if matches!(self.peek().kind, TokKind::LParen) {
                let payload_start = self.bump().span; // consume `(`
                let payload = self.variant_payload()?;
                self.expect(TokKind::RParen, "after a variant's payload")?;
                let payload_end = self.toks[self.pos.saturating_sub(1)].span.end;
                if matches!(self.peek().kind, TokKind::LBrace) {
                    // D-TAG1: a payload on a group name — payloads live on leaves only.
                    self.diags.push(Diagnostic::error(
                        "E0331",
                        format!("group `{}` can't carry a payload", path),
                        "a variant group only names its subtree — data lives on the leaf variants (D-TAG1)".to_string(),
                        "move the payload onto the leaf variants inside the `{ }`, or remove the `(...)`".to_string(),
                        Some(Span::new(payload_start.start, payload_end)),
                    ));
                    VariantPayload::Unit
                } else {
                    payload
                }
            } else {
                VariantPayload::Unit
            };
            if !matches!(self.peek().kind, TokKind::LBrace) {
                variants.push(Variant {
                    name: path,
                    name_span,
                    payload,
                    serde_markers,
                });
                return Ok(());
            }
            // A variant group: `Name { entry (,|newline entry)* }`.
            self.bump(); // consume `{`
            if !serde_markers.is_empty() {
                self.diags.push(Diagnostic::error(
                    "E0003",
                    format!("a `#[…]` marker can't sit on group `{}`", path),
                    "serde markers rename wire names, and only leaf variants reach the wire (D-TAG1)"
                        .to_string(),
                    "move the marker onto a leaf variant inside the `{ }`".to_string(),
                    Some(name_span),
                ));
            }
            let leaves_before = variants.len();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi | TokKind::Comma) {
                    self.bump();
                    continue;
                }
                let entry_markers = if self.at_marker_list() {
                    self.parse_marker_groups()?
                } else {
                    Vec::new()
                };
                self.variant_entry(&path, variants, groups, entry_markers)?;
                if matches!(self.peek().kind, TokKind::Semi | TokKind::Comma) {
                    self.bump();
                }
            }
            self.expect(TokKind::RBrace, "to close the variant group")?;
            if variants.len() == leaves_before {
                self.diags.push(Diagnostic::error(
                    "E0003",
                    format!("group `{}` has no variants", path),
                    "an empty group can never match anything — every group needs at least one leaf variant (D-TAG1)".to_string(),
                    "add a variant inside the `{ }`, or remove the group".to_string(),
                    Some(name_span),
                ));
            }
            groups.push(crate::AST::EnumGroup { path, name_span });
            Ok(())
        }
    
        fn variant_payload(&mut self) -> Result<VariantPayload, Diagnostic> {
            if matches!(self.peek().kind, TokKind::Ident(_)) {
                let peek2 = self.peek2().kind.clone();
                if matches!(peek2, TokKind::Colon) {
                    let mut fields = Vec::new();
                    loop {
                        let (name, name_span) = self.expect_ident("for a variant field name")?;
                        self.expect(TokKind::Colon, "after a variant field name")?;
                        let (ty, ty_span) = self.type_()?;
                        fields.push(VariantField {
                            name,
                            name_span,
                            ty,
                            ty_span,
                        });
                        if !matches!(self.peek().kind, TokKind::Comma) {
                            break;
                        }
                        self.bump();
                    }
                    Ok(VariantPayload::Named(fields))
                } else {
                    let (ty, ty_span) = self.type_()?;
                    Ok(VariantPayload::Single(ty, ty_span))
                }
            } else {
                let (ty, ty_span) = self.type_()?;
                Ok(VariantPayload::Single(ty, ty_span))
            }
        }
    
        /// D-ERR-CONV (ratified 2026-06-19): dispatch `impl …` to either the normal
        /// `ImplDef` path or the `impl Source -> Target { body }` error-conversion path.
        pub(super) fn impl_or_error_conv(&mut self) -> Result<Item, Diagnostic> {
            let item_start = self.peek().span.start;
            self.expect_kw(TokKind::KwImpl, "to start an `impl` block")?;
            // D-IMPLDOT1=A: trait impl is `impl Type.Trait { … }`. D-PROTO1/D-PROTO2 add
            // inherent impl on protocol handles `impl Payment.Client { … }` / `.Server`.
            let (type_name, type_span, mut trait_name, mut trait_span) = {
                let (first, span) = self.expect_ident("after `impl`")?;
                if matches!(self.peek().kind, TokKind::Dot) {
                    self.bump();
                    let (second, second_span) = self.expect_ident("after `.` in `impl`")?;
                    if matches!(self.peek().kind, TokKind::LBrace)
                        && (second == "Client" || second == "Server")
                    {
                        (format!("{first}.{second}"), span, None, None)
                    } else {
                        (first, span, Some(second), Some(second_span))
                    }
                } else {
                    (first, span, None, None)
                }
            };
            // Detect `impl Source -> Target { body }` — D-ERR-CONV.
            if matches!(self.peek().kind, TokKind::Arrow) {
                let _arrow = self.bump(); // consume `->`
                let (to_ty, to_span) = self.parse_type_path("after `->` in error conversion")?;
                // Peek the `{` span before consuming.
                if !matches!(self.peek().kind, TokKind::LBrace) {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `{` to open the error-conversion body".to_string(),
                        "an error conversion body is a block: `impl Source -> Target { … }`"
                            .to_string(),
                        "add `{` after the target type".to_string(),
                        Some(self.peek().span),
                    ));
                }
                let brace_start = self.bump().span.start; // consume `{`
                                                          // block_stmts consumes statements AND the closing `}`.
                                                          // We need to track where the `}` ended — peek first to record pos.
                let body = self.block_stmts();
                // After block_stmts, the `}` is consumed; the last consumed token is at pos-1.
                let rbrace_end = self.toks[self.pos.saturating_sub(1)].span.end;
                let body_span = Span::new(brace_start, rbrace_end);
                return Ok(Item::ErrorConv(crate::AST::ErrorConvDef {
                    from_ty: type_name,
                    from_span: type_span,
                    to_ty,
                    to_span,
                    body,
                    body_span,
                }));
            }
            // Normal `impl` path — trait_name/trait_span were parsed above.
            if trait_name.is_none() && matches!(self.peek().kind, TokKind::Colon) {
                // Teaching error: old `impl Type: Trait` form.
                let colon_span = self.peek().span;
                self.diags.push(Diagnostic::error(
                    "E0321",
                    format!("trait separator is now `.`, not `:`"),
                    "the impl separator reads \"Type's Trait\", matching the dot accessor".to_string(),
                    format!("write `impl {}.Trait {{ … }}`", type_name),
                    Some(colon_span),
                ));
                self.bump(); // consume old `:`
                let (t, ts) = self.expect_ident("after `:` in `impl Type: Trait`")?;
                trait_name = Some(t);
                trait_span = Some(ts);
            }
            // S62: `impl Type.Trait using field_name;` — delegation form.
            if let TokKind::Ident(ref kw) = self.peek().kind.clone() {
                if kw == "using" && trait_name.is_some() {
                    self.bump(); // consume `using`
                    let (field, _) = self.expect_ident("after `using` for the delegation field")?;
                    self.finish_stmt()?;
                    return Ok(Item::Impl(ImplDef {
                        span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                        type_name,
                        type_span,
                        trait_name,
                        trait_span,
                        methods: Vec::new(),
                        delegation_field: Some(field),
                        assoc_type_impls: Vec::new(),
                        is_generated_serde: false,
                        os_target: None,
                    }));
                }
            }
            self.expect(TokKind::LBrace, "to open the `impl` body")?;
            let mut methods = Vec::new();
            let mut assoc_type_impls = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                if let TokKind::Ident(ref kw) = self.peek().kind.clone() {
                    if kw == "type" {
                        let kw_span = self.bump().span;
                        let (assoc_name, name_span) = self.expect_ident("after `type` in impl body")?;
                        self.expect(TokKind::Eq, "after associated type name")?;
                        let (assoc_ty, _) = self.type_()?;
                        self.finish_stmt()?;
                        assoc_type_impls.push((
                            assoc_name,
                            Span::new(kw_span.start, name_span.end),
                            assoc_ty,
                        ));
                        continue;
                    }
                }
                methods.push(self.method_in_type()?);
            }
            let item_end = self.bump().span.end;
            Ok(Item::Impl(ImplDef {
                span: Span::new(item_start, item_end),
                type_name,
                type_span,
                trait_name,
                trait_span,
                methods,
                delegation_field: None,
                assoc_type_impls,
                is_generated_serde: false,
                os_target: None,
            }))
        }
    
        /// D-OSTARGET1=A: parse the `impl` block that must follow a `#Target(Os.X)`
        /// marker and attach `os` to it. Reuses `impl_or_error_conv` (same grammar,
        /// same `impl Type.Trait { … }` / delegation / error-conversion forms) and
        /// stamps the OS gate onto the resulting `ImplDef` afterward — no need to
        /// thread a new parameter through every `impl` parse path or its other
        /// caller (`Modules.rs`'s inline-module item loop calls this same helper).
        pub(super) fn os_gated_impl(
            &mut self,
            os: crate::Syntax::OsTarget,
        ) -> Result<Item, Diagnostic> {
            // S6-R: a synthetic statement-terminator `;` may follow the marker
            // line (same as the `TokKind::Semi` skip at the top of the top-level
            // item loop) — swallow it before checking what actually follows.
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            if !matches!(self.peek().kind, TokKind::KwImpl) {
                let span = self.peek().span;
                return Err(Diagnostic::error(
                    "E0003",
                    format!("`#Target(Os.{})` isn't valid here", os.name()),
                    "`Os.Linux`/`Os.Macos`/`Os.Windows` gates a whole `impl` block, not a module, function, or any other item".to_string(),
                    format!("write `#Target(Os.{}) impl Type.Trait {{ … }}`", os.name()),
                    Some(span),
                ));
            }
            match self.impl_or_error_conv()? {
                Item::Impl(mut i) => {
                    i.os_target = Some(os);
                    Ok(Item::Impl(i))
                }
                Item::ErrorConv(ec) => Err(Diagnostic::error(
                    "E0003",
                    format!("`#Target(Os.{})` isn't valid on an error-conversion `impl`", os.name()),
                    "`impl Source -> Target { … }` error conversions run on every platform; OS gating only makes sense for a real trait/inherent impl".to_string(),
                    format!("remove the `#Target(Os.{})` marker", os.name()),
                    Some(ec.from_span),
                )),
                other => Ok(other),
            }
        }
    
        /// S28: `impl Trait { … }` inside a struct/enum body.
        fn trait_impl_block(&mut self) -> Result<TraitImplBlock, Diagnostic> {
            self.expect_kw(TokKind::KwImpl, "to start a trait impl block")?;
            let (trait_name, trait_span) = self.expect_ident("after `impl`")?;
            self.expect(TokKind::LBrace, "to open the trait impl body")?;
            let mut methods = Vec::new();
            let mut assoc_type_impls = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                // D-LIB2: `type Name = ConcreteType;`
                if let TokKind::Ident(ref kw) = self.peek().kind.clone() {
                    if kw == "type" {
                        let kw_span = self.bump().span;
                        let (assoc_name, name_span) = self.expect_ident("after `type` in impl body")?;
                        self.expect(TokKind::Eq, "after associated type name")?;
                        let (assoc_ty, _) = self.type_()?;
                        self.finish_stmt()?;
                        assoc_type_impls.push((
                            assoc_name,
                            Span::new(kw_span.start, name_span.end),
                            assoc_ty,
                        ));
                        continue;
                    }
                }
                methods.push(self.method_in_type()?);
            }
            self.bump();
            Ok(TraitImplBlock {
                trait_name,
                trait_span,
                methods,
                assoc_type_impls,
            })
        }
    
        /// S55: `derive Comparable;` inside a type body.
        fn derive_line(&mut self) -> Result<(String, Span), Diagnostic> {
            let start = self.bump().span;
            let (trait_name, _) = self.expect_ident("after `derive`")?;
            self.finish_stmt()?;
            Ok((trait_name, start))
        }
    
        /// True when the cursor is at a `#[ … ]` bracket-marker group (D-ATTR2).
        pub(super) fn at_marker_list(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash) && matches!(self.peek2().kind, TokKind::LBracket)
        }
    
        /// D-MARKER-FAMILY1/G2: true at a `@[ … ]` contract-derive bracket group —
        /// the `@` sibling of `at_marker_list`.
        pub(super) fn at_contract_marker_list(&self) -> bool {
            matches!(self.peek().kind, TokKind::At) && matches!(self.peek2().kind, TokKind::LBracket)
        }
    
        /// D-ATTR1/D-MARKER-CANON1: a PascalCase `#Marker` immediately before `struct`/`enum`.
        pub(super) fn at_single_type_marker(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::Hash) {
                return false;
            }
            let TokKind::Ident(name) = &self.peek2().kind else {
                return false;
            };
            if !Self::is_pascal_type_marker_name(name) || Self::is_reserved_hash_item_prefix(name) {
                return false;
            }
            self.type_marker_prefix_leads_to_type_def(self.pos + 2)
        }
    
        /// D-MARKER-FAMILY1/G3: a PascalCase `@Marker` immediately before
        /// `struct`/`enum` — the `@` sibling of `at_single_type_marker` (contract
        /// derives: Codable, Encode, Decode, Debug, Summarize, Comparable).
        pub(super) fn at_single_contract_type_marker(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::At) {
                return false;
            }
            let TokKind::Ident(name) = &self.peek2().kind else {
                return false;
            };
            if !Self::is_pascal_type_marker_name(name) || Self::is_reserved_at_item_prefix(name) {
                return false;
            }
            self.type_marker_prefix_leads_to_type_def(self.pos + 2)
        }
    
        /// `@` markers that own their own item parser (MustUse/PublishedSchema
        /// struct dispatch, Numeric distinct-type dispatch) — not bare
        /// contract-derive prefixes routed through the generic marker-list
        /// mechanism.
        fn is_reserved_at_item_prefix(name: &str) -> bool {
            matches!(
                name,
                Syntax::ATTR_MUST_USE | Syntax::ATTR_PUBLISHED_SCHEMA | Syntax::ATTR_NUMERIC
            )
        }
    
        fn is_pascal_type_marker_name(name: &str) -> bool {
            name.chars().next().is_some_and(|c| c.is_uppercase())
        }
    
        /// `#` markers that own their own item parser — not bare type derive prefixes.
        fn is_reserved_hash_item_prefix(name: &str) -> bool {
            matches!(
                name,
                Syntax::KW_TEST
                    | Syntax::KW_BENCH
                    | Syntax::KW_UNSAFE
                    | Syntax::KW_REACTIVE
                    | Syntax::KW_PURE
                    | Syntax::KW_SANITIZER
                    | Syntax::KW_STATE
                    | Syntax::KW_TRANSITION
                    | Syntax::MARKER_PUB_FILE
                    | Syntax::ATTR_UNIT_FAMILY
                    | Syntax::ATTR_NUMERIC
                    | Syntax::ATTR_LAYOUT
                    | Syntax::ATTR_PUBLISHED_SCHEMA
                    | Syntax::ATTR_SINGLE_USE
                    | Syntax::ATTR_MUST_USE
                    | Syntax::ATTR_EXTERN_MODULE
                    | Syntax::ATTR_BINDGEN
                    | Syntax::ATTR_TARGET
                    | Syntax::ATTR_WASM
                    | Syntax::ATTR_JS
                    | Syntax::ATTR_WASM_EXPORT
            )
        }
    
        /// After a `#Marker` (and optional `(args)`), does the next real token start a type?
        fn type_marker_prefix_leads_to_type_def(&self, mut i: usize) -> bool {
            if i >= self.toks.len() {
                return false;
            }
            // Skip this marker's own optional `(args)`.
            i = match Self::skip_balanced_parens(&self.toks, i) {
                Some(n) => n,
                None => return false,
            };
            // D-MARKER-FAMILY1/G2: a type declaration may carry several stacked
            // marker prefixes (`@[Codable] #[RenameAll(camel)] struct …`, or
            // `@MustUse @[Codable] struct …`) — keep skipping marker-shaped
            // prefixes (bracket groups or lone `#Name`/`@Name(args)?`) until none
            // remain, then require `pub`? `struct`/`enum`.
            loop {
                while i < self.toks.len() && matches!(self.toks[i].kind, TokKind::Semi) {
                    i += 1;
                }
                if i >= self.toks.len() {
                    return false;
                }
                let is_marker_sigil = matches!(self.toks[i].kind, TokKind::Hash | TokKind::At);
                if !is_marker_sigil {
                    break;
                }
                match self.toks.get(i + 1).map(|t| &t.kind) {
                    Some(TokKind::LBracket) => {
                        i = match Self::skip_bracket_group(&self.toks, i + 1) {
                            Some(n) => n,
                            None => return false,
                        };
                    }
                    Some(TokKind::Ident(name)) if Self::is_pascal_type_marker_name(name) => {
                        i += 2; // `#`/`@` and the ident
                        i = match Self::skip_balanced_parens(&self.toks, i) {
                            Some(n) => n,
                            None => return false,
                        };
                    }
                    _ => break,
                }
            }
            if i < self.toks.len() && matches!(self.toks[i].kind, TokKind::KwPub) {
                i += 1;
            }
            i < self.toks.len() && matches!(self.toks[i].kind, TokKind::KwStruct | TokKind::KwEnum)
        }
    
        /// If `toks[i]` is `(`, returns the index just past its matching `)`;
        /// otherwise returns `i` unchanged. `None` on an unbalanced/unterminated
        /// paren group.
        fn skip_balanced_parens(toks: &[Token], mut i: usize) -> Option<usize> {
            if i >= toks.len() || !matches!(toks[i].kind, TokKind::LParen) {
                return Some(i);
            }
            let mut depth = 0usize;
            loop {
                if i >= toks.len() {
                    return None;
                }
                match toks[i].kind {
                    TokKind::LParen => depth += 1,
                    TokKind::RParen => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        }
    
        /// `toks[i]` is the `[` opening a `#[…]`/`@[…]` marker-bracket group;
        /// returns the index just past its matching `]`, or `None` if
        /// unterminated.
        fn skip_bracket_group(toks: &[Token], mut i: usize) -> Option<usize> {
            debug_assert!(matches!(toks[i].kind, TokKind::LBracket));
            let mut depth = 0usize;
            loop {
                if i >= toks.len() {
                    return None;
                }
                match toks[i].kind {
                    TokKind::LBracket => depth += 1,
                    TokKind::RBracket => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        }
    
}
