impl<'a> Parser<'a> {
        /// D-MUSTUSE1 / D-MARKERMOVE1 (I7/R3 chokepoint): consume a `@MustUse` /
        /// retired `@MustUse` prefix already confirmed present by `at_must_use_fn`.
        /// Teaches E0062 and keeps parsing when the retired `#` spelling is used,
        /// instead of cascading into an unrelated parse error.
        fn bump_must_use_marker(&mut self) -> Result<Span, Diagnostic> {
            let start = self.peek().span;
            let sigil = self.bump(); // `#` or `@`
            let (name, name_span) = self.expect_ident("after the marker sigil")?;
            if matches!(sigil.kind, TokKind::Hash) {
                self.diags
                    .push(Self::e0062_contract_on_hash(&name, name_span));
            }
            Ok(Span::new(start.start, name_span.end))
        }
    
        /// S60 (D-CASING1 follow-on) / D-MARKERMOVE1/2: consume a `@Pure` /
        /// retired `@Pure` prefix already confirmed present by `at_pure_fn`.
        /// Teaches E0062 when the retired `#` spelling is used.
        fn bump_pure_marker(&mut self) {
            let sigil = self.bump(); // `#` or `@`
            let name_tok = self.bump(); // `Pure`
            if matches!(sigil.kind, TokKind::Hash) {
                self.diags.push(Self::e0062_contract_on_hash(
                    Syntax::KW_PURE,
                    Span::new(sigil.span.start, name_tok.span.end),
                ));
            }
        }
    
        /// D-METHODMACRO1=A (I7/R3 chokepoint): consume a `@Inline`/`@InlineAlways`
        /// prefix already confirmed present by `at_inline_fn`. Returns `true` when
        /// the marker was `InlineAlways` (`false` for the soft `Inline` hint),
        /// plus its span. Teaches E0062 when the retired `#` spelling is used.
        fn bump_inline_marker(&mut self) -> Result<(bool, Span), Diagnostic> {
            let start = self.peek().span;
            let sigil = self.bump(); // `#` or `@`
            let (name, name_span) = self.expect_ident("after the marker sigil")?;
            if matches!(sigil.kind, TokKind::Hash) {
                self.diags
                    .push(Self::e0062_contract_on_hash(&name, name_span));
            }
            let is_always = name == Syntax::CONTRACT_INLINE_ALWAYS;
            Ok((is_always, Span::new(start.start, name_span.end)))
        }
    
        /// D-METHODMACRO1=A: parse the `@Inline`/`@InlineAlways` marker slot —
        /// zero or one marker, with a second one (either name, either order)
        /// rejected as E0920 (pick one). Shared by `func()` and `method_in_type()`.
        fn parse_inline_marker(&mut self) -> Result<(bool, bool, Option<Span>), Diagnostic> {
            if !self.at_inline_fn() {
                return Ok((false, false, None));
            }
            let (is_always, span) = self.bump_inline_marker()?;
            let (mut is_inline, mut is_inline_always) = (!is_always, is_always);
            let mut span = Some(span);
            if self.at_inline_fn() {
                let (is_always2, span2) = self.bump_inline_marker()?;
                self.diags
                    .push(Self::e0920_conflicting_inline_markers(span2));
                is_inline = !is_always2;
                is_inline_always = is_always2;
                span = Some(span2);
            }
            Ok((is_inline, is_inline_always, span))
        }
    
