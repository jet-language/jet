use super::super::{
    Diagnostic, ImplDef, Item, Parser, Span, StrTokPart, Syntax, TokKind, describe,
};

impl<'a> Parser<'a> {
        pub(super) fn normalize_external_method_item(item: Item) -> Item {
            match item {
                Item::Func(mut f) => {
                    if let Some((type_name, type_span)) = f.external_type.take() {
                        let span = f.span;
                        Item::Impl(ImplDef {
                            span,
                            type_name,
                            type_span,
                            trait_name: None,
                            trait_span: None,
                            methods: vec![f],
                            delegation_field: None,
                            assoc_type_impls: Vec::new(),
                            is_generated_serde: false,
                            os_target: None,
                        })
                    } else {
                        Item::Func(f)
                    }
                }
                other => other,
            }
        }
    
        /// D-VISDEFAULT2=A: is the cursor at `#PubFile`?
        pub(super) fn at_pub_file(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_PUB_FILE)
        }

        /// D-PRELUDEX1=A: is the cursor at `#NoPrelude`?
        pub(super) fn at_no_prelude(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_NO_PRELUDE)
        }
    
        /// S43 (D-CASING1 follow-on): true when the cursor is at the `#Test` marker.
        pub(in crate::Parser) fn at_test_def(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_TEST)
        }
    
        /// Parse `#Test "name" { … }` (D-CASING1 follow-on). The bare lowercase
        /// `test` path enters via `test_def_after_kw` after emitting E0052.
        pub(in crate::Parser) fn test_def(&mut self) -> Result<crate::AST::TestDef, Diagnostic> {
            self.expect(TokKind::Hash, "before `Test`")?;
            self.bump(); // the `Test` marker ident (guaranteed by at_test_def)
            self.test_def_after_kw()
        }
    
        pub(super) fn test_def_after_kw(&mut self) -> Result<crate::AST::TestDef, Diagnostic> {
            let item_start = self.toks[self.pos.saturating_sub(1)].span.start;
            // D-TEST1 (ratified 2026-06-22, option B): the property-test form is
            // `#Test fn name(params) { … }`. A parameter list means inputs are
            // generated from the param types and shrunk on failure; the bare
            // `#Test "name" { … }` block form is a plain unit test.
            if matches!(self.peek().kind, TokKind::KwFn) {
                let fn_span = self.bump().span; // the `fn` keyword
                let (name, name_span) = self.expect_ident("after `fn`")?;
                self.expect(TokKind::LParen, "after the property test name")?;
                let mut params = Vec::new();
                if !matches!(self.peek().kind, TokKind::RParen) {
                    loop {
                        params.push(self.param()?);
                        if matches!(self.peek().kind, TokKind::RParen) {
                            break;
                        }
                        self.expect(TokKind::Comma, "between parameters")?;
                    }
                }
                self.expect(TokKind::RParen, "to close the parameter list")?;
                self.validate_variadic_params(&params);
                self.expect(TokKind::LBrace, "to open the property test body")?;
                let body = self.block_stmts();
                return Ok(crate::AST::TestDef {
                    span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                    name,
                    name_span,
                    params,
                    fn_keyword_span: Some(fn_span),
                    body,
                });
            }
            let (name, name_span) = self.expect_test_name()?;
            self.expect(TokKind::LBrace, "to open the test body")?;
            let body = self.block_stmts();
            Ok(crate::AST::TestDef {
                span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                name,
                name_span,
                params: Vec::new(),
                fn_keyword_span: None,
                body,
            })
        }
    
        /// D-BENCH1/D-BENCH-MARKER1=A: true when cursor is at `#Bench`.
        pub(in crate::Parser) fn at_bench_def(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_BENCH)
        }
    
        /// Parse `#Bench("name") { … }` (D-BENCH1/D-BENCH-MARKER1=A). Structurally identical to
        /// `test_def`; there is no retired lowercase spelling for benches.
        pub(in crate::Parser) fn bench_def(&mut self) -> Result<crate::AST::BenchDef, Diagnostic> {
            let item_start = self.peek().span.start;
            self.expect(TokKind::Hash, "before `Bench`")?;
            self.bump(); // the `Bench` marker ident (guaranteed by at_bench_def)
            self.expect(TokKind::LParen, "after `#Bench`")?;
            let (name, name_span) = self.expect_marker_name(Syntax::KW_BENCH)?;
            self.expect(TokKind::RParen, "to close `#Bench(…)`")?;
            self.expect(TokKind::LBrace, "to open the benchmark body")?;
            let body = self.block_stmts();
            Ok(crate::AST::BenchDef {
                span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                name,
                name_span,
                body,
            })
        }
    
        /// S14: a bare lowercase `test` introduces a test block only when followed by
        /// a quoted name (so an ordinary identifier named `test` is unaffected).
        pub(super) fn foreign_test_follows(&self) -> bool {
            matches!(self.peek2().kind, TokKind::Str(_))
        }
    
        pub(super) fn foreign_test_diag(&self, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0052",
                format!(
                    "test blocks are written with `#{}`, not bare `{}`",
                    Syntax::KW_TEST,
                    Syntax::FOREIGN_TEST
                ),
                format!(
                    "`#{}` is a marker, like every other `#`-tag, so a test declaration draws the eye",
                    Syntax::KW_TEST
                ),
                format!("write: #{} (\"name\") {{ ... }}", Syntax::KW_TEST),
                Some(span),
            )
        }
    
        /// S60 (D-CASING1 follow-on) / D-MARKERMOVE1/2: true when the cursor is at
        /// `@Pure fn`/`@Pure pub` — or the retired `@Pure` spelling, so `func()`
        /// can consume it and teach E0062 instead of falling through elsewhere.
        pub(in crate::Parser) fn at_pure_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash | TokKind::At)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_PURE)
                && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
        }
    
        /// D-TAINT1: true when the cursor is at `#Sanitizer fn`/`#Sanitizer pub fn`.
        pub(in crate::Parser) fn at_sanitizer_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_SANITIZER)
                && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
        }
    
        /// D-REPLAY1: true when the cursor is at `#Replayable fn` /
        /// `#Replayable pub fn`.
        pub(in crate::Parser) fn at_replayable_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_REPLAYABLE)
                && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
        }

        /// D-SCHEDULE1 (card #505): true when the cursor is at `#Task` (a bare
        /// schedule fn marker). Unlike `#Sanitizer`/`#Replayable`, this does
        /// NOT require `fn`/`pub` immediately next — the ratified spelling
        /// stacks `#Task` before `#Every(…)` (`#Task #Every(5min) fn …`), so
        /// the marker after `#Task` is usually another marker, not `fn`
        /// itself. Same looseness as `#State(`/`#Transition(`/`#Every(`,
        /// which only look as far as their own shape.
        pub(in crate::Parser) fn at_task_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_TASK)
        }

        /// D-SCHEDULE1: true when the cursor is at `#Every(…)` (a schedule fn
        /// marker). Token stream: `# Every (`.
        pub(in crate::Parser) fn at_every_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_EVERY)
                && matches!(self.peek3().kind, TokKind::LParen)
        }
    
        /// D-MUSTUSE1 (c18iwxqx) / D-MARKERMOVE1: true when the cursor is at
        /// `@MustUse fn` / `@MustUse pub fn` — or the retired `@MustUse` spelling,
        /// so `func()` can consume it and teach E0062.
        pub(in crate::Parser) fn at_must_use_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash | TokKind::At)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_MUST_USE)
                && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
        }
    
        /// D-METHODMACRO1=A: true when the cursor is at `@Inline fn`/`@Inline pub
        /// fn` or `@InlineAlways fn`/`@InlineAlways pub fn` — or the retired `#`
        /// spelling, so `func()`/`method_in_type()` can consume it and teach
        /// E0062.
        pub(super) fn at_inline_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash | TokKind::At)
                && matches!(&self.peek2().kind, TokKind::Ident(n)
                    if n == Syntax::CONTRACT_INLINE || n == Syntax::CONTRACT_INLINE_ALWAYS)
                && (matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
                    // D-METHODMACRO1=A: `@Inline @InlineAlways fn …` (or the other
                    // order) — recognize the doubled marker too, so `func()` reaches
                    // `parse_inline_marker`'s E0920 conflict check instead of
                    // falling through to the generic `@` teaching error (E0990).
                    || (matches!(self.peek3().kind, TokKind::Hash | TokKind::At)
                        && matches!(&self.peek4().kind, TokKind::Ident(n)
                            if n == Syntax::CONTRACT_INLINE || n == Syntax::CONTRACT_INLINE_ALWAYS)
                        && matches!(self.peek5().kind, TokKind::KwFn | TokKind::KwPub)))
        }
    
        /// D-STATE1: true when the cursor is at `#State(…)` (a require-state fn marker).
        /// Token stream: `# State (`.
        pub(in crate::Parser) fn at_state_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_STATE)
                && matches!(self.peek3().kind, TokKind::LParen)
        }
    
        /// D-STATE1: true when the cursor is at `#Transition(…)` (a transition fn marker).
        /// Token stream: `# Transition (`.
        pub(in crate::Parser) fn at_transition_fn(&self) -> bool {
            matches!(self.peek().kind, TokKind::Hash)
                && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_TRANSITION)
                && matches!(self.peek3().kind, TokKind::LParen)
        }
    
        /// S14: bare lowercase `pure` introduces a function only when `fn`/`pub`
        /// follows (so an ordinary identifier named `pure` is unaffected).
        pub(super) fn foreign_pure_follows(&self) -> bool {
            matches!(self.peek2().kind, TokKind::KwFn | TokKind::KwPub)
        }
    
        /// D-TAINT-SAN: bare lowercase `sanitizer` is the retired spelling of the
        /// taint-strip modifier, recognized only when `fn`/`pub` follows (so an
        /// ordinary identifier named `sanitizer` elsewhere is unaffected). The
        /// modifier is now the marker `#Sanitizer` — point the user at it (E0059),
        /// mirroring `pure` → `@Pure` (E0053).
        pub(super) fn foreign_sanitizer_follows(&self) -> bool {
            matches!(self.peek2().kind, TokKind::KwFn | TokKind::KwPub)
        }
    
        pub(super) fn foreign_sanitizer_diag(&self, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0059",
                format!(
                    "the taint-strip modifier is written `#{}`, not bare `{}`",
                    Syntax::KW_SANITIZER,
                    Syntax::FOREIGN_SANITIZER
                ),
                format!(
                    "`#{}` is a marker, like every other `#`-tag, so the taint-strip contract draws the eye",
                    Syntax::KW_SANITIZER
                ),
                format!("write: #{} fn name() {{ ... }}", Syntax::KW_SANITIZER),
                Some(span),
            )
        }
    
        pub(super) fn foreign_pure_diag(&self, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0053",
                format!(
                    "the purity modifier is written `#{}`, not bare `{}`",
                    Syntax::KW_PURE,
                    Syntax::FOREIGN_PURE
                ),
                format!(
                    "`#{}` is a marker, like every other `#`-tag, so the purity contract draws the eye",
                    Syntax::KW_PURE
                ),
                format!("write: #{} fn name() {{ ... }}", Syntax::KW_PURE),
                Some(span),
            )
        }
    
        /// D-TESTPAREN1=A: `#Test` block name is a parenthesized string — `#Test("name")`.
        /// Old bare-string form `#Test "name"` emits a teaching error (E0052).
        fn expect_test_name(&mut self) -> Result<(String, Span), Diagnostic> {
            // Detect old form: bare string directly after `#Test` — teaching error.
            if matches!(&self.peek().kind, TokKind::Str(_)) {
                let span = self.peek().span;
                return Err(Diagnostic::error(
                    "E0052",
                    format!(
                        "test name must be parenthesized: `#{}(\"name\")`",
                        Syntax::KW_TEST
                    ),
                    "the name is now an argument to the marker, not a bare adjacent string".to_string(),
                    format!(
                        "write: #{} (\"describes this block\") {{ ... }}",
                        Syntax::KW_TEST
                    ),
                    Some(span),
                ));
            }
            self.expect(TokKind::LParen, &format!("after `#{}`", Syntax::KW_TEST))?;
            let (name, name_span) = self.expect_test_name_str()?;
            self.expect(
                TokKind::RParen,
                &format!("to close `#{}(…)`", Syntax::KW_TEST),
            )?;
            Ok((name, name_span))
        }
    
        /// Inner: parse the plain string literal inside `#Test("…")`.
        fn expect_test_name_str(&mut self) -> Result<(String, Span), Diagnostic> {
            let parts = match &self.peek().kind {
                TokKind::Str(parts) => parts.clone(),
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected a name in quotes inside `#{}(…)`, found {}",
                            Syntax::KW_TEST,
                            describe(other)
                        ),
                        "each test needs a name so failures are easy to find".to_string(),
                        format!(
                            "write: #{} (\"describes this test\") {{ ... }}",
                            Syntax::KW_TEST
                        ),
                        Some(self.peek().span),
                    ));
                }
            };
            let span = self.bump().span;
            if parts.len() != 1 {
                return Err(Diagnostic::error(
                    "E0003",
                    "a test name must be one piece of quoted text".to_string(),
                    "names are labels, not interpolated messages".to_string(),
                    format!("write: #{} (\"my name\") {{ ... }}", Syntax::KW_TEST),
                    Some(span),
                ));
            }
            match &parts[0] {
                StrTokPart::Lit(s) => Ok((s.clone(), span)),
                StrTokPart::Interp(_) => Err(Diagnostic::error(
                    "E0003",
                    "a test name can't contain `{ }` interpolation".to_string(),
                    "names are fixed labels".to_string(),
                    format!("write: #{} (\"my name\") {{ ... }}", Syntax::KW_TEST),
                    Some(span),
                )),
            }
        }
    
        /// A `#Test`/`#Bench` block name: one plain string literal, no
        /// interpolation. `kw` is the marker keyword for the error copy.
        pub(super) fn expect_marker_name(&mut self, kw: &str) -> Result<(String, Span), Diagnostic> {
            let parts = match &self.peek().kind {
                TokKind::Str(parts) => parts.clone(),
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected a name in quotes after `#{}`, found {}",
                            kw,
                            describe(other)
                        ),
                        "each block needs a name so results are easy to find".to_string(),
                        format!("write: #{} \"describes this block\" {{ ... }}", kw),
                        Some(self.peek().span),
                    ));
                }
            };
            let span = self.bump().span;
            if parts.len() != 1 {
                return Err(Diagnostic::error(
                    "E0003",
                    "a block name must be one piece of quoted text".to_string(),
                    "names are labels, not interpolated messages".to_string(),
                    format!("write: #{} \"my name\" {{ ... }}", kw),
                    Some(span),
                ));
            }
            match &parts[0] {
                StrTokPart::Lit(s) => Ok((s.clone(), span)),
                StrTokPart::Interp(_) => Err(Diagnostic::error(
                    "E0003",
                    "a block name can't contain `{ }` interpolation".to_string(),
                    "names are fixed labels".to_string(),
                    format!("write: #{} \"my name\" {{ ... }}", kw),
                    Some(span),
                )),
            }
        }
    
        /// S50 (M7): `extern rust "crate@version" { fn … = "rust::path"; }`
        pub(super) fn extern_rust_block(&mut self) -> Result<crate::AST::ExternRustBlock, Diagnostic> {
            let start = self.bump().span;
            if !matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_RUST) {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected `{}` after `{}`, found {}",
                        Syntax::KW_RUST,
                        Syntax::KW_EXTERN,
                        describe(&self.peek().kind)
                    ),
                    format!(
                        "foreign Rust functions are declared in `{} {} \"crate@version\" {{ … }}`",
                        Syntax::KW_EXTERN,
                        Syntax::KW_RUST
                    ),
                    format!(
                        "write: {} {} \"std\" {{ fn name() -> Int = \"std::path\"; }}",
                        Syntax::KW_EXTERN,
                        Syntax::KW_RUST
                    ),
                    Some(self.peek().span),
                ));
            }
            self.bump();
            let (crate_spec, crate_span) = self.expect_plain_string(
                "after `extern rust`",
                "the crate name must be one piece of quoted text",
                "write: extern rust \"base64@0.22\" { ... }",
            )?;
            self.expect(TokKind::LBrace, "to open the extern block")?;
            let mut functions = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    continue;
                }
                functions.push(self.extern_fn()?);
            }
            self.expect(TokKind::RBrace, "to close the extern block")?;
            let end = self.toks[self.pos - 1].span.end;
            Ok(crate::AST::ExternRustBlock {
                crate_spec,
                crate_span,
                functions,
                span: Span::new(start.start, end),
            })
        }
    
        pub(super) fn extern_fn(&mut self) -> Result<crate::AST::ExternFn, Diagnostic> {
            let fn_span = self.peek().span;
            self.expect_kw(TokKind::KwFn, "to declare a foreign function")?;
            let fn_start = fn_span.start;
            let (name, name_span) = self.expect_ident("after `fn`")?;
            self.expect(TokKind::LParen, "after the function name")?;
            let mut params = Vec::new();
            if !matches!(self.peek().kind, TokKind::RParen) {
                loop {
                    params.push(self.param()?);
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    self.expect(TokKind::Comma, "between parameters")?;
                }
            }
            self.expect(TokKind::RParen, "to close the parameter list")?;
            self.validate_variadic_params(&params);
    
            let mut return_type = None;
            let mut return_type_span = None;
            if matches!(self.peek().kind, TokKind::Arrow) {
                self.bump();
                let (ty, span) = self.return_type()?;
                return_type = Some(ty);
                return_type_span = Some(span);
            }
    
            self.expect(TokKind::Eq, "before the Rust path")?;
            let (rust_path, rust_path_span) = self.expect_plain_string(
                "after `=`",
                "the Rust path must be one piece of quoted text",
                "write: = \"crate::function\"",
            )?;
            self.expect(TokKind::Semi, "after the foreign path")?;
            let end = self.toks[self.pos - 1].span.end;
            Ok(crate::AST::ExternFn {
                name,
                name_span,
                params,
                return_type,
                return_type_span,
                rust_path,
                rust_path_span,
                span: Span::new(fn_start, end),
            })
        }
    
}
