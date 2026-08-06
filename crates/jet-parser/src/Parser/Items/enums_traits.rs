use super::super::{
    Diagnostic, EnumDef, ImplDef, Item, Parser, Span, Syntax, TokKind, Token, TraitImplBlock,
    Variant, VariantField, VariantPayload,
};

impl<'a> Parser<'a> {
        /// Parse `enum Name { … }` given that pub/is_pub was already handled. Factors
        /// out the body of `enum_def` (mirrors `struct_def_after_pub`) so the
        /// `#SingleUse enum` path can reuse it.
        pub(super) fn enum_def_after_pub(
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
                if self.method_starts_here() {
                    methods.push(self.method_in_type()?);
                    continue;
                }
                // D-SERDE5/7 + D-SHAPE2: one bare marker or one multi-marker list.
                if self.at_marker_list() || matches!(self.peek().kind, TokKind::Hash) {
                    let variant_markers =
                        self.parse_field_markers(crate::Policy::RuleSite::Variant)?;
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
                auto_derive_default: true,
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
            for marker in &serde_markers {
                self.bind_rule_fact(
                    marker.name_span,
                    Some(name_span),
                    crate::Policy::RuleSite::Variant,
                );
            }
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
            let discriminant = if matches!(self.peek().kind, TokKind::Eq) {
                self.bump();
                let negative = if matches!(self.peek().kind, TokKind::Minus) {
                    self.bump();
                    true
                } else { false };
                let tok = self.bump();
                let TokKind::Int(raw, _) = tok.kind else {
                    return Err(Diagnostic::error(
                        "E0035", "An enum discriminant must be an integer literal.".to_string(),
                        "C enum values are fixed integers known at compile time.".to_string(),
                        "Write an integer after `=`, such as `Ok = 0`.".to_string(), Some(tok.span)));
                };
                let value = if negative { raw.checked_neg() } else { Some(raw) }.ok_or_else(|| Diagnostic::error(
                    "E0035", "This enum discriminant is outside the supported integer range.".to_string(),
                    "C enum discriminants must fit in a signed 64-bit integer.".to_string(),
                    "Choose a smaller absolute value.".to_string(), Some(tok.span)))?;
                Some(value)
            } else { None };
            if !matches!(self.peek().kind, TokKind::LBrace) {
                variants.push(Variant {
                    name: path,
                    name_span,
                    payload,
                    discriminant,
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
                let entry_markers =
                    if self.at_marker_list() || matches!(self.peek().kind, TokKind::Hash) {
                    self.parse_field_markers(crate::Policy::RuleSite::Variant)?
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
                    if matches!(self.peek().kind, TokKind::Comma) {
                        return Err(Diagnostic::error(
                            "E0003",
                            "a positional enum variant can carry only one value".to_string(),
                            "one positional payload has one type; variants with two or more values use named fields"
                                .to_string(),
                            "name each field, for example `Hit(text: String, count: Int)`"
                                .to_string(),
                            Some(self.peek().span),
                        ));
                    }
                    Ok(VariantPayload::Single(ty, ty_span))
                }
            } else {
                let (ty, ty_span) = self.type_()?;
                if matches!(self.peek().kind, TokKind::Comma) {
                    return Err(Diagnostic::error(
                        "E0003",
                        "a positional enum variant can carry only one value".to_string(),
                        "one positional payload has one type; variants with two or more values use named fields"
                            .to_string(),
                        "name each field, for example `Hit(text: String, count: Int)`"
                            .to_string(),
                        Some(self.peek().span),
                    ));
                }
                Ok(VariantPayload::Single(ty, ty_span))
            }
        }
    
        /// D-ERR-CONV (ratified 2026-06-19): dispatch `impl …` to either the normal
        /// `ImplDef` path or the `impl Source => Target { body }` error-conversion path.
        pub(in crate::Parser) fn impl_or_error_conv(&mut self) -> Result<Item, Diagnostic> {
            let item_start = self.peek().span.start;
            self.expect_kw(TokKind::KwImpl, "to start an `impl` block")?;
            // D-IMPLDOT1=A: trait impl is `impl Type.Trait { … }`. D-PROTO1/D-PROTO2 add
            // inherent impl on protocol handles `impl Payment.Client { … }` / `.Server`.
            let (type_name, type_span, mut trait_name, mut trait_span) = {
                let (first, first_span) = self.expect_ident("after `impl`")?;
                let mut parts = vec![(first, first_span)];
                while matches!(self.peek().kind, TokKind::Dot) {
                    self.bump();
                    parts.push(self.expect_ident("after `.` in `impl`")?);
                }
                let is_error_conversion =
                    matches!(self.peek().kind, TokKind::LambdaArrow | TokKind::Arrow);
                let is_protocol_impl = parts.len() == 2
                    && matches!(parts[1].0.as_str(), "Client" | "Server")
                    && matches!(self.peek().kind, TokKind::LBrace);
                if parts.len() == 1 || is_error_conversion || is_protocol_impl {
                    let end = parts.last().unwrap().1.end;
                    let name = parts
                        .iter()
                        .map(|(part, _)| part.as_str())
                        .collect::<Vec<_>>()
                        .join(".");
                    (name, Span::new(first_span.start, end), None, None)
                } else {
                    let (last, last_span) = parts.pop().unwrap();
                    let owner_end = parts.last().unwrap().1.end;
                    let owner = parts
                        .iter()
                        .map(|(part, _)| part.as_str())
                        .collect::<Vec<_>>()
                        .join(".");
                    (
                        owner,
                        Span::new(first_span.start, owner_end),
                        Some(last),
                        Some(last_span),
                    )
                }
            };
            // Detect `impl Source => Target { body }` — D-ERR-CONV as respelled
            // by D-ARROW-CONTROL1. Accept `->` only to emit its migration error.
            if matches!(self.peek().kind, TokKind::LambdaArrow | TokKind::Arrow) {
                let arrow = self.bump();
                if matches!(arrow.kind, TokKind::Arrow) {
                    self.diags.push(Self::retired_callable_arrow(arrow.span));
                }
                let (to_ty, to_span) = self.parse_type_path("after `=>` in error conversion")?;
                // Peek the `{` span before consuming.
                if !matches!(self.peek().kind, TokKind::LBrace) {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `{` to open the error-conversion body".to_string(),
                        "an error conversion body is a block: `impl Source => Target { … }`"
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
    
        /// D-OSTARGET1=A: parse the `impl` block that must follow a `#Target(OS.X)`
        /// marker and attach `os` to it. Reuses `impl_or_error_conv` (same grammar,
        /// same `impl Type.Trait { … }` / delegation / error-conversion forms) and
        /// stamps the OS gate onto the resulting `ImplDef` afterward — no need to
        /// thread a new parameter through every `impl` parse path or its other
        /// caller (`Modules.rs`'s inline-module item loop calls this same helper).
        pub(in crate::Parser) fn os_gated_impl(
            &mut self,
            os: crate::Syntax::OSTarget,
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
                    format!("`#Target(OS.{})` isn't valid here", os.name()),
                    "`OS.Linux`/`OS.MacOS`/`OS.Windows` gates a whole `impl` block, not a module, function, or any other item".to_string(),
                    format!("write `#Target(OS.{}) impl Type.Trait {{ … }}`", os.name()),
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
                    format!("`#Target(OS.{})` isn't valid on an error-conversion `impl`", os.name()),
                    "`impl Source => Target { … }` error conversions run on every platform; OS gating only makes sense for a real trait/inherent impl".to_string(),
                    format!("remove the `#Target(OS.{})` marker", os.name()),
                    Some(ec.from_span),
                )),
                other => Ok(other),
            }
        }
    
        /// S28: `impl Trait { … }` inside a struct/enum body.
        pub(super) fn trait_impl_block(&mut self) -> Result<TraitImplBlock, Diagnostic> {
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
        pub(super) fn derive_line(&mut self) -> Result<(String, Span), Diagnostic> {
            let start = self.bump().span;
            let (trait_name, _) = self.expect_ident("after `derive`")?;
            self.finish_stmt()?;
            Ok((trait_name, start))
        }
    
        /// True when the cursor is at an `#[ … ]` applied-rule group (D-SHAPE2).
        pub(in crate::Parser) fn at_marker_list(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash) && matches!(self.peek2().kind, TokKind::LBracket)
        }
    
        /// D-ATTR1/D-MARKER-CANON1: a PascalCase applied rule immediately before `struct`/`enum`.
        pub(in crate::Parser) fn at_single_type_marker(&self) -> bool {
            if !matches!(self.peek().kind, TokKind::Hash) {
                return false;
            }
            let (name, after_name) = if matches!(self.peek2().kind, TokKind::Bang) {
                let TokKind::Ident(name) = &self.toks[self.pos + 2].kind else {
                    return false;
                };
                (name, self.pos + 3)
            } else {
                let TokKind::Ident(name) = &self.peek2().kind else {
                    return false;
                };
                (name, self.pos + 2)
            };
            if !Self::is_pascal_type_marker_name(name) || Self::is_reserved_item_rule_prefix(name) {
                return false;
            }
            self.type_marker_prefix_leads_to_type_def(after_name)
        }
    
        fn is_pascal_type_marker_name(name: &str) -> bool {
            name.chars().next().is_some_and(|c| c.is_uppercase())
        }
    
        /// Applied rules that own dedicated item parsers rather than the generic
        /// type-derive path.
        fn is_reserved_item_rule_prefix(name: &str) -> bool {
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
                    | Syntax::MARKER_UNIT_FAMILY
                    | Syntax::MARKER_NUMERIC
                    | Syntax::MARKER_LAYOUT
                    | Syntax::MARKER_PUBLISHED_SCHEMA
                    | Syntax::MARKER_SINGLE_USE
                    | Syntax::MARKER_MUST_USE
                    | Syntax::MARKER_EXTERN_MODULE
                    | Syntax::MARKER_BINDGEN
                    | Syntax::MARKER_TARGET
                    | Syntax::MARKER_WASM_EXPORT
            )
        }
    
        /// After an applied rule (and optional `(args)`), does the next real token start a type?
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
            // rule prefixes (`#[Codable, RenameAll(camel)] struct …`, or
            // `#MustUse #[Codable] struct …`) — keep skipping rule-shaped
            // prefixes (bracket groups or lone `#Name`/`#Name(args)?`) until none
            // remain, then require `pub`? `struct`/`enum`.
            loop {
                while i < self.toks.len() && matches!(self.toks[i].kind, TokKind::Semi) {
                    i += 1;
                }
                if i >= self.toks.len() {
                    return false;
                }
                let is_marker_sigil = matches!(self.toks[i].kind, TokKind::Hash);
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
    
        /// `toks[i]` is the `[` opening a `#[…]`/`#[…]` marker-bracket group;
        /// returns the index just past its matching `]`, or `None` if
        /// unterminated.
        pub(super) fn skip_bracket_group(toks: &[Token], mut i: usize) -> Option<usize> {
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
