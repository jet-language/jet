use super::super::{Diagnostic, EnumLitArg, Expr, Parser, Pattern, Span, Syntax, TokKind, Type};

impl<'a> Parser<'a> {
        /// S37/S38: `[a, b]` or `["k": v]` or `[]`. D-EMPTYLIT1: `[]` is the one
        /// empty-collection spelling — type-directed, list or map decided by the
        /// expected-type context (sema). `[:]` is gone; `[` immediately followed
        /// by `:` falls through to an ordinary expression-expected parse error.
        pub(super) fn list_or_map_lit(&mut self) -> Result<Expr, Diagnostic> {
            let open = self.bump().span;
            if matches!(self.peek().kind, TokKind::RBracket) {
                let close = self.bump().span;
                return Ok(Expr::ListLit(Vec::new(), Span::new(open.start, close.end)));
            }
            let first = self.list_elem()?;
            if matches!(self.peek().kind, TokKind::Colon) {
                self.bump();
                let value = self.expr()?;
                let mut entries = vec![(first, value)];
                while matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                    self.bump();
                    if matches!(self.peek().kind, TokKind::RBracket) {
                        break;
                    }
                    let key = self.expr()?;
                    self.expect(TokKind::Colon, "between a map key and its value")?;
                    let val = self.expr()?;
                    entries.push((key, val));
                }
                self.expect(TokKind::RBracket, "to close the map literal")?;
                let close = self.toks[self.pos - 1].span;
                return Ok(Expr::MapLit(entries, Span::new(open.start, close.end)));
            }
            let mut elems = vec![first];
            while matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                self.bump();
                if matches!(self.peek().kind, TokKind::RBracket) {
                    break;
                }
                elems.push(self.list_elem()?);
            }
            self.expect(TokKind::RBracket, "to close the list literal")?;
            let close = self.toks[self.pos - 1].span;
            Ok(Expr::ListLit(elems, Span::new(open.start, close.end)))
        }
    
        /// S37 / D-VARIADIC1: one list element — either `expr` or `...expr` spread.
        fn list_elem(&mut self) -> Result<Expr, Diagnostic> {
            if matches!(self.peek().kind, TokKind::DotDotDot) {
                let start = self.bump().span.start;
                let inner = self.expr()?;
                let end = inner.span().end;
                return Ok(Expr::Spread(Box::new(inner), Span::new(start, end)));
            }
            self.expr()
        }
    
        pub(super) fn struct_lit_after_name(
            &mut self,
            type_name: String,
            type_args: Vec<Type>,
            start_span: Span,
        ) -> Result<Expr, Diagnostic> {
            self.expect(TokKind::LBrace, "to open a struct literal")?;
            let mut fields = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                let (field, field_span) = self.expect_ident("for a field name")?;
                let value = if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    self.expr()?
                } else {
                    // S77: field punning — `{ name }` means `{ name: name }`.
                    Expr::Ident(field.clone(), field_span)
                };
                fields.push((field, field_span, value));
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                }
            }
            let end = self.peek().span.end;
            self.bump();
            Ok(Expr::StructLit {
                type_name,
                type_args,
                import_ns: None,
                as_trait: None,
                fields,
                inferred: false,
                span: Span::new(start_span.start, end),
            })
        }
    
        pub(super) fn struct_lit_after_import(
            &mut self,
            alias: String,
            type_name: String,
            start: usize,
        ) -> Result<Expr, Diagnostic> {
            self.expect(TokKind::LBrace, "to open a struct literal")?;
            let mut fields = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                let (field, field_span) = self.expect_ident("for a field name")?;
                let value = if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    self.expr()?
                } else {
                    // S77: field punning — `{ name }` means `{ name: name }`.
                    Expr::Ident(field.clone(), field_span)
                };
                fields.push((field, field_span, value));
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                }
            }
            let end = self.peek().span.end;
            self.bump();
            Ok(Expr::StructLit {
                type_name,
                type_args: Vec::new(),
                import_ns: Some(alias),
                as_trait: None,
                fields,
                inferred: false,
                span: Span::new(start, end),
            })
        }
    
        /// D-DOTCTOR1: inferred struct lit `.{ field: val, … }`.
        /// The leading `.` was already consumed. Parses `{ field: val, … }`.
        pub(super) fn struct_lit_inferred(&mut self, dot_start: usize) -> Result<Expr, Diagnostic> {
            self.expect(TokKind::LBrace, "to open an inferred struct literal")?;
            let mut fields = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                let (field, field_span) = self.expect_ident("for a field name")?;
                let value = if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    self.expr()?
                } else {
                    Expr::Ident(field.clone(), field_span)
                };
                fields.push((field, field_span, value));
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                }
            }
            let end = self.peek().span.end;
            self.bump();
            Ok(Expr::StructLit {
                type_name: String::new(),
                type_args: Vec::new(),
                import_ns: None,
                as_trait: None,
                fields,
                inferred: true,
                span: Span::new(dot_start, end),
            })
        }
    
        /// D-UITREE1/D-DOTCTOR1: named-payload enum literal fields `{ field: val, … }`.
        /// The leading `.` before `{` was already consumed; parses the brace body and
        /// returns `EnumLitArg::Named` entries (S77 field punning applies, matching
        /// struct dot-construction).
        pub(super) fn enum_lit_named_fields(&mut self) -> Result<(Vec<EnumLitArg>, usize), Diagnostic> {
            self.expect(TokKind::LBrace, "to open a named enum-variant literal")?;
            let mut args = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                let (label, label_span) = self.expect_ident("for a variant field name")?;
                let value = if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    self.expr()?
                } else {
                    // S77: field punning — `{ name }` means `{ name: name }`.
                    Expr::Ident(label.clone(), label_span)
                };
                args.push(EnumLitArg::Named { label, expr: value });
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                }
            }
            let end = self.peek().span.end;
            self.bump();
            Ok((args, end))
        }
    
        /// S31: try to parse a pattern on the right of `==`.
        ///
        /// Only unambiguous pattern spellings: `None`, `Val(n)`, and
        /// `Variant(bindings)`. A bare identifier is ordinary value equality
        /// (`a == b`); unit-variant tests like `light == Red` are resolved in
        /// sema when `Red` is not a variable but is a variant on the subject.
        pub(in crate::Parser) fn try_pattern_rhs(&mut self) -> Result<Option<Pattern>, Diagnostic> {
            match &self.peek().kind {
                // D-PARSESTR1: an interpolation literal in pattern position —
                // `subject == "prefix-{id:Int}-suffix"`. Each hole must reduce to
                // a bare identifier with an optional `:Type` suffix; anything
                // else (an arbitrary expression) isn't part of this decision, so
                // fall through to ordinary `Expr::Str` parsing instead.
                TokKind::Str(_) => {
                    return self.try_str_match_pattern();
                }
                TokKind::KwNull => {
                    let span = self.bump().span;
                    return Ok(Some(Pattern::Absent(span)));
                }
                TokKind::KwOk => {
                    let start = self.bump().span;
                    self.expect(TokKind::LParen, "after `ok`")?;
                    let (binding, binding_span) = self.expect_ident("inside `ok(...)`")?;
                    self.expect(TokKind::RParen, "after the binding in `ok(...)`")?;
                    return Ok(Some(Pattern::Ok {
                        binding,
                        span: Span::new(start.start, binding_span.end),
                    }));
                }
                TokKind::KwErr => {
                    let start = self.bump().span;
                    self.expect(TokKind::LParen, "after `err`")?;
                    let (binding, binding_span) = self.expect_ident("inside `err(...)`")?;
                    self.expect(TokKind::RParen, "after the binding in `err(...)`")?;
                    return Ok(Some(Pattern::Err {
                        binding,
                        span: Span::new(start.start, binding_span.end),
                    }));
                }
                TokKind::Ident(name) if name == Syntax::LIT_VALUE => {
                    let start = self.bump().span;
                    self.expect(TokKind::LParen, &format!("after `{}`", Syntax::LIT_VALUE))?;
                    let (binding, binding_span) =
                        self.expect_ident(&format!("inside `{}(...)`", Syntax::LIT_VALUE))?;
                    self.expect(
                        TokKind::RParen,
                        &format!("after the binding in `{}(...)`", Syntax::LIT_VALUE),
                    )?;
                    return Ok(Some(Pattern::Present {
                        binding,
                        span: Span::new(start.start, binding_span.end),
                    }));
                }
                // D-DESTRUCT1: struct-shaped dispatch arm head:
                // `.{ kind: "page", title, .. } -> ...`.
                TokKind::Dot
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::LBrace)
                    ) =>
                {
                    return self.struct_pattern_rhs().map(Some);
                }
                TokKind::Ident(variant)
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::LParen)
                    ) =>
                {
                    let variant = variant.clone();
                    let span = self.peek().span;
                    self.bump();
                    self.bump();
                    let mut bindings: Vec<crate::AST::PatSlot> = Vec::new();
                    if !matches!(self.peek().kind, TokKind::RParen) {
                        loop {
                            // D-PATW: `_` in payload slot = wildcard (ignore field, bind nothing).
                            let slot = if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::PAT_WILDCARD_SLOT)
                            {
                                self.bump();
                                crate::AST::PatSlot::Wildcard
                            } else if let TokKind::Int(lo_val) = &self.peek().kind.clone() {
                                // D-PATR: `lo..hi` range in payload slot.
                                let lo = *lo_val;
                                self.bump();
                                if matches!(self.peek().kind, TokKind::DotDot) {
                                    self.bump(); // consume `..`
                                    if let TokKind::Int(hi_val) = &self.peek().kind.clone() {
                                        let hi = *hi_val;
                                        self.bump();
                                        crate::AST::PatSlot::Range { lo, hi }
                                    } else {
                                        return Err(Diagnostic::error(
                                            "E0003",
                                            "expected an integer after `..` in a range pattern"
                                                .to_string(),
                                            "range patterns need both ends: `lo..hi`".to_string(),
                                            "write `0..100` for an inclusive range".to_string(),
                                            Some(self.peek().span),
                                        ));
                                    }
                                } else {
                                    return Err(Diagnostic::error(
                                        "E0003",
                                        "expected `..` after the lower bound of a range pattern"
                                            .to_string(),
                                        "range patterns need `lo..hi` syntax".to_string(),
                                        "write `0..100` for an inclusive range".to_string(),
                                        Some(self.peek().span),
                                    ));
                                }
                            } else {
                                let (b, _) = self.expect_ident("for a pattern binding")?;
                                crate::AST::PatSlot::Bind(b)
                            };
                            bindings.push(slot);
                            if matches!(self.peek().kind, TokKind::RParen) {
                                break;
                            }
                            self.expect(TokKind::Comma, "between pattern bindings")?;
                        }
                    }
                    self.expect(TokKind::RParen, "after pattern bindings")?;
                    let end = self.toks[self.pos.saturating_sub(1)].span.end;
                    // D-PATO: check for `| AltVariant(bindings)` after this pattern.
                    let base = Pattern::Variant {
                        variant,
                        bindings,
                        span: Span::new(span.start, end),
                    };
                    if matches!(self.peek().kind, TokKind::Pipe) {
                        // Collect or-pattern alternatives.
                        let or_start = span;
                        let mut alts = vec![base];
                        while matches!(self.peek().kind, TokKind::Pipe) {
                            self.bump(); // consume `|`
                                         // Parse the next alternative (must be a Variant pattern).
                            if let Some(alt) = self.try_pattern_rhs()? {
                                alts.push(alt);
                            } else {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    "expected a variant pattern after `|` in an or-pattern".to_string(),
                                    "or-patterns join two variant patterns: `A(x) | B(x)`".to_string(),
                                    "write a variant name with bindings after `|`".to_string(),
                                    Some(self.peek().span),
                                ));
                            }
                        }
                        let or_end = self.toks[self.pos.saturating_sub(1)].span.end;
                        return Ok(Some(Pattern::Or(alts, Span::new(or_start.start, or_end))));
                    }
                    return Ok(Some(base));
                }
                // D-ENUMDOT1 (ratified 2026-06-26): `.Variant` or `.Variant(binding)` in
                // pattern position. Reads as "a member of the inferred enum", resolving the
                // bare-name-vs-variable ambiguity from S31. Normalises to the same
                // Pattern::Variant AST node — no `leading_dot` field added.
                TokKind::Dot
                    if matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::Ident(_))
                    ) =>
                {
                    let dot_span = self.bump().span; // consume `.`
                    let (mut variant, mut variant_span) =
                        self.expect_ident("after `.` in a variant pattern")?;
                    // D-TAG1: a dotted path in pattern position — `.Fire.Burn` names a
                    // leaf, `.Fire` alone names a group (matches its whole subtree).
                    while matches!(self.peek().kind, TokKind::Dot)
                        && matches!(&self.peek2().kind, TokKind::Ident(n) if n.chars().next().map_or(false, |c| c.is_uppercase()))
                    {
                        self.bump(); // consume `.`
                        let (seg, seg_span) = self.expect_ident("after `.` in a variant pattern")?;
                        variant = format!("{variant}.{seg}");
                        variant_span = seg_span;
                    }
                    let span_start = dot_span.start;
                    let (bindings, end) = if matches!(self.peek().kind, TokKind::LParen) {
                        self.bump(); // consume `(`
                        let mut bindings: Vec<crate::AST::PatSlot> = Vec::new();
                        if !matches!(self.peek().kind, TokKind::RParen) {
                            loop {
                                let slot = if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::PAT_WILDCARD_SLOT)
                                {
                                    self.bump();
                                    crate::AST::PatSlot::Wildcard
                                } else if let TokKind::Int(lo_val) = &self.peek().kind.clone() {
                                    let lo = *lo_val;
                                    self.bump();
                                    if matches!(self.peek().kind, TokKind::DotDot) {
                                        self.bump();
                                        if let TokKind::Int(hi_val) = &self.peek().kind.clone() {
                                            let hi = *hi_val;
                                            self.bump();
                                            crate::AST::PatSlot::Range { lo, hi }
                                        } else {
                                            return Err(Diagnostic::error(
                                                "E0003",
                                                "expected an integer after `..` in a range pattern"
                                                    .to_string(),
                                                "range patterns need both ends: `lo..hi`".to_string(),
                                                "write `0..100` for an inclusive range".to_string(),
                                                Some(self.peek().span),
                                            ));
                                        }
                                    } else {
                                        return Err(Diagnostic::error(
                                            "E0003",
                                            "expected `..` after the lower bound of a range pattern"
                                                .to_string(),
                                            "range patterns need `lo..hi` syntax".to_string(),
                                            "write `0..100` for an inclusive range".to_string(),
                                            Some(self.peek().span),
                                        ));
                                    }
                                } else {
                                    let (b, _) = self.expect_ident("for a pattern binding")?;
                                    crate::AST::PatSlot::Bind(b)
                                };
                                bindings.push(slot);
                                if matches!(self.peek().kind, TokKind::RParen) {
                                    break;
                                }
                                self.expect(TokKind::Comma, "between pattern bindings")?;
                            }
                        }
                        self.expect(TokKind::RParen, "after pattern bindings")?;
                        let end = self.toks[self.pos.saturating_sub(1)].span.end;
                        (bindings, end)
                    } else {
                        // Unit variant with dot: `.Empty`
                        (vec![], variant_span.end)
                    };
                    let base = Pattern::Variant {
                        variant,
                        bindings,
                        span: Span::new(span_start, end),
                    };
                    if matches!(self.peek().kind, TokKind::Pipe) {
                        let or_start = Span::new(span_start, end);
                        let mut alts = vec![base];
                        while matches!(self.peek().kind, TokKind::Pipe) {
                            self.bump(); // consume `|`
                            if let Some(alt) = self.try_pattern_rhs()? {
                                alts.push(alt);
                            } else {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    "expected a variant pattern after `|` in an or-pattern".to_string(),
                                    "or-patterns join two variant patterns: `A(x) | B(x)`".to_string(),
                                    "write a variant name with bindings after `|`".to_string(),
                                    Some(self.peek().span),
                                ));
                            }
                        }
                        let or_end = self.toks[self.pos.saturating_sub(1)].span.end;
                        return Ok(Some(Pattern::Or(alts, Span::new(or_start.start, or_end))));
                    }
                    Ok(Some(base))
                }
                _ => Ok(None),
            }
        }
    
}
