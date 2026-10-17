use super::super::{
    describe, leading_dot_variant, Diagnostic, Expr, Parser, Pattern, Span, StrMatchPart,
    StrTokPart, Syntax, TokKind, Token, Type,
};

enum TypedPatternKind {
    Bytes,
    Text,
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_match_part_walk_parses_text_and_bytes() {
        let src = r#"
fn run() {
    text :: "prefix-42-suffix"
    if text == {
        "prefix-{value}-suffix" -> print(value)
        else -> print("miss")
    }
    bytes :: [U8]{ 7 }
    if bytes == {
        [U8]{"{value:U8}"} -> print(value)
        else -> print("miss")
    }
}
"#;
        let (tokens, lex) = crate::Lexer::lex(src);
        assert!(lex.is_empty(), "{lex:?}");
        assert!(crate::Parser::parse(&tokens).is_ok());
    }
}

pub(super) fn is_byte_list_head(ty: &Type) -> bool {
    matches!(
        ty,
        Type::List(elem) | Type::FixedList { elem, .. }
            if matches!(
                elem.as_ref(),
                Type::IntN {
                    signed: false,
                    bits: 8
                }
            )
    )
}

impl<'a> Parser<'a> {
    /// D-PARSESTR1 / D-BINPAT1: one literal-part walk for both pattern
    /// modes. Only hole elaboration and literal conversion vary by mode;
    /// the pattern engine owns the token-part traversal.
    fn build_match_parts<T, L, B, H>(
        &mut self,
        parts: Vec<StrTokPart>,
        mut literal: L,
        mut byte: B,
        mut hole: H,
    ) -> Result<Vec<T>, Diagnostic>
    where
        L: FnMut(String) -> Option<T>,
        B: FnMut(u8) -> Option<T>,
        H: FnMut(&mut Self, Vec<Token>) -> Result<Option<T>, Diagnostic>,
    {
        let mut out = Vec::new();
        for part in parts {
            match part {
                StrTokPart::Lit(s) => {
                    if let Some(part) = literal(s) {
                        out.push(part);
                    }
                }
                StrTokPart::Byte(byte_value) => {
                    if let Some(part) = byte(byte_value) {
                        out.push(part);
                    }
                }
                StrTokPart::Interp(toks) => {
                    if let Some(part) = hole(self, toks)? {
                        out.push(part);
                    }
                }
            }
        }
        Ok(out)
    }

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
        let match_parts = self.build_match_parts(
            parts,
            |s| Some(StrMatchPart::Lit(s)),
            |byte| Some(StrMatchPart::Lit(char::from(byte).to_string())),
            |parser, toks| parser.parse_str_match_hole(toks).map(Some),
        )?;

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

