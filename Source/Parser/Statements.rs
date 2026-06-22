use super::*;

impl<'a> Parser<'a> {
    /// S58 (E2-M13, D-LL2): parse a `#Unsafe { … }` audited region in
    /// statement position, with an optional `#Audit("…")` reason on the line
    /// above. The reason is required at runtime by lint L3101, not by the
    /// grammar, so a missing `#Audit` parses fine and is flagged in sema.
    fn at_unsafe_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        // Optional `#Audit("…")`.
        let mut audit = None;
        if matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_AUDIT)
        {
            self.bump(); // `#`
            self.bump(); // `audit`
            self.expect(TokKind::LParen, "after `#Audit`")?;
            let (reason, _) = self.expect_plain_string(
                "for the audit reason",
                "`#Audit` takes one piece of quoted text explaining why the block is safe",
                "write: #Audit(\"index checked against len\")",
            )?;
            self.expect(TokKind::RParen, "after the audit reason")?;
            audit = Some(reason);
            // S6-R: `#Audit("…")` ends a line; the synthetic terminator before
            // the `#Unsafe` it annotates is trivia — skip it.
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
        }
        // Required `#Unsafe { … }`.
        if !(matches!(self.peek().kind, TokKind::Hash)
            && matches!(self.peek2().kind, TokKind::KwUnsafe))
        {
            return Err(Diagnostic::error(
                "E0003",
                format!("`#{}` must be followed by a `#{}` block", Syntax::ATTR_AUDIT, Syntax::KW_UNSAFE),
                "an audit reason annotates the gated region it sits above".to_string(),
                format!(
                    "write `#{}(\"…\") #{} {{ … }}`",
                    Syntax::ATTR_AUDIT,
                    Syntax::KW_UNSAFE
                ),
                Some(self.peek().span),
            ));
        }
        self.bump(); // `#`
        self.bump(); // `unsafe`
        self.expect(TokKind::LBrace, "after `#Unsafe`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Unsafe {
            audit,
            body,
            span: Span::new(start.start, end),
        })
    }

    /// D-CTX1 (ratified 2026-06-22, G2): parse `#Context(field: value, …) { … }`.
    /// Cursor is on the `#` token. Emits E0760 for `=` spelling, E0761 for
    /// unknown fields.
    fn at_context_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // `#`
        self.bump(); // `Context`
        self.expect(TokKind::LParen, &format!("after `#{}`", Syntax::CTX_BLOCK))?;
        let mut fields: Vec<(String, Expr, Span)> = Vec::new();
        // Parse comma-separated `ident : expr` pairs.
        while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
            let field_start = self.peek().span;
            let (field_name, field_name_span) =
                self.expect_ident("for the context field name")?;
            // E0760: `=` is reassignment (S17); context fields use `:`.
            if matches!(self.peek().kind, TokKind::Eq) {
                let eq_span = self.peek().span;
                return Err(Diagnostic::error(
                    "E0760",
                    "context fields are set with `:`, not `=`".to_string(),
                    "`=` is reassignment (S17); the `name: value` form sets a context field (D-CTX1)".to_string(),
                    format!(
                        "write `#{}({}: …) {{ … }}`",
                        Syntax::CTX_BLOCK,
                        field_name
                    ),
                    Some(eq_span),
                ));
            }
            self.expect(TokKind::Colon, &format!("after the field name `{}`", field_name))?;
            // E0761: unknown field name.
            if field_name != Syntax::CTX_FIELD_ALLOCATOR
                && field_name != Syntax::CTX_FIELD_LOGGER
            {
                return Err(Diagnostic::error(
                    "E0761",
                    format!("`{}` isn't a context field", field_name),
                    "the context bundle holds `allocator` and `logger`".to_string(),
                    format!(
                        "write `#{}(allocator: …)` or `#{}(logger: …)`",
                        Syntax::CTX_BLOCK,
                        Syntax::CTX_BLOCK
                    ),
                    Some(Span::new(field_start.start, field_name_span.end)),
                ));
            }
            let value = self.expr()?;
            let field_end = self.toks[self.pos - 1].span.end;
            fields.push((field_name, value, Span::new(field_start.start, field_end)));
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RParen, &format!("after `#{}(…`", Syntax::CTX_BLOCK))?;
        self.expect(
            TokKind::LBrace,
            &format!("after `#{}(…)`", Syntax::CTX_BLOCK),
        )?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ContextBlock {
            fields,
            body,
            span: Span::new(start.start, end),
        })
    }

    /// S19 + D-LABEL1: parse a `loop` statement (all three header forms), with an
    /// optional `@name` label already parsed by the caller. The cursor is on the
    /// `loop` keyword.
    fn loop_stmt(&mut self, label: Option<(String, Span)>) -> Result<Stmt, Diagnostic> {
        let span = self.bump().span; // `loop`
        // S19-amend: `loop` handles all three loop forms by header.
        //   loop { }               → infinite
        //   loop cond { }          → conditional (was `while`)
        //   loop x in ... { }      → iteration (was `for`)
        //   loop k, v in ... { }   → key-value iteration
        if matches!(self.peek().kind, TokKind::LBrace) {
            // Infinite loop
            self.bump();
            let body = self.block_stmts();
            Ok(Stmt::Loop { body, span, label })
        } else if matches!(&self.peek().kind, TokKind::Ident(_))
            && matches!(&self.peek2().kind, TokKind::KwIn | TokKind::Comma)
        {
            // Iteration: loop x in ... { } or loop k, v in ... { }
            let (var, var_span) = self.expect_ident("as the loop variable")?;
            let mut var2 = None;
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                let (v2, s2) = self.expect_ident("after `,` in `loop key, value in`")?;
                var2 = Some((v2, s2));
            }
            self.expect_kw(TokKind::KwIn, "after the loop variable")?;
            let first = self.expr_no_struct_lit()?;
            let kind = if matches!(self.peek().kind, TokKind::DotDot) {
                self.bump();
                let end = self.expr_no_struct_lit()?;
                let step = if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_RANGE_STEP)
                {
                    self.bump();
                    Some(self.expr_no_struct_lit()?)
                } else {
                    None
                };
                ForKind::Range { start: first, end, step }
            } else {
                ForKind::In { collection: first }
            };
            self.expect(TokKind::LBrace, "to open the loop body")?;
            let body = self.block_stmts();
            Ok(Stmt::For { var, var_span, var2, kind, body, span, label })
        } else {
            // Conditional: loop cond { }
            let cond = self.expr_no_struct_lit()?;
            self.expect(TokKind::LBrace, "to open the loop body")?;
            let body = self.block_stmts();
            Ok(Stmt::While { cond, body, span, label })
        }
    }

    /// Parse statements until the closing `}` (consumed). Recovers at
    /// statement boundaries so several problems surface in one run.
    pub(super) fn block_stmts(&mut self) -> Vec<Stmt> {
        let mut body = Vec::new();
        loop {
            match &self.peek().kind {
                TokKind::RBrace => {
                    self.bump();
                    break;
                }
                // S6-R: a block statement (`if`/`loop`/`@unsafe`/nested `{}`)
                // ends with `}`, after which the lexer inserts a synthetic
                // terminator. Those statements don't consume their own
                // terminator, so skip a stray one here.
                TokKind::Semi => {
                    self.bump();
                }
                TokKind::Eof => {
                    self.diags.push(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this block, found the end of the file".to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                    break;
                }
                _ => match self.stmt() {
                    Ok(s) => body.push(s),
                    Err(d) => {
                        self.diags.push(d);
                        self.sync_stmt();
                    }
                },
            }
        }
        body
    }

    /// D-UNINIT1 (opt C): parse `#Uninit name: Type` — a binding with no
    /// initializer, gated by `use core.mem` (the gate is checked in sema).
    /// The type annotation is required; an initializer is rejected here.
    /// Not yet wired into `stmt` — see the note in the `TokKind::Hash` arm.
    #[allow(dead_code)]
    fn uninit_binding(&mut self) -> Result<Stmt, Diagnostic> {
        let hash_span = self.peek().span;
        self.bump(); // `#`
        let marker = self.bump(); // `uninit`
        let marker_span = Span::new(hash_span.start, marker.span.end);
        // S6-R: the marker may end its line; skip the synthetic terminator before
        // the binding it annotates (same as `#Audit` before `#Unsafe`).
        if matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        let (name, name_span) = self.expect_ident("for the uninitialized binding name")?;
        if !matches!(self.peek().kind, TokKind::Colon) {
            return Err(Diagnostic::error(
                "E0421",
                format!("`#{}` needs a type annotation", Syntax::ATTR_UNINIT),
                "an uninitialized binding has no value to infer its type from, so the type must be written".to_string(),
                format!("write `#{} {}: <Type>`, e.g. `#{} buffer: [4096]U8`", Syntax::ATTR_UNINIT, name, Syntax::ATTR_UNINIT),
                Some(name_span),
            ));
        }
        self.bump(); // `:`
        let (ty, ty_span) = self.type_()?;
        if matches!(self.peek().kind, TokKind::ColonColon | TokKind::ColonEq) {
            let sigil_span = self.peek().span;
            return Err(Diagnostic::error(
                "E0422",
                format!("`#{}` has no initializer", Syntax::ATTR_UNINIT),
                format!("`#{}` declares a binding you fill in later; it cannot also be given a value here", Syntax::ATTR_UNINIT),
                format!("drop the initializer — write `#{} {}: <Type>` and write to `{}` before reading it", Syntax::ATTR_UNINIT, name, name),
                Some(sigil_span),
            ));
        }
        self.finish_stmt()?;
        Ok(Stmt::Val(Binding {
            mutable: true,
            name,
            name_span,
            pattern: None,
            ty: Some(ty),
            ty_span: Some(ty_span),
            // Harmless placeholder — never evaluated; sema/codegen branch on
            // `uninit` first and use `ty` for the binding's type.
            init: Expr::Int(0, marker_span),
            is_comptime: false,
            ct: None,
            uninit: true,
            arena_view: false,
        }))
    }

    fn stmt(&mut self) -> Result<Stmt, Diagnostic> {
        match &self.peek().kind {
            // S43 (D-CASING1 follow-on): a `#Test "name" { … }` block in statement
            // position is misplaced — E0601 points at the top level.
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_TEST) => {
                let span = self.peek().span;
                self.bump(); // `#`
                self.bump(); // `Test`
                if matches!(self.peek().kind, TokKind::Str(_)) {
                    self.bump();
                }
                if matches!(self.peek().kind, TokKind::LBrace) {
                    self.bump();
                    let _ = self.block_stmts();
                } else {
                    self.sync_stmt();
                }
                Err(Diagnostic::error(
                    "E0601",
                    format!("`#{}` blocks only belong at the top of a file", Syntax::KW_TEST),
                    "test blocks group checks that `jet test` runs separately from `main`"
                        .to_string(),
                    format!(
                        "move this block to the top level, after your functions: #{} \"name\" {{ ... }}",
                        Syntax::KW_TEST
                    ),
                    Some(span),
                ))
            }
            // D-BIND1: retired binding keywords `val` / `var` → E0985, then parse
            // the old `name = e` form so `jet fmt` can migrate them to sigils.
            TokKind::Ident(n) if (n == Syntax::FOREIGN_VAL || n == Syntax::FOREIGN_VAR)
                && self.binding_target_follows() =>
            {
                let t = self.bump();
                let foreign = if let TokKind::Ident(n) = &t.kind {
                    n.clone()
                } else {
                    unreachable!()
                };
                let mutable = foreign == Syntax::FOREIGN_VAR;
                let sigil = if mutable {
                    Syntax::SIGIL_BIND_MUT
                } else {
                    Syntax::SIGIL_BIND_IMMUT
                };
                self.diags.push(Diagnostic::error(
                    "E0985",
                    format!(
                        "`{}` is no longer a binding keyword in {}",
                        foreign,
                        Syntax::LANG_NAME
                    ),
                    format!(
                        "bindings are written with a sigil: `name {} value` (immutable) or `name {} value` (mutable)",
                        Syntax::SIGIL_BIND_IMMUT,
                        Syntax::SIGIL_BIND_MUT
                    ),
                    format!("write `name {} value` instead of `{} name = value`", sigil, foreign),
                    Some(t.span),
                ));
                let binding = self.binding_after_kw(mutable)?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::KwComptime => {
                // D-WHEN1 (ratified 2026-06-19): `comptime if <cond> { … }` is
                // a compile-time conditional — not a binding. Detect by peeking
                // at the second token; `comptime NAME` is always a binding.
                if matches!(self.peek2().kind, TokKind::KwIf) {
                    let stmt = self.comptime_if_stmt()?;
                    return Ok(stmt);
                }
                let binding = self.comptime_binding()?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Ident(n) if n == Syntax::FOREIGN_LET => {
                // S14 teaching error E0009, then parse as a binding.
                let t = self.bump();
                let is_mut = matches!(self.peek().kind, TokKind::KwMutate);
                if is_mut {
                    let mut_tok = self.bump();
                    let full_span = Span::new(t.span.start, mut_tok.span.end);
                    self.diags.push(Diagnostic::error(
                        "E0009",
                        format!(
                            "{} does not use `{}`",
                            Syntax::LANG_NAME,
                            Syntax::FOREIGN_LET_MUT
                        ),
                        binding_why(),
                        format!(
                            "write `name {} value` instead of `{} name = value`",
                            Syntax::SIGIL_BIND_MUT,
                            Syntax::FOREIGN_LET_MUT
                        ),
                        Some(full_span),
                    ));
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0009",
                        format!(
                            "{} does not use `{}`",
                            Syntax::LANG_NAME,
                            Syntax::FOREIGN_LET
                        ),
                        binding_why(),
                        format!(
                            "write `name {} value` instead of `{} name = value`",
                            Syntax::SIGIL_BIND_IMMUT,
                            Syntax::FOREIGN_LET
                        ),
                        Some(t.span),
                    ));
                }
                let binding = self.binding_after_kw(is_mut)?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Ident(n)
                if n == Syntax::FOREIGN_SET && matches!(self.peek2().kind, TokKind::Ident(_)) =>
            {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0010",
                    format!(
                        "{} does not use `{}`",
                        Syntax::LANG_NAME,
                        Syntax::FOREIGN_SET
                    ),
                    binding_why(),
                    format!(
                        "write `name {} value` instead of `{} name = value`",
                        Syntax::SIGIL_BIND_IMMUT,
                        Syntax::FOREIGN_SET
                    ),
                    Some(t.span),
                ));
                let binding = self.binding_after_kw(false)?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Ident(n) if n == Syntax::FOREIGN_MATCH => {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0016",
                    format!(
                        "{} does not use `{}`",
                        Syntax::LANG_NAME,
                        Syntax::FOREIGN_MATCH
                    ),
                    format!(
                        "choosing one branch from many is written with `{}` (D-IF1)",
                        Syntax::KW_IF
                    ),
                    format!(
                        "write `{} subject {{ value {} body … }}` instead",
                        Syntax::KW_IF,
                        Syntax::OP_ARM_ARROW
                    ),
                    Some(t.span),
                ));
                self.switch_after_kw(t.span)
            }
            TokKind::Ident(n) if n == Syntax::FOREIGN_SWITCH => {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0044",
                    format!(
                        "{} does not use `{}`",
                        Syntax::LANG_NAME,
                        Syntax::FOREIGN_SWITCH
                    ),
                    format!(
                        "choosing one branch from many is written with `{}` (D-IF1)",
                        Syntax::KW_IF
                    ),
                    format!(
                        "write `{} subject {{ value {} body … }}` instead",
                        Syntax::KW_IF,
                        Syntax::OP_ARM_ARROW
                    ),
                    Some(t.span),
                ));
                self.switch_after_kw(t.span)
            }
            TokKind::KwReturn => {
                let span = self.bump().span;
                let expr = if matches!(self.peek().kind, TokKind::Semi) {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.finish_stmt()?;
                Ok(Stmt::Return(expr, span))
            }
            TokKind::KwIf => self.if_or_dispatch(),
            TokKind::KwWhile => {
                // S19-amend (E0050): `while` is now a teaching error; use `loop cond { }`.
                let t = self.bump();
                let span = t.span;
                self.diags.push(Diagnostic::error(
                    "E0050",
                    format!(
                        "`{}` is not a keyword; write `{}` instead",
                        Syntax::FOREIGN_WHILE,
                        Syntax::KW_LOOP,
                    ),
                    format!(
                        "`{}` has a single loop keyword: `loop cond {{ }}` for conditional loops",
                        Syntax::LANG_NAME,
                    ),
                    format!(
                        "replace `{}` with `{}`",
                        Syntax::FOREIGN_WHILE,
                        Syntax::KW_LOOP,
                    ),
                    Some(span),
                ));
                let cond = self.expr_no_struct_lit()?;
                self.expect(TokKind::LBrace, "to open the loop body")?;
                let body = self.block_stmts();
                Ok(Stmt::While { cond, body, span, label: None })
            }
            TokKind::KwFor => {
                // S19-amend (E0051): `for` is now a teaching error; use `loop x in ... { }`.
                let t = self.bump();
                let span = t.span;
                self.diags.push(Diagnostic::error(
                    "E0051",
                    format!(
                        "`{}` is not a keyword; write `{} x in collection {{ }}` instead",
                        Syntax::FOREIGN_FOR,
                        Syntax::KW_LOOP,
                    ),
                    format!(
                        "`{}` has a single loop keyword: `loop x in list {{ }}` for iteration",
                        Syntax::LANG_NAME,
                    ),
                    format!(
                        "replace `{}` with `{}`",
                        Syntax::FOREIGN_FOR,
                        Syntax::KW_LOOP,
                    ),
                    Some(span),
                ));
                let (var, var_span) = self.expect_ident("after the loop variable name")?;
                let mut var2 = None;
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    let (v2, s2) = self.expect_ident("after `,` in `loop key, value in`")?;
                    var2 = Some((v2, s2));
                }
                self.expect_kw(TokKind::KwIn, "after the loop name")?;
                let first = self.expr_no_struct_lit()?;
                let kind = if matches!(self.peek().kind, TokKind::DotDot) {
                    self.bump();
                    let end = self.expr_no_struct_lit()?;
                    let step = if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_RANGE_STEP)
                    {
                        self.bump();
                        Some(self.expr_no_struct_lit()?)
                    } else {
                        None
                    };
                    ForKind::Range { start: first, end, step }
                } else {
                    ForKind::In { collection: first }
                };
                self.expect(TokKind::LBrace, "to open the loop body")?;
                let body = self.block_stmts();
                Ok(Stmt::For { var, var_span, var2, kind, body, span, label: None })
            }
            // D-IF1: `when` is retired — `if` is the one branching keyword. Emit
            // E0984, then parse the old body so `jet fmt` can migrate it to
            // `if subject { arm -> body }`.
            TokKind::KwSwitch => {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0984",
                    format!(
                        "`{}` is no longer a keyword in {}",
                        Syntax::KW_SWITCH,
                        Syntax::LANG_NAME
                    ),
                    format!(
                        "`{}` is the one branching keyword — multi-arm dispatch is `{} subject {{ arm {} body }}`",
                        Syntax::KW_IF,
                        Syntax::KW_IF,
                        Syntax::OP_ARM_ARROW
                    ),
                    format!(
                        "write `{} subject {{ value {} body … }}` (an `{} {} body` catch-all)",
                        Syntax::KW_IF,
                        Syntax::OP_ARM_ARROW,
                        Syntax::KW_ELSE,
                        Syntax::OP_ARM_ARROW
                    ),
                    Some(span),
                ));
                self.switch_after_kw(span)
            }
            TokKind::KwBreak => {
                let span = self.bump().span;
                // D-LABEL1: optional `@name` targets an enclosing labeled loop.
                if matches!(self.peek().kind, TokKind::At) {
                    self.bump(); // `@`
                    let (name, _) = self.expect_ident("after `break @` for the loop label")?;
                    self.finish_stmt()?;
                    return Ok(Stmt::BreakLabel(name, span));
                }
                self.finish_stmt()?;
                Ok(Stmt::Break(span))
            }
            TokKind::KwContinue => {
                let span = self.bump().span;
                if matches!(self.peek().kind, TokKind::At) {
                    self.bump(); // `@`
                    let (name, _) = self.expect_ident("after `continue @` for the loop label")?;
                    self.finish_stmt()?;
                    return Ok(Stmt::ContinueLabel(name, span));
                }
                self.finish_stmt()?;
                Ok(Stmt::Continue(span))
            }
            TokKind::KwLoop => self.loop_stmt(None),
            // D-BIND1: a sigil binding `name (: type)? (:: | :=) expr` — no
            // leading keyword. Detected before the general Ident statement path.
            _ if self.looks_like_sigil_binding() => {
                let binding = self.sigil_binding()?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            // S58 (E2-M13): the audit + unsafe gate is `#Audit("…")` then
            // `#Unsafe { … }`. Bare `unsafe { … }` is the rejected former
            // spelling — point users at the `#` form.
            TokKind::Ident(n) if n == Syntax::FOREIGN_UNSAFE => {
                let span = self.bump().span;
                Err(Diagnostic::error(
                    "E0003",
                    format!("`{}` blocks are written with `#{}`", Syntax::FOREIGN_UNSAFE, Syntax::KW_UNSAFE),
                    "the expert low-level gate is an attribute marker, never a bare keyword"
                        .to_string(),
                    format!(
                        "write `#{}(\"why this is safe\") #{} {{ … }}`",
                        Syntax::ATTR_AUDIT,
                        Syntax::KW_UNSAFE
                    ),
                    Some(span),
                ))
            }
            TokKind::Hash => {
                // D-UNINIT1 (opt C): `#Uninit name: Type` parsing is implemented in
                // `uninit_binding` but NOT yet wired here — it stays unexposed until
                // the sema write-before-read proof (E0420) and MaybeUninit codegen land,
                // so no mis-compiling/unsafe path exists. See sidequests/visible-uninit.md.
                // D-CTX1 (ratified 2026-06-22): `#Context(field: value) { … }`.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::CTX_BLOCK) {
                    return self.at_context_stmt();
                }
                // S58 (E2-M13): `#Audit("…")` / `#Unsafe { … }` — the audited gate.
                self.at_unsafe_stmt()
            }
            TokKind::At => {
                // D-LABEL1: `@name loop … { }` is a loop label. `@` in stmt position
                // is ONLY for labels now (D-ATTR3 = B); attributes use `#`.
                if let TokKind::Ident(_) = &self.peek2().kind {
                    if matches!(self.peek3().kind, TokKind::KwLoop) {
                        self.bump(); // `@`
                        let (label, lspan) =
                            self.expect_ident("for the loop label after `@`")?;
                        return self.loop_stmt(Some((label, lspan)));
                    }
                    // `@name` not followed by `loop` — mis-placed label (E0988).
                    let at_span = self.peek().span;
                    self.bump(); // `@`
                    let (_, name_span) = self.expect_ident("for the loop label after `@`")?;
                    return Err(Diagnostic::error(
                        "E0988",
                        "a `@name` loop label must be followed by `loop`".to_string(),
                        "the `@name` label sigil attaches only to a `loop` (D-LABEL1)"
                            .to_string(),
                        "write `@name loop { … }`, or remove the label".to_string(),
                        Some(Span::new(at_span.start, name_span.end)),
                    ));
                }
                // `@` not followed by an ident — teaching error for old attribute spelling.
                let t = self.bump();
                Err(Diagnostic::error(
                    "E0990",
                    format!("attributes use `{}`, not `@`", Syntax::ATTR_PREFIX),
                    "in Jet, `@` is for loop labels; attributes and markers use `#` (D-ATTR1)".to_string(),
                    "write `#Unsafe`, `#Audit(\"…\")`, `#Numeric`, or `#[Marker, …]` instead of `@…`".to_string(),
                    Some(t.span),
                ))
            }
            // D-REGION1 (opt B): `region r { … }` — an explicit allocation
            // region. `region` is a lowercase contextual keyword (D-CASING1):
            // recognized only when followed by `name {`, so a variable named
            // `region` still works everywhere else.
            TokKind::Ident(n)
                if n == Syntax::KW_REGION
                    && matches!(&self.peek2().kind, TokKind::Ident(_))
                    && matches!(self.peek3().kind, TokKind::LBrace) =>
            {
                let start = self.bump().span; // `region`
                let (name, name_span) = self.expect_ident("for the region name")?;
                self.expect(TokKind::LBrace, "after the region name")?;
                let body = self.block_stmts();
                let end = self.toks[self.pos - 1].span.end;
                return Ok(Stmt::Region {
                    name,
                    name_span,
                    body,
                    span: Span::new(start.start, end),
                });
            }
            // `self.items.push(x);` — method bodies state effects on `self`
            // exactly like on any other name (S27).
            TokKind::Ident(_) | TokKind::KwSelf => {
                let expr = self.expr()?;
                let next = &self.peek().kind;
                if matches!(next, TokKind::Eq) || next.compound_op().is_some() {
                    let op_tok = self.bump();
                    let op = op_tok.kind.compound_op();
                    let value = self.expr()?;
                    self.finish_stmt()?;
                    let target = self.expr_to_lvalue(expr)?;
                    return Ok(Stmt::Assign {
                        target,
                        op,
                        op_span: op_tok.span,
                        value,
                    });
                }
                match &expr {
                    Expr::Call(_)
                    | Expr::Field(_, _, _)
                    | Expr::MethodCall { .. }
                    | Expr::FanOut { .. }
                    // S7: `expr?;` propagates a fallible result as a statement (E2-M7).
                    | Expr::Try(_, _, _)
                    | Expr::OrFallback { .. } => {}
                    other => {
                        return Err(Diagnostic::error(
                            "E0003",
                            "this line computes a value but doesn't do anything with it"
                                .to_string(),
                            "only calls, bindings, assignments, and `return` are allowed here".to_string(),
                            format!(
                                "use the value, e.g. `x {} ...` or `{}(...)`",
                                Syntax::SIGIL_BIND_IMMUT,
                                Syntax::BUILTIN_PRINT
                            ),
                            Some(other.span()),
                        ));
                    }
                }
                self.finish_stmt()?;
                Ok(Stmt::Expr(expr))
            }
            other => Err(Diagnostic::error(
                "E0003",
                format!("expected a call, binding, assignment, or `return`, found {}", describe(other)),
                "inside a function body, write a call, binding, assignment, or `return`"
                    .to_string(),
                format!(
                    "e.g. {}(\"hello\") or x {} 1",
                    Syntax::BUILTIN_PRINT,
                    Syntax::SIGIL_BIND_IMMUT
                ),
                Some(self.peek().span),
            )),
        }
    }

    /// D-IF1: `if subject { … }` is either a conventional two-arm `if` (its body
    /// is statements, optional `else`) or a multi-arm dispatch when the body's
    /// first item is an arm `head -> body`. One token of lookahead after the
    /// arm head decides: `->` is not a valid expression operator, so an arm head
    /// followed by `->` unambiguously marks arm mode. Multi-arm `if` lowers to
    /// the same `Stmt::Switch` IR the former `when` used.
    fn if_or_dispatch(&mut self) -> Result<Stmt, Diagnostic> {
        let span = self.bump().span; // `if`
        let subject = self.expr_no_struct_lit()?;
        self.expect(TokKind::LBrace, "to open the `if` body")?;

        // Peek for arm mode: an `else ->` first arm, or an arm head followed by
        // `->`. Speculatively parse the head and look for `->`.
        if self.if_body_is_arms() {
            return self.if_arms(subject, span);
        }

        // Conventional `if`: the subject is the condition.
        let then_body = self.block_stmts();
        let mut else_branch = None;
        if matches!(self.peek().kind, TokKind::KwElse) {
            self.bump();
            if matches!(self.peek().kind, TokKind::KwIf) {
                else_branch = Some(ElseBranch::ElseIf(Box::new(self.if_stmt()?)));
            } else {
                self.expect(TokKind::LBrace, "to open the `else` body")?;
                else_branch = Some(ElseBranch::Else(self.block_stmts()));
            }
        }
        Ok(Stmt::If(IfStmt {
            cond: subject,
            then_body,
            else_branch,
            span,
        }))
    }

    /// D-IF1: is the `if` body (cursor just past `{`) a multi-arm dispatch?
    /// True when the body opens with `else ->`, or with an expression arm head
    /// immediately followed by `->`. Pure lookahead — restores the cursor.
    fn if_body_is_arms(&mut self) -> bool {
        // `else ->` catch-all as the first (only) arm.
        if matches!(self.peek().kind, TokKind::KwElse)
            && matches!(self.peek2().kind, TokKind::Arrow)
        {
            return true;
        }
        // An empty body `{}` is a conventional (empty) if, not arm mode.
        if matches!(self.peek().kind, TokKind::RBrace) {
            return false;
        }
        // D-PATR: `Int .. Int ->` is a range arm — detect without full expr parse.
        // Also detect `Int ..= Int ->` (E0318) and `Int .. Int step Int ->` (E0319)
        // porting hazards so we can emit teaching errors rather than confusing parse failures.
        if let TokKind::Int(_) = &self.peek().kind {
            if matches!(
                self.toks.get(self.pos + 1).map(|t| &t.kind),
                Some(TokKind::DotDot)
            ) {
                // `lo .. hi ->` (4 tokens: lo, .., hi, ->)
                if let Some(tok_after_hi) = self.toks.get(self.pos + 3) {
                    if matches!(tok_after_hi.kind, TokKind::Arrow) {
                        return true;
                    }
                }
                // `lo .. hi step n ->` (6 tokens: lo, .., hi, step, n, ->)  — E0319 porting hazard
                if matches!(self.toks.get(self.pos + 3).map(|t| &t.kind), Some(TokKind::Ident(s)) if s == Syntax::KW_RANGE_STEP) {
                    if let Some(tok_after_step_n) = self.toks.get(self.pos + 5) {
                        if matches!(tok_after_step_n.kind, TokKind::Arrow) {
                            return true;
                        }
                    }
                }
                // `lo ..= hi ->` (5 tokens: lo, .., =, hi, ->)  — E0318 porting hazard
                if matches!(self.toks.get(self.pos + 2).map(|t| &t.kind), Some(TokKind::Eq)) {
                    if let Some(tok_after_eq_hi) = self.toks.get(self.pos + 4) {
                        if matches!(tok_after_eq_hi.kind, TokKind::Arrow) {
                            return true;
                        }
                    }
                }
                return false;
            }
        }
        let save = self.pos;
        let saved_diags = self.diags.len();
        let is_arm = matches!(self.expr_no_struct_lit(), Ok(_))
            && matches!(self.peek().kind, TokKind::Arrow);
        self.pos = save;
        self.diags.truncate(saved_diags);
        is_arm
    }

    /// D-IF1: parse the arms of a multi-arm `if` (cursor just past `{`), lowering
    /// to `Stmt::Switch`. Each arm is `head -> body`; `head` with no top-level
    /// comparison/logical operator is a bare value compared against the subject
    /// (`subject == head`), otherwise a full `Bool` condition (D-IF2 Q3).
    /// `body` is a braceless single statement/expression or a `{ … }` block
    /// (D-IF2 Q2). `else -> body` is the catch-all (D-IF2 Q1).
    fn if_arms(&mut self, subject: Expr, span: Span) -> Result<Stmt, Diagnostic> {
        let mut arms: Vec<SwitchArm> = Vec::new();
        let mut else_body: Option<Vec<Stmt>> = None;
        loop {
            // Skip synthetic terminators between arms (S6-R).
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            match &self.peek().kind {
                TokKind::RBrace => {
                    self.bump();
                    break;
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this `if`, found the end of the file".to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                TokKind::KwElse => {
                    let arm_start = self.bump().span; // `else`
                    self.expect(TokKind::Arrow, "after `else` in an `if`")?;
                    let body = self.arm_body()?;
                    let _ = arm_start;
                    else_body = Some(body);
                }
                _ => {
                    let arm_start = self.peek().span;
                    // D-PATR: detect `Int .. Int ->` as a range-pattern arm head.
                    // C25: also detect `Int ..= Int ->` (E0318) and `Int .. Int step N ->` (E0319).
                    let raw_head = if let TokKind::Int(lo_val) = &self.peek().kind.clone() {
                        if matches!(
                            self.toks.get(self.pos + 1).map(|t| &t.kind),
                            Some(TokKind::DotDot)
                        ) {
                            let lo = *lo_val;
                            let range_start = self.bump().span; // consume lo
                            self.bump(); // consume `..`
                            // C25/E0318: `..=` is Rust's inclusive range — Jet's `..` is already inclusive.
                            // Push the error, then recover by consuming hi and building a valid range arm.
                            if matches!(self.peek().kind, TokKind::Eq) {
                                self.bump(); // consume `=`
                                if let TokKind::Int(hi_val) = &self.peek().kind.clone() {
                                    let hi = *hi_val;
                                    let range_end = self.bump().span; // consume hi
                                    let pat_span = Span::new(range_start.start, range_end.end);
                                    self.diags.push(Diagnostic::error(
                                        "E0318",
                                        "`..=` is not a Jet operator — Jet's `..` is already inclusive".to_string(),
                                        "in Rust, `..` is exclusive and `..=` is inclusive; in Jet, `..` includes both ends".to_string(),
                                        format!("write `{}..{}` — that already includes `{}`", lo, hi, hi),
                                        Some(pat_span),
                                    ));
                                    Expr::PatternTest {
                                        subject: Box::new(subject.clone()),
                                        pattern: Pattern::Range { lo, hi, span: pat_span },
                                        span: pat_span,
                                    }
                                } else {
                                    return Err(Diagnostic::error(
                                        "E0318",
                                        "`..=` is not a Jet operator — Jet's `..` is already inclusive".to_string(),
                                        "in Rust, `..` is exclusive and `..=` is inclusive; in Jet, `..` includes both ends".to_string(),
                                        "write `lo..hi` — that already includes `hi`".to_string(),
                                        Some(self.peek().span),
                                    ));
                                }
                            } else if let TokKind::Int(hi_val) = &self.peek().kind.clone() {
                                let hi = *hi_val;
                                let range_end = self.bump().span; // consume hi
                                let pat_span = Span::new(range_start.start, range_end.end);
                                // C25/E0319: `step` after a range arm is a loop modifier, not an arm construct.
                                // Push the error and skip `step N` so the arm can still be parsed.
                                if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_RANGE_STEP) {
                                    self.diags.push(Diagnostic::error(
                                        "E0319",
                                        "`step` is not allowed in a range arm — range arms test a band, not a sequence".to_string(),
                                        "`step` belongs in a loop (`loop i in lo..hi step n`); a range arm just checks if the subject falls between the two ends".to_string(),
                                        format!("remove `step …`, or use a full condition: `subject >= {} && subject <= {} && subject % n == 0 ->`", lo, hi),
                                        Some(pat_span),
                                    ));
                                    self.bump(); // consume `step`
                                    if matches!(self.peek().kind, TokKind::Int(_)) {
                                        self.bump(); // consume step value
                                    }
                                }
                                Expr::PatternTest {
                                    subject: Box::new(subject.clone()),
                                    pattern: Pattern::Range { lo, hi, span: pat_span },
                                    span: pat_span,
                                }
                            } else {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    "expected an integer after `..` in a range arm".to_string(),
                                    "range arms need both ends: `lo..hi -> body`".to_string(),
                                    "write `0..59 -> { body }` for an inclusive range arm".to_string(),
                                    Some(self.peek().span),
                                ));
                            }
                        } else {
                            let raw = self.expr_no_struct_lit()?;
                            Self::switch_pipe_cond(subject.clone(), raw)
                        }
                    } else {
                        let raw = self.expr_no_struct_lit()?;
                        Self::switch_pipe_cond(subject.clone(), raw)
                    };
                    self.expect(TokKind::Arrow, "after an `if` arm value or condition")?;
                    let body = self.arm_body()?;
                    let end = self
                        .toks
                        .get(self.pos.saturating_sub(1))
                        .map(|t| t.span.end)
                        .unwrap_or(arm_start.end);
                    arms.push(SwitchArm {
                        cond: raw_head,
                        body,
                        span: Span::new(arm_start.start, end),
                    });
                }
            }
        }
        Ok(Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        })
    }

    /// D-IF2 Q2: an arm body is a `{ … }` block or a single braceless statement.
    fn arm_body(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            Ok(self.block_stmts())
        } else {
            // Braceless single statement (a call, binding, return, etc.).
            let stmt = self.stmt()?;
            Ok(vec![stmt])
        }
    }

    fn if_stmt(&mut self) -> Result<IfStmt, Diagnostic> {
        let span = self.bump().span; // `if`
        let cond = self.expr_no_struct_lit()?;
        self.expect(TokKind::LBrace, "to open the `if` body")?;
        let then_body = self.block_stmts();
        let mut else_branch = None;
        if matches!(self.peek().kind, TokKind::KwElse) {
            self.bump();
            if matches!(self.peek().kind, TokKind::KwIf) {
                else_branch = Some(ElseBranch::ElseIf(Box::new(self.if_stmt()?)));
            } else {
                self.expect(TokKind::LBrace, "to open the `else` body")?;
                else_branch = Some(ElseBranch::Else(self.block_stmts()));
            }
        }
        Ok(IfStmt {
            cond,
            then_body,
            else_branch,
            span,
        })
    }

    /// `switch` body, after the keyword (S24): either legacy condition arms
    /// with `->`, or pipe arms where bare terms mean `subject == term`.
    /// S68 (D-SG2): parse an `if` expression — `if cond { … value } else { … }`.
    /// Each branch is a value block; `else` is required (an `if` with no value
    /// is a statement, parsed elsewhere).
    pub(super) fn parse_if_expr(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.bump().span; // `if`
        let cond = self.expr_no_struct_lit()?;
        let (then_body, then_value) = self.parse_value_block()?;
        if !matches!(self.peek().kind, TokKind::KwElse) {
            return Err(Diagnostic::error(
                "E0003",
                "an `if` used as a value needs an `else` branch".to_string(),
                "when `if` produces a value, both branches must produce one (S68)".to_string(),
                "add `else { … }` so every path has a value".to_string(),
                Some(self.peek().span),
            ));
        }
        self.bump(); // `else`
        // `else if …` nests as the else branch's value.
        let (else_body, else_value) = if matches!(self.peek().kind, TokKind::KwIf) {
            let e = self.parse_if_expr()?;
            (Vec::new(), e)
        } else {
            self.parse_value_block()?
        };
        let span = Span::new(start.start, else_value.span().end);
        Ok(Expr::If {
            cond: Box::new(cond),
            then_body,
            then_value: Box::new(then_value),
            else_body,
            else_value: Box::new(else_value),
            span,
        })
    }

    /// S68 (D-SG2): parse `{ stmt* tail-expr }` where the trailing expression
    /// (no `;`) is the block's value. Leading statements use the ordinary
    /// statement grammar; the tail is detected by speculatively parsing an
    /// expression and checking for the closing `}`.
    fn parse_value_block(&mut self) -> Result<(Vec<Stmt>, Expr), Diagnostic> {
        self.expect(TokKind::LBrace, "to open this `if` branch")?;
        let mut stmts = Vec::new();
        loop {
            match &self.peek().kind {
                TokKind::RBrace => {
                    let span = self.peek().span;
                    self.bump();
                    return Err(Diagnostic::error(
                        "E0003",
                        "this `if` branch is empty but is used as a value".to_string(),
                        "when `if` produces a value, each branch must end with one (S68)"
                            .to_string(),
                        "put a value as the last line, like `{ x }`".to_string(),
                        Some(span),
                    ));
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this `if` branch, found the end of the file"
                            .to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                _ => {}
            }
            // Try the current position as the trailing value expression.
            let save = self.pos;
            let saved_diags = self.diags.len();
            if let Ok(e) = self.expr() {
                // S6-R: the lexer inserts a synthetic terminator after the tail
                // value too (it ends a line before `}`); accept `expr }` or
                // `expr ; }` as the block's value.
                if matches!(self.peek().kind, TokKind::Semi)
                    && matches!(self.peek2().kind, TokKind::RBrace)
                {
                    self.bump(); // synthetic `;`
                }
                if matches!(self.peek().kind, TokKind::RBrace) {
                    self.bump();
                    return Ok((stmts, e));
                }
            }
            // Not the tail value — rewind and parse an ordinary statement.
            self.pos = save;
            self.diags.truncate(saved_diags);
            match self.stmt() {
                Ok(s) => stmts.push(s),
                Err(d) => {
                    self.diags.push(d);
                    self.sync_stmt();
                }
            }
        }
    }

    fn switch_after_kw(&mut self, span: Span) -> Result<Stmt, Diagnostic> {
        let subject = self.expr_no_struct_lit()?;
        self.expect(TokKind::LBrace, "to open the `switch` body")?;
        let mut arms = Vec::new();
        let mut else_body: Option<Vec<Stmt>> = None;
        loop {
            match &self.peek().kind {
                TokKind::RBrace => {
                    self.bump();
                    break;
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this `switch`, found the end of the file"
                            .to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                TokKind::Pipe => {
                    let arm_start = self.bump().span;
                    if matches!(self.peek().kind, TokKind::KwElse) {
                        self.bump();
                        self.expect(TokKind::LBrace, "to open the `else` arm")?;
                        let body = self.block_stmts();
                        if matches!(self.peek().kind, TokKind::Semi) {
                            self.bump();
                        }
                        else_body = Some(body);
                    } else {
                        let raw_cond = self.expr_no_struct_lit()?;
                        let cond = Self::switch_pipe_cond(subject.clone(), raw_cond);
                        self.expect(TokKind::LBrace, "to open the arm's body")?;
                        let body = self.block_stmts();
                        let end = self.peek().span.end;
                        if matches!(self.peek().kind, TokKind::Semi) {
                            self.bump();
                        }
                        arms.push(SwitchArm {
                            cond,
                            body,
                            span: Span::new(arm_start.start, end),
                        });
                    }
                }
                TokKind::Ident(name)
                    if name == Syntax::FOREIGN_CASE || name == Syntax::FOREIGN_DEFAULT =>
                {
                    let t = self.bump();
                    let foreign = if let TokKind::Ident(n) = &t.kind {
                        n.clone()
                    } else {
                        unreachable!()
                    };
                    self.diags.push(Diagnostic::error(
                        "E0023",
                        format!(
                            "`{}` arms are written `value {} body`, not `{}`",
                            Syntax::KW_IF,
                            Syntax::OP_ARM_ARROW,
                            foreign
                        ),
                        format!(
                            "choosing one branch from many uses `{}` with `{}` arms (D-IF1)",
                            Syntax::KW_IF,
                            Syntax::OP_ARM_ARROW
                        ),
                        format!(
                            "replace `{}` with a value or condition and `{}`, like `1 {} body`",
                            foreign,
                            Syntax::OP_ARM_ARROW,
                            Syntax::OP_ARM_ARROW
                        ),
                        Some(t.span),
                    ));
                    self.sync_stmt();
                    continue;
                }
                TokKind::KwElse => {
                    self.bump();
                    self.expect(TokKind::Arrow, "after `else` in a `switch`")?;
                    self.expect(TokKind::LBrace, "to open the `else` arm")?;
                    let body = self.block_stmts();
                    self.expect(TokKind::Semi, "after a `switch` arm's closing `}`")?;
                    else_body = Some(body);
                }
                _ => {
                    let arm_start = self.peek().span;
                    // D-PATR: detect `Int .. Int ->` as a range-pattern arm head.
                    // C25: also detect `Int ..= Int ->` (E0318) and `Int .. Int step N ->` (E0319).
                    let cond = if let TokKind::Int(lo_val) = &self.peek().kind.clone() {
                        if matches!(
                            self.toks.get(self.pos + 1).map(|t| &t.kind),
                            Some(TokKind::DotDot)
                        ) {
                            let lo = *lo_val;
                            let range_start = self.bump().span; // consume lo
                            self.bump(); // consume `..`
                            // C25/E0318: `..=` is Rust's inclusive range — Jet's `..` is already inclusive.
                            // Push the error, then recover by consuming hi and building a valid range arm.
                            if matches!(self.peek().kind, TokKind::Eq) {
                                self.bump(); // consume `=`
                                if let TokKind::Int(hi_val) = &self.peek().kind.clone() {
                                    let hi = *hi_val;
                                    let range_end = self.bump().span; // consume hi
                                    let pat_span = Span::new(range_start.start, range_end.end);
                                    self.diags.push(Diagnostic::error(
                                        "E0318",
                                        "`..=` is not a Jet operator — Jet's `..` is already inclusive".to_string(),
                                        "in Rust, `..` is exclusive and `..=` is inclusive; in Jet, `..` includes both ends".to_string(),
                                        format!("write `{}..{}` — that already includes `{}`", lo, hi, hi),
                                        Some(pat_span),
                                    ));
                                    Expr::PatternTest {
                                        subject: Box::new(subject.clone()),
                                        pattern: Pattern::Range { lo, hi, span: pat_span },
                                        span: pat_span,
                                    }
                                } else {
                                    return Err(Diagnostic::error(
                                        "E0318",
                                        "`..=` is not a Jet operator — Jet's `..` is already inclusive".to_string(),
                                        "in Rust, `..` is exclusive and `..=` is inclusive; in Jet, `..` includes both ends".to_string(),
                                        "write `lo..hi` — that already includes `hi`".to_string(),
                                        Some(self.peek().span),
                                    ));
                                }
                            } else if let TokKind::Int(hi_val) = &self.peek().kind.clone() {
                                let hi = *hi_val;
                                let range_end = self.bump().span; // consume hi
                                let pat_span = Span::new(range_start.start, range_end.end);
                                // C25/E0319: `step` after a range arm is a loop modifier, not an arm construct.
                                // Push the error and skip `step N` so the arm can still be parsed.
                                if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_RANGE_STEP) {
                                    self.diags.push(Diagnostic::error(
                                        "E0319",
                                        "`step` is not allowed in a range arm — range arms test a band, not a sequence".to_string(),
                                        "`step` belongs in a loop (`loop i in lo..hi step n`); a range arm just checks if the subject falls between the two ends".to_string(),
                                        format!("remove `step …`, or use a full condition: `subject >= {} && subject <= {} && subject % n == 0 ->`", lo, hi),
                                        Some(pat_span),
                                    ));
                                    self.bump(); // consume `step`
                                    if matches!(self.peek().kind, TokKind::Int(_)) {
                                        self.bump(); // consume step value
                                    }
                                }
                                // Wrap as PatternTest so sema/codegen treat it uniformly.
                                Expr::PatternTest {
                                    subject: Box::new(subject.clone()),
                                    pattern: Pattern::Range { lo, hi, span: pat_span },
                                    span: pat_span,
                                }
                            } else {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    "expected an integer after `..` in a range arm".to_string(),
                                    "range arms need both ends: `lo..hi -> body`".to_string(),
                                    "write `0..59 -> { body }` for an inclusive range arm".to_string(),
                                    Some(self.peek().span),
                                ));
                            }
                        } else {
                            self.expr_no_struct_lit()?
                        }
                    } else {
                        self.expr_no_struct_lit()?
                    };
                    self.expect(TokKind::Arrow, "after a `switch` arm's condition")?;
                    self.expect(TokKind::LBrace, "to open the arm's body")?;
                    let body = self.block_stmts();
                    // Capture the `;` end so SwitchArm.span covers the full arm.
                    let semi_end = self.peek().span.end;
                    self.expect(TokKind::Semi, "after a `switch` arm's closing `}`")?;
                    arms.push(SwitchArm {
                        cond,
                        body,
                        span: Span::new(arm_start.start, semi_end),
                    });
                }
            }
        }
        Ok(Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        })
    }

    fn switch_pipe_cond(subject: Expr, cond: Expr) -> Expr {
        match cond {
            Expr::Binary(BinOp::And, lhs, rhs, span) => Expr::Binary(
                BinOp::And,
                Box::new(Self::switch_pipe_cond(subject.clone(), *lhs)),
                Box::new(Self::switch_pipe_cond(subject, *rhs)),
                span,
            ),
            Expr::Binary(BinOp::Or, lhs, rhs, span) => Expr::Binary(
                BinOp::Or,
                Box::new(Self::switch_pipe_cond(subject.clone(), *lhs)),
                Box::new(Self::switch_pipe_cond(subject, *rhs)),
                span,
            ),
            Expr::Binary(op, lhs, rhs, span) if op.is_comparison() => {
                Expr::Binary(op, lhs, rhs, span)
            }
            Expr::PatternTest { .. } | Expr::Bool(_, _) => cond,
            other => {
                let span = Span::new(subject.span().start, other.span().end);
                Expr::Binary(BinOp::Eq, Box::new(subject), Box::new(other), span)
            }
        }
    }

    /// D-BIND1: a binding starting with the target (no keyword), written
    /// `name (: type)? (:: | :=) expr`. The sigil chooses mutability:
    /// `::` immutable (was `val`), `:=` mutable (was `var`).
    fn sigil_binding(&mut self) -> Result<Binding, Diagnostic> {
        // S74: a destructuring target — `[ … ]` for a list, `Ident { … }` for a
        // struct — instead of a plain `name`.
        if let Some(pattern) = self.try_bind_pattern()? {
            let mutable = self.expect_bind_sigil()?;
            let init = self.expr()?;
            return Ok(Binding {
                mutable,
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
            });
        }
        let (name, name_span) = self.expect_ident("for the binding name")?;
        let (ty, ty_span) = if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            let (t, s) = self.type_()?;
            (Some(t), Some(s))
        } else {
            (None, None)
        };
        let mutable = self.expect_bind_sigil()?;
        let init = self.expr()?;
        Ok(Binding {
            mutable,
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
        })
    }

    /// D-BIND1: after a retired `val`/`var` keyword, does the old binding shape
    /// `name (: type)? = …` or a destructuring pattern follow? Guards the E0985
    /// teaching path so a stray identifier literally named `val` isn't captured.
    fn binding_target_follows(&self) -> bool {
        match &self.peek2().kind {
            // `val name = …` / `val name : T = …` / `val Type { … } = …`
            TokKind::Ident(_) => matches!(
                self.peek3().kind,
                TokKind::Eq | TokKind::Colon | TokKind::LBrace
            ),
            // `val [a, b] = …` (list pattern) / `val (a, b) = …` (tuple pattern).
            TokKind::LBracket | TokKind::LParen => true,
            _ => false,
        }
    }

    /// D-BIND1: consume `::` (immutable) or `:=` (mutable); returns `mutable`.
    fn expect_bind_sigil(&mut self) -> Result<bool, Diagnostic> {
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

    /// D-BIND1: true when the tokens at the cursor begin a sigil binding —
    /// `name ::`, `name :=`, `name : type ::`, or a destructuring pattern target
    /// (`[ … ] ::`, `Ident { … } ::`). Used by the statement dispatcher to tell a
    /// binding apart from an expression/assignment that also starts with a name.
    fn looks_like_sigil_binding(&self) -> bool {
        match &self.peek().kind {
            // `name :: e` / `name := e`
            TokKind::Ident(_) | TokKind::KwSelf
                if matches!(self.peek2().kind, TokKind::ColonColon | TokKind::ColonEq) =>
            {
                true
            }
            // `name : type :: e` — typed binding. A bare `:` only ever opens a
            // type annotation here, so any `name :` at statement start is a
            // binding (a map index uses `[`, not `:`).
            TokKind::Ident(_) | TokKind::KwSelf
                if matches!(self.peek2().kind, TokKind::Colon) =>
            {
                true
            }
            // Destructuring targets: scan ahead to a `::`/`:=` after the matching
            // close. Cheap bounded lookahead. `[a, b] ::`, `(a, b) ::`, and
            // `Type { … } ::`.
            TokKind::LBracket | TokKind::LParen => self.pattern_target_is_binding(),
            TokKind::Ident(_) if matches!(self.peek2().kind, TokKind::LBrace) => {
                self.pattern_target_is_binding()
            }
            _ => false,
        }
    }

    /// Scan a `[ … ]` / `( … )` / `Ident { … }` destructuring target and check
    /// whether a binding sigil follows its close. Bounded by the bracket depth.
    fn pattern_target_is_binding(&self) -> bool {
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

    /// D-BIND1: parse the old keyword-led binding `val/var name (: type)? = expr`
    /// after the retired keyword was consumed, for the E0985 teaching path.
    fn binding_after_kw(&mut self, mutable: bool) -> Result<Binding, Diagnostic> {
        // S74: a destructuring target — `[ … ]` for a list, `Ident { … }` for a
        // struct — instead of a plain `name`.
        if let Some(pattern) = self.try_bind_pattern()? {
            self.expect(TokKind::Eq, "in a binding")?;
            let init = self.expr()?;
            return Ok(Binding {
                mutable,
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
            });
        }
        let (name, name_span) = self.expect_ident("after a binding keyword")?;
        let (ty, ty_span) = if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            let (t, s) = self.type_()?;
            (Some(t), Some(s))
        } else {
            (None, None)
        };
        self.expect(TokKind::Eq, "in a binding")?;
        let init = self.expr()?;
        Ok(Binding {
            mutable,
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
        })
    }

    /// S74: parse a `val`/`var` destructuring target if one starts here.
    /// `[ a, b ]` is a list pattern; `Ident { x, y }` is a struct pattern.
    /// A bare `name` (followed by `=` or `:`) is not a pattern.
    fn try_bind_pattern(&mut self) -> Result<Option<BindPattern>, Diagnostic> {
        match &self.peek().kind {
            TokKind::LBracket => {
                let start = self.bump().span;
                let mut elems = Vec::new();
                if !matches!(self.peek().kind, TokKind::RBracket) {
                    loop {
                        let (name, span) = self.expect_ident("for a list-pattern binding")?;
                        elems.push(BindName { name, span });
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
            TokKind::Ident(_) if matches!(self.peek2().kind, TokKind::LBrace) => {
                let (type_name, type_span) = self.expect_ident("for a struct pattern")?;
                self.expect(TokKind::LBrace, "to open the struct pattern")?;
                let mut fields = Vec::new();
                if !matches!(self.peek().kind, TokKind::RBrace) {
                    loop {
                        let (name, span) = self.expect_ident("for a struct-pattern field")?;
                        fields.push(BindName { name, span });
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                }
                let end = self.peek().span;
                self.expect(TokKind::RBrace, "to close the struct pattern")?;
                Ok(Some(BindPattern::Struct {
                    type_name,
                    type_span,
                    fields,
                    span: Span::new(type_span.start, end.end),
                }))
            }
            TokKind::LParen if !self.looks_like_named_tuple(false) => {
                let start = self.bump().span;
                let mut elems = Vec::new();
                if !matches!(self.peek().kind, TokKind::RParen) {
                    loop {
                        let (name, span) =
                            self.expect_ident("for a tuple-pattern binding")?;
                        elems.push(BindName { name, span });
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

    /// D-WHEN1 (ratified 2026-06-19): parse `comptime if <cond> { … } else { … }`.
    /// Both arms require `{ }` (braceless bodies are not allowed for `comptime if`).
    /// `else` is optional in statement position. Sema selects the arm; codegen
    /// emits only the selected arm (D-WHEN2: dropped arm is name-resolved only).
    fn comptime_if_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.bump().span; // `comptime`
        self.bump(); // `if`
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

    fn comptime_binding(&mut self) -> Result<Binding, Diagnostic> {
        let kw = self.peek().span;
        self.expect_kw(TokKind::KwComptime, "to start a comptime binding")?;
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
        self.expect(TokKind::Eq, "in a comptime binding")?;
        let init = self.expr()?;
        Ok(Binding {
            mutable: false,
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
        })
    }

    // --- expressions -----------------------------------------------------

}
