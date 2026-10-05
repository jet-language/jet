use super::super::*;

impl<'a> Parser<'a> {
    /// D-CHOOSE-TEST1=A: a pattern test with a diverging fallback is a
    /// statement binding. Keep the source subject in `Binding::init`; the
    /// refutable pattern and fallback stay together in the binding target so
    /// sema and every execution tier consume one canonical operation.
    pub(crate) fn try_refutable_test_binding(&self, expr: Expr) -> Result<Binding, Expr> {
        let names = match &expr {
            Expr::OrFallback { value, .. } => match value.as_ref() {
                Expr::PatternTest { pattern, .. } => pattern.binding_names(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        if names.is_empty() {
            return Err(expr);
        }
        let Expr::OrFallback {
            value,
            fallback,
            span,
            ..
        } = expr
        else {
            unreachable!("a refutable test binding must have a fallback")
        };
        let Expr::PatternTest {
            subject,
            pattern,
            span: pattern_span,
        } = *value
        else {
            unreachable!("a refutable test binding must test a pattern")
        };
        Ok(Binding {
            mutable: false,
            markers: Vec::new(),
            reactive_upgrade: false,
            meta: None,
            name: String::new(),
            name_span: pattern_span,
            sigil_span: None,
            pattern: Some(BindPattern::Refutable {
                pattern,
                fallback,
                names,
                span,
                synthesized: false,
            }),
            ty: None,
            ty_span: None,
            init: *subject,
            is_comptime: false,
            ct: None,
            uninit: false,
            arena_view: false,
            string_view: false,
            gc_promotion: None,
            gc_transferred: false,
        })
    }

    /// D-BIND-BARE1: a binding starting with the target (no keyword), written
    /// `name (:: | :=) expr`. The sigil chooses mutability.
    /// The retired typed form `name: Type :: expr` / `name: Type := expr`
    /// teaches E0393 (D-BIND-TYPE2=A); `name@ Type` stays a plain parse error.
    pub(in crate::Parser) fn sigil_binding(&mut self) -> Result<Binding, Diagnostic> {
        // S74: a destructuring target — `[ … ]` for a list, `Ident { … }` for a
        // struct — instead of a plain `name`.
        if let Some(pattern) = self.try_bind_pattern()? {
            let (mutable, sigil_span) = self.expect_bind_sigil()?;
            let init = self.expr()?;
            return Ok(Binding {
                mutable,
                markers: Vec::new(),
                reactive_upgrade: false,
                meta: None,
                name: String::new(),
                name_span: pattern.span(),
                sigil_span: Some(sigil_span),
                pattern: Some(pattern),
                ty: None,
                ty_span: None,
                init,
                is_comptime: false,
                ct: None,
                uninit: false,
                arena_view: false,
                string_view: false,
                gc_promotion: None,
                gc_transferred: false,
            });
        }
        let (name, name_span) = self.expect_ident("for the binding name")?;
        let retired = self.retired_const_decl_name(&name);
        let (name, retired) = match retired {
            Some(new) => (new, Some(name)),
            None => (name, None),
        };
        // D-COMPILER-NS1=A: the fact mark names compiler facts only; a
        // program's own build-time value is `NAME :: prep { value }`.
        if Syntax::is_comptime_name(&name) {
            let plain = Syntax::canonical_name_case(
                name.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '_'),
                Syntax::NameCase::Screaming,
            );
            return Err(Diagnostic::error(
                "E0003",
                format!("`{name}` is a compiler fact, not a binding name"),
                "a marked name reads a fact the compiler supplies; a program's own build-time value is an ordinary name bound with `prep { … }`".to_string(),
                format!("declare `{plain} :: {} {{ value }}`", Syntax::KW_PREP),
                Some(name_span),
            ));
        }
        if matches!(self.peek().kind, TokKind::Colon) {
            if let Some(binding) = self.retired_typed_binding(name.clone(), name_span)? {
                return Ok(binding);
            }
        }
        // Retired D-BINDEXPLICIT1 / D-BIND4 typed forms — ordinary parse error.
        // `name@ Type` and a `name:` that is not `name: Type ::` never open a
        // binding under D-BIND-BARE1.
        if matches!(self.peek().kind, TokKind::At | TokKind::Colon) {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected `{}` or `{}` in a binding, found {}",
                    Syntax::SIGIL_BIND_IMMUT,
                    Syntax::SIGIL_BIND_MUT,
                    describe(&self.peek().kind)
                ),
                format!(
                    "a binding is `name {} value` (immutable) or `name {} value` (mutable); types ride the value",
                    Syntax::SIGIL_BIND_IMMUT,
                    Syntax::SIGIL_BIND_MUT
                ),
                format!(
                    "write `name {} value` or put the type on the value (e.g. `Type{{ … }}`)",
                    Syntax::SIGIL_BIND_IMMUT
                ),
                Some(self.peek().span),
            ));
        }
        let (mutable, sigil_span) = self.expect_bind_sigil()?;
        // Bare `name := uninit` — an explicit type must precede the literal body.
        if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_UNINIT)
            && matches!(
                self.peek2().kind,
                TokKind::Semi | TokKind::RBrace | TokKind::Eof
            )
        {
            let uninit_span = self.peek().span;
            return Err(Diagnostic::error(
                "E0421",
                "`uninit` needs an explicit type".to_string(),
                "an uninitialized binding has no value to infer its type from, so the type must appear before the literal body".to_string(),
                format!(
                    "write `{name} {} <Type>{{ {} }}`, e.g. `buffer := [U8#4096]{{ {} }}`",
                    Syntax::SIGIL_BIND_MUT,
                    Syntax::KW_UNINIT,
                    Syntax::KW_UNINIT
                ),
                Some(uninit_span),
            ));
        }
        let (init, prepared) = self.binding_value(mutable)?;
        if let Some(old) = &retired {
            self.retire_const_decl(old, &name, name_span, &init, prepared, mutable);
        }
        // D-UNINIT-SENTINEL2: `name := Type{ uninit }` — uninit only as a
        // whole typed-literal body. Mutable bindings only (`:=`).
        if mutable {
            if let Some((ty, ty_span, marker_span)) = typed_lit_uninit_head(&init) {
                return Ok(Binding {
                    mutable: true,
                    markers: Vec::new(),
                    reactive_upgrade: false,
                    meta: None,
                    name,
                    name_span,
                    sigil_span: Some(sigil_span),
                    pattern: None,
                    ty: Some(ty),
                    ty_span: Some(ty_span),
                    init: Expr::Int(0, marker_span, None, None),
                    is_comptime: false,
                    ct: None,
                    uninit: true,
                    arena_view: false,
                    string_view: false,
                    gc_promotion: None,
                    gc_transferred: false,
                });
            }
        }
        Ok(Binding {
            mutable,
            markers: Vec::new(),
            reactive_upgrade: false,
            meta: None,
            // D-PREP-SURFACE2=A (owner ruling, 2026-09-30): compile time is
            // always explicit. Only `name :: prep { value }` is evaluated
            // while building; an ALL_CAPS name is an ordinary runtime value.
            // A retired `@name :: value` recovers with its old meaning.
            is_comptime: prepared || (retired.is_some() && !mutable),
            name,
            name_span,
            sigil_span: Some(sigil_span),
            pattern: None,
            ty: None,
            ty_span: None,
            init,
            ct: None,
            uninit: false,
            arena_view: false,
            string_view: false,
            gc_promotion: None,
            gc_transferred: false,
        })
    }

    /// D-BIND-TYPE2=A: `name: Type :: value` (or `:=`) teaches E0393 with a
    /// behavior-preserving edit to `name :: Type{value}`, then parses on as the
    /// untyped binding so one mistake reports once. Returns `None` with the
    /// cursor restored when the tokens after `:` are not `Type` and a sigil.
    fn retired_typed_binding(
        &mut self,
        name: String,
        name_span: Span,
    ) -> Result<Option<Binding>, Diagnostic> {
        let save = self.pos;
        let save_diags = self.diags.len();
        let colon_span = self.bump().span;
        let parsed = self.type_();
        let Ok((ty, ty_span)) = parsed else {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        };
        if self.diags.len() != save_diags
            || !matches!(self.peek().kind, TokKind::ColonColon | TokKind::ColonEq)
        {
            self.pos = save;
            self.diags.truncate(save_diags);
            return Ok(None);
        }
        let (mutable, sigil_span) = self.expect_bind_sigil()?;
        // The value's source text runs from its first token to the last one
        // parsed; `init.span()` is only the head of some forms (a method
        // call's span is its method name), which truncated the fix text.
        let value_start = self.peek().span.start;
        let init = self.expr()?;
        let value_span = Span::new(value_start, self.toks[self.pos - 1].span.end);
        let sigil = if mutable {
            Syntax::SIGIL_BIND_MUT
        } else {
            Syntax::SIGIL_BIND_IMMUT
        };
        let type_text = self.source_fragment(ty_span).unwrap_or_else(|| ty.name());
        let diagnostic = |replacement: &str| {
            Diagnostic::from_row(
                "E0393",
                &[
                    ("name", name.as_str()),
                    ("type", type_text.as_str()),
                    ("replacement", replacement),
                ],
                Some(Span::new(colon_span.start, ty_span.end)),
            )
        };
        let report = match self.source_fragment(value_span) {
            Some(value_text) => {
                let replacement = format!("{name} {sigil} {type_text}{{{value_text}}}");
                diagnostic(&replacement).with_edit(crate::Diagnostics::TextEdit {
                    span: Span::new(name_span.start, value_span.end),
                    new_text: replacement,
                })
            }
            None => diagnostic(&format!("{name} {sigil} {type_text}{{ … }}")),
        };
        self.diags.push(report);
        Ok(Some(Binding {
            mutable,
            markers: Vec::new(),
            reactive_upgrade: false,
            meta: None,
            is_comptime: false,
            name,
            name_span,
            sigil_span: Some(sigil_span),
            pattern: None,
            ty: None,
            ty_span: None,
            init,
            ct: None,
            uninit: false,
            arena_view: false,
            string_view: false,
            gc_promotion: None,
            gc_transferred: false,
        }))
    }

    /// D-BIND-BARE1: consume `::` (immutable) or `:=` (mutable), returning the
    /// mutability and exact source span for the consumed sigil.
    pub(super) fn expect_bind_sigil(&mut self) -> Result<(bool, Span), Diagnostic> {
        match self.peek().kind {
            TokKind::ColonColon => {
                let span = self.bump().span;
                Ok((false, span))
            }
            TokKind::ColonEq => {
                let span = self.bump().span;
                Ok((true, span))
            }
            _ => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected `{}` or `{}` in a binding, found {}",
                    Syntax::SIGIL_BIND_IMMUT,
                    Syntax::SIGIL_BIND_MUT,
                    describe(&self.peek().kind)
                ),
                format!(
                    "a binding is `name {} value` (immutable) or `name {} value` (mutable)",
                    Syntax::SIGIL_BIND_IMMUT,
                    Syntax::SIGIL_BIND_MUT
                ),
                format!("write `name {} value`", Syntax::SIGIL_BIND_IMMUT),
                Some(self.peek().span),
            )),
        }
    }

    /// D-BIND-BARE1: true when the tokens at the cursor begin a sigil binding —
    /// `name ::`, `name :=`, or a destructuring pattern target.
    /// Also matches retired `name : …` so sigil_binding can teach E0393
    /// (D-BIND-TYPE2=A) or report the ordinary parse error.
    /// Used by the statement dispatcher to tell a binding apart from an
    /// expression/assignment that also starts with a name.
    pub(in crate::Parser) fn looks_like_sigil_binding(&self) -> bool {
        match &self.peek().kind {
            // `name :: e` / `name := e`
            TokKind::Ident(_) | TokKind::KwSelf
                if matches!(self.peek2().kind, TokKind::ColonColon | TokKind::ColonEq) =>
            {
                true
            }
            // Retired typed form `name : Type ::/:= …` — still recognized so
            // sigil_binding can teach it with E0393 (D-BIND-TYPE2=A).
            TokKind::Ident(_) | TokKind::KwSelf if matches!(self.peek2().kind, TokKind::Colon) => {
                true
            }
            // Destructuring targets: scan ahead to a `::`/`:=` after the matching
            // close. Cheap bounded lookahead. `[ … ] ::`, `( … ) ::`,
            // and `Type{ … } ::` (plus the retired dotted migration form).
            TokKind::LBracket | TokKind::LParen => self.pattern_target_is_binding(),
            TokKind::Ident(_) if matches!(self.peek2().kind, TokKind::LBrace) => {
                self.pattern_target_is_binding()
            }
            // D-LIT-DOT1 migration arm: retired `Type.{ … } ::`.
            TokKind::Ident(_)
                if matches!(self.peek2().kind, TokKind::Dot)
                    && matches!(self.peek3().kind, TokKind::LBrace) =>
            {
                self.pattern_target_is_binding_dot()
            }
            _ => false,
        }
    }

    /// Scan a `[ … ]` / `( … )` / `Ident { … }` destructuring target and check
    /// whether a binding sigil follows its close. Bounded by the bracket depth.
    pub(super) fn pattern_target_is_binding(&self) -> bool {
        let mut i = self.pos;
        let n = self.toks.len();
        // skip a leading `Ident` for the struct-pattern form.
        if matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Ident(_))) {
            i += 1;
        }
        let (open, close) = match self.toks.get(i).map(|t| &t.kind) {
            Some(TokKind::LBracket) => (TokKind::LBracket, TokKind::RBracket),
            Some(TokKind::LBrace) => (TokKind::LBrace, TokKind::RBrace),
            Some(TokKind::LParen) => (TokKind::LParen, TokKind::RParen),
            _ => return false,
        };
        let mut depth = 0usize;
        while i < n {
            let k = &self.toks[i].kind;
            if std::mem::discriminant(k) == std::mem::discriminant(&open) {
                depth += 1;
            } else if std::mem::discriminant(k) == std::mem::discriminant(&close) {
                depth -= 1;
                if depth == 0 {
                    return matches!(
                        self.toks.get(i + 1).map(|t| &t.kind),
                        Some(TokKind::ColonColon | TokKind::ColonEq)
                    );
                }
            }
            i += 1;
        }
        false
    }

    /// D-LIT-DOT1: like `pattern_target_is_binding` but skips `Ident Dot` before
    /// the opening `{`, for the retired `Type.{ … } :: expr` migration form.
    pub(super) fn pattern_target_is_binding_dot(&self) -> bool {
        let mut i = self.pos;
        let n = self.toks.len();
        // Skip the type name (Ident).
        if matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Ident(_))) {
            i += 1;
        }
        // Skip the dot.
        if matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Dot)) {
            i += 1;
        }
        // Now expect `{` to open the struct pattern body.
        let (open, close) = match self.toks.get(i).map(|t| &t.kind) {
            Some(TokKind::LBrace) => (TokKind::LBrace, TokKind::RBrace),
            _ => return false,
        };
        let mut depth = 0usize;
        while i < n {
            let k = &self.toks[i].kind;
            if std::mem::discriminant(k) == std::mem::discriminant(&open) {
                depth += 1;
            } else if std::mem::discriminant(k) == std::mem::discriminant(&close) {
                depth -= 1;
                if depth == 0 {
                    return matches!(
                        self.toks.get(i + 1).map(|t| &t.kind),
                        Some(TokKind::ColonColon | TokKind::ColonEq)
                    );
                }
            }
            i += 1;
        }
        false
    }

    /// S74: parse a destructuring binding target if one starts here.
    /// `[ a, b ]` is a list pattern; `Ident { x, y }` is a struct pattern.
    /// A bare `name` (followed by `=` or `:`) is not a pattern.
    pub(super) fn try_bind_pattern(&mut self) -> Result<Option<BindPattern>, Diagnostic> {
        match &self.peek().kind {
            TokKind::LBracket => {
                let start = self.bump().span;
                let mut elems = Vec::new();
                if !matches!(self.peek().kind, TokKind::RBracket) {
                    loop {
                        let (name, span) = self.expect_ident("for a list-pattern binding")?;
                        elems.push(BindName {
                            name,
                            span,
                            rename: None,
                        });
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                }
                let end = self.peek().span;
                self.expect(TokKind::RBracket, "to close the list pattern")?;
                Ok(Some(BindPattern::List {
                    elems,
                    span: Span::new(start.start, end.end),
                }))
            }
            // Retired `Type.{ x, y }`. Fmt rewrites; compile rejects.
            TokKind::Ident(_)
                if matches!(self.peek2().kind, TokKind::Dot)
                    && matches!(self.peek3().kind, TokKind::LBrace) =>
            {
                let (type_name, type_span) = self.expect_ident("for a struct pattern")?;
                let dot_span = self.peek().span;
                self.expect(TokKind::Dot, "in a struct pattern")?;
                let diagnostic = Diagnostic::error(
                    "E0320",
                    format!(
                        "struct pattern uses `{}{{…}}`, not `{}.{{…}}`",
                        type_name, type_name
                    ),
                    "literal heads place no dot before their brace (D-LIT-DOT1)".to_string(),
                    format!("write `{}{{…}}`", type_name),
                    Some(dot_span),
                );
                if !self.migration_mode {
                    return Err(diagnostic);
                }
                self.diags.push(diagnostic);
                self.expect(TokKind::LBrace, "to open the struct pattern")?;
                let (fields, rest) = self.struct_pattern_fields()?;
                let end = self.peek().span;
                self.expect(TokKind::RBrace, "to close the struct pattern")?;
                let close_span = Span::new(type_span.start, end.end);
                Ok(Some(BindPattern::Struct {
                    type_name,
                    type_span,
                    fields,
                    rest,
                    span: close_span,
                }))
            }
            TokKind::Ident(_) if matches!(self.peek2().kind, TokKind::LBrace) => {
                // D-LIT-DOT1: canonical `Type{ x, y }` struct pattern.
                let (type_name, type_span) = self.expect_ident("for a struct pattern")?;
                self.expect(TokKind::LBrace, "to open the struct pattern")?;
                let (fields, rest) = self.struct_pattern_fields()?;
                let end = self.peek().span;
                self.expect(TokKind::RBrace, "to close the struct pattern")?;
                let close_span = Span::new(type_span.start, end.end);
                Ok(Some(BindPattern::Struct {
                    type_name,
                    type_span,
                    fields,
                    rest,
                    span: close_span,
                }))
            }
            TokKind::LParen if !self.looks_like_named_tuple(false) => {
                let start = self.bump().span;
                let mut elems = Vec::new();
                if !matches!(self.peek().kind, TokKind::RParen) {
                    loop {
                        let (name, span) = self.expect_ident("for a tuple-pattern binding")?;
                        elems.push(BindName {
                            name,
                            span,
                            rename: None,
                        });
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                }
                let end = self.peek().span;
                self.expect(TokKind::RParen, "to close the tuple pattern")?;
                Ok(Some(BindPattern::Tuple {
                    elems,
                    span: Span::new(start.start, end.end),
                }))
            }
            _ => Ok(None),
        }
    }

    /// D-DESTRUCT1: the field list inside a struct destructure's `{ … }` —
    /// `field` (bind same name), `field: name` (rename), and an optional
    /// trailing `..` (rest marker, `OP_RANGE`). Shared by the binding-position
    /// `Type{ … }` pattern and its E0320-recovery dotted spelling.
    pub(super) fn struct_pattern_fields(
        &mut self,
    ) -> Result<(Vec<BindName>, Option<Span>), Diagnostic> {
        let mut fields = Vec::new();
        let mut rest = None;
        if !matches!(self.peek().kind, TokKind::RBrace) {
            loop {
                if matches!(self.peek().kind, TokKind::DotDot) {
                    rest = Some(self.bump().span);
                    break;
                }
                let (name, span) = self.expect_ident("for a struct-pattern field")?;
                let rename = if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    let (rn, rs) = self.expect_ident("as the renamed binding")?;
                    Some((rn, rs))
                } else {
                    None
                };
                fields.push(BindName { name, span, rename });
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    if matches!(self.peek().kind, TokKind::RBrace) {
                        break;
                    }
                    continue;
                }
                break;
            }
        }
        Ok((fields, rest))
    }

    /// D-DESTRUCT1: `..` is mandatory whenever the pattern doesn't name every
    /// field of `type_name` (E0326), and redundant when it does (E0327). The
    /// parser doesn't know the struct's full field list (that's sema's job),
    /// so this only catches the REDUNDANT case structurally — cases genuinely
    /// requiring the struct's field count are re-checked in sema, which has
    /// the registry. A `..` present with zero named fields is never redundant.
    /// D-PREP-SURFACE2=A: parse the explicit shared-preparation block
    /// `prep { … }`. The retired `@ { … }` and `comptime { … }` heads recover
    /// through `take_mark`, which teaches their replacement. Erases at codegen
    /// (build-time only).
    pub(super) fn comptime_block_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.take_mark()?;
        self.expect(TokKind::LBrace, "to open the `prep` block body")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ComptimeBlock {
            body,
            is_template_loop: false,
            span: Span::new(start.start, end),
        })
    }

    /// D-PREP-BRANCH1=A (respelling D-META-STAGE1=B / D-ONCE-AT1=D): `prep loop …`
    /// is the ratified `loop` verb at compile time, not a second iteration
    /// form. It is one compile-time block holding one loop, so it folds
    /// through the same path as `prep { … }` and emits no runtime code.
    pub(super) fn comptime_loop_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.take_mark()?;
        let body = self.loop_stmt(None)?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ComptimeBlock {
            body: vec![body],
            is_template_loop: self.derive_template_depth > 0,
            span: Span::new(start.start, end),
        })
    }

    /// D-PREP-BRANCH1=A (respelling D-VERDICT-1308-2): parse
    /// `prep if <cond> { … } else { … }`.
    /// Both arms require `{ }` (braceless bodies are not allowed for `prep if`).
    /// `else` is optional in statement position. Sema selects the arm; codegen
    /// emits only the selected arm (D-WHEN2: dropped arm is name-resolved only).
    pub(super) fn comptime_if_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.take_mark()?;
        self.bump(); // `if`

        // D-OSTARGET2=B (ratified 2026-07-03): the dispatch form
        // `prep if $build.os == { .Linux -> … .MacOS -> … }`. Detected the
        // same way `if_or_dispatch` does — parse the subject below comparison
        // precedence so a trailing `== {` marker survives; reuse `if_arms` for
        // the arm grammar, then repackage the resulting `Stmt::Switch` as a
        // `Stmt::ComptimeSwitch` (sema folds it to the active-OS arm).
        let probe = self.pos;
        let probe_diags = self.diags.len();
        if let Ok(subject) = self.expr_no_struct_lit_no_cmp() {
            if matches!(self.peek().kind, TokKind::EqEq)
                && matches!(self.peek2().kind, TokKind::LBrace)
            {
                self.bump(); // `==`
                self.expect(TokKind::LBrace, "to open the `prep if` dispatch body")?;
                let switch = self.if_arms(subject, start, BinOp::Eq)?;
                let Stmt::Switch {
                    subject,
                    arms,
                    else_body,
                    ..
                } = switch
                else {
                    unreachable!("if_arms always returns Stmt::Switch");
                };
                let end = self.toks[self.pos - 1].span.end;
                return Ok(Stmt::ComptimeSwitch {
                    subject,
                    arms,
                    else_body,
                    span: Span::new(start.start, end),
                });
            }
        }
        // Not the dispatch form — rewind and parse the boolean `prep if`.
        self.pos = probe;
        self.diags.truncate(probe_diags);

        let cond_start = self.peek().span;
        let cond = self.expr_no_struct_lit()?;
        let cond_span = Span::new(cond_start.start, self.toks[self.pos - 1].span.end);
        self.expect(TokKind::LBrace, "to open the `prep if` body")?;
        let then_body = self.block_stmts();
        let else_body = if matches!(self.peek().kind, TokKind::KwElse) {
            self.bump();
            // Allow `else prep if` chained with another compile-time branch;
            // the retired `@if` / `comptime if` / `#Known if` heads recover
            // through `take_mark` and teach their replacement.
            if self.at_prep_verb(&TokKind::KwIf)
                || (matches!(self.peek().kind, TokKind::At | TokKind::KwComptime)
                    && matches!(self.peek2().kind, TokKind::KwIf))
                || (self.at_known_lead() && matches!(self.peek3().kind, TokKind::KwIf))
            {
                let chain = self.comptime_if_stmt()?;
                Some(vec![chain])
            } else {
                self.expect(TokKind::LBrace, "to open the `prep if` else body")?;
                Some(self.block_stmts())
            }
        } else {
            None
        };
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ComptimeIf {
            cond,
            cond_span,
            then_body,
            else_body,
            span: Span::new(start.start, end),
            selected_then: None,
        })
    }

    pub(super) fn comptime_binding(&mut self) -> Result<Binding, Diagnostic> {
        let retired = matches!(self.peek().kind, TokKind::KwComptime);
        // D-META-STAGE1=B: the mark rides the name, so nothing is consumed
        // before the name. A retired spelling is taken here only to teach.
        if retired || self.at_known_lead() {
            self.take_mark()?;
        }
        let (name, name_span) = self.expect_ident("for the compile-time binding")?;
        let sigil_span = if retired {
            self.expect(TokKind::Eq, "in the retired comptime binding")?;
            None
        } else {
            let span = self.peek().span;
            self.expect(TokKind::ColonColon, "after a compile-time binding name")?;
            Some(span)
        };
        let init = self.expr()?;
        Ok(Binding {
            mutable: false,
            markers: Vec::new(),
            reactive_upgrade: false,
            meta: None,
            name,
            name_span,
            sigil_span,
            pattern: None,
            ty: None,
            ty_span: None,
            init,
            is_comptime: true,
            ct: None,
            uninit: false,
            arena_view: false,
            string_view: false,
            gc_promotion: None,
            gc_transferred: false,
        })
    }

    /// D-META-STAGE1=B: true when the retired `#Known` spelling is at the
    /// cursor. It parses only far enough to teach the `@` form.
    pub(in crate::Parser) fn at_known_lead(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(name) if name == Syntax::RETIRED_MARKER_KNOWN)
    }

    /// D-META-STAGE1=B: `#Known` is retired. One teaching error covers all
    /// three of its forms: a constant is `NAME :: prep { value }`, and a
    /// build-time branch or block carries `prep` (D-PREP-SURFACE2=A).
    pub(in crate::Parser) fn retired_known_error(&self, span: Span, fix: String) -> Diagnostic {
        Diagnostic::error(
            "E0377",
            format!("`#{}` is retired", Syntax::RETIRED_MARKER_KNOWN),
            "compile-time work carries the word `prep`, and a compile-time constant is `NAME :: prep { value }`"
                .to_string(),
            fix,
            Some(span),
        )
    }

    /// D-PREP-BRANCH1=A / D-PREP-FN1=A: `prep` leads a build-time verb only
    /// when the verb keyword follows it on the same line. The lexer never
    /// reserves `prep`, so it stays an ordinary name everywhere else.
    pub(in crate::Parser) fn at_prep_verb(&self, verb: &TokKind) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(name) if name == Syntax::KW_PREP)
            && self.peek2().kind == *verb
    }

    /// D-PREP-SURFACE2=A (owner ruling, 2026-09-30): the value after a
    /// binding sigil, and whether it is prepared. Compile time is always
    /// explicit: `name :: prep { value }` — a block holding one final
    /// expression — is evaluated while building, at module and block scope
    /// alike; any other value is an ordinary runtime value, whatever the
    /// name's case. Any other `prep` block keeps E0391.
    pub(in crate::Parser) fn binding_value(&mut self, mutable: bool) -> Result<(Expr, bool), Diagnostic> {
        if mutable || !self.at_prep_verb(&TokKind::LBrace) {
            return Ok((self.expr()?, false));
        }
        // A binding statement directly inside another prepared value block
        // means that block was never closed (a value block holds a single
        // expression, never a binding). Hand the binding back to the enclosing
        // binding's parse, which reports the unclosed block; nesting every
        // later `name :: prep { … }` inside the first one recursed without
        // bound and overflowed the stack.
        if let Some((_, statements_depth)) = self.prep_value_block {
            let binding_start = self.pos.checked_sub(2).filter(|name| {
                matches!(self.toks[*name].kind, TokKind::Ident(_))
                    && matches!(self.toks[name + 1].kind, TokKind::ColonColon)
            });
            if let Some(binding_start) = binding_start.filter(|_| statements_depth == self.block_depth) {
                self.prep_value_escape = Some(binding_start);
                return Err(Diagnostic::from_row("E0391", &[], Some(self.peek().span)));
            }
        }
        let start = self.bump().span.start;
        let open_span = self.bump().span; // `{`
        // The block's final expression is its value, as in a callable body,
        // so a bare literal tail is accepted here.
        let saved_tail = (self.callable_tail_block_depth, self.callable_tail_expects_value);
        self.callable_tail_block_depth = Some(self.block_depth + 1);
        self.callable_tail_expects_value = true;
        let saved_block = self.prep_value_block.replace((open_span, self.block_depth + 1));
        let diags_before = self.diags.len();
        let mut body = self.block_stmts();
        self.prep_value_block = saved_block;
        (self.callable_tail_block_depth, self.callable_tail_expects_value) = saved_tail;
        if let Some(binding_start) = self.prep_value_escape.take() {
            // The block's contents were read against the wrong end, so their
            // own reports are noise; the unclosed `{` is the one finding.
            self.diags.truncate(diags_before);
            return Ok((self.unclosed_prep_value(open_span, binding_start), true));
        }
        let end = self.toks[self.pos - 1].span.end;
        match (body.pop(), body.is_empty()) {
            (Some(Stmt::Expr(value)), true) => Ok((value, true)),
            _ => Err(Diagnostic::from_row("E0391", &[], Some(Span::new(start, end)))),
        }
    }

    /// E0083 at the `{` of a prepared value block that a later binding showed
    /// was never closed. The parse resumes at the end of the line before that
    /// binding, so it is read again as the next statement or declaration.
    fn unclosed_prep_value(&mut self, open_span: Span, binding_start: usize) -> Expr {
        self.pos = binding_start;
        if binding_start > 0 && matches!(self.toks[binding_start - 1].kind, TokKind::Semi) {
            self.pos = binding_start - 1;
        }
        let name = match &self.toks[binding_start].kind {
            TokKind::Ident(name) => name.clone(),
            _ => String::new(),
        };
        let cutoff = format!("the binding `{name}`");
        let diagnostic = self
            .unclosed_report(open_span, "{", "}", &cutoff, "prepared value")
            .unwrap_or_else(|| {
                Diagnostic::error(
                    "E0003",
                    format!("expected `}}` to close this block, found {cutoff}"),
                    "every `{` needs a matching `}`".to_string(),
                    "add a closing `}`".to_string(),
                    Some(open_span),
                )
            });
        self.diags.push(diagnostic);
        Expr::Unit(open_span)
    }

    /// D-PREP-BRANCH1=A / D-PREP-FN1=A: the retired `@if`, `@loop` and `@fn`
    /// heads. The caller has seen `@` followed by `verb`; this consumes the
    /// `@`, teaches E0388 with the machine-applicable respelling, and leaves
    /// the verb for the caller to parse.
    pub(in crate::Parser) fn retired_at_verb(&mut self, verb: &str) -> Span {
        let at = self.bump().span;
        let head = Span::new(at.start, self.peek().span.end);
        let old = format!("@{verb}");
        let new = format!("{} {verb}", Syntax::KW_PREP);
        self.diags.push(
            Diagnostic::from_row("E0388", &[("old", old.as_str()), ("new", new.as_str())], Some(head)).with_edit(
                crate::Diagnostics::TextEdit {
                    span: at,
                    new_text: format!("{} ", Syntax::KW_PREP),
                },
            ),
        );
        at
    }

    /// D-PREP-SURFACE2=A: the retired `@ { … }` block head. The caller has
    /// seen `@` followed by `{`; this consumes the `@` and teaches E0388 with
    /// the machine-applicable respelling `prep { … }`.
    fn retired_at_block(&mut self) -> Span {
        let at = self.bump().span;
        let head = Span::new(at.start, self.peek().span.end);
        let new = format!("{} {{ … }}", Syntax::KW_PREP);
        self.diags.push(
            Diagnostic::from_row("E0388", &[("old", "@ { … }"), ("new", new.as_str())], Some(head)).with_edit(
                crate::Diagnostics::TextEdit {
                    span: at,
                    new_text: Syntax::KW_PREP.to_string(),
                },
            ),
        );
        at
    }

    /// D-PREP-SURFACE2=A (owner ruling, 2026-09-30): a module constant is an
    /// ALL_CAPS name and a block-scope binding a snake_case local
    /// (D-SHAPE-CASE1); compile time is always the explicit
    /// `name :: prep { value }`. The respelling of a retired `@name`, or
    /// `None` when the name carries no retired mark. A read of an ALL_CAPS
    /// name keeps it, since it names a module constant.
    fn retired_const_name(&self, name: &str, read: bool) -> Option<String> {
        let new = Syntax::retired_constant_respelling(name)?;
        let rest = &name[Syntax::RETIRED_COMPTIME_MARK.len()..];
        if self.block_depth == 0 || (read && Syntax::is_constant_name(rest)) {
            return Some(new);
        }
        Some(Syntax::canonical_name_case(&new, Syntax::NameCase::Snake))
    }

    /// A retired `@name` read teaches E0388 with the behavior-preserving
    /// respelling and the parse continues under the plain name, so each
    /// mention reports once and nothing cascades.
    pub(in crate::Parser) fn retire_const_mark(&mut self, name: String, span: Span) -> String {
        let Some(new) = self.retired_const_name(&name, true) else {
            return name;
        };
        self.diags.push(
            Diagnostic::from_row("E0388", &[("old", name.as_str()), ("new", new.as_str())], Some(span))
                .with_edit(crate::Diagnostics::TextEdit {
                    span,
                    new_text: new.clone(),
                }),
        );
        new
    }

    /// The respelled name of a retired `@name` declaration, or `None`.
    pub(in crate::Parser) fn retired_const_decl_name(&self, name: &str) -> Option<String> {
        self.retired_const_name(name, false)
    }

    /// A retired `@name :: value` declaration teaches E0388. The mark always
    /// meant compile time, so the behavior-preserving edit is
    /// `name :: prep { value }`; a value already written `prep { … }` (or a
    /// mutable binding, which `prep` cannot bind) only loses the mark.
    pub(in crate::Parser) fn retire_const_decl(
        &mut self,
        old: &str,
        new: &str,
        name_span: Span,
        value: &Expr,
        prepared: bool,
        mutable: bool,
    ) {
        let diagnostic = |replacement: &str| {
            Diagnostic::from_row("E0388", &[("old", old), ("new", replacement)], Some(name_span))
        };
        let report = if prepared || mutable {
            diagnostic(new).with_edit(crate::Diagnostics::TextEdit {
                span: name_span,
                new_text: new.to_string(),
            })
        } else {
            let value_span = value.span();
            match self.source_fragment(value_span) {
                Some(text) => {
                    let replacement = format!(
                        "{new} {} {} {{ {text} }}",
                        Syntax::SIGIL_BIND_IMMUT,
                        Syntax::KW_PREP
                    );
                    diagnostic(&replacement).with_edit(crate::Diagnostics::TextEdit {
                        span: Span::new(name_span.start, value_span.end),
                        new_text: replacement.clone(),
                    })
                }
                None => diagnostic(&format!(
                    "{new} {} {} {{ … }}",
                    Syntax::SIGIL_BIND_IMMUT,
                    Syntax::KW_PREP
                )),
            }
        };
        self.diags.push(report);
    }

    /// True when a `@name` read names a retired constant: an ALL_CAPS name
    /// under the mark, or a name this file declares as `@name :: …`. Other
    /// `@` words are compiler facts and keep their own path.
    pub(in crate::Parser) fn reads_retired_constant(&self, name: &str) -> bool {
        let Some(rest) = name.strip_prefix(Syntax::RETIRED_COMPTIME_MARK) else {
            return false;
        };
        Syntax::is_constant_name(rest)
            || self.toks.windows(2).any(|pair| {
                matches!(&pair[0].kind, TokKind::Ident(declared) if declared == name)
                    && matches!(pair[1].kind, TokKind::ColonColon)
            })
    }

    /// D-META-STAGE1=B / D-PREP-BRANCH1=A / D-PREP-SURFACE2=A: consume
    /// whatever opened a compile-time construct. The ratified heads are `prep`
    /// before `if`/`loop` or a `{` block; the retired `@if`, `@loop`, `@ { … }`,
    /// `#Known` and `comptime` spellings are recovered here so each teaches
    /// its replacement once.
    fn take_mark(&mut self) -> Result<Span, Diagnostic> {
        if self.at_prep_verb(&TokKind::KwIf)
            || self.at_prep_verb(&TokKind::KwLoop)
            || self.at_prep_verb(&TokKind::LBrace)
        {
            return Ok(self.bump().span);
        }
        if self.at_known_lead() {
            let head = self.read_marker_head()?;
            let fix = if matches!(self.peek().kind, TokKind::KwIf) {
                format!("write `{} if <condition> {{ … }}`", Syntax::KW_PREP)
            } else if matches!(self.peek().kind, TokKind::LBrace) {
                format!("write `{} {{ … }}`", Syntax::KW_PREP)
            } else if let TokKind::Ident(name) = &self.peek().kind {
                format!(
                    "write `{} :: {} {{ … }}`",
                    Syntax::canonical_name_case(name, Syntax::NameCase::Snake),
                    Syntax::KW_PREP
                )
            } else {
                format!("write a prepared value: `name :: {} {{ … }}`", Syntax::KW_PREP)
            };
            self.diags.push(self.retired_known_error(head.span, fix));
            return Ok(head.span);
        }
        if matches!(self.peek().kind, TokKind::KwComptime) {
            let span = self.bump().span;
            self.diags.push(Diagnostic::error(
                "E0374",
                "`comptime` is retired".to_string(),
                "Jet folds ordinary foldable expressions automatically; explicit compile-time demand lives on the marker plane"
                    .to_string(),
                "remove the keyword; a build-time value is written `name :: prep { … }`, and `prep { … }` runs other work while building"
                    .to_string(),
                Some(span),
            ));
            return Ok(span);
        }
        if matches!(self.peek().kind, TokKind::At) {
            return Ok(match self.peek2().kind {
                TokKind::KwIf => self.retired_at_verb(Syntax::KW_IF),
                TokKind::KwLoop => self.retired_at_verb(Syntax::KW_LOOP),
                TokKind::LBrace => self.retired_at_block(),
                _ => self.bump().span,
            });
        }
        // The control keyword still reads its `#Known` head through the one
        // shared marker reader; the name it accepts is not a registry question.
        Ok(self.read_marker_head()?.span)
    }

    // --- expressions -----------------------------------------------------
}

