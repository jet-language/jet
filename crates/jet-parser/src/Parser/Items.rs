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
                // D-TAINT1: `#Sanitizer fn name(…)` taint-strip modifier.
                TokKind::Hash if self.at_sanitizer_fn() => self.func().map(Item::Func),
                // D-STATE1: `#State(S) fn` / `#Transition(From -> To) fn` typestate
                // markers on a free function.
                TokKind::Hash if self.at_state_fn() || self.at_transition_fn() => {
                    self.func().map(Item::Func)
                }
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
                    // D-LIN1: `pub #SingleUse struct|enum Name { … }`
                    TokKind::Hash if {
                        matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::ATTR_SINGLE_USE)
                    } => {
                        self.bump(); // consume `pub`
                        self.single_use_type_def(true)
                    }
                    // D-QUAL3: `pub #UnitFamily(name) { m, … }`
                    TokKind::Hash if {
                        matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::ATTR_UNIT_FAMILY)
                    } => {
                        self.bump(); // consume `pub`
                        self.unit_family_def(true).map(Item::UnitFamily)
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
                TokKind::KwTag => self.tag_def(false).map(Item::Tag),
                TokKind::KwImpl => self.impl_or_error_conv(),
                // D-ATTR2 / D-SERDE: `#[Codable, RenameAll(camel)] struct …`.
                TokKind::Hash if self.at_marker_list() => self.type_def_with_markers(),
                TokKind::Hash if self.at_c_module() => self.c_module().map(Item::CModule),
                TokKind::Hash if self.at_unsafe_fn() => self.unsafe_fn().map(Item::Func),
                TokKind::Hash if self.at_unit_family_def() => {
                    self.unit_family_def(false).map(Item::UnitFamily)
                }
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
                // D-LIN1: `#SingleUse struct|enum Name { … }`
                TokKind::Hash if self.at_single_use_type() => {
                    self.single_use_type_def(false)
                }
                // D-MIGRATE1: `migration TypeName { rename a -> b }`
                TokKind::Ident(n) if n == Syntax::KW_MIGRATION && self.at_migration_block() => {
                    self.migration_decl().map(Item::Migration)
                }
                // D-STATE-DECL: `state TypeName { A, B, C }`
                TokKind::Ident(n) if n == Syntax::KW_STATE_DECL && self.at_state_block() => {
                    self.state_decl(false).map(Item::StateDecl)
                }
                // D-METADERIVE1=A: `derive Trait for T { … }` — user-authored derive.
                TokKind::KwDerive => self.user_derive_def().map(Item::UserDerive),
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
                // D-NAMESPACE1=A: `namespace` is not a Jet keyword — E0323 teaching error.
                TokKind::Ident(name) if name == Syntax::FOREIGN_NAMESPACE => {
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0323",
                        "in-file grouping uses `module name { }`, not `namespace`".to_string(),
                        "Jet has one spelling for in-file grouping: a named `module` block".to_string(),
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
                    self.func_after_fn(false, false, false, false, None, None).map(Item::Func)
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

    /// S60 (D-CASING1 follow-on): true when the cursor is at `#Pure fn`/`#Pure pub`.
    pub(super) fn at_pure_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_PURE)
            && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
    }

    /// D-TAINT1: true when the cursor is at `#Sanitizer fn`/`#Sanitizer pub fn`.
    pub(super) fn at_sanitizer_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_SANITIZER)
            && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
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
    /// mirroring `pure` → `#Pure` (E0053).
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
                format!("test name must be parenthesized: `#{}(\"name\")`", Syntax::KW_TEST),
                "the name is now an argument to the marker, not a bare adjacent string".to_string(),
                format!("write: #{} (\"describes this block\") {{ ... }}", Syntax::KW_TEST),
                Some(span),
            ));
        }
        self.expect(TokKind::LParen, &format!("after `#{}`", Syntax::KW_TEST))?;
        let (name, name_span) = self.expect_test_name_str()?;
        self.expect(TokKind::RParen, &format!("to close `#{}(…)`", Syntax::KW_TEST))?;
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
                    format!("write: #{} (\"describes this test\") {{ ... }}", Syntax::KW_TEST),
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
            is_view_return = self.parse_view_return_marker();
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
        self.func_after_fn(is_pub, true, false, false, None, None)
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
        // D-TAINT1: `#Sanitizer fn` / `#Sanitizer pub fn` — the taint-strip
        // modifier (guaranteed by `at_sanitizer_fn` when present at dispatch).
        let is_sanitizer = if self.at_sanitizer_fn() {
            self.bump(); // `#`
            self.bump(); // `Sanitizer`
            true
        } else {
            false
        };
        // D-STATE1: `#State(S) fn …` / `#Transition(From -> To) fn …` typestate
        // markers. Each appears at most once before `fn`; either may precede the
        // (already-consumed) `#Pure`/`#Sanitizer` slots or follow them.
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
        self.func_with_modifiers_full(is_pure, is_sanitizer, state_requires, state_transition)
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
        self.expect(TokKind::Arrow, "between the from- and to-state in `#Transition(From -> To)`")?;
        let (to, _) = self.expect_ident("the to-state inside `#Transition(…)`")?;
        let end = self.peek().span.end;
        self.expect(TokKind::RParen, "to close `#Transition(…)`")?;
        Ok(crate::AST::StateTransition { from, to, span: Span::new(start, end) })
    }

    /// Parse a function whose purity is already known (the bare-`pure` teaching
    /// path enters here after emitting E0053 and consuming the `pure` word).
    pub(super) fn func_with_purity(&mut self, is_pure: bool) -> Result<Func, Diagnostic> {
        self.func_with_modifiers(is_pure, false)
    }

    /// Parse a function whose `#Pure`/`#Sanitizer` modifiers are already known.
    pub(super) fn func_with_modifiers(
        &mut self,
        is_pure: bool,
        is_sanitizer: bool,
    ) -> Result<Func, Diagnostic> {
        self.func_with_modifiers_full(is_pure, is_sanitizer, None, None)
    }

    /// Parse a function whose `#Pure`/`#Sanitizer` and D-STATE1 typestate markers
    /// are already known.
    pub(super) fn func_with_modifiers_full(
        &mut self,
        is_pure: bool,
        is_sanitizer: bool,
        state_requires: Option<(String, Span)>,
        state_transition: Option<crate::AST::StateTransition>,
    ) -> Result<Func, Diagnostic> {
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "to start a function definition")?;
        self.func_after_fn(is_pub, false, is_pure, is_sanitizer, state_requires, state_transition)
    }

    fn func_after_fn(
        &mut self,
        is_pub: bool,
        is_unsafe: bool,
        is_pure: bool,
        is_sanitizer: bool,
        state_requires: Option<(String, Span)>,
        state_transition: Option<crate::AST::StateTransition>,
    ) -> Result<Func, Diagnostic> {
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
        // sema, not here. D-EFF2: the same slot also admits a `#(via f)` tight
        // pass-through.
        let (declared_effects, effect_via) = self.parse_opt_func_effects()?;

        let mut return_type = None;
        let mut is_view_return = false;
        if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            is_view_return = self.parse_view_return_marker();
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
                is_sanitizer,
                declared_effects,
                effect_via,
                state_requires,
                state_transition,
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
            is_sanitizer,
            declared_effects,
            effect_via,
            state_requires,
            state_transition,
            body,
        })
    }

    /// D-EFF1 / D-QUAL1: parse an optional `#(Net, Db)` effect bound. Returns
    /// `None` when the cursor is not at `#(`. Effect names are bare idents here;
    /// sema validates them against the known effect vocabulary.
    fn parse_opt_effect_annotation(
        &mut self,
    ) -> Result<Option<Vec<(String, Span)>>, Diagnostic> {
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
                let (name, span) = self.expect_ident("for an effect name")?;
                effects.push((name, span));
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between effects in the list")?;
            }
        }
        self.expect(TokKind::RParen, "to close the effect list")?;
        Ok((Some(effects), None))
    }

    /// D-CAP7: the view-return marker after `->`. `&T` is the sigil spelling
    /// (`fn name_of(...) -> &String`). The retired `view` keyword still lexes,
    /// so route it to the E0058 teaching error and recover as a view return
    /// (S14 idiom). Returns true when a view-return marker was consumed.
    fn parse_view_return_marker(&mut self) -> bool {
        match self.peek().kind {
            TokKind::Amp => {
                self.bump();
                true
            }
            TokKind::KwView => {
                let span = self.bump().span;
                self.push_cap_keyword_teach("E0058", Syntax::KW_VIEW, Syntax::SIGIL_VIEW, span);
                true
            }
            _ => false,
        }
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
            // D-SERDE5: `#[Rename("x")] who: String` — field-level serde markers.
            if self.at_marker_list() {
                let field_markers = self.parse_marker_groups()?;
                let mut f = self.field()?;
                f.serde_markers = field_markers;
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
            layout: None,
            layout_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
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
        self.enum_def_after_pub(is_pub)
    }

    /// Parse `enum Name { … }` given that pub/is_pub was already handled. Factors
    /// out the body of `enum_def` (mirrors `struct_def_after_pub`) so the
    /// `#SingleUse enum` path can reuse it.
    fn enum_def_after_pub(&mut self, is_pub: bool) -> Result<EnumDef, Diagnostic> {
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
            // D-SERDE5/7: `#[Rename("x")]` on a variant — variant-level serde markers.
            if self.at_marker_list() {
                let variant_markers = self.parse_marker_groups()?;
                let mut v = self.variant()?;
                v.serde_markers = variant_markers;
                variants.push(v);
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
            is_single_use: false,
            single_use_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
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
            serde_markers: Vec::new(),
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
        // D-IMPLDOT1=A: `impl Type.Trait` — the LAST `.Ident` segment belongs to
        // the trait, not the type path. Parse path segments greedily, but stop
        // consuming dots when the NEXT `.Ident` is NOT followed by another `.`
        // (meaning that dot is the trait separator, not a path component).
        let (type_name, type_span) = {
            let (first, span) = self.expect_ident("after `impl`")?;
            let mut name = first;
            // Consume `.Ident` segments only when they're followed by ANOTHER `.`
            // (path continuation), leaving the final `.Ident` for the trait check.
            loop {
                if !matches!(self.peek().kind, TokKind::Dot) { break; }
                if !matches!(self.peek2().kind, TokKind::Ident(_)) { break; }
                // peek3() is the token after the candidate ident — if it's `.`,
                // this is a path component; otherwise it's the trait separator.
                if !matches!(self.peek3().kind, TokKind::Dot) { break; }
                self.bump(); // `.`
                let (part, _) = self.expect_ident("in type path after `impl`")?;
                name = format!("{name}.{part}");
            }
            (name, span)
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
        // D-IMPLDOT1=A: trait separator is `.` (`impl Type.Trait`).
        // Old `:` form emits a teaching error pointing at the dot form.
        let (trait_name, trait_span) = if matches!(self.peek().kind, TokKind::Dot) {
            self.bump();
            let (t, ts) = self.expect_ident("after `.` in `impl Type.Trait`")?;
            (Some(t), Some(ts))
        } else if matches!(self.peek().kind, TokKind::Colon) {
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
            (Some(t), Some(ts))
        } else {
            (None, None)
        };
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

    /// True when the cursor is at a `#[ … ]` bracket-marker group (D-ATTR2).
    fn at_marker_list(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(self.peek2().kind, TokKind::LBracket)
    }

    /// D-ATTR2 / D-SERDE2–8: parse one or more stacked `#[ … ]` bracket-marker
    /// groups. Each group is `#[ Name | Name(arg, …) (, …)* ]`. Returns every marker
    /// flattened in source order. Args parse as expressions (string literal, bare
    /// word, or any expression).
    fn parse_marker_groups(&mut self) -> Result<Vec<Marker>, Diagnostic> {
        let mut out = Vec::new();
        while self.at_marker_list() {
            self.bump(); // `#`
            self.bump(); // `[`
            loop {
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
                out.push(Marker {
                    name,
                    name_span,
                    args,
                    span: Span::new(name_span.start, end),
                });
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect(TokKind::RBracket, "to close a `#[…]` marker list")?;
            // A marker group may end its line; skip the synthetic terminator.
            while matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
        }
        Ok(out)
    }

    /// Split parsed markers on a struct/enum: derive-trait markers
    /// (`Codable`→`Encode`+`Decode`, `Encode`, `Decode`, `Comparable`, user traits)
    /// are pushed onto `derives`; serde *attribute* markers are returned raw for sema.
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
                // Any other name is a derive-trait (D-ATTR2: `#[Comparable]`, user traits).
                _ => derives.push((m.name.clone(), m.name_span)),
            }
        }
        serde
    }

    /// D-ATTR2: `#[markers] [pub] [#layout|#PublishedSchema] (struct|enum) …`.
    /// Parses the leading bracket markers, then the type that follows, and attaches
    /// derives + raw serde markers to it.
    pub(super) fn type_def_with_markers(&mut self) -> Result<Item, Diagnostic> {
        let markers = self.parse_marker_groups()?;
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        let item = match &self.peek().kind {
            TokKind::KwStruct => self.struct_def(false).map(Item::Struct)?,
            TokKind::KwEnum => self.enum_def(false).map(Item::Enum)?,
            TokKind::Hash
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_LAYOUT) =>
            {
                self.layout_struct_def(is_pub).map(Item::Struct)?
            }
            TokKind::Hash
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_PUBLISHED_SCHEMA) =>
            {
                self.published_schema_struct_def(is_pub).map(Item::Struct)?
            }
            other => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "a `#[…]` marker list must sit before a struct or enum, found {}",
                        describe(other)
                    ),
                    "derive markers like `#[Codable]` and serde attributes attach to a type"
                        .to_string(),
                    "write `#[Codable] struct Name { … }`".to_string(),
                    Some(self.peek().span),
                ));
            }
        };
        Ok(match item {
            Item::Struct(mut s) => {
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
        })
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

    /// D-QUAL2: `tag Name;` or `tag Name { … }` — a marker qualifier with no
    /// methods. The body is parsed permissively (it may syntactically contain
    /// method signatures so a stray method doesn't derail the parser); sema
    /// reports each method as E0732.
    pub(super) fn tag_def(&mut self, nested: bool) -> Result<TagDef, Diagnostic> {
        let is_pub = if nested {
            false
        } else {
            matches!(self.peek().kind, TokKind::KwPub)
        };
        if is_pub {
            self.bump();
        }
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
                    self.bump();
                    self.bump();
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
        // D-EFF3: optional `#(Gpu)` effect bound between params and the arrow.
        let declared_effects = self.parse_opt_effect_annotation()?;
        let mut return_type = None;
        let mut is_view_return = false;
        if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            is_view_return = self.parse_view_return_marker();
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
        // D-TAINT1: `#Sanitizer fn` is valid on methods too.
        let is_sanitizer = if self.at_sanitizer_fn() {
            self.bump(); // `#`
            self.bump(); // `Sanitizer`
            true
        } else {
            false
        };
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
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "to start a method")?;
        self.func_after_fn(is_pub, false, is_pure, is_sanitizer, state_requires, state_transition)
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
            serde_markers: Vec::new(),
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

    // --- unit families (D-QUAL3) --------------------------------------------

    /// D-QUAL3 (ratified 2026-06-24): true when `#UnitFamily(` is at the cursor.
    /// Token stream: `# UnitFamily (`.
    fn at_unit_family_def(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_UNIT_FAMILY)
            && matches!(&self.peek3().kind, TokKind::LParen)
    }

    /// D-QUAL3: parse `#UnitFamily(family) { m1, m2, … }`. Each member mints a
    /// `#Numeric` distinct type erasing to `Float` (lowered in sema/codegen).
    pub(super) fn unit_family_def(
        &mut self,
        is_pub: bool,
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
    fn layout_struct_def(&mut self, outer_is_pub: bool) -> Result<crate::AST::StructDef, Diagnostic> {
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
                let mut def = self.enum_def_after_pub(want_pub)?;
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
            // D-SERDE5: `#[Rename("x")] who: String` — field-level serde markers.
            if self.at_marker_list() {
                let field_markers = self.parse_marker_groups()?;
                let mut f = self.field()?;
                f.serde_markers = field_markers;
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

    /// D-STATE-DECL: true when `state <TypeName> {` is at the cursor (contextual).
    fn at_state_block(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_STATE_DECL)
            && matches!(&self.peek2().kind, TokKind::Ident(_))
            && matches!(&self.peek3().kind, TokKind::LBrace)
    }

    /// D-STATE-DECL (ratified 2026-06-25, option B): parse
    /// `[pub] state TypeName { A, B, C }`.
    ///
    /// The state names are comma-separated PascalCase identifiers. The block may have
    /// a trailing comma; semicolons between names are allowed for formatting flexibility.
    fn state_decl(&mut self, is_pub: bool) -> Result<crate::AST::StateDecl, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // consume `state`
        let (type_name, type_name_span) = self.expect_ident("the type name in `state TypeName { … }`")?;
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
        Ok(crate::AST::StateDecl { is_pub, type_name, type_name_span, states, span: Span::new(start.start, end) })
    }

    /// D-METADERIVE1=A: `derive Trait for T { … }` — user-authored derive block.
    fn user_derive_def(&mut self) -> Result<crate::AST::DeriveDef, Diagnostic> {
        let start = self.peek().span.start;
        self.bump(); // consume `derive`
        let (trait_name, trait_span) = self.expect_ident("after `derive`")?;
        self.expect(TokKind::KwFor, "after the trait name in `derive Trait for T`")?;
        let (type_param, _) = self.expect_ident("after `for` in `derive Trait for T`")?;
        self.expect(TokKind::LBrace, "after the type parameter in `derive Trait for T`")?;
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
