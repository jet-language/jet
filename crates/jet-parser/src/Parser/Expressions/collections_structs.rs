use super::super::{
    Diagnostic, EnumLitArg, Expr, Parser, Pattern, Span, Syntax, TokKind, Token, Type, TypedLitBody,
};

fn leading_dot_variant(kind: &TokKind) -> Option<String> {
    match kind {
        TokKind::Ident(name) if name.chars().next().is_some_and(char::is_uppercase) => {
            Some(name.clone())
        }
        TokKind::KwNull => Some(Syntax::LIT_NULL.to_string()),
        _ => None,
    }
}

/// D-DOTCTOR3: does the brace body start like record fields / field punning?
/// `pos` points at the first token inside `{ … }` (not at `{`).
fn brace_body_looks_like_fields(toks: &[Token], pos: usize) -> bool {
    match toks.get(pos).map(|t| &t.kind) {
        Some(TokKind::RBrace) => true,
        Some(TokKind::Ident(_)) => matches!(
            toks.get(pos + 1).map(|t| &t.kind),
            Some(TokKind::Colon | TokKind::Comma | TokKind::Semi | TokKind::RBrace)
        ),
        _ => false,
    }
}

impl<'a> Parser<'a> {
        /// S37/S38: `[a, b]` or `["k": v]` or `[]`. D-EMPTYLIT1: `[]` is the one
        /// empty-collection spelling — type-directed, list or map decided by the
        /// expected-type context (sema). `[:]` is gone; `[` immediately followed
        /// by `:` falls through to an ordinary expression-expected parse error.
        /// D-DOTCTOR3: `[T].{ … }` / `[T#N].{ … }` / `[K: V].{ … }` is a typed-
        /// literal head, not a list value whose first element is a type name.
        pub(super) fn list_or_map_lit(&mut self) -> Result<Expr, Diagnostic> {
            if let Some(lit) = self.try_typed_lit_from_bracket()? {
                return Ok(lit);
            }
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

        /// D-DOTCTOR3: probe `[Type].{` / `[Type#N].{` / `[K: V].{`.
        fn try_typed_lit_from_bracket(&mut self) -> Result<Option<Expr>, Diagnostic> {
            let save = self.pos;
            let save_diags = self.diags.len();
            if !matches!(self.peek().kind, TokKind::LBracket) {
                return Ok(None);
            }
            // Parse the whole collection type (`[T]`, `[T#N]`, `[K: V]`), not the
            // element alone — bumping `[` then `type_()` would see `U8` as a scalar.
            let parsed = self.type_();
            let Ok((head, head_span)) = parsed else {
                self.pos = save;
                self.diags.truncate(save_diags);
                return Ok(None);
            };
            if !matches!(
                head,
                Type::List(_) | Type::FixedList { .. } | Type::Map { .. }
            ) {
                self.pos = save;
                self.diags.truncate(save_diags);
                return Ok(None);
            }
            if !matches!(self.peek().kind, TokKind::Dot) {
                self.pos = save;
                self.diags.truncate(save_diags);
                return Ok(None);
            }
            self.bump();
            if !matches!(self.peek().kind, TokKind::LBrace) {
                self.pos = save;
                self.diags.truncate(save_diags);
                return Ok(None);
            }
            let body = self.typed_lit_body_for_head(&head)?;
            let end = self.toks[self.pos.saturating_sub(1)].span.end;
            Ok(Some(Expr::TypedLit {
                head: Some(head),
                body,
                span: Span::new(head_span.start, end),
            }))
        }

        /// D-DOTCTOR3: parse `{ … }` body shaped for `head`.
        pub(super) fn typed_lit_body_for_head(
            &mut self,
            head: &Type,
        ) -> Result<TypedLitBody, Diagnostic> {
            self.expect(TokKind::LBrace, "to open a typed literal")?;
            if matches!(self.peek().kind, TokKind::RBrace) {
                self.bump();
                return Ok(TypedLitBody::Empty);
            }
            match head {
                Type::Map { .. } => self.finish_typed_lit_entries(),
                Type::List(_) | Type::FixedList { .. } => self.finish_typed_lit_elements(),
                Type::Named(_) | Type::Apply { .. }
                    if brace_body_looks_like_fields(&self.toks, self.pos) =>
                {
                    let fields = self.finish_struct_fields_already_open()?;
                    Ok(TypedLitBody::Fields(fields))
                }
                _ => {
                    let value = self.expr()?;
                    self.expect(TokKind::RBrace, "to close a typed literal")?;
                    Ok(TypedLitBody::Value(Box::new(value)))
                }
            }
        }

        fn finish_typed_lit_elements(&mut self) -> Result<TypedLitBody, Diagnostic> {
            let mut elems = vec![self.list_elem()?];
            while matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                self.bump();
                if matches!(self.peek().kind, TokKind::RBrace) {
                    break;
                }
                elems.push(self.list_elem()?);
            }
            self.expect(TokKind::RBrace, "to close a typed list literal")?;
            Ok(TypedLitBody::Elements(elems))
        }