/// D-UNINIT-SENTINEL2: `Type{ uninit }` as a whole typed-literal body.
/// Returns `(head_type, ty_span, uninit_span)`. Non-whole bodies are ordinary
/// typed literals (not this trigger).
fn typed_lit_uninit_head(init: &Expr) -> Option<(Type, Span, Span)> {
    let Expr::TypedLit { head, body, span } = init else {
        return None;
    };
    let head = head.as_ref()?;
    let marker_span = match body {
        TypedLitBody::Value(inner) => match inner.as_ref() {
            Expr::Ident(n, sp) if n == Syntax::KW_UNINIT => *sp,
            _ => return None,
        },
        TypedLitBody::Elements(elems) if elems.len() == 1 => match &elems[0] {
            Expr::Ident(n, sp) if n == Syntax::KW_UNINIT => *sp,
            _ => return None,
        },
        // Named heads parse a bare `uninit` as a one-field shorthand.
        TypedLitBody::Fields(fields) if fields.len() == 1 => {
            let (fname, fspan, val) = &fields[0];
            match val {
                Expr::Ident(n, _) if fname == Syntax::KW_UNINIT && n == Syntax::KW_UNINIT => *fspan,
                _ => return None,
            }
        }
        _ => return None,
    };
    Some((head.clone(), *span, marker_span))
}

