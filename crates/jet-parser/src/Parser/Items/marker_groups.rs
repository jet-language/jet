use super::super::{
    Diagnostic, Item, Marker, Parser, Span, Syntax, TagDef, TokKind, TraitDef, describe,
};

/// D-VERDICT-1455-1: what the one shared marker reader returns before any
/// registry question is asked. The name is whatever was written.
pub(in crate::Parser) struct MarkerHead {
    pub(in crate::Parser) name: String,
    pub(in crate::Parser) name_span: Span,
    /// The written marker head: `#` (or `!`, inside a `#[…]` group) to the end
    /// of the name.
    pub(in crate::Parser) span: Span,
    pub(in crate::Parser) negated_span: Option<Span>,
}

pub(in crate::Parser) struct BoundRuleArguments<'m> {
    marker: &'m Marker,
    bindings: Vec<crate::Policy::RuleArgumentBinding>,
}

impl<'m> BoundRuleArguments<'m> {
    pub(in crate::Parser) fn parameter(&self, index: usize) -> Option<&'m crate::AST::Expr> {
        self.bindings
            .iter()
            .find(|binding| binding.parameter_index == Some(index))
            .map(|binding| &self.marker.args[binding.source_index])
    }

    pub(in crate::Parser) fn parameter_for_source(&self, source_index: usize) -> Option<usize> {
        self.bindings
            .iter()
            .find(|binding| binding.source_index == source_index)
            .and_then(|binding| binding.parameter_index)
    }

    pub(in crate::Parser) fn variadic(&self) -> impl Iterator<Item = &'m crate::AST::Expr> + '_ {
        self.bindings
            .iter()
            .filter(|binding| binding.parameter_index.is_none())
            .map(|binding| &self.marker.args[binding.source_index])
    }
}

impl<'a> Parser<'a> {
        pub(super) fn marker_ident_path(expression: &crate::AST::Expr) -> Option<String> {
            match expression {
                crate::AST::Expr::Ident(name, _) => Some(name.clone()),
                crate::AST::Expr::Field(base, member, _) => {
                    Some(format!("{}.{}", Self::marker_ident_path(base)?, member))
                }
                _ => None,
            }
        }

        pub(in crate::Parser) fn strip_marker_enum_prefix(path: String, enum_name: &str) -> String {
            let segments: Vec<&str> = path.split('.').collect();
            segments
                .iter()
                .position(|segment| *segment == enum_name)
                .and_then(|index| segments.get(index + 1..))
                .filter(|segments| !segments.is_empty())
                .map(|segments| segments.join("."))
                .unwrap_or(path)
        }

        pub(in crate::Parser) fn marker_enum_path(
            expression: &crate::AST::Expr,
            enum_name: &str,
        ) -> Option<String> {
            Self::marker_ident_path(expression)
                .map(|path| Self::strip_marker_enum_prefix(path, enum_name))
        }

        pub(super) fn marker_enum_variant(expression: &crate::AST::Expr) -> Option<&str> {
            match expression {
                crate::AST::Expr::Ident(name, _) => Some(name),
                crate::AST::Expr::Field(_, member, _) => Some(member),
                crate::AST::Expr::EnumLit { variant, .. } => Some(variant),
                _ => None,
            }
        }
        /// D-VERDICT-1455-1: the one reader for a written marker name. It runs
        /// at every marker position — item, statement, expression — and accepts
        /// any name, keyword-lexed ones included. The registry is consulted
        /// after the read, never during it. Cursor sits on `!` or the name.
        pub(in crate::Parser) fn read_marker_name(&mut self) -> Result<MarkerHead, Diagnostic> {
            let negated_span = matches!(self.peek().kind, TokKind::Bang)
                .then(|| self.bump().span);
            let name_token = self.bump();
            let TokKind::Ident(name) = name_token.kind else {
                return Err(Diagnostic::error(
                    "E0003",
                    "expected a marker name".to_string(),
                    "marker names follow `#` and use the registered applied-rule vocabulary"
                        .to_string(),
                    "write a registered marker name after `#`".to_string(),
                    Some(name_token.span),
                ));
            };
            let start = negated_span.unwrap_or(name_token.span).start;
            Ok(MarkerHead {
                name,
                name_span: name_token.span,
                span: Span::new(start, name_token.span.end),
                negated_span,
            })
        }

        /// The same reader with the leading `#` — every `#Name` position uses
        /// this, whatever the marker turns out to mean.
        pub(in crate::Parser) fn read_marker_head(&mut self) -> Result<MarkerHead, Diagnostic> {
            let hash = self.peek().span;
            self.expect(TokKind::Hash, "before a marker")?;
            let mut head = self.read_marker_name()?;
            head.span = Span::new(hash.start, head.name_span.end);
            Ok(head)
        }

        /// D-MARKSIG1=A: complete a read head with the ordinary call-argument
        /// reader.
        fn marker_from_head(&mut self, head: MarkerHead) -> Result<Marker, Diagnostic> {
            if let Some(negated_span) = head.negated_span {
                if !matches!(
                    head.name.as_str(),
                    crate::Generics::PRINTABLE
                        | crate::Generics::EQUATABLE
                        | crate::Generics::DEBUG
                ) {
                    return Err(Diagnostic::error(
                        "E0931",
                        format!("`!{}` is not a signed auto-derive trait", head.name),
                        "`!` rejects compiler generation only for Printable, Equatable, or Debug"
                            .to_string(),
                        format!("remove `!` from `#{}`, or use it with an auto-derived trait", head.name),
                        Some(Span::new(negated_span.start, head.name_span.end)),
                    ));
                }
            }
            let negated = head.negated_span.is_some();
            let mut marker = self.finish_rule_marker(head.name, head.name_span)?;
            marker.negated = negated;
            if let Some(application) = self.rule_facts.last_mut() {
                application.marker.negated = negated;
            }
            Ok(marker)
        }

