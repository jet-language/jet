use super::super::*;

impl<'a> Parser<'a> {
    /// D-BIND4: a binding starting with the target (no keyword), written
    /// `name (: type)? (:: | :=) expr`. The sigil chooses mutability:
    /// `::` immutable (was `val` / `::`), `:=` mutable (was `var`).
    pub(super) fn sigil_binding(&mut self) -> Result<Binding, Diagnostic> {
        // S74: a destructuring target — `[ … ]` for a list, `Ident { … }` for a
        // struct — instead of a plain `name`.
        if let Some(pattern) = self.try_bind_pattern()? {
            let mutable = self.expect_bind_sigil()?;
            let init = self.expr()?;
            return Ok(Binding {
                mutable,
                track: false,
                track_span: None,
                meta: None,
                name: String::new(),
                name_span: pattern.span(),
                pattern: Some(pattern),
                ty: None,
                ty_span: None,
                init,
                is_comptime: false,
                ct: None,
                uninit: false,
                arena_view: false,
                string_view: false,
            });
        }
        let (name, name_span) = self.expect_ident("for the binding name")?;
        // D-BIND4: retired D-BINDEXPLICIT1 form `name@ Type = expr`.
        if matches!(self.peek().kind, TokKind::At) {
            self.bump(); // `@`
            let (ty, ty_span) = self.type_()?;
            self.diags.push(Diagnostic::error(
                "E0998",
                format!(
                    "explicit immutable binding uses `name: Type {}`, not `name@ Type =`",
                    Syntax::SIGIL_BIND_IMMUT
                ),
                format!(
                    "explicit immutable bindings use `: Type {}` (D-BIND4)",
                    Syntax::SIGIL_BIND_IMMUT
                ),
                format!(
                    "write `{name}: <Type> {}` instead of `{name}@ <Type> =`",
                    Syntax::SIGIL_BIND_IMMUT
                ),
                Some(name_span),
            ));
            self.expect(TokKind::Eq, "after the type in `name@ Type = expr`")?;
            let init = self.expr()?;
            return Ok(Binding {
                mutable: false,
                track: false,
                track_span: None,
                meta: None,
                name,
                name_span,
                pattern: None,
                ty: Some(ty),
                ty_span: Some(ty_span),
                init,
                is_comptime: false,
                ct: None,
                uninit: false,
                arena_view: false,
                string_view: false,
            });
        }
        let (ty, ty_span) = if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            let (t, s) = self.type_()?;
            (Some(t), Some(s))
        } else {
            (None, None)
        };
        if ty.is_some() {
            let sigil_span = self.peek().span;
            match self.peek().kind {
                // D-BIND4: `name: Type :: expr` — explicit immutable.
                TokKind::ColonColon => {
                    self.bump();
                    let init = self.expr()?;
                    return Ok(Binding {
                        mutable: false,
                        track: false,
                        track_span: None,
                meta: None,
                        name,
                        name_span,
                        pattern: None,
                        ty,
                        ty_span,
                        init,
                        is_comptime: false,
                        ct: None,
                        uninit: false,
                        arena_view: false,
                        string_view: false,
                    });
                }
                // D-BIND4: `name: Type := expr` — explicit mutable.
                TokKind::ColonEq => {
                    self.bump();
                    // D-UNINIT-SENTINEL1: `name: Type := uninit` — the contextual
                    // `uninit` keyword, legal only when it is the whole initializer
                    // (nothing else follows on the line). Reuses the existing
                    // sema flow-analysis engine (E0420/E0423/E0424) unchanged; only
                    // this trigger moved from the retired `#Uninit` marker.
                    if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_UNINIT)
                        && matches!(
                            self.peek2().kind,
                            TokKind::Semi | TokKind::RBrace | TokKind::Eof
                        )
                    {
                        let marker_span = self.bump().span; // `uninit`
                        return Ok(Binding {
                            mutable: true,
                            track: false,
                            track_span: None,
                meta: None,
                            name,
                            name_span,
                            pattern: None,
                            ty,
                            ty_span,
                            // Harmless placeholder — never evaluated; sema/codegen
                            // branch on `uninit` first and use `ty` for the type.
                            init: Expr::Int(0, marker_span, None, None),
                            is_comptime: false,
                            ct: None,
                            uninit: true,
                            arena_view: false,
                            string_view: false,
                        });
                    }
                    let init = self.expr()?;
                    return Ok(Binding {
                        mutable: true,
                        track: false,
                        track_span: None,
                meta: None,
                        name,
                        name_span,
                        pattern: None,
                        ty,
                        ty_span,
                        init,
                        is_comptime: false,
                        ct: None,
                        uninit: false,
                        arena_view: false,
                        string_view: false,
                    });
                }
                // Retired: `name: Type : expr`.
                TokKind::Colon => {
                    self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0998",
                        format!(
                            "explicit immutable binding uses `name: Type {}`, not `name: Type :`",
                            Syntax::SIGIL_BIND_IMMUT
                        ),
                        format!(
                            "when the type is written, immutable uses `{}` and mutable uses `{}` (D-BIND4)",
                            Syntax::SIGIL_BIND_IMMUT,
                            Syntax::SIGIL_BIND_MUT
                        ),
                        format!(
                            "write `{name}: <Type> {}` instead of `{name}: <Type> :`",
                            Syntax::SIGIL_BIND_IMMUT
                        ),
                        Some(sigil_span),
                    ));
                    let init = self.expr()?;
                    return Ok(Binding {
                        mutable: false,
                        track: false,
                        track_span: None,
                meta: None,
                        name,
                        name_span,
                        pattern: None,
                        ty,
                        ty_span,
                        init,
                        is_comptime: false,
                        ct: None,
                        uninit: false,
                        arena_view: false,
                        string_view: false,
                    });
                }
                // Retired: `name: Type = expr`.
                TokKind::Eq => {
                    self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0998",
                        format!(
                            "explicit mutable binding uses `name: Type {}`, not `name: Type =`",
                            Syntax::SIGIL_BIND_MUT
                        ),
                        format!(
                            "when the type is written, immutable uses `{}` and mutable uses `{}` (D-BIND4)",
                            Syntax::SIGIL_BIND_IMMUT,
                            Syntax::SIGIL_BIND_MUT
                        ),
                        format!(
                            "write `{name}: <Type> {}` instead of `{name}: <Type> =`",
                            Syntax::SIGIL_BIND_MUT
                        ),
                        Some(sigil_span),
                    ));
                    let init = self.expr()?;
                    return Ok(Binding {
                        mutable: true,
                        track: false,
                        track_span: None,
                meta: None,
                        name,
                        name_span,
                        pattern: None,
                        ty,
                        ty_span,
                        init,
                        is_comptime: false,
                        ct: None,
                        uninit: false,
                        arena_view: false,
                        string_view: false,
                    });
                }
                _ => {}
            }
        }
        let mutable = self.expect_bind_sigil()?;
        // D-UNINIT-SENTINEL1: `name := uninit` with no type annotation — the type
        // can't be inferred from `uninit`, so this is E0421 (same rule the retired
        // `#Uninit name` marker enforced).
        if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_UNINIT)
            && matches!(
                self.peek2().kind,
                TokKind::Semi | TokKind::RBrace | TokKind::Eof
            )
        {
            let uninit_span = self.peek().span;
            return Err(Diagnostic::error(
                "E0421",
                "`uninit` needs a type annotation".to_string(),
                "an uninitialized binding has no value to infer its type from, so the type must be written".to_string(),
                format!("write `{name}: <Type> := uninit`, e.g. `buffer: [4096]U8 := uninit`"),
                Some(uninit_span),
            ));
        }
        let init = self.expr()?;
        Ok(Binding {
            mutable,
            track: false,
            track_span: None,
                meta: None,
            name,
            name_span,
            pattern: None,
            ty,
            ty_span,
            init,
            is_comptime: false,
            ct: None,
            uninit: false,
            arena_view: false,
            string_view: false,
        })
    }

    /// D-BIND4: consume `::` (immutable) or `:=` (mutable); returns `mutable`.
    pub(super) fn expect_bind_sigil(&mut self) -> Result<bool, Diagnostic> {
        match self.peek().kind {
            TokKind::ColonColon => {
                self.bump();
                Ok(false)
            }
            TokKind::ColonEq => {
                self.bump();
                Ok(true)
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

    /// D-BIND4: true when the tokens at the cursor begin a sigil binding —
    /// `name ::`, `name :=`, `name : type ::`, `name : type :=`, or a destructuring
    /// pattern target (`[ … ] ::`, `Ident { … } ::`).
    /// Used by the statement dispatcher to tell a binding apart from an
    /// expression/assignment that also starts with a name.
    pub(super) fn looks_like_sigil_binding(&self) -> bool {
        match &self.peek().kind {
            // `name :: e` / `name := e`
            TokKind::Ident(_) | TokKind::KwSelf
                if matches!(self.peek2().kind, TokKind::ColonColon | TokKind::ColonEq) =>
            {
                true
            }
            // `name : type :: e` / `name : type := e` / `name: Type := e` — typed binding.
            // A bare `:` only ever opens a type annotation here, so any `name :` at
            // statement start is a binding (a map index uses `[`, not `:`).
            TokKind::Ident(_) | TokKind::KwSelf if matches!(self.peek2().kind, TokKind::Colon) => {
                true
            }
            // Destructuring targets: scan ahead to a `::`/`:=` after the matching
            // close. Cheap bounded lookahead. `[a, b] ::`, `(a, b) ::`,
            // `Type { … } ::` (E0320 recovery), and `Type.{ … } ::` (D-DOTCTOR1).
            TokKind::LBracket | TokKind::LParen => self.pattern_target_is_binding(),
            TokKind::Ident(_) if matches!(self.peek2().kind, TokKind::LBrace) => {
                self.pattern_target_is_binding()
            }
            // D-DOTCTOR1: `Type.{ … } ::` — new form.
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

    /// D-DOTCTOR1: like `pattern_target_is_binding` but skips `Ident Dot` before
    /// the opening `{`, for the `Type.{ … } :: expr` destructuring form.
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
            // D-DOTCTOR1: `Type.{ x, y }` new form (new) or `Type { x, y }` (E0320 recovery).
            TokKind::Ident(_)
                if matches!(self.peek2().kind, TokKind::Dot)
                    && matches!(self.peek3().kind, TokKind::LBrace) =>
            {
                let (type_name, type_span) = self.expect_ident("for a struct pattern")?;
                self.expect(TokKind::Dot, "in a struct pattern")?;
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
                // D-DOTCTOR2: old dotless `Type { x, y }` — E0320 recovery.
                let (type_name, type_span) = self.expect_ident("for a struct pattern")?;
                let brace_span = self.peek().span;
                self.diags.push(Diagnostic::error(
                    "E0320",
                    format!(
                        "struct pattern uses `{}.{{…}}`, not `{} {{…}}`",
                        type_name, type_name
                    ),
                    "the struct pattern needs a dot before the brace (D-DOTCTOR1)".to_string(),
                    format!("write `{}.{{…}}` instead", type_name),
                    Some(brace_span),
                ));
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
    /// `Type.{ … }` pattern (both the D-DOTCTOR1 and E0320-recovery spellings).
    pub(super) fn struct_pattern_fields(&mut self) -> Result<(Vec<BindName>, Option<Span>), Diagnostic> {
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
    /// D-CTMARKER1 (ratified 2026-06-25, piece 2): parse `comptime { … }`.
    /// Erases at codegen (build-time only). `$name` splice deferred to c155.
    pub(super) fn comptime_block_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.bump().span; // `comptime`
        self.expect(TokKind::LBrace, "to open the `comptime` block body")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ComptimeBlock {
            body,
            span: Span::new(start.start, end),
        })
    }

    /// D-WHEN1 (ratified 2026-06-19): parse `comptime if <cond> { … } else { … }`.
    /// Both arms require `{ }` (braceless bodies are not allowed for `comptime if`).
    /// `else` is optional in statement position. Sema selects the arm; codegen
    /// emits only the selected arm (D-WHEN2: dropped arm is name-resolved only).
    pub(super) fn comptime_if_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.bump().span; // `comptime`
        self.bump(); // `if`

        // D-OSTARGET2=B (ratified 2026-07-03): the dispatch form
        // `comptime if build.os == { .Linux -> … .Macos -> … }`. Detected the
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
                self.expect(TokKind::LBrace, "to open the `comptime if` dispatch body")?;
                let switch = self.if_arms(subject, start)?;
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
        // Not the dispatch form — rewind and parse the boolean `comptime if`.
        self.pos = probe;
        self.diags.truncate(probe_diags);

        let cond_start = self.peek().span;
        let cond = self.expr_no_struct_lit()?;
        let cond_span = Span::new(cond_start.start, self.toks[self.pos - 1].span.end);
        self.expect(TokKind::LBrace, "to open the `comptime if` body")?;
        let then_body = self.block_stmts();
        let else_body = if matches!(self.peek().kind, TokKind::KwElse) {
            self.bump();
            // Allow `else if` chained with another `comptime if`.
            if matches!(self.peek().kind, TokKind::KwComptime)
                && matches!(self.peek2().kind, TokKind::KwIf)
            {
                let chain = self.comptime_if_stmt()?;
                Some(vec![chain])
            } else {
                self.expect(TokKind::LBrace, "to open the `comptime if` else body")?;
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
        self.expect_kw(TokKind::KwComptime, "to start a comptime binding")?;
        let (name, name_span) = self.expect_ident("after `comptime`")?;
        self.expect(TokKind::Eq, "in a comptime binding")?;
        let init = self.expr()?;
        Ok(Binding {
            mutable: false,
            track: false,
            track_span: None,
                meta: None,
            name,
            name_span,
            pattern: None,
            ty: None,
            ty_span: None,
            init,
            is_comptime: true,
            ct: None,
            uninit: false,
            arena_view: false,
            string_view: false,
        })
    }

    // --- expressions -----------------------------------------------------
}

/// D-LAYOUT1: is `field` one of the recognized box-anchor names? `left`/
/// `right`/`width` are horizontal (`HVar`); `top`/`bottom`/`height` are
/// vertical (`VVar`). Returns the `LayoutHandle` accessor method name (`h`/
/// `v`) or `None` if `field` isn't an anchor (an ordinary field access,
/// left untouched).
fn layout_anchor_method(field: &str) -> Option<&'static str> {
    match field {
        "left" | "right" | "width" => Some("h"),
        "top" | "bottom" | "height" => Some("v"),
        _ => None,
    }
}

/// D-LAYOUT1: purely structural, parse-time rewrite. Inside `layout NAME { … }`,
/// a bare `box.anchor` read (`Expr::Field(Expr::Ident(box), anchor, _)`, where
/// `anchor` is a recognized anchor name) becomes `NAME.h(box, anchor)` /
/// `NAME.v(box, anchor)` — an ordinary `MethodCall` on the layout handle.
/// Any other field access (unrecognized anchor name, non-`Ident` base) is left
/// alone and falls through to normal resolution (and normal errors) in sema.
/// No type information is used here (I3: all checking still lives in sema —
/// this only changes which AST shape sema sees, it decides nothing).
pub(super) fn desugar_layout_anchors(layout_name: &str, stmt: &mut Stmt) {
    match stmt {
        Stmt::Expr(e) => desugar_layout_expr(layout_name, e),
        Stmt::Val(b) => desugar_layout_expr(layout_name, &mut b.init),
        _ => {}
    }
}

fn desugar_layout_expr(layout_name: &str, e: &mut Expr) {
    // Rewrite this node in place if it's a `box.anchor` read.
    if let Expr::Field(base, field, field_span) = e {
        if let Expr::Ident(box_name, ident_span) = base.as_ref() {
            if let Some(method) = layout_anchor_method(field) {
                let box_name = box_name.clone();
                let anchor = field.clone();
                let span = *field_span;
                let ident_span = *ident_span;
                *e = Expr::MethodCall {
                    receiver: Box::new(Expr::Ident(layout_name.to_string(), ident_span)),
                    method: method.to_string(),
                    method_span: span,
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