/// D-LAYOUT1 / D-LAYOUT-CTOR1: is `field` one of the recognized box-anchor
/// names? `left`/`right`/`width` are horizontal (`HVar`); `top`/`bottom`/
/// `height` are vertical (`VVar`). Returns the `Layout` accessor method name
/// (`h`/`v`) or `None` if `field` isn't an anchor (an ordinary field access,
/// left untouched).
fn layout_anchor_method(field: &str) -> Option<&'static str> {
    match field {
        "left" | "right" | "width" => Some("h"),
        "top" | "bottom" | "height" => Some("v"),
        _ => None,
    }
}

/// D-LAYOUT1 / D-LAYOUT-CTOR1: purely structural, parse-time rewrite. Inside
/// `name :: Layout.{ … }`, a bare `box.anchor` read
/// (`Expr::Field(Expr::Ident(box), anchor, _)`, where `anchor` is a recognized
/// anchor name) becomes `name.h(box, anchor)` / `name.v(box, anchor)` — an
/// ordinary `MethodCall` on the layout handle. `self.anchor` vivifies the
/// container box named after the binding (same box id as `name`). Any other
/// field access (unrecognized anchor name, non-`Ident` base) is left alone
/// and falls through to normal resolution (and normal errors) in sema. No
/// type information is used here (I3: all checking still lives in sema —
/// this only changes which AST shape sema sees, it decides nothing).
pub(super) fn desugar_layout_anchors(layout_name: &str, stmt: &mut Stmt) {
    match stmt {
        Stmt::Expr(e) | Stmt::DeferClose { close: e, .. } => desugar_layout_expr(layout_name, e),
        Stmt::Val(b) => desugar_layout_expr(layout_name, &mut b.init),
        _ => {}
    }
}

