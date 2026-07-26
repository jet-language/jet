use super::super::{
    Diagnostic, Expr, Parser, Pattern, Span, StrMatchPart, StrTokPart, Syntax, TokKind, Token,
    describe,
};

impl<'a> Parser<'a> {
        /// D-PARSESTR1: try to read the string-literal token at the cursor as a
        /// str-match pattern — `"prefix-{id:Int}-suffix"`. Each hole must reduce
        /// to a bare identifier, optionally followed by `:Type`; any other hole
        /// shape (an arbitrary expression) means this string isn't a pattern, so
        /// the token is left untouched and the caller falls back to ordinary
        /// `Expr::Str` parsing. E0147 (two holes with nothing to split on between
        /// them) is checked here, at parse time, once we've committed.
        pub(super) fn try_str_match_pattern(&mut self) -> Result<Option<Pattern>, Diagnostic> {
            let TokKind::Str(parts) = &self.peek().kind else {
                return Ok(None);
            };
            let parts = parts.clone();
            // I8: a hole-free string literal is plain text equality — ordinary
            // `Expr::Str`/`==`, not a pattern (one way to mean it). Only a string
            // with at least one `{hole}` is a str-match pattern.
            if !parts.iter().any(|p| matches!(p, StrTokPart::Interp(_))) {
                return Ok(None);
            }
            // First pass: every hole must be a bare identifier, optionally
            // followed by `:Type`, with nothing else in the sub-token-stream.
            for part in &parts {
                if let StrTokPart::Interp(toks) = part {
                    let Some(Token {
                        kind: TokKind::Ident(_),
                        ..
                    }) = toks.first()
                    else {
                        return Ok(None);
                    };
                    match toks.get(1).map(|t| &t.kind) {
                        None | Some(TokKind::Eof) => {}
                        Some(TokKind::Colon) => {
                            // `name : Type` — the rest (from index 2) must parse
                            // as a type and consume the whole remaining stream.
                            if toks.len() < 3 {
                                return Ok(None);
                            }
                        }
                        _ => return Ok(None),
                    }
                }
            }
    
            let span = self.bump().span;
            let match_parts = self.build_str_match_parts(parts)?;
            Ok(Some(Pattern::StrMatch {
                parts: match_parts,
                span,
            }))
        }
    
