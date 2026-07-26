use super::super::{
    Diagnostic, Item, Marker, Parser, Span, Syntax, TagDef, TokKind, TraitDef, describe,
};

impl<'a> Parser<'a> {
        /// D-MARKSIG1=A: parse one marker with the ordinary call-argument
        /// reader; cursor sits on the name.
        pub(super) fn parse_one_marker(&mut self) -> Result<Marker, Diagnostic> {
            let name_token = self.bump();
            let (name, name_span) = match name_token.kind {
                TokKind::Ident(name) => (name, name_token.span),
                TokKind::KwUnsafe => (Syntax::KW_UNSAFE.to_string(), name_token.span),
                _ => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected a marker name".to_string(),
                        "marker names follow `#` and use the registered applied-rule vocabulary"
                            .to_string(),
                        "write a registered marker name after `#`".to_string(),
                        Some(name_token.span),
                    ));
                }
            };
            self.finish_rule_marker(name, name_span)
        }

        /// Complete a marker after its name was consumed by a placement parser.
        /// The argument reader remains identical to an ordinary function call.
        pub(in crate::Parser) fn finish_rule_marker(
            &mut self,
            name: String,
            name_span: Span,
        ) -> Result<Marker, Diagnostic> {
            let mut args = Vec::new();
            let mut arg_labels = Vec::new();
            let mut end = name_span.end;
            if matches!(self.peek().kind, TokKind::LParen) {
                self.bump(); // `(`
                if !matches!(self.peek().kind, TokKind::RParen) {
                    loop {
                        let arg = self.call_arg()?;
                        arg_labels.push(arg.label);
                        args.push(arg.expr);
                        if matches!(self.peek().kind, TokKind::RParen) {
                            break;
                        }
                        self.expect(TokKind::Comma, "between marker arguments")?;
                    }
                }
                end = self.peek().span.end;
                self.expect(TokKind::RParen, "to close marker arguments")?;
            }
            Ok(Marker {
                name,
                name_span,
                args,
                arg_labels,
                span: Span::new(name_span.start, end),
                ct: None,
            })
        }

        /// Shared entry for a bare `#Name` / `#Name(args)` application.
        pub(in crate::Parser) fn parse_rule_marker(&mut self) -> Result<Marker, Diagnostic> {
            let start = self.peek().span.start;
            self.expect(TokKind::Hash, "before a marker")?;
            let mut marker = self.parse_one_marker()?;
            marker.span.start = start;
            Ok(marker)
        }
    
        /// D-SHAPE2: parse one `#[ Name (, …)* ]` group; cursor on `@`.
        fn parse_marker_bracket_group(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            let group_span = self.bump().span; // `#`
            self.bump(); // `[`
            let mut group = Vec::new();
            loop {
                let m = self.parse_one_marker()?;
                group.push(m);
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            let close = self.peek().span;
            self.expect(TokKind::RBracket, "to close an `#[…]` rule list")?;
            if group.len() == 1 {
                let mut diagnostic = Diagnostic::error(
                    "E0999",
                    "one marker is written without brackets".to_string(),
                    "brackets group two or more markers; one marker stays bare"
                        .to_string(),
                    format!("replace `#[{}]` with `#{}`", group[0].name, group[0].name),
                    Some(Span::new(group_span.start, close.end)),
                );
                if group[0].args.is_empty() {
                    diagnostic.edit = Some(crate::Diagnostics::TextEdit {
                        span: Span::new(group_span.start, close.end),
                        new_text: format!("#{}", group[0].name),
                    });
                }
                self.diags.push(diagnostic);
            }
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            Ok(group)
        }
    
        /// D-SHAPE2: parse `#[ … ]` applied-rule groups. A second consecutive
        /// group is teaching error E0999 (merge into one list).
        pub(super) fn parse_marker_groups(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            let mut out = Vec::new();
            let mut groups = 0usize;
            while self.at_marker_list() {
                groups += 1;
                if groups > 1 {
                    self.diags.push(Diagnostic::error(
                        "E0999",
                        "multiple `#[…]` rule lines belong in one comma-separated list".to_string(),
                        "Jet attaches every applied rule on a type in a single `#[A, B]` group (D-SHAPE2); one rule alone is `#A`".to_string(),
                        "merge them: `#[RenameAll(camel), Skip]`, or use `#RenameAll(camel)` when there is only one".to_string(),
                        Some(self.peek().span),
                    ));
                }
                out.extend(self.parse_marker_bracket_group()?);
            }
            Ok(out)
        }
    
        /// D-SHAPE2: parse one bare applied rule, with optional args, before `struct`/`enum`.
        fn parse_single_type_prefix_marker(&mut self) -> Result<Marker, Diagnostic> {
            let start = self.bump().span.start;
            let mut marker = self.parse_one_marker()?;
            marker.span.start = start;
            Ok(marker)
        }
    
        /// D-SHAPE2: parse leading `#[…]` applied-rule groups before a
        /// struct/enum field (e.g. `#[Redact, Rename("x")]`).
        /// Used at field position, which only ever supports the bracket form
        /// (no bare `#Redact`/`#Rename` without brackets).
        pub(super) fn parse_field_markers(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            let mut out = Vec::new();
            let mut bare = 0usize;
            loop {
                if self.at_marker_list() {
                    out.extend(self.parse_marker_groups()?);
                } else if matches!(self.peek().kind, TokKind::Hash) {
                    let start = self.bump().span.start;
                    let mut marker = self.parse_one_marker()?;
                    marker.span.start = start;
                    out.push(marker);
                    bare += 1;
                } else {
                    break;
                }
            }
            if bare > 1 {
                let span = Span::new(out[0].span.start, out.last().unwrap().span.end);
                let mut diagnostic = Diagnostic::error(
                    "E0999",
                    "multiple field markers belong in one bracket list".to_string(),
                    "two or more markers use one `#[A, B]` group".to_string(),
                    "replace the bare stack with one bracket list".to_string(),
                    Some(span),
                );
                if out.iter().all(|marker| marker.args.is_empty()) {
                    diagnostic.edit = Some(crate::Diagnostics::TextEdit {
                        span,
                        new_text: format!(
                            "#[{}]",
                            out.iter()
                                .map(|marker| marker.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
                self.diags.push(diagnostic);
            }
            Ok(out)
        }

        pub(super) fn marker_list_leads_to_function(&self) -> bool {
            if !self.at_marker_list() {
                return false;
            }
            let Some(mut i) = Self::skip_bracket_group(&self.toks, self.pos + 1) else {
                return false;
            };
            while matches!(self.toks.get(i).map(|token| &token.kind), Some(TokKind::Semi)) {
                i += 1;
            }
            matches!(
                self.toks.get(i).map(|token| &token.kind),
                Some(TokKind::KwFn | TokKind::KwPub)
            )
        }

        /// D-MARK-STACK1=A: apply one bracket list to a function. Marker
        /// payloads were already parsed by the shared call-grammar reader.
        pub(super) fn func_with_marker_list(&mut self) -> Result<crate::AST::Func, Diagnostic> {
            let markers = self.parse_marker_groups()?;
            let mut function = self.func()?;
            for marker in markers {
                match marker.name.as_str() {
                    Syntax::KW_TASK if marker.args.is_empty() => {
                        function.is_task = true;
                        function.task_span = Some(marker.span);
                    }
                    Syntax::ATTR_EVERY => {
                        if marker.args.len() != 1 || marker.arg_labels[0].is_some() {
                            return Err(Self::marker_argument_shape_error(
                                Syntax::ATTR_EVERY,
                                marker.span,
                            ));
                        }
                        let arg = match &marker.args[0] {
                            crate::AST::Expr::UnitLit {
                                int,
                                float,
                                suffix,
                                suffix_span,
                                ..
                            } => crate::AST::EveryArg::Duration {
                                int: *int,
                                float: *float,
                                suffix: suffix.clone(),
                                suffix_span: *suffix_span,
                            },
                            crate::AST::Expr::Str(parts, span) if parts.len() == 1 => {
                                match &parts[0] {
                                    crate::AST::StrPart::Lit(text) => crate::AST::EveryArg::WallClock {
                                        text: text.clone(),
                                        text_span: *span,
                                    },
                                    crate::AST::StrPart::Interp(..) => {
                                        return Err(Self::marker_argument_shape_error(
                                            Syntax::ATTR_EVERY,
                                            *span,
                                        ));
                                    }
                                }
                            }
                            other => {
                                return Err(Self::marker_argument_shape_error(
                                    Syntax::ATTR_EVERY,
                                    other.span(),
                                ));
                            }
                        };
                        function.every = Some(crate::AST::EveryMarker {
                            arg,
                            span: marker.span,
                        });
                    }
                    Syntax::ATTR_MUST_USE if marker.args.is_empty() => {
                        function.is_must_use = true;
                        function.must_use_span = Some(marker.span);
                    }
                    Syntax::ATTR_REPLAYABLE if marker.args.is_empty() => {
                        function.is_replayable = true;
                        function.replayable_span = Some(marker.span);
                    }
                    Syntax::KW_SANITIZER if marker.args.is_empty() => function.is_sanitizer = true,
                    Syntax::CONTRACT_INLINE => {
                        function.inline_span = Some(marker.span);
                        if marker.args.is_empty() {
                            function.is_inline = true;
                        } else if marker.args.len() == 1
                            && matches!(&marker.args[0], crate::AST::Expr::Ident(mode, _) if mode == "Always")
                        {
                            function.is_inline_always = true;
                        } else {
                            return Err(Self::marker_argument_shape_error(
                                Syntax::CONTRACT_INLINE,
                                marker.span,
                            ));
                        }
                    }
                    Syntax::CONTRACT_PRE | Syntax::CONTRACT_POST => {
                        if marker.args.len() != 2
                            || marker.arg_labels.iter().any(Option::is_some)
                        {
                            return Err(Self::marker_argument_shape_error(
                                &marker.name,
                                marker.span,
                            ));
                        }
                        let (message, message_span) = match &marker.args[1] {
                            crate::AST::Expr::Str(parts, span) if parts.len() == 1 => {
                                match &parts[0] {
                                    crate::AST::StrPart::Lit(message) => (message.clone(), *span),
                                    crate::AST::StrPart::Interp(..) => {
                                        return Err(Self::marker_argument_shape_error(
                                            &marker.name,
                                            *span,
                                        ));
                                    }
                                }
                            }
                            other => {
                                return Err(Self::marker_argument_shape_error(
                                    &marker.name,
                                    other.span(),
                                ));
                            }
                        };
                        let clause = crate::AST::ContractClause {
                            cond: marker.args[0].clone(),
                            message,
                            message_span,
                            span: marker.span,
                        };
                        if marker.name == Syntax::CONTRACT_PRE {
                            function.pre.push(clause);
                        } else {
                            function.post.push(clause);
                        }
                    }
                    Syntax::KW_STATE => {
                        let [crate::AST::Expr::Ident(state, _)] = marker.args.as_slice() else {
                            return Err(Self::marker_argument_shape_error(
                                Syntax::KW_STATE,
                                marker.span,
                            ));
                        };
                        function.state_requires = Some((state.clone(), marker.span));
                    }
                    Syntax::KW_TRANSITION => {
                        let [crate::AST::Expr::Ident(from, _), crate::AST::Expr::Ident(to, _)] =
                            marker.args.as_slice()
                        else {
                            return Err(Self::marker_argument_shape_error(
                                Syntax::KW_TRANSITION,
                                marker.span,
                            ));
                        };
                        function.state_transition = Some(crate::AST::StateTransition {
                            from: (from != Syntax::STATE_ENTRY).then(|| from.clone()),
                            to: to.clone(),
                            span: marker.span,
                        });
                    }
                    _ => {
                        return Err(Diagnostic::error(
                            "E0355",
                            format!("`#{}` cannot attach through this function marker list", marker.name),
                            "the marker registry gives every marker exact sites and a typed signature"
                                .to_string(),
                            "remove the marker or move it to its registered site".to_string(),
                            Some(marker.span),
                        ));
                    }
                }
            }
            Ok(function)
        }
    
        /// Split parsed markers on a struct/enum: derive-trait markers
        /// (`Codable`→`Encode`+`Decode`, `Encode`, `Decode`, `Debug`, `Summarize`,
        /// `Comparable`, user traits) are pushed onto `derives`; serde *attribute*
        /// markers are returned raw for sema. Markers arrive already validated for
        /// This only classifies each rule's job after the single `@` parser.
        fn split_type_markers(markers: Vec<Marker>, derives: &mut Vec<(String, Span)>) -> Vec<Marker> {
            let mut serde = Vec::new();
            for m in markers {
                match m.name.as_str() {
                    Syntax::ATTR_CODABLE => {
                        derives.push((Syntax::ATTR_ENCODE.to_string(), m.name_span));
                        derives.push((Syntax::ATTR_DECODE.to_string(), m.name_span));
                    }
                    Syntax::ATTR_ENCODE => derives.push((Syntax::ATTR_ENCODE.to_string(), m.name_span)),
                    Syntax::ATTR_DECODE => derives.push((Syntax::ATTR_DECODE.to_string(), m.name_span)),
                    Syntax::ATTR_RENAME_ALL
                    | Syntax::ATTR_DENY_UNKNOWN_FIELDS
                    | Syntax::ATTR_TAG
                    | Syntax::ATTR_UNTAGGED
                    | Syntax::ATTR_RENAME
                    | Syntax::ATTR_SKIP
                    | Syntax::ATTR_DEFAULT
                    | Syntax::ATTR_FLATTEN => serde.push(m),
                    // Any other name is a derive-trait: the D-MARKERMOVE3 built-ins
                    // (`#[Debug]`, `#[Summarize]`, `#[Comparable]`) or a user
                    // derive-trait name.
                    _ => derives.push((m.name.clone(), m.name_span)),
                }
            }
            serde
        }
    
        /// Attach parsed type markers to a freshly parsed struct/enum item.
        fn attach_type_markers(
            &mut self,
            markers: Vec<Marker>,
            item: Item,
        ) -> Result<Item, Diagnostic> {
            Ok(match item {
                Item::Struct(mut s) => {
                    // D-MIGRATE1 (I2/E0910 fix): `PublishedSchema` appearing inside an
                    // item-level `#[…]` bracket LIST (e.g. `#[PublishedSchema, Codable]
                    // struct …`) previously only got recorded in `type_markers` — the
                    // dedicated `is_published_schema`/`published_schema_span` fields
                    // (which `SchemaMigration.rs`'s E0910 check guards on) were only ever
                    // set by the single-prefix `#PublishedSchema struct …` form
                    // (`published_schema_struct_def`). A schema published this way
                    // silently skipped E0910 migration validation. Mirror that form here.
                    if let Some(m) = markers
                        .iter()
                        .find(|m| m.name == Syntax::ATTR_PUBLISHED_SCHEMA)
                    {
                        s.is_published_schema = true;
                        s.published_schema_span = Some(m.span);
                    }
                    s.type_markers = markers.clone();
                    s.serde_markers = Self::split_type_markers(markers, &mut s.derives);
                    Item::Struct(s)
                }
                Item::Enum(mut e) => {
                    e.type_markers = markers.clone();
                    e.serde_markers = Self::split_type_markers(markers, &mut e.derives);
                    Item::Enum(e)
                }
                Item::Distinct(mut d) => {
                    d.type_markers = markers.clone();
                    for marker in markers {
                        match marker.name.as_str() {
                            Syntax::ATTR_NUMERIC => d.is_numeric = true,
                            Syntax::CONTRACT_BUNDLE_COMPARABLE => {
                                d.is_comparable = true;
                                d.comparable_span = Some(marker.span);
                            }
                            Syntax::CONTRACT_BUNDLE_PRINTABLE => {
                                d.is_printable = true;
                                d.printable_span = Some(marker.span);
                            }
                            Syntax::CONTRACT_BUNDLE_CODABLE_AS_BASE => {
                                d.is_codable_as_base = true;
                                d.codable_as_base_span = Some(marker.span);
                            }
                            Syntax::ATTR_INVARIANT => {
                                let (low, high, span, text) =
                                    self.parse_invariant_range(marker)?;
                                d.range = Some((low, high, span));
                                d.invariant = Some((text, span));
                            }
                            _ => {}
                        }
                    }
                    Item::Distinct(d)
                }
                other => other,
            })
        }
    
        /// Parse the type item that follows leading markers.
        fn parse_type_after_markers(&mut self) -> Result<Item, Diagnostic> {
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let is_pub = matches!(self.peek().kind, TokKind::KwPub);
            if is_pub {
                self.bump();
            }
            match &self.peek().kind {
                TokKind::KwStruct => self.struct_def_after_pub(is_pub).map(Item::Struct),
                TokKind::KwEnum => self.enum_def_after_pub(is_pub, false).map(Item::Enum),
                TokKind::Ident(_)
                    if matches!(self.peek2().kind, TokKind::ColonColon)
                        && matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::KW_DISTINCT) =>
                {
                    self.distinct_def(is_pub, false).map(Item::Distinct)
                }
                TokKind::Hash
                    if matches!(
                        &self.peek2().kind,
                        TokKind::Ident(n) if n == Syntax::ATTR_LAYOUT
                    ) =>
                {
                    self.layout_type_def(is_pub)
                }
                TokKind::Hash
                    if matches!(
                        &self.peek2().kind,
                        TokKind::Ident(n) if n == Syntax::ATTR_PUBLISHED_SCHEMA
                    ) =>
                {
                    self.published_schema_struct_def(is_pub).map(Item::Struct)
                }
                other => Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "type markers must sit before a struct or enum, found {}",
                        describe(other)
                    ),
                    "derive markers like `#Codable` / `#[Codable]` and serde attributes attach to a type"
                        .to_string(),
                    "write `#Codable struct Name { … }` or `#[Codable, RenameAll(camel)] struct …`"
                        .to_string(),
                    Some(self.peek().span),
                )),
            }
        }
    
        /// D-SHAPE2: parse leading `#[…]`/`#Name` applied rules, then the
        /// struct/enum they attach to.
        pub(in crate::Parser) fn type_def_with_any_markers(&mut self) -> Result<Item, Diagnostic> {
            let mut markers = Vec::new();
            let mut bare_markers = 0usize;
            loop {
                // A marker line may end with an auto-inserted/explicit `;` before
                // the next stacked rule line or the `struct`/`enum`.
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                if self.at_marker_list() {
                    markers.extend(self.parse_marker_groups()?);
                } else if self.at_single_type_marker() {
                    markers.push(self.parse_single_type_prefix_marker()?);
                    bare_markers += 1;
                } else {
                    break;
                }
            }
            if bare_markers > 1 {
                let span = Span::new(markers[0].span.start, markers.last().unwrap().span.end);
                let mut diagnostic = Diagnostic::error(
                    "E0999",
                    "multiple markers belong in one bracket list".to_string(),
                    "two or more markers use one `#[A, B]` group".to_string(),
                    "replace the bare stack with one bracket list".to_string(),
                    Some(span),
                );
                if markers.iter().all(|marker| marker.args.is_empty()) {
                    diagnostic.edit = Some(crate::Diagnostics::TextEdit {
                        span,
                        new_text: format!(
                            "#[{}]",
                            markers
                                .iter()
                                .map(|marker| marker.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
                self.diags.push(diagnostic);
            }
            let item = self.parse_type_after_markers()?;
            self.attach_type_markers(markers, item)
        }
    
        /// S28: top-level `trait Name { fn sig(self) -> T; … }`.
        pub(in crate::Parser) fn trait_def(&mut self, nested: bool) -> Result<TraitDef, Diagnostic> {
            let item_start = self.peek().span.start;
            let (is_pub, is_package_pub) = if nested {
                (false, false)
            } else {
                self.parse_item_visibility()
            };
            self.expect_kw(TokKind::KwTrait, "to start a trait definition")?;
            let (name, name_span) = self.expect_ident("after `trait`")?;
            self.expect(TokKind::LBrace, "to open the trait body")?;
            let mut methods = Vec::new();
            let mut assoc_types = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                // D-LIB2: `type Name;` associated type declaration.
                if let TokKind::Ident(ref kw) = self.peek().kind.clone() {
                    if kw == "type" {
                        let kw_span = self.bump().span;
                        let (assoc_name, name_span) =
                            self.expect_ident("after `type` in trait body")?;
                        self.finish_stmt()?;
                        assoc_types.push((assoc_name, Span::new(kw_span.start, name_span.end)));
                        continue;
                    }
                }
                // D-EFF3 / D-MARKERMOVE2: a trait method may carry a `#Pure` prefix
                // declaring the empty effect set as its upper bound.
                let is_pure = if self.at_pure_fn() {
                    self.bump_pure_marker();
                    true
                } else {
                    false
                };
                methods.push(self.trait_method_sig(is_pure)?);
            }
            let item_end = self.bump().span.end;
            Ok(TraitDef {
                span: Span::new(item_start, item_end),
                is_pub,
                is_package_pub,
                name,
                name_span,
                assoc_types,
                methods,
            })
        }
    
        /// D-QUAL2: `tag Name;` or `tag Name { … }` — a marker qualifier with no
        /// methods. The body is parsed permissively (it may syntactically contain
        /// method signatures so a stray method doesn't derail the parser); sema
        /// reports each method as E0732.
        pub(in crate::Parser) fn tag_def(&mut self, nested: bool) -> Result<TagDef, Diagnostic> {
            let (is_pub, is_package_pub) = if nested {
                (false, false)
            } else {
                self.parse_item_visibility()
            };
            let start = self.peek().span;
            self.expect_kw(TokKind::KwTag, "to start a tag definition")?;
            let (name, name_span) = self.expect_ident("after `tag`")?;
            let mut methods = Vec::new();
            if matches!(self.peek().kind, TokKind::LBrace) {
                self.bump();
                while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                    if matches!(self.peek().kind, TokKind::Semi) {
                        self.bump();
                        continue;
                    }
                    // A `tag` carries no methods. We still parse a stray `fn …` so the
                    // parser recovers cleanly; sema flags it as E0732.
                    let is_pure = if self.at_pure_fn() {
                        self.bump_pure_marker();
                        true
                    } else {
                        false
                    };
                    methods.push(self.trait_method_sig(is_pure)?);
                }
                self.bump();
            } else {
                // Bare `tag Name;` — the common marker spelling.
                self.finish_stmt()?;
            }
            let end = self.toks[self.pos - 1].span.end;
            Ok(TagDef {
                is_pub,
                is_package_pub,
                name,
                name_span,
                methods,
                span: Span::new(start.start, end),
            })
        }
    
}
