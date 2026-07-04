use super::*;

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): what a `#Target(…)` marker parsed to — a
/// partition-ceiling `Bucket` (`Wasm`/`Js`, existing D-WASM1 meaning),
/// `DefaultWeb` (`Web` — this file's default CLI backend, a different axis),
/// or `Os` (D-OSTARGET1=A: `Os.Linux`/`Os.Macos`/`Os.Windows` — the native
/// platform-gating axis, item-scoped rather than file/module-scoped).
pub(super) enum TargetMarker {
    Bucket(crate::Syntax::WebBucket),
    DefaultWeb,
    Os(crate::Syntax::OsTarget),
}

impl<'a> Parser<'a> {
    /// D-MEM1/S7 (D-NOALLOC-SEM1=A): `policy no_alloc;` — the file's
    /// allocation floor. `no_alloc` is the only ratified policy name today;
    /// any other name is an ordinary "expected X, found Y" parse error (the
    /// full policy list is a follow-on ballot, not this stage's business).
    fn policy_decl(&mut self) -> Result<Span, Diagnostic> {
        let start = self.bump().span; // consume `policy`
        match &self.peek().kind {
            TokKind::Ident(n) if n == Syntax::POLICY_NO_ALLOC => {
                let name_span = self.bump().span;
                self.expect(TokKind::Semi, "after a `policy` declaration")?;
                Ok(Span::new(start.start, name_span.end))
            }
            other => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected `{}` after `{}`, found {}",
                    Syntax::POLICY_NO_ALLOC,
                    Syntax::KW_POLICY,
                    describe(other)
                ),
                format!(
                    "`{}` is the only ratified policy name today",
                    Syntax::POLICY_NO_ALLOC
                ),
                format!("write `{} {}`", Syntax::KW_POLICY, Syntax::POLICY_NO_ALLOC),
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
            TokKind::Int(n) => {
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
        while matches!(self.peek().kind, TokKind::Dot) && matches!(self.peek2().kind, TokKind::Int(_))
        {
            self.bump(); // `.`
            let TokKind::Int(n) = self.peek().kind else {
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

    pub(super) fn program(&mut self) -> Program {
        let mut imports = Vec::new();
        let mut items = Vec::new();
        let mut web_target_ceiling = None;
        let mut default_target: Option<String> = None;
        let mut html_path: Option<String> = None;
        let mut pub_file = false;
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
                TokKind::Ident(n) if n == Syntax::KW_POLICY => match self.policy_decl() {
                    Ok(span) => {
                        if let Some(first) = no_alloc_policy {
                            self.diags.push(Diagnostic::error(
                                "E0003",
                                "only one `policy no_alloc` declaration is allowed per file"
                                    .to_string(),
                                "a file may declare its allocation floor once".to_string(),
                                "remove the duplicate `policy no_alloc` line".to_string(),
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
                TokKind::Hash if self.at_web_target() => match self.parse_web_target_marker() {
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
                // S60 (D-CASING1 follow-on) / D-MARKERMOVE2: `@Pure fn name(…)`
                // purity modifier (old `@Pure` spelling is E0062, taught in `func()`).
                // D-MATURITY1=B / D-MARKERMOVE1: `@Experimental`/`@Tested`/`@Hardened`
                // before fn/pub fn — consumed and silently ignored
                // (docs/reference/maturity-tags.md). Old `#` spelling is E0062.
                TokKind::Hash | TokKind::At if self.at_maturity_fn() => {
                    let sigil = self.bump(); // `#` or `@`
                    let tag_tok = self.bump(); // tag name — erase, no AST field
                    if matches!(sigil.kind, TokKind::Hash) {
                        let tag_name = match &tag_tok.kind {
                            TokKind::Ident(n) => n.clone(),
                            _ => String::new(),
                        };
                        self.diags.push(Self::e0062_contract_on_hash(
                            &tag_name,
                            Span::new(sigil.span.start, tag_tok.span.end),
                        ));
                    }
                    continue; // re-enter item loop; next token is `fn` or `pub`
                }
                TokKind::Hash | TokKind::At if self.at_pure_fn() => self.func().map(Item::Func),
                // D-TAINT1: `#Sanitizer fn name(…)` taint-strip modifier.
                TokKind::Hash if self.at_sanitizer_fn() => self.func().map(Item::Func),
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
                // S14: bare lowercase `pure` is the retired spelling (E0053).
                TokKind::Ident(n) if n == Syntax::FOREIGN_PURE && self.foreign_pure_follows() => {
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
                                    false,
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
                                    false,
                                    false,
                                    None,
                                    None,
                                    false,
                                    None,
                                    false,
                                    None,
                                    false,
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
                                self.layout_struct_def(true).map(Item::Struct)
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
                // D-BENCH1: `#Bench "name" { … }`.
                TokKind::Hash if self.at_bench_def() => self.bench_def().map(Item::Bench),
                // S14: bare lowercase `test "name" { … }` is the retired spelling (E0052).
                TokKind::Ident(n) if n == Syntax::FOREIGN_TEST && self.foreign_test_follows() => {
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
                TokKind::Hash if self.at_unsafe_fn() => self.unsafe_fn().map(Item::Func),
                TokKind::Hash if self.at_reactive_fn() => self.reactive_fn().map(Item::Func),
                TokKind::Hash if self.at_unit_family_def() => {
                    let (is_pub, is_package_pub) = self.parse_item_visibility();
                    self.unit_family_def(is_pub, is_package_pub)
                        .map(Item::UnitFamily)
                }
                TokKind::Hash | TokKind::At if self.at_bundle_distinct_def() => {
                    let (is_pub, is_package_pub) = self.parse_item_visibility();
                    self.distinct_def(is_pub, is_package_pub)
                        .map(Item::Distinct)
                }
                // D-REPRC1: `#layout(c) struct Name { … }`
                TokKind::Hash if self.at_layout_struct() => {
                    self.layout_struct_def(false).map(Item::Struct)
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
                TokKind::Ident(name) if name == Syntax::FOREIGN_CLASS => {
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
                TokKind::Ident(name) if name == Syntax::FOREIGN_INTERFACE => {
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
                    if name == Syntax::FOREIGN_DEF || name == Syntax::FOREIGN_FUNC =>
                {
                    // S14 teaching error E0008, then parse as if `fn`.
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
                        false, false, false, false, false, None, None, false, None, false, None,
                        false, false, None,
                    )
                    .map(Item::Func)
                }
                TokKind::Ident(name) if name == Syntax::FOREIGN_IMPORT => {
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
            default_target,
            html_path,
            no_alloc_policy,
        }
    }

    fn normalize_external_method_item(item: Item) -> Item {
        match item {
            Item::Func(mut f) => {
                if let Some((type_name, type_span)) = f.external_type.take() {
                    Item::Impl(ImplDef {
                        type_name,
                        type_span,
                        trait_name: None,
                        trait_span: None,
                        methods: vec![f],
                        delegation_field: None,
                        assoc_type_impls: Vec::new(),
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
    fn at_pub_file(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_PUB_FILE)
    }

    /// S43 (D-CASING1 follow-on): true when the cursor is at the `#Test` marker.
    pub(super) fn at_test_def(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_TEST)
    }

    /// Parse `#Test "name" { … }` (D-CASING1 follow-on). The bare lowercase
    /// `test` path enters via `test_def_after_kw` after emitting E0052.
    pub(super) fn test_def(&mut self) -> Result<crate::AST::TestDef, Diagnostic> {
        self.expect(TokKind::Hash, "before `Test`")?;
        self.bump(); // the `Test` marker ident (guaranteed by at_test_def)
        self.test_def_after_kw()
    }

    fn test_def_after_kw(&mut self) -> Result<crate::AST::TestDef, Diagnostic> {
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
            name,
            name_span,
            params: Vec::new(),
            fn_keyword_span: None,
            body,
        })
    }

    /// D-BENCH1: true when the cursor is at the `#Bench` marker — the exact
    /// sibling of `at_test_def`.
    pub(super) fn at_bench_def(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_BENCH)
    }

    /// Parse `#Bench "name" { … }` (D-BENCH1). Structurally identical to
    /// `test_def`; there is no retired lowercase spelling for benches.
    pub(super) fn bench_def(&mut self) -> Result<crate::AST::BenchDef, Diagnostic> {
        self.expect(TokKind::Hash, "before `Bench`")?;
        self.bump(); // the `Bench` marker ident (guaranteed by at_bench_def)
        let (name, name_span) = self.expect_marker_name(Syntax::KW_BENCH)?;
        self.expect(TokKind::LBrace, "to open the benchmark body")?;
        let body = self.block_stmts();
        Ok(crate::AST::BenchDef {
            name,
            name_span,
            body,
        })
    }

    /// S14: a bare lowercase `test` introduces a test block only when followed by
    /// a quoted name (so an ordinary identifier named `test` is unaffected).
    fn foreign_test_follows(&self) -> bool {
        matches!(self.peek2().kind, TokKind::Str(_))
    }

    fn foreign_test_diag(&self, span: Span) -> Diagnostic {
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

    /// D-MATURITY1=B / D-MARKERMOVE1: true when the cursor is at a maturity-tag
    /// marker before `fn`/`pub`. Handles both `@Experimental fn` (same line) and
    /// `@Experimental\npub fn` (next line, where the lexer inserts a synthetic
    /// `;` terminator after the marker). Matches on either sigil — a stray
    /// `@Experimental` is still recognized here so `func()` can teach E0062
    /// instead of falling through to an unrelated parse error.
    pub(super) fn at_maturity_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash | TokKind::At)
            && matches!(&self.peek2().kind, TokKind::Ident(n)
                if n == Syntax::ATTR_EXPERIMENTAL
                    || n == Syntax::ATTR_TESTED
                    || n == Syntax::ATTR_HARDENED)
            && matches!(
                self.peek3().kind,
                TokKind::KwFn | TokKind::KwPub | TokKind::Semi | TokKind::Eof
            )
    }

    /// S60 (D-CASING1 follow-on) / D-MARKERMOVE1/2: true when the cursor is at
    /// `@Pure fn`/`@Pure pub` — or the retired `@Pure` spelling, so `func()`
    /// can consume it and teach E0062 instead of falling through elsewhere.
    pub(super) fn at_pure_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash | TokKind::At)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_PURE)
            && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
    }

    /// D-TAINT1: true when the cursor is at `#Sanitizer fn`/`#Sanitizer pub fn`.
    pub(super) fn at_sanitizer_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_SANITIZER)
            && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
    }

    /// D-MUSTUSE1 (c18iwxqx) / D-MARKERMOVE1: true when the cursor is at
    /// `@MustUse fn` / `@MustUse pub fn` — or the retired `@MustUse` spelling,
    /// so `func()` can consume it and teach E0062.
    pub(super) fn at_must_use_fn(&self) -> bool {
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
    pub(super) fn at_state_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_STATE)
            && matches!(self.peek3().kind, TokKind::LParen)
    }

    /// D-STATE1: true when the cursor is at `#Transition(…)` (a transition fn marker).
    /// Token stream: `# Transition (`.
    pub(super) fn at_transition_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_TRANSITION)
            && matches!(self.peek3().kind, TokKind::LParen)
    }

    /// S14: bare lowercase `pure` introduces a function only when `fn`/`pub`
    /// follows (so an ordinary identifier named `pure` is unaffected).
    fn foreign_pure_follows(&self) -> bool {
        matches!(self.peek2().kind, TokKind::KwFn | TokKind::KwPub)
    }

    /// D-TAINT-SAN: bare lowercase `sanitizer` is the retired spelling of the
    /// taint-strip modifier, recognized only when `fn`/`pub` follows (so an
    /// ordinary identifier named `sanitizer` elsewhere is unaffected). The
    /// modifier is now the marker `#Sanitizer` — point the user at it (E0059),
    /// mirroring `pure` → `@Pure` (E0053).
    fn foreign_sanitizer_follows(&self) -> bool {
        matches!(self.peek2().kind, TokKind::KwFn | TokKind::KwPub)
    }

    fn foreign_sanitizer_diag(&self, span: Span) -> Diagnostic {
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

    fn foreign_pure_diag(&self, span: Span) -> Diagnostic {
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
    fn expect_marker_name(&mut self, kw: &str) -> Result<(String, Span), Diagnostic> {
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
    fn extern_rust_block(&mut self) -> Result<crate::AST::ExternRustBlock, Diagnostic> {
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

    fn extern_fn(&mut self) -> Result<crate::AST::ExternFn, Diagnostic> {
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
        if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            let (ty, _) = self.return_type()?;
            return_type = Some(ty);
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
            rust_path,
            rust_path_span,
            span: Span::new(fn_start, end),
        })
    }

    /// D-REACTCORE1: is the cursor at `#Reactive fn …` or `#Reactive pub fn …`?
    pub(crate) fn at_reactive_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_REACTIVE)
            && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
    }

    /// D-WASM1=A: is the cursor at `#Wasm fn` / `#Js fn` / `#WasmExport fn`?
    pub(crate) fn at_web_partition_fn(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::Hash) {
            return false;
        }
        let is_marker = matches!(
            &self.peek2().kind,
            TokKind::Ident(n)
                if n == Syntax::ATTR_WASM
                    || n == Syntax::ATTR_JS
                    || n == Syntax::ATTR_WASM_EXPORT
        );
        if !is_marker {
            return false;
        }
        self.token_after_web_marker_is_fn(2)
    }

    /// True when `fn` / `pub fn` follows a web partition marker, allowing a line break.
    fn token_after_web_marker_is_fn(&self, start: usize) -> bool {
        let mut i = self.pos + start;
        while i < self.toks.len() {
            match &self.toks[i].kind {
                TokKind::Semi => i += 1,
                TokKind::KwFn => return true,
                TokKind::KwPub => return true,
                _ => return false,
            }
        }
        false
    }

    /// D-REACTCORE1 (ratified 2026-06-27, opt D): parse `#Reactive fn …`. The body
    /// lowers to a reactive effect scope at codegen; sema requires a unit return.
    pub(crate) fn reactive_fn(&mut self) -> Result<Func, Diagnostic> {
        self.expect(TokKind::Hash, "before `Reactive`")?;
        self.expect_ident(&format!("`#{}`", Syntax::KW_REACTIVE))?;
        let (is_pub, is_package_pub) = self.parse_item_visibility();
        self.expect_kw(TokKind::KwFn, "after `#Reactive`")?;
        self.func_after_fn(
            is_pub,
            is_package_pub,
            false,
            false,
            false,
            None,
            None,
            true,
            None,
            false,
            None,
            false,
            false,
            None,
        )
    }

    /// D-UNSAFE2: is the cursor at `#Unsafe fn …` or `#Unsafe("…") fn …`?
    fn at_unsafe_fn(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::Hash) {
            return false;
        }
        if !matches!(self.peek2().kind, TokKind::KwUnsafe) {
            return false;
        }
        // `#Unsafe fn` or `#Unsafe pub fn` (no reason arg)
        if matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub) {
            return true;
        }
        // `#Unsafe("…") fn` or `#Unsafe("…") pub fn`
        // tokens (same line): `#`[0] `Unsafe`[1] `(`[2] `"str"`[3] `)`[4] `fn`/`pub`[5]
        // tokens (split line): `#`[0] `Unsafe`[1] `(`[2] `"str"`[3] `)`[4] `;`[5] `fn`/`pub`[6]
        // S6-R inserts a synthetic `;` after `)` when the reason and `fn` are on separate lines.
        if matches!(self.peek3().kind, TokKind::LParen) {
            let after_close = if matches!(self.peek6().kind, TokKind::Semi) {
                &self.peek7().kind
            } else {
                &self.peek6().kind
            };
            return matches!(after_close, TokKind::KwFn | TokKind::KwPub);
        }
        false
    }

    /// D-UNSAFE2 (ratified 2026-06-22, opt B): parse `#Unsafe("reason") fn …`
    /// or bare `#Unsafe fn …` (reason-less; L3101 fires in sema). The body is
    /// checked like any other fn; the contract is enforced at call sites (E3103).
    fn unsafe_fn(&mut self) -> Result<Func, Diagnostic> {
        self.expect(TokKind::Hash, "before `Unsafe`")?;
        self.expect_kw(TokKind::KwUnsafe, "to mark a whole-function contract")?;
        // Optional `("reason")` argument.
        if matches!(self.peek().kind, TokKind::LParen) {
            self.bump(); // `(`
            let _ = self.expect_plain_string(
                "for the safety reason",
                "`#Unsafe` takes one piece of quoted text explaining why the function is safe to call",
                "write: #Unsafe(\"caller must ensure …\") fn …",
            )?;
            self.expect(TokKind::RParen, "after the safety reason")?;
            // S6-R: when `#Unsafe("reason")` is on its own line above `fn`,
            // the lexer inserts a synthetic `;` after `)`. Skip it.
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
        }
        let (is_pub, is_package_pub) = self.parse_item_visibility();
        self.expect_kw(TokKind::KwFn, "after `#Unsafe`")?;
        self.func_after_fn(
            is_pub,
            is_package_pub,
            true,
            false,
            false,
            None,
            None,
            false,
            None,
            false,
            None,
            false,
            false,
            None,
        )
    }

    /// S59 (E2-M14): is the cursor at the start of a C FFI module — `#Extern
    /// module …` or `#Bindgen module …`? Retired lowercase markers are also
    /// recognized here so E0060 can recover to the canonical form.
    fn at_c_module(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::Hash) {
            return false;
        }
        let intro_is_c = match &self.peek2().kind {
            TokKind::KwExtern => true,
            TokKind::Ident(n) => {
                n == Syntax::ATTR_EXTERN_MODULE
                    || n == Syntax::ATTR_BINDGEN
                    || n == Syntax::ATTR_BINDGEN_RETIRED
            }
            _ => false,
        };
        intro_is_c && matches!(self.peek3().kind, TokKind::KwModule)
    }

    fn at_retired_at_c_module(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::At) {
            return false;
        }
        let intro_is_c = match &self.peek2().kind {
            TokKind::KwExtern => true,
            TokKind::Ident(n) => n == Syntax::ATTR_BINDGEN_RETIRED,
            _ => false,
        };
        intro_is_c && matches!(self.peek3().kind, TokKind::KwModule)
    }

    /// S59 (E2-M14): parse `#Extern module c.<lib> { … }` (overlay) or
    /// `#Bindgen module c.<lib>.__bindgen__ { … }` (generated cache). Body
    /// declarations share the `extern_fn` shape (`fn name(args) -> T = "Sym";`).
    fn c_module(&mut self) -> Result<crate::AST::CModule, Diagnostic> {
        use crate::AST::CModuleKind;
        let start = self.bump().span; // `#`
        let kind = match &self.peek().kind {
            TokKind::KwExtern => {
                let span = Span::new(start.start, self.bump().span.end);
                self.diags
                    .push(self.retired_c_module_marker_diag("#extern", "#Extern", span));
                CModuleKind::Extern
            }
            TokKind::Ident(n) if n == Syntax::ATTR_EXTERN_MODULE => {
                self.bump();
                CModuleKind::Extern
            }
            TokKind::Ident(n) if n == Syntax::ATTR_BINDGEN => {
                self.bump();
                CModuleKind::Bindgen
            }
            TokKind::Ident(n) if n == Syntax::ATTR_BINDGEN_RETIRED => {
                let span = Span::new(start.start, self.bump().span.end);
                self.diags
                    .push(self.retired_c_module_marker_diag("#bindgen", "#Bindgen", span));
                CModuleKind::Bindgen
            }
            other => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected `{}` or `{}` after `#`, found {}",
                        Syntax::ATTR_EXTERN_MODULE,
                        Syntax::ATTR_BINDGEN,
                        describe(other)
                    ),
                    "a C FFI module begins with `#Extern module c.<lib>` or `#Bindgen module c.<lib>.__bindgen__`".to_string(),
                    "write: #Extern module c.raylib { fn init_window(w: Int, h: Int, title: String) = \"InitWindow\"; }".to_string(),
                    Some(self.peek().span),
                ));
            }
        };
        self.c_module_after_kind(start, kind)
    }

    fn retired_at_c_module(&mut self) -> Result<crate::AST::CModule, Diagnostic> {
        use crate::AST::CModuleKind;
        let start = self.bump().span; // `@`
        let kind = match &self.peek().kind {
            TokKind::KwExtern => {
                let span = Span::new(start.start, self.bump().span.end);
                self.diags
                    .push(self.retired_c_module_marker_diag("@extern", "#Extern", span));
                CModuleKind::Extern
            }
            TokKind::Ident(n) if n == Syntax::ATTR_BINDGEN_RETIRED => {
                let span = Span::new(start.start, self.bump().span.end);
                self.diags
                    .push(self.retired_c_module_marker_diag("@bindgen", "#Bindgen", span));
                CModuleKind::Bindgen
            }
            _ => unreachable!("at_retired_at_c_module guards marker spelling"),
        };
        self.c_module_after_kind(start, kind)
    }

    fn retired_c_module_marker_diag(&self, old: &str, new: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            "E0060",
            format!("C FFI modules use `{}`, not `{}`", new, old),
            "C FFI markers are PascalCase `#` markers so generated and hand-written bindings share one marker family"
                .to_string(),
            format!("write `{}` before `module c.<lib>`", new),
            Some(span),
        )
    }

    fn c_module_after_kind(
        &mut self,
        start: Span,
        kind: crate::AST::CModuleKind,
    ) -> Result<crate::AST::CModule, Diagnostic> {
        use crate::AST::CModuleKind;
        self.expect_kw(TokKind::KwModule, "to declare a C FFI module")?;

        // Parse the dotted module path: `c` `.` `<lib>` [ `.` `__bindgen__` ].
        let path_start = self.peek().span;
        let (root, _) = self.expect_ident("after `module`")?;
        if root != Syntax::C_MODULE_ROOT {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "a C FFI module path starts with `{}.`, found `{}`",
                    Syntax::C_MODULE_ROOT,
                    root
                ),
                "C libraries live under the `c.` module root — `c.raylib`, `c.sqlite3`".to_string(),
                format!(
                    "write: {} module {}.<lib> {{ … }}",
                    match kind {
                        CModuleKind::Extern => "#Extern",
                        CModuleKind::Bindgen => "#Bindgen",
                    },
                    Syntax::C_MODULE_ROOT
                ),
                Some(path_start),
            ));
        }
        self.expect(TokKind::Dot, "after `c` in a C FFI module path")?;
        let (lib, lib_span) = self.expect_ident("for the C library name")?;
        let mut has_bindgen_seg = false;
        let mut path_end = lib_span.end;
        if matches!(self.peek().kind, TokKind::Dot) {
            self.bump();
            let (seg, seg_span) = self.expect_ident("after `.` in a C FFI module path")?;
            path_end = seg_span.end;
            if seg == Syntax::C_BINDGEN_SEGMENT {
                has_bindgen_seg = true;
            } else {
                return Err(Diagnostic::error(
                    "E0003",
                    format!("a C FFI module path can't have a `.{}` segment", seg),
                    "the only legal third segment is the reserved `__bindgen__` on a generated cache module".to_string(),
                    format!("write: #Extern module {}.{} {{ … }}", Syntax::C_MODULE_ROOT, lib),
                    Some(seg_span),
                ));
            }
        }
        let path_span = Span::new(path_start.start, path_end);

        // E3206: a user overlay must not name the reserved `__bindgen__` segment.
        if kind == CModuleKind::Extern && has_bindgen_seg {
            return Err(Diagnostic::error(
                "E3206",
                format!(
                    "module path `{}.{}.{}` uses the reserved segment `{}`",
                    Syntax::C_MODULE_ROOT, lib, Syntax::C_BINDGEN_SEGMENT, Syntax::C_BINDGEN_SEGMENT
                ),
                format!(
                    "autogen lives in `{}.<lib>.{}`; users declare overlays as `#{} module {}.<lib>` only",
                    Syntax::C_MODULE_ROOT, Syntax::C_BINDGEN_SEGMENT, Syntax::ATTR_EXTERN_MODULE, Syntax::C_MODULE_ROOT
                ),
                format!(
                    "drop `{}` from your module path, or use `#{} module {}.{} {{ … }}`",
                    Syntax::C_BINDGEN_SEGMENT, Syntax::ATTR_EXTERN_MODULE, Syntax::C_MODULE_ROOT, lib
                ),
                Some(path_span),
            ));
        }
        // A `#Bindgen` module must carry the `__bindgen__` segment (it is the
        // generated surface). Without it the path is malformed.
        if kind == CModuleKind::Bindgen && !has_bindgen_seg {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "a `#Bindgen` module path must end in `.{}`",
                    Syntax::C_BINDGEN_SEGMENT
                ),
                "the compiler generates `#Bindgen module c.<lib>.__bindgen__` cache files"
                    .to_string(),
                format!(
                    "write: #Bindgen module {}.{}.{} {{ … }}",
                    Syntax::C_MODULE_ROOT,
                    lib,
                    Syntax::C_BINDGEN_SEGMENT
                ),
                Some(path_span),
            ));
        }

        self.expect(TokKind::LBrace, "to open the C FFI module body")?;
        let mut functions = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            functions.push(self.extern_fn()?);
        }
        self.expect(TokKind::RBrace, "to close the C FFI module body")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::CModule {
            kind,
            lib,
            path_span,
            functions,
            span: Span::new(start.start, end),
        })
    }

    pub(super) fn expect_plain_string(
        &mut self,
        context: &str,
        why_interp: &str,
        fix: &str,
    ) -> Result<(String, Span), Diagnostic> {
        let parts = match &self.peek().kind {
            TokKind::Str(parts) => parts.clone(),
            other => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected a piece of quoted text {}, found {}",
                        context,
                        describe(other)
                    ),
                    why_interp.to_string(),
                    fix.to_string(),
                    Some(self.peek().span),
                ));
            }
        };
        let span = self.bump().span;
        if parts.len() != 1 {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected a piece of quoted text {}, found interpolation",
                    context
                ),
                why_interp.to_string(),
                fix.to_string(),
                Some(span),
            ));
        }
        match &parts[0] {
            StrTokPart::Lit(s) => Ok((s.clone(), span)),
            StrTokPart::Interp(_) => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected a piece of quoted text {}, found interpolation",
                    context
                ),
                why_interp.to_string(),
                fix.to_string(),
                Some(span),
            )),
        }
    }

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
                        "native OS targets are `Os.Linux`, `Os.Macos`, or `Os.Windows`"
                            .to_string(),
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

    /// D-VISDEFAULT2=A: parse one top-level item after `priv` / `private`.
    fn item_after_visibility(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<Item, Diagnostic> {
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
                    false,
                    None,
                )
                .map(Item::Func),
            TokKind::KwModule if self.is_code_module_at(1) => {
                self.code_module_with_pkg(is_pub, is_package_pub)
            }
            TokKind::Ident(ref n) if n.as_str() == Syntax::KW_STATE_DECL => self
                .state_decl_with_pkg(is_pub, is_package_pub)
                .map(Item::StateDecl),
            TokKind::Ident(ref n) if n.as_str() == Syntax::KW_PROTOCOL => self
                .protocol_decl_with_pkg(is_pub, is_package_pub)
                .map(Item::ProtocolDecl),
            TokKind::Ident(ref n) if n.as_str() == Syntax::KW_ALIAS => self
                .type_alias_def(is_pub, is_package_pub)
                .map(Item::TypeAlias),
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_UNIT_FAMILY) => {
                self.unit_family_def(is_pub, is_package_pub)
                    .map(Item::UnitFamily)
            }
            _ => {
                let d = Diagnostic::error(
                    "E0003",
                    format!(
                        "expected `{}`, `{}`, `{}`, or `{}` after `{}`",
                        Syntax::KW_FN,
                        Syntax::KW_STRUCT,
                        Syntax::KW_ENUM,
                        Syntax::KW_ALIAS,
                        Syntax::KW_PRIV
                    ),
                    format!(
                        "`{}` marks one top-level item as private in a `#{}` file",
                        Syntax::KW_PRIV,
                        Syntax::MARKER_PUB_FILE
                    ),
                    format!(
                        "write `{} fn …`, `{} struct …`, or `{} alias …`",
                        Syntax::KW_PRIV,
                        Syntax::KW_PRIV,
                        Syntax::KW_PRIV
                    ),
                    Some(self.peek().span),
                );
                Err(d)
            }
        }
    }

    /// D-VISDEFAULT2=A: parse top-level item visibility (`priv`, `pub`, defaults).
    fn parse_item_visibility(&mut self) -> (bool, bool) {
        if matches!(self.peek().kind, TokKind::KwPub | TokKind::KwPriv)
            && matches!(self.peek2().kind, TokKind::Colon)
        {
            let span = Span::new(self.peek().span.start, self.peek2().span.end);
            self.bump();
            self.bump();
            self.diags.push(Diagnostic::error(
                "E0415",
                "section visibility labels like `pub:` / `priv:` are not supported".to_string(),
                "moving an item above or below a label would silently change whether it exports"
                    .to_string(),
                format!(
                    "write `#{}` once at the top of the file, then mark exceptions with `{}`",
                    Syntax::MARKER_PUB_FILE,
                    Syntax::KW_PRIV
                ),
                Some(span),
            ));
        }
        if let TokKind::Ident(ref n) = self.peek().kind {
            if (n == Syntax::KW_PUB || n == Syntax::KW_PRIV || n == Syntax::FOREIGN_PRIVATE)
                && matches!(self.peek2().kind, TokKind::Colon)
            {
                let span = Span::new(self.peek().span.start, self.peek2().span.end);
                self.bump();
                self.bump();
                self.diags.push(Diagnostic::error(
                    "E0415",
                    "section visibility labels like `pub:` / `priv:` are not supported".to_string(),
                    "moving an item above or below a label would silently change whether it exports"
                        .to_string(),
                    format!(
                        "write `#{}` once at the top of the file, then mark exceptions with `{}`",
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PRIV
                    ),
                    Some(span),
                ));
            }
        }
        if matches!(
            &self.peek().kind,
            TokKind::Ident(n) if n == Syntax::FOREIGN_PRIVATE
        ) {
            let span = self.peek().span;
            self.bump();
            self.diags.push(Diagnostic::error(
                "E0412",
                format!(
                    "write `{}`, not `{}`",
                    Syntax::KW_PRIV,
                    Syntax::FOREIGN_PRIVATE
                ),
                format!(
                    "inside a `#{}` file, `{}` marks an item that stays private to this file",
                    Syntax::MARKER_PUB_FILE,
                    Syntax::KW_PRIV
                ),
                format!(
                    "write `{} fn …` instead of `{} fn …`",
                    Syntax::KW_PRIV,
                    Syntax::FOREIGN_PRIVATE
                ),
                Some(span),
            ));
            if matches!(self.peek().kind, TokKind::KwPub) {
                let span = Span::new(span.start, self.peek().span.end);
                self.bump();
                let _ = self.try_parse_pub_package_suffix();
                self.diags.push(Diagnostic::error(
                    "E0417",
                    format!(
                        "`{}` and `{}` can't both apply to one item",
                        Syntax::KW_PRIV,
                        Syntax::KW_PUB
                    ),
                    "an item is either public or private — pick one qualifier".to_string(),
                    format!(
                        "drop `{}` (already public in a `#{}` file) or remove `{}`",
                        Syntax::KW_PUB,
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PRIV
                    ),
                    Some(span),
                ));
            }
            if !self.pub_file_default {
                self.diags.push(Diagnostic::error(
                    "E0413",
                    format!(
                        "`{}` only applies inside a `#{}` file",
                        Syntax::KW_PRIV,
                        Syntax::MARKER_PUB_FILE
                    ),
                    format!(
                        "without `#{}`, items are private by default and export with `{}`",
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PUB
                    ),
                    format!(
                        "add `#{}` at the top of the file, or write `{}` instead of `{}`",
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PUB,
                        Syntax::KW_PRIV
                    ),
                    Some(span),
                ));
            }
            return (false, false);
        }
        if matches!(self.peek().kind, TokKind::KwPriv) {
            let span = self.peek().span;
            self.bump();
            if matches!(self.peek().kind, TokKind::KwPub) {
                let span = Span::new(span.start, self.peek().span.end);
                self.bump();
                let _ = self.try_parse_pub_package_suffix();
                self.diags.push(Diagnostic::error(
                    "E0417",
                    format!(
                        "`{}` and `{}` can't both apply to one item",
                        Syntax::KW_PRIV,
                        Syntax::KW_PUB
                    ),
                    "an item is either public or private — pick one qualifier".to_string(),
                    format!(
                        "drop `{}` (already public in a `#{}` file) or remove `{}`",
                        Syntax::KW_PUB,
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PRIV
                    ),
                    Some(span),
                ));
            }
            if !self.pub_file_default {
                self.diags.push(Diagnostic::error(
                    "E0413",
                    format!(
                        "`{}` only applies inside a `#{}` file",
                        Syntax::KW_PRIV,
                        Syntax::MARKER_PUB_FILE
                    ),
                    format!(
                        "without `#{}`, items are private by default and export with `{}`",
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PUB
                    ),
                    format!(
                        "add `#{}` at the top of the file, or write `{}` instead of `{}`",
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PUB,
                        Syntax::KW_PRIV
                    ),
                    Some(span),
                ));
            }
            return (false, false);
        }
        if matches!(self.peek().kind, TokKind::KwPub) {
            let (is_pub, is_package_pub) = self.parse_pub_qualifier();
            if self.pub_file_default && is_pub && !is_package_pub {
                self.diags.push(Diagnostic::error(
                    "E0414",
                    format!(
                        "`{}` is redundant in a `#{}` file",
                        Syntax::KW_PUB,
                        Syntax::MARKER_PUB_FILE
                    ),
                    format!(
                        "after `#{}`, top-level items are already public unless marked `{}`",
                        Syntax::MARKER_PUB_FILE,
                        Syntax::KW_PRIV
                    ),
                    format!(
                        "drop `{}` or mark exceptions with `{}`",
                        Syntax::KW_PUB,
                        Syntax::KW_PRIV
                    ),
                    Some(self.toks[self.pos.saturating_sub(1)].span),
                ));
            }
            return (is_pub, is_package_pub);
        }
        if self.pub_file_default {
            (true, false)
        } else {
            (false, false)
        }
    }

    /// D-PUBPKG1=A: parse an optional `pub` or `pub(package)` qualifier.
    /// Returns `(is_pub, is_package_pub)`. On `pub(other)` pushes E0411 and returns `(true, false)`.
    /// Non-failing: never returns Err.
    fn parse_pub_qualifier(&mut self) -> (bool, bool) {
        if !matches!(self.peek().kind, TokKind::KwPub) {
            return (false, false);
        }
        self.bump(); // consume `pub`
        self.try_parse_pub_package_suffix()
    }

    /// Consume optional `(package)` after `pub` was already eaten.
    fn try_parse_pub_package_suffix(&mut self) -> (bool, bool) {
        if !matches!(self.peek().kind, TokKind::LParen) {
            return (true, false);
        }
        // pub(…)
        self.bump(); // consume `(`
        match self.peek().kind.clone() {
            TokKind::Ident(ref n) if n == Syntax::PUB_PACKAGE_QUALIFIER => {
                self.bump(); // consume `package`
                             // consume `)` — push error if missing but don't abort
                if matches!(self.peek().kind, TokKind::RParen) {
                    self.bump();
                } else {
                    let sp = self.peek().span;
                    self.diags.push(Diagnostic::error(
                        "E0003",
                        "expected `)` to close `pub(package)`".to_string(),
                        "write `pub(package)` with no extra content inside the parentheses".to_string(),
                        "use `pub(package)` to restrict access to sibling packages in the same payload".to_string(),
                        Some(sp),
                    ));
                }
                (true, true)
            }
            _ => {
                // pub(something_else) — reject
                let sp = self.peek().span;
                self.diags.push(Diagnostic::error(
                    "E0411",
                    format!("unknown `pub(…)` qualifier — only `pub(package)` is supported"),
                    "`pub(package)` restricts access to sibling packages in the same payload"
                        .to_string(),
                    "write `pub` (public to all) or `pub(package)` (package-scoped)".to_string(),
                    Some(sp),
                ));
                // skip to `)`
                while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
                    self.bump();
                }
                if matches!(self.peek().kind, TokKind::RParen) {
                    self.bump();
                }
                (true, false)
            }
        }
    }

    /// Parse a function whose purity is already known (the bare-`pure` teaching
    /// path enters here after emitting E0053 and consuming the `pure` word).
    pub(super) fn func_with_purity(&mut self, is_pure: bool) -> Result<Func, Diagnostic> {
        self.func_with_modifiers(is_pure, false)
    }

    /// Parse a function whose `@Pure`/`#Sanitizer` modifiers are already known.
    pub(super) fn func_with_modifiers(
        &mut self,
        is_pure: bool,
        is_sanitizer: bool,
    ) -> Result<Func, Diagnostic> {
        self.func_with_modifiers_full(
            is_pure,
            is_sanitizer,
            None,
            None,
            None,
            false,
            None,
            false,
            false,
            None,
        )
    }

    /// Parse a function whose `@Pure`/`#Sanitizer` and D-STATE1 typestate markers
    /// are already known.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn func_with_modifiers_full(
        &mut self,
        is_pure: bool,
        is_sanitizer: bool,
        state_requires: Option<(String, Span)>,
        state_transition: Option<crate::AST::StateTransition>,
        web_marker: Option<crate::Syntax::WebPartitionMarker>,
        is_must_use: bool,
        must_use_span: Option<Span>,
        is_inline: bool,
        is_inline_always: bool,
        inline_span: Option<Span>,
    ) -> Result<Func, Diagnostic> {
        let (is_pub, is_package_pub) = self.parse_item_visibility();
        self.expect_kw(TokKind::KwFn, "to start a function definition")?;
        self.func_after_fn(
            is_pub,
            is_package_pub,
            false,
            is_pure,
            is_sanitizer,
            state_requires,
            state_transition,
            false,
            web_marker,
            is_must_use,
            must_use_span,
            is_inline,
            is_inline_always,
            inline_span,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn func_after_fn(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
        is_unsafe: bool,
        is_pure: bool,
        is_sanitizer: bool,
        state_requires: Option<(String, Span)>,
        state_transition: Option<crate::AST::StateTransition>,
        is_reactive: bool,
        web_marker: Option<crate::Syntax::WebPartitionMarker>,
        is_must_use: bool,
        must_use_span: Option<Span>,
        is_inline: bool,
        is_inline_always: bool,
        inline_span: Option<Span>,
    ) -> Result<Func, Diagnostic> {
        let (mut name, mut name_span) = self.expect_ident("after `fn`")?;
        let external_type = if (matches!(self.peek().kind, TokKind::Dot)
            || matches!(self.peek().kind, TokKind::TildeTilde))
            && matches!(self.peek2().kind, TokKind::Ident(_))
            && matches!(self.peek3().kind, TokKind::LParen)
        {
            let type_name = name;
            let type_span = name_span;
            if matches!(self.peek().kind, TokKind::TildeTilde) {
                let sep_span = self.peek().span;
                self.diags.push(Diagnostic::error(
                    "E0325",
                    format!(
                        "external methods attach with `{}`, not `{}`",
                        Syntax::EXTERNAL_METHOD_CONNECTOR,
                        Syntax::EXTERNAL_METHOD_CONNECTOR_RETIRED
                    ),
                    "the dot connector matches trait impls and ordinary member access".to_string(),
                    format!("write `fn {}.method(...)`", type_name),
                    Some(sep_span),
                ));
            }
            self.bump(); // `.` or retired `~~`
            let (method_name, method_span) =
                self.expect_ident("after the connector in an external method definition")?;
            name = method_name;
            name_span = method_span;
            Some((type_name, type_span))
        } else {
            None
        };
        let type_params = self.parse_opt_type_params()?;
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

        // D-EFF1 / D-QUAL1: an optional `#(Net, Db)` effect bound, between the
        // parameter list and the return arrow. Effect names are validated in
        // sema, not here. D-EFF2: the same slot also admits a `#(via f)` tight
        // pass-through.
        let (declared_effects, effect_via) = self.parse_opt_func_effects()?;

        let mut return_type = None;
        if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            let (ty, _) = self.return_type()?;
            return_type = Some(ty);
        }

        // Single-expression body: `fn name(...) -> T = expr;`
        // Desugars to `return expr;` so the "path must return" check is satisfied.
        if matches!(self.peek().kind, TokKind::Eq) {
            let start = self.bump().span.start;
            let expr = self.expr_no_struct_lit()?;
            let expr_end = expr.span().end;
            self.expect(TokKind::Semi, "after the single-expression function body")?;
            let end = if self.pos > 0 {
                self.toks[self.pos - 1].span.end
            } else {
                expr_end
            };
            let ret_span = Span::new(start, end);
            let body = vec![crate::AST::Stmt::Return(Some(expr), ret_span)];
            return Ok(Func {
                is_pub,
                is_package_pub,
                external_type,
                name,
                name_span,
                type_params,
                params,
                return_type,
                is_unsafe,
                is_pure,
                is_sanitizer,
                is_reactive,
                declared_effects,
                effect_via,
                state_requires,
                state_transition,
                web_marker,
                is_must_use,
                must_use_span,
                is_inline,
                is_inline_always,
                inline_span,
                pre: Vec::new(),
                post: Vec::new(),
                body,
            });
        }
        self.expect(TokKind::LBrace, "to open the function body")?;
        let body = self.block_stmts();
        Ok(Func {
            is_pub,
            is_package_pub,
            external_type,
            name,
            name_span,
            type_params,
            params,
            return_type,
            is_unsafe,
            is_pure,
            is_sanitizer,
            is_reactive,
            declared_effects,
            effect_via,
            state_requires,
            state_transition,
            web_marker,
            is_must_use,
            must_use_span,
            is_inline,
            is_inline_always,
            inline_span,
            pre: Vec::new(),
            post: Vec::new(),
            body,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn bump_then_func_after_fn(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
        is_unsafe: bool,
        is_pure: bool,
        is_sanitizer: bool,
        state_requires: Option<(String, Span)>,
        state_transition: Option<crate::AST::StateTransition>,
        web_marker: Option<crate::Syntax::WebPartitionMarker>,
        is_must_use: bool,
        must_use_span: Option<Span>,
    ) -> Result<Func, Diagnostic> {
        self.expect_kw(TokKind::KwFn, "to start a function definition")?;
        self.func_after_fn(
            is_pub,
            is_package_pub,
            is_unsafe,
            is_pure,
            is_sanitizer,
            state_requires,
            state_transition,
            false,
            web_marker,
            is_must_use,
            must_use_span,
            false,
            false,
            None,
        )
    }

    /// D-EFF1 / D-QUAL1: parse an optional `#(Net, Db)` effect bound. Returns
    /// `None` when the cursor is not at `#(`. D-EFFTREE1: an entry may be a
    /// dotted effect path (`Fs.Read`); sema validates the root against the
    /// known effect vocabulary.
    fn parse_opt_effect_annotation(&mut self) -> Result<Option<Vec<(String, Span)>>, Diagnostic> {
        // Trait methods (and any caller that can't host a `#(via f)` pass-through)
        // route through here: a `via` clause is parsed and discarded as a list,
        // so it surfaces as an unknown-effect E0119 in sema rather than silently
        // working. The two `Func` sites use `parse_opt_func_effects` instead.
        Ok(self.parse_opt_func_effects()?.0)
    }

    /// D-EFF1 / D-EFF2: parse the `#(…)` signature annotation, which is either a
    /// declared effect bound (`#(Net, Db)`) or a `#(via f)` pass-through. Returns
    /// `(declared_effects, effect_via)` — at most one is `Some`. `None`/`None` when
    /// the cursor is not at `#(`.
    pub(super) fn parse_opt_func_effects(
        &mut self,
    ) -> Result<(Option<Vec<(String, Span)>>, Option<(String, Span)>), Diagnostic> {
        if !(matches!(self.peek().kind, TokKind::Hash)
            && matches!(self.peek2().kind, TokKind::LParen))
        {
            return Ok((None, None));
        }
        self.bump(); // `#`
        self.expect(TokKind::LParen, "after `#` to start an effect list")?;
        // D-EFF2 `#(via f)`: a tight pass-through publishing param `f`'s effects.
        if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_VIA) {
            self.bump(); // `via`
            let (param, span) = self.expect_ident("for the callback parameter name after `via`")?;
            self.expect(TokKind::RParen, "to close the `#(via …)` annotation")?;
            return Ok((None, Some((param, span))));
        }
        let mut effects = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                // D-PROP2=A: `!Effect` is a prohibition — the function (and its
                // whole reachable call graph) must not use that effect.
                let prohibited = matches!(self.peek().kind, TokKind::Bang);
                if prohibited {
                    self.bump(); // consume `!`
                }
                let (name, span) = self.expect_effect_path_name("for an effect name")?;
                let entry = if prohibited {
                    format!("!{}", name)
                } else {
                    name
                };
                effects.push((entry, span));
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between effects in the list")?;
            }
        }
        self.expect(TokKind::RParen, "to close the effect list")?;
        Ok((Some(effects), None))
    }

    fn param(&mut self) -> Result<Param, Diagnostic> {
        let mut convention = self.parse_access_prefix();
        let (name, name_span) = if matches!(self.peek().kind, TokKind::KwSelf) {
            let span = self.bump().span;
            (Syntax::KW_SELF.to_string(), span)
        } else {
            self.expect_ident("for a parameter name")?
        };
        let (ty, ty_span, variadic, variadic_bound_list) = if matches!(
            self.peek().kind,
            TokKind::Colon
        ) {
            self.bump();
            // D-MEM1: the capability sigil rides the type side — `name: &T`/`^T`.
            // (Receivers carry it on `self` instead: `&self`, parsed above.)
            if let Some(type_cap) = self.parse_capability_sigil() {
                // A real pre-name marker (not the unmarked `Read` default) plus a
                // type-side sigil is two markers (E0029).
                if convention != AccessConvention::Read {
                    self.diags.push(Diagnostic::error(
                        "E0029",
                        format!("`{}` has two capability markers", name),
                        "a parameter's access capability is written once — on the type \
                         (`name: &Type`), or on `self` for a receiver"
                            .to_string(),
                        "keep the sigil on the type and remove the other".to_string(),
                        Some(name_span),
                    ));
                }
                convention = type_cap;
            }
            // D-VARIADIC1: `name: ...T` — variadic rest parameter (last position only).
            if matches!(self.peek().kind, TokKind::DotDotDot) {
                self.bump();
                // D-ANY-JAI1/D-VARARGBOUND1: `...[TraitA, TraitB]` — an explicit
                // trait-bound list. `[` never starts a legal *concrete* variadic
                // element type here (list `[T]` and map `[K, V]` types don't make
                // sense as a rest-parameter's own element type spelled this way),
                // so this position always means a bound list. Bare `...Trait` /
                // `...T` both go through `type_()` unchanged — sema tells a
                // trait name from a concrete type the same way `resolve_type_name`
                // already does elsewhere.
                if matches!(self.peek().kind, TokKind::LBracket) {
                    let (bounds, bracket_span) = self.parse_bracket_trait_bound_list()?;
                    (Type::Named(String::new()), bracket_span, true, Some(bounds))
                } else {
                    let (t, ts) = self.type_()?;
                    (t, ts, true, None)
                }
            } else {
                let (t, ts) = self.type_()?;
                (t, ts, false, None)
            }
        } else if name == Syntax::KW_SELF {
            // S27: receiver type is the owning struct/enum; sema fills it in.
            (Type::Named(String::new()), name_span, false, None)
        } else {
            return Err(Diagnostic::error(
                "E0003",
                format!("expected `:` after the parameter `{}`", name),
                "every parameter except `self` needs a type after its name".to_string(),
                format!("write `{}: Type`", name),
                Some(name_span),
            ));
        };
        // S61: optional trailing `= expr` default value.
        let default = if matches!(self.peek().kind, TokKind::Eq) {
            self.bump();
            Some(Box::new(self.expr_no_struct_lit()?))
        } else {
            None
        };
        if variadic && default.is_some() {
            self.diags.push(Diagnostic::error(
                "E1310",
                format!("variadic parameter `{}` can't have a default value", name),
                "a `...` rest parameter collects trailing arguments — it can't also be optional"
                    .to_string(),
                "remove the `= …` default from the variadic parameter".to_string(),
                Some(name_span),
            ));
        }
        Ok(Param {
            convention,
            name,
            name_span,
            ty,
            ty_span,
            default,
            variadic,
            variadic_bound_list,
        })
    }

    /// D-VARIADIC1: a variadic `...` parameter must be the last one in the list.
    pub(super) fn validate_variadic_params(&mut self, params: &[Param]) {
        for (i, p) in params.iter().enumerate() {
            if p.variadic && i + 1 != params.len() {
                self.diags.push(Diagnostic::error(
                    "E1310",
                    format!("variadic parameter `{}` must be last", p.name),
                    "a `name: ...T` rest parameter collects every trailing argument, so nothing may follow it".to_string(),
                    "move `{}` to the end of the parameter list, or remove the `...`".to_string(),
                    Some(p.name_span),
                ));
            }
        }
    }

    pub(super) fn struct_def(&mut self, nested: bool) -> Result<StructDef, Diagnostic> {
        let (is_pub, is_package_pub) = if nested {
            (false, false)
        } else {
            self.parse_item_visibility()
        };
        self.expect_kw(TokKind::KwStruct, "to start a struct definition")?;
        let (name, name_span) = self.parse_dotted_type_name("after `struct`")?;
        let type_params = self.parse_opt_type_params()?;
        self.expect(TokKind::LBrace, "to open the struct body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut trait_impls = Vec::new();
        let mut derives = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            // D-SERDE5: `#[Rename("x")] who: String` — field-level serde markers.
            // D-DEBUG-REDACT/D-MARKERMOVE1: `@[Redact] who: String` — contract
            // plane; stackable with a `#[…]` serde group on the same field.
            if self.at_marker_list() || self.at_contract_marker_list() {
                let field_markers = self.parse_field_markers()?;
                let mut f = self.field()?;
                let mut redact = false;
                let mut serde_markers = Vec::new();
                for m in field_markers {
                    if m.name == crate::Syntax::ATTR_REDACT {
                        redact = true;
                    } else {
                        serde_markers.push(m);
                    }
                }
                f.serde_markers = serde_markers;
                f.redact = redact;
                fields.push(f);
                if matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                    self.bump();
                }
                continue;
            }
            if matches!(self.peek().kind, TokKind::KwDerive) {
                derives.push(self.derive_line()?);
            } else if matches!(self.peek().kind, TokKind::KwImpl) {
                trait_impls.push(self.trait_impl_block()?);
            } else {
                let is_method = matches!(self.peek().kind, TokKind::KwFn)
                    || (matches!(self.peek().kind, TokKind::KwPub)
                        && matches!(self.peek2().kind, TokKind::KwFn))
                    || self.at_pure_fn()
                    || self.at_sanitizer_fn()
                    || self.at_inline_fn()
                    || self.at_state_fn()
                    || self.at_transition_fn();
                if is_method {
                    methods.push(self.method_in_type()?);
                } else {
                    fields.push(self.field()?);
                    if matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                        self.bump();
                    }
                }
            }
        }
        self.bump(); // }
        Ok(StructDef {
            is_pub,
            is_package_pub,
            name,
            name_span,
            type_params,
            fields,
            methods,
            trait_impls,
            derives,
            is_published_schema: false,
            published_schema_span: None,
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            layout: None,
            layout_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
        })
    }

    pub(super) fn enum_def(&mut self, nested: bool) -> Result<EnumDef, Diagnostic> {
        let (is_pub, is_package_pub) = if nested {
            (false, false)
        } else {
            self.parse_item_visibility()
        };
        self.enum_def_after_pub(is_pub, is_package_pub)
    }

    /// Parse `enum Name { … }` given that pub/is_pub was already handled. Factors
    /// out the body of `enum_def` (mirrors `struct_def_after_pub`) so the
    /// `#SingleUse enum` path can reuse it.
    fn enum_def_after_pub(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<EnumDef, Diagnostic> {
        self.expect_kw(TokKind::KwEnum, "to start an enum definition")?;
        let (name, name_span) = self.expect_ident("after `enum`")?;
        let type_params = self.parse_opt_type_params()?;
        self.expect(TokKind::LBrace, "to open the enum body")?;
        let mut variants = Vec::new();
        let mut groups = Vec::new();
        let mut methods = Vec::new();
        let mut trait_impls = Vec::new();
        let mut derives = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            // D-SERDE5/7: `#[Rename("x")]` on a variant — variant-level serde markers.
            if self.at_marker_list() {
                let variant_markers = self.parse_marker_groups()?;
                self.variant_entry("", &mut variants, &mut groups, variant_markers)?;
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                continue;
            }
            if matches!(self.peek().kind, TokKind::KwDerive) {
                derives.push(self.derive_line()?);
            } else if matches!(self.peek().kind, TokKind::KwImpl) {
                trait_impls.push(self.trait_impl_block()?);
            } else if matches!(self.peek().kind, TokKind::KwFn | TokKind::KwPub) {
                methods.push(self.method_in_type()?);
            } else {
                self.variant_entry("", &mut variants, &mut groups, Vec::new())?;
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
            }
        }
        self.bump();
        Ok(EnumDef {
            is_pub,
            is_package_pub,
            name,
            name_span,
            type_params,
            variants,
            methods,
            trait_impls,
            derives,
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
            groups,
        })
    }

    /// D-TAG1 (ratified 2026-07-03): parse one enum-body entry — a leaf variant
    /// or a variant group `Name { … }`. Leaves are recorded flat with their full
    /// dotted path (`prefix.Name`); each group path is recorded in `groups` so
    /// sema and the formatter see the tree. Groups nest to any depth. Payloads
    /// live on leaves only (E0331).
    fn variant_entry(
        &mut self,
        prefix: &str,
        variants: &mut Vec<crate::AST::Variant>,
        groups: &mut Vec<crate::AST::EnumGroup>,
        serde_markers: Vec<crate::AST::Marker>,
    ) -> Result<(), Diagnostic> {
        let (name, name_span) = self.expect_ident("for a variant name")?;
        let path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}.{name}")
        };
        let payload = if matches!(self.peek().kind, TokKind::LParen) {
            let payload_start = self.bump().span; // consume `(`
            let payload = self.variant_payload()?;
            self.expect(TokKind::RParen, "after a variant's payload")?;
            let payload_end = self.toks[self.pos.saturating_sub(1)].span.end;
            if matches!(self.peek().kind, TokKind::LBrace) {
                // D-TAG1: a payload on a group name — payloads live on leaves only.
                self.diags.push(Diagnostic::error(
                    "E0331",
                    format!("group `{}` can't carry a payload", path),
                    "a variant group only names its subtree — data lives on the leaf variants (D-TAG1)".to_string(),
                    "move the payload onto the leaf variants inside the `{ }`, or remove the `(...)`".to_string(),
                    Some(Span::new(payload_start.start, payload_end)),
                ));
                VariantPayload::Unit
            } else {
                payload
            }
        } else {
            VariantPayload::Unit
        };
        if !matches!(self.peek().kind, TokKind::LBrace) {
            variants.push(Variant {
                name: path,
                name_span,
                payload,
                serde_markers,
            });
            return Ok(());
        }
        // A variant group: `Name { entry (,|newline entry)* }`.
        self.bump(); // consume `{`
        if !serde_markers.is_empty() {
            self.diags.push(Diagnostic::error(
                "E0003",
                format!("a `#[…]` marker can't sit on group `{}`", path),
                "serde markers rename wire names, and only leaf variants reach the wire (D-TAG1)"
                    .to_string(),
                "move the marker onto a leaf variant inside the `{ }`".to_string(),
                Some(name_span),
            ));
        }
        let leaves_before = variants.len();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi | TokKind::Comma) {
                self.bump();
                continue;
            }
            let entry_markers = if self.at_marker_list() {
                self.parse_marker_groups()?
            } else {
                Vec::new()
            };
            self.variant_entry(&path, variants, groups, entry_markers)?;
            if matches!(self.peek().kind, TokKind::Semi | TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the variant group")?;
        if variants.len() == leaves_before {
            self.diags.push(Diagnostic::error(
                "E0003",
                format!("group `{}` has no variants", path),
                "an empty group can never match anything — every group needs at least one leaf variant (D-TAG1)".to_string(),
                "add a variant inside the `{ }`, or remove the group".to_string(),
                Some(name_span),
            ));
        }
        groups.push(crate::AST::EnumGroup { path, name_span });
        Ok(())
    }

    fn variant_payload(&mut self) -> Result<VariantPayload, Diagnostic> {
        if matches!(self.peek().kind, TokKind::Ident(_)) {
            let peek2 = self.peek2().kind.clone();
            if matches!(peek2, TokKind::Colon) {
                let mut fields = Vec::new();
                loop {
                    let (name, name_span) = self.expect_ident("for a variant field name")?;
                    self.expect(TokKind::Colon, "after a variant field name")?;
                    let (ty, ty_span) = self.type_()?;
                    fields.push(VariantField {
                        name,
                        name_span,
                        ty,
                        ty_span,
                    });
                    if !matches!(self.peek().kind, TokKind::Comma) {
                        break;
                    }
                    self.bump();
                }
                Ok(VariantPayload::Named(fields))
            } else {
                let (ty, ty_span) = self.type_()?;
                Ok(VariantPayload::Single(ty, ty_span))
            }
        } else {
            let (ty, ty_span) = self.type_()?;
            Ok(VariantPayload::Single(ty, ty_span))
        }
    }

    /// D-ERR-CONV (ratified 2026-06-19): dispatch `impl …` to either the normal
    /// `ImplDef` path or the `impl Source -> Target { body }` error-conversion path.
    pub(super) fn impl_or_error_conv(&mut self) -> Result<Item, Diagnostic> {
        self.expect_kw(TokKind::KwImpl, "to start an `impl` block")?;
        // D-IMPLDOT1=A: trait impl is `impl Type.Trait { … }`. D-PROTO1/D-PROTO2 add
        // inherent impl on protocol handles `impl Payment.Client { … }` / `.Server`.
        let (type_name, type_span, mut trait_name, mut trait_span) = {
            let (first, span) = self.expect_ident("after `impl`")?;
            if matches!(self.peek().kind, TokKind::Dot) {
                self.bump();
                let (second, second_span) = self.expect_ident("after `.` in `impl`")?;
                if matches!(self.peek().kind, TokKind::LBrace)
                    && (second == "Client" || second == "Server")
                {
                    (format!("{first}.{second}"), span, None, None)
                } else {
                    (first, span, Some(second), Some(second_span))
                }
            } else {
                (first, span, None, None)
            }
        };
        // Detect `impl Source -> Target { body }` — D-ERR-CONV.
        if matches!(self.peek().kind, TokKind::Arrow) {
            let _arrow = self.bump(); // consume `->`
            let (to_ty, to_span) = self.parse_type_path("after `->` in error conversion")?;
            // Peek the `{` span before consuming.
            if !matches!(self.peek().kind, TokKind::LBrace) {
                return Err(Diagnostic::error(
                    "E0003",
                    "expected `{` to open the error-conversion body".to_string(),
                    "an error conversion body is a block: `impl Source -> Target { … }`"
                        .to_string(),
                    "add `{` after the target type".to_string(),
                    Some(self.peek().span),
                ));
            }
            let brace_start = self.bump().span.start; // consume `{`
                                                      // block_stmts consumes statements AND the closing `}`.
                                                      // We need to track where the `}` ended — peek first to record pos.
            let body = self.block_stmts();
            // After block_stmts, the `}` is consumed; the last consumed token is at pos-1.
            let rbrace_end = self.toks[self.pos.saturating_sub(1)].span.end;
            let body_span = Span::new(brace_start, rbrace_end);
            return Ok(Item::ErrorConv(crate::AST::ErrorConvDef {
                from_ty: type_name,
                from_span: type_span,
                to_ty,
                to_span,
                body,
                body_span,
            }));
        }
        // Normal `impl` path — trait_name/trait_span were parsed above.
        if trait_name.is_none() && matches!(self.peek().kind, TokKind::Colon) {
            // Teaching error: old `impl Type: Trait` form.
            let colon_span = self.peek().span;
            self.diags.push(Diagnostic::error(
                "E0321",
                format!("trait separator is now `.`, not `:`"),
                "the impl separator reads \"Type's Trait\", matching the dot accessor".to_string(),
                format!("write `impl {}.Trait {{ … }}`", type_name),
                Some(colon_span),
            ));
            self.bump(); // consume old `:`
            let (t, ts) = self.expect_ident("after `:` in `impl Type: Trait`")?;
            trait_name = Some(t);
            trait_span = Some(ts);
        }
        // S62: `impl Type.Trait using field_name;` — delegation form.
        if let TokKind::Ident(ref kw) = self.peek().kind.clone() {
            if kw == "using" && trait_name.is_some() {
                self.bump(); // consume `using`
                let (field, _) = self.expect_ident("after `using` for the delegation field")?;
                self.finish_stmt()?;
                return Ok(Item::Impl(ImplDef {
                    type_name,
                    type_span,
                    trait_name,
                    trait_span,
                    methods: Vec::new(),
                    delegation_field: Some(field),
                    assoc_type_impls: Vec::new(),
                    os_target: None,
                }));
            }
        }
        self.expect(TokKind::LBrace, "to open the `impl` body")?;
        let mut methods = Vec::new();
        let mut assoc_type_impls = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            if let TokKind::Ident(ref kw) = self.peek().kind.clone() {
                if kw == "type" {
                    let kw_span = self.bump().span;
                    let (assoc_name, name_span) = self.expect_ident("after `type` in impl body")?;
                    self.expect(TokKind::Eq, "after associated type name")?;
                    let (assoc_ty, _) = self.type_()?;
                    self.finish_stmt()?;
                    assoc_type_impls.push((
                        assoc_name,
                        Span::new(kw_span.start, name_span.end),
                        assoc_ty,
                    ));
                    continue;
                }
            }
            methods.push(self.method_in_type()?);
        }
        self.bump();
        Ok(Item::Impl(ImplDef {
            type_name,
            type_span,
            trait_name,
            trait_span,
            methods,
            delegation_field: None,
            assoc_type_impls,
            os_target: None,
        }))
    }

    /// D-OSTARGET1=A: parse the `impl` block that must follow a `#Target(Os.X)`
    /// marker and attach `os` to it. Reuses `impl_or_error_conv` (same grammar,
    /// same `impl Type.Trait { … }` / delegation / error-conversion forms) and
    /// stamps the OS gate onto the resulting `ImplDef` afterward — no need to
    /// thread a new parameter through every `impl` parse path or its other
    /// caller (`Modules.rs`'s inline-module item loop calls this same helper).
    pub(super) fn os_gated_impl(&mut self, os: crate::Syntax::OsTarget) -> Result<Item, Diagnostic> {
        // S6-R: a synthetic statement-terminator `;` may follow the marker
        // line (same as the `TokKind::Semi` skip at the top of the top-level
        // item loop) — swallow it before checking what actually follows.
        while matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        if !matches!(self.peek().kind, TokKind::KwImpl) {
            let span = self.peek().span;
            return Err(Diagnostic::error(
                "E0003",
                format!("`#Target(Os.{})` isn't valid here", os.name()),
                "`Os.Linux`/`Os.Macos`/`Os.Windows` gates a whole `impl` block, not a module, function, or any other item".to_string(),
                format!("write `#Target(Os.{}) impl Type.Trait {{ … }}`", os.name()),
                Some(span),
            ));
        }
        match self.impl_or_error_conv()? {
            Item::Impl(mut i) => {
                i.os_target = Some(os);
                Ok(Item::Impl(i))
            }
            Item::ErrorConv(ec) => Err(Diagnostic::error(
                "E0003",
                format!("`#Target(Os.{})` isn't valid on an error-conversion `impl`", os.name()),
                "`impl Source -> Target { … }` error conversions run on every platform; OS gating only makes sense for a real trait/inherent impl".to_string(),
                format!("remove the `#Target(Os.{})` marker", os.name()),
                Some(ec.from_span),
            )),
            other => Ok(other),
        }
    }

    /// S28: `impl Trait { … }` inside a struct/enum body.
    fn trait_impl_block(&mut self) -> Result<TraitImplBlock, Diagnostic> {
        self.expect_kw(TokKind::KwImpl, "to start a trait impl block")?;
        let (trait_name, trait_span) = self.expect_ident("after `impl`")?;
        self.expect(TokKind::LBrace, "to open the trait impl body")?;
        let mut methods = Vec::new();
        let mut assoc_type_impls = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            // D-LIB2: `type Name = ConcreteType;`
            if let TokKind::Ident(ref kw) = self.peek().kind.clone() {
                if kw == "type" {
                    let kw_span = self.bump().span;
                    let (assoc_name, name_span) = self.expect_ident("after `type` in impl body")?;
                    self.expect(TokKind::Eq, "after associated type name")?;
                    let (assoc_ty, _) = self.type_()?;
                    self.finish_stmt()?;
                    assoc_type_impls.push((
                        assoc_name,
                        Span::new(kw_span.start, name_span.end),
                        assoc_ty,
                    ));
                    continue;
                }
            }
            methods.push(self.method_in_type()?);
        }
        self.bump();
        Ok(TraitImplBlock {
            trait_name,
            trait_span,
            methods,
            assoc_type_impls,
        })
    }

    /// S55: `derive Comparable;` inside a type body.
    fn derive_line(&mut self) -> Result<(String, Span), Diagnostic> {
        let start = self.bump().span;
        let (trait_name, _) = self.expect_ident("after `derive`")?;
        self.finish_stmt()?;
        Ok((trait_name, start))
    }

    /// True when the cursor is at a `#[ … ]` bracket-marker group (D-ATTR2).
    pub(super) fn at_marker_list(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash) && matches!(self.peek2().kind, TokKind::LBracket)
    }

    /// D-MARKER-FAMILY1/G2: true at a `@[ … ]` contract-derive bracket group —
    /// the `@` sibling of `at_marker_list`.
    pub(super) fn at_contract_marker_list(&self) -> bool {
        matches!(self.peek().kind, TokKind::At) && matches!(self.peek2().kind, TokKind::LBracket)
    }

    /// D-ATTR1/D-MARKER-CANON1: a PascalCase `#Marker` immediately before `struct`/`enum`.
    pub(super) fn at_single_type_marker(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::Hash) {
            return false;
        }
        let TokKind::Ident(name) = &self.peek2().kind else {
            return false;
        };
        if !Self::is_pascal_type_marker_name(name) || Self::is_reserved_hash_item_prefix(name) {
            return false;
        }
        self.type_marker_prefix_leads_to_type_def(self.pos + 2)
    }

    /// D-MARKER-FAMILY1/G3: a PascalCase `@Marker` immediately before
    /// `struct`/`enum` — the `@` sibling of `at_single_type_marker` (contract
    /// derives: Codable, Encode, Decode, Debug, Summarize, Comparable).
    pub(super) fn at_single_contract_type_marker(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::At) {
            return false;
        }
        let TokKind::Ident(name) = &self.peek2().kind else {
            return false;
        };
        if !Self::is_pascal_type_marker_name(name) || Self::is_reserved_at_item_prefix(name) {
            return false;
        }
        self.type_marker_prefix_leads_to_type_def(self.pos + 2)
    }

    /// `@` markers that own their own item parser (MustUse/PublishedSchema
    /// struct dispatch, Numeric distinct-type dispatch) — not bare
    /// contract-derive prefixes routed through the generic marker-list
    /// mechanism.
    fn is_reserved_at_item_prefix(name: &str) -> bool {
        matches!(
            name,
            Syntax::ATTR_MUST_USE | Syntax::ATTR_PUBLISHED_SCHEMA | Syntax::ATTR_NUMERIC
        )
    }

    fn is_pascal_type_marker_name(name: &str) -> bool {
        name.chars().next().is_some_and(|c| c.is_uppercase())
    }

    /// `#` markers that own their own item parser — not bare type derive prefixes.
    fn is_reserved_hash_item_prefix(name: &str) -> bool {
        matches!(
            name,
            Syntax::KW_TEST
                | Syntax::KW_BENCH
                | Syntax::KW_UNSAFE
                | Syntax::KW_REACTIVE
                | Syntax::KW_PURE
                | Syntax::KW_SANITIZER
                | Syntax::KW_STATE
                | Syntax::KW_TRANSITION
                | Syntax::MARKER_PUB_FILE
                | Syntax::ATTR_UNIT_FAMILY
                | Syntax::ATTR_NUMERIC
                | Syntax::ATTR_LAYOUT
                | Syntax::ATTR_PUBLISHED_SCHEMA
                | Syntax::ATTR_SINGLE_USE
                | Syntax::ATTR_MUST_USE
                | Syntax::ATTR_EXTERN_MODULE
                | Syntax::ATTR_BINDGEN
                | Syntax::ATTR_TARGET
                | Syntax::ATTR_WASM
                | Syntax::ATTR_JS
                | Syntax::ATTR_WASM_EXPORT
        )
    }

    /// After a `#Marker` (and optional `(args)`), does the next real token start a type?
    fn type_marker_prefix_leads_to_type_def(&self, mut i: usize) -> bool {
        if i >= self.toks.len() {
            return false;
        }
        // Skip this marker's own optional `(args)`.
        i = match Self::skip_balanced_parens(&self.toks, i) {
            Some(n) => n,
            None => return false,
        };
        // D-MARKER-FAMILY1/G2: a type declaration may carry several stacked
        // marker prefixes (`@[Codable] #[RenameAll(camel)] struct …`, or
        // `@MustUse @[Codable] struct …`) — keep skipping marker-shaped
        // prefixes (bracket groups or lone `#Name`/`@Name(args)?`) until none
        // remain, then require `pub`? `struct`/`enum`.
        loop {
            while i < self.toks.len() && matches!(self.toks[i].kind, TokKind::Semi) {
                i += 1;
            }
            if i >= self.toks.len() {
                return false;
            }
            let is_marker_sigil = matches!(self.toks[i].kind, TokKind::Hash | TokKind::At);
            if !is_marker_sigil {
                break;
            }
            match self.toks.get(i + 1).map(|t| &t.kind) {
                Some(TokKind::LBracket) => {
                    i = match Self::skip_bracket_group(&self.toks, i + 1) {
                        Some(n) => n,
                        None => return false,
                    };
                }
                Some(TokKind::Ident(name)) if Self::is_pascal_type_marker_name(name) => {
                    i += 2; // `#`/`@` and the ident
                    i = match Self::skip_balanced_parens(&self.toks, i) {
                        Some(n) => n,
                        None => return false,
                    };
                }
                _ => break,
            }
        }
        if i < self.toks.len() && matches!(self.toks[i].kind, TokKind::KwPub) {
            i += 1;
        }
        i < self.toks.len() && matches!(self.toks[i].kind, TokKind::KwStruct | TokKind::KwEnum)
    }

    /// If `toks[i]` is `(`, returns the index just past its matching `)`;
    /// otherwise returns `i` unchanged. `None` on an unbalanced/unterminated
    /// paren group.
    fn skip_balanced_parens(toks: &[Token], mut i: usize) -> Option<usize> {
        if i >= toks.len() || !matches!(toks[i].kind, TokKind::LParen) {
            return Some(i);
        }
        let mut depth = 0usize;
        loop {
            if i >= toks.len() {
                return None;
            }
            match toks[i].kind {
                TokKind::LParen => depth += 1,
                TokKind::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// `toks[i]` is the `[` opening a `#[…]`/`@[…]` marker-bracket group;
    /// returns the index just past its matching `]`, or `None` if
    /// unterminated.
    fn skip_bracket_group(toks: &[Token], mut i: usize) -> Option<usize> {
        debug_assert!(matches!(toks[i].kind, TokKind::LBracket));
        let mut depth = 0usize;
        loop {
            if i >= toks.len() {
                return None;
            }
            match toks[i].kind {
                TokKind::LBracket => depth += 1,
                TokKind::RBracket => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// Parse one marker name and optional `(args)`; cursor sits on the name.
    fn parse_one_marker(&mut self) -> Result<Marker, Diagnostic> {
        let (name, name_span) = self.expect_ident("for a marker name")?;
        let mut args = Vec::new();
        let mut end = name_span.end;
        if matches!(self.peek().kind, TokKind::LParen) {
            self.bump(); // `(`
            while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
                args.push(self.expr_no_struct_lit()?);
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            end = self.peek().span.end;
            self.expect(TokKind::RParen, "to close marker arguments")?;
        }
        Ok(Marker {
            name,
            name_span,
            args,
            span: Span::new(name_span.start, end),
        })
    }

    /// D-ATTR2: parse one `#[ Name (, …)* ]` group; cursor on `#`.
    fn parse_marker_bracket_group(&mut self) -> Result<Vec<Marker>, Diagnostic> {
        self.bump(); // `#`
        self.bump(); // `[`
        let mut group = Vec::new();
        loop {
            let m = self.parse_one_marker()?;
            self.check_marker_plane(&m, false);
            group.push(m);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokKind::RBracket, "to close a `#[…]` marker list")?;
        while matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        Ok(group)
    }

    /// D-MARKER-FAMILY1/G2: parse one `@[ Name (, …)* ]` contract-derive
    /// group; cursor on `@`. The `@` sibling of `parse_marker_bracket_group`.
    fn parse_contract_marker_bracket_group(&mut self) -> Result<Vec<Marker>, Diagnostic> {
        self.bump(); // `@`
        self.bump(); // `[`
        let mut group = Vec::new();
        loop {
            let m = self.parse_one_marker()?;
            self.check_marker_plane(&m, true);
            group.push(m);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokKind::RBracket, "to close a `@[…]` marker list")?;
        while matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        Ok(group)
    }

    /// D-MARKER-FAMILY1 (I7/R3 chokepoint): after parsing a marker name in a
    /// bracket/single-marker group, check it landed on the plane its sigil
    /// implies. `on_at` is true when the enclosing group opened with `@`. A
    /// moved contract marker (§2a/§2b/G3) written after `#` is E0062; a
    /// directive name written after `@` is E0063. Any other name (a user
    /// derive on `#`, or an unrecognized `@` name) is left for downstream —
    /// derive resolution, or the generic `@` teaching error — to judge.
    fn check_marker_plane(&mut self, marker: &Marker, on_at: bool) {
        if on_at {
            if Syntax::is_directive_marker(&marker.name) {
                self.diags
                    .push(Self::e0063_directive_on_at(&marker.name, marker.name_span));
            }
        } else if Syntax::is_contract_marker(&marker.name) {
            self.diags
                .push(Self::e0062_contract_on_hash(&marker.name, marker.name_span));
        }
    }

    /// E0062: a contract marker (moved to `@` by D-MARKERMOVE1/2/3) was
    /// written with `#`. `name` is the marker's bare name (no sigil).
    pub(super) fn e0062_contract_on_hash(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            "E0062",
            format!("`#{name}` states a contract — write it with `@`, not `#`"),
            "`@` marks a promise about the declaration below it (`@Pure`, `@MustUse`, \
             `@Codable`); `#` is for compiler directives (`#Unsafe`, `#Test`). One glance \
             at the first character tells a reader which it is (D-MARKER-FAMILY1)."
                .to_string(),
            format!("write `@{name}` (`@` + the same PascalCase name)."),
            Some(span),
        )
    }

    /// E0063: a directive marker (stays on `#`) was written with `@`.
    pub(super) fn e0063_directive_on_at(name: &str, span: Span) -> Diagnostic {
        Diagnostic::error(
            "E0063",
            format!("`@{name}` is a compiler directive — write it with `#`, not `@`"),
            "`#` changes what compiles or runs (`#Test`, `#Unsafe`, `#Caps`); `@` is \
             reserved for contracts stated on the declaration below (D-MARKER-FAMILY1)."
                .to_string(),
            format!("write `#{name}` instead of `@{name}`."),
            Some(span),
        )
    }

    /// D-ATTR2 / D-SERDE2–8: parse `#[ … ]` bracket-marker groups. A second
    /// consecutive `#[ … ]` line is teaching error E0999 (merge into one list).
    fn parse_marker_groups(&mut self) -> Result<Vec<Marker>, Diagnostic> {
        let mut out = Vec::new();
        let mut groups = 0usize;
        while self.at_marker_list() {
            groups += 1;
            if groups > 1 {
                self.diags.push(Diagnostic::error(
                    "E0999",
                    "multiple `#[…]` marker lines belong in one comma-separated list".to_string(),
                    "Jet attaches every marker on a type in a single `#[A, B]` group (D-ATTR2); one marker alone is `#A`".to_string(),
                    "merge them: `#[RenameAll(camel), Skip]`, or use `#RenameAll(camel)` when there is only one".to_string(),
                    Some(self.peek().span),
                ));
            }
            out.extend(self.parse_marker_bracket_group()?);
        }
        Ok(out)
    }

    /// D-MARKER-FAMILY1/G2: parse `@[ … ]` contract-marker bracket groups. A
    /// second consecutive `@[ … ]` line is E0999 (same rule, same plane only
    /// — a `@[…]` group and a `#[…]` group may stack on one declaration, so
    /// this never fires across planes).
    fn parse_contract_marker_groups(&mut self) -> Result<Vec<Marker>, Diagnostic> {
        let mut out = Vec::new();
        let mut groups = 0usize;
        while self.at_contract_marker_list() {
            groups += 1;
            if groups > 1 {
                self.diags.push(Diagnostic::error(
                    "E0999",
                    "multiple `@[…]` marker lines belong in one comma-separated list".to_string(),
                    "Jet attaches every contract marker on a declaration in a single `@[A, B]` group (D-MARKER-FAMILY1); one marker alone is `@A`".to_string(),
                    "merge them: `@[Codable, Debug]`, or use `@Codable` when there is only one".to_string(),
                    Some(self.peek().span),
                ));
            }
            out.extend(self.parse_contract_marker_bracket_group()?);
        }
        Ok(out)
    }

    /// D-ATTR1: parse a lone `#Marker` (or `#Marker(args)`) before `struct`/`enum`.
    fn parse_single_type_prefix_marker(&mut self) -> Result<Marker, Diagnostic> {
        self.bump(); // `#`
        let m = self.parse_one_marker()?;
        self.check_marker_plane(&m, false);
        Ok(m)
    }

    /// D-MARKER-FAMILY1/G3: parse a lone `@Marker` (or `@Marker(args)`) before
    /// `struct`/`enum` — the `@` sibling of `parse_single_type_prefix_marker`.
    fn parse_single_contract_type_marker(&mut self) -> Result<Marker, Diagnostic> {
        self.bump(); // `@`
        let m = self.parse_one_marker()?;
        self.check_marker_plane(&m, true);
        Ok(m)
    }

    /// D-MARKER-FAMILY1/G2: parse leading `@[…]` contract-marker groups
    /// and/or `#[…]` directive+serde-marker groups before a struct/enum
    /// field, both optional and stackable (e.g. `@[Redact] #[Rename("x")]`).
    /// Used at field position, which only ever supports the bracket form
    /// (no bare `@Redact`/`#Rename` without brackets).
    fn parse_field_markers(&mut self) -> Result<Vec<Marker>, Diagnostic> {
        let mut out = Vec::new();
        loop {
            if self.at_contract_marker_list() {
                out.extend(self.parse_contract_marker_groups()?);
            } else if self.at_marker_list() {
                out.extend(self.parse_marker_groups()?);
            } else {
                break;
            }
        }
        Ok(out)
    }

    /// Split parsed markers on a struct/enum: derive-trait markers
    /// (`Codable`→`Encode`+`Decode`, `Encode`, `Decode`, `Debug`, `Summarize`,
    /// `Comparable`, user traits) are pushed onto `derives`; serde *attribute*
    /// markers are returned raw for sema. Markers arrive already validated for
    /// plane (E0062/E0063 pushed by the caller if misplaced, D-MARKER-FAMILY1)
    /// — this only classifies what job each name does, independent of which
    /// sigil it came from.
    fn split_type_markers(markers: Vec<Marker>, derives: &mut Vec<(String, Span)>) -> Vec<Marker> {
        let mut serde = Vec::new();
        for m in markers {
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
                // (`@[Debug]`, `@[Summarize]`, `@[Comparable]`) or a `#[…]` user
                // derive-trait name.
                _ => derives.push((m.name.clone(), m.name_span)),
            }
        }
        serde
    }

    /// Attach parsed type markers to a freshly parsed struct/enum item.
    fn attach_type_markers(markers: Vec<Marker>, item: Item) -> Item {
        match item {
            Item::Struct(mut s) => {
                // D-MIGRATE1 (I2/E0910 fix): `PublishedSchema` appearing inside an
                // item-level `@[…]` bracket LIST (e.g. `@[PublishedSchema, Codable]
                // struct …`) previously only got recorded in `type_markers` — the
                // dedicated `is_published_schema`/`published_schema_span` fields
                // (which `SchemaMigration.rs`'s E0910 check guards on) were only ever
                // set by the single-prefix `@PublishedSchema struct …` form
                // (`published_schema_struct_def`). A schema published this way
                // silently skipped E0910 migration validation. Mirror that form here.
                if let Some(m) = markers.iter().find(|m| m.name == Syntax::ATTR_PUBLISHED_SCHEMA) {
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
            other => other,
        }
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
            TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
            TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
            TokKind::Hash
                if matches!(
                    &self.peek2().kind,
                    TokKind::Ident(n) if n == Syntax::ATTR_LAYOUT
                ) =>
            {
                self.layout_struct_def(is_pub).map(Item::Struct)
            }
            TokKind::Hash | TokKind::At
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
                "derive markers like `@Codable` / `@[Codable]` and serde attributes attach to a type"
                    .to_string(),
                "write `@Codable struct Name { … }` or `@[Codable] #[RenameAll(camel)] struct …`"
                    .to_string(),
                Some(self.peek().span),
            )),
        }
    }

    /// D-MARKER-FAMILY1/G2: parse leading `@[…]`/`@Name` contract markers
    /// and/or `#[…]`/`#Name` directive+serde markers, in either order, both
    /// optional, both stackable on one declaration (E0999 only fires for two
    /// groups on the SAME plane — a `@[…]` group and a `#[…]` group may
    /// stack). Then parses the struct/enum they attach to. Single dispatch
    /// entry point for both sigils at type-marker position (Items.rs top
    /// match, Modules.rs inline-module body).
    pub(super) fn type_def_with_any_markers(&mut self) -> Result<Item, Diagnostic> {
        let mut markers = Vec::new();
        loop {
            // A marker line may end with an auto-inserted/explicit `;` before
            // the next stacked marker line or the `struct`/`enum` (G2 — both
            // `@[…]` and `#[…]` groups, or lone forms, may stack).
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            if self.at_contract_marker_list() {
                markers.extend(self.parse_contract_marker_groups()?);
            } else if self.at_single_contract_type_marker() {
                markers.push(self.parse_single_contract_type_marker()?);
            } else if self.at_marker_list() {
                markers.extend(self.parse_marker_groups()?);
            } else if self.at_single_type_marker() {
                markers.push(self.parse_single_type_prefix_marker()?);
            } else {
                break;
            }
        }
        let item = self.parse_type_after_markers()?;
        Ok(Self::attach_type_markers(markers, item))
    }

    /// S28: top-level `trait Name { fn sig(self) -> T; … }`.
    pub(super) fn trait_def(&mut self, nested: bool) -> Result<TraitDef, Diagnostic> {
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
            // D-EFF3 / D-MARKERMOVE2: a trait method may carry a `@Pure` prefix
            // declaring the empty effect set as its upper bound.
            let is_pure = if self.at_pure_fn() {
                self.bump_pure_marker();
                true
            } else {
                false
            };
            methods.push(self.trait_method_sig(is_pure)?);
        }
        self.bump();
        Ok(TraitDef {
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
    pub(super) fn tag_def(&mut self, nested: bool) -> Result<TagDef, Diagnostic> {
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

    fn trait_method_sig(&mut self, is_pure: bool) -> Result<TraitMethodSig, Diagnostic> {
        let start = self.peek().span;
        self.expect_kw(TokKind::KwFn, "to start a trait method signature")?;
        let (name, name_span) = self.expect_ident("after `fn`")?;
        self.expect(TokKind::LParen, "after the method name")?;
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
        // D-EFF3: optional `#(Gpu)` effect bound between params and the arrow.
        let declared_effects = self.parse_opt_effect_annotation()?;
        let mut return_type = None;
        if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            let (ty, _) = self.return_type()?;
            return_type = Some(ty);
        }
        // D-LIB2: optional default body `{ … }` instead of `;`.
        let default_body = if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            let stmts = self.block_stmts();
            Some(stmts)
        } else {
            let end = self.peek().span.end;
            self.finish_stmt()?;
            let _ = end;
            None
        };
        let end = self.peek().span.end;
        Ok(TraitMethodSig {
            name,
            name_span,
            params,
            return_type,
            span: Span::new(start.start, end),
            default_body,
            is_pure,
            declared_effects,
        })
    }

    /// S27: method inside a type body or `impl` block.
    fn method_in_type(&mut self) -> Result<Func, Diagnostic> {
        let (is_must_use, must_use_span) = if self.at_must_use_fn() {
            (true, Some(self.bump_must_use_marker()?))
        } else {
            (false, None)
        };
        // S60 (D-CASING1 follow-on) / D-MARKERMOVE2: allow `@Pure fn` on methods
        // too; the marker precedes `pub`.
        let is_pure = if self.at_pure_fn() {
            self.bump_pure_marker();
            true
        } else if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::FOREIGN_PURE)
            && self.foreign_pure_follows()
        {
            let t = self.bump();
            self.diags.push(self.foreign_pure_diag(t.span));
            true
        } else {
            false
        };
        // D-TAINT1: `#Sanitizer fn` is valid on methods too.
        let is_sanitizer = if self.at_sanitizer_fn() {
            self.bump(); // `#`
            self.bump(); // `Sanitizer`
            true
        } else {
            false
        };
        // D-METHODMACRO1=A: `@Inline`/`@InlineAlways` on methods too.
        let (is_inline, is_inline_always, inline_span) = self.parse_inline_marker()?;
        // D-STATE1: `#State(S)` / `#Transition(From -> To)` typestate markers on
        // methods — the common case (typestate methods carry `self`).
        let mut state_requires = None;
        let mut state_transition = None;
        loop {
            if state_requires.is_none() && self.at_state_fn() {
                state_requires = Some(self.parse_state_require_marker()?);
            } else if state_transition.is_none() && self.at_transition_fn() {
                state_transition = Some(self.parse_transition_marker()?);
            } else {
                break;
            }
        }
        let (is_pub, is_package_pub) = self.parse_pub_qualifier();
        self.expect_kw(TokKind::KwFn, "to start a method")?;
        self.func_after_fn(
            is_pub,
            is_package_pub,
            false,
            is_pure,
            is_sanitizer,
            state_requires,
            state_transition,
            false,
            None,
            is_must_use,
            must_use_span,
            is_inline,
            is_inline_always,
            inline_span,
        )
    }

    fn field(&mut self) -> Result<Field, Diagnostic> {
        let (is_pub, is_package_pub) = self.parse_pub_qualifier();
        let (name, name_span) = self.expect_ident("for a field name")?;
        self.expect(TokKind::Colon, "after a field name")?;
        let (ty, ty_span) = self.type_()?;
        // D-FIELDPOL1: `name: T => expr` — a computed field. `expr` is a
        // single expression (no block); sibling field names inside it are
        // still bare `Ident`s here — `Sema::CheckerFieldPolicy` rewrites them
        // to `self.<field>` once every field of the struct is known.
        let computed = if matches!(self.peek().kind, TokKind::LambdaArrow) {
            self.bump();
            Some(Box::new(self.expr()?))
        } else {
            None
        };
        Ok(Field {
            is_pub,
            is_package_pub,
            name,
            name_span,
            ty,
            ty_span,
            serde_markers: Vec::new(),
            redact: false,
            computed,
        })
    }

    /// D-PERSIST1: true at `@Persist` immediately before `const`/`#` (module
    /// top level only — this predicate is never consulted by the statement
    /// parser, so a local binding's `@Persist` falls through to the E0145
    /// teaching diagnostic in `Statements.rs` instead).
    fn at_persist_const(&self) -> bool {
        matches!(&self.peek().kind, TokKind::At)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::CONTRACT_PERSIST)
    }

    pub(super) fn const_def(&mut self) -> Result<ConstDef, Diagnostic> {
        // D-PERSIST1: optional `@Persist` (retired `#Persist` teaches E0062).
        let (is_persist, persist_span) = if matches!(&self.peek().kind, TokKind::At | TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::CONTRACT_PERSIST)
        {
            let sigil = self.bump(); // `#` or `@`
            let name_tok = self.bump(); // `Persist`
            let span = Span::new(sigil.span.start, name_tok.span.end);
            if matches!(sigil.kind, TokKind::Hash) {
                self.diags
                    .push(Self::e0062_contract_on_hash(Syntax::CONTRACT_PERSIST, span));
            }
            (true, Some(span))
        } else {
            (false, None)
        };
        let mut attrs = Vec::new();
        while matches!(self.peek().kind, TokKind::Hash) {
            self.bump();
            let (attr_name, _) = self.expect_ident("after `#`")?;
            match attr_name.as_str() {
                "static" => attrs.push(ConstAttr::ForceStatic),
                "inline" => attrs.push(ConstAttr::ForceInline),
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("`#{}` isn't a known attribute on a const", other),
                        "only `#static` and `#inline` are supported on const declarations"
                            .to_string(),
                        "remove the attribute or use `#static` or `#inline`".to_string(),
                        Some(self.peek().span),
                    ));
                }
            }
        }
        self.expect_kw(TokKind::KwConst, "to start a const declaration")?;
        let (name, name_span) = self.expect_ident("after `const`")?;
        self.expect(TokKind::Eq, "after the const name")?;
        let value = self.expr()?;
        self.expect(TokKind::Semi, "after a const value")?;
        Ok(ConstDef {
            name,
            name_span,
            value,
            attrs,
            rust_kind: crate::AST::RustConstKind::Const,
            is_comptime: false,
            ct: None,
            is_persist,
            persist_span,
        })
    }

    /// S57 (M9.5): `comptime NAME = expr;` — a compile-time constant binding.
    pub(super) fn comptime_def(&mut self) -> Result<ConstDef, Diagnostic> {
        let kw = self.peek().span;
        self.expect_kw(TokKind::KwComptime, "to start a comptime binding")?;
        // E0954: `comptime val` / `comptime var` — one keyword suffices.
        if let TokKind::Ident(n) = &self.peek().kind {
            if (n == Syntax::FOREIGN_VAL || n == Syntax::FOREIGN_VAR)
                && matches!(self.peek2().kind, TokKind::Ident(_))
            {
                let foreign = n.clone();
                let extra = self.peek().span;
                return Err(Diagnostic::error(
                    "E0954",
                    format!(
                        "write `{} NAME = ...`, not `{} {} NAME = ...`",
                        Syntax::KW_COMPTIME,
                        Syntax::KW_COMPTIME,
                        foreign
                    ),
                    format!(
                        "`{}` is already the binding keyword, and a comptime value is always a constant",
                        Syntax::KW_COMPTIME
                    ),
                    format!("remove the extra keyword: `{} NAME = ...`", Syntax::KW_COMPTIME),
                    Some(Span::new(kw.start, extra.end)),
                ));
            }
        }
        let (name, name_span) = self.expect_ident("after `comptime`")?;
        self.expect(TokKind::Eq, "after the comptime name")?;
        let value = self.expr()?;
        self.expect(TokKind::Semi, "after a comptime value")?;
        Ok(ConstDef {
            name,
            name_span,
            value,
            attrs: Vec::new(),
            rust_kind: crate::AST::RustConstKind::Const,
            is_comptime: true,
            ct: None,
            is_persist: false,
            persist_span: None,
        })
    }

    // --- statements ------------------------------------------------------

    // --- distinct types --------------------------------------------------

    /// D-DIST1/D-BIND4: true when the cursor is at `Name :: distinct`.
    fn at_distinct_def(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(_))
            && matches!(&self.peek2().kind, TokKind::ColonColon)
            && matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::KW_DISTINCT)
    }

    /// D-CAPBUNDLE1: is `name` one of the four capability-bundle marker names
    /// (`@Numeric`, `@Comparable`, `@Printable`, `@CodableAsBase`) that may
    /// stack before a `distinct` type declaration?
    fn is_capability_bundle_marker(name: &str) -> bool {
        name == Syntax::ATTR_NUMERIC
            || name == Syntax::CONTRACT_BUNDLE_COMPARABLE
            || name == Syntax::CONTRACT_BUNDLE_PRINTABLE
            || name == Syntax::CONTRACT_BUNDLE_CODABLE_AS_BASE
    }

    /// D-DIST3 / D-CAPBUNDLE1 / D-MARKERMOVE1 (ratified 2026-06-20 /
    /// 2026-07-01): true when a stack of one or more capability-bundle
    /// markers (`@Numeric`, `@Comparable`, `@Printable`, `@CodableAsBase`,
    /// any order, retired `#` spelling included so `distinct_def` can teach
    /// E0062) precedes `Name :: distinct` at the cursor. The `@Numeric`-only
    /// sibling of the old `at_numeric_distinct_def` predicate, generalized to
    /// the four fixed bundles.
    fn at_bundle_distinct_def(&self) -> bool {
        let mut i = self.pos;
        let mut saw_marker = false;
        loop {
            match self.toks.get(i).map(|t| &t.kind) {
                Some(TokKind::Hash) | Some(TokKind::At) => {
                    match self.toks.get(i + 1).map(|t| &t.kind) {
                        Some(TokKind::Ident(n)) if Self::is_capability_bundle_marker(n) => {
                            saw_marker = true;
                            i += 2;
                        }
                        _ => break,
                    }
                }
                _ => break,
            }
        }
        if !saw_marker {
            return false;
        }
        matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Ident(_)))
            && matches!(
                self.toks.get(i + 1).map(|t| &t.kind),
                Some(TokKind::ColonColon)
            )
            && matches!(
                self.toks.get(i + 2).map(|t| &t.kind),
                Some(TokKind::Ident(n)) if n == Syntax::KW_DISTINCT
            )
    }

    /// D-DIST1/D-DIST3/D-CAPBUNDLE1/D-MARKERMOVE1: parse
    /// `[@Numeric] [@Comparable] [@Printable] [@CodableAsBase] Name :: distinct BaseType`
    /// — a stack of zero or more capability-bundle markers, any order.
    pub(super) fn distinct_def(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<crate::AST::DistinctDef, Diagnostic> {
        let start = self.peek().span;
        // D-CAPBUNDLE1: zero or more stacked bundle markers (retired `#`
        // spelling on any of them teaches E0062).
        let mut is_numeric = false;
        let mut is_comparable = false;
        let mut comparable_span = None;
        let mut is_printable = false;
        let mut printable_span = None;
        let mut is_codable_as_base = false;
        let mut codable_as_base_span = None;
        while matches!(&self.peek().kind, TokKind::Hash | TokKind::At)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if Self::is_capability_bundle_marker(n))
        {
            let sigil = self.bump(); // consume `#` or `@`
            let (attr, attr_span) = self.expect_ident("after the marker sigil")?;
            if matches!(sigil.kind, TokKind::Hash) {
                self.diags
                    .push(Self::e0062_contract_on_hash(&attr, attr_span));
            }
            if attr == Syntax::ATTR_NUMERIC {
                is_numeric = true;
            } else if attr == Syntax::CONTRACT_BUNDLE_COMPARABLE {
                is_comparable = true;
                comparable_span = Some(attr_span);
            } else if attr == Syntax::CONTRACT_BUNDLE_PRINTABLE {
                is_printable = true;
                printable_span = Some(attr_span);
            } else if attr == Syntax::CONTRACT_BUNDLE_CODABLE_AS_BASE {
                is_codable_as_base = true;
                codable_as_base_span = Some(attr_span);
            }
        }
        // A `#`/`@` here that isn't one of the four bundle markers is a
        // mistake — teach the closed set instead of falling through to a
        // confusing "expected a name" error.
        if matches!(&self.peek().kind, TokKind::Hash | TokKind::At) {
            let sigil = self.peek().kind.clone();
            let attr_span = self.peek2().span;
            let attr = if let TokKind::Ident(n) = &self.peek2().kind {
                n.clone()
            } else {
                String::new()
            };
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "`{}{}` isn't a valid attribute on a distinct type declaration",
                    if matches!(sigil, TokKind::At) { "@" } else { "#" },
                    attr
                ),
                "only the four capability bundles — `@Numeric`, `@Comparable`, `@Printable`, `@CodableAsBase` — are supported before a distinct type".to_string(),
                "use one of the four capability bundles, or remove the attribute".to_string(),
                Some(attr_span),
            ));
        }
        let (name, name_span) = self.expect_ident("as the distinct type name")?;
        // D-BIND4: distinct definitions use `::`.
        match self.peek().kind {
            TokKind::ColonColon => {
                self.bump();
            }
            _ => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected `{}` after the distinct type name, found {}",
                        Syntax::SIGIL_BIND_IMMUT,
                        describe(&self.peek().kind)
                    ),
                    format!(
                        "a distinct type declaration is `Name {} {} BaseType`",
                        Syntax::SIGIL_BIND_IMMUT,
                        Syntax::KW_DISTINCT
                    ),
                    format!(
                        "write: {} {} {} Int",
                        name,
                        Syntax::SIGIL_BIND_IMMUT,
                        Syntax::KW_DISTINCT
                    ),
                    Some(self.peek().span),
                ));
            }
        }
        // consume `distinct` keyword
        match &self.peek().kind {
            TokKind::Ident(n) if n == Syntax::KW_DISTINCT => {
                self.bump();
            }
            other => {
                let other = other.clone();
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected `{}` here, found {}",
                        Syntax::KW_DISTINCT,
                        describe(&other)
                    ),
                    format!(
                        "a distinct type declaration is `Name {} {} BaseType`",
                        Syntax::SIGIL_BIND_IMMUT,
                        Syntax::KW_DISTINCT
                    ),
                    format!(
                        "write: {} {} {} Int",
                        name,
                        Syntax::SIGIL_BIND_IMMUT,
                        Syntax::KW_DISTINCT
                    ),
                    Some(self.peek().span),
                ));
            }
        }
        let (base, base_ty_span) = self.type_()?;
        let base_span = base_ty_span;
        // D-RANGETYPE1: an optional literal range constraint right after the
        // base type — `distinct Int(0..10)`. `..` is inclusive (S22).
        let range = if matches!(self.peek().kind, TokKind::LParen) {
            let open = self.bump().span;
            let (lo, lo_span) = self.expect_range_bound_int("as the range's lower bound")?;
            self.expect(TokKind::DotDot, "between the range's bounds")?;
            let (hi, hi_span) = self.expect_range_bound_int("as the range's upper bound")?;
            let close = self.peek().span;
            self.expect(TokKind::RParen, "to close the range constraint")?;
            let range_span = Span::new(open.start, close.end);
            if lo > hi {
                self.diags.push(Diagnostic::error(
                    "E0137",
                    format!("this range is empty — {} is after {}", lo, hi),
                    "a range's low bound must not be greater than its high bound".to_string(),
                    format!(
                        "write `{}..{}` (swap the bounds), or fix the values",
                        hi, lo
                    ),
                    Some(Span::new(lo_span.start, hi_span.end)),
                ));
            }
            Some((lo, hi, range_span))
        } else {
            None
        };
        self.expect(TokKind::Semi, "after a distinct type declaration")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::DistinctDef {
            is_pub,
            is_package_pub,
            is_numeric,
            is_comparable,
            comparable_span,
            is_printable,
            printable_span,
            is_codable_as_base,
            codable_as_base_span,
            name,
            name_span,
            base,
            base_span,
            range,
            span: Span::new(start.start, end),
        })
    }

    /// D-RANGETYPE1: a plain (non-negative — S34 doesn't lex a leading `-` into
    /// the literal) integer literal used as one bound of a `distinct
    /// Base(lo..hi)` range constraint.
    fn expect_range_bound_int(&mut self, where_: &str) -> Result<(i64, Span), Diagnostic> {
        match self.peek().kind {
            TokKind::Int(n) => {
                let span = self.bump().span;
                Ok((n, span))
            }
            _ => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected a whole number {}, found {}",
                    where_,
                    describe(&self.peek().kind)
                ),
                "a range constraint's bounds are literal whole numbers".to_string(),
                "write a plain integer, e.g. `0..10`".to_string(),
                Some(self.peek().span),
            )),
        }
    }

    /// D-TYPEALIAS1: `alias Name<T, E> = T ? E;`
    pub(super) fn type_alias_def(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<crate::AST::TypeAliasDef, Diagnostic> {
        let start = self.bump().span; // `alias`
        let (name, name_span) = self.expect_ident("after `alias`")?;
        let type_params = self.parse_opt_type_params()?;
        self.expect(TokKind::Eq, "in a type alias declaration")?;
        let (target, target_span) = self.type_()?;
        self.expect(TokKind::Semi, "after a type alias declaration")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::TypeAliasDef {
            is_pub,
            is_package_pub,
            name,
            name_span,
            type_params,
            target,
            target_span,
            span: Span::new(start.start, end),
        })
    }

    // --- unit families (D-QUAL3) --------------------------------------------

    /// D-QUAL3 (ratified 2026-06-24): true when `#UnitFamily(` is at the cursor.
    /// Token stream: `# UnitFamily (`.
    fn at_unit_family_def(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_UNIT_FAMILY)
            && matches!(&self.peek3().kind, TokKind::LParen)
    }

    /// D-QUAL3: parse `#UnitFamily(family) { m1, m2, … }`. Each member mints a
    /// `@Numeric` distinct type erasing to `Float` (lowered in sema/codegen).
    pub(super) fn unit_family_def(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<crate::AST::UnitFamilyDef, Diagnostic> {
        let start = self.peek().span;
        self.expect(TokKind::Hash, "before `UnitFamily`")?;
        // consume the `UnitFamily` marker ident
        let (marker, _) = self.expect_ident("after `#`")?;
        debug_assert_eq!(marker, Syntax::ATTR_UNIT_FAMILY);
        self.expect(TokKind::LParen, "after `#UnitFamily`")?;
        let (family, family_span) = self.expect_ident("as the unit family name")?;
        self.expect(TokKind::RParen, "after the unit family name")?;
        self.expect(TokKind::LBrace, "to open the unit family member list")?;
        let mut members: Vec<(String, Span)> = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            let (member, member_span) = self.expect_ident("as a unit family member")?;
            members.push((member, member_span));
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        self.expect(TokKind::RBrace, "to close the unit family member list")?;
        // The closing `}` ends the item; the lexer inserts a synthetic `;`.
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::UnitFamilyDef {
            is_pub,
            is_package_pub,
            family,
            family_span,
            members,
            span: Span::new(start.start, end),
        })
    }

    // --- layout attribute (D-REPRC1) ----------------------------------------

    /// D-REPRC1: true when `#layout(…) struct` or `#layout(…) pub struct` is at
    /// the cursor. Token stream: `# layout ( variant ) [struct | pub]`.
    fn at_layout_struct(&self) -> bool {
        if !matches!(&self.peek().kind, TokKind::Hash) {
            return false;
        }
        if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_LAYOUT) {
            return false;
        }
        // peek3 must be `(`
        matches!(&self.peek3().kind, TokKind::LParen)
    }

    /// D-REPRC1 / D-SOA1: parse `#layout(variant) [pub] struct Name { … }`.
    /// `c` (C-compatible) and `columnar` (struct-of-arrays) are supported;
    /// `packed`, `align` parse-and-error; the partial form `columnar: f, g`
    /// (D-SOA2B) is rejected (deferred post-v1).
    fn layout_struct_def(
        &mut self,
        outer_is_pub: bool,
    ) -> Result<crate::AST::StructDef, Diagnostic> {
        let attr_start = self.peek().span;
        self.bump(); // consume `#`
        let (attr_name, attr_name_span) = self.expect_ident("after `#`")?;
        debug_assert_eq!(attr_name, Syntax::ATTR_LAYOUT);
        self.expect(TokKind::LParen, "after `#Layout`")?;
        let (variant, variant_span) = self.expect_ident("inside `#Layout(…)`")?;
        // D-SOA2B: partial columnar (`#layout(columnar: x, y)`) is deferred — a
        // `:` after the variant is the partial form. Reject with a clear message.
        if variant == Syntax::LAYOUT_COLUMNAR && matches!(&self.peek().kind, TokKind::Colon) {
            let colon_span = self.peek().span;
            return Err(Diagnostic::error(
                "E1109",
                "partial `#Layout(columnar: …)` isn't supported yet".to_string(),
                "v1 supports whole-struct columnar only — every field becomes a column".to_string(),
                "write `#Layout(columnar)` to convert the whole struct".to_string(),
                Some(Span::new(variant_span.start, colon_span.end)),
            ));
        }
        let layout = match variant.as_str() {
            v if v == Syntax::LAYOUT_C => Some(crate::AST::StructLayout::C),
            v if v == Syntax::LAYOUT_COLUMNAR => Some(crate::AST::StructLayout::Columnar),
            v if v == Syntax::LAYOUT_PACKED || v == Syntax::LAYOUT_ALIGN => {
                return Err(Diagnostic::error(
                    "E1105",
                    format!("`#Layout({})` is reserved and not yet supported", v),
                    "the supported variants are `c` (C-compatible) and `columnar` (struct-of-arrays)".to_string(),
                    "use `#Layout(c)` or `#Layout(columnar)`, or omit `#Layout` for the default".to_string(),
                    Some(variant_span),
                ));
            }
            _ => {
                return Err(Diagnostic::error(
                    "E1105",
                    format!("`#Layout({})` isn't a known layout variant", variant),
                    "the supported variants are `c` (C-compatible) and `columnar` (struct-of-arrays)".to_string(),
                    "write `#Layout(c)` or `#Layout(columnar)`".to_string(),
                    Some(variant_span),
                ));
            }
        };
        let attr_end = self.peek().span;
        self.expect(TokKind::RParen, "to close `#Layout(…)`")?;
        let attr_span = Span::new(attr_start.start, attr_end.end);
        // Consume optional semicolons (newline-inserted) before `struct`/`pub`.
        while matches!(&self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        let is_pub = outer_is_pub
            || if matches!(&self.peek().kind, TokKind::KwPub) {
                self.bump();
                true
            } else {
                false
            };
        let mut def = self.struct_def_after_pub(is_pub)?;
        def.layout = layout;
        def.layout_span = Some(attr_span);
        let _ = attr_name_span;
        Ok(def)
    }

    // --- published-schema marker + migration blocks (D-MIGRATE1) -----------

    /// D-MIGRATE1 / D-MARKERMOVE1 (ratified 2026-06-22 / 2026-07-01): true when
    /// `@PublishedSchema struct` or `@PublishedSchema pub struct` is at the
    /// cursor. Also matches the retired `@PublishedSchema` spelling so
    /// `published_schema_struct_def` can teach E0062.
    /// Note: the lexer inserts a `Semi` after an identifier at end-of-line, so the
    /// token stream is `@ PublishedSchema [Semi] struct` — we check peek4 (pos+3)
    /// when peek3 is a `Semi`, or peek3 when the marker is on the same line.
    fn at_published_schema_struct(&self) -> bool {
        if !matches!(&self.peek().kind, TokKind::Hash | TokKind::At) {
            return false;
        }
        if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_PUBLISHED_SCHEMA) {
            return false;
        }
        // peek3 may be Semi (newline after marker) or KwStruct/KwPub (same-line)
        let peek3 = &self.peek3().kind;
        if matches!(peek3, TokKind::KwStruct | TokKind::KwPub) {
            return true;
        }
        if matches!(peek3, TokKind::Semi) {
            // look one further
            let peek4 = &self.toks[(self.pos + 3).min(self.toks.len() - 1)].kind;
            return matches!(peek4, TokKind::KwStruct | TokKind::KwPub);
        }
        false
    }

    /// D-LIN1 (ratified 2026-06-21): true when `#SingleUse struct` / `#SingleUse enum`
    /// (with an optional newline `Semi` after the marker) is at the cursor. The
    /// `pub #SingleUse …` case is handled inline in the `KwPub` dispatch arm.
    fn at_single_use_type(&self) -> bool {
        if !matches!(&self.peek().kind, TokKind::Hash) {
            return false;
        }
        if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_SINGLE_USE) {
            return false;
        }
        let peek3 = &self.peek3().kind;
        if matches!(peek3, TokKind::KwStruct | TokKind::KwEnum | TokKind::KwPub) {
            return true;
        }
        if matches!(peek3, TokKind::Semi) {
            let peek4 = &self.toks[(self.pos + 3).min(self.toks.len() - 1)].kind;
            return matches!(peek4, TokKind::KwStruct | TokKind::KwEnum | TokKind::KwPub);
        }
        false
    }

    /// D-LIN1 (ratified 2026-06-21): parse `#SingleUse [pub] (struct|enum) Name { … }`.
    /// Sets the `is_single_use` flag on the produced struct/enum so sema can enforce
    /// must-consume-once (E0140/E0141) and no-alias (E0142). The marker erases in
    /// codegen (I3).
    fn single_use_type_def(&mut self, outer_is_pub: bool) -> Result<crate::AST::Item, Diagnostic> {
        let attr_start = self.peek().span;
        self.bump(); // consume `#`
        let (attr, attr_name_span) = self.expect_ident("after `#`")?;
        debug_assert_eq!(attr, Syntax::ATTR_SINGLE_USE);
        let attr_span = Span::new(attr_start.start, attr_name_span.end);
        // The lexer may insert a `Semi` after the marker identifier when the type
        // keyword is on the next line. Consume it so the next token is the keyword.
        while matches!(&self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        // optional `pub` after the marker (the `pub #SingleUse` form already ate it)
        let want_pub = outer_is_pub || matches!(&self.peek().kind, TokKind::KwPub);
        if !outer_is_pub && matches!(&self.peek().kind, TokKind::KwPub) {
            self.bump();
        }
        match self.peek().kind {
            TokKind::KwStruct => {
                let mut def = self.struct_def_after_pub(want_pub)?;
                def.is_single_use = true;
                def.single_use_span = Some(attr_span);
                Ok(crate::AST::Item::Struct(def))
            }
            TokKind::KwEnum => {
                let mut def = self.enum_def_after_pub(want_pub, false)?;
                def.is_single_use = true;
                def.single_use_span = Some(attr_span);
                Ok(crate::AST::Item::Enum(def))
            }
            _ => Err(Diagnostic::error(
                "E0003",
                "`#SingleUse` marks a `struct` or `enum`".to_string(),
                "the marker says values of this type must be used exactly once — only a type can carry that rule".to_string(),
                "write `#SingleUse struct Name { … }` or `#SingleUse enum Name { … }`".to_string(),
                Some(attr_span),
            )),
        }
    }

    /// D-MUSTUSE1 (c18iwxqx) / D-MARKERMOVE1: true when `@MustUse struct` /
    /// `@MustUse enum` is at the cursor. Also matches the retired `@MustUse`
    /// spelling so `must_use_type_def` can teach E0062.
    fn at_must_use_type(&self) -> bool {
        if !matches!(&self.peek().kind, TokKind::Hash | TokKind::At) {
            return false;
        }
        if !matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_MUST_USE) {
            return false;
        }
        let peek3 = &self.peek3().kind;
        if matches!(peek3, TokKind::KwStruct | TokKind::KwEnum | TokKind::KwPub) {
            return true;
        }
        if matches!(peek3, TokKind::Semi) {
            let peek4 = &self.toks[(self.pos + 3).min(self.toks.len() - 1)].kind;
            return matches!(peek4, TokKind::KwStruct | TokKind::KwEnum | TokKind::KwPub);
        }
        false
    }

    /// D-MUSTUSE1/D-MARKERMOVE1: parse `@MustUse [pub] (struct|enum) Name { … }`.
    fn must_use_type_def(&mut self, outer_is_pub: bool) -> Result<crate::AST::Item, Diagnostic> {
        let attr_start = self.peek().span;
        let sigil = self.bump(); // consume `#` or `@`
        let (attr, attr_name_span) = self.expect_ident("after the marker sigil")?;
        debug_assert_eq!(attr, Syntax::ATTR_MUST_USE);
        let attr_span = Span::new(attr_start.start, attr_name_span.end);
        if matches!(sigil.kind, TokKind::Hash) {
            self.diags
                .push(Self::e0062_contract_on_hash(&attr, attr_name_span));
        }
        while matches!(&self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        let want_pub = outer_is_pub || matches!(&self.peek().kind, TokKind::KwPub);
        if !outer_is_pub && matches!(&self.peek().kind, TokKind::KwPub) {
            self.bump();
        }
        match self.peek().kind {
            TokKind::KwStruct => {
                let mut def = self.struct_def_after_pub(want_pub)?;
                def.is_must_use = true;
                def.must_use_span = Some(attr_span);
                Ok(crate::AST::Item::Struct(def))
            }
            TokKind::KwEnum => {
                let mut def = self.enum_def_after_pub(want_pub, false)?;
                def.is_must_use = true;
                def.must_use_span = Some(attr_span);
                Ok(crate::AST::Item::Enum(def))
            }
            _ => Err(Diagnostic::error(
                "E0003",
                "`@MustUse` marks a `struct` or `enum`".to_string(),
                "the marker says values of this type must not be silently ignored — only a type can carry that rule".to_string(),
                "write `@MustUse struct Name { … }` or `@MustUse enum Name { … }`".to_string(),
                Some(attr_span),
            )),
        }
    }

    /// D-MIGRATE1: true when `migration <TypeName> {` is at the cursor (contextual).
    fn at_migration_block(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_MIGRATION)
            && matches!(&self.peek2().kind, TokKind::Ident(_))
            && matches!(&self.peek3().kind, TokKind::LBrace)
    }

    /// D-MIGRATE1/D-MARKERMOVE1 (ratified 2026-06-22 / 2026-07-01): parse
    /// `@PublishedSchema [pub] struct Name { … }`. The retired `@PublishedSchema`
    /// spelling teaches E0062.
    fn published_schema_struct_def(
        &mut self,
        outer_is_pub: bool,
    ) -> Result<crate::AST::StructDef, Diagnostic> {
        let attr_start = self.peek().span;
        let sigil = self.bump(); // consume `#` or `@`
        let (attr, attr_name_span) = self.expect_ident("after the marker sigil")?;
        if attr != Syntax::ATTR_PUBLISHED_SCHEMA {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "`{}{}` isn't a valid attribute on a struct declaration",
                    if matches!(sigil.kind, TokKind::At) {
                        "@"
                    } else {
                        "#"
                    },
                    attr
                ),
                "only `@PublishedSchema` is supported here".to_string(),
                "write `@PublishedSchema` before the struct".to_string(),
                Some(attr_name_span),
            ));
        }
        if matches!(sigil.kind, TokKind::Hash) {
            self.diags
                .push(Self::e0062_contract_on_hash(&attr, attr_name_span));
        }
        let attr_span = Span::new(attr_start.start, attr_name_span.end);
        // The lexer may insert a `Semi` after the marker identifier when `struct` is
        // on the next line. Consume it so the next token is `struct` or `pub`.
        while matches!(&self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        // optional `pub` after the marker
        let is_pub = outer_is_pub
            || if matches!(&self.peek().kind, TokKind::KwPub) {
                self.bump();
                true
            } else {
                false
            };
        let mut def = self.struct_def_after_pub(is_pub)?;
        def.is_published_schema = true;
        def.published_schema_span = Some(attr_span);
        Ok(def)
    }

    /// Parse `struct Name { … }` given that pub/is_pub was already handled.
    /// Factors out the body of `struct_def` when the `pub` keyword is already consumed.
    fn struct_def_after_pub(&mut self, is_pub: bool) -> Result<crate::AST::StructDef, Diagnostic> {
        self.struct_def_after_pub_pkg(is_pub, false)
    }

    fn struct_def_after_pub_pkg(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<crate::AST::StructDef, Diagnostic> {
        self.expect_kw(TokKind::KwStruct, "to start a struct definition")?;
        let (name, name_span) = self.parse_dotted_type_name("after `struct`")?;
        let type_params = self.parse_opt_type_params()?;
        self.expect(TokKind::LBrace, "to open the struct body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut trait_impls = Vec::new();
        let mut derives = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            // D-SERDE5: `#[Rename("x")] who: String` — field-level serde markers.
            // D-DEBUG-REDACT/D-MARKERMOVE1: `@[Redact] who: String` — contract
            // plane; stackable with a `#[…]` serde group on the same field.
            if self.at_marker_list() || self.at_contract_marker_list() {
                let field_markers = self.parse_field_markers()?;
                let mut f = self.field()?;
                let mut redact = false;
                let mut serde_markers = Vec::new();
                for m in field_markers {
                    if m.name == crate::Syntax::ATTR_REDACT {
                        redact = true;
                    } else {
                        serde_markers.push(m);
                    }
                }
                f.serde_markers = serde_markers;
                f.redact = redact;
                fields.push(f);
                if matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                    self.bump();
                }
                continue;
            }
            if matches!(self.peek().kind, TokKind::KwDerive) {
                derives.push(self.derive_line()?);
            } else if matches!(self.peek().kind, TokKind::KwImpl) {
                trait_impls.push(self.trait_impl_block()?);
            } else {
                let is_method = matches!(self.peek().kind, TokKind::KwFn)
                    || (matches!(self.peek().kind, TokKind::KwPub)
                        && matches!(self.peek2().kind, TokKind::KwFn))
                    || self.at_pure_fn()
                    || self.at_sanitizer_fn()
                    || self.at_inline_fn()
                    || self.at_state_fn()
                    || self.at_transition_fn();
                if is_method {
                    methods.push(self.method_in_type()?);
                } else {
                    fields.push(self.field()?);
                    if matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                        self.bump();
                    }
                }
            }
        }
        self.bump(); // }
        Ok(crate::AST::StructDef {
            is_pub,
            is_package_pub,
            name,
            name_span,
            type_params,
            fields,
            methods,
            trait_impls,
            derives,
            is_published_schema: false,
            published_schema_span: None,
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            layout: None,
            layout_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
        })
    }

    /// D-MIGRATE1 (ratified 2026-06-22): parse `migration TypeName { rename a -> b; … }`.
    fn migration_decl(&mut self) -> Result<crate::AST::MigrationDecl, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // consume `migration` ident
        let (type_name, type_span) = self.expect_ident("as the migrated type name")?;
        self.expect(TokKind::LBrace, "after the migration type name")?;
        let mut ops = Vec::new();
        while !matches!(&self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            // consume optional semicolons between ops
            if matches!(&self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            let op_tok = self.bump();
            match &op_tok.kind {
                // D-MIGRATE1: `rename old -> new`.
                TokKind::Ident(kw) if kw == Syntax::KW_RENAME => {
                    let (from, from_span) = self.expect_ident("as the field to rename")?;
                    self.expect(
                        TokKind::Arrow,
                        "between the old and new field names in `rename`",
                    )?;
                    let (to, to_span) = self.expect_ident("as the new field name")?;
                    ops.push(crate::AST::MigrationOp::Rename {
                        from,
                        from_span,
                        to,
                        to_span,
                    });
                }
                // D-MIGRATE2A: `add field: Type = default`.
                TokKind::Ident(kw) if kw == Syntax::KW_ADD => {
                    let (field, field_span) = self.expect_ident("as the field to add")?;
                    self.expect(TokKind::Colon, "after the added field name")?;
                    let (ty, ty_span) = self.type_()?;
                    self.expect(TokKind::Eq, "before the default value of an added field")?;
                    let default = self.expr()?;
                    let default_span = default.span();
                    ops.push(crate::AST::MigrationOp::Add {
                        field,
                        field_span,
                        ty,
                        ty_span,
                        default,
                        default_span,
                        default_fn: None,
                    });
                }
                // D-MIGRATE2D: `remove field`.
                TokKind::Ident(kw) if kw == Syntax::KW_REMOVE => {
                    let (field, field_span) = self.expect_ident("as the field to remove")?;
                    ops.push(crate::AST::MigrationOp::Remove { field, field_span });
                }
                // D-MIGRATE2E: `change field: Old -> New [via { expr }]`.
                TokKind::Ident(kw) if kw == Syntax::KW_CHANGE => {
                    let (field, field_span) =
                        self.expect_ident("as the field whose type changes")?;
                    self.expect(TokKind::Colon, "after the changed field name")?;
                    let (from_ty, from_span) = self.type_()?;
                    self.expect(
                        TokKind::Arrow,
                        "between the old and new field types in `change`",
                    )?;
                    let (to_ty, to_span) = self.type_()?;
                    // Optional `via { expr }` inline converter (D-MIGRATE2B).
                    let (converter, converter_span) = if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_VIA)
                    {
                        let via_start = self.bump().span; // consume `via`
                        self.expect(TokKind::LBrace, "to open the `via { … }` converter body")?;
                        // Tolerate a newline-inserted `Semi` after `{`.
                        while matches!(&self.peek().kind, TokKind::Semi) {
                            self.bump();
                        }
                        let body = self.expr()?;
                        while matches!(&self.peek().kind, TokKind::Semi) {
                            self.bump();
                        }
                        let rbrace_end = self.peek().span.end;
                        self.expect(TokKind::RBrace, "to close the `via { … }` converter body")?;
                        (Some(body), Some(Span::new(via_start.start, rbrace_end)))
                    } else {
                        (None, None)
                    };
                    ops.push(crate::AST::MigrationOp::Change {
                        field,
                        field_span,
                        from_ty,
                        from_span,
                        to_ty,
                        to_span,
                        converter,
                        converter_span,
                        conv_fn: None,
                    });
                }
                // D-MIGRATE2D: `drop` → teach `remove` (E0911 teaching error).
                TokKind::Ident(kw) if kw == Syntax::KW_DROP_RETIRED => {
                    self.diags.push(Diagnostic::error(
                        "E0911",
                        format!("`{}` isn't a migration verb — use `remove`", Syntax::KW_DROP_RETIRED),
                        "a migration deletes a field with `remove`; `drop` is not a Jet keyword here".to_string(),
                        "write `remove <field>` to delete a published field".to_string(),
                        Some(op_tok.span),
                    ));
                    while !matches!(
                        &self.peek().kind,
                        TokKind::Semi | TokKind::RBrace | TokKind::Eof
                    ) {
                        self.bump();
                    }
                }
                // D-MIGRATE2F: `reorder` → reordering needs no migration.
                TokKind::Ident(kw) if kw == Syntax::KW_REORDER_RETIRED => {
                    self.diags.push(Diagnostic::error(
                        "E0911",
                        "`reorder` isn't a migration verb — field order isn't a breaking change".to_string(),
                        "a `@PublishedSchema` record is keyed by field name, so reordering fields is safe and needs no migration".to_string(),
                        "delete the `reorder` line; just write the fields in the order you want".to_string(),
                        Some(op_tok.span),
                    ));
                    while !matches!(
                        &self.peek().kind,
                        TokKind::Semi | TokKind::RBrace | TokKind::Eof
                    ) {
                        self.bump();
                    }
                }
                TokKind::Ident(other) => {
                    let other = other.clone();
                    self.diags.push(Diagnostic::error(
                        "E0911",
                        format!("`{}` isn't a known migration verb", other),
                        "a migration block contains `rename`, `add`, `remove`, or `change` operations".to_string(),
                        "use `rename old -> new`, `add f: T = default`, `remove f`, or `change f: Old -> New via { … }`".to_string(),
                        Some(op_tok.span),
                    ));
                    while !matches!(
                        &self.peek().kind,
                        TokKind::Semi | TokKind::RBrace | TokKind::Eof
                    ) {
                        self.bump();
                    }
                }
                _ => {
                    let desc = describe(&op_tok.kind);
                    self.diags.push(Diagnostic::error(
                        "E0003",
                        format!("expected a migration operation, found {}", desc),
                        "a migration block contains `rename`, `add`, `remove`, or `change` operations".to_string(),
                        "write `rename fieldA -> fieldB` (or `add` / `remove` / `change`)".to_string(),
                        Some(op_tok.span),
                    ));
                    while !matches!(
                        &self.peek().kind,
                        TokKind::Semi | TokKind::RBrace | TokKind::Eof
                    ) {
                        self.bump();
                    }
                }
            }
        }
        self.expect(TokKind::RBrace, "to close the migration block")?;
        let end = self.toks[self.pos - 1].span;
        let span = Span::new(start.start, end.end);
        Ok(crate::AST::MigrationDecl {
            type_name,
            type_span,
            ops,
            span,
        })
    }

    /// D-STATE-DECL: true when `state <TypeName> {` is at the cursor (contextual).
    fn at_state_block(&self) -> bool {
        if !matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_STATE_DECL) {
            return false;
        }
        // `state Reservation {` or `state Payment.Client {` — scan to the `{`.
        let mut i = self.pos + 1;
        while i < self.toks.len() {
            match &self.toks[i].kind {
                TokKind::LBrace => return true,
                TokKind::Semi | TokKind::Eof => return false,
                _ => i += 1,
            }
        }
        false
    }

    /// D-STATE-DECL (ratified 2026-06-25, option B): parse
    /// `[pub] state TypeName { A, B, C }`.
    ///
    /// The state names are comma-separated PascalCase identifiers. The block may have
    /// a trailing comma; semicolons between names are allowed for formatting flexibility.
    fn state_decl(&mut self, is_pub: bool) -> Result<crate::AST::StateDecl, Diagnostic> {
        self.state_decl_with_pkg(is_pub, false)
    }

    fn state_decl_with_pkg(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<crate::AST::StateDecl, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // consume `state`
        let (type_name, type_name_span) =
            self.parse_dotted_type_name("the type name in `state TypeName { … }`")?;
        self.expect(TokKind::LBrace, "to open the state declaration block")?;
        let mut states = Vec::new();
        while !matches!(&self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(&self.peek().kind, TokKind::Comma | TokKind::Semi) {
                self.bump();
                continue;
            }
            let (name, name_span) = self.expect_ident("a state name inside `state { … }`")?;
            states.push((name, name_span));
            // Consume optional trailing comma between state names.
            if matches!(&self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the state declaration block")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::StateDecl {
            is_pub,
            is_package_pub,
            type_name,
            type_name_span,
            states,
            span: Span::new(start.start, end),
        })
    }

    /// D-PROTO1/D-PROTO2: true when `protocol Name {` is at the cursor (contextual).
    fn at_protocol_block(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_PROTOCOL)
            && matches!(&self.peek2().kind, TokKind::Ident(_))
            && matches!(&self.peek3().kind, TokKind::LBrace)
    }

    /// D-PROTO1/D-PROTO2: parse `[pub] protocol Name { … }`.
    fn protocol_decl(&mut self, is_pub: bool) -> Result<crate::AST::ProtocolDecl, Diagnostic> {
        self.protocol_decl_with_pkg(is_pub, false)
    }

    fn protocol_decl_with_pkg(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<crate::AST::ProtocolDecl, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // consume `protocol`
        let (name, name_span) = self.expect_ident("the protocol name in `protocol Name { … }`")?;
        self.expect(TokKind::LBrace, "to open the protocol declaration block")?;
        let mut messages = Vec::new();
        while !matches!(&self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(&self.peek().kind, TokKind::Comma | TokKind::Semi) {
                self.bump();
                continue;
            }
            messages.push(self.protocol_message()?);
        }
        self.expect(TokKind::RBrace, "to close the protocol declaration block")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::ProtocolDecl {
            is_pub,
            is_package_pub,
            name,
            name_span,
            messages,
            span: Span::new(start.start, end),
        })
    }

    /// D-PROTO2: parse one message line — `client -> server: Hello(version: Int)`.
    fn protocol_message(&mut self) -> Result<crate::AST::ProtocolMessage, Diagnostic> {
        let start = self.peek().span;
        let (from, _) = self.expect_ident("the sender in `client -> server: Msg(…)`")?;
        let direction = match from.as_str() {
            Syntax::PROTO_CLIENT => {
                self.expect(
                    TokKind::Arrow,
                    "between the endpoints in a protocol message",
                )?;
                let (to, _) = self.expect_ident("the receiver in `client -> server: Msg(…)`")?;
                if to != Syntax::PROTO_SERVER {
                    return Err(Diagnostic::error(
                        "E0154",
                        format!(
                            "after `{from} ->` expected `{}`, found `{to}`",
                            Syntax::PROTO_SERVER
                        ),
                        "a client-to-server message is written `client -> server: Name(…)`"
                            .to_string(),
                        format!("write `client -> {}: …`", Syntax::PROTO_SERVER),
                        Some(self.peek().span),
                    ));
                }
                crate::AST::ProtocolDirection::ClientToServer
            }
            Syntax::PROTO_SERVER => {
                self.expect(
                    TokKind::Arrow,
                    "between the endpoints in a protocol message",
                )?;
                let (to, _) = self.expect_ident("the receiver in `server -> client: Msg(…)`")?;
                if to != Syntax::PROTO_CLIENT {
                    return Err(Diagnostic::error(
                        "E0154",
                        format!(
                            "after `{from} ->` expected `{}`, found `{to}`",
                            Syntax::PROTO_CLIENT
                        ),
                        "a server-to-client message is written `server -> client: Name(…)`"
                            .to_string(),
                        format!("write `server -> {}: …`", Syntax::PROTO_CLIENT),
                        Some(self.peek().span),
                    ));
                }
                crate::AST::ProtocolDirection::ServerToClient
            }
            other => {
                return Err(Diagnostic::error(
                    "E0154",
                    format!(
                        "protocol messages start with `{}` or `{}`, not `{other}`",
                        Syntax::PROTO_CLIENT,
                        Syntax::PROTO_SERVER
                    ),
                    "each line names who sends and who receives before the message".to_string(),
                    format!(
                        "write `client -> {}: …` or `server -> {}: …`",
                        Syntax::PROTO_SERVER,
                        Syntax::PROTO_CLIENT
                    ),
                    Some(start),
                ));
            }
        };
        self.expect(
            TokKind::Colon,
            "after the endpoint pair in a protocol message",
        )?;
        let (msg_name, name_span) = self.expect_ident("the message name in a protocol line")?;
        self.expect(TokKind::LParen, "to open the message payload")?;
        let fields = self.protocol_message_fields()?;
        self.expect(TokKind::RParen, "to close the message payload")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::ProtocolMessage {
            direction,
            name: msg_name,
            name_span,
            fields,
            span: Span::new(start.start, end),
        })
    }

    fn protocol_message_fields(&mut self) -> Result<Vec<(String, crate::AST::Type)>, Diagnostic> {
        let mut fields = Vec::new();
        if matches!(self.peek().kind, TokKind::RParen) {
            return Ok(fields);
        }
        loop {
            let (name, _) = self.expect_ident("a field name in a protocol message")?;
            self.expect(
                TokKind::Colon,
                "after each field name in a protocol message",
            )?;
            let (ty, _) = self.type_()?;
            fields.push((name, ty));
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                continue;
            }
            break;
        }
        Ok(fields)
    }

    /// D-METADERIVE1=A (amended 2026-07-01): `derive T.Trait { … }` — user-authored
    /// derive block. The retired `derive Trait for T` spelling teaches the dot form (E2714).
    fn user_derive_def(&mut self) -> Result<crate::AST::DeriveDef, Diagnostic> {
        let start = self.peek().span.start;
        self.bump(); // consume `derive`
        let (type_param, type_param_span) = self.expect_ident("after `derive`")?;
        if matches!(self.peek().kind, TokKind::KwFor) {
            // Old spelling `derive Trait for T`: the first ident was the trait name.
            return Err(Diagnostic::error(
                "E2714",
                format!("a user derive is written `derive T.{}`", type_param),
                "the type parameter comes first, joined to the trait name with a dot; `derive Trait for T` was retired".to_string(),
                format!("write `derive T.{} {{ … }}`", type_param),
                Some(type_param_span),
            ));
        }
        self.expect(TokKind::Dot, "after the type parameter in `derive T.Trait`")?;
        let (trait_name, trait_span) = self.expect_ident("after `.` in `derive T.Trait`")?;
        self.expect(TokKind::LBrace, "after the trait name in `derive T.Trait`")?;
        let body = self.block_stmts(); // consumes `}`
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::DeriveDef {
            trait_name,
            trait_span,
            type_param,
            body,
            span: Span::new(start, end),
        })
    }
}

/// U11: render one `Float`-lexed version segment (`major.minor`, e.g. `1.4`,
/// `1.0`) back to text. The lexer only ever produces this token from
/// `digits '.' digits`, so it always has a decimal point — but
/// `f64::to_string()` drops it for a whole-number float (`1.0.to_string()`
/// is `"1"`), which would silently turn `use pkg#1.0;` into `1` (a different,
/// wrong version). Force the point back on; the one still-documented edge
/// case is a trailing zero merged into the fraction (`1.10` vs `1.1` are
/// indistinguishable once lexed — see `Parser::inline_version`).
fn format_version_segment(f: f64) -> String {
    let s = f.to_string();
    if s.contains('.') {
        s
    } else {
        format!("{s}.0")
    }
}