        /// Parse one marker whose `#` was already consumed by the group form.
        pub(super) fn parse_one_marker(&mut self) -> Result<Marker, Diagnostic> {
            let head = self.read_marker_name()?;
            self.marker_from_head(head)
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
            let parenthesized = matches!(self.peek().kind, TokKind::LParen);
            let paren_start = self.peek().span.start;
            if parenthesized {
                self.bump(); // `(`
                if !matches!(self.peek().kind, TokKind::RParen) {
                    loop {
                        let arg = self.marker_call_arg()?;
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
            let marker = Marker {
                name,
                negated: false,
                name_span,
                args,
                arg_labels,
                span: Span::new(name_span.start, end),
                ct: None,
            };
            self.validate_registered_rule_marker(&marker, parenthesized)?;
            // D-MARK-FORM1=A: an empty pair is a leftover, not a different
            // spelling. Report it and keep parsing so `jet fmt` can apply the
            // delete edit — a hard stop would leave the file unformattable.
            if parenthesized
                && marker.args.is_empty()
                && crate::Policy::applied_rule(&marker.name)
                    .is_some_and(|rule| rule.signature.accepts_arguments())
            {
                self.diags.push(crate::Policy::marker_empty_arguments_error(
                    &marker.name,
                    Span::new(paren_start, end),
                ));
            }
            self.rule_facts.push(crate::AST::AppliedRuleApplication {
                marker: marker.clone(),
                target: None,
                site: None,
            });
            Ok(marker)
        }

        pub(in crate::Parser) fn bind_rule_fact(
            &mut self,
            name_span: Span,
            target: Option<Span>,
            site: crate::Policy::RuleSite,
        ) {
            if let Some(application) = self
                .rule_facts
                .iter_mut()
                .rev()
                .find(|application| application.marker.name_span == name_span)
            {
                application.target = target;
                application.site = Some(site);
            }
        }

        fn wrong_rule_site(
            marker: &Marker,
            site: crate::Policy::RuleSite,
            noun: &str,
        ) -> Diagnostic {
            Diagnostic::error(
                "E0355",
                format!("`#{}` cannot attach to this {noun}", marker.name),
                format!(
                    "the applied-rule registry does not allow `#{}` at the {site:?} site",
                    marker.name
                ),
                "remove the marker or move it to one of its registered sites".to_string(),
                Some(marker.span),
            )
        }

        fn validate_registered_rule_marker(
            &self,
            marker: &Marker,
            parenthesized: bool,
        ) -> Result<(), Diagnostic> {
            let Some(rule) = crate::Policy::applied_rule(&marker.name) else {
                return Ok(());
            };
            if matches!(rule.status, crate::Policy::RuleStatus::Retired { .. }) {
                return Ok(());
            }
            // D-MARK-FORM1=A, one placement law: the signature alone decides
            // whether parentheses may and must appear. There is no written-form
            // column and no per-marker grammar category.
            if parenthesized && !rule.signature.accepts_arguments() {
                return Err(crate::Policy::marker_argument_shape_error(&marker.name, marker.span));
            }
            // `#Unsafe` keeps its product diagnostic E3112 for a missing reason.
            if !parenthesized
                && rule.signature.arguments_required()
                && marker.name != Syntax::KW_UNSAFE
            {
                return Err(crate::Policy::marker_argument_shape_error(&marker.name, marker.span));
            }
            if marker.name == Syntax::KW_UNSAFE && marker.args.is_empty() {
                return Ok(());
            }
            self.validate_registered_rule_arguments(marker)
        }

        pub(in crate::Parser) fn validate_registered_rule_arguments(&self, marker: &Marker) -> Result<(), Diagnostic> {
            let Some(rule) = crate::Policy::applied_rule(&marker.name) else {
                return Ok(());
            };
            if matches!(rule.status, crate::Policy::RuleStatus::Retired { .. })
                || marker.name == Syntax::KW_UNSAFE && marker.args.is_empty()
            {
                return Ok(());
            }
            if marker.name == Syntax::CTX_BLOCK {
                if let Some((field_name, field_span)) =
                    marker.arg_labels.iter().flatten().find(|(field_name, _)| {
                        field_name != Syntax::CTX_FIELD_ALLOCATOR
                            && field_name != Syntax::CTX_FIELD_LOGGER
                            && field_name != Syntax::CTX_FIELD_DEADLINE
                    })
                {
                    return Err(Diagnostic::error(
                        "E0761",
                        format!("`{field_name}` isn't a context field"),
                        "the context bundle holds `allocator`, `logger`, and `deadline`".to_string(),
                        format!(
                            "write `#{}(allocator: …)`, `#{}(logger: …)`, or `#{}(deadline: …)`",
                            Syntax::CTX_BLOCK,
                            Syntax::CTX_BLOCK,
                            Syntax::CTX_BLOCK
                        ),
                        Some(*field_span),
                    ));
                }
            }
            // `#Rename(name: String)` shares the String signature gate with
            // `#Discriminant(field: String)` — Known string bindings and
            // quoted literals both resolve in sema; wrong types are E0930.
            if rule.signature.marker_argument_bindings(marker).is_none() {
                return Err(crate::Policy::marker_argument_shape_error(&marker.name, marker.span));
            }
            Ok(())
        }

        pub(in crate::Parser) fn bound_registered_rule_arguments<'m>(
            &self,
            marker: &'m Marker,
        ) -> Result<BoundRuleArguments<'m>, Diagnostic> {
            let Some(rule) = crate::Policy::applied_rule(&marker.name) else {
                return Ok(BoundRuleArguments {
                    marker,
                    bindings: Vec::new(),
                });
            };
            if matches!(rule.status, crate::Policy::RuleStatus::Retired { .. }) {
                return Ok(BoundRuleArguments {
                    marker,
                    bindings: Vec::new(),
                });
            }
            let Some(bindings) = rule.signature.marker_argument_bindings(marker) else {
                return Err(crate::Policy::marker_argument_shape_error(&marker.name, marker.span));
            };
            Ok(BoundRuleArguments { marker, bindings })
        }

        /// Shared entry for a bare `#Name` / `#Name(args)` application.
        pub(in crate::Parser) fn parse_rule_marker(&mut self) -> Result<Marker, Diagnostic> {
            let head = self.read_marker_head()?;
            let start = head.span.start;
            let mut marker = self.marker_from_head(head)?;
            marker.span.start = start;
            Ok(marker)
        }

        fn marker_fix_source(marker: &Marker) -> Option<String> {
            fn expr_source(expr: &crate::AST::Expr) -> Option<String> {
                match expr {
                    crate::AST::Expr::Str(parts, _) => {
                        let [crate::AST::StrPart::Lit(value)] = parts.as_slice() else {
                            return None;
                        };
                        Some(format!("{value:?}"))
                    }
                    crate::AST::Expr::Ident(name, _) => Some(name.clone()),
                    crate::AST::Expr::Int(value, _, _, source) => {
                        Some(source.clone().unwrap_or_else(|| value.to_string()))
                    }
                    crate::AST::Expr::Bool(value, _) => Some(value.to_string()),
                    crate::AST::Expr::UnitLit { raw, suffix, .. } => {
                        Some(format!("{raw}{suffix}"))
                    }
                    crate::AST::Expr::Field(base, field, _) => {
                        Some(format!("{}.{}", expr_source(base)?, field))
                    }
                    crate::AST::Expr::EnumLit {
                        type_name,
                        variant,
                        args,
                        ..
                    } if args.is_empty() => Some(if type_name.is_empty() {
                        format!(".{variant}")
                    } else {
                        format!("{type_name}.{variant}")
                    }),
                    _ => None,
                }
            }

            if marker.args.is_empty() {
                return Some(format!(
                    "{}{}",
                    if marker.negated { "!" } else { "" },
                    marker.name
                ));
            }
            let mut args = Vec::new();
            for (argument, label) in marker.args.iter().zip(&marker.arg_labels) {
                let value = expr_source(argument)?;
                args.push(match label {
                    Some((name, _)) => format!("{name}: {value}"),
                    None => value,
                });
            }
            Some(format!(
                "{}{}({})",
                if marker.negated { "!" } else { "" },
                marker.name,
                args.join(", ")
            ))
        }

        /// D-SHAPE2: parse one `#[ Name (, …)* ]` group; cursor on `@`.
        fn parse_marker_bracket_group(
            &mut self,
            site: crate::Policy::RuleSite,
            noun: &str,
        ) -> Result<Vec<Marker>, Diagnostic> {
            let group_span = self.bump().span; // `#`
            self.bump(); // `[`
            let mut group = Vec::new();
            loop {
                let m = self.parse_one_marker()?;
                self.bind_rule_fact(m.name_span, None, site);
                group.push(m);
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            let close = self.peek().span;
            self.expect(TokKind::RBracket, "to close an `#[…]` rule list")?;
            for marker in &group {
                if crate::Policy::applied_rule(&marker.name).is_some()
                    && !crate::Policy::rule_allows_with_companions(
                        &marker.name,
                        site,
                        group.iter().map(|other| other.name.as_str()),
                    )
                {
                    return Err(Self::wrong_rule_site(marker, site, noun));
                }
            }
            if group.len() == 1 {
                let mut diagnostic = Diagnostic::error(
                    "E0999",
                    "one marker is written without brackets".to_string(),
                    "brackets group two or more markers; one marker stays bare"
                        .to_string(),
                    format!("replace `#[{}]` with `#{}`", group[0].name, group[0].name),
                    Some(Span::new(group_span.start, close.end)),
                );
                if let Some(marker) = Self::marker_fix_source(&group[0]) {
                    diagnostic.set_structured_edit(crate::Diagnostics::TextEdit {
                        span: Span::new(group_span.start, close.end),
                        new_text: format!("#{marker}"),
                    });
                }
                self.diags.push(diagnostic);
            }
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            Ok(group)
        }

        pub(in crate::Parser) fn parse_attached_marker_sequence(
            &mut self,
            site: crate::Policy::RuleSite,
            noun: &str,
        ) -> Result<Vec<Marker>, Diagnostic> {
            let sequence_start = self.peek().span.start;
            let mut sequence_end = sequence_start;
            let mut markers = Vec::new();
            let mut chunks = 0usize;
            let mut bracket_chunks = 0usize;
            loop {
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                if self.at_marker_list() {
                    if !markers.is_empty()
                        && !self.bracket_group_allows_site(self.pos + 1, site)
                    {
                        break;
                    }
                    let close_index = Self::skip_bracket_group(&self.toks, self.pos + 1)
                        .ok_or_else(|| Diagnostic::error(
                            "E0003",
                            "this marker list is not closed".to_string(),
                            "a marker group ends with `]`".to_string(),
                            "close the marker group with `]`".to_string(),
                            Some(self.peek().span),
                        ))?;
                    sequence_end = self.toks[close_index - 1].span.end;
                    chunks += 1;
                    bracket_chunks += 1;
                    markers.extend(self.parse_marker_bracket_group(site, noun)?);
                    continue;
                }
                let Some(name) = self.marker_name_at(self.pos).map(str::to_string) else { break };
                if !markers.is_empty()
                    && crate::Policy::applied_rule(&name).is_some()
                    && !crate::Policy::rule_allows(&name, site)
                {
                    break;
                }
                chunks += 1;
                let marker = self.parse_rule_marker()?;
                self.bind_rule_fact(marker.name_span, None, site);
                if crate::Policy::applied_rule(&name).is_some()
                    && !crate::Policy::rule_allows(&name, site)
                    && !(site == crate::Policy::RuleSite::Method
                        && matches!(name.as_str(), Syntax::KW_JOB | Syntax::MARKER_EVERY))
                {
                    return Err(Self::wrong_rule_site(&marker, site, noun));
                }
                sequence_end = marker.span.end;
                markers.push(marker);
            }
            let distinct_markers = markers
                .iter()
                .map(|marker| marker.name.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len();
            if distinct_markers > 1 && !(chunks == 1 && bracket_chunks == 1) {
                let span = Span::new(sequence_start, sequence_end);
                let mut diagnostic = Diagnostic::error(
                    "E0999",
                    format!("adjacent {noun} markers belong in one bracket list"),
                    format!("two or more rules attached to one {noun} use one ordered `#[A, B]` group"),
                    "replace the adjacent markers with one bracket list".to_string(),
                    Some(span),
                );
                if let Some(rendered) = markers
                    .iter()
                    .map(Self::marker_fix_source)
                    .collect::<Option<Vec<_>>>()
                {
                    diagnostic.set_structured_edit(crate::Diagnostics::TextEdit {
                        span,
                        new_text: format!("#[{}]", rendered.join(", ")),
                    });
                }
                self.diags.push(diagnostic);
            }
            self.diagnose_repeated_markers(&markers, noun);
            Ok(markers)
        }

        /// D-MARK-REPEAT1=A: one rule written twice on one target is an error
        /// with a drop-the-repeat fix. Rows whose repetition carries meaning
        /// carry `repeatable` in the registry; the check reads that column and
        /// never a name list.
        pub(in crate::Parser) fn diagnose_repeated_markers(
            &mut self,
            markers: &[Marker],
            noun: &str,
        ) {
            for (index, marker) in markers.iter().enumerate() {
                if crate::Policy::applied_rule(&marker.name)
                    .is_none_or(|rule| rule.repeatable)
                {
                    continue;
                }
                if !markers[..index]
                    .iter()
                    .any(|earlier| earlier.name == marker.name)
                {
                    continue;
                }
                // No autofix: deleting one entry of `#[A, A]` in place would
                // leave a dangling comma. The writer removes the repeat.
                self.diags.push(crate::Policy::marker_repeated_error(
                    &marker.name,
                    noun,
                    marker.span,
                ));
            }
        }

        /// D-SHAPE2: parse leading `#[…]` applied-rule groups before a
        /// struct/enum field (e.g. `#[Redact, Rename("x")]`).
        /// Used at field position, which only ever supports the bracket form
        /// (no bare `#Redact`/`#Rename` without brackets).
        pub(super) fn parse_field_markers(
            &mut self,
            site: crate::Policy::RuleSite,
        ) -> Result<Vec<Marker>, Diagnostic> {
            let noun = if site == crate::Policy::RuleSite::Variant {
                "variant"
            } else {
                "field"
            };
            self.parse_attached_marker_sequence(site, noun)
        }

        fn marker_name_at(&self, index: usize) -> Option<&str> {
            if !matches!(
                self.toks.get(index).map(|token| &token.kind),
                Some(TokKind::Hash)
            ) {
                return None;
            }
            let name_index = index
                + if matches!(
                    self.toks.get(index + 1).map(|token| &token.kind),
                    Some(TokKind::Bang)
                ) {
                    2
                } else {
                    1
                };
            match self.toks.get(name_index).map(|token| &token.kind) {
                Some(TokKind::Ident(name)) => Some(name),
                _ => None,
            }
        }

        fn target_marker_selects_file_web_at(&self, name_index: usize) -> bool {
            if !matches!(
                (
                    self.toks.get(name_index).map(|token| &token.kind),
                    self.toks.get(name_index + 1).map(|token| &token.kind),
                ),
                (Some(TokKind::Ident(name)), Some(TokKind::LParen))
                    if name == Syntax::MARKER_TARGET
            ) {
                return false;
            }
            let mut segments = Vec::new();
            let mut cursor = name_index + 2;
            loop {
                match self.toks.get(cursor).map(|token| &token.kind) {
                    Some(TokKind::Ident(segment)) => segments.push(segment.as_str()),
                    Some(TokKind::Dot) => {}
                    Some(TokKind::RParen) => break,
                    _ => return false,
                }
                cursor += 1;
            }
            segments == [Syntax::WEB_TARGET_DEFAULT_WEB]
                || segments.ends_with(&["Target", Syntax::WEB_TARGET_DEFAULT_WEB])
        }

        fn skip_bare_marker(&self, index: usize) -> Option<usize> {
            if !matches!(self.toks.get(index).map(|token| &token.kind), Some(TokKind::Hash)) {
                return None;
            }
            self.marker_name_at(index)?;
            let mut cursor = index
                + if matches!(
                    self.toks.get(index + 1).map(|token| &token.kind),
                    Some(TokKind::Bang)
                ) {
                    3
                } else {
                    2
                };
            if matches!(self.toks.get(cursor).map(|token| &token.kind), Some(TokKind::LParen)) {
                let mut depth = 0usize;
                while let Some(token) = self.toks.get(cursor) {
                    match token.kind {
                        TokKind::LParen => depth += 1,
                        TokKind::RParen => {
                            depth = depth.checked_sub(1)?;
                            cursor += 1;
                            if depth == 0 {
                                break;
                            }
                            continue;
                        }
                        TokKind::Eof => return None,
                        _ => {}
                    }
                    cursor += 1;
                }
                if depth != 0 {
                    return None;
                }
            }
            Some(cursor)
        }

        fn bracket_group_allows_site(
            &self,
            open_index: usize,
            site: crate::Policy::RuleSite,
        ) -> bool {
            let mut cursor = open_index + 1;
            loop {
                let negated = matches!(
                    self.toks.get(cursor).map(|token| &token.kind),
                    Some(TokKind::Bang)
                );
                if negated {
                    cursor += 1;
                }
                let name = match self.toks.get(cursor).map(|token| &token.kind) {
                    Some(TokKind::Ident(name)) => name.as_str(),
                    _ => return false,
                };
                if site == crate::Policy::RuleSite::Function
                    && self.target_marker_selects_file_web_at(cursor)
                {
                    return false;
                }
                if !crate::Policy::rule_allows(name, site) {
                    return false;
                }
                cursor += 1;
                if matches!(self.toks.get(cursor).map(|token| &token.kind), Some(TokKind::LParen)) {
                    let mut depth = 0usize;
                    while let Some(token) = self.toks.get(cursor) {
                        match token.kind {
                            TokKind::LParen => depth += 1,
                            TokKind::RParen => {
                                let Some(next_depth) = depth.checked_sub(1) else { return false };
                                depth = next_depth;
                                cursor += 1;
                                if depth == 0 {
                                    break;
                                }
                                continue;
                            }
                            TokKind::Eof => return false,
                            _ => {}
                        }
                        cursor += 1;
                    }
                }
                match self.toks.get(cursor).map(|token| &token.kind) {
                    Some(TokKind::Comma) => cursor += 1,
                    Some(TokKind::RBracket) => return true,
                    _ => return false,
                }
            }
        }

        pub(in crate::Parser) fn marker_sequence_leads_to_function(&self) -> bool {
            if self.at_test_def()
                || self.at_bench_def()
                // D-MARK-META1=B: maturity is a `#Meta` field, not a
                // standalone marker. Let the top-level parser issue its
                // ordinary unknown-marker diagnostic instead of classifying
                // this spelling as an attached function marker.
                || matches!(self.marker_name_at(self.pos), Some(name)
                    if name == Syntax::MARKER_EXPERIMENTAL
                        || name == Syntax::MARKER_TESTED
                        || name == Syntax::MARKER_HARDENED)
                || matches!(self.marker_name_at(self.pos), Some(name)
                    if name == Syntax::MARKER_POLICY && self.policy_is_file_decl())
            {
                return false;
            }
            let mut cursor = self.pos;
            let mut saw_marker = false;
            let mut all_file_only = true;
            loop {
                while matches!(self.toks.get(cursor).map(|token| &token.kind), Some(TokKind::Semi)) {
                    cursor += 1;
                }
                let (next, file_only) = if matches!(
                    (
                        self.toks.get(cursor).map(|token| &token.kind),
                        self.toks.get(cursor + 1).map(|token| &token.kind),
                    ),
                    (Some(TokKind::Hash), Some(TokKind::LBracket))
                ) {
                    (
                        Self::skip_bracket_group(&self.toks, cursor + 1),
                        self.bracket_group_allows_site(
                            cursor + 1,
                            crate::Policy::RuleSite::File,
                        ) && !self.bracket_group_allows_site(
                            cursor + 1,
                            crate::Policy::RuleSite::Function,
                        ),
                    )
                } else {
                    let Some(name) = self.marker_name_at(cursor) else { break };
                    (
                        self.skip_bare_marker(cursor),
                        crate::Policy::rule_allows(name, crate::Policy::RuleSite::File)
                            && (!crate::Policy::rule_allows(
                                name,
                                crate::Policy::RuleSite::Function,
                            ) || self.target_marker_selects_file_web_at(cursor + 1)),
                    )
                };
                let Some(next) = next else { break };
                saw_marker = true;
                all_file_only &= file_only;
                cursor = next;
                if all_file_only {
                    break;
                }
            }
            while matches!(self.toks.get(cursor).map(|token| &token.kind), Some(TokKind::Semi)) {
                cursor += 1;
            }
            saw_marker
                && !all_file_only
                && (matches!(
                    self.toks.get(cursor).map(|token| &token.kind),
                    Some(TokKind::KwFn)
                ) || matches!(
                    (
                        self.toks.get(cursor).map(|token| &token.kind),
                        self.toks.get(cursor + 1).map(|token| &token.kind),
                    ),
                    (Some(TokKind::KwPub), Some(TokKind::KwFn))
                ))
        }

        pub(in crate::Parser) fn method_starts_here(&self) -> bool {
            matches!(self.peek().kind, TokKind::KwFn)
                || matches!(self.peek().kind, TokKind::KwPub)
                    && matches!(self.peek2().kind, TokKind::KwFn)
                || self.marker_sequence_leads_to_function()
        }

        fn parse_function_marker_sequence(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            self.parse_attached_marker_sequence(
                crate::Policy::RuleSite::Function,
                "function",
            )
        }

        pub(in crate::Parser) fn parse_method_marker_sequence(
            &mut self,
        ) -> Result<Vec<Marker>, Diagnostic> {
            self.parse_attached_marker_sequence(crate::Policy::RuleSite::Method, "method")
        }

        pub(super) fn marker_list_is_file_rules(&self) -> bool {
            if !self.at_marker_list() || self.marker_sequence_leads_to_function() {
                return false;
            }
            self.bracket_group_allows_site(self.pos + 1, crate::Policy::RuleSite::File)
        }

        pub(super) fn file_marker_stack_starts_here(&self) -> bool {
            if self.marker_sequence_leads_to_function() {
                return false;
            }
            if self.at_marker_list() {
                return self.marker_list_is_file_rules();
            }
            let mut cursor = self.pos;
            let mut count = 0usize;
            loop {
                while matches!(self.toks.get(cursor).map(|token| &token.kind), Some(TokKind::Semi)) {
                    cursor += 1;
                }
                let Some(name) = self.marker_name_at(cursor) else { break };
                if !crate::Policy::rule_allows(name, crate::Policy::RuleSite::File) {
                    break;
                }
                let Some(next) = self.skip_bare_marker(cursor) else { break };
                count += 1;
                cursor = next;
            }
            count > 1
        }

        pub(super) fn parse_file_marker_sequence(&mut self) -> Result<Vec<Marker>, Diagnostic> {
            self.parse_attached_marker_sequence(crate::Policy::RuleSite::File, "file")
        }

        /// D-MARK-STACK1=A: collect every marker attached to one function,
        /// diagnose non-canonical adjacent spellings, then lower through one
        /// applicator. FFI changes only the body parser.
        pub(in crate::Parser) fn func_with_marker_list(&mut self) -> Result<crate::AST::Func, Diagnostic> {
            let markers = self.parse_function_marker_sequence()?;
            if markers.iter().any(|marker| marker.name == Syntax::MARKER_FFI) {
                return self.ffi_fn_from_markers(markers);
            }
            let function = self.func()?;
            self.apply_function_markers(function, markers)
        }

        pub(in crate::Parser) fn apply_function_markers(
            &mut self,
            function: crate::AST::Func,
            markers: Vec<Marker>,
        ) -> Result<crate::AST::Func, Diagnostic> {
            self.apply_callable_markers(
                function,
                markers,
                crate::Policy::RuleSite::Function,
            )
        }

        pub(in crate::Parser) fn apply_method_markers(
            &mut self,
            function: crate::AST::Func,
            markers: Vec<Marker>,
        ) -> Result<crate::AST::Func, Diagnostic> {
            self.apply_callable_markers(function, markers, crate::Policy::RuleSite::Method)
        }

        fn apply_callable_markers(
            &mut self,
            mut function: crate::AST::Func,
            markers: Vec<Marker>,
            site: crate::Policy::RuleSite,
        ) -> Result<crate::AST::Func, Diagnostic> {
            let ordered_markers = markers.clone();
            let mut policy = Vec::new();
            for marker in markers {
                if crate::Policy::applied_rule(&marker.name).is_some()
                    && !crate::Policy::rule_allows_with_companions(
                        &marker.name,
                        site,
                        ordered_markers.iter().map(|other| other.name.as_str()),
                    )
                {
                    if site == crate::Policy::RuleSite::Method
                        && matches!(marker.name.as_str(), Syntax::KW_JOB | Syntax::MARKER_EVERY)
                    {
                        return Err(Diagnostic::error(
                            "E0925",
                            "`#Job`/`#Every(…)` only mark a top-level function".to_string(),
                            "a task needs a free-standing name for `jet run --task <name> <entry>` — a method has no such name, so it can't be one (D-JPK-TASKRUN1).".to_string(),
                            "move this function to the top level, beside `fn run()`.".to_string(),
                            Some(marker.span),
                        ));
                    }
                    return Err(Self::wrong_rule_site(
                        &marker,
                        site,
                        if site == crate::Policy::RuleSite::Method {
                            "method"
                        } else {
                            "function"
                        },
                    ));
                }
                // D-VERDICT-1455-1: a retired row teaches its replacement and
                // applies nothing. `#Pure` and `#InlineAlways` used to set
                // their flags after diagnosing, so retired spellings kept
                // working and the registry's status column lied.
                if let Some(crate::Policy::RuleStatus::Retired { replacement }) =
                    crate::Policy::applied_rule(&marker.name).map(|rule| rule.status)
                {
                    self.diags.push(Diagnostic::error(
                        "E0927",
                        format!("`#{}` is retired", marker.name),
                        "the applied-rule registry owns retired spellings and replacements"
                            .to_string(),
                        format!("write `{replacement}`"),
                        Some(marker.span),
                    ));
                    continue;
                }
                // D-VERDICT-1455-1: an unregistered name at a callable site is
                // a typo, not a user derive (derives attach to types), so it
                // gets the one E0927 vocabulary family instead of a site error.
                if crate::Policy::applied_rule(&marker.name).is_none() {
                    return Err(crate::Policy::marker_unknown_error(
                        &marker.name,
                        &crate::Policy::active_rule_names(),
                        marker.name_span,
                    ));
                }
                if !Self::function_marker_has_applicator(&marker.name) {
                    return Err(Diagnostic::error(
                        "E0355",
                        format!(
                            "`#{}` cannot attach through this function marker list",
                            marker.name
                        ),
                        "the marker registry gives every marker exact sites and a typed signature"
                            .to_string(),
                        "remove the marker or move it to its registered site".to_string(),
                        Some(marker.span),
                    ));
                }
                if marker.name == Syntax::KW_UNSAFE && marker.args.is_empty() {
                    self.apply_unsafe_function_marker(&mut function, &marker)?;
                    unreachable!("a missing Unsafe reason always returns E3112");
                }
                self.validate_registered_rule_arguments(&marker)?;
                let arguments = self.bound_registered_rule_arguments(&marker)?;
                match marker.name.as_str() {
                    Syntax::MARKER_POLICY => {
                        policy.extend(self.policy_declarations_from_marker(
                            marker.clone(),
                            crate::Policy::PolicyScope::Function,
                        )?);
                    }
                    Syntax::MARKER_META => {
                        if function.meta.is_some() {
                            return Err(Diagnostic::error(
                                "E0355",
                                "a function can have only one `#Meta` rule".to_string(),
                                "metadata fields belong in one function marker".to_string(),
                                "merge the fields into one `#Meta(...)` entry".to_string(),
                                Some(marker.span),
                            ));
                        }
                        let meta = self.meta_attr_from_marker(marker)?;
                        if let Some((maturity, span)) = meta.maturity() {
                            function.maturity = Some(maturity);
                            function.maturity_span = Some(span);
                        }
                        function.meta = Some(meta);
                    }
                    Syntax::KW_UNSAFE => {
                        if let Some(declaration) =
                            self.apply_unsafe_function_marker(&mut function, &marker)?
                        {
                            policy.push(declaration);
                        }
                    }
                    Syntax::KW_JOB => {
                        function.is_task = true;
                        function.task_span = Some(marker.span);
                        function.task_metadata = self.task_metadata_from_marker(&marker)?;
                    }
                    // D-TASKS-LIST1=A: only a group that also has `#Job`
                    // reaches this arm. Discovery reads the retained marker.
                    Syntax::MARKER_DOC => {}
                    Syntax::MARKER_EVERY => {
                        let Some(schedule) = arguments.parameter(0) else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                Syntax::MARKER_EVERY,
                                marker.span,
                            ));
                        };
                        let arg = match schedule {
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
                                    crate::AST::StrPart::Interp(..) => crate::AST::EveryArg::Expression(schedule.clone()),
                                }
                            }
                            other => crate::AST::EveryArg::Expression(other.clone()),
                        };
                        function.every = Some(crate::AST::EveryMarker {
                            arg,
                            span: marker.span,
                        });
                    }
                    Syntax::MARKER_MUST_USE if marker.args.is_empty() => {
                        function.is_must_use = true;
                        function.must_use_span = Some(marker.span);
                    }
                    Syntax::MARKER_REPLAYABLE if marker.args.is_empty() => {
                        function.is_replayable = true;
                        function.replayable_span = Some(marker.span);
                    }
                    Syntax::MARKER_WASM_EXPORT if marker.args.is_empty() => {
                        function.web_marker =
                            Some(crate::Syntax::WebPartitionMarker::WasmExport);
                    }
                    Syntax::MARKER_TARGET => {
                        function.web_marker = Some(match self.web_target_from_marker(&marker)? {
                            super::TargetMarker::Bucket(crate::Syntax::WebBucket::Wasm) => {
                                crate::Syntax::WebPartitionMarker::Wasm
                            }
                            super::TargetMarker::Bucket(crate::Syntax::WebBucket::JS) => {
                                crate::Syntax::WebPartitionMarker::JS
                            }
                            _ => {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    "`#Target` on a function needs `Wasm` or `JS`".to_string(),
                                    "function target rules select one web partition".to_string(),
                                    "write `#Target(Wasm)` or `#Target(JS)`".to_string(),
                                    Some(marker.span),
                                ));
                            }
                        });
                    }
                    Syntax::KW_REACTIVE if marker.args.is_empty() => function.is_reactive = true,
                    Syntax::KW_SCRUB => {
                        let Some(crate::AST::Expr::Ident(tag, _)) = arguments.parameter(0) else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                Syntax::KW_SCRUB,
                                marker.span,
                            ));
                        };
                        function.scrub_tag = Some(tag.clone());
                    }
                    Syntax::MARKER_INLINE => {
                        function.inline_span = Some(marker.span);
                        if arguments.parameter(0).is_none() {
                            function.is_inline = true;
                        } else if arguments
                            .parameter(0)
                            .and_then(Self::marker_enum_variant)
                            == Some("Always")
                        {
                            function.is_inline_always = true;
                        } else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                Syntax::MARKER_INLINE,
                                marker.span,
                            ));
                        }
                    }
                    // D-COMPUTE-KERNEL-SURFACE1=B: preserve the explicit
                    // kernel declaration in the AST. Sema attaches the proof
                    // only after it has checked the body.
                    Syntax::MARKER_KERNEL => {
                        if function.kernel.is_some() {
                            return Err(Diagnostic::error(
                                "E1130",
                                "a function may have only one `#Kernel` marker".to_string(),
                                "the kernel mode is a single declaration, not a stack of execution policies".to_string(),
                                "keep one `#Kernel(.parallel)` marker".to_string(),
                                Some(marker.span),
                            ));
                        }
                        let mode = match arguments
                            .parameter(0)
                            .and_then(Self::marker_enum_variant)
                        {
                            Some("parallel") => crate::AST::KernelMode::Parallel,
                            _ => {
                                return Err(crate::Policy::marker_argument_shape_error(
                                    Syntax::MARKER_KERNEL,
                                    marker.span,
                                ));
                            }
                        };
                        function.kernel = Some(crate::AST::KernelMarker {
                            mode,
                            span: marker.span,
                            proof: None,
                        });
                    }
                    Syntax::MARKER_PRE | Syntax::MARKER_POST => {
                        let (Some(condition), Some(message_argument)) =
                            (arguments.parameter(0), arguments.parameter(1))
                        else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                &marker.name,
                                marker.span,
                            ));
                        };
                        let message_span = match message_argument {
                            crate::AST::Expr::Str(parts, span) if parts.len() == 1 => {
                                match &parts[0] {
                                    crate::AST::StrPart::Lit(_) | crate::AST::StrPart::Interp(..) => *span,
                                }
                            }
                            other => other.span(),
                        };
                        let clause = crate::AST::ContractClause {
                            cond: condition.clone(),
                            message_expr: message_argument.clone(),
                            message_span,
                            span: marker.span,
                        };
                        if marker.name == Syntax::MARKER_PRE {
                            function.pre.push(clause);
                        } else {
                            function.post.push(clause);
                        }
                    }
                    Syntax::KW_STATE => {
                        let Some(state) = arguments
                            .parameter(0)
                            .and_then(Self::marker_ident_path)
                        else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                Syntax::KW_STATE,
                                marker.span,
                            ));
                        };
                        function.state_requires = Some((state, marker.span));
                    }
                    Syntax::KW_TRANSITION => {
                        let (Some(from), Some(to)) = (
                            arguments.parameter(0).and_then(Self::marker_ident_path),
                            arguments.parameter(1).and_then(Self::marker_ident_path),
                        )
                        else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                Syntax::KW_TRANSITION,
                                marker.span,
                            ));
                        };
                        function.state_transition = Some(crate::AST::StateTransition {
                            from: (from != Syntax::STATE_ENTRY).then_some(from),
                            to,
                            span: marker.span,
                        });
                    }
                    Syntax::MARKER_FFI if function.inline_foreign.is_some() => {}
                    Syntax::MARKER_UNDO => {
                        let Some(crate::AST::Expr::Ident(inverse, span)) = arguments.parameter(0) else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                Syntax::MARKER_UNDO,
                                marker.span,
                            ));
                        };
                        function.undo = Some((inverse.clone(), *span));
                    }
                    Syntax::MARKER_ABI => {
                        return Err(Diagnostic::error(
                            "E3212",
                            "`#ABI` only applies to C declarations".to_string(),
                            "ordinary Jet functions do not select a native C calling convention"
                                .to_string(),
                            "move the function into a `#Extern module c.<library> { … }` declaration or remove `#ABI`"
                                .to_string(),
                            Some(marker.span),
                        ));
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
            for declaration in &mut policy {
                declaration.target = Some(function.span);
            }
            if let Some(every) = &function.every {
                if !function.is_task {
                    self.diags
                        .push(Self::e0925_every_without_task(every.span));
                }
            }
            self.policy_declarations.extend(policy);
            // D-VERDICT-1455-1: keep the marker nodes on the callable. The
            // typed fields above stay for codegen; consumers read these instead
            // of rebuilding a marker from flags.
            function.markers = ordered_markers
                .iter()
                .filter(|marker| {
                    crate::Policy::applied_rule(&marker.name).is_some_and(|rule| {
                        matches!(rule.status, crate::Policy::RuleStatus::Active)
                    })
                })
                .cloned()
                .collect();
            for marker in &ordered_markers {
                self.bind_rule_fact(
                    marker.name_span,
                    Some(function.span),
                    site,
                );
            }
            self.applied_rules.extend(
                ordered_markers
                    .into_iter()
                    .filter(|marker| {
                        crate::Policy::applied_rule(&marker.name).is_some_and(|rule| {
                            matches!(rule.status, crate::Policy::RuleStatus::Active)
                        })
                    })
                    .map(|marker| crate::AST::AppliedRuleApplication {
                        marker,
                        target: Some(function.span),
                        site: Some(site),
                    }),
            );
            Ok(function)
        }

        pub(in crate::Parser) fn function_marker_has_applicator(name: &str) -> bool {
            matches!(
                name,
                Syntax::MARKER_POLICY
                    | Syntax::KW_UNSAFE
                    | Syntax::KW_SCRUB
                    | Syntax::MARKER_PRE
                    | Syntax::MARKER_POST
                    | Syntax::MARKER_INLINE
                    | Syntax::MARKER_KERNEL
                    | Syntax::MARKER_DOC
                    | Syntax::KW_JOB
                    | Syntax::MARKER_EVERY
                    | Syntax::MARKER_REPLAYABLE
                    | Syntax::MARKER_WASM_EXPORT
                    | Syntax::KW_STATE
                    | Syntax::KW_TRANSITION
                    | Syntax::MARKER_FFI
                    | Syntax::MARKER_UNDO
                    | Syntax::MARKER_ABI
                    | Syntax::MARKER_MUST_USE
                    | Syntax::MARKER_META
                    | Syntax::KW_REACTIVE
                    | Syntax::MARKER_TARGET
            )
        }

        pub(in crate::Parser) fn apply_unsafe_function_marker(
            &mut self,
            function: &mut crate::AST::Func,
            marker: &Marker,
        ) -> Result<Option<crate::Policy::PolicyDeclaration>, Diagnostic> {
            if marker.args.is_empty() {
                return Err(Diagnostic::error(
                    "E3112",
                    "an `#Unsafe` function needs a reason".to_string(),
                    "every unsafe function records why callers can rely on its unchecked contract"
                        .to_string(),
                    "write `#Unsafe(\"why this is safe\") fn …`".to_string(),
                    Some(marker.span),
                ));
            }
            let arguments = self.bound_registered_rule_arguments(marker)?;
            let reason = match arguments.parameter(0) {
                Some(crate::AST::Expr::Str(parts, _)) if parts.len() == 1 => match &parts[0] {
                    crate::AST::StrPart::Lit(value) => Some(value.clone()),
                    crate::AST::StrPart::Interp(..) => None,
                },
                _ => None,
            };
            function.is_unsafe = true;
            function.unsafe_reason = reason;
            function.unsafe_span = Some(marker.span);
            let Some(value) = arguments.parameter(1) else { return Ok(None) };
            let mode = match value {
                crate::AST::Expr::EnumLit {
                    type_name,
                    variant,
                    args,
                    ..
                } if type_name.is_empty() && args.is_empty() => variant.as_str(),
                _ => {
                    return Err(crate::Policy::marker_argument_shape_error(
                        Syntax::KW_UNSAFE,
                        value.span(),
                    ));
                }
            };
            let value = match mode {
                "None" => return Ok(None),
                "Track" => crate::Policy::PolicyValue::UnsafeTrack,
                "Skip" => crate::Policy::PolicyValue::UnsafeSkip,
                _ => {
                    return Err(Diagnostic::error(
                        "E3108",
                        format!("`.{mode}` is not a per-site obligation mode"),
                        "a gate tracks typed obligations, skips them when policy permits, or keeps the default"
                            .to_string(),
                        "write `.Track`, `.Skip`, or `.None`".to_string(),
                        Some(value.span()),
                    ));
                }
            };
            Ok(Some(crate::Policy::PolicyDeclaration {
                key: crate::Policy::PolicyKey::Unsafe,
                value,
                scope: crate::Policy::PolicyScope::Function,
                span: marker.span,
                target: Some(function.span),
                source: "<source>".to_string(),
            }))
        }
    
        /// Split parsed markers on a struct/enum: derive-trait markers
        /// (`Codable`→`Encode`+`Decode`, `Encode`, `Decode`, `Debug`, `Summarize`,
        /// `Comparable`, user traits) are pushed onto `derives`; serde *attribute*
        /// markers are returned raw for sema. Markers arrive already validated for
        /// This only classifies each rule's job after the single `@` parser.
        fn split_type_markers(markers: Vec<Marker>, derives: &mut Vec<(String, Span)>) -> Vec<Marker> {
            let mut serde = Vec::new();
            for m in markers {
                if m.negated
                    && matches!(
                        m.name.as_str(),
                        crate::Generics::PRINTABLE
                            | crate::Generics::EQUATABLE
                            | crate::Generics::DEBUG
                    )
                {
                    continue;
                }
                match m.name.as_str() {
                    Syntax::MARKER_CODABLE => {
                        derives.push((Syntax::MARKER_ENCODE.to_string(), m.name_span));
                        derives.push((Syntax::MARKER_DECODE.to_string(), m.name_span));
                    }
                    Syntax::MARKER_ENCODE => derives.push((Syntax::MARKER_ENCODE.to_string(), m.name_span)),
                    Syntax::MARKER_DECODE => derives.push((Syntax::MARKER_DECODE.to_string(), m.name_span)),
                    Syntax::MARKER_RENAME_ALL
                    | Syntax::MARKER_DENY_UNKNOWN_FIELDS
                    | Syntax::MARKER_TAG
                    | Syntax::MARKER_UNTAGGED
                    | Syntax::MARKER_RENAME
                    | Syntax::MARKER_SKIP
                    | Syntax::MARKER_DEFAULT
                    | Syntax::MARKER_FLATTEN => serde.push(m),
                    // D-REPRC1: `Layout` is a type-layout fact, not a derive.
                    // Keep it in `type_markers`; `attach_type_markers` projects
                    // its variant into `StructDef.layout` below.
                    Syntax::MARKER_LAYOUT => {}
                    // Any other name is a derive-trait: the D-VERDICT-732-1
                    // (formerly D-MARKERMOVE3) built-ins (`#[Debug]`,
                    // `#[Summarize]`, `#[Comparable]`) or a user derive-trait name.
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
            let markers = markers
                .into_iter()
                .filter(|marker| {
                    let Some(crate::Policy::RuleStatus::Retired { replacement }) =
                        crate::Policy::applied_rule(&marker.name).map(|rule| rule.status)
                    else {
                        return true;
                    };
                    self.diags.push(Diagnostic::error(
                        "E0927",
                        format!("`#{}` is retired", marker.name),
                        "the applied-rule registry owns retired spellings and replacements"
                            .to_string(),
                        format!("write `{replacement}`"),
                        Some(marker.span),
                    ));
                    false
                })
                .collect::<Vec<_>>();
            let target = match &item {
                Item::Struct(item) => item.span,
                Item::Enum(item) => item.span,
                Item::Distinct(item) => item.span,
                _ => Span::new(0, 0),
            };
            for marker in &markers {
                self.bind_rule_fact(
                    marker.name_span,
                    Some(target),
                    crate::Policy::RuleSite::Type,
                );
            }
            Ok(match item {
                Item::Struct(mut s) => {
                    // D-REPRC1: a bracket marker list is the canonical way to
                    // combine `#Layout(...)` with another type marker. The
                    // dedicated bare-layout parser fills these fields for the
                    // single-marker spelling; mirror it here for
                    // `#[Layout(c), Codable]` and user derive markers.
                    if let Some(marker) = markers
                        .iter()
                        .find(|marker| marker.name == Syntax::MARKER_LAYOUT)
                    {
                        if let Some(crate::AST::Expr::Ident(variant, _)) = marker.args.first() {
                            s.layout = match variant.as_str() {
                                Syntax::LAYOUT_C => Some(crate::AST::StructLayout::C),
                                Syntax::LAYOUT_COLUMNAR => {
                                    Some(crate::AST::StructLayout::Columnar)
                                }
                                _ => s.layout,
                            };
                            s.layout_span = Some(marker.span);
                        }
                    }
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
                        .find(|m| m.name == Syntax::MARKER_PUBLISHED_SCHEMA)
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
                            Syntax::MARKER_NUMERIC
                            | Syntax::MARKER_BUNDLE_COMPARABLE
                            | Syntax::MARKER_BUNDLE_PRINTABLE => {
                                d.derives.push((marker.name.clone(), marker.name_span));
                            }
                            Syntax::MARKER_BUNDLE_CODABLE_AS_BASE => {
                                d.derives
                                    .push((crate::Generics::ENCODE.to_string(), marker.name_span));
                                d.derives
                                    .push((crate::Generics::DECODE.to_string(), marker.name_span));
                            }
                            Syntax::MARKER_INVARIANT => {
                                let (bounds, span, text) =
                                    self.parse_invariant_range(marker)?;
                                d.range = bounds.map(|(low, high)| (low, high, span));
                                d.invariant = text.map(|text| (text, span));
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
                        TokKind::Ident(n) if n == Syntax::MARKER_LAYOUT
                    ) =>
                {
                    self.layout_type_def(is_pub)
                }
                TokKind::Hash
                    if matches!(
                        &self.peek2().kind,
                        TokKind::Ident(n) if n == Syntax::MARKER_PUBLISHED_SCHEMA
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
            let markers =
                self.parse_attached_marker_sequence(crate::Policy::RuleSite::Type, "type")?;
            let item = self.parse_type_after_markers()?;
            self.attach_type_markers(markers, item)
        }
    
        /// S28 + D-ARROW-CONTROL1: top-level
        /// `trait Name { fn sig(self) => T; … }`.
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
                // D-VERDICT-1455-1 / #1492: trait-body markers share the callable
                // registry path — retired `#Pure` is E0927 with the same fix text.
                let markers = if matches!(self.peek().kind, TokKind::Hash) {
                    self.parse_method_marker_sequence()?
                } else {
                    Vec::new()
                };
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                for marker in &markers {
                    if let Some(crate::Policy::RuleStatus::Retired { replacement }) =
                        crate::Policy::applied_rule(&marker.name).map(|rule| rule.status)
                    {
                        self.diags.push(Diagnostic::error(
                            "E0927",
                            format!("`#{}` is retired", marker.name),
                            "the applied-rule registry owns retired spellings and replacements"
                                .to_string(),
                            format!("write `{replacement}`"),
                            Some(marker.span),
                        ));
                        continue;
                    }
                    if crate::Policy::applied_rule(&marker.name).is_none() {
                        return Err(crate::Policy::marker_unknown_error(
                            &marker.name,
                            &crate::Policy::active_rule_names(),
                            marker.name_span,
                        ));
                    }
                }
                methods.push(self.trait_method_sig(false)?);
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
    
        /// D-TAG-SURFACE1=A: `tag Name { deny: [...], from: [...] }`.
        pub(in crate::Parser) fn tag_def(&mut self, nested: bool) -> Result<TagDef, Diagnostic> {
            let (is_pub, is_package_pub) = if nested {
                (false, false)
            } else {
                self.parse_item_visibility()
            };
            let start = self.peek().span;
            self.expect_kw(TokKind::KwTag, "to start a tag definition")?;
            let (name, name_span) = self.expect_ident("after `tag`")?;
            if !matches!(self.peek().kind, TokKind::LBrace) {
                return Err(Diagnostic::error(
                    "E0734",
                    format!("tag `{name}` needs a policy body"),
                    "a tag declares a dataflow fact, including where that fact is denied"
                        .to_string(),
                    format!("write `tag {name} {{ deny: [Effect] }}`"),
                    Some(name_span),
                ));
            }
            self.bump();
            let mut deny = None;
            let mut from = None;
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi | TokKind::Comma) {
                    self.bump();
                    continue;
                }
                if matches!(self.peek().kind, TokKind::KwFn) {
                    let method = self.trait_method_sig(false)?;
                    return Err(crate::Generics::e0732(
                        &name,
                        &method.name,
                        method.name_span,
                    ));
                }
                let (field, field_span) = self.expect_ident("for a tag policy field")?;
                self.expect(TokKind::Colon, "after a tag policy field")?;
                let values = self.tag_policy_path_list()?;
                match field.as_str() {
                    "deny" if deny.is_none() => deny = Some(values),
                    "from" if from.is_none() => from = Some(values),
                    "deny" | "from" => {
                        return Err(Diagnostic::error(
                            "E0734",
                            format!("tag `{name}` repeats `{field}`"),
                            "each tag policy field is declared once".to_string(),
                            format!("keep one `{field}: [...]` entry"),
                            Some(field_span),
                        ));
                    }
                    _ => {
                        return Err(Diagnostic::error(
                            "E0734",
                            format!("`{field}` is not a tag policy field"),
                            "tag bodies have required `deny` and optional `from` fields"
                                .to_string(),
                            "write `deny: [...]` or `from: [...]`".to_string(),
                            Some(field_span),
                        ));
                    }
                }
            }
            let end = self.peek().span.end;
            self.expect(TokKind::RBrace, "to close the tag policy")?;
            let deny = deny.unwrap_or_default();
            if deny.is_empty() {
                return Err(Diagnostic::error(
                    "E0734",
                    format!("tag `{name}` needs at least one denied destination"),
                    "a tag without a denied destination has no enforceable policy".to_string(),
                    "add a sink or effect to `deny: [...]`".to_string(),
                    Some(name_span),
                ));
            }
            Ok(TagDef {
                is_pub,
                is_package_pub,
                name,
                name_span,
                deny,
                from: from.unwrap_or_default(),
                span: Span::new(start.start, end),
            })
        }

        fn tag_policy_path_list(&mut self) -> Result<Vec<(String, Span)>, Diagnostic> {
            self.expect(TokKind::LBracket, "to open a tag policy list")?;
            let mut paths = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBracket | TokKind::Eof) {
                let (mut path, start) = self.expect_ident("in a tag policy list")?;
                let mut end = start.end;
                while matches!(self.peek().kind, TokKind::Dot) {
                    self.bump();
                    let (segment, span) = self.expect_ident("after `.` in a tag policy path")?;
                    path.push('.');
                    path.push_str(&segment);
                    end = span.end;
                }
                paths.push((path, Span::new(start.start, end)));
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect(TokKind::RBracket, "to close a tag policy list")?;
            Ok(paths)
        }
    
}