        /// E0920: both `@Inline` and `@InlineAlways` were written on the same
        /// function/method declaration.
        pub(super) fn e0920_conflicting_inline_markers(span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0920",
                "a function can't be both `@Inline` and `@InlineAlways`".to_string(),
                "`@Inline` is a soft hint the compiler may ignore; `@InlineAlways` is a checked \
                 promise it must honor or reject — one declaration can't carry both meanings."
                    .to_string(),
                "keep one: `@Inline` to suggest inlining, `@InlineAlways` to require it.".to_string(),
                Some(span),
            )
        }
    
        pub(super) fn func(&mut self) -> Result<Func, Diagnostic> {
            // D-MUSTUSE1 / D-MARKERMOVE1: `@MustUse fn` / `@MustUse pub fn`.
            let (is_must_use, must_use_span) = if self.at_must_use_fn() {
                (true, Some(self.bump_must_use_marker()?))
            } else {
                (false, None)
            };
            // S60 (D-CASING1 follow-on) / D-MARKERMOVE2: `@Pure fn` / `@Pure pub fn`.
            let is_pure = if self.at_pure_fn() {
                self.bump_pure_marker();
                true
            } else {
                false
            };
            // D-TAINT1: `#Sanitizer fn` / `#Sanitizer pub fn` — the taint-strip
            // modifier (guaranteed by `at_sanitizer_fn` when present at dispatch).
            let is_sanitizer = if self.at_sanitizer_fn() {
                self.bump(); // `#`
                self.bump(); // `Sanitizer`
                true
            } else {
                false
            };
            // D-METHODMACRO1=A: `@Inline fn` / `@InlineAlways fn` — checked inline
            // contracts (E0920 if both are written).
            let (is_inline, is_inline_always, inline_span) = self.parse_inline_marker()?;
            // D-STATE1: `#State(S) fn …` / `#Transition(From -> To) fn …` typestate
            // markers. Each appears at most once before `fn`; either may precede the
            // (already-consumed) `@Pure`/`#Sanitizer` slots or follow them.
            let mut state_requires = None;
            let mut state_transition = None;
            let mut web_marker = None;
            let mut is_replayable = false;
            let mut replayable_span = None;
            // D-PREPOST1: `@Pre(cond, "msg")` / `@Post(cond, "msg")` — repeatable,
            // any order, alongside the typestate/web markers above.
            let mut pre = Vec::new();
            let mut post = Vec::new();
            loop {
                // D-PREPOST1: a stacked marker sequence (`@Pre(…)` / `@Post(…)` /
                // typestate / web) may have a lexer-inserted `;` between lines —
                // skip it before checking for the next marker, not just once
                // after the whole loop (the pre-existing single skip below only
                // covered the tail, before `fn`/`pub`).
                while matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                if state_requires.is_none() && self.at_state_fn() {
                    state_requires = Some(self.parse_state_require_marker()?);
                } else if state_transition.is_none() && self.at_transition_fn() {
                    state_transition = Some(self.parse_transition_marker()?);
                } else if !is_replayable && self.at_replayable_fn() {
                    let start = self.bump().span.start; // `#`
                    let end = self.bump().span.end; // `Replayable`
                    is_replayable = true;
                    replayable_span = Some(Span::new(start, end));
                } else if self.at_contract_clause_fn(Syntax::CONTRACT_PRE) {
                    pre.push(self.parse_contract_clause(Syntax::CONTRACT_PRE)?);
                } else if self.at_contract_clause_fn(Syntax::CONTRACT_POST) {
                    post.push(self.parse_contract_clause(Syntax::CONTRACT_POST)?);
                } else if web_marker.is_none() {
                    if let Some(m) = self.try_parse_web_partition_marker()? {
                        web_marker = Some(m);
                    } else {
                        break;
                    }
                } else if self.try_parse_web_partition_marker()?.is_some() {
                    let span = self.peek().span;
                    return Err(Diagnostic::error(
                        "E0003",
                        "only one web partition marker (`#Wasm`, `#Js`, `#WasmExport`) is allowed per function"
                            .to_string(),
                        "per-function web overrides are mutually exclusive".to_string(),
                        "keep one of `#Wasm`, `#Js`, or `#WasmExport`".to_string(),
                        Some(span),
                    ));
                } else {
                    break;
                }
            }
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let f = self.func_with_modifiers_full(
                is_pure,
                is_sanitizer,
                state_requires,
                state_transition,
                web_marker,
                is_must_use,
                must_use_span,
                is_inline,
                is_inline_always,
                inline_span,
                is_replayable,
                replayable_span,
            )?;
            Ok(Func { pre, post, ..f })
        }
    
        /// D-PREPOST1: is the cursor at `@Pre(`/`@Post(` (or the retired `#`
        /// spelling, so `parse_contract_clause` can teach E0062)?
        fn at_contract_clause_fn(&self, kw: &str) -> bool {
            matches!(self.peek().kind, TokKind::Hash | TokKind::At)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == kw)
                && matches!(self.peek3().kind, TokKind::LParen)
        }
    
        /// D-PREPOST1: parse `@Pre(cond, "msg")` / `@Post(cond, "msg")`; cursor on
        /// the sigil. `kw` is `CONTRACT_PRE` or `CONTRACT_POST`.
        fn parse_contract_clause(
            &mut self,
            kw: &str,
        ) -> Result<crate::AST::ContractClause, Diagnostic> {
            let start = self.peek().span;
            let sigil = self.bump(); // `#` or `@`
            let (_, name_span) = self.expect_ident("after the marker sigil")?;
            if matches!(sigil.kind, TokKind::Hash) {
                self.diags.push(Self::e0062_contract_on_hash(kw, name_span));
            }
            self.expect(TokKind::LParen, &format!("after `@{kw}`"))?;
            let cond = self.expr_no_struct_lit()?;
            self.expect(
                TokKind::Comma,
                &format!("between the condition and message in `@{kw}(…)`"),
            )?;
            let (message, message_span) = self.expect_marker_name(kw)?;
            let end = self.peek().span;
            self.expect(TokKind::RParen, &format!("to close `@{kw}(…)`"))?;
            Ok(crate::AST::ContractClause {
                cond,
                message,
                message_span,
                span: Span::new(start.start, end.end),
            })
        }
    
        /// D-WASM1=A: is the cursor at `#Target(Wasm|Js)`?
        pub(super) fn at_web_target(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_TARGET)
                && matches!(self.peek3().kind, TokKind::LParen)
        }
    
        /// D-HTMLPAIR1 (ratified 2026-07-01, c134): detect `#Html(`.
        pub(super) fn at_html_marker(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_HTML)
                && matches!(self.peek3().kind, TokKind::LParen)
        }
    
        /// D-HTMLPAIR1 (ratified 2026-07-01, c134): parse `#Html("path.html")` — the file's
        /// explicit companion host page for `--target=web` builds.
        pub(super) fn parse_html_marker(&mut self) -> Result<String, Diagnostic> {
            self.bump(); // `#`
            self.bump(); // `Html`
            self.expect(TokKind::LParen, "after `#Html`")?;
            let parts = match &self.peek().kind {
                TokKind::Str(parts) => parts.clone(),
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected a path in quotes inside `#Html(…)`, found {}",
                            describe(other)
                        ),
                        "the companion host page is a fixed file path".to_string(),
                        "write `#Html(\"index.html\")`".to_string(),
                        Some(self.peek().span),
                    ));
                }
            };
            let span = self.bump().span;
            self.expect(TokKind::RParen, "to close `#Html(…)`")?;
            if parts.len() != 1 {
                return Err(Diagnostic::error(
                    "E0003",
                    "a `#Html(…)` path must be one piece of quoted text".to_string(),
                    "paths are fixed labels, not interpolated messages".to_string(),
                    "write `#Html(\"index.html\")`".to_string(),
                    Some(span),
                ));
            }
            match &parts[0] {
                StrTokPart::Lit(s) => Ok(s.clone()),
                StrTokPart::Interp(_) => Err(Diagnostic::error(
                    "E0003",
                    "a `#Html(…)` path can't contain `{ }` interpolation".to_string(),
                    "paths are fixed labels".to_string(),
                    "write `#Html(\"index.html\")`".to_string(),
                    Some(span),
                )),
            }
        }
    
        /// D-WASM1=A / D-WEBDEFAULT1 (ratified 2026-07-01, c134): `#Target(Wasm)` / `#Target(Js)`
        /// (a partition ceiling) or `#Target(Web)` (this file's default CLI
        /// backend — a different axis, same marker).
        pub(super) fn parse_web_target_marker(&mut self) -> Result<TargetMarker, Diagnostic> {
            self.bump(); // `#`
            self.bump(); // `Target`
            self.expect(TokKind::LParen, "after `#Target`")?;
            let (name, name_span) = self.expect_ident(
                "the partition name inside `#Target(…)` (`Wasm`, `Js`, `Web`, or `Os.Linux`/`Os.Macos`/`Os.Windows`)",
            )?;
            // D-OSTARGET1=A: `#Target(Os.Linux|Os.Macos|Os.Windows)` — the second,
            // mutually-exclusive axis (native platform gating on an `impl`).
            if name == crate::Syntax::TARGET_OS_NAMESPACE {
                self.expect(TokKind::Dot, "after `Os` inside `#Target(Os. … )`")?;
                let (os_name, os_span) = self.expect_ident(
                    "the OS name inside `#Target(Os. … )` (`Linux`, `Macos`, or `Windows`)",
                )?;
                self.expect(TokKind::RParen, "to close `#Target(…)`")?;
                return crate::Syntax::OsTarget::parse(&os_name)
                    .map(TargetMarker::Os)
                    .ok_or_else(|| {
                        Diagnostic::error(
                            "E0003",
                            format!("`#Target(Os.{os_name})` is not a known native OS"),
                            "native OS targets are `Os.Linux`, `Os.Macos`, or `Os.Windows`".to_string(),
                            format!(
                                "write `#Target(Os.{})`, `#Target(Os.{})`, or `#Target(Os.{})`",
                                Syntax::TARGET_OS_LINUX,
                                Syntax::TARGET_OS_MACOS,
                                Syntax::TARGET_OS_WINDOWS,
                            ),
                            Some(os_span),
                        )
                    });
            }
            self.expect(TokKind::RParen, "to close `#Target(…)`")?;
            if name == crate::Syntax::WEB_TARGET_DEFAULT_WEB {
                return Ok(TargetMarker::DefaultWeb);
            }
            crate::Syntax::WebBucket::parse(&name)
                .map(TargetMarker::Bucket)
                .ok_or_else(|| {
                    Diagnostic::error(
                    "E0003",
                    format!("`#Target({name})` is not a known web partition"),
                    "web targets are `Wasm` (compute), `Js` (DOM/view), `Web` (default CLI backend), or `Os.Linux`/`Os.Macos`/`Os.Windows` (native platform gating)"
                        .to_string(),
                    format!(
                        "write `#Target({})`, `#Target({})`, `#Target({})`, or `#Target(Os.{{Linux|Macos|Windows}})`",
                        Syntax::WEB_BUCKET_WASM,
                        Syntax::WEB_BUCKET_JS,
                        Syntax::WEB_TARGET_DEFAULT_WEB,
                    ),
                    Some(name_span),
                )
                })
        }
    
        /// D-WASM1=A: consume `#Wasm` / `#Js` / `#WasmExport` when present.
        fn try_parse_web_partition_marker(
            &mut self,
        ) -> Result<Option<crate::Syntax::WebPartitionMarker>, Diagnostic> {
            if !matches!(self.peek().kind, TokKind::Hash) {
                return Ok(None);
            }
            let marker = match &self.peek2().kind {
                TokKind::Ident(n) if n == Syntax::ATTR_WASM => crate::Syntax::WebPartitionMarker::Wasm,
                TokKind::Ident(n) if n == Syntax::ATTR_JS => crate::Syntax::WebPartitionMarker::Js,
                TokKind::Ident(n) if n == Syntax::ATTR_WASM_EXPORT => {
                    crate::Syntax::WebPartitionMarker::WasmExport
                }
                _ => return Ok(None),
            };
            if !matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
                && !self.token_after_web_marker_is_fn(2)
            {
                return Ok(None);
            }
            self.bump(); // `#`
            self.bump(); // marker name
            Ok(Some(marker))
        }
    
        /// D-STATE1: parse `#State(StateName)` and return `(name, marker_span)`.
        fn parse_state_require_marker(&mut self) -> Result<(String, Span), Diagnostic> {
            let start = self.bump().span.start; // `#`
            self.bump(); // `State`
            self.expect(TokKind::LParen, "after `#State`")?;
            let (name, _) = self.expect_ident("the required state inside `#State(…)`")?;
            let end = self.peek().span.end;
            self.expect(TokKind::RParen, "to close `#State(…)`")?;
            Ok((name, Span::new(start, end)))
        }
    
        /// D-STATE1: parse `#Transition(From -> To)`. `From` may be the wildcard `_`
        /// (an entry transition → `from = None`).
        fn parse_transition_marker(&mut self) -> Result<crate::AST::StateTransition, Diagnostic> {
            let start = self.bump().span.start; // `#`
            self.bump(); // `Transition`
            self.expect(TokKind::LParen, "after `#Transition`")?;
            // From-state: `_` (entry) or a state-tag ident.
            let from = if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::STATE_ENTRY) {
                self.bump();
                None
            } else {
                let (n, _) = self.expect_ident("the from-state inside `#Transition(…)`")?;
                Some(n)
            };
            self.expect(
                TokKind::Arrow,
                "between the from- and to-state in `#Transition(From -> To)`",
            )?;
            let (to, _) = self.expect_ident("the to-state inside `#Transition(…)`")?;
            let end = self.peek().span.end;
            self.expect(TokKind::RParen, "to close `#Transition(…)`")?;
            Ok(crate::AST::StateTransition {
                from,
                to,
                span: Span::new(start, end),
            })
        }
    
}
