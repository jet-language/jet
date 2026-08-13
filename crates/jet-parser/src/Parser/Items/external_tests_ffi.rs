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
            let marker = self.parse_rule_marker()?;
            self.bind_rule_fact(
                marker.name_span,
                None,
                crate::Policy::RuleSite::Test,
            );
            if matches!(self.peek().kind, TokKind::KwFn) {
                if !marker.args.is_empty() {
                    return Err(crate::Policy::marker_argument_shape_error(
                        Syntax::KW_TEST,
                        marker.span,
                    ));
                }
                return self.test_def_after_kw();
            }
            let arguments = self.bound_registered_rule_arguments(&marker)?;
            let Some(name_argument) = arguments.parameter(0) else {
                return Err(crate::Policy::marker_argument_shape_error(
                    Syntax::KW_TEST,
                    marker.span,
                ));
            };
            let (name, name_span) = match name_argument {
                crate::AST::Expr::Str(parts, span) if parts.len() == 1 => match &parts[0] {
                    crate::AST::StrPart::Lit(name) => (Some(name.clone()), *span),
                    crate::AST::StrPart::Interp(..) => (None, *span),
                },
                other => (None, other.span()),
            };
            let item_start = marker.span.start;
            self.expect(TokKind::LBrace, "to open the test body")?;
            let body = self.block_stmts();
            return Ok(crate::AST::TestDef {
                span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                name,
                name_expr: Some(name_argument.clone()),
                name_prefix: None,
                name_span,
                params: Vec::new(),
                fn_keyword_span: None,
                body,
            });
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
                let params = self.parse_param_list()?;
                self.validate_variadic_params(&params);
                self.validate_param_labels(&params);
                self.expect(TokKind::LBrace, "to open the property test body")?;
                let body = self.block_stmts();
                return Ok(crate::AST::TestDef {
                    span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                    name: Some(name),
                    name_expr: None,
                    name_prefix: None,
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
                name: Some(name),
                name_expr: None,
                name_prefix: None,
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
            let marker = self.parse_rule_marker()?;
            self.bind_rule_fact(
                marker.name_span,
                None,
                crate::Policy::RuleSite::Bench,
            );
            let arguments = self.bound_registered_rule_arguments(&marker)?;
            let Some(name_argument) = arguments.parameter(0) else {
                return Err(crate::Policy::marker_argument_shape_error(
                    Syntax::KW_BENCH,
                    marker.span,
                ));
            };
            let (name, name_span) = match name_argument {
                crate::AST::Expr::Str(parts, span) if parts.len() == 1 => match &parts[0] {
                    crate::AST::StrPart::Lit(name) => (Some(name.clone()), *span),
                    crate::AST::StrPart::Interp(..) => (None, *span),
                },
                other => (None, other.span()),
            };
            self.expect(TokKind::LBrace, "to open the benchmark body")?;
            let body = self.block_stmts();
            Ok(crate::AST::BenchDef {
                span: Span::new(item_start, self.toks[self.pos.saturating_sub(1)].span.end),
                name,
                name_expr: name_argument.clone(),
                name_prefix: None,
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

        /// D-TASK-META1=A: decode optional static fields on the existing task
        /// marker. Task execution remains an ordinary function call.
        pub(in crate::Parser) fn task_metadata_from_marker(
            &self,
            marker: &crate::AST::Marker,
        ) -> Result<Option<crate::AST::TaskMetadata>, Diagnostic> {
            if marker.args.is_empty() {
                return Ok(None);
            }
            let arguments = self.bound_registered_rule_arguments(marker)?;
            let mut metadata = crate::AST::TaskMetadata::default();
            metadata.packages = Self::task_string_list(arguments.parameter(0), marker.span)?;
            metadata.cwd = Self::task_optional_string(arguments.parameter(1), marker.span, "cwd")?;
            metadata.inputs = Self::task_string_list(arguments.parameter(2), marker.span)?;
            metadata.outputs = Self::task_string_list(arguments.parameter(3), marker.span)?;
            metadata.skip = Self::task_skip(arguments.parameter(4), marker.span)?;
            if let Some(cache) = arguments.parameter(5) {
                metadata.cache = match Self::task_word(cache) {
                    Some("Local") => crate::AST::TaskCachePolicy::Local,
                    Some("Shared") => crate::AST::TaskCachePolicy::Shared,
                    Some("Uncached") | Some("Off") => crate::AST::TaskCachePolicy::Uncached,
                    _ => {
                        return Err(Self::task_metadata_error(
                            "cache",
                            ".Uncached, .Local, or .Shared",
                            cache.span(),
                        ))
                    }
                };
            }
            if let Some(limits) = arguments.parameter(6) {
                metadata.limits = Self::task_limits(limits, marker.span)?;
            }
            Ok(Some(metadata))
        }

        fn task_string_list(
            expr: Option<&crate::AST::Expr>,
            marker_span: Span,
        ) -> Result<Vec<String>, Diagnostic> {
            let Some(expr) = expr else { return Ok(Vec::new()) };
            match expr {
                crate::AST::Expr::ListLit(values, _) => values
                    .iter()
                    .map(|value| {
                        Self::task_word(value).map(str::to_string).ok_or_else(|| {
                            Self::task_metadata_error(
                                "list",
                                "a list of strings or names",
                                value.span(),
                            )
                        })
                    })
                    .collect(),
                _ => Self::task_word(expr)
                    .map(|value| vec![value.to_string()])
                    .ok_or_else(|| {
                        Self::task_metadata_error(
                            "list",
                            "a list of strings or names",
                            marker_span,
                        )
                    }),
            }
        }

        fn task_optional_string(
            expr: Option<&crate::AST::Expr>,
            marker_span: Span,
            field: &str,
        ) -> Result<Option<String>, Diagnostic> {
            let Some(expr) = expr else { return Ok(None) };
            match Self::task_word(expr) {
                Some(value) if value != "none" && value != "None" => Ok(Some(value.to_string())),
                Some(_) => Ok(None),
                None => Err(Self::task_metadata_error(field, "a string or name", marker_span)),
            }
        }

        fn task_skip(
            expr: Option<&crate::AST::Expr>,
            marker_span: Span,
        ) -> Result<Option<crate::AST::TaskSkip>, Diagnostic> {
            let Some(expr) = expr else { return Ok(None) };
            if let Some(value) = Self::task_word(expr) {
                return Ok((value != "none" && value != "None")
                    .then(|| crate::AST::TaskSkip::Always(value.to_string())));
            }
            let crate::AST::Expr::EnumLit { variant, args, .. } = expr else {
                return Err(Self::task_metadata_error(
                    "skip",
                    "a reason string or .Unless(.Platform(.Linux))",
                    marker_span,
                ));
            };
            let [crate::AST::EnumLitArg::Positional(platform)] = args.as_slice() else {
                return Err(Self::task_metadata_error(
                    "skip",
                    ".Unless(.Platform(.Linux))",
                    marker_span,
                ));
            };
            if variant != "Unless" {
                return Err(Self::task_metadata_error(
                    "skip",
                    ".Unless(.Platform(.Linux))",
                    marker_span,
                ));
            }
            let crate::AST::Expr::EnumLit {
                variant: platform_constructor,
                args: platform_args,
                ..
            } = platform
            else {
                return Err(Self::task_metadata_error(
                    "skip",
                    ".Unless(.Platform(.Linux))",
                    marker_span,
                ));
            };
            let [crate::AST::EnumLitArg::Positional(platform)] = platform_args.as_slice() else {
                return Err(Self::task_metadata_error(
                    "skip",
                    ".Unless(.Platform(.Linux))",
                    marker_span,
                ));
            };
            let Some(platform) = Self::task_word(platform) else {
                return Err(Self::task_metadata_error(
                    "skip",
                    ".Unless(.Platform(.Linux))",
                    marker_span,
                ));
            };
            if platform_constructor != "Platform"
                || !matches!(platform, "Linux" | "MacOS" | "Windows" | "FreeBSD")
            {
                return Err(Self::task_metadata_error(
                    "skip",
                    ".Unless(.Platform(.Linux))",
                    marker_span,
                ));
            }
            Ok(Some(crate::AST::TaskSkip::UnlessPlatform {
                platform: platform.to_string(),
            }))
        }

        fn task_limits(
            expr: &crate::AST::Expr,
            marker_span: Span,
        ) -> Result<std::collections::BTreeMap<String, String>, Diagnostic> {
            let crate::AST::Expr::MapLit(entries, _) = expr else {
                return Err(Self::task_metadata_error(
                    "limits",
                    "a map of names to static values",
                    marker_span,
                ));
            };
            let mut limits = std::collections::BTreeMap::new();
            for (key, value) in entries {
                let Some(key) = Self::task_word(key) else {
                    return Err(Self::task_metadata_error(
                        "limits",
                        "a map of names to static values",
                        key.span(),
                    ));
                };
                let Some(value) = Self::task_static_word(value) else {
                    return Err(Self::task_metadata_error(
                        "limits",
                        "a map of names to static values",
                        value.span(),
                    ));
                };
                if limits.insert(key.to_string(), value).is_some() {
                    return Err(Self::task_metadata_error(
                        "limits",
                        "a map with unique keys",
                        marker_span,
                    ));
                }
            }
            Ok(limits)
        }

        fn task_word(expr: &crate::AST::Expr) -> Option<&str> {
            match expr {
                crate::AST::Expr::Ident(value, _) => Some(value.as_str()),
                crate::AST::Expr::EnumLit { variant, args, .. } if args.is_empty() => {
                    Some(variant.as_str())
                }
                crate::AST::Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
                    crate::AST::StrPart::Lit(value) => Some(value.as_str()),
                    crate::AST::StrPart::Interp(..) => None,
                },
                _ => None,
            }
        }

        fn task_static_word(expr: &crate::AST::Expr) -> Option<String> {
            Self::task_word(expr).map(str::to_string).or_else(|| match expr {
                crate::AST::Expr::Int(value, ..) => Some(value.to_string()),
                crate::AST::Expr::Bool(value, _) => Some(value.to_string()),
                _ => None,
            })
        }

        fn task_metadata_error(field: &str, expected: &str, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E1330",
                format!("task metadata {field} has the wrong shape"),
                format!("D-TASK-META1: {field} is {expected}"),
                format!("write {field}: as {expected}"),
                Some(span),
            )
        }

    
    
    
    
    

        /// D-TAINT-SAN: bare lowercase `sanitizer` is the retired spelling of the
        /// taint-strip modifier, recognized only when `fn`/`pub` follows (so an
        /// ordinary identifier named `sanitizer` elsewhere is unaffected). The
        /// modifier is now the typed marker `#Scrub(Tag)` (E0059).
        pub(super) fn foreign_sanitizer_follows(&self) -> bool {
            matches!(self.peek2().kind, TokKind::KwFn | TokKind::KwPub)
        }
    
        pub(super) fn foreign_sanitizer_diag(&self, span: Span) -> Diagnostic {
            Diagnostic::error(
                "E0059",
                format!(
                    "the taint-strip modifier is written `#Scrub(Tag)`, not bare `{}`",
                    Syntax::FOREIGN_SANITIZER
                ),
                "`#Scrub(Tag)` names the exact fact removed by the function".to_string(),
                format!("write: #{}(Tag) fn name() {{ ... }}", Syntax::KW_SCRUB),
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
                        "write: {} {} \"std\" {{ fn name() => Int = \"std::path\"; }}",
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
            let (abi, undo) = if matches!(self.peek().kind, TokKind::Hash) {
                let markers = self.parse_attached_marker_sequence(
                    crate::Policy::RuleSite::Function,
                    "C declaration",
                )?;
                let mut abi = None;
                let mut undo = None;
                for marker in &markers {
                    match marker.name.as_str() {
                        Syntax::MARKER_ABI => {
                            if abi.is_some() {
                                return Err(Diagnostic::error(
                                    "E3212",
                                    "a foreign declaration may have only one `#ABI` marker".to_string(),
                                    "the calling convention is one property of the binding".to_string(),
                                    "remove the repeated `#ABI(...)` marker".to_string(),
                                    Some(marker.span),
                                ));
                            }
                            let arguments = self.bound_registered_rule_arguments(marker)?;
                            let Some(crate::AST::Expr::Ident(name, span)) = arguments.parameter(0) else {
                                return Err(crate::Policy::marker_argument_shape_error(Syntax::MARKER_ABI, marker.span));
                            };
                            abi = Some((name.clone(), *span));
                        }
                        Syntax::MARKER_UNDO => {
                            if undo.is_some() {
                                return Err(Diagnostic::error(
                                    "E3212",
                                    "a foreign declaration may have only one `#Undo` marker".to_string(),
                                    "one binding has one compensating function".to_string(),
                                    "remove the repeated `#Undo(...)` marker".to_string(),
                                    Some(marker.span),
                                ));
                            }
                            let arguments = self.bound_registered_rule_arguments(marker)?;
                            let Some(crate::AST::Expr::Ident(name, span)) = arguments.parameter(0) else {
                                return Err(crate::Policy::marker_argument_shape_error(Syntax::MARKER_UNDO, marker.span));
                            };
                            undo = Some((name.clone(), *span));
                        }
                        _ => {
                            return Err(Diagnostic::error(
                                "E3212",
                                format!("unknown foreign declaration marker `#{}`", marker.name),
                                "foreign declarations accept `#ABI(name)` and `#Undo(inverse)` markers"
                                    .to_string(),
                                "remove the marker or use `#Undo(inverse)` for a compensating call"
                                    .to_string(),
                                Some(marker.name_span),
                            ));
                        }
                    }
                }
                while matches!(self.peek().kind, TokKind::Semi) { self.bump(); }
                (abi, undo)
            } else { (None, None) };
            let fn_span = self.peek().span;
            self.expect_kw(TokKind::KwFn, "to declare a foreign function")?;
            let fn_start = fn_span.start;
            let (name, name_span) = self.expect_ident("after `fn`")?;
            self.expect(TokKind::LParen, "after the function name")?;
            let params = self.parse_param_list()?;
            self.validate_variadic_params(&params);
            self.validate_param_labels(&params);
    
            let mut return_type = None;
            let mut return_type_span = None;
            let mut arrow_return = false;
            if matches!(self.peek().kind, TokKind::LambdaArrow | TokKind::Arrow) {
                arrow_return = true;
                let arrow = self.bump();
                if matches!(arrow.kind, TokKind::Arrow) {
                    self.diags.push(Self::retired_callable_arrow(arrow.span));
                }
                let (ty, span) = self.return_type()?;
                return_type = Some(ty);
                return_type_span = Some(span);
            } else if let Some((ty, span)) = self.parse_unit_fallible_return()? {
                return_type = Some(ty);
                return_type_span = Some(span);
            }
            if arrow_return
                && return_type
                    .as_ref()
                    .is_some_and(|ty| Self::is_unit_fallible_type(ty))
            {
                self.diags.push(Self::retired_unit_fallible_signature(
                    return_type_span.unwrap_or(self.peek().span),
                ));
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
                abi,
                name,
                name_span,
                params,
                return_type,
                return_type_span,
                rust_path,
                rust_path_span,
                effect_root: None,
                undo,
                span: Span::new(fn_start, end),
            })
        }
    
}