    fn parse_str_match_hole(&mut self, toks: Vec<Token>) -> Result<StrMatchPart, Diagnostic> {
        let mut sub = Parser {
            toks: &toks,
            source: None,
            explicit_semicolon_reported: false,
            retired_step: None,
            pos: 0,
            diags: Vec::new(),
            pending_type_gt: false,
            depth: self.depth,
            result_handler_depth: self.result_handler_depth,
            type_generic_depth: 0,
            type_generic_chain: Vec::new(),
            type_generic_truncated: false,
            in_layout_body: self.in_layout_body,
            adjacent_if_body_depth: 0,
            block_depth: 0,
            callable_tail_block_depth: None,
            callable_tail_expects_value: false,
            lambda_tail_block_depth: None,
            module_arg_expr_depth: None,
            allow_lowercase_leading_dot: self.allow_lowercase_leading_dot,
            allow_environment_reads: self.allow_environment_reads,
            migration_mode: self.migration_mode,
            derive_template_depth: self.derive_template_depth,
            policy_declarations: Vec::new(),
            user_policy_declarations: Vec::new(),
            applied_rules: Vec::new(),
            rule_facts: Vec::new(),
            block_spans: Vec::new(),
            prep_value_block: None,
            prep_value_escape: None,
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
        Ok(StrMatchPart::Hole {
            name,
            ty,
            span: hole_span,
        })
    }

    /// D-SHIFT1 / D-UNIFYLIT1=A: parse the sole argument of `take_pattern(…)`.
    /// Accepts a bare `"…"` text pattern, `String{"…"}`, or `[U8]{"…"}`
    /// (byte mode). Byte values use the typed-list value form as well.
    pub(super) fn parse_take_pattern_literal(&mut self) -> Result<Expr, Diagnostic> {
        if let Some(expr) = self.try_typed_pattern_literal_expr()? {
            return Ok(expr);
        }
        let parts = match self.peek().kind.clone() {
            TokKind::Str(parts) => parts,
            TokKind::RawStr(text) => vec![StrTokPart::Lit(text)],
            _ => {
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
                        "write `{}(\"literal-{{hole}}-pattern\")` or {}([U8]{{\"…\"}}) for bytes",
                        Syntax::METHOD_TAKE_PATTERN,
                        Syntax::METHOD_TAKE_PATTERN
                    ),
                    Some(self.peek().span),
                ));
            }
        };
        let span = self.bump().span;
        let match_parts = self.build_str_match_parts(parts)?;
        Ok(Expr::StrMatchLit(match_parts, span))
    }

    /// D-UNIFYLIT1=A: `[U8]{"…"}` / `String{"…"}` as a take_pattern /
    /// pattern-position literal expression.
    fn try_typed_pattern_literal_expr(&mut self) -> Result<Option<Expr>, Diagnostic> {
        let save = self.pos;
        let save_diags = self.diags.len();
        let Some((kind, parts, span)) = self.try_typed_pattern_str_body()? else {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        };
        match kind {
            TypedPatternKind::Bytes => {
                let match_parts = self.build_bin_match_parts(parts, span)?;
                Ok(Some(Expr::BinMatchLit(match_parts, span)))
            }
            TypedPatternKind::Text => {
                Self::reject_bad_take_pattern_holes(&parts, span)?;
                let match_parts = self.build_str_match_parts(parts)?;
                Ok(Some(Expr::StrMatchLit(match_parts, span)))
            }
        }
    }

    /// Hard-error gate shared by bare `"…"` and `String{"…"}` take_pattern args.
    fn reject_bad_take_pattern_holes(
        parts: &[StrTokPart],
        fallback_span: Span,
    ) -> Result<(), Diagnostic> {
        for part in parts {
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
                    let hole_span = toks.first().map(|t| t.span).unwrap_or(fallback_span);
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
        Ok(())
    }

    /// D-BINPAT1 / D-UNIFYLIT1=A: `[U8]{"…"}` binary pattern — byte-mode
    /// sibling of `try_str_match_pattern`. Malformed holes are hard errors
    /// (no byte-value literal fallback).
    pub(super) fn try_bin_match_pattern(&mut self) -> Result<Option<Pattern>, Diagnostic> {
        let save = self.pos;
        let save_diags = self.diags.len();
        let Some((kind, parts, span)) = self.try_typed_pattern_str_body()? else {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        };
        if !matches!(kind, TypedPatternKind::Bytes) {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        }
        let match_parts = self.build_bin_match_parts(parts, span)?;
        Ok(Some(Pattern::BinMatch {
            parts: match_parts,
            span,
        }))
    }

    /// Optional `String{"…"}` text pattern head (symmetry with `[U8]{"…"}`).
    pub(super) fn try_string_typed_str_match_pattern(
        &mut self,
    ) -> Result<Option<Pattern>, Diagnostic> {
        let save = self.pos;
        let save_diags = self.diags.len();
        let Some((kind, parts, span)) = self.try_typed_pattern_str_body()? else {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        };
        if !matches!(kind, TypedPatternKind::Text) {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        }
        if !parts.iter().any(|p| matches!(p, StrTokPart::Interp(_))) {
            // Hole-free String.{"…"} is plain equality, not a str-match pattern.
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        }
        for part in &parts {
            if let StrTokPart::Interp(toks) = part {
                let Some(Token {
                    kind: TokKind::Ident(_),
                    ..
                }) = toks.first()
                else {
                    self.pos = save;
                    self.diags.truncate(save_diags);
                    return Ok(None);
                };
                match toks.get(1).map(|t| &t.kind) {
                    None | Some(TokKind::Eof) => {}
                    Some(TokKind::Colon) => {
                        if toks.len() < 3 {
                            self.pos = save;
                            self.diags.truncate(save_diags);
                            return Ok(None);
                        }
                    }
                    _ => {
                        self.pos = save;
                        self.diags.truncate(save_diags);
                        return Ok(None);
                    }
                }
            }
        }
        let match_parts = self.build_str_match_parts(parts)?;
        Ok(Some(Pattern::StrMatch {
            parts: match_parts,
            span,
        }))
    }

    /// Parse `Head{"…"}` where Head is `[U8]` / `[U8#N]` or `String`.
    /// Returns the raw string token parts without elaborating holes as exprs.
    fn try_typed_pattern_str_body(
        &mut self,
    ) -> Result<Option<(TypedPatternKind, Vec<StrTokPart>, Span)>, Diagnostic> {
        let save = self.pos;
        let save_diags = self.diags.len();
        let kind = if matches!(self.peek().kind, TokKind::LBracket) {
            let Ok((head, head_span)) = self.type_() else {
                self.pos = save;
                self.diags.truncate(save_diags);
                return Ok(None);
            };
            if !is_byte_list_head(&head) {
                self.pos = save;
                self.diags.truncate(save_diags);
                return Ok(None);
            }
            (TypedPatternKind::Bytes, head_span.start)
        } else if let TokKind::Ident(n) = &self.peek().kind {
            if n != "String" {
                return Ok(None);
            }
            let head_span = self.bump().span;
            (TypedPatternKind::Text, head_span.start)
        } else {
            return Ok(None);
        };
        let (kind, start) = kind;
        let dotted = matches!(self.peek().kind, TokKind::Dot);
        if dotted {
            self.bump();
        } else if !matches!(self.peek().kind, TokKind::LBrace) {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        }
        if dotted {
            let label = if matches!(&kind, TypedPatternKind::Bytes) {
                "[U8]"
            } else {
                "String"
            };
            self.diags.push(Diagnostic::error(
                "E0320",
                format!("typed pattern construction uses `{label}{{…}}`, not `{label}.{{…}}`"),
                "D-LIT-DOT1 drops the constructor dot from every literal head".to_string(),
                format!("write `{label}{{…}}`"),
                Some(self.toks[self.pos.saturating_sub(1)].span),
            ));
        }
        if !matches!(self.peek().kind, TokKind::LBrace) {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        }
        self.bump(); // opening brace
        let parts = match self.peek().kind.clone() {
            TokKind::Str(parts) => parts,
            TokKind::RawStr(text) => vec![StrTokPart::Lit(text)],
            _ => {
                self.pos = save;
                self.diags.truncate(save_diags);
                return Ok(None);
            }
        };
        self.bump(); // string
        if !matches!(self.peek().kind, TokKind::RBrace) {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        }
        let end = self.bump().span.end;
        Ok(Some((kind, parts, Span::new(start, end))))
    }

    /// D-BINPAT1: turn a pattern string's lexed `StrTokPart`s into
    /// `BinMatchPart`s — fixed literal bytes, or bit-typed holes — checking
    /// the bit-spec grammar (E1007), endianness rules (E1008), and the
    /// rest-must-be-final law (E1009, the byte-mode analog of E0147).
    fn build_bin_match_parts(
        &mut self,
        parts: Vec<StrTokPart>,
        lit_span: Span,
    ) -> Result<Vec<crate::AST::BinMatchPart>, Diagnostic> {
        use crate::AST::{BinMatchPart, BinSpec};
        let out = self.build_match_parts(
            parts,
            |s| (!s.is_empty()).then(|| BinMatchPart::Lit(s.into_bytes())),
            |byte| Some(BinMatchPart::Lit(vec![byte])),
            |parser, toks| Ok(parser.parse_bin_match_hole(toks, lit_span)),
        )?;
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

    fn parse_bin_match_hole(
        &mut self,
        toks: Vec<Token>,
        lit_span: Span,
    ) -> Option<crate::AST::BinMatchPart> {
        use crate::AST::{BinEndian, BinMatchPart, BinSpec};
        // Hole shape: `name : U<width>[be|le]` or `name : ...`.
        // Malformed holes are PUSHED (not returned) with a recovery
        // placeholder, so the rest of the arm table and file still parse
        // (M1 recovery, matching E0147).
        let (name, name_span) = match toks.first() {
            Some(Token {
                kind: TokKind::Ident(n),
                span,
            }) => (n.clone(), *span),
            _ => {
                self.diags.push(self.bin_bad_hole(lit_span));
                return None;
            }
        };
        if !matches!(toks.get(1).map(|t| &t.kind), Some(TokKind::Colon)) {
            self.diags.push(self.bin_bad_hole(name_span));
            return None;
        }
        let spec_span = toks.get(2).map(|t| t.span).unwrap_or(name_span);
        let trailing = toks.get(3).is_some_and(|t| !matches!(t.kind, TokKind::Eof));
        let spec = match toks.get(2).map(|t| &t.kind) {
            Some(TokKind::DotDotDot) if !trailing => BinSpec::Rest,
            Some(TokKind::Ident(spec)) if !trailing => self.parse_bin_bits(spec, spec_span),
            _ => {
                self.diags.push(self.bin_bad_hole(spec_span));
                BinSpec::Bits {
                    width: 8,
                    endian: BinEndian::None,
                }
            }
        };
        let hole_span = Span::new(name_span.start, spec_span.end);
        Some(BinMatchPart::Hole {
            name,
            spec,
            span: hole_span,
        })
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
        let width: Option<u32> = if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit())
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
                BinSpec::Bits {
                    width,
                    endian: BinEndian::Big,
                }
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
                BinSpec::Bits {
                    width,
                    endian: BinEndian::None,
                }
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

    /// D-DESTRUCT1 / D-PAT-RECORD1=A: the brace body of a struct pattern,
    /// `{field, field: local, field: value, field: pattern, ..}`. The cursor
    /// is on the `{` (or the retired `.{`); `type_name` is the written head
    /// of `Point{…}`, already consumed by the caller.
    pub(super) fn struct_pattern_rhs(
        &mut self,
        type_name: Option<(String, Span)>,
    ) -> Result<Pattern, Diagnostic> {
        let dotted = matches!(self.peek().kind, TokKind::Dot);
        let start = type_name
            .as_ref()
            .map_or(self.peek().span.start, |(_, span)| span.start);
        if dotted {
            let dot = self.bump();
            self.diags.push(Diagnostic::error(
                "E0320",
                "record patterns use `{…}`, not `.{…}`".to_string(),
                "D-LIT-DOT1 drops the constructor dot from every literal head".to_string(),
                "write `{…}`".to_string(),
                Some(dot.span),
            ));
        }
        self.expect(TokKind::LBrace, "to open a struct pattern")?;
        let mut fields = Vec::new();
        let mut rest = None;
        loop {
            self.skip_named_pattern_line_breaks();
            if matches!(self.peek().kind, TokKind::RBrace) {
                break;
            }
            if matches!(self.peek().kind, TokKind::DotDot) {
                let rest_span = self.bump().span;
                self.skip_named_pattern_line_breaks();
                if !matches!(self.peek().kind, TokKind::RBrace) {
                    return Err(Self::rest_not_last(rest_span, "struct"));
                }
                rest = Some(rest_span);
                break;
            }
            let (field, field_span) = self.expect_ident("for a struct-pattern field")?;
            if matches!(self.peek().kind, TokKind::Colon) {
                self.bump();
                let local = match &self.peek().kind {
                    TokKind::Ident(local)
                        if !Syntax::is_constant_name(local)
                            && !local.chars().next().is_some_and(char::is_uppercase)
                            && matches!(
                                self.toks.get(self.pos + 1).map(|t| &t.kind),
                                Some(TokKind::Comma | TokKind::RBrace | TokKind::Semi)
                            ) =>
                    {
                        Some(local.clone())
                    }
                    _ => None,
                };
                if let Some(local) = local {
                    let local_span = self.bump().span;
                    fields.push(crate::AST::StructPatField::Bind {
                        field,
                        field_span,
                        local,
                        local_span,
                    });
                } else if self.nested_pattern_ahead(false) {
                    let pattern = self.nested_pattern()?;
                    fields.push(crate::AST::StructPatField::Nested {
                        field,
                        field_span,
                        pattern: Box::new(pattern),
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
                fields.push(crate::AST::StructPatField::Bind {
                    local: field.clone(),
                    local_span: field_span,
                    field,
                    field_span,
                });
            }
            self.skip_named_pattern_line_breaks();
            if !matches!(self.peek().kind, TokKind::Comma) {
                break;
            }
            self.bump();
        }
        self.skip_named_pattern_line_breaks();
        self.expect(TokKind::RBrace, "after struct-pattern fields")?;
        let end = self.toks[self.pos.saturating_sub(1)].span.end;
        Ok(Pattern::Struct {
            type_name,
            fields,
            rest,
            span: Span::new(start, end),
        })
    }

    fn rest_not_last(span: Span, form: &str) -> Diagnostic {
        Diagnostic::error(
            "E0003",
            format!("`..` must be the last entry of a {form} pattern"),
            "`..` stands for everything the pattern does not list, so nothing can follow it"
                .to_string(),
            "move `..` to the end of the pattern".to_string(),
            Some(span),
        )
    }

    /// D-PAT-RECORD1=A: `Point{…}` starts a named struct pattern when the
    /// name is a type name and the brace body reads as pattern entries
    /// (the D-PAT-NAMED-NEST1 lookahead), so `{` still opens a body
    /// otherwise and conversion heads such as `Float{b}` stay values.
    pub(super) fn named_struct_pattern_ahead(&self) -> bool {
        let TokKind::Ident(name) = &self.peek().kind else {
            return false;
        };
        if !name.chars().next().is_some_and(char::is_uppercase)
            || Syntax::is_constant_name(name)
            || name == "String"
        {
            return false;
        }
        let kind = |offset: usize| self.toks.get(self.pos + offset).map(|t| &t.kind);
        if !matches!(kind(1), Some(TokKind::LBrace)) {
            return false;
        }
        match kind(2) {
            Some(TokKind::DotDot) => true,
            Some(TokKind::Ident(field)) if field != Syntax::PAT_WILDCARD_SLOT => match kind(3) {
                Some(TokKind::Colon | TokKind::Comma) => true,
                Some(TokKind::RBrace) => matches!(
                    kind(4),
                    Some(
                        TokKind::LBrace
                            | TokKind::UnifiedArrow
                            | TokKind::LambdaArrow
                            | TokKind::Comma
                            | TokKind::RParen
                            | TokKind::RBracket
                            | TokKind::RBrace
                            | TokKind::AndAnd
                    )
                ),
                _ => false,
            },
            _ => false,
        }
    }

    /// D-PAT-RECORD1=A: parse `Point{…}` as a struct pattern. Unless
    /// `forced`, a head whose entries are all `field: value` comparisons is
    /// the struct value it spells (today's equality), so it is left for the
    /// expression parser.
    pub(super) fn try_named_struct_pattern(
        &mut self,
        forced: bool,
    ) -> Result<Option<Pattern>, Diagnostic> {
        let save = self.pos;
        let save_diags = self.diags.len();
        let TokKind::Ident(name) = self.peek().kind.clone() else {
            return Ok(None);
        };
        let name_span = self.bump().span;
        let pattern = match self.struct_pattern_rhs(Some((name, name_span))) {
            Ok(pattern) => pattern,
            Err(error) if forced => return Err(error),
            Err(_) => {
                self.pos = save;
                self.diags.truncate(save_diags);
                return Ok(None);
            }
        };
        if forced || Self::pattern_binds_or_shapes(&pattern) {
            return Ok(Some(pattern));
        }
        self.pos = save;
        self.diags.truncate(save_diags);
        Ok(None)
    }

    /// Whether a struct, list or map pattern does more than compare written
    /// values: it binds, skips (`..`, `_`, `?:`), nests a pattern, or is the
    /// empty `[]` / `[:]` shape.
    fn pattern_binds_or_shapes(pattern: &Pattern) -> bool {
        use crate::AST::{PatSlot, StructPatField};
        let slot_shapes = |slot: &PatSlot| !matches!(slot, PatSlot::Value(_));
        match pattern {
            Pattern::Struct { fields, rest, .. } => {
                rest.is_some()
                    || fields
                        .iter()
                        .any(|field| !matches!(field, StructPatField::Value { .. }))
            }
            Pattern::List { items, rest, .. } => {
                items.is_empty() || rest.is_some() || items.iter().any(slot_shapes)
            }
            Pattern::Map { entries, rest, .. } => {
                entries.is_empty()
                    || rest.is_some()
                    || entries
                        .iter()
                        .any(|entry| entry.optional || slot_shapes(&entry.slot))
            }
            _ => true,
        }
    }

    /// D-PAT-RECORD1 / D-PAT-MAP1: whether the cursor starts a nested
    /// pattern rather than a compared value. `in_list` admits a bare unit
    /// case `.Red` as a nested pattern; in a struct field it stays the
    /// D-DESTRUCT1 value comparison it has always been.
    pub(super) fn nested_pattern_ahead(&self, in_list: bool) -> bool {
        let kind = |offset: usize| self.toks.get(self.pos + offset).map(|t| &t.kind);
        match kind(0) {
            Some(TokKind::Dot) => {
                if !kind(1).is_some_and(|k| leading_dot_variant(k).is_some()) {
                    return false;
                }
                in_list || matches!(kind(2), Some(TokKind::LParen | TokKind::LBrace | TokKind::Pipe))
            }
            Some(TokKind::LBrace) => self.brace_starts_record(),
            Some(TokKind::LBracket) => !self.bracket_starts_typed_lit(),
            Some(TokKind::Ident(_)) => self.named_struct_pattern_ahead(),
            _ => false,
        }
    }

    /// Parse the nested pattern `nested_pattern_ahead` found.
    fn nested_pattern(&mut self) -> Result<Pattern, Diagnostic> {
        match self.peek().kind {
            TokKind::LBrace => self.struct_pattern_rhs(None),
            TokKind::LBracket => self.bracket_pattern(true).map(|p| {
                p.expect("a forced bracket pattern always yields a pattern")
            }),
            TokKind::Ident(_) => {
                let TokKind::Ident(name) = self.peek().kind.clone() else {
                    unreachable!("guarded by the match arm")
                };
                let name_span = self.bump().span;
                self.struct_pattern_rhs(Some((name, name_span)))
            }
            _ => match self.try_pattern_rhs(true)? {
                Some(pattern) => Ok(pattern),
                None => Err(Diagnostic::error(
                    "E0003",
                    format!("expected a pattern, found {}", describe(&self.peek().kind)),
                    "this position holds a nested pattern".to_string(),
                    "write an enum case such as `.Val(x)`, a struct, list or map pattern"
                        .to_string(),
                    Some(self.peek().span),
                )),
            },
        }
    }

    /// D-PAT-RECORD1 / D-PAT-MAP1: one item of a list pattern, one value of
    /// a map pattern, or one enum payload slot. A bare name binds, `_`
    /// skips, `lo..hi` tests a range, a nested shape tests inside, and any
    /// other expression is a value compared with `==`.
    pub(in crate::Parser) fn pattern_item(
        &mut self,
        in_list: bool,
    ) -> Result<crate::AST::PatSlot, Diagnostic> {
        use crate::AST::PatSlot;
        let kind = |p: &Self, offset: usize| p.toks.get(p.pos + offset).map(|t| t.kind.clone());
        match kind(self, 0) {
            Some(TokKind::Ident(name)) if name == Syntax::PAT_WILDCARD_SLOT => {
                self.bump();
                return Ok(PatSlot::Wildcard);
            }
            Some(TokKind::Int(lo, _))
                if matches!(kind(self, 1), Some(TokKind::DotDot))
                    && matches!(kind(self, 2), Some(TokKind::Int(..))) =>
            {
                self.bump();
                self.bump();
                let Some(TokKind::Int(hi, _)) = kind(self, 0) else {
                    unreachable!("guarded by the lookahead")
                };
                self.bump();
                return Ok(PatSlot::Range { lo, hi });
            }
            Some(TokKind::Ident(name))
                if !Syntax::is_constant_name(&name)
                    && !name.chars().next().is_some_and(char::is_uppercase)
                    && matches!(
                        kind(self, 1),
                        Some(
                            TokKind::Comma
                                | TokKind::RParen
                                | TokKind::RBracket
                                | TokKind::RBrace
                                | TokKind::Semi
                        )
                    ) =>
            {
                let span = self.bump().span;
                return Ok(PatSlot::Bind { name, span });
            }
            _ => {}
        }
        if self.nested_pattern_ahead(in_list) {
            return Ok(PatSlot::Nested(Box::new(self.nested_pattern()?)));
        }
        let value = self.expr_no_struct_lit()?;
        Ok(PatSlot::Value(Box::new(value)))
    }

    /// D-PAT-RECORD1 / D-PAT-MAP1: a bracket pattern. `[]`, `[x]`,
    /// `[first, ..rest]` are list patterns; a first entry followed by `:` or
    /// `?:` makes a map pattern; `[:]` is the empty map. Unless `forced`
    /// (a nested position), a bracket whose entries only compare values is
    /// the list or map value it spells, so `None` leaves it to the
    /// expression parser with today's equality meaning.
    pub(super) fn bracket_pattern(&mut self, forced: bool) -> Result<Option<Pattern>, Diagnostic> {
        if !matches!(self.peek().kind, TokKind::LBracket) || self.bracket_starts_typed_lit() {
            return Ok(None);
        }
        let save = self.pos;
        let save_diags = self.diags.len();
        let start = self.bump().span.start;
        self.skip_named_pattern_line_breaks();
        let finish = |p: &mut Self| p.toks[p.pos.saturating_sub(1)].span.end;
        if matches!(self.peek().kind, TokKind::Colon)
            && matches!(self.peek2().kind, TokKind::RBracket)
        {
            self.bump();
            self.bump();
            let end = finish(self);
            return Ok(Some(Pattern::Map {
                entries: Vec::new(),
                rest: None,
                span: Span::new(start, end),
            }));
        }
        let parsed = if self.map_pattern_entry_ahead() {
            self.map_pattern_body(start)
        } else {
            self.list_pattern_body(start)
        };
        let pattern = match parsed {
            Ok(pattern) => pattern,
            // A bracket that already wrote a `..` rest is a pattern; report
            // its error. Anything else may still be a list or map value.
            Err(error)
                if forced
                    || self.toks[save..=self.pos.min(self.toks.len() - 1)]
                        .iter()
                        .any(|t| matches!(t.kind, TokKind::DotDot)) =>
            {
                return Err(error)
            }
            Err(_) => {
                self.pos = save;
                self.diags.truncate(save_diags);
                return Ok(None);
            }
        };
        if forced || Self::pattern_binds_or_shapes(&pattern) {
            return Ok(Some(pattern));
        }
        self.pos = save;
        self.diags.truncate(save_diags);
        Ok(None)
    }

    /// A map pattern entry starts with a key followed by `:` or `?:` at the
    /// top bracket depth.
    fn map_pattern_entry_ahead(&self) -> bool {
        let mut depth = 0usize;
        let mut index = self.pos;
        while let Some(token) = self.toks.get(index) {
            match &token.kind {
                TokKind::LParen | TokKind::LBracket | TokKind::LBrace => depth += 1,
                TokKind::RParen | TokKind::RBrace => depth = depth.saturating_sub(1),
                TokKind::RBracket if depth == 0 => return false,
                TokKind::RBracket => depth -= 1,
                TokKind::Comma | TokKind::DotDot if depth == 0 => return false,
                TokKind::Colon if depth == 0 => return true,
                TokKind::Question if depth == 0 => {
                    return matches!(
                        self.toks.get(index + 1).map(|t| &t.kind),
                        Some(TokKind::Colon)
                    )
                }
                TokKind::Eof => return false,
                _ => {}
            }
            index += 1;
        }
        false
    }

    fn list_pattern_body(&mut self, start: usize) -> Result<Pattern, Diagnostic> {
        let mut items = Vec::new();
        let mut rest = None;
        loop {
            self.skip_named_pattern_line_breaks();
            if matches!(self.peek().kind, TokKind::RBracket) {
                break;
            }
            if matches!(self.peek().kind, TokKind::DotDot) {
                let dots = self.bump().span;
                let name = match self.peek().kind.clone() {
                    TokKind::Ident(name) if name != Syntax::PAT_WILDCARD_SLOT => {
                        Some((name, self.bump().span))
                    }
                    _ => None,
                };
                let span = name
                    .as_ref()
                    .map_or(dots, |(_, name_span)| Span::new(dots.start, name_span.end));
                self.skip_named_pattern_line_breaks();
                if !matches!(self.peek().kind, TokKind::RBracket) {
                    return Err(Self::rest_not_last(span, "list"));
                }
                rest = Some(crate::AST::ListPatRest { span, name });
                break;
            }
            items.push(self.pattern_item(true)?);
            self.skip_named_pattern_line_breaks();
            if !matches!(self.peek().kind, TokKind::Comma) {
                break;
            }
            self.bump();
        }
        self.skip_named_pattern_line_breaks();
        self.expect(TokKind::RBracket, "to close the list pattern")?;
        let end = self.toks[self.pos.saturating_sub(1)].span.end;
        Ok(Pattern::List {
            items,
            rest,
            span: Span::new(start, end),
        })
    }

    fn map_pattern_body(&mut self, start: usize) -> Result<Pattern, Diagnostic> {
        let mut entries = Vec::new();
        let mut rest = None;
        loop {
            self.skip_named_pattern_line_breaks();
            if matches!(self.peek().kind, TokKind::RBracket) {
                break;
            }
            if matches!(self.peek().kind, TokKind::DotDot) {
                let dots = self.bump().span;
                self.skip_named_pattern_line_breaks();
                if !matches!(self.peek().kind, TokKind::RBracket) {
                    return Err(Self::rest_not_last(dots, "map"));
                }
                rest = Some(dots);
                break;
            }
            let key = self.map_pattern_key()?;
            let key_start = key.span().start;
            let optional = matches!(self.peek().kind, TokKind::Question);
            if optional {
                self.bump();
            }
            self.expect(TokKind::Colon, "between a map-pattern key and its pattern")?;
            let slot = self.pattern_item(true)?;
            let end = self.toks[self.pos.saturating_sub(1)].span.end;
            entries.push(crate::AST::MapPatEntry {
                key: Box::new(key),
                optional,
                slot,
                span: Span::new(key_start, end),
            });
            self.skip_named_pattern_line_breaks();
            if !matches!(self.peek().kind, TokKind::Comma) {
                break;
            }
            self.bump();
        }
        self.skip_named_pattern_line_breaks();
        self.expect(TokKind::RBracket, "to close the map pattern")?;
        let end = self.toks[self.pos.saturating_sub(1)].span.end;
        Ok(Pattern::Map {
            entries,
            rest,
            span: Span::new(start, end),
        })
    }

    /// D-PAT-MAP1: a map-pattern key is a literal, an enum case or a
    /// constant (`"type"`, `-1`, `.Red`, `Color.Red`, `MAX_KEY`); sema checks
    /// it. The key stops before `?:` / `:`.
    fn map_pattern_key(&mut self) -> Result<Expr, Diagnostic> {
        if matches!(self.peek().kind, TokKind::Minus) {
            let minus = self.bump().span;
            let inner = self.expr_primary(false)?;
            let span = Span::new(minus.start, inner.span().end);
            return Ok(Expr::Unary(crate::AST::UnOp::Neg, Box::new(inner), span));
        }
        let mut key = self.expr_primary(false)?;
        while matches!(self.peek().kind, TokKind::Dot)
            && matches!(self.peek2().kind, TokKind::Ident(_))
        {
            self.bump();
            let (member, member_span) = self.expect_ident("after `.` in a map-pattern key")?;
            key = Expr::Field(Box::new(key), member, member_span);
        }
        Ok(key)
    }

    /// D-PAT-NAMED-NEST1=A: in a condition, where `{` usually opens the body,
    /// `.Case{` starts named payload entries only when its first entry reads
    /// as one: `..`, `field:`, `field,`, or a lone `field}` directly followed
    /// by the body (`{` or `->`). No block starts with those tokens.
    pub(super) fn named_pattern_brace_ahead(&self) -> bool {
        let kind = |offset: usize| self.toks.get(self.pos + offset).map(|t| &t.kind);
        if !matches!(kind(0), Some(TokKind::LBrace)) {
            return false;
        }
        match kind(1) {
            Some(TokKind::DotDot) => true,
            Some(TokKind::Ident(name)) if name != Syntax::PAT_WILDCARD_SLOT => match kind(2) {
                Some(TokKind::Colon | TokKind::Comma) => true,
                Some(TokKind::RBrace) => matches!(
                    kind(3),
                    Some(TokKind::LBrace | TokKind::UnifiedArrow | TokKind::LambdaArrow)
                ),
                _ => false,
            },
            _ => false,
        }
    }

    /// D-PAT-NAMED-NEST1=A: the brace body of `.Case{field: pat, field, ..}`.
    /// Each entry is `field: slot` or the shorthand `field` (binds `field`);
    /// an optional `..` ends the list. Sema maps the entries onto the case's
    /// positional payload slots. Returns the slots and the closing `}` end.
    pub(super) fn named_pattern_slots(
        &mut self,
        variant: &str,
    ) -> Result<(Vec<crate::AST::PatSlot>, usize), Diagnostic> {
        use crate::AST::PatSlot;
        self.expect(TokKind::LBrace, "to open a named-field pattern")?;
        let mut slots = Vec::new();
        loop {
            self.skip_named_pattern_line_breaks();
            if matches!(self.peek().kind, TokKind::RBrace) {
                break;
            }
            if matches!(self.peek().kind, TokKind::DotDot) {
                let rest = self.bump().span;
                self.skip_named_pattern_line_breaks();
                if !matches!(self.peek().kind, TokKind::RBrace) {
                    return Err(Diagnostic::error(
                        "E0003",
                        "`..` must be the last entry of a named-field pattern".to_string(),
                        "`..` stands for every field the pattern does not name".to_string(),
                        format!("move `..` to the end: `.{variant}{{field: pattern, ..}}`"),
                        Some(rest),
                    ));
                }
                slots.push(PatSlot::Rest(rest));
                break;
            }
            let field = match &self.peek().kind {
                TokKind::Ident(name) if name != Syntax::PAT_WILDCARD_SLOT => name.clone(),
                _ => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("`.{variant}{{…}}` mixes a positional entry with field names"),
                        "a named-field pattern labels every entry; one pattern uses one form"
                            .to_string(),
                        format!(
                            "write `field: pattern` for this entry, or match by position with `.{variant}(…)`"
                        ),
                        Some(self.peek().span),
                    ));
                }
            };
            let field_span = self.bump().span;
            let slot = if matches!(self.peek().kind, TokKind::Colon) {
                self.bump();
                self.pattern_slot()?
            } else {
                PatSlot::Bind {
                    name: field.clone(),
                    span: field_span,
                }
            };
            slots.push(PatSlot::Named {
                field,
                field_span,
                slot: Box::new(slot),
            });
            self.skip_named_pattern_line_breaks();
            if !matches!(self.peek().kind, TokKind::Comma) {
                break;
            }
            self.bump();
        }
        self.expect(TokKind::RBrace, "after named-field pattern entries")?;
        let end = self.toks[self.pos.saturating_sub(1)].span.end;
        Ok((slots, end))
    }

    /// Line breaks inside a multi-line named-field pattern lex as `;`.
    fn skip_named_pattern_line_breaks(&mut self) {
        while matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
        }
    }
}
