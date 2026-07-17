use super::super::{Diagnostic, Parser, Span, Syntax, TokKind, describe, string_literal_value};
use super::helpers::parse_invariant_bounds;

impl<'a> Parser<'a> {
        // --- statements ------------------------------------------------------
    
        // --- distinct types --------------------------------------------------
    
        /// D-DIST1/D-BIND4: true when the cursor is at `Name :: distinct`.
        pub(super) fn at_distinct_def(&self) -> bool {
            matches!(&self.peek().kind, TokKind::Ident(_))
                && matches!(&self.peek2().kind, TokKind::ColonColon)
                && matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::KW_DISTINCT)
        }
    
        /// D-CAPBUNDLE1: is `name` one of the four capability-bundle marker names
        /// (`@Numeric`, `@Comparable`, `@Printable`, `@CodableAsBase`) that may
        /// stack before a `distinct` type declaration?
        fn is_capability_bundle_marker(name: &str) -> bool {
            name == Syntax::ATTR_NUMERIC
                || name == Syntax::CONTRACT_BUNDLE_COMPARABLE
                || name == Syntax::CONTRACT_BUNDLE_PRINTABLE
                || name == Syntax::CONTRACT_BUNDLE_CODABLE_AS_BASE
        }
    
        fn is_distinct_prefix_marker(name: &str) -> bool {
            Self::is_capability_bundle_marker(name) || name == Syntax::ATTR_INVARIANT
        }
    
        /// D-DIST3 / D-CAPBUNDLE1 / D-MARKERMOVE1 (ratified 2026-06-20 /
        /// 2026-07-01): true when a stack of one or more capability-bundle
        /// markers (`@Numeric`, `@Comparable`, `@Printable`, `@CodableAsBase`,
        /// any order, retired `#` spelling included so `distinct_def` can teach
        /// E0062) precedes `Name :: distinct` at the cursor. The `@Numeric`-only
        /// sibling of the old `at_numeric_distinct_def` predicate, generalized to
        /// the four fixed bundles.
        pub(super) fn at_bundle_distinct_def(&self) -> bool {
            let mut i = self.pos;
            let mut saw_marker = false;
            loop {
                match self.toks.get(i).map(|t| &t.kind) {
                    Some(TokKind::At) => {
                        match self.toks.get(i + 1).map(|t| &t.kind) {
                            Some(TokKind::Ident(n)) if Self::is_distinct_prefix_marker(n) => {
                                saw_marker = true;
                                i += 2;
                                if matches!(
                                    self.toks.get(i - 1).map(|t| &t.kind),
                                    Some(TokKind::Ident(n)) if n == Syntax::ATTR_INVARIANT
                                ) && matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::LParen))
                                {
                                    let mut depth = 0i32;
                                    while let Some(tok) = self.toks.get(i) {
                                        match tok.kind {
                                            TokKind::LParen => depth += 1,
                                            TokKind::RParen => {
                                                depth -= 1;
                                                i += 1;
                                                if depth == 0 {
                                                    break;
                                                }
                                                continue;
                                            }
                                            _ => {}
                                        }
                                        i += 1;
                                    }
                                }
                                while matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Semi)) {
                                    i += 1;
                                }
                            }
                            _ => break,
                        }
                    }
                    _ => break,
                }
            }
            if !saw_marker {
                return false;
            }
            matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Ident(_)))
                && matches!(
                    self.toks.get(i + 1).map(|t| &t.kind),
                    Some(TokKind::ColonColon)
                )
                && matches!(
                    self.toks.get(i + 2).map(|t| &t.kind),
                    Some(TokKind::Ident(n)) if n == Syntax::KW_DISTINCT
                )
        }
    
        /// D-DIST1/D-DIST3/D-CAPBUNDLE1/D-MARKERMOVE1: parse
        /// `[@Numeric] [@Comparable] [@Printable] [@CodableAsBase] Name :: distinct BaseType`
        /// — a stack of zero or more capability-bundle markers, any order.
        pub(super) fn distinct_def(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
        ) -> Result<crate::AST::DistinctDef, Diagnostic> {
            let start = self.peek().span;
            // D-CAPBUNDLE1: zero or more stacked bundle markers (retired `#`
            // spelling on any of them teaches E0062).
            let mut is_numeric = false;
            let mut is_comparable = false;
            let mut comparable_span = None;
            let mut is_printable = false;
            let mut printable_span = None;
            let mut is_codable_as_base = false;
            let mut codable_as_base_span = None;
            let mut invariant_range = None;
            loop {
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                if !(matches!(&self.peek().kind, TokKind::At)
                    && matches!(&self.peek2().kind, TokKind::Ident(n) if Self::is_distinct_prefix_marker(n)))
                {
                    break;
                }
                self.bump(); // consume `@`
                let (attr, attr_span) = self.expect_ident("after the marker sigil")?;
                if attr == Syntax::ATTR_INVARIANT {
                    let (lo, hi, span) = self.parse_invariant_range(attr_span)?;
                    invariant_range = Some((lo, hi, span));
                    continue;
                }
                if attr == Syntax::ATTR_NUMERIC {
                    is_numeric = true;
                } else if attr == Syntax::CONTRACT_BUNDLE_COMPARABLE {
                    is_comparable = true;
                    comparable_span = Some(attr_span);
                } else if attr == Syntax::CONTRACT_BUNDLE_PRINTABLE {
                    is_printable = true;
                    printable_span = Some(attr_span);
                } else if attr == Syntax::CONTRACT_BUNDLE_CODABLE_AS_BASE {
                    is_codable_as_base = true;
                    codable_as_base_span = Some(attr_span);
                }
            }
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            // An `@` here that isn't one of the four bundle markers is a
            // mistake — teach the closed set instead of falling through to a
            // confusing "expected a name" error.
            if matches!(&self.peek().kind, TokKind::At) {
                let attr_span = self.peek2().span;
                let attr = if let TokKind::Ident(n) = &self.peek2().kind {
                    n.clone()
                } else {
                    String::new()
                };
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "`{}{}` isn't a valid attribute on a distinct type declaration",
                        "@",
                        attr
                    ),
                    "only the four capability bundles — `@Numeric`, `@Comparable`, `@Printable`, `@CodableAsBase` — are supported before a distinct type".to_string(),
                    "use one of the four capability bundles, or remove the attribute".to_string(),
                    Some(attr_span),
                ));
            }
            let (name, name_span) = self.expect_ident("as the distinct type name")?;
            // D-BIND4: distinct definitions use `::`.
            match self.peek().kind {
                TokKind::ColonColon => {
                    self.bump();
                }
                _ => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}` after the distinct type name, found {}",
                            Syntax::SIGIL_BIND_IMMUT,
                            describe(&self.peek().kind)
                        ),
                        format!(
                            "a distinct type declaration is `Name {} {} BaseType`",
                            Syntax::SIGIL_BIND_IMMUT,
                            Syntax::KW_DISTINCT
                        ),
                        format!(
                            "write: {} {} {} Int",
                            name,
                            Syntax::SIGIL_BIND_IMMUT,
                            Syntax::KW_DISTINCT
                        ),
                        Some(self.peek().span),
                    ));
                }
            }
            // consume `distinct` keyword
            match &self.peek().kind {
                TokKind::Ident(n) if n == Syntax::KW_DISTINCT => {
                    self.bump();
                }
                other => {
                    let other = other.clone();
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}` here, found {}",
                            Syntax::KW_DISTINCT,
                            describe(&other)
                        ),
                        format!(
                            "a distinct type declaration is `Name {} {} BaseType`",
                            Syntax::SIGIL_BIND_IMMUT,
                            Syntax::KW_DISTINCT
                        ),
                        format!(
                            "write: {} {} {} Int",
                            name,
                            Syntax::SIGIL_BIND_IMMUT,
                            Syntax::KW_DISTINCT
                        ),
                        Some(self.peek().span),
                    ));
                }
            }
            let (base, base_ty_span) = self.type_()?;
            let base_span = base_ty_span;
            // D-RANGETYPE1: an optional literal range constraint right after the
            // base type — `distinct Int(0..10)`. `..` is inclusive (S22).
            let range = if matches!(self.peek().kind, TokKind::LParen) {
                let open = self.bump().span;
                let (lo, lo_span) = self.expect_range_bound_int("as the range's lower bound")?;
                self.expect(TokKind::DotDot, "between the range's bounds")?;
                let (hi, hi_span) = self.expect_range_bound_int("as the range's upper bound")?;
                let close = self.peek().span;
                self.expect(TokKind::RParen, "to close the range constraint")?;
                let range_span = Span::new(open.start, close.end);
                if lo > hi {
                    self.diags.push(Diagnostic::error(
                        "E0137",
                        format!("this range is empty — {} is after {}", lo, hi),
                        "a range's low bound must not be greater than its high bound".to_string(),
                        format!(
                            "write `{}..{}` (swap the bounds), or fix the values",
                            hi, lo
                        ),
                        Some(Span::new(lo_span.start, hi_span.end)),
                    ));
                }
                Some((lo, hi, range_span))
            } else {
                invariant_range
            };
            self.expect(TokKind::Semi, "after a distinct type declaration")?;
            let end = self.toks[self.pos - 1].span.end;
            Ok(crate::AST::DistinctDef {
                is_pub,
                is_package_pub,
                is_numeric,
                is_comparable,
                comparable_span,
                is_printable,
                printable_span,
                is_codable_as_base,
                codable_as_base_span,
                name,
                name_span,
                base,
                base_span,
                range,
                span: Span::new(start.start, end),
            })
        }
    
        /// D-RANGETYPE1: a plain (non-negative — S34 doesn't lex a leading `-` into
        /// the literal) integer literal used as one bound of a `distinct
        /// Base(lo..hi)` range constraint.
        fn expect_range_bound_int(&mut self, where_: &str) -> Result<(i64, Span), Diagnostic> {
            match self.peek().kind {
                TokKind::Int(n, _) => {
                    let span = self.bump().span;
                    Ok((n, span))
                }
                _ => Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected a whole number {}, found {}",
                        where_,
                        describe(&self.peek().kind)
                    ),
                    "a range constraint's bounds are literal whole numbers".to_string(),
                    "write a plain integer, e.g. `0..10`".to_string(),
                    Some(self.peek().span),
                )),
            }
        }
    
        /// D-REFINE1: first shipped `@Invariant` prover accepts a quoted linear
        /// integer range over the reserved value name:
        /// `@Invariant("value >= 0 && value < 4")`.
        fn parse_invariant_range(&mut self, attr_span: Span) -> Result<(i64, i64, Span), Diagnostic> {
            let open = self.peek().span;
            self.expect(TokKind::LParen, "after `@Invariant`")?;
            let text_span = self.peek().span;
            let text = match &self.peek().kind {
                TokKind::Str(parts) => string_literal_value(parts)?,
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("expected invariant text, found {}", describe(other)),
                        "`@Invariant` takes a quoted linear integer bound over `value`".to_string(),
                        "write `@Invariant(\"value >= 0 && value < 10\")`".to_string(),
                        Some(self.peek().span),
                    ));
                }
            };
            self.bump();
            let close = self.peek().span;
            self.expect(TokKind::RParen, "after the invariant text")?;
            let span = Span::new(open.start, close.end);
            match parse_invariant_bounds(&text) {
                Some((lo, hi)) if lo <= hi => Ok((lo, hi, span)),
                Some((lo, hi)) => Err(Diagnostic::error(
                    "E0137",
                    format!("this invariant range is empty — {} is after {}", lo, hi),
                    "a refinement's low bound must not be greater than its high bound".to_string(),
                    "fix the `@Invariant` bounds".to_string(),
                    Some(text_span),
                )),
                None => Err(Diagnostic::error(
                    "E0003",
                    "`@Invariant` only supports linear integer bounds over `value`".to_string(),
                    "the first D-REFINE1 prover accepts comparisons joined with `&&`".to_string(),
                    "write `value >= lo && value < hi`, `lo <= value && value <= hi`, or `value == n`".to_string(),
                    Some(attr_span),
                )),
            }
        }
    
        /// D-TYPEALIAS1: `alias Name<T, E> = T ? E;`
        pub(super) fn type_alias_def(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
        ) -> Result<crate::AST::TypeAliasDef, Diagnostic> {
            let start = self.bump().span; // `alias`
            let (name, name_span) = self.expect_ident("after `alias`")?;
            let type_params = self.parse_opt_type_params()?;
            self.expect(TokKind::Eq, "in a type alias declaration")?;
            let (target, target_span) = self.type_()?;
            self.expect(TokKind::Semi, "after a type alias declaration")?;
            let end = self.toks[self.pos - 1].span.end;
            Ok(crate::AST::TypeAliasDef {
                is_pub,
                is_package_pub,
                name,
                name_span,
                type_params,
                target,
                target_span,
                span: Span::new(start.start, end),
            })
        }
    
        // --- unit families (D-QUAL3) --------------------------------------------
    
        /// D-QUAL3 (ratified 2026-06-24): true when `@UnitFamily(` is at the cursor.
        /// Token stream: `@ UnitFamily (`.
        pub(super) fn at_unit_family_def(&self) -> bool {
            matches!(&self.peek().kind, TokKind::At)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_UNIT_FAMILY)
                && matches!(&self.peek3().kind, TokKind::LParen)
        }
    
        /// D-QUAL3: parse `@UnitFamily(Family) { m1, m2, … }`. Each member mints a
        /// `@Numeric` distinct type erasing to `Float` (lowered in sema/codegen).
        pub(super) fn unit_family_def(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
        ) -> Result<crate::AST::UnitFamilyDef, Diagnostic> {
            let start = self.peek().span;
            self.expect(TokKind::At, "before `UnitFamily`")?;
            // consume the `UnitFamily` marker ident
            let (marker, _) = self.expect_ident("after `@`")?;
            debug_assert_eq!(marker, Syntax::ATTR_UNIT_FAMILY);
            self.expect(TokKind::LParen, "after `@UnitFamily`")?;
            let (family, family_span) = self.expect_ident("as the unit family name")?;
            self.expect(TokKind::RParen, "after the unit family name")?;
            self.expect(TokKind::LBrace, "to open the unit family member list")?;
            let mut members: Vec<(String, Span)> = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                let (member, member_span) = self.expect_ident("as a unit family member")?;
                members.push((member, member_span));
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect(TokKind::RBrace, "to close the unit family member list")?;
            // The closing `}` ends the item; the lexer inserts a synthetic `;`.
            let end = self.toks[self.pos - 1].span.end;
            Ok(crate::AST::UnitFamilyDef {
                is_pub,
                is_package_pub,
                family,
                family_span,
                members,
                span: Span::new(start.start, end),
            })
        }
    
        // --- layout attribute (D-REPRC1) ----------------------------------------
    
        /// D-REPRC1: true when `#layout(…) struct` or `#layout(…) pub struct` is at
        /// the cursor. Token stream: `# layout ( variant ) [struct | pub]`.
        pub(super) fn at_layout_struct(&self) -> bool {
            if !matches!(&self.peek().kind, TokKind::At) {
                return false;
            }
            if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_LAYOUT) {
                return false;
            }
            // peek3 must be `(`
            matches!(&self.peek3().kind, TokKind::LParen)
        }
    
        /// D-REPRC1 / D-SOA1: parse `@Layout(variant) [pub] struct Name { … }`.
        /// `c` (C-compatible) and `columnar` (struct-of-arrays) are supported;
        /// `packed`, `align` parse-and-error; the partial form `columnar: f, g`
        /// (D-SOA2B) is rejected (deferred post-v1).
        pub(super) fn layout_type_def(
            &mut self,
            outer_is_pub: bool,
        ) -> Result<crate::AST::Item, Diagnostic> {
            let attr_start = self.peek().span;
            self.bump(); // consume `@`
            let (attr_name, attr_name_span) = self.expect_ident("after `@`")?;
            debug_assert_eq!(attr_name, Syntax::ATTR_LAYOUT);
            self.expect(TokKind::LParen, "after `@Layout`")?;
            let (variant, variant_span) = self.expect_ident("inside `@Layout(…)`")?;
            let mut tag_width = None;
            if variant == Syntax::LAYOUT_C && matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                let (label, label_span) = if matches!(self.peek().kind, TokKind::KwTag) {
                    let span = self.bump().span;
                    ("tag".to_string(), span)
                } else {
                    self.expect_ident("after `,` in `@Layout(c, …)`")?
                };
                if label != "tag" {
                    return Err(Diagnostic::error("E1105", format!("`{label}` isn't a C enum layout option"),
                        "D-REPRC2 supports only the enum tag width override here.".to_string(),
                        "Write `@Layout(c, tag: U8)` or use `@Layout(c)`.".to_string(), Some(label_span)));
                }
                self.expect(TokKind::Colon, "after `tag` in `@Layout(c, tag: …)`")?;
                tag_width = Some(self.expect_ident("for the C enum tag width")?);
            }
            // D-SOA2B: partial columnar (`#layout(columnar: x, y)`) is deferred — a
            // `:` after the variant is the partial form. Reject with a clear message.
            if variant == Syntax::LAYOUT_COLUMNAR && matches!(&self.peek().kind, TokKind::Colon) {
                let colon_span = self.peek().span;
                return Err(Diagnostic::error(
                    "E1109",
                    "partial `@Layout(columnar: …)` isn't supported yet".to_string(),
                    "v1 supports whole-struct columnar only — every field becomes a column".to_string(),
                    "write `@Layout(columnar)` to convert the whole struct".to_string(),
                    Some(Span::new(variant_span.start, colon_span.end)),
                ));
            }
            let layout = match variant.as_str() {
                v if v == Syntax::LAYOUT_C => Some(crate::AST::StructLayout::C),
                v if v == Syntax::LAYOUT_COLUMNAR => Some(crate::AST::StructLayout::Columnar),
                v if v == Syntax::LAYOUT_PACKED || v == Syntax::LAYOUT_ALIGN => {
                    return Err(Diagnostic::error(
                        "E1105",
                        format!("`@Layout({})` is reserved and not yet supported", v),
                        "the supported variants are `c` (C-compatible) and `columnar` (struct-of-arrays)".to_string(),
                        "use `@Layout(c)` or `@Layout(columnar)`, or omit `@Layout` for the default".to_string(),
                        Some(variant_span),
                    ));
                }
                _ => {
                    return Err(Diagnostic::error(
                        "E1105",
                        format!("`@Layout({})` isn't a known layout variant", variant),
                        "the supported variants are `c` (C-compatible) and `columnar` (struct-of-arrays)".to_string(),
                        "write `@Layout(c)` or `@Layout(columnar)`".to_string(),
                        Some(variant_span),
                    ));
                }
            };
            let attr_end = self.peek().span;
            self.expect(TokKind::RParen, "to close `@Layout(…)`")?;
            let attr_span = Span::new(attr_start.start, attr_end.end);
            // Consume optional semicolons (newline-inserted) before `struct`/`pub`.
            while matches!(&self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let is_pub = outer_is_pub
                || if matches!(&self.peek().kind, TokKind::KwPub) {
                    self.bump();
                    true
                } else {
                    false
                };
            let _ = attr_name_span;
            if matches!(self.peek().kind, TokKind::KwEnum) {
                if layout != Some(crate::AST::StructLayout::C) {
                    return Err(Diagnostic::error("E1105", "Only C layout applies to enums.".to_string(),
                        "Columnar layout describes struct collections, not enum representation.".to_string(),
                        "Use `@Layout(c)` on this enum.".to_string(), Some(variant_span)));
                }
                let mut def = self.enum_def_after_pub(is_pub, false)?;
                let mut args = vec![crate::AST::Expr::Ident("c".to_string(), variant_span)];
                if let Some((width, span)) = tag_width { args.push(crate::AST::Expr::Ident(width, span)); }
                def.type_markers.push(crate::AST::Marker { name: Syntax::ATTR_LAYOUT.to_string(), name_span: attr_name_span, args, span: attr_span, ct: None });
                Ok(crate::AST::Item::Enum(def))
            } else {
                if tag_width.is_some() {
                    return Err(Diagnostic::error("E1105", "A tag width applies only to enums.".to_string(),
                        "Structs have fields and padding but no discriminant tag.".to_string(),
                        "Remove `tag: …`, or put this layout on an enum.".to_string(), Some(attr_span)));
                }
                let mut def = self.struct_def_after_pub(is_pub)?;
                def.layout = layout;
                def.layout_span = Some(attr_span);
                Ok(crate::AST::Item::Struct(def))
            }
        }
    
        // --- published-schema marker + migration blocks (D-MIGRATE1) -----------
    
        /// D-MIGRATE1 / D-MARKERMOVE1 (ratified 2026-06-22 / 2026-07-01): true when
        /// `@PublishedSchema struct` or `@PublishedSchema pub struct` is at the
        /// cursor. Also matches the retired `@PublishedSchema` spelling so
        /// `published_schema_struct_def` can teach E0062.
        /// Note: the lexer inserts a `Semi` after an identifier at end-of-line, so the
        /// token stream is `@ PublishedSchema [Semi] struct` — we check peek4 (pos+3)
        /// when peek3 is a `Semi`, or peek3 when the marker is on the same line.
        pub(super) fn at_published_schema_struct(&self) -> bool {
            if !matches!(&self.peek().kind, TokKind::At) {
                return false;
            }
            if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_PUBLISHED_SCHEMA) {
                return false;
            }
            // peek3 may be Semi (newline after marker) or KwStruct/KwPub (same-line)
            let peek3 = &self.peek3().kind;
            if matches!(peek3, TokKind::KwStruct | TokKind::KwPub) {
                return true;
            }
            if matches!(peek3, TokKind::Semi) {
                // look one further
                let peek4 = &self.toks[(self.pos + 3).min(self.toks.len() - 1)].kind;
                return matches!(peek4, TokKind::KwStruct | TokKind::KwPub);
            }
            false
        }
    
        /// D-LIN1 (ratified 2026-06-21): true when `@SingleUse struct` / `@SingleUse enum`
        /// (with an optional newline `Semi` after the marker) is at the cursor. The
        /// `pub @SingleUse …` case is handled inline in the `KwPub` dispatch arm.
        pub(super) fn at_single_use_type(&self) -> bool {
            if !matches!(&self.peek().kind, TokKind::At) {
                return false;
            }
            if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_SINGLE_USE) {
                return false;
            }
            let peek3 = &self.peek3().kind;
            if matches!(peek3, TokKind::KwStruct | TokKind::KwEnum | TokKind::KwPub) {
                return true;
            }
            if matches!(peek3, TokKind::Semi) {
                let peek4 = &self.toks[(self.pos + 3).min(self.toks.len() - 1)].kind;
                return matches!(peek4, TokKind::KwStruct | TokKind::KwEnum | TokKind::KwPub);
            }
            false
        }
    
        /// D-LIN1 (ratified 2026-06-21): parse `@SingleUse [pub] (struct|enum) Name { … }`.
        /// Sets the `is_single_use` flag on the produced struct/enum so sema can enforce
        /// must-consume-once (E0140/E0141) and no-alias (E0142). The marker erases in
        /// codegen (I3).
        pub(super) fn single_use_type_def(&mut self, outer_is_pub: bool) -> Result<crate::AST::Item, Diagnostic> {
            let attr_start = self.peek().span;
            self.bump(); // consume `@`
            let (attr, attr_name_span) = self.expect_ident("after `@`")?;
            debug_assert_eq!(attr, Syntax::ATTR_SINGLE_USE);
            let attr_span = Span::new(attr_start.start, attr_name_span.end);
            // The lexer may insert a `Semi` after the marker identifier when the type
            // keyword is on the next line. Consume it so the next token is the keyword.
            while matches!(&self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            // optional `pub` after the marker (the `pub @SingleUse` form already ate it)
            let want_pub = outer_is_pub || matches!(&self.peek().kind, TokKind::KwPub);
            if !outer_is_pub && matches!(&self.peek().kind, TokKind::KwPub) {
                self.bump();
            }
            match self.peek().kind {
                TokKind::KwStruct => {
                    let mut def = self.struct_def_after_pub(want_pub)?;
                    def.is_single_use = true;
                    def.single_use_span = Some(attr_span);
                    Ok(crate::AST::Item::Struct(def))
                }
                TokKind::KwEnum => {
                    let mut def = self.enum_def_after_pub(want_pub, false)?;
                    def.is_single_use = true;
                    def.single_use_span = Some(attr_span);
                    Ok(crate::AST::Item::Enum(def))
                }
                _ => Err(Diagnostic::error(
                    "E0003",
                    "`@SingleUse` marks a `struct` or `enum`".to_string(),
                    "the marker says values of this type must be used exactly once — only a type can carry that rule".to_string(),
                    "write `@SingleUse struct Name { … }` or `@SingleUse enum Name { … }`".to_string(),
                    Some(attr_span),
                )),
            }
        }
    
        /// D-MUSTUSE1 (c18iwxqx) / D-MARKERMOVE1: true when `@MustUse struct` /
        /// `@MustUse enum` is at the cursor. Also matches the retired `@MustUse`
        /// spelling so `must_use_type_def` can teach E0062.
        pub(super) fn at_must_use_type(&self) -> bool {
            if !matches!(&self.peek().kind, TokKind::At) {
                return false;
            }
            if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_MUST_USE) {
                return false;
            }
            let peek3 = &self.peek3().kind;
            if matches!(peek3, TokKind::KwStruct | TokKind::KwEnum | TokKind::KwPub) {
                return true;
            }
            if matches!(peek3, TokKind::Semi) {
                let peek4 = &self.toks[(self.pos + 3).min(self.toks.len() - 1)].kind;
                return matches!(peek4, TokKind::KwStruct | TokKind::KwEnum | TokKind::KwPub);
            }
            false
        }
    
        /// D-MUSTUSE1/D-MARKERMOVE1: parse `@MustUse [pub] (struct|enum) Name { … }`.
        pub(super) fn must_use_type_def(&mut self, outer_is_pub: bool) -> Result<crate::AST::Item, Diagnostic> {
            let attr_start = self.peek().span;
            self.bump(); // consume `@`
            let (attr, attr_name_span) = self.expect_ident("after the marker sigil")?;
            debug_assert_eq!(attr, Syntax::ATTR_MUST_USE);
            let attr_span = Span::new(attr_start.start, attr_name_span.end);
            while matches!(&self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let want_pub = outer_is_pub || matches!(&self.peek().kind, TokKind::KwPub);
            if !outer_is_pub && matches!(&self.peek().kind, TokKind::KwPub) {
                self.bump();
            }
            match self.peek().kind {
                TokKind::KwStruct => {
                    let mut def = self.struct_def_after_pub(want_pub)?;
                    def.is_must_use = true;
                    def.must_use_span = Some(attr_span);
                    Ok(crate::AST::Item::Struct(def))
                }
                TokKind::KwEnum => {
                    let mut def = self.enum_def_after_pub(want_pub, false)?;
                    def.is_must_use = true;
                    def.must_use_span = Some(attr_span);
                    Ok(crate::AST::Item::Enum(def))
                }
                _ => Err(Diagnostic::error(
                    "E0003",
                    "`@MustUse` marks a `struct` or `enum`".to_string(),
                    "the marker says values of this type must not be silently ignored — only a type can carry that rule".to_string(),
                    "write `@MustUse struct Name { … }` or `@MustUse enum Name { … }`".to_string(),
                    Some(attr_span),
                )),
            }
        }
    
        /// D-MIGRATE1: true when `migration <TypeName> {` is at the cursor (contextual).
        pub(super) fn at_migration_block(&self) -> bool {
            matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_MIGRATION)
                && matches!(&self.peek2().kind, TokKind::Ident(_))
                && matches!(&self.peek3().kind, TokKind::LBrace)
        }
    
        /// D-MIGRATE1/D-MARKERMOVE1 (ratified 2026-06-22 / 2026-07-01): parse
        /// `@PublishedSchema [pub] struct Name { … }`. The retired `@PublishedSchema`
        /// spelling teaches E0062.
        pub(super) fn published_schema_struct_def(
            &mut self,
            outer_is_pub: bool,
        ) -> Result<crate::AST::StructDef, Diagnostic> {
            let attr_start = self.peek().span;
            self.bump(); // consume `@`
            let (attr, attr_name_span) = self.expect_ident("after the marker sigil")?;
            if attr != Syntax::ATTR_PUBLISHED_SCHEMA {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "`{}{}` isn't a valid attribute on a struct declaration",
                        "@",
                        attr
                    ),
                    "only `@PublishedSchema` is supported here".to_string(),
                    "write `@PublishedSchema` before the struct".to_string(),
                    Some(attr_name_span),
                ));
            }
            let attr_span = Span::new(attr_start.start, attr_name_span.end);
            // The lexer may insert a `Semi` after the marker identifier when `struct` is
            // on the next line. Consume it so the next token is `struct` or `pub`.
            while matches!(&self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            // optional `pub` after the marker
            let is_pub = outer_is_pub
                || if matches!(&self.peek().kind, TokKind::KwPub) {
                    self.bump();
                    true
                } else {
                    false
                };
            let mut def = self.struct_def_after_pub(is_pub)?;
            def.is_published_schema = true;
            def.published_schema_span = Some(attr_span);
            Ok(def)
        }
    
        /// Parse `struct Name { … }` given that pub/is_pub was already handled.
        /// Factors out the body of `struct_def` when the `pub` keyword is already consumed.
        pub(super) fn struct_def_after_pub(&mut self, is_pub: bool) -> Result<crate::AST::StructDef, Diagnostic> {
            self.struct_def_after_pub_pkg(is_pub, false)
        }
    
        pub(super) fn struct_def_after_pub_pkg(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
        ) -> Result<crate::AST::StructDef, Diagnostic> {
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
                // D-SHAPE2: field rules share one `@[…]` group.
                if self.at_marker_list() {
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
                } else if self.at_validate_block() {
                    let (stmts, span) = self.validate_block()?;
                    validate_block = stmts;
                    validate_span = Some(span);
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
            Ok(crate::AST::StructDef {
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
                validate_block,
                validate_span,
            })
        }

}