fn desugar_layout_expr(layout_name: &str, e: &mut Expr) {
    // Rewrite this node in place if it's a `box.anchor` / `self.anchor` read.
    if let Expr::Field(base, field, field_span) = e {
        if let Expr::Ident(box_name, ident_span) = base.as_ref() {
            if let Some(method) = layout_anchor_method(field) {
                let box_name = if box_name == Syntax::KW_SELF {
                    layout_name.to_string()
                } else {
                    box_name.clone()
                };
                let anchor = field.clone();
                let span = *field_span;
                let ident_span = *ident_span;
                *e = Expr::MethodCall {
                    receiver: Box::new(Expr::Ident(layout_name.to_string(), ident_span)),
                    method: method.to_string(),
                    method_span: span,
                    owner_type_args: Vec::new(),
                    type_args: Vec::new(),
                    args: vec![
                        CallArg {
                            convention: AccessConvention::Read,
                            expr: Expr::Str(vec![StrPart::Lit(box_name)], ident_span),
                            span: ident_span,
                            flags: crate::AST::CallArgFlags::default(),
                            label: None,
                            spread: false,
                        },
                        CallArg {
                            convention: AccessConvention::Read,
                            expr: Expr::Str(vec![StrPart::Lit(anchor)], span),
                            span,
                            flags: crate::AST::CallArgFlags::default(),
                            label: None,
                            spread: false,
                        },
                    ],
                    recv_type: None,
                    resolved_ret: None,
                    operator_rhs: None,
                    checked_widen: false,
                };
                return;
            }
        }
    }
    // Recurse structurally so anchors nested in arithmetic/comparisons are
    // still found: `label.right + 16.0 == input.left`, `.priority()` chains, …
    match e {
        Expr::Binary(_, l, r, _) => {
            desugar_layout_expr(layout_name, l);
            desugar_layout_expr(layout_name, r);
        }
        Expr::Unary(_, x, _) => desugar_layout_expr(layout_name, x),
        Expr::Field(base, _, _) => desugar_layout_expr(layout_name, base),
        Expr::MethodCall { receiver, args, .. } => {
            desugar_layout_expr(layout_name, receiver);
            for a in args {
                desugar_layout_expr(layout_name, &mut a.expr);
            }
        }
        Expr::Call(call) => {
            for a in &mut call.args {
                desugar_layout_expr(layout_name, &mut a.expr);
            }
        }
        _ => {}
    }
}
