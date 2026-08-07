use super::super::*;

impl<'a> Parser<'a> {
    /// D-BIND-BARE1: a binding starting with the target (no keyword), written
    /// `name (:: | :=) expr`. The sigil chooses mutability.
    /// Typed forms `name: Type :: expr` / `name: Type := expr` are retired —
    /// ordinary parse error, no teaching window.
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
                reactive_local: false,
                reactive_local_span: None,
                reactive_shared: false,
                reactive_shared_span: None,
                reactive_upgrade: false,
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
                gc_promotion: None,
                gc_transferred: false,
            });
        }
        let (name, name_span) = self.expect_ident("for the binding name")?;
        // Retired D-BINDEXPLICIT1 / D-BIND4 typed forms — ordinary parse error.
        // `name@ Type` and `name: Type` never open a binding under D-BIND-BARE1.
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
                    "write `name {} value` or put the type on the value (e.g. `Type.{{ … }}`)",
                    Syntax::SIGIL_BIND_IMMUT
                ),
                Some(self.peek().span),
            ));
        }
        let mutable = self.expect_bind_sigil()?;
        // Bare `name := uninit` — type must ride a `Type.{ uninit }` head.
        if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_UNINIT)
            && matches!(
                self.peek2().kind,
                TokKind::Semi | TokKind::RBrace | TokKind::Eof
            )
        {
            let uninit_span = self.peek().span;
            return Err(Diagnostic::error(
                "E0421",
                "`uninit` needs a typed-literal head".to_string(),
                "an uninitialized binding has no value to infer its type from, so the type must head the literal".to_string(),
                format!(
                    "write `{name} {} <Type>.{{ {} }}`, e.g. `buffer := [U8#4096].{{ {} }}`",
                    Syntax::SIGIL_BIND_MUT,
                    Syntax::KW_UNINIT,
                    Syntax::KW_UNINIT
                ),
                Some(uninit_span),
            ));
        }
        let init = self.expr()?;
        // D-UNINIT-SENTINEL2: `name := Type.{ uninit }` — uninit only as a
        // whole typed-literal body. Mutable bindings only (`:=`).
        if mutable {
            if let Some((ty, ty_span, marker_span)) = typed_lit_uninit_head(&init) {
                return Ok(Binding {
                    mutable: true,
                    track: false,
                    track_span: None,
                reactive_local: false,
                reactive_local_span: None,
                reactive_shared: false,
                reactive_shared_span: None,
                reactive_upgrade: false,
                    meta: None,
                    name,
                    name_span,
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
            track: false,
            track_span: None,
                reactive_local: false,
                reactive_local_span: None,
                reactive_shared: false,
                reactive_shared_span: None,
                reactive_upgrade: false,
            meta: None,
            name,
            name_span,
            pattern: None,
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
        })
    }

    /// D-BIND-BARE1: consume `::` (immutable) or `:=` (mutable); returns `mutable`.
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

    /// D-BIND-BARE1: true when the tokens at the cursor begin a sigil binding —
    /// `name ::`, `name :=`, or a destructuring pattern target.
    /// Also matches retired `name : …` so sigil_binding can emit the ordinary
    /// parse error (no teaching window).
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
            // Retired typed form `name : Type ::/:= …` — still recognized so
            // sigil_binding can reject it with E0003 (D-BIND-BARE1).
            TokKind::Ident(_) | TokKind::KwSelf if matches!(self.peek2().kind, TokKind::Colon) => {
                true
            }
            // Destructuring targets: scan ahead to a `::`/`:=` after the matching
            // close. Cheap bounded lookahead. `[ … ] ::`, `( … ) ::`,
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
    /// D-VERDICT-1308-1: parse `#Known { … }`; recover retired `comptime`.
    /// Erases at codegen (build-time only). `$name` splice deferred to c155.
    pub(super) fn comptime_block_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.take_mark()?;
        self.expect(TokKind::LBrace, "to open the `#Known` block body")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ComptimeBlock {
            body,
            span: Span::new(start.start, end),
        })
    }

    /// D-META-STAGE1=B: `$loop …` is the ratified `loop` verb at compile time,
    /// not a second iteration form. It is one compile-time block holding one
    /// loop, so it folds through the same path as `$ { … }` and emits no
    /// runtime code.
    pub(super) fn comptime_loop_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.take_mark()?;
        let body = self.loop_stmt(None)?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ComptimeBlock {
            body: vec![body],
            span: Span::new(start.start, end),
        })
    }

    /// D-VERDICT-1308-2: parse `#Known if <cond> { … } else { … }`.
    /// Both arms require `{ }` (braceless bodies are not allowed for `#Known if`).
    /// `else` is optional in statement position. Sema selects the arm; codegen
    /// emits only the selected arm (D-WHEN2: dropped arm is name-resolved only).
    pub(super) fn comptime_if_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.take_mark()?;
        self.bump(); // `if`

        // D-OSTARGET2=B (ratified 2026-07-03): the dispatch form
        // `#Known if build.os == { .Linux -> … .MacOS -> … }`. Detected the
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
                self.expect(TokKind::LBrace, "to open the `#Known if` dispatch body")?;
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
        // Not the dispatch form — rewind and parse the boolean `#Known if`.
        self.pos = probe;
        self.diags.truncate(probe_diags);

        let cond_start = self.peek().span;
        let cond = self.expr_no_struct_lit()?;
        let cond_span = Span::new(cond_start.start, self.toks[self.pos - 1].span.end);
        self.expect(TokKind::LBrace, "to open the `#Known if` body")?;
        let then_body = self.block_stmts();
        let else_body = if matches!(self.peek().kind, TokKind::KwElse) {
            self.bump();
            // Allow `else if` chained with another `$if`.
            if (matches!(
                self.peek().kind,
                TokKind::Dollar | TokKind::KwComptime
            ) && matches!(self.peek2().kind, TokKind::KwIf))
                || (self.at_known_lead() && matches!(self.peek3().kind, TokKind::KwIf))
            {
                let chain = self.comptime_if_stmt()?;
                Some(vec![chain])
            } else {
                self.expect(TokKind::LBrace, "to open the `#Known if` else body")?;
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
        self.take_mark()?;
        let (name, name_span) = self.expect_ident("after `#Known`")?;
        if retired {
            self.expect(TokKind::Eq, "in the retired comptime binding")?;
        } else {
            self.expect(TokKind::ColonColon, "in a `#Known` binding")?;
        }
        let init = self.expr()?;
        Ok(Binding {
            mutable: false,
            track: false,
            track_span: None,
                reactive_local: false,
                reactive_local_span: None,
                reactive_shared: false,
                reactive_shared_span: None,
                reactive_upgrade: false,
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
            gc_promotion: None,
            gc_transferred: false,
        })
    }

    /// D-VERDICT-1308-1/2: true when `#Known` is at the cursor.
    pub(in crate::Parser) fn at_known_lead(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(name) if name == Syntax::RETIRED_MARKER_KNOWN)
    }

    /// B5 revert (card #1456): #1537's own checkpoint made this teach E0377-
    /// E0379 as retired spellings, but #1537 hasn't landed its migration of
    /// the 327 in-repo `#Known` uses yet. `#Known` parses like master again —
    /// silently, no diagnostic — until #1537 lands the full retirement.
    /// `comptime` stays taught (pre-existing, unrelated to this revert). The
    /// bare `$` mark this checkpoint also added ($ blocks, $if, $loop) still
    /// parses too — it's a new, additive spelling, not a hard-error source.
    fn take_mark(&mut self) -> Result<Span, Diagnostic> {
        if matches!(self.peek().kind, TokKind::KwComptime) {
            let span = self.bump().span;
            self.diags.push(Diagnostic::error(
                "E0374",
                "`comptime` is retired".to_string(),
                "Jet folds ordinary foldable expressions automatically; explicit compile-time demand lives on the marker plane"
                    .to_string(),
                "remove the keyword for ordinary code, or replace it with `#Known` when failure to compute now must stop the build"
                    .to_string(),
                Some(span),
            ));
            return Ok(span);
        }
        if matches!(self.peek().kind, TokKind::Dollar) {
            return Ok(self.bump().span);
        }
        // The control keyword still reads its `#Known` head through the one
        // shared marker reader; the name it accepts is not a registry question.
        Ok(self.read_marker_head()?.span)
    }

    // --- expressions -----------------------------------------------------
}

/// D-UNINIT-SENTINEL2: `Type.{ uninit }` as a whole typed-literal body.
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
        Stmt::Expr(e) => desugar_layout_expr(layout_name, e),
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