        /// D-PARSESTR1 (shared, extended for D-SHIFT1's `take_pattern` consume
        /// mode — I8, one matcher engine): turn a string token's already-lexed
        /// `StrTokPart`s into `StrMatchPart`s — `{name}` or `{name:Type}` holes,
        /// literal text otherwise — and check E0147 (two holes with nothing
        /// between them to split on). Callers have already validated hole shape
        /// (bare ident, optional `:Type`) before calling this; that check lives
        /// at each call site because the fallback-on-mismatch behavior differs
        /// (`==` position falls back to ordinary `Expr::Str`; `take_pattern`'s
        /// argument has no alternate legal meaning, so it errors instead).
        fn build_str_match_parts(
            &mut self,
            parts: Vec<StrTokPart>,
        ) -> Result<Vec<StrMatchPart>, Diagnostic> {
            let mut match_parts: Vec<StrMatchPart> = Vec::new();
            for part in parts {
                match part {
                    StrTokPart::Lit(s) => match_parts.push(StrMatchPart::Lit(s)),
                    StrTokPart::Interp(toks) => {
                        let mut sub = Parser {
                            toks: &toks,
                            pos: 0,
                            diags: Vec::new(),
                            pending_type_gt: false,
                            depth: self.depth,
                            type_generic_depth: 0,
                            type_generic_chain: Vec::new(),
                            type_generic_truncated: false,
                            pub_file_default: false,
                            in_layout_body: self.in_layout_body,
                            module_arg_expr_depth: None,
                            policy_declarations: Vec::new(),
                            applied_rules: Vec::new(),
                            rule_facts: Vec::new(),
                            block_spans: Vec::new(),
                        };
                        let (name, name_span) = sub.expect_ident("in a pattern hole")?;
                        let ty = if matches!(sub.peek().kind, TokKind::Colon) {
                            sub.bump();
                            let (t, _) = sub.type_()?;
                            Some(t)
                        } else {
                            None
                        };
                        if !sub.diags.is_empty() {
                            let mut ds = sub.diags;
                            let first = ds.remove(0);
                            self.diags.extend(ds);
                            return Err(first);
                        }
                        let hole_span = if matches!(sub.peek().kind, TokKind::Eof) {
                            name_span
                        } else {
                            Span::new(name_span.start, sub.peek().span.end)
                        };
                        match_parts.push(StrMatchPart::Hole {
                            name,
                            ty,
                            span: hole_span,
                        });
                    }
                }
            }
    
            // E0147: two holes with no literal text (or only an empty literal)
            // between them — a pattern splits text at the fixed characters
            // between holes, so back-to-back holes are ambiguous. Pushed (not
            // returned as `Err`) so the rest of this arm table — and the rest of
            // the file — still parses and reports its own problems (M1 recovery),
            // matching the `..=`/E0318 arm-head recovery convention.
            let mut i = 0;
            while i + 1 < match_parts.len() {
                let adjacent_holes = matches!(match_parts[i], StrMatchPart::Hole { .. })
                    && match match_parts.get(i + 1) {
                        Some(StrMatchPart::Hole { .. }) => true,
                        Some(StrMatchPart::Lit(s)) => {
                            s.is_empty()
                                && matches!(match_parts.get(i + 2), Some(StrMatchPart::Hole { .. }))
                        }
                        None => false,
                    };
                if adjacent_holes {
                    let StrMatchPart::Hole { span: s1, .. } = &match_parts[i] else {
                        unreachable!()
                    };
                    let next_hole_idx = if matches!(match_parts[i + 1], StrMatchPart::Hole { .. }) {
                        i + 1
                    } else {
                        i + 2
                    };
                    let StrMatchPart::Hole { span: s2, .. } = &match_parts[next_hole_idx] else {
                        unreachable!()
                    };
                    self.diags.push(Diagnostic::error(
                        "E0147",
                        "these two `{}` holes have nothing between them to split on".to_string(),
                        "a pattern splits the matched text at the fixed characters between holes; \
                         back-to-back holes give it nothing to split on"
                            .to_string(),
                        "put literal text between them, or type them so the boundary is unambiguous"
                            .to_string(),
                        Some(Span::new(s1.start, s2.end)),
                    ));
                }
                i += 1;
            }
    
            Ok(match_parts)
        }
    
