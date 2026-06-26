use super::*;

impl<'a> Parser<'a> {
    /// U3 (unified-ecosystem §4): `module name { contributions… }`. Many
    /// modules may share a file; a leading-`_` name disables one. The body is a
    /// list of typed namespace contributions (`env.dev: Env { … }`).
    pub(super) fn module_decl(&mut self) -> Result<ModuleDecl, Diagnostic> {
        let start = self.bump().span; // `module`
        // S84: module names may be kebab-case (a module is the package the
        // payload manifest discovers by name).
        let (name, name_span) = self.expect_dashed_name("for the module name")?;
        let disabled = name.starts_with('_');
        self.expect(TokKind::LBrace, "to open the module body")?;
        let mut sources = Vec::new();
        let mut imports = Vec::new();
        let mut members = Vec::new();
        let mut contributions = Vec::new();
        // A module body holds four kinds of entry (U3/U8/D-WORKSPACE1):
        // `sources:`, `imports:`, and `members:` fields, and typed
        // `namespace.path: Value` contributions. The first three are
        // distinguished by their reserved name followed by `:`; contributions
        // begin with a namespace name followed by `.`.
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            match &self.peek().kind {
                TokKind::Ident(n)
                    if n == Syntax::MODULE_FIELD_SOURCES
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    sources.extend(self.module_sources()?);
                }
                TokKind::Ident(n)
                    if n == Syntax::MODULE_FIELD_IMPORTS
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    imports.push(self.module_import()?);
                }
                TokKind::Ident(n)
                    if n == Syntax::MODULE_FIELD_MEMBERS
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    members.push(self.module_members()?);
                }
                _ => contributions.push(self.contribution()?),
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close the module body")?;
        Ok(ModuleDecl {
            name,
            name_span,
            disabled,
            sources,
            imports,
            members,
            contributions,
            span: Span::new(start.start, end),
        })
    }

    /// D-MOD1/2: decide whether `module name` at position `offset` (1 = current pos
    /// already on `module`, 2 = one past `pub`) starts a code module vs a JetOS module.
    ///
    /// Rules (from the plan):
    /// - `module name ;` → code module (file declaration), always.
    /// - `module name {` and first token inside is `fn`, `struct`, `enum`, `impl`,
    ///   `pub`, `const`, `use`, `module`, `}` → code module (inline body).
    /// - `module name {` and first token inside is `sources`, `imports`, or an
    ///   ident followed by `.` → JetOS module.
    pub(super) fn is_code_module_at(&self, offset: usize) -> bool {
        // After `module` is at pos+offset, name is at pos+offset+1.
        let kw_pos = self.pos + offset - 1; // position of `module` token
        // If the token after the name is `-`, the name is dashed (my-host) —
        // that's always a JetOS module; code module names are plain identifiers.
        let after_name = kw_pos + 2;
        if matches!(
            self.toks.get(after_name).map(|t| &t.kind),
            Some(TokKind::Minus)
        ) {
            return false;
        }
        // Find the `{` or `;` that follows the (plain) name.
        let scan = after_name;
        let next = &self.toks[scan.min(self.toks.len() - 1)];
        match &next.kind {
            TokKind::Semi => true, // `module name;` — always a code module
            TokKind::LBrace => {
                // Peek inside the `{` at scan+1
                let inside = &self.toks[(scan + 1).min(self.toks.len() - 1)];
                match &inside.kind {
                    // Code module body starters
                    TokKind::KwFn
                    | TokKind::KwStruct
                    | TokKind::KwEnum
                    | TokKind::KwImpl
                    | TokKind::KwPub
                    | TokKind::KwConst
                    | TokKind::KwUse
                    | TokKind::KwModule
                    | TokKind::KwTrait
                    | TokKind::KwTag
                    | TokKind::KwComptime
                    // D-CASING1 follow-on: `#Test`/`#Pure`/`#Unsafe` markers start
                    // with `#` inside a code-module body.
                    | TokKind::Hash
                    | TokKind::RBrace => true,
                    // JetOS body starters: `sources:`, `imports:`, `members:`, or `ident .`
                    TokKind::Ident(n)
                        if n == Syntax::MODULE_FIELD_SOURCES
                            || n == Syntax::MODULE_FIELD_IMPORTS
                            || n == Syntax::MODULE_FIELD_MEMBERS =>
                    {
                        false
                    }
                    TokKind::Ident(_) => {
                        // `ident .` → JetOS contribution; anything else → code module
                        let after_inside =
                            &self.toks[(scan + 2).min(self.toks.len() - 1)];
                        !matches!(after_inside.kind, TokKind::Dot)
                    }
                    _ => false, // unknown → treat as JetOS to be safe
                }
            }
            _ => false,
        }
    }

    /// D-MOD1/2: parse `[pub] module name ;` or `[pub] module name { items }`.
    /// `is_pub` is true when `pub` was already peeked (but NOT consumed).
    pub(super) fn code_module(&mut self, is_pub: bool) -> Result<CodeModule, Diagnostic> {
        if is_pub {
            self.bump(); // consume `pub`
        }
        let start = self.bump().span; // consume `module`
        let (name, name_span) = self.expect_ident("for the code module name")?;
        match &self.peek().kind {
            TokKind::Semi => {
                let end = self.bump().span.end; // consume `;`
                Ok(CodeModule {
                    name,
                    name_span,
                    is_pub,
                    body: None,
                    span: Span::new(start.start, end),
                })
            }
            TokKind::LBrace => {
                self.bump(); // consume `{`
                let mut items = Vec::new();
                while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
                    match self.top_level_item_in_code_module() {
                        Ok(item) => items.push(item),
                        Err(d) => {
                            self.diags.push(d);
                            self.sync_top();
                        }
                    }
                }
                let end = self.peek().span.end;
                self.expect(TokKind::RBrace, "to close the module body")?;
                Ok(CodeModule {
                    name,
                    name_span,
                    is_pub,
                    body: Some(items),
                    span: Span::new(start.start, end),
                })
            }
            other => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected `{{` or `;` after a module name, found {}",
                    describe(other)
                ),
                "write `module name;` to load a file, or `module name { … }` for an inline module"
                    .to_string(),
                "example: `module math;` or `module math { pub fn double(n: Int) -> Int { … } }`"
                    .to_string(),
                Some(self.peek().span),
            )),
        }
    }

    /// Parse one top-level item inside an inline `module name { … }` body.
    fn top_level_item_in_code_module(&mut self) -> Result<Item, Diagnostic> {
        match &self.peek().kind {
            TokKind::KwFn => self.func().map(Item::Func),
            // S60 (D-CASING1 follow-on): `#Pure fn` inside a module body.
            TokKind::Hash if self.at_pure_fn() => self.func().map(Item::Func),
            // D-TAINT1: `#Sanitizer fn` inside a module body.
            TokKind::Hash if self.at_sanitizer_fn() => self.func().map(Item::Func),
            // D-STATE1: `#State(S) fn` / `#Transition(From -> To) fn` in a module.
            TokKind::Hash if self.at_state_fn() || self.at_transition_fn() => {
                self.func().map(Item::Func)
            }
            // D-ATTR2 / D-SERDE: `#[Codable] struct …` inside a module body.
            TokKind::Hash
                if matches!(self.peek2().kind, TokKind::LBracket) =>
            {
                self.type_def_with_markers()
            }
            TokKind::KwPub => match self.peek2().kind {
                TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
                TokKind::KwTag => self.tag_def(false).map(Item::Tag),
                TokKind::KwUse => {
                    let span = self.peek().span;
                    self.sync_stmt();
                    Err(Diagnostic::error(
                        "E0003",
                        "`pub use` inside an inline code module is not yet supported".to_string(),
                        "unqualified imports inside modules need D-MOD4 ratification".to_string(),
                        "call items by their qualified path for now".to_string(),
                        Some(span),
                    ))
                }
                _ => self.func().map(Item::Func),
            },
            TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
            TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
            TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
            TokKind::KwTag => self.tag_def(false).map(Item::Tag),
            TokKind::KwImpl => self.impl_or_error_conv(),
            TokKind::KwConst | TokKind::At => self.const_def().map(Item::Const),
            TokKind::KwComptime => self.comptime_def().map(Item::Const),
            TokKind::Hash if self.at_test_def() => self.test_def().map(Item::Test),
            // D-BENCH1: `#Bench "name" { … }`.
            TokKind::Hash if self.at_bench_def() => self.bench_def().map(Item::Bench),
            TokKind::KwUse => {
                let span = self.peek().span;
                self.sync_stmt();
                Err(Diagnostic::error(
                    "E0003",
                    "`use` inside an inline code module is not yet supported".to_string(),
                    "unqualified imports inside modules need D-MOD4 ratification".to_string(),
                    "call items by their qualified path for now".to_string(),
                    Some(span),
                ))
            }
            other => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected a function, struct, enum, or other item inside a module, found {}",
                    describe(other)
                ),
                "an inline code module body may only contain top-level items".to_string(),
                "example: `pub fn double(n: Int) -> Int { return n * 2; }`".to_string(),
                Some(self.peek().span),
            )),
        }
    }

    /// U8: a module's `sources: { name: provider@target, … }` block. Each ref is
    /// not a single token (it carries `@`, `/`, `-`, `.`), so we record its
    /// source span and leave validation to modeval (`classify_provider_ref`).
    fn module_sources(&mut self) -> Result<Vec<crate::AST::SourceDecl>, Diagnostic> {
        self.bump(); // `sources`
        self.expect(TokKind::Colon, "after `sources`")?;
        self.expect(TokKind::LBrace, "to open the sources block")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            let (name, name_span) = self.expect_ident("for a source name")?;
            self.expect(TokKind::Colon, "after a source name")?;
            // Consume the `provider@target` ref tokens up to the next `,`/`}`;
            // the recovered span slices back to the exact written text.
            let ref_start = self.peek().span;
            let mut ref_end = ref_start.end;
            if matches!(self.peek().kind, TokKind::Comma | TokKind::RBrace | TokKind::Eof) {
                return Err(Diagnostic::error(
                    "E0003",
                    "a source needs a `provider@target` ref".to_string(),
                    "every named source resolves to an upstream, e.g. `default: github@NixOS/nixpkgs/nixos-24.05`"
                        .to_string(),
                    "write the ref after the `:`".to_string(),
                    Some(ref_start),
                ));
            }
            while !matches!(
                self.peek().kind,
                TokKind::Comma | TokKind::RBrace | TokKind::Eof
            ) {
                ref_end = self.peek().span.end;
                self.bump();
            }
            out.push(crate::AST::SourceDecl {
                name,
                name_span,
                ref_span: Span::new(ref_start.start, ref_end),
                span: Span::new(name_span.start, ref_end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the sources block")?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(out)
    }

    /// U8: a module's `imports: find("./modules")` directive. The value is an
    /// ordinary call expression; the `find` walk itself lands with U4 discovery.
    fn module_import(&mut self) -> Result<Expr, Diagnostic> {
        self.bump(); // `imports`
        self.expect(TokKind::Colon, "after `imports`")?;
        let value = self.expr()?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(value)
    }

    /// D-WORKSPACE1=B: `members: <expr>` in `module workspace { … }`. The value
    /// is any expression — most commonly `find("./packages")` or a list literal.
    fn module_members(&mut self) -> Result<Expr, Diagnostic> {
        self.bump(); // `members`
        self.expect(TokKind::Colon, "after `members`")?;
        let value = self.expr()?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(value)
    }

    /// U3 (unified-ecosystem §5): one typed namespace contribution,
    /// `namespace.path: Value`, e.g. `env.dev: Env { … }`. The value reuses the
    /// ordinary expression parser (struct literals, lists, strings).
    fn contribution(&mut self) -> Result<Contribution, Diagnostic> {
        let (ns_name, ns_span) =
            self.expect_ident("for a namespace (`env`, `system`, or `image`)")?;
        let namespace = match ns_name.as_str() {
            Syntax::NS_ENV => Namespace::Env,
            Syntax::NS_SYSTEM => Namespace::System,
            Syntax::NS_IMAGE => Namespace::Image,
            _ => {
                return Err(Diagnostic::error(
                    "E0960",
                    format!("`{}` is not a module namespace", ns_name),
                    format!(
                        "a module contributes to exactly three reserved namespaces: `{}` (a dev environment), `{}` (a whole machine), and `{}` (a disk image)",
                        Syntax::NS_ENV, Syntax::NS_SYSTEM, Syntax::NS_IMAGE
                    ),
                    format!(
                        "begin the contribution with `{}`, `{}`, or `{}`",
                        Syntax::NS_ENV, Syntax::NS_SYSTEM, Syntax::NS_IMAGE
                    ),
                    Some(ns_span),
                ));
            }
        };
        self.expect(TokKind::Dot, "after the namespace name")?;
        // S84: contribution names (`system.<name>`, `image.<name>`, `env.<name>`)
        // may be kebab-case, e.g. `image.halcyon-iso`.
        let (path, path_span) = self.expect_dashed_name("for the contribution name")?;
        self.expect(TokKind::Colon, "after the contribution name")?;
        // U11/U14/U18: `system.<name>:` and `image.<name>:` parse into dedicated
        // typed literals (the U13 `options` list, the typed `target` value, the
        // U12 `Service` map, and U18 bare `{ … }` don't fit the ordinary
        // expression grammar). `env.<name>:` keeps the ordinary expression parser.
        let value = match namespace {
            Namespace::Env => crate::AST::ContribValue::Expr(self.expr()?),
            Namespace::System => crate::AST::ContribValue::System(self.system_lit()?),
            Namespace::Image => crate::AST::ContribValue::Image(self.image_lit()?),
        };
        let end = value.span().end;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(Contribution {
            namespace,
            path,
            path_span,
            value,
            span: Span::new(ns_span.start, end),
        })
    }

    /// U11/U18: parse a `System { … }` or bare `{ … }` record. The type name
    /// `System` is optional (U18 inferred constructor); when present it is
    /// recorded so modeval can keep allowing the explicit form (S29).
    fn system_lit(&mut self) -> Result<crate::AST::SystemLit, Diagnostic> {
        let start = self.peek().span.start;
        let explicit_type = self.opt_record_type(Syntax::TYPE_SYSTEM)?;
        self.expect(TokKind::LBrace, "to open a `System` record")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            fields.push(self.system_field()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close a `System` record")?;
        Ok(crate::AST::SystemLit {
            explicit_type,
            fields,
            span: Span::new(start, end),
        })
    }

    /// U18: an optional record type name before `{`. Returns its span when the
    /// author wrote it (`System.{ … }` / `Image.{ … }` / `Service.{ … }`), `None`
    /// for a bare `{ … }`. D-DOTCTOR1: also accepts the old dotless `System { … }`
    /// form (E0320 recovery — `jet fmt` auto-fixes).
    fn opt_record_type(&mut self, expected: &str) -> Result<Option<Span>, Diagnostic> {
        // D-DOTCTOR1: new form `TypeName.{` — consume Ident and Dot.
        if self.peek_is_ident(expected)
            && matches!(self.peek2().kind, TokKind::Dot)
            && matches!(self.peek3().kind, TokKind::LBrace)
        {
            let span = self.bump().span; // consume type name
            self.bump(); // consume dot
            return Ok(Some(span));
        }
        // D-DOTCTOR2 recovery: old dotless `TypeName {` — emit E0320, consume Ident.
        if self.peek_is_ident(expected) && matches!(self.peek2().kind, TokKind::LBrace) {
            let span = self.bump().span;
            let brace_span = self.peek().span;
            self.diags.push(Diagnostic::error(
                "E0320",
                format!(
                    "struct construction uses `{}.{{…}}`, not `{} {{…}}`",
                    expected, expected
                ),
                "named construction has a dot before the brace (D-DOTCTOR1)".to_string(),
                format!("write `{}.{{…}}` instead", expected),
                Some(brace_span),
            ));
            return Ok(Some(span));
        }
        Ok(None)
    }

    /// U11/U12/U13: one field inside a `System { … }` record.
    fn system_field(&mut self) -> Result<crate::AST::SystemField, Diagnostic> {
        let (name, name_span) = self.expect_ident("for a `System` field name")?;
        self.expect(TokKind::Colon, "after a `System` field name")?;
        let value = match name.as_str() {
            Syntax::SYSTEM_FIELD_TARGET => {
                let (os, arch, span) = self.platform_value()?;
                crate::AST::SystemFieldValue::Platform { os, arch, span }
            }
            Syntax::SYSTEM_FIELD_PACKAGES => {
                crate::AST::SystemFieldValue::Packages(self.expr()?)
            }
            Syntax::SYSTEM_FIELD_SERVICES => {
                crate::AST::SystemFieldValue::Services(self.services_map()?)
            }
            Syntax::SYSTEM_FIELD_OPTIONS => {
                crate::AST::SystemFieldValue::Options(self.options_list()?)
            }
            _ => crate::AST::SystemFieldValue::Other(self.expr()?),
        };
        let end = value_end_system(&value);
        Ok(crate::AST::SystemField {
            name,
            name_span,
            value,
            span: Span::new(name_span.start, end),
        })
    }

    /// U13: a dotted typed platform value — `linux.x64`. Two name segments joined
    /// by `.`; modeval checks they name a known platform.
    fn platform_value(&mut self) -> Result<(String, String, Span), Diagnostic> {
        let (os, os_span) = self.expect_ident("for a platform, e.g. `linux`")?;
        self.expect(TokKind::Dot, "between the platform and its architecture")?;
        let (arch, arch_span) = self.expect_ident("for an architecture, e.g. `x64`")?;
        Ok((os, arch, Span::new(os_span.start, arch_span.end)))
    }

    /// U12/U18: a `services: { name: { … }, … }` map — each entry is a service
    /// name and an inferred (or explicit) `Service` record.
    fn services_map(&mut self) -> Result<Vec<crate::AST::ServiceEntry>, Diagnostic> {
        self.expect(TokKind::LBrace, "to open the `services` map")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            let (name, name_span) = self.expect_ident("for a service name")?;
            self.expect(TokKind::Colon, "after a service name")?;
            let explicit_type = self.opt_record_type(Syntax::TYPE_SERVICE)?;
            self.expect(TokKind::LBrace, "to open a `Service` record")?;
            let mut fields = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
                let (field, field_span) = self.expect_ident("for a `Service` field name")?;
                self.expect(TokKind::Colon, "after a `Service` field name")?;
                let value = self.expr()?;
                fields.push((field, field_span, value));
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                }
            }
            let rec_end = self.peek().span.end;
            self.expect(TokKind::RBrace, "to close a `Service` record")?;
            out.push(crate::AST::ServiceEntry {
                name,
                name_span,
                explicit_type,
                fields,
                span: Span::new(name_span.start, rec_end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the `services` map")?;
        Ok(out)
    }

    /// U13: an `options: [ dotted.key: value, … ]` ordered list. Each entry is a
    /// dotted key path and a value expression (bare identifier, dotted typed
    /// value, list, or quoted free-form string).
    fn options_list(&mut self) -> Result<Vec<crate::AST::OptionEntry>, Diagnostic> {
        self.expect(TokKind::LBracket, "to open the `options` list")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBracket | TokKind::Eof) {
            let (mut key, key_start) = self.expect_ident("for an option key, e.g. `net.hostName`")?;
            let mut key_end = key_start.end;
            while matches!(self.peek().kind, TokKind::Dot) {
                self.bump();
                let (seg, seg_span) = self.expect_ident("for the next part of the option key")?;
                key.push('.');
                key.push_str(&seg);
                key_end = seg_span.end;
            }
            let key_span = Span::new(key_start.start, key_end);
            self.expect(TokKind::Colon, "after an option key")?;
            let value_start = self.peek().span.start;
            let value = self.expr()?;
            // Record the value's full written span from the first token of the
            // value to the last token consumed (the token before the cursor) —
            // robust for dotted typed values like `default.fish` whose `Expr`
            // span covers only the final member.
            let value_end = self.prev_end();
            out.push(crate::AST::OptionEntry {
                key,
                key_span,
                value,
                value_span: Span::new(value_start, value_end),
                span: Span::new(key_start.start, value_end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBracket, "to close the `options` list")?;
        Ok(out)
    }

    /// U14/U18: parse an `Image { … }` or bare `{ … }` record.
    fn image_lit(&mut self) -> Result<crate::AST::ImageLit, Diagnostic> {
        let start = self.peek().span.start;
        let explicit_type = self.opt_record_type(Syntax::TYPE_IMAGE)?;
        self.expect(TokKind::LBrace, "to open an `Image` record")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            fields.push(self.image_field()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close an `Image` record")?;
        Ok(crate::AST::ImageLit {
            explicit_type,
            fields,
            span: Span::new(start, end),
        })
    }

    /// U14: one field inside an `Image { … }` record.
    fn image_field(&mut self) -> Result<crate::AST::ImageField, Diagnostic> {
        let (name, name_span) = self.expect_ident("for an `Image` field name")?;
        self.expect(TokKind::Colon, "after an `Image` field name")?;
        let value = match name.as_str() {
            Syntax::IMAGE_FIELD_FROM => {
                // `from: system.<name>` — the `system` keyword then the name.
                let (kw, kw_span) = self.expect_ident("for `system`, e.g. `system.halcyon`")?;
                if kw != Syntax::NS_SYSTEM {
                    return Err(image_from_not_system(kw_span));
                }
                self.expect(TokKind::Dot, "after `system`")?;
                // S84: `from: system.<name>` may reference a kebab-case System
                // name; must read the same way the definition does so the E0978
                // cross-check still string-matches.
                let (sys, sys_span) = self.expect_dashed_name("for the system name")?;
                crate::AST::ImageFieldValue::From {
                    system: sys,
                    span: Span::new(kw_span.start, sys_span.end),
                }
            }
            Syntax::IMAGE_FIELD_FORMAT => {
                let (word, span) = self.expect_ident("for a format, e.g. `iso`")?;
                crate::AST::ImageFieldValue::Format { word, span }
            }
            Syntax::SYSTEM_FIELD_TARGET => {
                let (os, arch, span) = self.platform_value()?;
                crate::AST::ImageFieldValue::Platform { os, arch, span }
            }
            _ => crate::AST::ImageFieldValue::Other(self.expr()?),
        };
        let end = value_end_image(&value);
        Ok(crate::AST::ImageField {
            name,
            name_span,
            value,
            span: Span::new(name_span.start, end),
        })
    }

}
