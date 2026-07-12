use super::super::{
    Diagnostic, Item, Marker, Parser, Span, Syntax, TagDef, TokKind, TraitDef, describe,
};

impl<'a> Parser<'a> {
        /// Parse one marker name and optional `(args)`; cursor sits on the name.
        /// `sigil` is the plane prefix the caller already consumed (`'@'` or
        /// `'#'`) — recorded on the marker for formatter re-emission.
        fn parse_one_marker(&mut self, sigil: char) -> Result<Marker, Diagnostic> {
            let (name, name_span) = self.expect_ident("for a marker name")?;
            let mut args = Vec::new();
            let mut end = name_span.end;
            if matches!(self.peek().kind, TokKind::LParen) {
                self.bump(); // `(`
                while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
                    args.push(self.expr_no_struct_lit()?);
                    if matches!(self.peek().kind, TokKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                end = self.peek().span.end;
                self.expect(TokKind::RParen, "to close marker arguments")?;
            }
            Ok(Marker {
                name,
                name_span,
                args,
                span: Span::new(name_span.start, end),
                sigil,
                ct: None,
            })
        }
    
        /// D-ATTR2: parse one `#[ Name (, …)* ]` group; cursor on `#`.
        fn parse_marker_bracket_group(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            self.bump(); // `#`
            self.bump(); // `[`
            let mut group = Vec::new();
            loop {
                let m = self.parse_one_marker('#')?;
                self.check_marker_plane(&m, false);
                group.push(m);
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect(TokKind::RBracket, "to close a `#[…]` marker list")?;
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            Ok(group)
        }
    
        /// D-MARKER-FAMILY1/G2: parse one `@[ Name (, …)* ]` contract-derive
        /// group; cursor on `@`. The `@` sibling of `parse_marker_bracket_group`.
        fn parse_contract_marker_bracket_group(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            self.bump(); // `@`
            self.bump(); // `[`
            let mut group = Vec::new();
            loop {
                let m = self.parse_one_marker('@')?;
                self.check_marker_plane(&m, true);
                group.push(m);
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect(TokKind::RBracket, "to close a `@[…]` marker list")?;
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            Ok(group)
        }
    
        /// D-MARKER-FAMILY1 (I7/R3 chokepoint): after parsing a marker name in a
        /// bracket/single-marker group, check it landed on the plane its sigil
        /// implies. `on_at` is true when the enclosing group opened with `@`. A
        /// moved contract marker (§2a/§2b/G3) written after `#` is E0062; a
        /// directive name written after `@` is E0063. Any other name (a user
        /// derive on `#`, or an unrecognized `@` name) is left for downstream —
        /// derive resolution, or the generic `@` teaching error — to judge.
        fn check_marker_plane(&mut self, marker: &Marker, on_at: bool) {
            if on_at {
                if Syntax::is_directive_marker(&marker.name) {
                    self.diags
                        .push(Self::e0063_directive_on_at(&marker.name, marker.name_span));
                }
            } else if Syntax::is_contract_marker(&marker.name) {
                self.diags
                    .push(Self::e0062_contract_on_hash(&marker.name, marker.name_span));
            }
        }
    
        /// E0062: a contract marker (moved to `@` by D-MARKERMOVE1/2/3) was
        /// written with `#`. `name` is the marker's bare name (no sigil).
        pub(in crate::Parser) fn e0062_contract_on_hash(name: &str, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0062",
                format!("`#{name}` states a contract — write it with `@`, not `#`"),
                "`@` marks a promise about the declaration below it (`@Pure`, `@MustUse`, \
                 `@Codable`); `#` is for compiler directives (`#Unsafe`, `#Test`). One glance \
                 at the first character tells a reader which it is (D-MARKER-FAMILY1)."
                    .to_string(),
                format!("write `@{name}` (`@` + the same PascalCase name)."),
                Some(span),
            )
        }
    
        /// E0063: a directive marker (stays on `#`) was written with `@`.
        pub(in crate::Parser) fn e0063_directive_on_at(name: &str, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0063",
                format!("`@{name}` is a compiler directive — write it with `#`, not `@`"),
                "`#` changes what compiles or runs (`#Test`, `#Unsafe`, `#Caps`); `@` is \
                 reserved for contracts stated on the declaration below (D-MARKER-FAMILY1)."
                    .to_string(),
                format!("write `#{name}` instead of `@{name}`."),
                Some(span),
            )
        }
    
        /// D-ATTR2 / D-SERDE2–8: parse `#[ … ]` bracket-marker groups. A second
        /// consecutive `#[ … ]` line is teaching error E0999 (merge into one list).
        pub(super) fn parse_marker_groups(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            let mut out = Vec::new();
            let mut groups = 0usize;
            while self.at_marker_list() {
                groups += 1;
                if groups > 1 {
                    self.diags.push(Diagnostic::error(
                        "E0999",
                        "multiple `#[…]` marker lines belong in one comma-separated list".to_string(),
                        "Jet attaches every marker on a type in a single `#[A, B]` group (D-ATTR2); one marker alone is `#A`".to_string(),
                        "merge them: `#[RenameAll(camel), Skip]`, or use `#RenameAll(camel)` when there is only one".to_string(),
                        Some(self.peek().span),
                    ));
                }
                out.extend(self.parse_marker_bracket_group()?);
            }
            Ok(out)
        }
    
        /// D-MARKER-FAMILY1/G2: parse `@[ … ]` contract-marker bracket groups. A
        /// second consecutive `@[ … ]` line is E0999 (same rule, same plane only
        /// — a `@[…]` group and a `#[…]` group may stack on one declaration, so
        /// this never fires across planes).
        fn parse_contract_marker_groups(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            let mut out = Vec::new();
            let mut groups = 0usize;
            while self.at_contract_marker_list() {
                groups += 1;
                if groups > 1 {
                    self.diags.push(Diagnostic::error(
                        "E0999",
                        "multiple `@[…]` marker lines belong in one comma-separated list".to_string(),
                        "Jet attaches every contract marker on a declaration in a single `@[A, B]` group (D-MARKER-FAMILY1); one marker alone is `@A`".to_string(),
                        "merge them: `@[Codable, Debug]`, or use `@Codable` when there is only one".to_string(),
                        Some(self.peek().span),
                    ));
                }
                out.extend(self.parse_contract_marker_bracket_group()?);
            }
            Ok(out)
        }
    
        /// D-ATTR1: parse a lone `#Marker` (or `#Marker(args)`) before `struct`/`enum`.
        fn parse_single_type_prefix_marker(&mut self) -> Result<Marker, Diagnostic> {
            self.bump(); // `#`
            let m = self.parse_one_marker('#')?;
            self.check_marker_plane(&m, false);
            Ok(m)
        }
    
        /// D-MARKER-FAMILY1/G3: parse a lone `@Marker` (or `@Marker(args)`) before
        /// `struct`/`enum` — the `@` sibling of `parse_single_type_prefix_marker`.
        fn parse_single_contract_type_marker(&mut self) -> Result<Marker, Diagnostic> {
            self.bump(); // `@`
            let m = self.parse_one_marker('@')?;
            self.check_marker_plane(&m, true);
            Ok(m)
        }
    
        /// D-MARKER-FAMILY1/G2: parse leading `@[…]` contract-marker groups
        /// and/or `#[…]` directive+serde-marker groups before a struct/enum
        /// field, both optional and stackable (e.g. `@[Redact] #[Rename("x")]`).
        /// Used at field position, which only ever supports the bracket form
        /// (no bare `@Redact`/`#Rename` without brackets).
        pub(super) fn parse_field_markers(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            let mut out = Vec::new();
            loop {
                if self.at_contract_marker_list() {
                    out.extend(self.parse_contract_marker_groups()?);
                } else if self.at_marker_list() {
                    out.extend(self.parse_marker_groups()?);
                } else {
                    break;
                }
            }
            Ok(out)
        }
    
        /// Split parsed markers on a struct/enum: derive-trait markers
        /// (`Codable`→`Encode`+`Decode`, `Encode`, `Decode`, `Debug`, `Summarize`,
        /// `Comparable`, user traits) are pushed onto `derives`; serde *attribute*
        /// markers are returned raw for sema. Markers arrive already validated for
        /// plane (E0062/E0063 pushed by the caller if misplaced, D-MARKER-FAMILY1)
        /// — this only classifies what job each name does, independent of which
        /// sigil it came from.
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
                    // (`@[Debug]`, `@[Summarize]`, `@[Comparable]`) or a `#[…]` user
                    // derive-trait name.
                    _ => derives.push((m.name.clone(), m.name_span)),
                }
            }
            serde
        }
    
        /// Attach parsed type markers to a freshly parsed struct/enum item.
        fn attach_type_markers(markers: Vec<Marker>, item: Item) -> Item {
            match item {
                Item::Struct(mut s) => {
                    // D-MIGRATE1 (I2/E0910 fix): `PublishedSchema` appearing inside an
                    // item-level `@[…]` bracket LIST (e.g. `@[PublishedSchema, Codable]
                    // struct …`) previously only got recorded in `type_markers` — the
                    // dedicated `is_published_schema`/`published_schema_span` fields
                    // (which `SchemaMigration.rs`'s E0910 check guards on) were only ever
                    // set by the single-prefix `@PublishedSchema struct …` form
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
                other => other,
            }
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
                TokKind::Hash
                    if matches!(
                        &self.peek2().kind,
                        TokKind::Ident(n) if n == Syntax::ATTR_LAYOUT
                    ) =>
                {
                    self.layout_struct_def(is_pub).map(Item::Struct)
                }
                TokKind::Hash | TokKind::At
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
                    "derive markers like `@Codable` / `@[Codable]` and serde attributes attach to a type"
                        .to_string(),
                    "write `@Codable struct Name { … }` or `@[Codable] #[RenameAll(camel)] struct …`"
                        .to_string(),
                    Some(self.peek().span),
                )),
            }
        }
    
        /// D-MARKER-FAMILY1/G2: parse leading `@[…]`/`@Name` contract markers
        /// and/or `#[…]`/`#Name` directive+serde markers, in either order, both
        /// optional, both stackable on one declaration (E0999 only fires for two
        /// groups on the SAME plane — a `@[…]` group and a `#[…]` group may
        /// stack). Then parses the struct/enum they attach to. Single dispatch
        /// entry point for both sigils at type-marker position (Items.rs top
        /// match, Modules.rs inline-module body).
        pub(in crate::Parser) fn type_def_with_any_markers(&mut self) -> Result<Item, Diagnostic> {
            let mut markers = Vec::new();
            loop {
                // A marker line may end with an auto-inserted/explicit `;` before
                // the next stacked marker line or the `struct`/`enum` (G2 — both
                // `@[…]` and `#[…]` groups, or lone forms, may stack).
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                if self.at_contract_marker_list() {
                    markers.extend(self.parse_contract_marker_groups()?);
                } else if self.at_single_contract_type_marker() {
                    markers.push(self.parse_single_contract_type_marker()?);
                } else if self.at_marker_list() {
                    markers.extend(self.parse_marker_groups()?);
                } else if self.at_single_type_marker() {
                    markers.push(self.parse_single_type_prefix_marker()?);
                } else {
                    break;
                }
            }
            let item = self.parse_type_after_markers()?;
            Ok(Self::attach_type_markers(markers, item))
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
                // D-EFF3 / D-MARKERMOVE2: a trait method may carry a `@Pure` prefix
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
