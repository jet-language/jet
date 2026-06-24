use super::*;

impl<'a> Parser<'a> {
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
                            // use alias.{A, B, ...}
                            self.bump(); // consume `.`
                            let lbrace_span = self.bump().span; // consume `{`
                            let mut items = Vec::new();
                            loop {
                                if matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                                    break;
                                }
                                let (item, _) = self.expect_ident("inside `use alias.{…}`")?;
                                items.push(item);
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
                            // Check if token after the item ident is `;` (no `as`).
                            // peek3() is now 2 positions past current (dot + ident).
                            let after_item = &self.peek3().kind;
                            if matches!(after_item, TokKind::Semi | TokKind::Eof) {
                                // use alias.item ; — Unqualified single
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
                                        items: vec![item.clone()],
                                        items_span,
                                        span: Span::new(start.start, end),
                                    },
                                    alias: item.clone(),
                                    alias_span: item_span,
                                    span: Span::new(start.start, end),
                                    is_pub: false,
                                })
                            } else {
                                // use core.fs as fs — Module path with dots
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
        })
    }

    pub(super) fn program(&mut self) -> Program {
        let mut imports = Vec::new();
        let mut items = Vec::new();
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
                TokKind::KwExtern => self.extern_rust_block().map(Item::ExternRust),
                TokKind::KwFn => self.func().map(Item::Func),
                // S60 (D-CASING1 follow-on): `#Pure fn name(…)` purity modifier.
                TokKind::Hash if self.at_pure_fn() => self.func().map(Item::Func),
                // S14: bare lowercase `pure` is the retired spelling (E0053).
                TokKind::Ident(n) if n == Syntax::FOREIGN_PURE && self.foreign_pure_follows() => {
                    let t = self.bump();
                    self.diags.push(self.foreign_pure_diag(t.span));
                    self.func_with_purity(true).map(Item::Func)
                }
                TokKind::KwPub => match self.peek2().kind {
                    // D-REPRC1: `pub #layout(c) struct Name { … }`
                    TokKind::Hash if {
                        matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::ATTR_LAYOUT)
                    } => {
                        self.bump(); // consume `pub`
                        self.layout_struct_def(true).map(Item::Struct)
                    }
                    // D-MIGRATE1: `pub #PublishedSchema struct Name { … }`
                    TokKind::Hash if {
                        matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::ATTR_PUBLISHED_SCHEMA)
                    } => {
                        self.bump(); // consume `pub`
                        self.published_schema_struct_def(true).map(Item::Struct)
                    }
                    TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                    TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                    TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
                    TokKind::KwModule if self.is_code_module_at(2) => {
                        self.code_module(true).map(Item::CodeModule)
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
                },
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
                TokKind::KwModule if self.is_code_module_at(1) => {
                    self.code_module(false).map(Item::CodeModule)
                }
                TokKind::KwModule => self.module_decl().map(Item::Module),
                TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
                TokKind::KwImpl => self.impl_or_error_conv(),
                TokKind::Hash if self.at_c_module() => self.c_module().map(Item::CModule),
                TokKind::Hash if self.at_unsafe_fn() => self.unsafe_fn().map(Item::Func),
                TokKind::Hash if self.at_numeric_distinct_def() => {
                    self.distinct_def(false).map(Item::Distinct)
                }
                // D-REPRC1: `#layout(c) struct Name { … }`
                TokKind::Hash if self.at_layout_struct() => {
                    self.layout_struct_def(false).map(Item::Struct)
                }
                // D-MIGRATE1: `#PublishedSchema struct Name { … }`
                TokKind::Hash if self.at_published_schema_struct() => {
                    self.published_schema_struct_def(false).map(Item::Struct)
                }
                // D-MIGRATE1: `migration TypeName { rename a -> b }`
                TokKind::Ident(n) if n == Syntax::KW_MIGRATION && self.at_migration_block() => {
                    self.migration_decl().map(Item::Migration)
                }
                TokKind::KwConst | TokKind::Hash => self.const_def().map(Item::Const),
                TokKind::At => {
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0990",
                        format!("attributes use `{}`, not `@`", Syntax::ATTR_PREFIX),
                        "in Jet, `@` is for loop labels; attributes and markers use `#` (D-ATTR1)".to_string(),
                        "write `#Unsafe(\"…\")`, `#Numeric`, or `#[Marker, …]` instead of `@…`".to_string(),
                        Some(t.span),
                    ));
                    self.sync_top();
                    continue;
                }
                TokKind::KwComptime => self.comptime_def().map(Item::Const),
                TokKind::Ident(_) if self.at_distinct_def() => {
                    self.distinct_def(false).map(Item::Distinct)
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
                    self.func_after_fn(false, false, false).map(Item::Func)
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
                            "define a function ({} main() {{ ... }}), #{} block, struct, or const",
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
                Ok(item) => items.push(item),
                Err(d) => {
                    self.diags.push(d);
                    self.sync_top();
                }
            }
        }
        Program { imports, items }
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
        let (name, name_span) = self.expect_test_name()?;
        self.expect(TokKind::LBrace, "to open the test body")?;
        let body = self.block_stmts();
        Ok(crate::AST::TestDef {
            name,
            name_span,
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
            format!("write: #{} \"name\" {{ ... }}", Syntax::KW_TEST),
            Some(span),
        )
    }

    /// S60 (D-CASING1 follow-on): true when the cursor is at `#Pure fn`/`#Pure pub`.
    pub(super) fn at_pure_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_PURE)
            && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
    }

    /// S14: bare lowercase `pure` introduces a function only when `fn`/`pub`
    /// follows (so an ordinary identifier named `pure` is unaffected).
    fn foreign_pure_follows(&self) -> bool {
        matches!(self.peek2().kind, TokKind::KwFn | TokKind::KwPub)
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

    /// Test names are plain string literals — no interpolation (S43).
    fn expect_test_name(&mut self) -> Result<(String, Span), Diagnostic> {
        self.expect_marker_name(Syntax::KW_TEST)
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
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
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

        let mut return_type = None;
        let mut is_view_return = false;
        if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            if matches!(self.peek().kind, TokKind::KwView) {
                is_view_return = true;
                self.bump();
            }
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
            is_view_return,
            rust_path,
            rust_path_span,
            span: Span::new(fn_start, end),
        })
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
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "after `#Unsafe`")?;
        self.func_after_fn(is_pub, true, false)
    }

    /// S59 (E2-M14): is the cursor at the start of a C FFI module — `#extern
    /// module …` or `#bindgen module …`? (Distinguishes from `#static const`,
    /// and from bare `extern rust`.)
    fn at_c_module(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::Hash) {
            return false;
        }
        let intro_is_c = match &self.peek2().kind {
            TokKind::KwExtern => true,
            TokKind::Ident(n) => n == Syntax::ATTR_BINDGEN,
            _ => false,
        };
        intro_is_c && matches!(self.peek3().kind, TokKind::KwModule)
    }

    /// S59 (E2-M14): parse `#extern module c.<lib> { … }` (overlay) or
    /// `#bindgen module c.<lib>.__bindgen__ { … }` (generated cache). Body
    /// declarations share the `extern_fn` shape (`fn name(args) -> T = "Sym";`).
    fn c_module(&mut self) -> Result<crate::AST::CModule, Diagnostic> {
        use crate::AST::CModuleKind;
        let start = self.bump().span; // `#`
        let kind = match &self.peek().kind {
            TokKind::KwExtern => {
                self.bump();
                CModuleKind::Extern
            }
            TokKind::Ident(n) if n == Syntax::ATTR_BINDGEN => {
                self.bump();
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
                    "a C FFI module begins with `#extern module c.<lib>` or `#bindgen module c.<lib>.__bindgen__`".to_string(),
                    "write: #extern module c.raylib { fn init_window(w: Int, h: Int, title: String) = \"InitWindow\"; }".to_string(),
                    Some(self.peek().span),
                ));
            }
        };
        self.expect_kw(TokKind::KwModule, "to declare a C FFI module")?;

        // Parse the dotted module path: `c` `.` `<lib>` [ `.` `__bindgen__` ].
        let path_start = self.peek().span;
        let (root, _) = self.expect_ident("after `module`")?;
        if root != Syntax::C_MODULE_ROOT {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "a C FFI module path starts with `{}.`, found `{}`",
                    Syntax::C_MODULE_ROOT, root
                ),
                "C libraries live under the `c.` module root — `c.raylib`, `c.sqlite3`".to_string(),
                format!("write: {} module {}.<lib> {{ … }}",
                    match kind { CModuleKind::Extern => "#extern", CModuleKind::Bindgen => "#bindgen" },
                    Syntax::C_MODULE_ROOT),
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
                    format!("write: #extern module {}.{} {{ … }}", Syntax::C_MODULE_ROOT, lib),
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
        // A `#bindgen` module must carry the `__bindgen__` segment (it is the
        // generated surface). Without it the path is malformed.
        if kind == CModuleKind::Bindgen && !has_bindgen_seg {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "a `#bindgen` module path must end in `.{}`",
                    Syntax::C_BINDGEN_SEGMENT
                ),
                "the compiler generates `#bindgen module c.<lib>.__bindgen__` cache files".to_string(),
                format!(
                    "write: #bindgen module {}.{}.{} {{ … }}",
                    Syntax::C_MODULE_ROOT, lib, Syntax::C_BINDGEN_SEGMENT
                ),
                Some(path_span),
            ));
        }

        self.expect(TokKind::LBrace, "to open the C FFI module body")?;
        let mut functions = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
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

    pub(super) fn func(&mut self) -> Result<Func, Diagnostic> {
        // S60 (D-CASING1 follow-on): `#Pure fn` / `#Pure pub fn` — consume the
        // `#Pure` marker (guaranteed by `at_pure_fn` when present).
        let is_pure = if self.at_pure_fn() {
            self.bump(); // `#`
            self.bump(); // `Pure`
            true
        } else {
            false
        };
        self.func_with_purity(is_pure)
    }

    /// Parse a function whose purity is already known (the bare-`pure` teaching
    /// path enters here after emitting E0053 and consuming the `pure` word).
    pub(super) fn func_with_purity(&mut self, is_pure: bool) -> Result<Func, Diagnostic> {
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "to start a function definition")?;
        self.func_after_fn(is_pub, false, is_pure)
    }

    fn func_after_fn(&mut self, is_pub: bool, is_unsafe: bool, is_pure: bool) -> Result<Func, Diagnostic> {
        let (name, name_span) = self.expect_ident("after `fn`")?;
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

        // D-EFF1 / D-QUAL1: an optional `#(Net, Db)` effect bound, between the
        // parameter list and the return arrow. Effect names are validated in
        // sema, not here.
        let declared_effects = self.parse_opt_effect_annotation()?;

        let mut return_type = None;
        let mut is_view_return = false;
        if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            if matches!(self.peek().kind, TokKind::KwView) {
                is_view_return = true;
                self.bump();
            }
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
            let end = if self.pos > 0 { self.toks[self.pos - 1].span.end } else { expr_end };
            let ret_span = Span::new(start, end);
            let body = vec![crate::AST::Stmt::Return(Some(expr), ret_span)];
            return Ok(Func {
                is_pub,
                name,
                name_span,
                type_params,
                params,
                return_type,
                is_view_return,
                is_unsafe,
                is_pure,
                declared_effects,
                body,
            });
        }
        self.expect(TokKind::LBrace, "to open the function body")?;
        let body = self.block_stmts();
        Ok(Func {
            is_pub,
            name,
            name_span,
            type_params,
            params,
            return_type,
            is_view_return,
            is_unsafe,
            is_pure,
            declared_effects,
            body,
        })
    }

    /// D-EFF1 / D-QUAL1: parse an optional `#(Net, Db)` effect bound. Returns
    /// `None` when the cursor is not at `#(`. Effect names are bare idents here;
    /// sema validates them against the known effect vocabulary.
    fn parse_opt_effect_annotation(
        &mut self,
    ) -> Result<Option<Vec<(String, Span)>>, Diagnostic> {
        if !(matches!(self.peek().kind, TokKind::Hash)
            && matches!(self.peek2().kind, TokKind::LParen))
        {
            return Ok(None);
        }
        self.bump(); // `#`
        self.expect(TokKind::LParen, "after `#` to start an effect list")?;
        let mut effects = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (name, span) = self.expect_ident("for an effect name")?;
                effects.push((name, span));
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between effects in the list")?;
            }
        }
        self.expect(TokKind::RParen, "to close the effect list")?;
        Ok(Some(effects))
    }

    fn param(&mut self) -> Result<Param, Diagnostic> {
        let mut convention = self.parse_access_prefix();
        let (name, name_span) = if matches!(self.peek().kind, TokKind::KwSelf) {
            let span = self.bump().span;
            (Syntax::KW_SELF.to_string(), span)
        } else {
            self.expect_ident("for a parameter name")?
        };
        let (ty, ty_span) = if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            // D-CAP7: the capability sigil rides the type side — `name: ~T`/`^T`/`&T`.
            // (Receivers carry it on `self` instead: `~self`, parsed above.)
            if let Some(type_cap) = self.parse_capability_sigil() {
                // A real pre-name marker (not the unmarked `Infer` default) plus a
                // type-side sigil is two markers (E0029).
                if convention != AccessConvention::Infer {
                    self.diags.push(Diagnostic::error(
                        "E0029",
                        format!("`{}` has two capability markers", name),
                        "a parameter's access capability is written once — on the type \
                         (`name: ~Type`), or on `self` for a receiver"
                            .to_string(),
                        "keep the sigil on the type and remove the other".to_string(),
                        Some(name_span),
                    ));
                }
                convention = type_cap;
            }
            self.type_()?
        } else if name == Syntax::KW_SELF {
            // S27: receiver type is the owning struct/enum; sema fills it in.
            (Type::Named(String::new()), name_span)
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
        Ok(Param {
            convention,
            name,
            name_span,
            ty,
            ty_span,
            default,
        })
    }

    pub(super) fn struct_def(&mut self, nested: bool) -> Result<StructDef, Diagnostic> {
        let is_pub = if nested {
            false
        } else {
            matches!(self.peek().kind, TokKind::KwPub)
        };
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwStruct, "to start a struct definition")?;
        let (name, name_span) = self.expect_ident("after `struct`")?;
        let type_params = self.parse_opt_type_params()?;
        self.expect(TokKind::LBrace, "to open the struct body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut trait_impls = Vec::new();
        let mut derives = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            if matches!(self.peek().kind, TokKind::KwDerive) {
                derives.push(self.derive_line()?);
            } else if matches!(self.peek().kind, TokKind::KwImpl) {
                trait_impls.push(self.trait_impl_block()?);
            } else {
                let is_method = matches!(self.peek().kind, TokKind::KwFn)
                    || (matches!(self.peek().kind, TokKind::KwPub)
                        && matches!(self.peek2().kind, TokKind::KwFn));
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
            name,
            name_span,
            type_params,
            fields,
            methods,
            trait_impls,
            derives,
            is_published_schema: false,
            published_schema_span: None,
            layout: None,
            layout_span: None,
        })
    }

    pub(super) fn enum_def(&mut self, nested: bool) -> Result<EnumDef, Diagnostic> {
        let is_pub = if nested {
            false
        } else {
            matches!(self.peek().kind, TokKind::KwPub)
        };
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwEnum, "to start an enum definition")?;
        let (name, name_span) = self.expect_ident("after `enum`")?;
        let type_params = self.parse_opt_type_params()?;
        self.expect(TokKind::LBrace, "to open the enum body")?;
        let mut variants = Vec::new();
        let mut methods = Vec::new();
        let mut trait_impls = Vec::new();
        let mut derives = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            if matches!(self.peek().kind, TokKind::KwDerive) {
                derives.push(self.derive_line()?);
            } else if matches!(self.peek().kind, TokKind::KwImpl) {
                trait_impls.push(self.trait_impl_block()?);
            } else if matches!(self.peek().kind, TokKind::KwFn | TokKind::KwPub) {
                methods.push(self.method_in_type()?);
            } else {
                variants.push(self.variant()?);
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
            }
        }
        self.bump();
        Ok(EnumDef {
            is_pub,
            name,
            name_span,
            type_params,
            variants,
            methods,
            trait_impls,
            derives,
        })
    }

    fn variant(&mut self) -> Result<Variant, Diagnostic> {
        let (name, name_span) = self.expect_ident("for a variant name")?;
        let payload = if matches!(self.peek().kind, TokKind::LParen) {
            self.bump();
            let payload = self.variant_payload()?;
            self.expect(TokKind::RParen, "after a variant's payload")?;
            payload
        } else {
            VariantPayload::Unit
        };
        Ok(Variant {
            name,
            name_span,
            payload,
        })
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
        let (type_name, type_span) = self.parse_type_path("after `impl`")?;
        // Detect `impl Source -> Target { body }` — D-ERR-CONV.
        if matches!(self.peek().kind, TokKind::Arrow) {
            let _arrow = self.bump(); // consume `->`
            let (to_ty, to_span) = self.parse_type_path("after `->` in error conversion")?;
            // Peek the `{` span before consuming.
            if !matches!(self.peek().kind, TokKind::LBrace) {
                return Err(Diagnostic::error(
                    "E0003",
                    "expected `{` to open the error-conversion body".to_string(),
                    "an error conversion body is a block: `impl Source -> Target { … }`".to_string(),
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
        // Normal `impl` path — re-enter parsing without re-consuming `impl` or type name.
        let (trait_name, trait_span) = if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            let (t, ts) = self.expect_ident("after `:` in `impl Type: Trait`")?;
            (Some(t), Some(ts))
        } else {
            (None, None)
        };
        // S62: `impl Type: Trait using field_name;` — delegation form.
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
                }));
            }
        }
        self.expect(TokKind::LBrace, "to open the `impl` body")?;
        let mut methods = Vec::new();
        let mut assoc_type_impls = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            if let TokKind::Ident(ref kw) = self.peek().kind.clone() {
                if kw == "type" {
                    let kw_span = self.bump().span;
                    let (assoc_name, name_span) = self.expect_ident("after `type` in impl body")?;
                    self.expect(TokKind::Eq, "after associated type name")?;
                    let (assoc_ty, _) = self.type_()?;
                    self.finish_stmt()?;
                    assoc_type_impls.push((assoc_name, Span::new(kw_span.start, name_span.end), assoc_ty));
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
        }))
    }

    /// S28: `impl Trait { … }` inside a struct/enum body.
    fn trait_impl_block(&mut self) -> Result<TraitImplBlock, Diagnostic> {
        self.expect_kw(TokKind::KwImpl, "to start a trait impl block")?;
        let (trait_name, trait_span) = self.expect_ident("after `impl`")?;
        self.expect(TokKind::LBrace, "to open the trait impl body")?;
        let mut methods = Vec::new();
        let mut assoc_type_impls = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            // D-LIB2: `type Name = ConcreteType;`
            if let TokKind::Ident(ref kw) = self.peek().kind.clone() {
                if kw == "type" {
                    let kw_span = self.bump().span;
                    let (assoc_name, name_span) = self.expect_ident("after `type` in impl body")?;
                    self.expect(TokKind::Eq, "after associated type name")?;
                    let (assoc_ty, _) = self.type_()?;
                    self.finish_stmt()?;
                    assoc_type_impls.push((assoc_name, Span::new(kw_span.start, name_span.end), assoc_ty));
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

    /// S28: top-level `trait Name { fn sig(self) -> T; … }`.
    pub(super) fn trait_def(&mut self, nested: bool) -> Result<TraitDef, Diagnostic> {
        let is_pub = if nested {
            false
        } else {
            matches!(self.peek().kind, TokKind::KwPub)
        };
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwTrait, "to start a trait definition")?;
        let (name, name_span) = self.expect_ident("after `trait`")?;
        self.expect(TokKind::LBrace, "to open the trait body")?;
        let mut methods = Vec::new();
        let mut assoc_types = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
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
            // D-EFF3: a trait method may carry a `#Pure` prefix declaring the
            // empty effect set as its upper bound.
            let is_pure = if self.at_pure_fn() {
                self.bump(); // `#`
                self.bump(); // `Pure`
                true
            } else {
                false
            };
            methods.push(self.trait_method_sig(is_pure)?);
        }
        self.bump();
        Ok(TraitDef {
            is_pub,
            name,
            name_span,
            assoc_types,
            methods,
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
        // D-EFF3: optional `#(Gpu)` effect bound between params and the arrow.
        let declared_effects = self.parse_opt_effect_annotation()?;
        let mut return_type = None;
        let mut is_view_return = false;
        if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            if matches!(self.peek().kind, TokKind::KwView) {
                is_view_return = true;
                self.bump();
            }
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
            is_view_return,
            span: Span::new(start.start, end),
            default_body,
            is_pure,
            declared_effects,
        })
    }

    /// S27: method inside a type body or `impl` block.
    fn method_in_type(&mut self) -> Result<Func, Diagnostic> {
        // S60 (D-CASING1 follow-on): allow `#Pure fn` on methods too; the marker
        // precedes `pub`.
        let is_pure = if self.at_pure_fn() {
            self.bump(); // `#`
            self.bump(); // `Pure`
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
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "to start a method")?;
        self.func_after_fn(is_pub, false, is_pure)
    }

    fn field(&mut self) -> Result<Field, Diagnostic> {
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        let mut is_stored_ref = false;
        let mut stored_ref_label = None;
        if matches!(self.peek().kind, TokKind::KwStored) {
            is_stored_ref = true;
            self.bump();
            if matches!(self.peek().kind, TokKind::LBracket) {
                self.bump();
                let (label, _) = self.expect_ident("inside `ref[...]`")?;
                stored_ref_label = Some(label);
                self.expect(TokKind::RBracket, "after a ref label")?;
            }
        }
        let (name, name_span) = self.expect_ident("for a field name")?;
        self.expect(TokKind::Colon, "after a field name")?;
        let (ty, ty_span) = self.type_()?;
        Ok(Field {
            is_pub,
            is_stored_ref,
            stored_ref_label,
            name,
            name_span,
            ty,
            ty_span,
        })
    }

    pub(super) fn const_def(&mut self) -> Result<ConstDef, Diagnostic> {
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
        })
    }

    // --- statements ------------------------------------------------------

    // --- distinct types --------------------------------------------------

    /// D-DIST1 (ratified 2026-06-19): true when the cursor is at `Name @= distinct` (or retired `Name :: distinct`).
    fn at_distinct_def(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(_))
            && matches!(&self.peek2().kind, TokKind::AtEq | TokKind::ColonColon)
            && matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::KW_DISTINCT)
    }

    /// D-DIST3 (ratified 2026-06-20): true when `#Numeric Name @= distinct` is at
    /// the cursor — the `#Numeric` marker followed by a distinct type declaration.
    /// Also accepts retired `Name :: distinct` form (for E0991 teaching error).
    fn at_numeric_distinct_def(&self) -> bool {
        if !matches!(&self.peek().kind, TokKind::Hash) {
            return false;
        }
        // peek2 = Numeric, peek3 = name, peek4 = @= (or retired ::), peek5 = distinct
        let peek4 = &self.toks[(self.pos + 3).min(self.toks.len() - 1)].kind;
        let peek5 = &self.toks[(self.pos + 4).min(self.toks.len() - 1)].kind;
        matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_NUMERIC)
            && matches!(&self.peek3().kind, TokKind::Ident(_))
            && matches!(peek4, TokKind::AtEq | TokKind::ColonColon)
            && matches!(peek5, TokKind::Ident(n) if n == Syntax::KW_DISTINCT)
    }

    /// D-DIST1/D-DIST3: parse `[#Numeric] Name @= distinct BaseType`.
    pub(super) fn distinct_def(&mut self, is_pub: bool) -> Result<crate::AST::DistinctDef, Diagnostic> {
        let start = self.peek().span;
        // optional `#Numeric` attribute
        let is_numeric = if matches!(&self.peek().kind, TokKind::Hash) {
            self.bump(); // consume `#`
            let (attr, _) = self.expect_ident("after `#`")?;
            if attr != Syntax::ATTR_NUMERIC {
                return Err(Diagnostic::error(
                    "E0003",
                    format!("`#{}` isn't a valid attribute on a distinct type declaration", attr),
                    "only `#Numeric` is supported before a distinct type".to_string(),
                    "write `#Numeric` before the declaration, or remove the attribute".to_string(),
                    Some(self.peek().span),
                ));
            }
            true
        } else {
            false
        };
        let (name, name_span) = self.expect_ident("as the distinct type name")?;
        // Accept `@=` (D-BIND2) or retired `::` (E0991 teaching error).
        match self.peek().kind {
            TokKind::AtEq => { self.bump(); }
            TokKind::ColonColon => {
                let span = self.peek().span;
                self.bump();
                self.diags.push(Diagnostic::error(
                    "E0991",
                    format!(
                        "`{}` is the old immutable-binding sigil — use `{}` instead",
                        Syntax::SIGIL_BIND_IMMUT_RETIRED,
                        Syntax::SIGIL_BIND_IMMUT
                    ),
                    format!(
                        "the immutable binding sigil changed from `{}` to `{}` (D-BIND2)",
                        Syntax::SIGIL_BIND_IMMUT_RETIRED,
                        Syntax::SIGIL_BIND_IMMUT
                    ),
                    format!(
                        "write `{} {} {} BaseType` instead of `{} {} BaseType`",
                        name,
                        Syntax::SIGIL_BIND_IMMUT,
                        Syntax::KW_DISTINCT,
                        name,
                        Syntax::SIGIL_BIND_IMMUT_RETIRED
                    ),
                    Some(span),
                ));
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
                    format!("write: {} {} {} Int", name, Syntax::SIGIL_BIND_IMMUT, Syntax::KW_DISTINCT),
                    Some(self.peek().span),
                ));
            }
        }
        // consume `distinct` keyword
        match &self.peek().kind {
            TokKind::Ident(n) if n == Syntax::KW_DISTINCT => { self.bump(); }
            other => {
                let other = other.clone();
                return Err(Diagnostic::error(
                    "E0003",
                    format!("expected `{}` here, found {}", Syntax::KW_DISTINCT, describe(&other)),
                    format!("a distinct type declaration is `Name {} {} BaseType`", Syntax::SIGIL_BIND_IMMUT, Syntax::KW_DISTINCT),
                    format!("write: {} {} {} Int", name, Syntax::SIGIL_BIND_IMMUT, Syntax::KW_DISTINCT),
                    Some(self.peek().span),
                ));
            }
        }
        let (base, base_ty_span) = self.type_()?;
        let base_span = base_ty_span;
        self.expect(TokKind::Semi, "after a distinct type declaration")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::DistinctDef {
            is_pub,
            is_numeric,
            name,
            name_span,
            base,
            base_span,
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

    /// D-REPRC1: parse `#layout(variant) [pub] struct Name { … }`.
    /// Only `c` is supported; `packed`, `align`, `columnar` parse-and-error.
    fn layout_struct_def(&mut self, outer_is_pub: bool) -> Result<crate::AST::StructDef, Diagnostic> {
        let attr_start = self.peek().span;
        self.bump(); // consume `#`
        let (attr_name, attr_name_span) = self.expect_ident("after `#`")?;
        debug_assert_eq!(attr_name, Syntax::ATTR_LAYOUT);
        self.expect(TokKind::LParen, "after `#layout`")?;
        let (variant, variant_span) = self.expect_ident("inside `#layout(…)`")?;
        let layout = match variant.as_str() {
            v if v == Syntax::LAYOUT_C => Some(crate::AST::StructLayout::C),
            v if v == Syntax::LAYOUT_PACKED
                || v == Syntax::LAYOUT_ALIGN
                || v == Syntax::LAYOUT_COLUMNAR =>
            {
                return Err(Diagnostic::error(
                    "E1105",
                    format!("`#layout({})` is reserved and not yet supported", v),
                    "only `#layout(c)` is implemented in this release".to_string(),
                    "use `#layout(c)` for C-compatible layout, or omit `#layout` for the default".to_string(),
                    Some(variant_span),
                ));
            }
            _ => {
                return Err(Diagnostic::error(
                    "E1105",
                    format!("`#layout({})` isn't a known layout variant", variant),
                    "the supported variants are: `c` (C-compatible layout)".to_string(),
                    "write `#layout(c)` for C-compatible layout".to_string(),
                    Some(variant_span),
                ));
            }
        };
        let attr_end = self.peek().span;
        self.expect(TokKind::RParen, "to close `#layout(…)`")?;
        let attr_span = Span::new(attr_start.start, attr_end.end);
        // Consume optional semicolons (newline-inserted) before `struct`/`pub`.
        while matches!(&self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        let is_pub = outer_is_pub || if matches!(&self.peek().kind, TokKind::KwPub) {
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

    /// D-MIGRATE1 (ratified 2026-06-22): true when `#PublishedSchema struct` or
    /// `#PublishedSchema pub struct` is at the cursor.
    /// Note: the lexer inserts a `Semi` after an identifier at end-of-line, so the
    /// token stream is `# PublishedSchema [Semi] struct` — we check peek4 (pos+3)
    /// when peek3 is a `Semi`, or peek3 when the marker is on the same line.
    fn at_published_schema_struct(&self) -> bool {
        if !matches!(&self.peek().kind, TokKind::Hash) {
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

    /// D-MIGRATE1: true when `migration <TypeName> {` is at the cursor (contextual).
    fn at_migration_block(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_MIGRATION)
            && matches!(&self.peek2().kind, TokKind::Ident(_))
            && matches!(&self.peek3().kind, TokKind::LBrace)
    }

    /// D-MIGRATE1 (ratified 2026-06-22): parse `#PublishedSchema [pub] struct Name { … }`.
    fn published_schema_struct_def(&mut self, outer_is_pub: bool) -> Result<crate::AST::StructDef, Diagnostic> {
        let attr_start = self.peek().span;
        self.bump(); // consume `#`
        let (attr, attr_name_span) = self.expect_ident("after `#`")?;
        if attr != Syntax::ATTR_PUBLISHED_SCHEMA {
            return Err(Diagnostic::error(
                "E0003",
                format!("`#{}` isn't a valid attribute on a struct declaration", attr),
                "only `#PublishedSchema` is supported here".to_string(),
                "write `#PublishedSchema` before the struct".to_string(),
                Some(attr_name_span),
            ));
        }
        let attr_span = Span::new(attr_start.start, attr_name_span.end);
        // The lexer may insert a `Semi` after the marker identifier when `struct` is
        // on the next line. Consume it so the next token is `struct` or `pub`.
        while matches!(&self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        // optional `pub` after the marker
        let is_pub = outer_is_pub || if matches!(&self.peek().kind, TokKind::KwPub) {
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
        self.expect_kw(TokKind::KwStruct, "to start a struct definition")?;
        let (name, name_span) = self.expect_ident("after `struct`")?;
        let type_params = self.parse_opt_type_params()?;
        self.expect(TokKind::LBrace, "to open the struct body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut trait_impls = Vec::new();
        let mut derives = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            if matches!(self.peek().kind, TokKind::KwDerive) {
                derives.push(self.derive_line()?);
            } else if matches!(self.peek().kind, TokKind::KwImpl) {
                trait_impls.push(self.trait_impl_block()?);
            } else {
                let is_method = matches!(self.peek().kind, TokKind::KwFn)
                    || (matches!(self.peek().kind, TokKind::KwPub)
                        && matches!(self.peek2().kind, TokKind::KwFn));
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
            name,
            name_span,
            type_params,
            fields,
            methods,
            trait_impls,
            derives,
            is_published_schema: false,
            published_schema_span: None,
            layout: None,
            layout_span: None,
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
                    self.expect(TokKind::Arrow, "between the old and new field names in `rename`")?;
                    let (to, to_span) = self.expect_ident("as the new field name")?;
                    ops.push(crate::AST::MigrationOp::Rename { from, from_span, to, to_span });
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
                        field, field_span, ty, ty_span, default, default_span,
                    });
                }
                // D-MIGRATE2D: `remove field`.
                TokKind::Ident(kw) if kw == Syntax::KW_REMOVE => {
                    let (field, field_span) = self.expect_ident("as the field to remove")?;
                    ops.push(crate::AST::MigrationOp::Remove { field, field_span });
                }
                // D-MIGRATE2E: `change field: Old -> New [via { expr }]`.
                TokKind::Ident(kw) if kw == Syntax::KW_CHANGE => {
                    let (field, field_span) = self.expect_ident("as the field whose type changes")?;
                    self.expect(TokKind::Colon, "after the changed field name")?;
                    let (from_ty, from_span) = self.type_()?;
                    self.expect(TokKind::Arrow, "between the old and new field types in `change`")?;
                    let (to_ty, to_span) = self.type_()?;
                    // Optional `via { expr }` inline converter (D-MIGRATE2B).
                    let (converter, converter_span) = if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_VIA) {
                        let via_start = self.bump().span; // consume `via`
                        self.expect(TokKind::LBrace, "to open the `via { … }` converter body")?;
                        // Tolerate a newline-inserted `Semi` after `{`.
                        while matches!(&self.peek().kind, TokKind::Semi) { self.bump(); }
                        let body = self.expr()?;
                        while matches!(&self.peek().kind, TokKind::Semi) { self.bump(); }
                        let rbrace_end = self.peek().span.end;
                        self.expect(TokKind::RBrace, "to close the `via { … }` converter body")?;
                        (Some(body), Some(Span::new(via_start.start, rbrace_end)))
                    } else {
                        (None, None)
                    };
                    ops.push(crate::AST::MigrationOp::Change {
                        field, field_span, from_ty, from_span, to_ty, to_span, converter, converter_span,
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
                    while !matches!(&self.peek().kind, TokKind::Semi | TokKind::RBrace | TokKind::Eof) {
                        self.bump();
                    }
                }
                // D-MIGRATE2F: `reorder` → reordering needs no migration.
                TokKind::Ident(kw) if kw == Syntax::KW_REORDER_RETIRED => {
                    self.diags.push(Diagnostic::error(
                        "E0911",
                        "`reorder` isn't a migration verb — field order isn't a breaking change".to_string(),
                        "a `#PublishedSchema` record is keyed by field name, so reordering fields is safe and needs no migration".to_string(),
                        "delete the `reorder` line; just write the fields in the order you want".to_string(),
                        Some(op_tok.span),
                    ));
                    while !matches!(&self.peek().kind, TokKind::Semi | TokKind::RBrace | TokKind::Eof) {
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
                    while !matches!(&self.peek().kind, TokKind::Semi | TokKind::RBrace | TokKind::Eof) {
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
                    while !matches!(&self.peek().kind, TokKind::Semi | TokKind::RBrace | TokKind::Eof) {
                        self.bump();
                    }
                }
            }
        }
        self.expect(TokKind::RBrace, "to close the migration block")?;
        let end = self.toks[self.pos - 1].span;
        let span = Span::new(start.start, end.end);
        Ok(crate::AST::MigrationDecl { type_name, type_span, ops, span })
    }
}
