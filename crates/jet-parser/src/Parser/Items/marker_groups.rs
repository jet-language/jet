use super::super::{
    Diagnostic, Item, Marker, Parser, Span, Syntax, TagDef, TokKind, TraitDef, describe,
};

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
        /// D-MARKSIG1=A: parse one marker with the ordinary call-argument
        /// reader; cursor sits on the name.
        pub(super) fn parse_one_marker(&mut self) -> Result<Marker, Diagnostic> {
            let negated = matches!(self.peek().kind, TokKind::Bang);
            if negated {
                self.bump();
            }
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
            let mut marker = self.finish_rule_marker(name, name_span)?;
            marker.negated = negated;
            if let Some(application) = self.rule_facts.last_mut() {
                application.marker.negated = negated;
            }
            Ok(marker)
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
            if parenthesized {
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
            if marker.name == Syntax::KW_TEST && marker.args.is_empty() {
                return Ok(());
            }
            use crate::Policy::RuleForm;
            if matches!(rule.status, crate::Policy::RuleStatus::Retired { .. }) {
                return Ok(());
            }
            let call_required = matches!(rule.form, RuleForm::Call)
                || matches!(rule.form, RuleForm::Block) && rule.signature.required() > 0;
            let bare_required = matches!(rule.form, RuleForm::Bare);
            // `#Unsafe` keeps its product diagnostic E3112 for a missing reason.
            if (call_required && !parenthesized && marker.name != Syntax::KW_UNSAFE)
                || bare_required && parenthesized
            {
                return Err(crate::Policy::marker_argument_shape_error(&marker.name, marker.span));
            }
            if marker.name == Syntax::KW_UNSAFE && marker.args.is_empty() {
                return Ok(());
            }
            self.validate_registered_rule_arguments(marker)
        }

        fn validate_registered_rule_arguments(&self, marker: &Marker) -> Result<(), Diagnostic> {
            let Some(rule) = crate::Policy::applied_rule(&marker.name) else {
                return Ok(());
            };
            if matches!(rule.status, crate::Policy::RuleStatus::Retired { .. })
                || marker.name == Syntax::KW_UNSAFE && marker.args.is_empty()
            {
                return Ok(());
            }
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
            let start = self.peek().span.start;
            self.expect(TokKind::Hash, "before a marker")?;
            let mut marker = self.parse_one_marker()?;
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
            let task_group = site == crate::Policy::RuleSite::Function
                && group
                    .iter()
                    .any(|marker| marker.name == Syntax::KW_TASK);
            for marker in &group {
                let task_doc = task_group && marker.name == Syntax::CONTRACT_DOC;
                if crate::Policy::applied_rule(&marker.name).is_some()
                    && !crate::Policy::rule_allows(&marker.name, site)
                    && !task_doc
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
                    diagnostic.edit = Some(crate::Diagnostics::TextEdit {
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
                        && matches!(name.as_str(), Syntax::KW_TASK | Syntax::ATTR_EVERY))
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
                    diagnostic.edit = Some(crate::Diagnostics::TextEdit {
                        span,
                        new_text: format!("#[{}]", rendered.join(", ")),
                    });
                }
                self.diags.push(diagnostic);
            }
            Ok(markers)
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
                Some(TokKind::KwUnsafe) => Some(Syntax::KW_UNSAFE),
                _ => None,
            }
        }

        fn target_marker_selects_file_web_at(&self, name_index: usize) -> bool {
            matches!(
                (
                    self.toks.get(name_index).map(|token| &token.kind),
                    self.toks.get(name_index + 1).map(|token| &token.kind),
                    self.toks.get(name_index + 2).map(|token| &token.kind),
                ),
                (
                    Some(TokKind::Ident(name)),
                    Some(TokKind::LParen),
                    Some(TokKind::Ident(target)),
                ) if name == Syntax::ATTR_TARGET && target == Syntax::WEB_TARGET_DEFAULT_WEB
            )
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
                    Some(TokKind::KwUnsafe) => Syntax::KW_UNSAFE,
                    _ => return false,
                };
                if negated
                    && !matches!(
                        name,
                        crate::Generics::PRINTABLE
                            | crate::Generics::EQUATABLE
                            | crate::Generics::DEBUG
                    )
                {
                    return false;
                }
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
                || matches!(self.marker_name_at(self.pos), Some(name)
                    if name == Syntax::ATTR_POLICY && self.policy_is_file_decl())
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
            if markers.iter().any(|marker| marker.name == Syntax::ATTR_FFI) {
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
            let task_group = site == crate::Policy::RuleSite::Function
                && ordered_markers
                    .iter()
                    .any(|marker| marker.name == Syntax::KW_TASK);
            let mut policy = Vec::new();
            for marker in markers {
                let task_doc = task_group && marker.name == Syntax::CONTRACT_DOC;
                if crate::Policy::applied_rule(&marker.name).is_some()
                    && !crate::Policy::rule_allows(&marker.name, site)
                    && !task_doc
                {
                    if site == crate::Policy::RuleSite::Method
                        && matches!(marker.name.as_str(), Syntax::KW_TASK | Syntax::ATTR_EVERY)
                    {
                        return Err(Diagnostic::error(
                            "E0925",
                            "`#Task`/`#Every(…)` only mark a top-level function".to_string(),
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
                if let Some(crate::Policy::RuleStatus::Retired { replacement }) =
                    crate::Policy::applied_rule(&marker.name).map(|rule| rule.status)
                {
                    match marker.name.as_str() {
                        Syntax::KW_PURE => {
                            self.diags.push(Self::retired_effect_syntax(Span::new(
                                marker.span.start,
                                marker.span.start + 1,
                            )));
                            function.is_pure = true;
                        }
                        "InlineAlways" => {
                            self.diags.push(Diagnostic::error(
                                "E0927",
                                "`#InlineAlways` is retired".to_string(),
                                "one `#Inline` marker carries both inline modes".to_string(),
                                format!("write `{replacement}`"),
                                Some(marker.span),
                            ));
                            function.is_inline_always = true;
                            function.inline_span = Some(marker.span);
                        }
                        _ => {
                            self.diags.push(Diagnostic::error(
                                "E0927",
                                format!("`#{}` is retired", marker.name),
                                "the applied-rule registry owns retired spellings and replacements"
                                    .to_string(),
                                format!("write `{replacement}`"),
                                Some(marker.span),
                            ));
                        }
                    }
                    continue;
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
                    Syntax::ATTR_POLICY => {
                        policy.extend(self.policy_declarations_from_marker(
                            marker.clone(),
                            crate::Policy::PolicyScope::Function,
                        )?);
                    }
                    Syntax::ATTR_META => {
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
                    Syntax::KW_TASK if marker.args.is_empty() => {
                        function.is_task = true;
                        function.task_span = Some(marker.span);
                    }
                    // D-TASKS-LIST1=A: only a group that also has `#Task`
                    // reaches this arm. Discovery reads the retained marker.
                    Syntax::CONTRACT_DOC => {}
                    Syntax::ATTR_EVERY => {
                        let Some(schedule) = arguments.parameter(0) else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                Syntax::ATTR_EVERY,
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
                    Syntax::ATTR_MUST_USE if marker.args.is_empty() => {
                        function.is_must_use = true;
                        function.must_use_span = Some(marker.span);
                    }
                    Syntax::ATTR_REPLAYABLE if marker.args.is_empty() => {
                        function.is_replayable = true;
                        function.replayable_span = Some(marker.span);
                    }
                    Syntax::ATTR_WASM_EXPORT if marker.args.is_empty() => {
                        function.web_marker =
                            Some(crate::Syntax::WebPartitionMarker::WasmExport);
                    }
                    Syntax::ATTR_TARGET => {
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
                    Syntax::KW_SANITIZER if marker.args.is_empty() => function.is_sanitizer = true,
                    Syntax::CONTRACT_INLINE => {
                        function.inline_span = Some(marker.span);
                        if arguments.parameter(0).is_none() {
                            function.is_inline = true;
                        } else if matches!(arguments.parameter(0), Some(crate::AST::Expr::Ident(mode, _)) if mode == "Always")
                        {
                            function.is_inline_always = true;
                        } else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                Syntax::CONTRACT_INLINE,
                                marker.span,
                            ));
                        }
                    }
                    Syntax::CONTRACT_PRE | Syntax::CONTRACT_POST => {
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
                        if marker.name == Syntax::CONTRACT_PRE {
                            function.pre.push(clause);
                        } else {
                            function.post.push(clause);
                        }
                    }
                    Syntax::KW_STATE => {
                        let Some(crate::AST::Expr::Ident(state, _)) = arguments.parameter(0) else {
                            return Err(crate::Policy::marker_argument_shape_error(
                                Syntax::KW_STATE,
                                marker.span,
                            ));
                        };
                        function.state_requires = Some((state.clone(), marker.span));
                    }
                    Syntax::KW_TRANSITION => {
                        let (
                            Some(crate::AST::Expr::Ident(from, _)),
                            Some(crate::AST::Expr::Ident(to, _)),
                        ) = (arguments.parameter(0), arguments.parameter(1))
                        else {
                            return Err(crate::Policy::marker_argument_shape_error(
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
                    Syntax::ATTR_FFI if function.inline_foreign.is_some() => {}
                    Syntax::ATTR_ABI => {
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
                Syntax::ATTR_POLICY
                    | Syntax::KW_UNSAFE
                    | Syntax::KW_SANITIZER
                    | Syntax::CONTRACT_PRE
                    | Syntax::CONTRACT_POST
                    | Syntax::CONTRACT_INLINE
                    | Syntax::CONTRACT_DOC
                    | Syntax::KW_TASK
                    | Syntax::ATTR_EVERY
                    | Syntax::ATTR_REPLAYABLE
                    | Syntax::ATTR_WASM_EXPORT
                    | Syntax::KW_STATE
                    | Syntax::KW_TRANSITION
                    | Syntax::ATTR_FFI
                    | Syntax::ATTR_ABI
                    | Syntax::ATTR_MUST_USE
                    | Syntax::ATTR_META
                    | Syntax::KW_REACTIVE
                    | Syntax::ATTR_TARGET
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