        /// D-SHIFT1 (c7shift): parse the sole argument of `cursor.take_pattern(…)`
        /// as a pattern literal (`Expr::StrMatchLit`), reusing the D-PARSESTR1
        /// hole grammar/engine (`build_str_match_parts`) instead of a second
        /// pattern parser (I8). Unlike `try_str_match_pattern` (the `==`
        /// position), a hole-free literal IS legal here (a fixed prefix to
        /// consume with no bindings), and a malformed hole is a hard parse error
        /// — there's no ordinary-`Expr::Str` fallback for a `take_pattern`
        /// argument, so silently falling back would just move the failure to a
        /// confusing type error later.
        pub(super) fn parse_take_pattern_literal(&mut self) -> Result<Expr, Diagnostic> {
            // D-BINPAT1 (card #506 follow-up): `reader.take_pattern(b"…")` —
            // the byte-mode sibling. Receiver type isn't known yet at parse
            // time (`Cursor` vs `Reader`), so both literal kinds are accepted
            // here; sema rejects the mismatched pairing (text pattern on a
            // `Reader`, byte pattern on a `Cursor`). Byte-mode holes have no
            // alternate legal meaning already (I8: reuse `build_bin_match_parts`,
            // the same hole engine `try_bin_match_pattern` uses).
            if let TokKind::BinStr(parts) = self.peek().kind.clone() {
                let span = self.bump().span;
                let match_parts = self.build_bin_match_parts(parts, span)?;
                return Ok(Expr::BinMatchLit(match_parts, span));
            }
            let TokKind::Str(parts) = self.peek().kind.clone() else {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "`{}` takes a literal pattern string, not {}",
                        Syntax::METHOD_TAKE_PATTERN,
                        describe(&self.peek().kind)
                    ),
                    "the pattern is matched at compile time, so it must be written directly as a string literal"
                        .to_string(),
                    format!(
                        "write `{}(\"literal-{{hole}}-pattern\")`",
                        Syntax::METHOD_TAKE_PATTERN
                    ),
                    Some(self.peek().span),
                ));
            };
            for part in &parts {
                if let StrTokPart::Interp(toks) = part {
                    let bad_hole = match toks.first() {
                        Some(Token {
                            kind: TokKind::Ident(_),
                            ..
                        }) => match toks.get(1).map(|t| &t.kind) {
                            None | Some(TokKind::Eof) => false,
                            Some(TokKind::Colon) => toks.len() < 3,
                            _ => true,
                        },
                        _ => true,
                    };
                    if bad_hole {
                        let hole_span = toks.first().map(|t| t.span).unwrap_or(self.peek().span);
                        return Err(Diagnostic::error(
                            "E0003",
                            "a `take_pattern` hole must be a bare name, optionally typed".to_string(),
                            "each `{hole}` binds a name (`{id}`) or a typed, fallibly-parsed name (`{id:Int}`) — the same grammar an `if == {}` pattern hole uses"
                                .to_string(),
                            "write `{name}` or `{name:Type}`".to_string(),
                            Some(hole_span),
                        ));
                    }
                }
            }
            let span = self.bump().span;
            let match_parts = self.build_str_match_parts(parts)?;
            Ok(Expr::StrMatchLit(match_parts, span))
        }
    
        /// D-BINPAT1 (card #506): read the `b"…"` token at the cursor as a
        /// binary pattern — the byte-mode sibling of `try_str_match_pattern`.
        /// Every hole must be `{name:U<width>[be|le]}` or `{name:...}`; the
        /// `b"…"` token has no alternate legal meaning (there is no byte-value
        /// literal), so a malformed hole is a hard error here rather than a
        /// fall-through.
        pub(super) fn try_bin_match_pattern(&mut self) -> Result<Option<Pattern>, Diagnostic> {
            let TokKind::BinStr(parts) = &self.peek().kind else {
                return Ok(None);
            };
            let parts = parts.clone();
            let span = self.bump().span;
            let match_parts = self.build_bin_match_parts(parts, span)?;
            Ok(Some(Pattern::BinMatch {
                parts: match_parts,
                span,
            }))
        }

        /// D-BINPAT1: turn a `b"…"` token's lexed `StrTokPart`s into
        /// `BinMatchPart`s — fixed literal bytes, or bit-typed holes — checking
        /// the bit-spec grammar (E1007), endianness rules (E1008), and the
        /// rest-must-be-final law (E1009, the byte-mode analog of E0147).
        fn build_bin_match_parts(
            &mut self,
            parts: Vec<StrTokPart>,
            lit_span: Span,
        ) -> Result<Vec<crate::AST::BinMatchPart>, Diagnostic> {
            use crate::AST::{BinEndian, BinMatchPart, BinSpec};
            let mut out: Vec<BinMatchPart> = Vec::new();
            for part in parts {
                match part {
                    StrTokPart::Lit(s) => {
                        if !s.is_empty() {
                            out.push(BinMatchPart::Lit(s.into_bytes()));
                        }
                    }
                    StrTokPart::Interp(toks) => {
                        // Hole shape: `name : U<width>[be|le]` or `name : ...`.
                        // Malformed holes are PUSHED (not returned) with a
                        // recovery placeholder, so the rest of the arm table and
                        // file still parse (M1 recovery, matching E0147).
                        let (name, name_span) = match toks.first() {
                            Some(Token {
                                kind: TokKind::Ident(n),
                                span,
                            }) => (n.clone(), *span),
                            _ => {
                                self.diags.push(self.bin_bad_hole(lit_span));
                                continue;
                            }
                        };
                        if !matches!(toks.get(1).map(|t| &t.kind), Some(TokKind::Colon)) {
                            self.diags.push(self.bin_bad_hole(name_span));
                            continue;
                        }
                        let spec_span = toks.get(2).map(|t| t.span).unwrap_or(name_span);
                        let trailing = toks
                            .get(3)
                            .is_some_and(|t| !matches!(t.kind, TokKind::Eof));
                        let spec = match toks.get(2).map(|t| &t.kind) {
                            Some(TokKind::DotDotDot) if !trailing => BinSpec::Rest,
                            Some(TokKind::Ident(spec)) if !trailing => {
                                self.parse_bin_bits(spec, spec_span)
                            }
                            _ => {
                                self.diags.push(self.bin_bad_hole(spec_span));
                                BinSpec::Bits {
                                    width: 8,
                                    endian: BinEndian::None,
                                }
                            }
                        };
                        let hole_span = Span::new(name_span.start, spec_span.end);
                        out.push(BinMatchPart::Hole {
                            name,
                            spec,
                            span: hole_span,
                        });
                    }
                }
            }
            // E1009 (byte-mode E0147 analog): a `...` rest capture must be the
            // final part — nothing can follow a greedy tail. Pushed, not
            // returned, so the rest of the file still parses (M1 recovery).
            for i in 0..out.len() {
                let is_rest = matches!(
                    &out[i],
                    BinMatchPart::Hole {
                        spec: BinSpec::Rest,
                        ..
                    }
                );
                if is_rest && i + 1 < out.len() {
                    let sp = match &out[i] {
                        BinMatchPart::Hole { span, .. } => *span,
                        BinMatchPart::Lit(_) => lit_span,
                    };
                    self.diags.push(Diagnostic::error(
                        "E1009",
                        "a `{...}` rest capture must be the last part of a binary pattern".to_string(),
                        "the rest capture takes every remaining byte, so nothing can come after it"
                            .to_string(),
                        "move the `{name:...}` hole to the end, or give it a fixed width".to_string(),
                        Some(sp),
                    ));
                    break;
                }
            }
            Ok(out)
        }

        /// D-BINPAT1: parse a bit-width spec ident — `U4`, `U16be`, `U16le`.
        /// Width is 1..=64 bits; a multi-byte read (width > 8) requires an
        /// endian suffix (E1008), a single-byte-or-smaller read forbids one.
        fn parse_bin_bits(&mut self, spec: &str, span: Span) -> crate::AST::BinSpec {
            use crate::AST::{BinEndian, BinSpec};
            let fallback = BinSpec::Bits {
                width: 8,
                endian: BinEndian::None,
            };
            let Some(rest) = spec.strip_prefix('U') else {
                self.diags.push(self.bin_bad_spec(spec, span));
                return fallback;
            };
            let (digits, endian) = if let Some(d) = rest.strip_suffix(Syntax::BINPAT_ENDIAN_BIG) {
                (d, BinEndian::Big)
            } else if let Some(d) = rest.strip_suffix(Syntax::BINPAT_ENDIAN_LITTLE) {
                (d, BinEndian::Little)
            } else {
                (rest, BinEndian::None)
            };
            let width: Option<u32> = if digits.is_empty()
                || !digits.chars().all(|c| c.is_ascii_digit())
            {
                None
            } else {
                digits.parse().ok().filter(|w| *w >= 1 && *w <= 64)
            };
            let Some(width) = width else {
                self.diags.push(self.bin_bad_spec(spec, span));
                return fallback;
            };
            let width = width as u8;
            match (width > 8, endian) {
                // Multi-byte read must say which byte order.
                (true, BinEndian::None) => {
                    self.diags.push(Diagnostic::error(
                        "E1008",
                        format!("`{{…:U{width}}}` reads more than one byte, so it needs a byte order"),
                        "a read wider than 8 bits spans multiple bytes; Jet won't guess big- vs little-endian"
                            .to_string(),
                        format!("write `U{width}be` (big-endian / network order) or `U{width}le` (little-endian)"),
                        Some(span),
                    ));
                    BinSpec::Bits { width, endian: BinEndian::Big }
                }
                // Single-byte read: an endian suffix is meaningless.
                (false, BinEndian::Big | BinEndian::Little) => {
                    self.diags.push(Diagnostic::error(
                        "E1008",
                        format!("`U{width}` reads a single byte, so a `be`/`le` suffix has no meaning"),
                        "byte order only matters when a read spans more than one byte".to_string(),
                        format!("drop the suffix — write `U{width}`"),
                        Some(span),
                    ));
                    BinSpec::Bits { width, endian: BinEndian::None }
                }
                _ => BinSpec::Bits { width, endian },
            }
        }

        fn bin_bad_hole(&self, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E1007",
                "a binary pattern hole must be `{name:U<width>}` or `{name:...}`".to_string(),
                "each hole binds a name to a fixed-width bit field (`{v:U4}`, `{len:U16be}`) or the trailing bytes (`{rest:...}`)"
                    .to_string(),
                "write `{name:U8}`, `{name:U16be}`, or `{name:...}`".to_string(),
                Some(span),
            )
        }

        fn bin_bad_spec(&self, spec: &str, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E1007",
                format!("`{spec}` isn't a valid bit width in a binary pattern"),
                "a bit field is `U` followed by a width of 1 to 64 bits, optionally with a `be`/`le` byte order"
                    .to_string(),
                "write e.g. `U4`, `U8`, `U16be`, or `U32le`".to_string(),
                Some(span),
            )
        }

        pub(super) fn struct_pattern_rhs(&mut self) -> Result<Pattern, Diagnostic> {
            let dot_span = self.bump().span;
            self.expect(TokKind::LBrace, "after `.` in a struct pattern")?;
            let mut fields = Vec::new();
            let mut rest = None;
            if !matches!(self.peek().kind, TokKind::RBrace) {
                loop {
                    if matches!(self.peek().kind, TokKind::DotDot) {
                        rest = Some(self.bump().span);
                        break;
                    }
                    let (field, field_span) = self.expect_ident("for a struct-pattern field")?;
                    if matches!(self.peek().kind, TokKind::Colon) {
                        self.bump();
                        if let TokKind::Ident(local) = self.peek().kind.clone() {
                            let local_span = self.peek().span;
                            let next = self.toks.get(self.pos + 1).map(|t| &t.kind);
                            if matches!(next, Some(TokKind::Comma | TokKind::RBrace)) {
                                self.bump();
                                fields.push(crate::AST::StructPatField::Bind {
                                    field,
                                    field_span,
                                    local,
                                    local_span,
                                });
                            } else {
                                let value = self.expr_no_struct_lit()?;
                                fields.push(crate::AST::StructPatField::Value {
                                    field,
                                    field_span,
                                    value: Box::new(value),
                                });
                            }
                        } else {
                            let value = self.expr_no_struct_lit()?;
                            fields.push(crate::AST::StructPatField::Value {
                                field,
                                field_span,
                                value: Box::new(value),
                            });
                        }
                    } else {
                        fields.push(crate::AST::StructPatField::Bind {
                            local: field.clone(),
                            local_span: field_span,
                            field,
                            field_span,
                        });
                    }
                    if matches!(self.peek().kind, TokKind::Comma) {
                        self.bump();
                        if matches!(self.peek().kind, TokKind::RBrace) {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            self.expect(TokKind::RBrace, "after struct-pattern fields")?;
            let end = self.toks[self.pos.saturating_sub(1)].span.end;
            Ok(Pattern::Struct {
                fields,
                rest,
                span: Span::new(dot_span.start, end),
            })
        }
    
}