        fn finish_typed_lit_entries(&mut self) -> Result<TypedLitBody, Diagnostic> {
            let key = self.expr()?;
            self.expect(TokKind::Colon, "between a map key and its value")?;
            let value = self.expr()?;
            let mut entries = vec![(key, value)];
            while matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                self.bump();
                if matches!(self.peek().kind, TokKind::RBrace) {
                    break;
                }
                let key = self.expr()?;
                self.expect(TokKind::Colon, "between a map key and its value")?;
                let val = self.expr()?;
                entries.push((key, val));
            }
            self.expect(TokKind::RBrace, "to close a typed map literal")?;
            Ok(TypedLitBody::Entries(entries))
        }

        fn finish_struct_fields_already_open(
            &mut self,
        ) -> Result<Vec<(String, Span, Expr)>, Diagnostic> {
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
            self.bump(); // RBrace
            Ok(fields)
        }

        /// D-DOTCTOR3: `Head.{ body }` when `Head` is a type spelling (scalar,
        /// named non-field body, etc.).
        pub(super) fn typed_lit_after_type(
            &mut self,
            head: Type,
            start_span: Span,
        ) -> Result<Expr, Diagnostic> {
            let body = self.typed_lit_body_for_head(&head)?;
            let end = self.toks[self.pos.saturating_sub(1)].span.end;
            Ok(Expr::TypedLit {
                head: Some(head),
                body,
                span: Span::new(start_span.start, end),
            })
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
            // Peek is `{`. Body tokens start at pos+1.
            let body_pos = self.pos + 1;
            let head = if let Some(ty) = crate::AST::numeric_type_from_name(&type_name) {
                Some(ty)
            } else if type_name == Syntax::TYPE_BOOL {
                Some(Type::Bool)
            } else if type_name == Syntax::TYPE_STRING {
                Some(Type::String)
            } else if type_name == Syntax::TYPE_CHAR {
                Some(Type::Char)
            } else {
                None
            };
            // D-DOTCTOR3: builtin scalars always use TypedLit; named heads with a
            // non-field body are one-expression assertions.
            if head.is_some() || !brace_body_looks_like_fields(&self.toks, body_pos) {
                let head = match head {
                    Some(ty) => ty,
                    None if !type_args.is_empty() => Type::Apply {
                        name: type_name,
                        args: type_args,
                    },
                    None => Type::Named(type_name),
                };
                return self.typed_lit_after_type(head, start_span);
            }
            self.expect(TokKind::LBrace, "to open a struct literal")?;
            let fields = self.finish_struct_fields_already_open()?;
            let end = self.toks[self.pos.saturating_sub(1)].span.end;
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
            let fields = self.finish_struct_fields_already_open()?;
            let end = self.toks[self.pos.saturating_sub(1)].span.end;
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
        /// D-DOTCTOR3: also `.{ elems }` / `.{ value }` when the body is not a
        /// record field list — elaborates against the expected type in sema.
        /// The leading `.` was already consumed. Parses `{ … }`.
        pub(super) fn struct_lit_inferred(&mut self, dot_start: usize) -> Result<Expr, Diagnostic> {
            self.expect(TokKind::LBrace, "to open an inferred struct literal")?;
            if matches!(self.peek().kind, TokKind::RBrace) {
                let end = self.bump().span.end;
                return Ok(Expr::StructLit {
                    type_name: String::new(),
                    type_args: Vec::new(),
                    import_ns: None,
                    as_trait: None,
                    fields: Vec::new(),
                    inferred: true,
                    span: Span::new(dot_start, end),
                });
            }
            if brace_body_looks_like_fields(&self.toks, self.pos) {
                let fields = self.finish_struct_fields_already_open()?;
                let end = self.toks[self.pos.saturating_sub(1)].span.end;
                return Ok(Expr::StructLit {
                    type_name: String::new(),
                    type_args: Vec::new(),
                    import_ns: None,
                    as_trait: None,
                    fields,
                    inferred: true,
                    span: Span::new(dot_start, end),
                });
            }
            // Non-field body: elements, a single value, or map entries.
            // Map entries start with a non-ident key then `:`, or ident keys are
            // field-shaped above. Remaining forms are element lists / values.
            let first = self.list_elem()?;
            if matches!(self.peek().kind, TokKind::Colon) {
                // Rare inferred map body `.{ key: val }` where key wasn't a bare
                // Ident (e.g. string key). Parse as entries.
                self.bump();
                let value = self.expr()?;
                let mut entries = vec![(first, value)];
                while matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                    self.bump();
                    if matches!(self.peek().kind, TokKind::RBrace) {
                        break;
                    }
                    let key = self.expr()?;
                    self.expect(TokKind::Colon, "between a map key and its value")?;
                    let val = self.expr()?;
                    entries.push((key, val));
                }
                self.expect(TokKind::RBrace, "to close an inferred map literal")?;
                let end = self.toks[self.pos.saturating_sub(1)].span.end;
                return Ok(Expr::TypedLit {
                    head: None,
                    body: TypedLitBody::Entries(entries),
                    span: Span::new(dot_start, end),
                });
            }
            if matches!(self.peek().kind, TokKind::RBrace) {
                let end = self.bump().span.end;
                return Ok(Expr::TypedLit {
                    head: None,
                    body: TypedLitBody::Value(Box::new(first)),
                    span: Span::new(dot_start, end),
                });
            }
            let mut elems = vec![first];
            while matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                self.bump();
                if matches!(self.peek().kind, TokKind::RBrace) {
                    break;
                }
                elems.push(self.list_elem()?);
            }
            self.expect(TokKind::RBrace, "to close an inferred list literal")?;
            let end = self.toks[self.pos.saturating_sub(1)].span.end;
            Ok(Expr::TypedLit {
                head: None,
                body: TypedLitBody::Elements(elems),
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
            // D-UNIFYLIT1=A: typed pattern heads before bare tokens.
            if let Some(pat) = self.try_bin_match_pattern()? {
                return Ok(Some(pat));
            }
            if let Some(pat) = self.try_string_typed_str_match_pattern()? {
                return Ok(Some(pat));
            }
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
                        binding_span,
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
                            } else if let TokKind::Int(lo_val, _) = &self.peek().kind.clone() {
                                // D-PATR: `lo..hi` range in payload slot.
                                let lo = *lo_val;
                                self.bump();
                                if matches!(self.peek().kind, TokKind::DotDot) {
                                    self.bump(); // consume `..`
                                    if let TokKind::Int(hi_val, _) = &self.peek().kind.clone() {
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
                                let (name, span) = self.expect_ident("for a pattern binding")?;
                                crate::AST::PatSlot::Bind { name, span }
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
                    if self.toks.get(self.pos + 1)
                        .and_then(|token| leading_dot_variant(&token.kind))
                        .is_some() =>
                {
                    let dot_span = self.bump().span; // consume `.`
                    let variant_token = self.bump();
                    let mut variant = leading_dot_variant(&variant_token.kind)
                        .expect("leading-dot variant guard and token must agree");
                    let mut variant_span = variant_token.span;
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
                                } else if let TokKind::Int(lo_val, _) = &self.peek().kind.clone() {
                                    let lo = *lo_val;
                                    self.bump();
                                    if matches!(self.peek().kind, TokKind::DotDot) {
                                        self.bump();
                                        if let TokKind::Int(hi_val, _) = &self.peek().kind.clone() {
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
                                    let (name, span) = self.expect_ident("for a pattern binding")?;
                                    crate::AST::PatSlot::Bind { name, span }
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
                    let span = Span::new(span_start, end);
                    let base = Pattern::Variant { variant, bindings, span };
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
