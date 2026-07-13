use super::super::{
    Diagnostic, Item, Parser, Program, Span, Syntax, TokKind, describe, string_literal_value,
    retired_s14_teaching_enabled,
};
use super::TargetMarker;
use super::helpers::format_version_segment;

impl<'a> Parser<'a> {
        /// D-POLICY-WORD1=A: `#Policy(no_alloc)` — file allocation floor.
        /// allocation floor. `no_alloc` is the only ratified policy name today;
        /// any other name is an ordinary "expected X, found Y" parse error (the
        /// full policy list is a follow-on ballot, not this stage's business).
        fn policy_decl(&mut self) -> Result<Span, Diagnostic> {
            let start = self.bump().span; // consume `#`
            self.bump(); // `Policy`
            self.expect(TokKind::LParen, "after `#Policy`")?;
            match &self.peek().kind {
                TokKind::Ident(n) if n == Syntax::POLICY_NO_ALLOC => {
                    let name_span = self.bump().span;
                    self.expect(TokKind::RParen, "after the policy name")?;
                    Ok(Span::new(start.start, name_span.end))
                }
                other => Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected `{}` inside `#{}`, found {}",
                        Syntax::POLICY_NO_ALLOC,
                        Syntax::ATTR_POLICY,
                        describe(other)
                    ),
                    format!(
                        "`{}` is the only ratified policy name today",
                        Syntax::POLICY_NO_ALLOC
                    ),
                    format!("write `#{}({})`", Syntax::ATTR_POLICY, Syntax::POLICY_NO_ALLOC),
                    Some(self.peek().span),
                )),
            }
        }
    
        /// S16 (M6): `import "path" [as alias];` or `import name [as alias];`
        fn import_decl(&mut self) -> Result<crate::AST::ImportDecl, Diagnostic> {
            let start = self.bump().span; // consume `use`
            match &self.peek().kind {
                TokKind::Str(parts) => {
                    let path = string_literal_value(parts)?;
                    let path_span = self.bump().span;
                    let alias_default = path.rsplit('/').next().unwrap_or("module").to_string();
                    let (alias, alias_span) = if matches!(
                        &self.peek().kind,
                        TokKind::Ident(n) if n == Syntax::KW_AS
                    ) {
                        self.bump();
                        let (name, span) = self.expect_ident("after `as`")?;
                        (name, span)
                    } else {
                        (alias_default, start)
                    };
                    self.expect(TokKind::Semi, "after an import")?;
                    let end = self.toks[self.pos - 1].span.end;
                    Ok(crate::AST::ImportDecl {
                        kind: crate::AST::ImportKind::File(path, path_span),
                        alias,
                        alias_span,
                        span: Span::new(start.start, end),
                        is_pub: false,
                        is_package_pub: false,
                        inline_version: None,
                    })
                }
                TokKind::Ident(_) => {
                    // Peek ahead to decide which import form this is:
                    //   use ident.{A, B}    → Unqualified group
                    //   use ident.ident ;   → Unqualified single (no `as`)
                    //   use ident.ident.*   → error: wildcard
                    //   use ident ...       → Module (may have dots + `as alias`)
                    let (first, first_span) = self.expect_ident("after `use`")?;
                    if matches!(self.peek().kind, TokKind::Dot) {
                        // Look two tokens ahead (past the dot).
                        let after_dot = &self.peek2().kind;
                        match after_dot {
                            TokKind::LBrace => {
                                // use alias.{A, B as C, ...}
                                self.bump(); // consume `.`
                                let lbrace_span = self.bump().span; // consume `{`
                                let mut items: Vec<(String, Option<String>)> = Vec::new();
                                loop {
                                    if matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                                        break;
                                    }
                                    let (item, _) = self.expect_ident("inside `use alias.{…}`")?;
                                    // D-SELIMPORT1=A: optional `as alias` after each item.
                                    let alias = if matches!(
                                        &self.peek().kind,
                                        TokKind::Ident(n) if n == Syntax::KW_AS
                                    ) {
                                        self.bump(); // consume `as`
                                        let (a, _) = self.expect_ident("after `as` in import list")?;
                                        Some(a)
                                    } else {
                                        None
                                    };
                                    items.push((item, alias));
                                    if matches!(self.peek().kind, TokKind::Comma) {
                                        self.bump();
                                    } else {
                                        break;
                                    }
                                }
                                let rbrace_span = self.peek().span;
                                self.expect(TokKind::RBrace, "to close `use alias.{…}`")?;
                                let items_span = Span::new(lbrace_span.start, rbrace_span.end);
                                self.expect(TokKind::Semi, "after an import")?;
                                let end = self.toks[self.pos - 1].span.end;
                                Ok(crate::AST::ImportDecl {
                                    kind: crate::AST::ImportKind::Unqualified {
                                        module_alias: first.clone(),
                                        module_alias_span: first_span,
                                        items,
                                        items_span,
                                        span: Span::new(start.start, end),
                                    },
                                    alias: first,
                                    alias_span: first_span,
                                    span: Span::new(start.start, end),
                                    is_pub: false,
                                    is_package_pub: false,
                                    inline_version: None,
                                })
                            }
                            TokKind::Star => {
                                // use alias.* — wildcard: reject
                                self.bump(); // consume `.`
                                let star_span = self.bump().span; // consume `*`
                                return Err(Diagnostic::error(
                                    "E0612",
                                    "wildcard imports are not supported".to_string(),
                                    "`use math.*` would hide where each name comes from".to_string(),
                                    "list each item instead: `use math.{clamp, lerp}`".to_string(),
                                    Some(star_span),
                                ));
                            }
                            TokKind::Ident(_) => {
                                // Check if token after the item ident is `;` (no `as`, no more dots).
                                // peek3() is 2 positions past current (dot + ident).
                                let after_item = &self.peek3().kind;
                                if matches!(after_item, TokKind::Semi | TokKind::Eof) {
                                    // use alias.item ; — Unqualified single (no alias for single form)
                                    self.bump(); // consume `.`
                                    let (item, item_span) =
                                        self.expect_ident("after `.` in a `use` import")?;
                                    let items_span = item_span;
                                    self.expect(TokKind::Semi, "after an import")?;
                                    let end = self.toks[self.pos - 1].span.end;
                                    Ok(crate::AST::ImportDecl {
                                        kind: crate::AST::ImportKind::Unqualified {
                                            module_alias: first,
                                            module_alias_span: first_span,
                                            items: vec![(item.clone(), None)],
                                            items_span,
                                            span: Span::new(start.start, end),
                                        },
                                        alias: item.clone(),
                                        alias_span: item_span,
                                        span: Span::new(start.start, end),
                                        is_pub: false,
                                        is_package_pub: false,
                                        inline_version: None,
                                    })
                                } else {
                                    // use core.files as fs — Module path with dots
                                    self.import_decl_module_path(start, first, first_span)
                                }
                            }
                            _ => {
                                // use alias. <something unexpected> — fall through to module path
                                self.import_decl_module_path(start, first, first_span)
                            }
                        }
                    } else {
                        // No dot: use module_name (optionally `as alias`)
                        if matches!(self.peek().kind, TokKind::LBrace) {
                            return Err(Diagnostic::error(
                                "E0003",
                                "selective imports aren't part of Jet".to_string(),
                                "modules keep their namespace so call sites show where a library function comes from"
                                    .to_string(),
                                "import the module with `as`, then call items through the alias: `use core.math as math; math.clamp(x, lo, hi);`"
                                    .to_string(),
                                Some(self.peek().span),
                            ));
                        }
                        // U11 (D-JPK-SCRIPTDEP1=A): `use pkg#version;` — an inline
                        // script dependency. Only the bare (no-dot) module-name
                        // form takes a version; `use core.files#1.0;` is nonsensical
                        // and isn't accepted here (the stray `#` falls through to
                        // a normal "expected `;`" parse error).
                        let inline_version = if matches!(self.peek().kind, TokKind::Hash) {
                            Some(self.inline_version()?)
                        } else {
                            None
                        };
                        let (alias, alias_span) = if matches!(
                            &self.peek().kind,
                            TokKind::Ident(n) if n == Syntax::KW_AS
                        ) {
                            self.bump();
                            let (name, span) = self.expect_ident("after `as`")?;
                            (name, span)
                        } else {
                            (first.clone(), first_span)
                        };
                        self.expect(TokKind::Semi, "after an import")?;
                        let end = self.toks[self.pos - 1].span.end;
                        Ok(crate::AST::ImportDecl {
                            kind: crate::AST::ImportKind::Module(first, first_span),
                            alias,
                            alias_span,
                            span: Span::new(start.start, end),
                            is_pub: false,
                            is_package_pub: false,
                            inline_version,
                        })
                    }
                }
                other => {
                    let other = other.clone();
                    Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected a file path in quotes or a module name after `{}`, found {}",
                            Syntax::KW_USE,
                            describe(&other)
                        ),
                        format!(
                            "write `{} \"path/to/file\";` or `{} module_name;`",
                            Syntax::KW_USE,
                            Syntax::KW_USE
                        ),
                        format!(
                            "e.g. `{} \"util/helpers\";` or `{} scoring;`",
                            Syntax::KW_USE,
                            Syntax::KW_USE
                        ),
                        Some(self.peek().span),
                    ))
                }
            }
        }
    
        /// Helper: finish parsing a module import whose first ident is already consumed.
        /// Handles `use first.sub.module [as alias];`.
        fn import_decl_module_path(
            &mut self,
            start: Span,
            first: String,
            first_span: Span,
        ) -> Result<crate::AST::ImportDecl, Diagnostic> {
            // Continue eating dots to build the full dotted name.
            let mut name = first;
            let mut end = first_span.end;
            while matches!(self.peek().kind, TokKind::Dot) {
                self.bump();
                let (part, span) = self.expect_ident("after `.` in an import")?;
                name.push('.');
                name.push_str(&part);
                end = span.end;
            }
            let module_span = Span::new(first_span.start, end);
            if matches!(self.peek().kind, TokKind::LBrace) {
                return Err(Diagnostic::error(
                    "E0003",
                    "selective imports aren't part of Jet".to_string(),
                    "modules keep their namespace so call sites show where a library function comes from"
                        .to_string(),
                    "import the module with `as`, then call items through the alias: `use core.math as math; math.clamp(x, lo, hi);`"
                        .to_string(),
                    Some(self.peek().span),
                ));
            }
            let alias_default = name.rsplit('.').next().unwrap_or(name.as_str()).to_string();
            let (alias, alias_span) = if matches!(
                &self.peek().kind,
                TokKind::Ident(n) if n == Syntax::KW_AS
            ) {
                self.bump();
                let (n, s) = self.expect_ident("after `as`")?;
                (n, s)
            } else {
                (alias_default, start)
            };
            self.expect(TokKind::Semi, "after an import")?;
            let end = self.toks[self.pos - 1].span.end;
            Ok(crate::AST::ImportDecl {
                kind: crate::AST::ImportKind::Module(name, module_span),
                alias,
                alias_span,
                span: Span::new(start.start, end),
                is_pub: false,
                is_package_pub: false,
                // A dotted module path (`use core.files;`) never takes U11's `#version`
                // — that's the single-segment `pkg` form only.
                inline_version: None,
            })
        }
    
        /// U11 (D-JPK-SCRIPTDEP1=A): parse `#<version>` on `use pkg#version;` — a
        /// dotted numeric selector (`1`, `1.4`, `1.4.2`, …). `#` is already
        /// `TokKind::Hash` (the same token `#Marker`/`[T#N]` use); the selector
        /// itself isn't its own lexer token, so it's rebuilt segment-by-segment
        /// from the `Int`/`Float` tokens the number lexer already produced
        /// (`1.4.2` lexes as `Float(1.4)`, `Dot`, `Int(2)`). One known edge case:
        /// a two-segment run with a trailing zero (`1.10`) collapses through
        /// `f64` to the same bits as `1.1` — indistinguishable once lexed. Real
        /// versions rarely hinge on that, so it's an accepted limitation rather
        /// than new lexer machinery.
        fn inline_version(&mut self) -> Result<crate::AST::InlineVersion, Diagnostic> {
            let hash_span = self.bump().span; // consume `#`
            let mut text = String::new();
            let mut end;
            match self.peek().kind.clone() {
                TokKind::Int(n, _) => {
                    text.push_str(&n.to_string());
                    end = self.bump().span.end;
                }
                TokKind::Float(f) => {
                    text.push_str(&format_version_segment(f));
                    end = self.bump().span.end;
                }
                _ => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected a version number after `#`".to_string(),
                        "`use pkg#version;` (U11) pins an inline script dependency to a version."
                            .to_string(),
                        "write digits after `#`, e.g. `use textkit#1.4;` or `use textkit#1.4.2;`"
                            .to_string(),
                        Some(self.peek().span),
                    ));
                }
            }
            while matches!(self.peek().kind, TokKind::Dot)
                && matches!(self.peek2().kind, TokKind::Int(_, _))
            {
                self.bump(); // `.`
                let TokKind::Int(n, _) = self.peek().kind else {
                    unreachable!("guarded by the match above")
                };
                text.push('.');
                text.push_str(&n.to_string());
                end = self.bump().span.end;
            }
            Ok(crate::AST::InlineVersion {
                text,
                span: Span::new(hash_span.start, end),
            })
        }
    
        pub(in crate::Parser) fn program(&mut self) -> Program {
            let mut imports = Vec::new();
            let mut items = Vec::new();
            let mut web_target_ceiling = None;
            let mut default_target: Option<String> = None;
            let mut html_path: Option<String> = None;
            let mut pub_file = false;
            let mut no_prelude = false;
            let mut no_alloc_policy: Option<Span> = None;
            loop {
                let r = match &self.peek().kind {
                    TokKind::Eof => break,
                    // S6-R: the lexer inserts a synthetic terminator after the `}`
                    // that closes an item (a `}` ends a statement). At the top level
                    // it is trivia between items — skip it.
                    TokKind::Semi => {
                        self.bump();
                        continue;
                    }
                    TokKind::KwUse => match self.import_decl() {
                        Ok(imp) => {
                            imports.push(imp);
                            continue;
                        }
                        Err(d) => {
                            self.diags.push(d);
                            self.sync_stmt();
                            continue;
                        }
                    },
                    // D-MEM1/S7 (D-NOALLOC-SEM1=A): `policy no_alloc;` — file-scoped
                    // allocation floor, parsed like `use`/`#PubFile` (not inside any
                    // `module { … }` body — only the top-level file item list).
                    TokKind::Hash
                        if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_POLICY) => match self.policy_decl() {
                        Ok(span) => {
                            if let Some(first) = no_alloc_policy {
                                self.diags.push(Diagnostic::error(
                                    "E0003",
                                    "only one `#Policy(no_alloc)` declaration is allowed per file"
                                        .to_string(),
                                    "a file may declare its allocation floor once".to_string(),
                                    "remove the duplicate `#Policy(no_alloc)` line".to_string(),
                                    Some(span),
                                ));
                                let _ = first;
                            } else {
                                no_alloc_policy = Some(span);
                            }
                            continue;
                        }
                        Err(d) => {
                            self.diags.push(d);
                            self.sync_top();
                            continue;
                        }
                    },
                    TokKind::Ident(n) if n == Syntax::FOREIGN_UNSAFE => {
                        let t = self.bump();
                        let ffi_attempt = matches!(&self.peek().kind, TokKind::KwExtern);
                        self.diags.push(Diagnostic::error(
                            "E0031",
                            format!(
                                "{} doesn't use `{}` to call Rust crates",
                                Syntax::LANG_NAME,
                                Syntax::FOREIGN_UNSAFE
                            ),
                            "foreign Rust functions live in whole `extern rust` blocks — callers never write `unsafe`"
                                .to_string(),
                            format!(
                                "write: {} {} \"crate@version\" {{ fn name(...) -> T = \"rust::path\"; }}",
                                Syntax::KW_EXTERN,
                                Syntax::KW_RUST
                            ),
                            Some(t.span),
                        ));
                        if ffi_attempt {
                            self.extern_rust_block().map(Item::ExternRust)
                        } else {
                            self.sync_top();
                            continue;
                        }
                    }
                    TokKind::Hash if self.at_pub_file() => {
                        if pub_file {
                            let span = self.peek().span;
                            self.diags.push(Diagnostic::error(
                                "E0416",
                                "only one `#PubFile` marker is allowed per file".to_string(),
                                "a file may declare at most one public-by-default visibility marker"
                                    .to_string(),
                                "remove the duplicate `#PubFile` marker".to_string(),
                                Some(span),
                            ));
                            self.bump();
                            self.bump();
                            self.sync_top();
                            continue;
                        }
                        self.bump(); // `#`
                        self.bump(); // `PubFile`
                        pub_file = true;
                        self.pub_file_default = true;
                        continue;
                    }
                    TokKind::Hash if self.at_no_prelude() => {
                        if no_prelude {
                            let span = self.peek().span;
                            self.diags.push(Diagnostic::error(
                                "E0428",
                                "only one `#NoPrelude` marker is allowed per file".to_string(),
                                "a file may opt out of the ambient prelude at most once"
                                    .to_string(),
                                "remove the duplicate `#NoPrelude` marker".to_string(),
                                Some(span),
                            ));
                            self.bump();
                            self.bump();
                            self.sync_top();
                            continue;
                        }
                        self.bump(); // `#`
                        self.bump(); // `NoPrelude`
                        no_prelude = true;
                        continue;
                    }
                    TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_PUBLIC_FILE) =>
                    {
                        let span = self.peek().span;
                        self.diags.push(Diagnostic::error(
                            "E0418",
                            format!(
                                "write `#{}`, not `#{}`",
                                Syntax::MARKER_PUB_FILE,
                                Syntax::MARKER_PUBLIC_FILE
                            ),
                            format!(
                                "`#{}` flips this file to public-by-default (D-VISDEFAULT2)",
                                Syntax::MARKER_PUB_FILE
                            ),
                            format!(
                                "write `#{}` at the top of the file",
                                Syntax::MARKER_PUB_FILE
                            ),
                            Some(span),
                        ));
                        if pub_file {
                            self.diags.push(Diagnostic::error(
                                "E0416",
                                "only one `#PubFile` marker is allowed per file".to_string(),
                                "a file may declare at most one public-by-default visibility marker"
                                    .to_string(),
                                "remove the duplicate marker".to_string(),
                                Some(span),
                            ));
                        } else {
                            pub_file = true;
                            self.pub_file_default = true;
                        }
                        self.bump();
                        self.bump();
                        continue;
                    }
                    // D-MARK-TARGET1=A: `#Target(Wasm)`/`#Target(Js)` immediately
                    // attached to a following `fn`/`pub fn` is the per-function
                    // bucket override (routed to `at_web_partition_fn` below,
                    // parsed inside `func()`), not the file/module ceiling.
                    TokKind::Hash if self.at_web_target() && !self.at_web_partition_fn() => match self.parse_web_target_marker() {
                        Ok(TargetMarker::DefaultWeb) => {
                            if matches!(self.peek().kind, TokKind::KwModule) {
                                let span = self.peek().span;
                                self.diags.push(Diagnostic::error(
                                        "E0003",
                                        "`#Target(Web)` isn't valid on a module".to_string(),
                                        "`Web` is a file-level default-backend marker, not a partition ceiling".to_string(),
                                        "move `#Target(Web)` to the top of the file, outside any module; use `#Target(Wasm)` or `#Target(Js)` on a module".to_string(),
                                        Some(span),
                                    ));
                                self.sync_top();
                                continue;
                            }
                            if default_target.is_some() {
                                let span = self.peek().span;
                                self.diags.push(Diagnostic::error(
                                    "E0003",
                                    "only one `#Target(Web)` marker is allowed per file".to_string(),
                                    "a file may declare at most one default backend".to_string(),
                                    "remove the duplicate `#Target(Web)` marker".to_string(),
                                    Some(span),
                                ));
                                self.sync_top();
                                continue;
                            }
                            default_target = Some(crate::Syntax::BUILD_TARGET_WEB.to_string());
                            continue;
                        }
                        Ok(TargetMarker::Bucket(target)) => {
                            if matches!(self.peek().kind, TokKind::KwModule) {
                                match self.code_module_with_pkg_and_target(false, false, Some(target)) {
                                    Ok(item) => items.push(item),
                                    Err(d) => {
                                        self.diags.push(d);
                                        self.sync_top();
                                    }
                                }
                                continue;
                            }
                            if web_target_ceiling.is_some() {
                                let span = self.peek().span;
                                self.diags.push(Diagnostic::error(
                                    "E0003",
                                    "only one `#Target(…)` ceiling is allowed per file".to_string(),
                                    "a file may declare at most one web partition ceiling".to_string(),
                                    "remove the duplicate `#Target(Wasm)` or `#Target(Js)` marker"
                                        .to_string(),
                                    Some(span),
                                ));
                                self.sync_top();
                                continue;
                            }
                            web_target_ceiling = Some(target);
                            continue;
                        }
                        // D-OSTARGET1=A: `#Target(Os.X)` attaches to the `impl` block
                        // that immediately follows — item scope, not file scope.
                        Ok(TargetMarker::Os(os)) => {
                            match self.os_gated_impl(os) {
                                Ok(item) => items.push(item),
                                Err(d) => {
                                    self.diags.push(d);
                                    self.sync_top();
                                }
                            }
                            continue;
                        }
                        Err(d) => {
                            self.diags.push(d);
                            self.sync_top();
                            continue;
                        }
                    },
                    // D-HTMLPAIR1 (ratified 2026-07-01, c134): `#Html("path.html")` — explicit
                    // companion host page for `--target=web` builds.
                    TokKind::Hash if self.at_html_marker() => match self.parse_html_marker() {
                        Ok(path) => {
                            if html_path.is_some() {
                                let span = self.peek().span;
                                self.diags.push(Diagnostic::error(
                                    "E0003",
                                    "only one `#Html(…)` marker is allowed per file".to_string(),
                                    "a file may declare at most one companion host page".to_string(),
                                    "remove the duplicate `#Html(…)` marker".to_string(),
                                    Some(span),
                                ));
                                self.sync_top();
                                continue;
                            }
                            html_path = Some(path);
                            continue;
                        }
                        Err(d) => {
                            self.diags.push(d);
                            self.sync_top();
                            continue;
                        }
                    },
                    TokKind::KwExtern => self.extern_rust_block().map(Item::ExternRust),
                    TokKind::KwFn => self.func().map(Item::Func),
                    TokKind::Hash if self.at_meta_attr() => {
                        if matches!(self.meta_attr_next_kind(), Some(TokKind::KwConst)) {
                            self.const_def().map(Item::Const)
                        } else if matches!(self.meta_attr_next_kind(), Some(TokKind::KwComptime)) {
                            self.comptime_def().map(Item::Const)
                        } else {
                            self.func().map(Item::Func)
                        }
                    }
                    // S60 (D-CASING1 follow-on) / D-MARKERMOVE2: `@Pure fn name(…)`
                    // purity modifier (old `#Pure` spelling is E0062, taught in `func()`).
                    TokKind::Hash | TokKind::At if self.at_pure_fn() => self.func().map(Item::Func),
                    // D-TAINT1: `#Sanitizer fn name(…)` taint-strip modifier.
                    TokKind::Hash if self.at_sanitizer_fn() => self.func().map(Item::Func),
                    // D-REPLAY1: `#Replayable fn name(…)` deterministic replay guard.
                    TokKind::Hash if self.at_replayable_fn() => self.func().map(Item::Func),
                    // D-SCHEDULE1 (card #505): `#Task fn name(…)` / `#Every(…) fn name(…)`
                    // schedule-as-code markers on a free function.
                    TokKind::Hash if self.at_task_fn() || self.at_every_fn() => {
                        self.func().map(Item::Func)
                    }
                    // D-MUSTUSE1 / D-MARKERMOVE1: `@MustUse fn name(…)` — result cannot be
                    // silently ignored (old `@MustUse` spelling is E0062, taught in `func()`).
                    TokKind::Hash | TokKind::At if self.at_must_use_fn() => self.func().map(Item::Func),
                    // D-METHODMACRO1=A: `@Inline fn name(…)` / `@InlineAlways fn name(…)`.
                    TokKind::Hash | TokKind::At if self.at_inline_fn() => self.func().map(Item::Func),
                    // D-STATE1: `#State(S) fn` / `#Transition(From -> To) fn` typestate
                    // markers on a free function.
                    TokKind::Hash if self.at_state_fn() || self.at_transition_fn() => {
                        self.func().map(Item::Func)
                    }
                    // D-PREPOST1: `@Pre(cond, "msg")` / `@Post(cond, "msg")` before a
                    // free function — parsed (and repeated/mixed) inside `func()`.
                    TokKind::Hash | TokKind::At
                        if self.at_contract_clause_fn(Syntax::CONTRACT_PRE)
                            || self.at_contract_clause_fn(Syntax::CONTRACT_POST) =>
                    {
                        self.func().map(Item::Func)
                    }
                    TokKind::Hash if self.at_web_partition_fn() => self.func().map(Item::Func),
                    // D-S14-PAUSE: bare lowercase `pure` teaching is paused.
                    TokKind::Ident(n)
                        if retired_s14_teaching_enabled()
                            && n == Syntax::FOREIGN_PURE
                            && self.foreign_pure_follows() =>
                    {
                        let t = self.bump();
                        self.diags.push(self.foreign_pure_diag(t.span));
                        self.func_with_purity(true).map(Item::Func)
                    }
                    // D-TAINT-SAN: bare lowercase `sanitizer fn` is the retired
                    // spelling of the taint-strip modifier (E0059). Point at
                    // `#Sanitizer`, then parse as if `#Sanitizer fn`.
                    TokKind::Ident(n)
                        if n == Syntax::FOREIGN_SANITIZER && self.foreign_sanitizer_follows() =>
                    {
                        let t = self.bump();
                        self.diags.push(self.foreign_sanitizer_diag(t.span));
                        self.func_with_modifiers(false, true).map(Item::Func)
                    }
                    TokKind::KwPriv | TokKind::KwPub if matches!(self.peek2().kind, TokKind::Colon) => {
                        let span = Span::new(self.peek().span.start, self.peek2().span.end);
                        self.bump();
                        self.bump();
                        self.diags.push(Diagnostic::error(
                            "E0415",
                            "section visibility labels like `pub:` / `priv:` are not supported"
                                .to_string(),
                            "moving an item above or below a label would silently change whether it exports"
                                .to_string(),
                            format!(
                                "write `#{}` once at the top of the file, then mark exceptions with `{}`",
                                Syntax::MARKER_PUB_FILE,
                                Syntax::KW_PRIV
                            ),
                            Some(span),
                        ));
                        continue;
                    }
                    TokKind::KwPriv => {
                        let (is_pub, is_package_pub) = self.parse_item_visibility();
                        self.item_after_visibility(is_pub, is_package_pub)
                    }
                    TokKind::Ident(n) if n == Syntax::FOREIGN_PRIVATE => {
                        let (is_pub, is_package_pub) = self.parse_item_visibility();
                        self.item_after_visibility(is_pub, is_package_pub)
                    }
                    TokKind::KwPub => {
                        // D-PUBPKG1=A: `pub(package)` qualifier — peek2 is `(`.
                        if matches!(self.peek2().kind, TokKind::LParen) {
                            let (is_pub, is_package_pub) = self.parse_pub_qualifier();
                            // Redispatch on what follows the qualifier.
                            match self.peek().kind.clone() {
                                TokKind::KwStruct => self
                                    .struct_def_after_pub_pkg(is_pub, is_package_pub)
                                    .map(Item::Struct),
                                TokKind::KwEnum => self
                                    .enum_def_after_pub(is_pub, is_package_pub)
                                    .map(Item::Enum),
                                TokKind::KwTrait => self.trait_def(false).map(|mut td| {
                                    td.is_pub = is_pub;
                                    td.is_package_pub = is_package_pub;
                                    Item::Trait(td)
                                }),
                                TokKind::KwTag => self.tag_def(false).map(|mut td| {
                                    td.is_pub = is_pub;
                                    td.is_package_pub = is_package_pub;
                                    Item::Tag(td)
                                }),
                                TokKind::KwFn => self
                                    .bump_then_func_after_fn(
                                        is_pub,
                                        is_package_pub,
                                        false,
                                        false,
                                        false,
                                        None,
                                        None,
                                        None,
                                        None,
                                        false,
                                        None,
                                        None,
                                        None,
                                    )
                                    .map(Item::Func),
                                TokKind::KwModule if self.is_code_module_at(1) => {
                                    self.code_module_with_pkg(is_pub, is_package_pub)
                                }
                                TokKind::KwUse => match self.import_decl() {
                                    Ok(mut imp) => {
                                        imp.is_pub = is_pub;
                                        imp.is_package_pub = is_package_pub;
                                        imports.push(imp);
                                        continue;
                                    }
                                    Err(d) => {
                                        self.diags.push(d);
                                        self.sync_stmt();
                                        continue;
                                    }
                                },
                                TokKind::Ident(ref n) if n.as_str() == Syntax::KW_STATE_DECL => self
                                    .state_decl_with_pkg(is_pub, is_package_pub)
                                    .map(Item::StateDecl),
                                TokKind::Ident(ref n) if n.as_str() == Syntax::KW_PROTOCOL => self
                                    .protocol_decl_with_pkg(is_pub, is_package_pub)
                                    .map(Item::ProtocolDecl),
                                TokKind::Ident(ref n) if n.as_str() == Syntax::KW_ALIAS => self
                                    .type_alias_def(is_pub, is_package_pub)
                                    .map(Item::TypeAlias),
                                _ => self
                                    .func_after_fn(
                                        is_pub,
                                        is_package_pub,
                                        false,
                                        None,
                                        None,
                                        false,
                                        false,
                                        None,
                                        None,
                                        None,
                                        false,
                                        None,
                                        false,
                                        None,
                                        None,
                                        None,
                                        false,
                                        false,
                                        None,
                                        false,
                                        None,
                                    )
                                    .map(Item::Func),
                            }
                        } else {
                            match self.peek2().kind {
                                // D-REPRC1: `pub #layout(c) struct Name { … }`
                                TokKind::Hash
                                    if {
                                        matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::ATTR_LAYOUT)
                                    } =>
                                {
                                    self.bump(); // consume `pub`
                                    self.layout_type_def(true)
                                }
                                // D-MIGRATE1/D-MARKERMOVE1: `pub @PublishedSchema struct
                                // Name { … }` (retired `pub @PublishedSchema` teaches E0062).
                                TokKind::Hash | TokKind::At
                                    if {
                                        matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::ATTR_PUBLISHED_SCHEMA)
                                    } =>
                                {
                                    self.bump(); // consume `pub`
                                    self.published_schema_struct_def(true).map(Item::Struct)
                                }
                                // D-LIN1: `pub #SingleUse struct|enum Name { … }`
                                TokKind::Hash
                                    if {
                                        matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::ATTR_SINGLE_USE)
                                    } =>
                                {
                                    self.bump(); // consume `pub`
                                    self.single_use_type_def(true)
                                }
                                // D-MUSTUSE1/D-MARKERMOVE1: `pub @MustUse struct|enum Name
                                // { … }` (retired `pub @MustUse` teaches E0062).
                                TokKind::Hash | TokKind::At
                                    if {
                                        matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::ATTR_MUST_USE)
                                    } =>
                                {
                                    self.bump(); // consume `pub`
                                    self.must_use_type_def(true)
                                }
                                // D-QUAL3: `pub #UnitFamily(name) { m, … }`
                                TokKind::Hash
                                    if {
                                        matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::ATTR_UNIT_FAMILY)
                                    } =>
                                {
                                    self.bump(); // consume `pub`
                                    self.unit_family_def(true, false).map(Item::UnitFamily)
                                }
                                TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                                TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                                TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
                                TokKind::KwTag => self.tag_def(false).map(Item::Tag),
                                // D-STATE-DECL: `pub state TypeName { A, B, C }`
                                TokKind::Ident(ref n) if n.as_str() == Syntax::KW_STATE_DECL => {
                                    self.bump(); // consume `pub`
                                    self.state_decl(true).map(Item::StateDecl)
                                }
                                // D-PROTO1/D-PROTO2: `pub protocol Name { … }`
                                TokKind::Ident(ref n) if n.as_str() == Syntax::KW_PROTOCOL => {
                                    self.bump(); // consume `pub`
                                    self.protocol_decl(true).map(Item::ProtocolDecl)
                                }
                                TokKind::Ident(ref n) if n.as_str() == Syntax::KW_ALIAS => {
                                    self.bump(); // consume `pub`
                                    self.type_alias_def(true, false).map(Item::TypeAlias)
                                }
                                TokKind::KwModule if self.is_code_module_at(2) => {
                                    self.code_module(true)
                                }
                                TokKind::KwUse => {
                                    self.bump(); // consume `pub`
                                    match self.import_decl() {
                                        Ok(mut imp) => {
                                            imp.is_pub = true;
                                            imports.push(imp);
                                            continue;
                                        }
                                        Err(d) => {
                                            self.diags.push(d);
                                            self.sync_stmt();
                                            continue;
                                        }
                                    }
                                }
                                _ => self.func().map(Item::Func),
                            }
                        }
                    }
                    // S43 (D-CASING1 follow-on): `#Test "name" { … }`.
                    TokKind::Hash if self.at_test_def() => self.test_def().map(Item::Test),
                    // D-BENCH1 + D-BENCH-MARKER1=A: `#Bench("name") { … }`.
                    TokKind::Hash if self.at_bench_def() => self.bench_def().map(Item::Bench),
                    // D-S14-PAUSE: bare lowercase `test "name" { … }` teaching is paused.
                    TokKind::Ident(n)
                        if retired_s14_teaching_enabled()
                            && n == Syntax::FOREIGN_TEST
                            && self.foreign_test_follows() =>
                    {
                        let t = self.bump();
                        self.diags.push(self.foreign_test_diag(t.span));
                        self.test_def_after_kw().map(Item::Test)
                    }
                    TokKind::KwModule if self.is_code_module_at(1) => self.code_module(false),
                    TokKind::KwModule => self.module_decl().map(Item::Module),
                    TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                    TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                    TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
                    TokKind::KwTag => self.tag_def(false).map(Item::Tag),
                    TokKind::KwImpl => self.impl_or_error_conv(),
                    TokKind::Hash | TokKind::At
                        if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_INVARIANT)
                            || self.at_bundle_distinct_def() =>
                    {
                        let (is_pub, is_package_pub) = self.parse_item_visibility();
                        self.distinct_def(is_pub, is_package_pub)
                            .map(Item::Distinct)
                    }
                    // D-ATTR2 / D-SERDE: `#[RenameAll(camel)] struct …` (serde stays `#`).
                    // D-ATTR1: `#Rename(...) struct …` — one marker, no brackets.
                    // D-MARKER-FAMILY1/G2/G3: `@[Codable, Debug]` / `@Codable struct …`
                    // — contract-plane derives, stackable with a `#[…]` serde group.
                    TokKind::Hash | TokKind::At
                        if self.at_marker_list()
                            || self.at_single_type_marker()
                            || self.at_contract_marker_list()
                            || self.at_single_contract_type_marker() =>
                    {
                        self.type_def_with_any_markers()
                    }
                    TokKind::Hash if self.at_c_module() => self.c_module().map(Item::CModule),
                    TokKind::At if self.at_retired_at_c_module() => {
                        self.retired_at_c_module().map(Item::CModule)
                    }
                    // D-FFI-INLINE1=A (card #501): `#FFI(<lang>) fn` inline
                    // foreign tier, incl. the `#Unsafe("…") #FFI(asm) fn` gated
                    // form. Checked before the unsafe arm so the gated form routes
                    // here rather than to the plain `#Unsafe fn` path.
                    TokKind::Hash if self.at_ffi_fn() => self.ffi_fn().map(Item::Func),
                    TokKind::Hash if self.at_unsafe_fn() => self.unsafe_fn().map(Item::Func),
                    TokKind::Hash if self.at_reactive_fn() => self.reactive_fn().map(Item::Func),
                    TokKind::Hash if self.at_unit_family_def() => {
                        let (is_pub, is_package_pub) = self.parse_item_visibility();
                        self.unit_family_def(is_pub, is_package_pub)
                            .map(Item::UnitFamily)
                    }
                    // D-REPRC1: `#layout(c) struct Name { … }`
                    TokKind::Hash if self.at_layout_struct() => {
                        self.layout_type_def(false)
                    }
                    // D-MIGRATE1/D-MARKERMOVE1: `@PublishedSchema struct Name { … }`
                    TokKind::Hash | TokKind::At if self.at_published_schema_struct() => {
                        self.published_schema_struct_def(false).map(Item::Struct)
                    }
                    // D-LIN1: `#SingleUse struct|enum Name { … }`
                    TokKind::Hash if self.at_single_use_type() => self.single_use_type_def(false),
                    // D-MUSTUSE1/D-MARKERMOVE1: `@MustUse struct|enum Name { … }`
                    TokKind::Hash | TokKind::At if self.at_must_use_type() => {
                        self.must_use_type_def(false)
                    }
                    // D-MIGRATE1: `migration TypeName { rename a -> b }`
                    TokKind::Ident(n) if n == Syntax::KW_MIGRATION && self.at_migration_block() => {
                        self.migration_decl().map(Item::Migration)
                    }
                    // D-STATE-DECL: `state TypeName { A, B, C }`
                    TokKind::Ident(n) if n == Syntax::KW_STATE_DECL && self.at_state_block() => {
                        let (is_pub, is_package_pub) = self.parse_item_visibility();
                        self.state_decl_with_pkg(is_pub, is_package_pub)
                            .map(Item::StateDecl)
                    }
                    // D-PROTO1/D-PROTO2: `protocol Name { client -> server: Msg(…) }`
                    TokKind::Ident(n) if n == Syntax::KW_PROTOCOL && self.at_protocol_block() => {
                        let (is_pub, is_package_pub) = self.parse_item_visibility();
                        self.protocol_decl_with_pkg(is_pub, is_package_pub)
                            .map(Item::ProtocolDecl)
                    }
                    // D-TYPEALIAS1: `alias Name<T> = …`
                    TokKind::Ident(n) if n == Syntax::KW_ALIAS => {
                        let (is_pub, is_package_pub) = self.parse_item_visibility();
                        self.type_alias_def(is_pub, is_package_pub)
                            .map(Item::TypeAlias)
                    }
                    // D-METADERIVE1=A (amended 2026-07-01): `derive T.Trait { … }` — user-authored derive.
                    TokKind::KwDerive => self.user_derive_def().map(Item::UserDerive),
                    TokKind::Hash
                        if matches!(
                            &self.peek2().kind,
                            TokKind::Ident(n)
                                if n == Syntax::ATTR_OFF || n == Syntax::ATTR_DEBUG_ONLY
                        ) =>
                    {
                        let hash = self.bump().span;
                        let name_tok = self.bump();
                        let name = match &name_tok.kind {
                            TokKind::Ident(n) => n.clone(),
                            _ => String::new(),
                        };
                        self.diags.push(Diagnostic::error(
                            "E0342",
                            format!("`#{}` belongs before a statement", name),
                            "statement switch attributes control code inside a function body, not top-level declarations".to_string(),
                            format!(
                                "move it inside a function, e.g. `#{} print(\"debug\")`, or remove it from the declaration",
                                name
                            ),
                            Some(Span::new(hash.start, name_tok.span.end)),
                        ));
                        continue;
                    }
                    // D-MARK-META1=B: maturity words are closed `#Meta` values,
                    // never standalone markers on either plane. Reject them as
                    // ordinary unknown markers with no retired-spelling teaching.
                    TokKind::Hash | TokKind::At
                        if matches!(&self.peek2().kind, TokKind::Ident(n)
                            if n == Syntax::ATTR_EXPERIMENTAL
                                || n == Syntax::ATTR_TESTED
                                || n == Syntax::ATTR_HARDENED) =>
                    {
                        let sigil = self.bump();
                        let name_tok = self.bump(); // guard proves Ident
                        let name = match &name_tok.kind {
                            TokKind::Ident(name) => name.clone(),
                            _ => unreachable!(),
                        };
                        let name_span = name_tok.span;
                        self.diags.push(Diagnostic::error(
                            "E0003",
                            format!("`{}{}` isn't a known marker", if matches!(sigil.kind, TokKind::Hash) { "#" } else { "@" }, name),
                            "maturity is tooling metadata, not a standalone marker".to_string(),
                            "remove this marker".to_string(),
                            Some(Span::new(sigil.span.start, name_span.end)),
                        ));
                        self.sync_top();
                        continue;
                    }
                    TokKind::KwConst | TokKind::Hash => self.const_def().map(Item::Const),
                    // D-PERSIST1: `@Persist const NAME = expr;` — module-level
                    // binding that survives a `jet dev` hot reload.
                    TokKind::At if self.at_persist_const() => self.const_def().map(Item::Const),
                    // D-MARKER-FAMILY1: a `@` not already claimed by a contract-marker
                    // arm above. If the name is a known `#` directive, teach E0063
                    // (wrong plane); otherwise it's the old bare-attribute spelling
                    // (E0990, D-ATTR1) — unknown name, or `@` not followed by an ident.
                    // `Unsafe` is a dedicated lexer keyword token (`KwUnsafe`), not a
                    // generic `Ident`, so it needs its own arm of this same guard.
                    TokKind::At
                        if matches!(&self.peek2().kind, TokKind::Ident(n) if Syntax::is_directive_marker(n))
                            || matches!(self.peek2().kind, TokKind::KwUnsafe) =>
                    {
                        let t = self.bump(); // `@`
                        let name_tok = self.bump(); // the directive name
                        let name = match &name_tok.kind {
                            TokKind::Ident(n) => n.clone(),
                            TokKind::KwUnsafe => Syntax::KW_UNSAFE.to_string(),
                            _ => String::new(),
                        };
                        self.diags.push(Self::e0063_directive_on_at(
                            &name,
                            Span::new(t.span.start, name_tok.span.end),
                        ));
                        self.sync_top();
                        continue;
                    }
                    TokKind::At => {
                        let t = self.bump();
                        self.diags.push(Diagnostic::error(
                            "E0990",
                            format!("attributes use `{}`, not `@`", Syntax::ATTR_PREFIX),
                            "in Jet, `@` is for loop labels; attributes and markers use `#` (D-ATTR1)"
                                .to_string(),
                            "write `#Unsafe(\"…\")`, `#Test(\"…\")`, or `@Codable`/`@[Codable, MustUse]` instead of `@…`"
                                .to_string(),
                            Some(t.span),
                        ));
                        self.sync_top();
                        continue;
                    }
                    TokKind::KwComptime => self.comptime_def().map(Item::Const),
                    TokKind::Ident(_) if self.at_distinct_def() => {
                        let (is_pub, is_package_pub) = self.parse_item_visibility();
                        self.distinct_def(is_pub, is_package_pub)
                            .map(Item::Distinct)
                    }
                    TokKind::Ident(name)
                        if retired_s14_teaching_enabled() && name == Syntax::FOREIGN_CLASS =>
                    {
                        let t = self.bump();
                        self.diags.push(Diagnostic::error(
                            "E0021",
                            format!(
                                "types are written with `{}`, not `{}`",
                                Syntax::KW_STRUCT,
                                Syntax::FOREIGN_CLASS
                            ),
                            format!(
                                "{} uses exactly one spelling for each thing, so all code reads the same",
                                Syntax::LANG_NAME
                            ),
                            format!(
                                "replace `{}` with `{}`",
                                Syntax::FOREIGN_CLASS,
                                Syntax::KW_STRUCT
                            ),
                            Some(t.span),
                        ));
                        self.struct_def(false).map(Item::Struct)
                    }
                    TokKind::Ident(name)
                        if retired_s14_teaching_enabled() && name == Syntax::FOREIGN_INTERFACE =>
                    {
                        let t = self.bump();
                        self.diags.push(Diagnostic::error(
                            "E0022",
                            format!(
                                "`{}` is spelled `{}` in {}",
                                Syntax::FOREIGN_INTERFACE,
                                Syntax::KW_TRAIT,
                                Syntax::LANG_NAME
                            ),
                            format!(
                                "traits are written with `{}` — see docs for `trait Name {{ … }}`",
                                Syntax::KW_TRAIT
                            ),
                            format!(
                                "replace `{}` with `{}`",
                                Syntax::FOREIGN_INTERFACE,
                                Syntax::KW_TRAIT
                            ),
                            Some(t.span),
                        ));
                        self.sync_top();
                        continue;
                    }
                    // D-NAMESPACE1=A: `namespace` is not a Jet keyword — E0323 teaching error.
                    TokKind::Ident(name) if name == Syntax::FOREIGN_NAMESPACE => {
                        let t = self.bump();
                        self.diags.push(Diagnostic::error(
                            "E0323",
                            "in-file grouping uses `module name { }`, not `namespace`".to_string(),
                            "Jet has one spelling for in-file grouping: a named `module` block"
                                .to_string(),
                            "write `module mygroup { fn foo() { … } }` instead".to_string(),
                            Some(t.span),
                        ));
                        self.sync_top();
                        continue;
                    }
                    TokKind::Ident(name)
                        if retired_s14_teaching_enabled()
                            && (name == Syntax::FOREIGN_DEF || name == Syntax::FOREIGN_FUNC) =>
                    {
                        // D-S14-PAUSE: def/func teaching is paused.
                        let t = self.bump();
                        let foreign = if let TokKind::Ident(n) = &t.kind {
                            n.clone()
                        } else {
                            unreachable!()
                        };
                        self.diags.push(Diagnostic::error(
                            "E0008",
                            format!(
                                "functions are written with `{}`, not `{}`",
                                Syntax::KW_FN,
                                foreign
                            ),
                            "Jet has exactly one spelling for each thing, so all code reads the same"
                                .to_string(),
                            format!("replace `{}` with `{}`", foreign, Syntax::KW_FN),
                            Some(t.span),
                        ));
                        self.func_after_fn(
                            false, false, false, None, None, false, false, None, None, None, false, None,
                            false, None, None, None, false, false, None, false, None,
                        )
                        .map(Item::Func)
                    }
                    TokKind::Ident(name)
                        if retired_s14_teaching_enabled() && name == Syntax::FOREIGN_IMPORT =>
                    {
                        let t = self.bump();
                        self.diags.push(Diagnostic::error(
                            "E0015",
                            format!(
                                "{} uses `{}`, not `{}`",
                                Syntax::LANG_NAME,
                                Syntax::KW_USE,
                                Syntax::FOREIGN_IMPORT
                            ),
                            format!(
                                "other files are brought in with `{} \"path\"` or `{} name` (S16; M6)",
                                Syntax::KW_USE,
                                Syntax::KW_USE
                            ),
                            format!(
                                "replace with `{} \"path\";`, `{} name;`, or `{} \"path\" {} alias;`",
                                Syntax::KW_USE,
                                Syntax::KW_USE,
                                Syntax::KW_USE,
                                Syntax::KW_AS
                            ),
                            Some(t.span),
                        ));
                        self.sync_stmt();
                        continue;
                    }
                    other => {
                        let d = Diagnostic::error(
                            "E0003",
                            format!(
                                "expected `{}`, `#{}`, `{}`, or `{}` here, found {}",
                                Syntax::KW_FN,
                                Syntax::KW_TEST,
                                Syntax::KW_STRUCT,
                                Syntax::KW_CONST,
                                describe(other)
                            ),
                            "at the top level of a file, only definitions can appear".to_string(),
                            format!(
                                "define a function ({} run() {{ ... }}), #{} block, struct, or const",
                                Syntax::KW_FN,
                                Syntax::KW_TEST
                            ),
                            Some(self.peek().span),
                        );
                        self.diags.push(d);
                        self.bump();
                        self.sync_top();
                        continue;
                    }
                };
                match r {
                    Ok(item) => items.push(Self::normalize_external_method_item(item)),
                    Err(d) => {
                        self.diags.push(d);
                        self.sync_top();
                    }
                }
            }
            Program {
                imports,
                items,
                web_target_ceiling,
                pub_file,
                no_prelude,
                default_target,
                html_path,
                no_alloc_policy,
            }
        }
    
}
