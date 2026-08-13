use super::super::{Diagnostic, Parser, Span, StrTokPart, Syntax, TokKind, describe};
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
    
        /// D-DIST3 / D-CAPBUNDLE1 / D-VERDICT-732-1 (formerly D-MARKERMOVE1) (ratified 2026-06-20 /
        /// 2026-07-01): true when a stack of one or more capability-bundle
        /// markers (`#Numeric`, `#Comparable`, `#Printable`, `#CodableAsBase`,
        /// any order, retired `#` spelling included so `distinct_def` can teach
        /// E0062) precedes `Name :: distinct` at the cursor. The `#Numeric`-only
        /// sibling of the old `at_numeric_distinct_def` predicate, generalized to
        /// the four fixed bundles.
        pub(super) fn at_bundle_distinct_def(&self) -> bool {
            let mut i = self.pos;
            let mut saw_marker = false;
            loop {
                match self.toks.get(i).map(|t| &t.kind) {
                    Some(TokKind::Hash) => {
                        match self.toks.get(i + 1).map(|t| &t.kind) {
                            Some(TokKind::Ident(_)) => {
                                saw_marker = true;
                                i += 2;
                                i = match Self::skip_balanced_parens(&self.toks, i) {
                                    Some(next) => next,
                                    None => break,
                                };
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
    
        /// D-DIST1/D-DIST3/D-CAPBUNDLE1/D-VERDICT-732-1 (formerly D-MARKERMOVE1): parse
        /// `[#Numeric] [#Comparable] [#Printable] [#CodableAsBase] Name :: distinct BaseType`
        /// — a stack of zero or more capability-bundle markers, any order.
        pub(super) fn distinct_def(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
        ) -> Result<crate::AST::DistinctDef, Diagnostic> {
            let start = self.peek().span;
            // D-CAPBUNDLE1: zero or more stacked bundle markers (retired `#`
            // spelling on any of them teaches E0062).
            let mut derives = Vec::new();
            let mut invariant_range = None;
            let mut invariant = None;
            let mut type_markers = Vec::new();
            let mut marker_count = 0usize;
            loop {
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                if !(matches!(&self.peek().kind, TokKind::Hash)
                    && matches!(&self.peek2().kind, TokKind::Ident(_)))
                {
                    break;
                }
                marker_count += 1;
                let marker = self.parse_registered_marker_at_site(crate::Policy::RuleSite::Type)?;
                if marker.name == Syntax::MARKER_INVARIANT {
                    let (bounds, span, text) = self.parse_invariant_range(marker.clone())?;
                    invariant_range = bounds.map(|(lo, hi)| (lo, hi, span));
                    invariant = text.map(|text| (text, span));
                    type_markers.push(marker);
                    continue;
                }
                if marker.name == Syntax::MARKER_NUMERIC
                    || marker.name == Syntax::MARKER_BUNDLE_COMPARABLE
                    || marker.name == Syntax::MARKER_BUNDLE_PRINTABLE
                {
                    derives.push((marker.name.clone(), marker.name_span));
                } else if marker.name == Syntax::MARKER_BUNDLE_CODABLE_AS_BASE {
                    derives.push((crate::Generics::ENCODE.to_string(), marker.name_span));
                    derives.push((crate::Generics::DECODE.to_string(), marker.name_span));
                }
                type_markers.push(marker);
            }
            if marker_count > 1 {
                let span = Span::new(
                    type_markers[0].span.start,
                    type_markers.last().unwrap().span.end,
                );
                let mut diagnostic = Diagnostic::error(
                    "E0999",
                    "multiple markers belong in one bracket list".to_string(),
                    "two or more markers use one `#[A, B]` group".to_string(),
                    "replace the bare stack with one bracket list".to_string(),
                    Some(span),
                );
                if type_markers.iter().all(|marker| marker.args.is_empty()) {
                    diagnostic.set_structured_edit(crate::Diagnostics::TextEdit {
                        span,
                        new_text: format!(
                            "#[{}]",
                            type_markers
                                .iter()
                                .map(|marker| marker.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    });
                }
                self.diags.push(diagnostic);
            }
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            // An `@` here that isn't one of the four bundle markers is a
            // mistake — teach the closed set instead of falling through to a
            // confusing "expected a name" error.
            if matches!(&self.peek().kind, TokKind::Hash) {
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
                    "only the four capability bundles — `#Numeric`, `#Comparable`, `#Printable`, `#CodableAsBase` — are supported before a distinct type".to_string(),
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
            let span = Span::new(start.start, end);
            for marker in &type_markers {
                self.bind_rule_fact(
                    marker.name_span,
                    Some(span),
                    crate::Policy::RuleSite::Type,
                );
            }
            Ok(crate::AST::DistinctDef {
                is_pub,
                is_package_pub,
                type_markers,
                derives,
                quantity: None,
                name,
                name_span,
                base,
                base_span,
                range,
                invariant,
                span,
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
    
        /// D-REFINE1: first shipped `#Invariant` prover accepts a quoted linear
        /// integer range over the reserved value name:
        /// `#Invariant("value >= 0 && value < 4")`.
        pub(in crate::Parser) fn parse_invariant_range(
            &mut self,
            marker: crate::AST::Marker,
        ) -> Result<(Option<(i64, i64)>, Span, Option<String>), Diagnostic> {
            let arguments = self.bound_registered_rule_arguments(&marker)?;
            let Some(invariant) = arguments.parameter(0) else {
                return Err(crate::Policy::marker_argument_shape_error(Syntax::MARKER_INVARIANT, marker.span));
            };
            let text = match invariant {
                crate::AST::Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
                    crate::AST::StrPart::Lit(text) => Some(text.clone()),
                    crate::AST::StrPart::Interp(..) => None,
                },
                _ => None,
            };
            let span = marker.span;
            let text_span = marker.args[0].span();
            let Some(text) = text else {
                return Ok((None, span, None));
            };
            match parse_invariant_bounds(&text) {
                Some((lo, hi)) if lo <= hi => Ok((Some((lo, hi)), span, Some(text))),
                Some((lo, hi)) => Err(Diagnostic::error(
                    "E0137",
                    format!("this invariant range is empty — {} is after {}", lo, hi),
                    "a refinement's low bound must not be greater than its high bound".to_string(),
                    "fix the `#Invariant` bounds".to_string(),
                    Some(text_span),
                )),
                None => Err(Diagnostic::error(
                    "E0003",
                    "`#Invariant` only supports linear integer bounds over `value`".to_string(),
                    "the first D-REFINE1 prover accepts comparisons joined with `&&`".to_string(),
                    "write `value >= lo && value < hi`, `lo <= value && value <= hi`, or `value == n`".to_string(),
                    Some(span),
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
    
        /// D-QUAL3 (ratified 2026-06-24): true when `#UnitFamily(` is at the cursor.
        /// Token stream: `@ UnitFamily (`.
        pub(super) fn at_unit_family_def(&self) -> bool {
            matches!(&self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_UNIT_FAMILY)
                && matches!(&self.peek3().kind, TokKind::LParen)
        }
    
        /// D-QUAL3: parse `#UnitFamily(Family) { m1, m2, … }`. Each member mints a
        /// `#Numeric` distinct type erasing to `Float` (lowered in sema/codegen).
        pub(super) fn unit_family_def(
            &mut self,
            is_pub: bool,
            is_package_pub: bool,
        ) -> Result<crate::AST::UnitFamilyDef, Diagnostic> {
            let marker = self.parse_registered_marker_at_site(crate::Policy::RuleSite::Type)?;
            let arguments = self.bound_registered_rule_arguments(&marker)?;
            let Some(crate::AST::Expr::Ident(family, family_span)) = arguments.parameter(0) else {
                return Err(crate::Policy::marker_argument_shape_error(Syntax::MARKER_UNIT_FAMILY, marker.span));
            };
            let family = family.clone();
            let family_span = *family_span;
            let dimension = arguments.parameter(1).map(|value| match value {
                crate::AST::Expr::Ident(name, span)
                    if name == Syntax::UNIT_FAMILY_DIMENSION_FIELD =>
                {
                    Ok(crate::AST::UnitDimensionDecl::Base(*span))
                }
                crate::AST::Expr::Ident(_, _)
                | crate::AST::Expr::Field(_, _, _)
                | crate::AST::Expr::Binary(crate::AST::BinOp::Mul | crate::AST::BinOp::Div, _, _, _) => {
                    Ok(crate::AST::UnitDimensionDecl::Derived(value.clone()))
                }
                _ => Err(Diagnostic::error(
                    "E0003",
                    "invalid dimension declaration".to_string(),
                    "a dimension is a new axis or a product of existing dimension names"
                        .to_string(),
                    "write `dimension` or `dimension: Mass * Length / Time / Time`"
                        .to_string(),
                    Some(value.span()),
                )),
            }).transpose()?;
            let mut base = if let Some(value) = arguments.parameter(2) {
                let crate::AST::Expr::Ident(base, base_span) = value else {
                    return Err(crate::Policy::marker_argument_shape_error(Syntax::MARKER_UNIT_FAMILY, value.span()));
                };
                Some((base.clone(), *base_span))
            } else {
                None
            };
            if matches!(dimension, Some(crate::AST::UnitDimensionDecl::Derived(_)))
                && base.is_none()
            {
                return Err(Diagnostic::error(
                    "E0003",
                    format!("derived dimension `{family}` has no canonical base"),
                    "every derived dimension needs one member whose scale is exactly one"
                        .to_string(),
                    "add `base: member_name` and keep that member's scale at 1 with offset 0"
                        .to_string(),
                    Some(family_span),
                ));
            }
            self.expect(TokKind::LBrace, "to open the unit family member list")?;
            let mut members = Vec::new();
            let mut has_conversion_metadata = false;
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                let (member, member_span) = self.expect_ident("as a unit family member")?;
                let mut scale = crate::AST::UnitRatio::integer(1);
                let mut scale_provenance = crate::AST::UnitScaleProvenance::Rational;
                let mut offset = crate::AST::UnitRatio::zero();
                let mut saw_scale = false;
                let mut saw_offset = false;
                if matches!(self.peek().kind, TokKind::LParen) {
                    self.bump();
                    while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
                        let (field, field_span) = self.expect_ident("as unit metadata")?;
                        self.expect(TokKind::Colon, "after unit metadata")?;
                        match field.as_str() {
                            Syntax::UNIT_FAMILY_SCALE_FIELD if !saw_scale => {
                                let (value, provenance) = self.unit_family_scale()?;
                                scale = value;
                                scale_provenance = provenance;
                                saw_scale = true;
                            }
                            Syntax::UNIT_FAMILY_OFFSET_FIELD if !saw_offset => {
                                offset = self.unit_family_ratio()?;
                                saw_offset = true;
                            }
                            Syntax::UNIT_FAMILY_SCALE_FIELD | Syntax::UNIT_FAMILY_OFFSET_FIELD => {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    format!("unit metadata `{field}` appears twice"),
                                    "each conversion fact has one exact value".to_string(),
                                    format!("keep one `{field}: ...` entry"),
                                    Some(field_span),
                                ));
                            }
                            _ => {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    format!("unknown unit metadata `{field}`"),
                                    "unit members accept exact `scale` and `offset` facts"
                                        .to_string(),
                                    "use `scale: numerator/denominator` or `offset: numerator/denominator`"
                                        .to_string(),
                                    Some(field_span),
                                ));
                            }
                        }
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                        } else {
                            break;
                        }
                    }
                    self.expect(TokKind::RParen, "after unit metadata")?;
                }
                if scale == crate::AST::UnitRatio::zero() {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("unit `{member}` has a zero scale"),
                        "a unit conversion must be invertible".to_string(),
                        "use a nonzero exact ratio for `scale`".to_string(),
                        Some(member_span),
                    ));
                }
                has_conversion_metadata |= saw_scale || saw_offset;
                members.push(crate::AST::UnitFamilyMember {
                    name: member,
                    name_span: member_span,
                    scale,
                    scale_provenance,
                    offset,
                });
                while matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                    self.bump();
                }
            }
            self.expect(TokKind::RBrace, "to close the unit family member list")?;
            // D-QUANTITY-DECL1=A: `base` documents a default of "first member".
            // A bare family (no `dimension`, no member conversion metadata —
            // currency, plain tags) stays fully nominal and never defaults: no
            // unit fact registers, per D-DIMENSION-OPEN1's "without a base,
            // members stay unrelated nominal types with no conversion." Once a
            // family claims a dimension or writes `scale`/`offset` on any
            // member, it has opted into conversion and the default applies.
            if base.is_none() && (dimension.is_some() || has_conversion_metadata) {
                if let Some(first) = members.first() {
                    base = Some((first.name.clone(), first.name_span));
                }
            }
            if let Some((base_name, base_span)) = &base {
                let Some(member) = members.iter().find(|member| member.name == *base_name) else {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("base `{base_name}` is not a member of `{family}`"),
                        "the canonical base is one member of its closed unit family".to_string(),
                        format!("add `{base_name}` to the family or name an existing member"),
                        Some(*base_span),
                    ));
                };
                if member.scale != crate::AST::UnitRatio::integer(1)
                    || member.offset != crate::AST::UnitRatio::zero()
                {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("base `{base_name}` changes its own scale or offset"),
                        "the canonical base always has scale 1 and offset 0".to_string(),
                        "remove metadata from the base member".to_string(),
                        Some(member.name_span),
                    ));
                }
            }
            // The closing `}` ends the item; the lexer inserts a synthetic `;`.
            let end = self.toks[self.pos - 1].span.end;
            self.bind_rule_fact(
                marker.name_span,
                Some(Span::new(marker.span.start, end)),
                crate::Policy::RuleSite::Type,
            );
            Ok(crate::AST::UnitFamilyDef {
                is_pub,
                is_package_pub,
                family,
                family_span,
                dimension,
                resolved_dimension: None,
                resolved_owner: None,
                base,
                members,
                span: Span::new(marker.span.start, end),
            })
        }

        fn unit_family_ratio(&mut self) -> Result<crate::AST::UnitRatio, Diagnostic> {
            let (numerator, numerator_span) = self.unit_family_integer("as conversion metadata")?;
            let (denominator, error_span) = if matches!(self.peek().kind, TokKind::Slash) {
                self.bump();
                let (denominator, denominator_span) =
                    self.unit_family_integer("as a ratio denominator")?;
                (denominator, denominator_span)
            } else {
                ("1".to_string(), numerator_span)
            };
            crate::AST::UnitRatio::parse_source(&numerator, &denominator).map_err(|reason| {
                Diagnostic::error(
                    "E0003",
                    format!("invalid unit conversion ratio: {reason}"),
                    "unit conversion metadata must be a finite exact ratio".to_string(),
                    "use a nonzero integer denominator".to_string(),
                    Some(error_span),
                )
            })
        }

        fn unit_family_scale(
            &mut self,
        ) -> Result<(crate::AST::UnitRatio, crate::AST::UnitScaleProvenance), Diagnostic> {
            if matches!(&self.peek().kind, TokKind::Ident(name) if name == "pi") {
                let pi_span = self.bump().span;
                self.expect(TokKind::Slash, "after `pi` in a symbolic unit scale")?;
                let (denominator, denominator_span) =
                    self.unit_family_integer("as the symbolic scale denominator")?;
                let denominator = crate::AST::UnitRatio::parse_source(&denominator, "1")
                    .map_err(|reason| unit_scale_error(reason, denominator_span))?;
                if denominator == crate::AST::UnitRatio::zero() {
                    return Err(unit_scale_error(
                        "symbolic scale denominator is zero".to_string(),
                        denominator_span,
                    ));
                }
                let pi = decimal_ratio(&std::f64::consts::PI.to_string())
                    .map_err(|reason| unit_scale_error(reason, pi_span))?;
                let effective = pi
                    .div(&denominator)
                    .map_err(|reason| unit_scale_error(reason, pi_span))?;
                return Ok((
                    effective,
                    crate::AST::UnitScaleProvenance::SymbolicPi {
                        numerator: crate::AST::UnitRatio::integer(1),
                        denominator,
                    },
                ));
            }

            let kind = match &self.peek().kind {
                TokKind::Ident(name) if name == "conventional" || name == "measured" => {
                    name.clone()
                }
                _ => {
                    return self.unit_family_ratio().map(|ratio| {
                        (ratio, crate::AST::UnitScaleProvenance::Rational)
                    });
                }
            };
            let kind_span = self.bump().span;
            self.expect(TokKind::LParen, "after the unit scale provenance")?;
            let value = self.unit_family_decimal("as the scale value")?;
            let effective =
                decimal_ratio(&value).map_err(|reason| unit_scale_error(reason, kind_span))?;
            self.expect(TokKind::Comma, "after the unit scale value")?;

            let provenance = if kind == "conventional" {
                self.expect_unit_scale_label("source")?;
                let source = self.unit_family_plain_string("as the convention source")?;
                crate::AST::UnitScaleProvenance::Conventional { value, source }
            } else {
                self.expect_unit_scale_label("uncertainty")?;
                let standard_uncertainty =
                    self.unit_family_decimal("as the standard uncertainty")?;
                self.expect(TokKind::Comma, "after the standard uncertainty")?;
                self.expect_unit_scale_label("source")?;
                let source = self.unit_family_plain_string("as the measurement source")?;
                crate::AST::UnitScaleProvenance::Measured {
                    central_value: value,
                    standard_uncertainty,
                    source,
                }
            };
            self.expect(TokKind::RParen, "after the unit scale provenance")?;
            Ok((effective, provenance))
        }

        fn expect_unit_scale_label(&mut self, expected: &str) -> Result<(), Diagnostic> {
            let (label, span) = self.expect_ident("as unit scale provenance metadata")?;
            if label != expected {
                return Err(unit_scale_error(
                    format!("expected `{expected}:`, found `{label}:`"),
                    span,
                ));
            }
            self.expect(TokKind::Colon, "after unit scale provenance metadata")
        }

        fn unit_family_decimal(&mut self, expected: &str) -> Result<String, Diagnostic> {
            let sign = if matches!(self.peek().kind, TokKind::Minus) {
                self.bump();
                "-"
            } else {
                ""
            };
            let token = self.bump();
            let value = match token.kind {
                TokKind::Int(_, raw) => raw,
                TokKind::Float(value) if value.is_finite() => value.to_string(),
                _ => {
                    return Err(unit_scale_error(
                        format!("expected a finite decimal {expected}"),
                        token.span,
                    ));
                }
            };
            Ok(format!("{sign}{value}"))
        }

        fn unit_family_plain_string(&mut self, expected: &str) -> Result<String, Diagnostic> {
            let token = self.bump();
            let TokKind::Str(parts) = token.kind else {
                return Err(unit_scale_error(format!("expected a plain string {expected}"), token.span));
            };
            if parts.len() != 1 {
                return Err(unit_scale_error(
                    "unit scale sources cannot contain interpolation".to_string(),
                    token.span,
                ));
            }
            let StrTokPart::Lit(value) = &parts[0] else {
                return Err(unit_scale_error(
                    "unit scale sources cannot contain interpolation".to_string(),
                    token.span,
                ));
            };
            Ok(value.clone())
        }

        fn unit_family_integer(&mut self, expected: &str) -> Result<(String, Span), Diagnostic> {
            let sign = if matches!(self.peek().kind, TokKind::Minus) {
                Some(self.bump().span)
            } else {
                None
            };
            let token = self.bump();
            let TokKind::Int(_, raw) = token.kind else {
                let span = sign.map_or(token.span, |sign| Span::new(sign.start, token.span.end));
                return Err(Diagnostic::error(
                    "E0003",
                    format!("expected an exact integer {expected}"),
                    "unit conversion metadata uses exact integer ratios, not floating point"
                        .to_string(),
                    "write an integer or `numerator/denominator`".to_string(),
                    Some(span),
                ));
            };
            let span = sign.map_or(token.span, |sign| Span::new(sign.start, token.span.end));
            let mut source = raw;
            if sign.is_some() {
                source.insert(0, '-');
            }
            Ok((source, span))
        }
    
        // --- layout attribute (D-REPRC1) ----------------------------------------
    
        /// D-REPRC1: true when `#layout(…) struct` or `#layout(…) pub struct` is at
        /// the cursor. Token stream: `# layout ( variant ) [struct | pub]`.
        pub(super) fn at_layout_struct(&self) -> bool {
            if !matches!(&self.peek().kind, TokKind::Hash) {
                return false;
            }
            if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_LAYOUT) {
                return false;
            }
            // peek3 must be `(`
            matches!(&self.peek3().kind, TokKind::LParen)
        }
    
        /// D-REPRC1 / D-SOA1: parse `#Layout(variant) [pub] struct Name { … }`.
        /// `c` (C-compatible) and `columnar` (struct-of-arrays) are supported;
        /// `packed`, `align` parse-and-error; the partial form `columnar: f, g`
        /// (D-SOA2B) is rejected (deferred post-v1).
        pub(super) fn layout_type_def(
            &mut self,
            outer_is_pub: bool,
        ) -> Result<crate::AST::Item, Diagnostic> {
            let marker = self.parse_registered_marker_at_site(crate::Policy::RuleSite::Type)?;
            let arguments = self.bound_registered_rule_arguments(&marker)?;
            let Some(crate::AST::Expr::Ident(variant, variant_span)) = arguments.parameter(0) else {
                return Err(crate::Policy::marker_argument_shape_error(Syntax::MARKER_LAYOUT, marker.span));
            };
            let variant = variant.clone();
            let variant_span = *variant_span;
            let mut tag_width = None;
            if let Some(value) = arguments.parameter(1) {
                let crate::AST::Expr::Ident(width, width_span) = value else {
                    return Err(crate::Policy::marker_argument_shape_error(Syntax::MARKER_LAYOUT, value.span()));
                };
                tag_width = Some((width.clone(), *width_span));
            }
            let layout = match variant.as_str() {
                v if v == Syntax::LAYOUT_C => Some(crate::AST::StructLayout::C),
                v if v == Syntax::LAYOUT_COLUMNAR => Some(crate::AST::StructLayout::Columnar),
                v if v == Syntax::LAYOUT_PACKED || v == Syntax::LAYOUT_ALIGN => {
                    return Err(Diagnostic::error(
                        "E1105",
                        format!("`#Layout({})` is reserved and not yet supported", v),
                        "the supported variants are `c` (C-compatible) and `columnar` (struct-of-arrays)".to_string(),
                        "use `#Layout(c)` or `#Layout(columnar)`, or omit `#Layout` for the default".to_string(),
                        Some(variant_span),
                    ));
                }
                _ => {
                    return Err(Diagnostic::error(
                        "E1105",
                        format!("`#Layout({})` isn't a known layout variant", variant),
                        "the supported variants are `c` (C-compatible) and `columnar` (struct-of-arrays)".to_string(),
                        "write `#Layout(c)` or `#Layout(columnar)`".to_string(),
                        Some(variant_span),
                    ));
                }
            };
            let attr_span = marker.span;
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
            if matches!(self.peek().kind, TokKind::KwEnum) {
                if layout != Some(crate::AST::StructLayout::C) {
                    return Err(Diagnostic::error("E1105", "Only C layout applies to enums.".to_string(),
                        "Columnar layout describes struct collections, not enum representation.".to_string(),
                        "Use `#Layout(c)` on this enum.".to_string(), Some(variant_span)));
                }
                let mut def = self.enum_def_after_pub(is_pub, false)?;
                let marker_name_span = marker.name_span;
                def.type_markers.push(marker);
                let item = crate::AST::Item::Enum(def);
                let target = match &item {
                    crate::AST::Item::Enum(def) => def.span,
                    _ => unreachable!(),
                };
                self.bind_rule_fact(
                    marker_name_span,
                    Some(target),
                    crate::Policy::RuleSite::Type,
                );
                Ok(item)
            } else {
                if tag_width.is_some() {
                    return Err(Diagnostic::error("E1105", "A tag width applies only to enums.".to_string(),
                        "Structs have fields and padding but no discriminant tag.".to_string(),
                        "Remove `tag: …`, or put this layout on an enum.".to_string(), Some(attr_span)));
                }
                let mut def = self.struct_def_after_pub(is_pub)?;
                def.layout = layout;
                def.layout_span = Some(attr_span);
                let marker_name_span = marker.name_span;
                def.type_markers.push(marker);
                let item = crate::AST::Item::Struct(def);
                let target = match &item {
                    crate::AST::Item::Struct(def) => def.span,
                    _ => unreachable!(),
                };
                self.bind_rule_fact(
                    marker_name_span,
                    Some(target),
                    crate::Policy::RuleSite::Type,
                );
                Ok(item)
            }
        }
    
        // --- published-schema marker + migration blocks (D-MIGRATE1) -----------
    
        /// D-MIGRATE1 / D-VERDICT-732-1 (formerly D-MARKERMOVE1) (ratified 2026-06-22 / 2026-07-01): true when
        /// `#PublishedSchema struct` or `#PublishedSchema pub struct` is at the
        /// cursor. Also matches the retired `#PublishedSchema` spelling so
        /// `published_schema_struct_def` can teach E0062.
        /// Note: the lexer inserts a `Semi` after an identifier at end-of-line, so the
        /// token stream is `@ PublishedSchema [Semi] struct` — we check peek4 (pos+3)
        /// when peek3 is a `Semi`, or peek3 when the marker is on the same line.
        pub(super) fn at_published_schema_struct(&self) -> bool {
            if !matches!(&self.peek().kind, TokKind::Hash) {
                return false;
            }
            if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_PUBLISHED_SCHEMA) {
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
    
        /// D-LIN1 (ratified 2026-06-21): true when `#SingleUse struct` / `#SingleUse enum`
        /// (with an optional newline `Semi` after the marker) is at the cursor. The
        /// `pub #SingleUse …` case is handled inline in the `KwPub` dispatch arm.
        pub(super) fn at_single_use_type(&self) -> bool {
            if !matches!(&self.peek().kind, TokKind::Hash) {
                return false;
            }
            if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_SINGLE_USE) {
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
    
        /// D-LIN1 (ratified 2026-06-21): parse `#SingleUse [pub] (struct|enum) Name { … }`.
        /// Sets the `is_single_use` flag on the produced struct/enum so sema can enforce
        /// must-consume-once (E0140/E0141) and no-alias (E0142). The marker erases in
        /// codegen (I3).
        pub(super) fn single_use_type_def(&mut self, outer_is_pub: bool) -> Result<crate::AST::Item, Diagnostic> {
            let attr_start = self.peek().span;
            self.bump(); // consume `@`
            let (attr, attr_name_span) = self.expect_ident("after `@`")?;
            debug_assert_eq!(attr, Syntax::MARKER_SINGLE_USE);
            let attr_span = Span::new(attr_start.start, attr_name_span.end);
            let marker = crate::AST::Marker {
                name: attr,
                negated: false,
                name_span: attr_name_span,
                args: Vec::new(),
                arg_labels: Vec::new(),
                span: attr_span,
                ct: None,
            };
            // The lexer may insert a `Semi` after the marker identifier when the type
            // keyword is on the next line. Consume it so the next token is the keyword.
            while matches!(&self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            // optional `pub` after the marker (the `pub #SingleUse` form already ate it)
            let want_pub = outer_is_pub || matches!(&self.peek().kind, TokKind::KwPub);
            if !outer_is_pub && matches!(&self.peek().kind, TokKind::KwPub) {
                self.bump();
            }
            match self.peek().kind {
                TokKind::KwStruct => {
                    let mut def = self.struct_def_after_pub(want_pub)?;
                    def.is_single_use = true;
                    def.single_use_span = Some(attr_span);
                    def.type_markers.push(marker);
                    Ok(crate::AST::Item::Struct(def))
                }
                TokKind::KwEnum => {
                    let mut def = self.enum_def_after_pub(want_pub, false)?;
                    def.is_single_use = true;
                    def.single_use_span = Some(attr_span);
                    def.type_markers.push(marker);
                    Ok(crate::AST::Item::Enum(def))
                }
                _ => Err(Diagnostic::error(
                    "E0003",
                    "`#SingleUse` marks a `struct` or `enum`".to_string(),
                    "the marker says values of this type must be used exactly once — only a type can carry that rule".to_string(),
                    "write `#SingleUse struct Name { … }` or `#SingleUse enum Name { … }`".to_string(),
                    Some(attr_span),
                )),
            }
        }
    
        /// D-MUSTUSE1 (c18iwxqx) / D-VERDICT-732-1 (formerly D-MARKERMOVE1): true when `#MustUse struct` /
        /// `#MustUse enum` is at the cursor. Also matches the retired `#MustUse`
        /// spelling so `must_use_type_def` can teach E0062.
        pub(super) fn at_must_use_type(&self) -> bool {
            if !matches!(&self.peek().kind, TokKind::Hash) {
                return false;
            }
            if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_MUST_USE) {
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
    
        /// D-MUSTUSE1/D-VERDICT-732-1 (formerly D-MARKERMOVE1): parse `#MustUse [pub] (struct|enum) Name { … }`.
        pub(super) fn must_use_type_def(&mut self, outer_is_pub: bool) -> Result<crate::AST::Item, Diagnostic> {
            let attr_start = self.peek().span;
            self.bump(); // consume `@`
            let (attr, attr_name_span) = self.expect_ident("after the marker sigil")?;
            debug_assert_eq!(attr, Syntax::MARKER_MUST_USE);
            let attr_span = Span::new(attr_start.start, attr_name_span.end);
            let marker = crate::AST::Marker {
                name: attr,
                negated: false,
                name_span: attr_name_span,
                args: Vec::new(),
                arg_labels: Vec::new(),
                span: attr_span,
                ct: None,
            };
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
                    def.type_markers.push(marker);
                    Ok(crate::AST::Item::Struct(def))
                }
                TokKind::KwEnum => {
                    let mut def = self.enum_def_after_pub(want_pub, false)?;
                    def.is_must_use = true;
                    def.must_use_span = Some(attr_span);
                    def.type_markers.push(marker);
                    Ok(crate::AST::Item::Enum(def))
                }
                _ => Err(Diagnostic::error(
                    "E0003",
                    "`#MustUse` marks a `struct` or `enum`".to_string(),
                    "the marker says values of this type must not be silently ignored — only a type can carry that rule".to_string(),
                    "write `#MustUse struct Name { … }` or `#MustUse enum Name { … }`".to_string(),
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
    
        /// D-MIGRATE1/D-VERDICT-732-1 (formerly D-MARKERMOVE1) (ratified 2026-06-22 / 2026-07-01): parse
        /// `#PublishedSchema [pub] struct Name { … }`. The retired `#PublishedSchema`
        /// spelling teaches E0062.
        pub(super) fn published_schema_struct_def(
            &mut self,
            outer_is_pub: bool,
        ) -> Result<crate::AST::StructDef, Diagnostic> {
            let attr_start = self.peek().span;
            self.bump(); // consume `@`
            let (attr, attr_name_span) = self.expect_ident("after the marker sigil")?;
            if attr != Syntax::MARKER_PUBLISHED_SCHEMA {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "`{}{}` isn't a valid attribute on a struct declaration",
                        "@",
                        attr
                    ),
                    "only `#PublishedSchema` is supported here".to_string(),
                    "write `#PublishedSchema` before the struct".to_string(),
                    Some(attr_name_span),
                ));
            }
            let attr_span = Span::new(attr_start.start, attr_name_span.end);
            let marker = crate::AST::Marker {
                name: attr,
                negated: false,
                name_span: attr_name_span,
                args: Vec::new(),
                arg_labels: Vec::new(),
                span: attr_span,
                ct: None,
            };
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
            def.type_markers.push(marker);
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
            let derives = Vec::new();
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
                // D-SHAPE2: one field rule is bare; two or more share `#[…]`.
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
                        if m.name == crate::Syntax::MARKER_REDACT {
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
                    return Err(self.retired_derive_line());
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

}

fn unit_scale_error(reason: String, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0003",
        format!("invalid unit scale: {reason}"),
        "unit scales preserve whether a value is exact, symbolic, conventional, or measured"
            .to_string(),
        "use an exact ratio, `pi / n`, `conventional(...)`, or `measured(...)`"
            .to_string(),
        Some(span),
    )
}

fn decimal_ratio(source: &str) -> Result<crate::AST::UnitRatio, String> {
    let source = source.replace('_', "");
    let (mantissa, exponent) = source
        .split_once(['e', 'E'])
        .map_or((source.as_str(), 0), |(mantissa, exponent)| {
            (mantissa, exponent.parse::<i32>().unwrap_or(i32::MIN))
        });
    if exponent == i32::MIN {
        return Err("invalid decimal exponent".to_string());
    }
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches(['-', '+']);
    let (whole, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, ""), |parts| parts);
    if whole.is_empty() && fraction.is_empty() {
        return Err("invalid decimal value".to_string());
    }
    let mut numerator = format!("{whole}{fraction}");
    if numerator.is_empty() {
        numerator.push('0');
    }
    let power = exponent - i32::try_from(fraction.len()).map_err(|_| "decimal is too long")?;
    let denominator = if power >= 0 {
        numerator.extend(std::iter::repeat_n('0', power as usize));
        "1".to_string()
    } else {
        format!("1{}", "0".repeat(power.unsigned_abs() as usize))
    };
    if negative {
        numerator.insert(0, '-');
    }
    crate::AST::UnitRatio::parse_source(&numerator, &denominator)
}

#[cfg(test)]
mod dimension_tests {
    use crate::{AST, Lexer, Parser};

    #[test]
    fn standard_units_source_parses_with_unique_members_and_provenance() {
        let source = include_str!("../../../../jet-codegen/src/Prelude/Units.jet");
        let (tokens, diagnostics) = Lexer::lex_generated(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let program = Parser::parse(&tokens).expect("standard units must parse");
        let families = program
            .items
            .iter()
            .filter_map(|item| match item {
                AST::Item::UnitFamily(family) => Some(family),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(families.len(), 32);
        for family in &families {
            let unique = family
                .members
                .iter()
                .map(|member| member.name.as_str())
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(unique.len(), family.members.len(), "{}", family.family);
            assert!(family
                .members
                .iter()
                .all(|member| member.scale != AST::UnitRatio::zero()));
        }
        let provenance = |member: &str| {
            families
                .iter()
                .flat_map(|family| &family.members)
                .find(|candidate| candidate.name == member)
                .unwrap()
                .scale_provenance
                .clone()
        };
        assert!(matches!(
            provenance("degree"),
            AST::UnitScaleProvenance::SymbolicPi { .. }
        ));
        assert!(matches!(
            provenance("dalton"),
            AST::UnitScaleProvenance::Measured { .. }
        ));
        assert!(matches!(
            provenance("mmHg"),
            AST::UnitScaleProvenance::Conventional { .. }
        ));
        let scale = |member: &str| {
            families
                .iter()
                .flat_map(|family| &family.members)
                .find(|candidate| candidate.name == member)
                .unwrap()
                .scale
                .to_string()
        };
        assert_eq!(scale("square_kilometer"), "1000000");
        assert_eq!(scale("cubic_kilometer"), "1000000000");
        assert!(!families
            .iter()
            .flat_map(|family| &family.members)
            .any(|member| member.name == "kilosquare_meter" || member.name == "kilocubic_meter"));
    }
}
