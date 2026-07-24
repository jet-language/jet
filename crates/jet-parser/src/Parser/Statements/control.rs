use super::super::*;
use super::bindings::desugar_layout_anchors;

impl<'a> Parser<'a> {
    pub(in super::super) fn at_meta_attr(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_META)
            && matches!(self.peek3().kind, TokKind::LParen)
    }

    pub(in super::super) fn meta_attr_next_kind(&self) -> Option<&TokKind> {
        if !self.at_meta_attr() {
            return None;
        }
        let mut depth = 0usize;
        let mut i = self.pos + 2;
        while let Some(tok) = self.toks.get(i) {
            match tok.kind {
                TokKind::LParen => depth += 1,
                TokKind::RParen => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        while matches!(
            self.toks.get(i).map(|t| &t.kind),
            Some(TokKind::Semi)
        ) {
            i += 1;
        }
        self.toks.get(i).map(|t| &t.kind)
    }

    pub(in super::super) fn parse_meta_attr(&mut self) -> Result<MetaAttr, Diagnostic> {
        let start = self.peek().span;
        self.expect(TokKind::Hash, "expected `#`")?;
        let (_, name_span) = self.expect_ident(&format!("`#{}`", Syntax::ATTR_META))?;
        self.expect(TokKind::LParen, &format!("after `#{}`", Syntax::ATTR_META))?;
        let mut fields = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (name, field_span) = self.expect_ident("for a `#Meta` field")?;
                if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    let value = self.expr_no_struct_lit()?;
                    let span = Span::new(field_span.start, value.span().end);
                    if name == Syntax::META_FIELD_CATEGORY {
                        fields.push(MetaField::Category { value, span });
                    } else if name == Syntax::META_FIELD_MATURITY {
                        let valid = matches!(&value,
                            Expr::EnumLit { type_name, variant, args, .. }
                                if type_name.is_empty()
                                    && args.is_empty()
                                    && matches!(variant.as_str(),
                                        Syntax::ATTR_EXPERIMENTAL
                                            | Syntax::ATTR_TESTED
                                            | Syntax::ATTR_HARDENED));
                        if !valid {
                            self.diags.push(Diagnostic::error(
                                "E0352",
                                "`#Meta` maturity needs a known maturity value".to_string(),
                                "maturity metadata is a closed documentation scale".to_string(),
                                "write `maturity: .Experimental`, `.Tested`, or `.Hardened`".to_string(),
                                Some(value.span()),
                            ));
                        }
                        fields.push(MetaField::Maturity { value, span });
                    } else {
                        fields.push(MetaField::Unknown {
                            name,
                            value: Some(value),
                            span: field_span,
                        });
                    }
                } else if name == Syntax::META_FIELD_TUNABLE {
                    fields.push(MetaField::Tunable { span: field_span });
                } else {
                    fields.push(MetaField::Unknown {
                        name,
                        value: None,
                        span: field_span,
                    });
                }
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    if matches!(self.peek().kind, TokKind::RParen) {
                        break;
                    }
                    continue;
                }
                break;
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RParen, &format!("to close `#{}`", Syntax::ATTR_META))?;
        Ok(MetaAttr {
            fields,
            span: Span::new(start.start, end.max(name_span.end)),
        })
    }

    pub(in super::super) fn meta_attr_wrong_place_diag(&self, span: Span, target: &str) -> Diagnostic {
        Diagnostic::error(
            "E0349",
            "`#Meta` attaches to a binding, const, or function".to_string(),
            "`#Meta` is a tooling fact about a named source item; expressions do not carry it"
                .to_string(),
            format!("move `#Meta(...)` before a {target}, or remove it"),
            Some(span),
        )
    }

    pub(super) fn at_statement_switch_stmt(&mut self, marker: &str) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.expect(TokKind::Hash, "expected `#`")?;
        let name_tok = self.bump();
        let attr_span = Span::new(start.start, name_tok.span.end);

        if matches!(self.peek().kind, TokKind::Hash)
            && matches!(
                &self.peek2().kind,
                TokKind::Ident(n) if n == Syntax::ATTR_OFF || n == Syntax::ATTR_DEBUG_ONLY
            )
        {
            let second_start = self.peek().span;
            let second_name = match &self.peek2().kind {
                TokKind::Ident(n) => n.clone(),
                _ => String::new(),
            };
            let second_end = self.peek2().span.end;
            self.diags.push(Diagnostic::error(
                "E0344",
                "only one switch-off attribute can be written on a statement".to_string(),
                format!(
                    "`#{}` and `#{}` both control whether the same statement emits code",
                    marker, second_name
                ),
                format!(
                    "keep one marker: `#{} <statement>` or `#{} <statement>`",
                    Syntax::ATTR_OFF,
                    Syntax::ATTR_DEBUG_ONLY
                ),
                Some(Span::new(second_start.start, second_end)),
            ));
        }

        let (body, end) = if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            let body = self.block_stmts();
            let end = self.toks[self.pos - 1].span.end;
            (body, end)
        } else {
            let stmt = self.stmt()?;
            let end = stmt.span().end;
            (vec![stmt], end)
        };
        let span = Span::new(attr_span.start, end);
        if marker == Syntax::ATTR_OFF {
            Ok(Stmt::Off { body, span })
        } else {
            Ok(Stmt::DebugOnly { body, span })
        }
    }

    fn at_policy_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let mut declarations = self.policy_decl(crate::Policy::PolicyScope::Block)?;
        self.expect(TokKind::LBrace, "after a block policy")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        let start = declarations.first().map(|d| d.span.start).unwrap_or(end);
        let span = Span::new(start, end);
        for declaration in &mut declarations { declaration.target = Some(span); }
        self.policy_declarations.extend(declarations.clone());
        Ok(Stmt::Policy { declarations, body, span })
    }

    /// D-UNSAFE2 (ratified 2026-06-22, opt B): parse `#Unsafe("reason") { … }`
    /// in statement position. The reason string is the argument of `#Unsafe`
    /// itself; the separate `#Audit` marker is retired (E0055 teaching error).
    /// A missing reason is allowed by the grammar and flagged in sema (L3101).
    pub(super) fn at_unsafe_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        // E0055: `#Audit("…")` is the retired spelling — teach the new form.
        if matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_AUDIT)
        {
            let audit_start = self.peek().span;
            self.bump(); // `#`
            self.bump(); // `Audit`
                         // Consume the optional `("…")` argument so we can keep parsing.
            if matches!(self.peek().kind, TokKind::LParen) {
                self.bump(); // `(`
                             // skip the string argument
                let _ = self.expect_plain_string(
                    "for the audit reason",
                    "`#Audit` is retired; write `#Unsafe(\"reason\") { … }` instead",
                    "write: #Unsafe(\"index checked against len\") { … }",
                );
                let _ = self.expect(TokKind::RParen, "after the audit reason");
            }
            // Skip synthetic line terminator between `#Audit(…)` and `#Unsafe`.
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            let audit_span = Span::new(audit_start.start, self.toks[self.pos - 1].span.end);
            self.diags.push(Diagnostic::error(
                "E0055",
                format!(
                    "`#{}` is retired — merge the reason into `#{}`",
                    Syntax::ATTR_AUDIT,
                    Syntax::KW_UNSAFE
                ),
                "D-UNSAFE2 merged the audit reason into the gate itself".to_string(),
                format!(
                    "write `#{}(\"why this is safe\") {{ … }}` (drop the separate `#{}` line)",
                    Syntax::KW_UNSAFE,
                    Syntax::ATTR_AUDIT
                ),
                Some(audit_span),
            ));
        }
        // Required `#Unsafe`.
        if !(matches!(self.peek().kind, TokKind::Hash)
            && matches!(self.peek2().kind, TokKind::KwUnsafe))
        {
            return Err(Diagnostic::error(
                "E0003",
                format!("expected `#{}` here", Syntax::KW_UNSAFE),
                "an audited region opens with `#Unsafe(\"reason\") { … }`".to_string(),
                format!(
                    "write `#{}(\"why this is safe\") {{ … }}`",
                    Syntax::KW_UNSAFE
                ),
                Some(self.peek().span),
            ));
        }
        self.bump(); // `#`
        self.bump(); // `Unsafe`
        // Optional reason plus D-UNSAFE-OBLIG1 per-site selection:
        // `#Unsafe("reason", obligations: .Track) { … }`.
        let mut audit = None;
        let mut obligation_mode = None;
        if matches!(self.peek().kind, TokKind::LParen) {
            self.bump(); // `(`
            if matches!(self.peek().kind, TokKind::Str(_)) {
                let (reason, _) = self.expect_plain_string(
                    "for the safety reason",
                    "`#Unsafe` takes quoted text explaining why the block is safe",
                    "write: #Unsafe(\"index checked against len\") { … }",
                )?;
                audit = Some(reason);
                if matches!(self.peek().kind, TokKind::Comma) { self.bump(); }
            }
            if !matches!(self.peek().kind, TokKind::RParen) {
                let (field, field_span) = self.expect_ident("for the `#Unsafe` option")?;
                if field != "obligations" {
                    return Err(Diagnostic::error("E3108", format!("`{field}` is not an unsafe-gate option"), "per-site control has one typed field: `obligations`".to_string(), "write `obligations: .Track` or `obligations: .Skip`".to_string(), Some(field_span)));
                }
                self.expect(TokKind::Colon, "after `obligations`")?;
                self.expect(TokKind::Dot, "before the obligation mode")?;
                let (mode, mode_span) = self.expect_ident("after `obligations: .`")?;
                obligation_mode = Some(match mode.as_str() {
                    "Track" => crate::Policy::PolicyValue::UnsafeTrack,
                    "Skip" => crate::Policy::PolicyValue::UnsafeSkip,
                    _ => return Err(Diagnostic::error("E3108", format!("`.{mode}` is not a per-site obligation mode"), "a gate either tracks typed obligations or explicitly skips them when policy permits".to_string(), "write `.Track` or `.Skip`".to_string(), Some(mode_span))),
                });
            }
            self.expect(TokKind::RParen, "after the safety reason")?;
        }
        self.expect(TokKind::LBrace, "after `#Unsafe(…)`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        let span = Span::new(start.start, end);
        if let Some(value) = obligation_mode {
            self.policy_declarations.push(crate::Policy::PolicyDeclaration {
                key: crate::Policy::PolicyKey::Unsafe,
                value,
                scope: crate::Policy::PolicyScope::Block,
                span: Span::new(start.start, self.toks[self.pos - 1].span.end),
                target: Some(span),
                source: "<source>".to_string(),
            });
        }
        Ok(Stmt::Unsafe {
            audit,
            body,
            span,
        })
    }

    /// D-CTEFFECT1 (ratified 2026-06-25): parse `#Impure("reason") { … }` in
    /// statement position. Mirrors `at_unsafe_stmt`. Missing reason → L3102 in sema.
    pub(super) fn at_impure_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // `#`
        self.bump(); // `Impure`
                     // Optional `("reason")` argument — absent means L3102 fires in sema.
        let mut reason = None;
        if matches!(self.peek().kind, TokKind::LParen) {
            self.bump(); // `(`
            let (r, _) = self.expect_plain_string(
                "for the impure reason",
                "`#Impure` takes one piece of quoted text explaining why ambient I/O is needed here",
                "write: #Impure(\"reading build config\") { … }",
            )?;
            self.expect(TokKind::RParen, "after the impure reason")?;
            reason = Some(r);
        }
        self.expect(TokKind::LBrace, "after `#Impure(…)`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Impure {
            reason,
            body,
            span: Span::new(start.start, end),
        })
    }

    /// D-REACTCORE1 (ratified 2026-06-27, opt D): parse `#Reactive { … }` in
    /// statement position. Lowers to a reactive effect scope at codegen.
    pub(super) fn at_reactive_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.expect(TokKind::Hash, "expected `#`")?;
        let _ = self.expect_ident(&format!("`#{}`", Syntax::KW_REACTIVE))?;
        self.expect(TokKind::LBrace, "after `#Reactive`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Reactive {
            body,
            span: Span::new(start.start, end),
        })
    }

    /// D-SHIELDNAME1=A (ratified 2026-07-11): parse `#Shield { … }` in statement
    /// position. Bare block only — no argument list. `#Shield(...)` is E0430.
    /// Lowers to `jet_scheduler_shield_enter`/`_leave` around the body at codegen.
    pub(super) fn at_shield_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.expect(TokKind::Hash, "expected `#`")?;
        let name_tok = self.bump(); // `Shield`
        if matches!(self.peek().kind, TokKind::LParen) {
            let lparen = self.peek().span;
            return Err(Diagnostic::error(
                "E0430",
                "`#Shield` takes no arguments".to_string(),
                "a shield region protects whatever runs inside it; there is nothing to configure"
                    .to_string(),
                "write `#Shield { … }`".to_string(),
                Some(Span::new(name_tok.span.start, lparen.end)),
            ));
        }
        self.expect(TokKind::LBrace, "after `#Shield`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Shield {
            body,
            span: Span::new(start.start, end),
        })
    }

    /// D-BLOCKPLANE1=A: `#Region(name) { … }`.
    pub(super) fn at_region_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.bump().span; // `#`
        self.bump(); // `Region`
        self.expect(TokKind::LParen, "after `#Region`")?;
        let (name, name_span) = self.expect_ident("for the region name")?;
        self.expect(TokKind::RParen, "after the region name")?;
        self.expect(TokKind::LBrace, "after `#Region(name)`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Region { name, name_span, body, span: Span::new(start.start, end) })
    }

    /// D-BLOCKPLANE1=A: `#Live { … }`.
    pub(super) fn at_live_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.bump().span; // `#`
        self.bump(); // `Live`
        self.expect(TokKind::LBrace, "after `#Live`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Live { body, span: Span::new(start.start, end) })
    }

    /// D-BLOCKPLANE1=A: audited `#Nondeterministic("reason") { … }`.
    pub(super) fn at_nondeterministic_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.bump().span; // `#`
        self.bump(); // `Nondeterministic`
        self.expect(TokKind::LParen, "after `#Nondeterministic`")?;
        let (reason, _) = self.expect_plain_string(
            "for the nondeterminism reason",
            "`#Nondeterministic` requires quoted audit text",
            "write: #Nondeterministic(\"OS clock is an explicit input\") { … }",
        )?;
        self.expect(TokKind::RParen, "after the nondeterminism reason")?;
        self.expect(TokKind::LBrace, "after `#Nondeterministic(…)`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::AssumeDet { reason, body, span: Span::new(start.start, end) })
    }

    /// D-CTX1 (ratified 2026-06-22, G2): parse `#Context(field: value, …) { … }`.
    /// Cursor is on the `#` token. Emits E0760 for `=` spelling, E0761 for
    /// unknown fields.
    pub(super) fn at_context_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // `#`
        self.bump(); // `Context`
        self.expect(TokKind::LParen, &format!("after `#{}`", Syntax::CTX_BLOCK))?;
        let mut fields: Vec<(String, Expr, Span)> = Vec::new();
        // Parse comma-separated `ident : expr` pairs.
        while !matches!(self.peek().kind, TokKind::RParen | TokKind::Eof) {
            let field_start = self.peek().span;
            let (field_name, field_name_span) = self.expect_ident("for the context field name")?;
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
            self.expect(
                TokKind::Colon,
                &format!("after the field name `{}`", field_name),
            )?;
            // E0761: unknown field name.
            if field_name != Syntax::CTX_FIELD_ALLOCATOR
                && field_name != Syntax::CTX_FIELD_LOGGER
                && field_name != Syntax::CTX_FIELD_DEADLINE
            {
                return Err(Diagnostic::error(
                    "E0761",
                    format!("`{}` isn't a context field", field_name),
                    "the context bundle holds `allocator`, `logger`, and `deadline`".to_string(),
                    format!(
                        "write `#{}(allocator: …)`, `#{}(logger: …)`, or `#{}(deadline: …)`",
                        Syntax::CTX_BLOCK,
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
        self.expect(
            TokKind::RParen,
            &format!("after `#{}(…`", Syntax::CTX_BLOCK),
        )?;
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

    /// D-EFF1 / D-QUAL1: parse a `#Caps(Net, Db) { … }` effect-restriction region
    /// in statement position. Cursor is on the `#` token. Effect names are bare
    /// idents; sema validates them against the known effect vocabulary (E0119).
    pub(super) fn at_caps_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // `#`
        self.bump(); // `Caps`
        let lparen = self.peek().span;
        self.expect(TokKind::LParen, &format!("after `#{}`", Syntax::KW_CAPS))?;
        let mut caps = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (name, span) = self.expect_effect_path_name("for an effect name")?;
                caps.push((name, span));
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between effects in the list")?;
            }
        }
        let rparen = self.peek().span;
        self.expect(
            TokKind::RParen,
            &format!("to close the `#{}(…)` list", Syntax::KW_CAPS),
        )?;
        self.expect(TokKind::LBrace, &format!("after `#{}(…)`", Syntax::KW_CAPS))?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Caps {
            caps,
            caps_span: Span::new(lparen.start, rparen.end),
            body,
            span: Span::new(start.start, end),
        })
    }

    /// D-SCAP1: parse a `#Grant(Fs) { caps -> … }` scoped-capability grant region
    /// in statement position. Cursor is on the `#` token. Effect names are bare
    /// idents (sema validates them, E0119); `caps` binds the first-class
    /// capability handle for the block. The dual of `#Caps`: `#Grant` authorizes
    /// the listed effects through the handle, RAII-revoked at scope end.
    pub(super) fn at_grant_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // `#`
        self.bump(); // `Grant`
        let lparen = self.peek().span;
        self.expect(TokKind::LParen, &format!("after `#{}`", Syntax::KW_GRANT))?;
        let mut caps = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (name, span) = self.expect_effect_path_name("for an effect name")?;
                caps.push((name, span));
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between effects in the list")?;
            }
        }
        let rparen = self.peek().span;
        self.expect(
            TokKind::RParen,
            &format!("to close the `#{}(…)` list", Syntax::KW_GRANT),
        )?;
        self.expect(
            TokKind::LBrace,
            &format!("after `#{}(…)`", Syntax::KW_GRANT),
        )?;
        // The capability handle binding: `{ caps -> … }`.
        let (binding, binding_span) = self.expect_ident("for the capability handle name")?;
        self.expect(
            TokKind::Arrow,
            &format!(
                "after the `#{}` handle name (`#{}(…) {{ caps {} … }}`)",
                Syntax::KW_GRANT,
                Syntax::KW_GRANT,
                Syntax::GRANT_ARROW
            ),
        )?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Grant {
            caps,
            caps_span: Span::new(lparen.start, rparen.end),
            binding,
            binding_span,
            body,
            span: Span::new(start.start, end),
        })
    }

    /// D-TXN4: parse a `#Transact(name) { … }` transaction block in statement
    /// position. Cursor is on the `#` token. `name` binds a user-chosen
    /// transaction handle (any ident, mirroring `region r { … }`).
    pub(super) fn at_transact_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // `#`
        self.bump(); // `Transact`
                     // D-TXN4: `#Transact(name) { … }` binds a handle; a bare `#Transact { … }`
                     // (no handle, hence no `on_commit` hooks) stays legal.
        let (name, name_span) = if matches!(self.peek().kind, TokKind::LParen) {
            self.bump(); // `(`
            let (n, ns) = self.expect_ident("for the transaction handle name")?;
            self.expect(
                TokKind::RParen,
                &format!("to close `#{}(name`", Syntax::KW_TRANSACT),
            )?;
            (Some(n), Some(ns))
        } else {
            (None, None)
        };
        self.expect(
            TokKind::LBrace,
            &format!("after `#{}`", Syntax::KW_TRANSACT),
        )?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Transact {
            name,
            name_span,
            body,
            span: Span::new(start.start, end),
        })
    }

    /// S19 + D-LOOPLABEL3: parse a `loop` statement (all three header forms), with an
    /// optional compile-time name already parsed by the caller. The cursor is on the
    /// `loop` keyword.
    pub(super) fn loop_stmt(&mut self, label: Option<(String, Span)>) -> Result<Stmt, Diagnostic> {
        let span = self.bump().span; // `loop`
                                     // S19-amend: `loop` handles all three loop forms by header.
                                     //   loop { }               → infinite
                                     //   loop cond { }          → conditional (was `while`)
                                     //   loop x; ... { }      → iteration (was `for`)
                                     //   loop k, v; ... { }   → key-value iteration
        if matches!(self.peek().kind, TokKind::LBrace) {
            // Infinite loop
            self.bump();
            let body = self.block_stmts();
            Ok(Stmt::Loop { body, span, label })
        } else if matches!(self.peek().kind, TokKind::Ident(_))
            && matches!(self.peek2().kind, TokKind::ColonEq | TokKind::Colon)
        {
            // D-LOOP-HEADER2=A: state loop. Only a plain mutable name binding
            // may initialize state; sigil_binding owns the optional type grammar.
            let init = self.sigil_binding()?;
            if !init.mutable || init.pattern.is_some() || init.name.is_empty() {
                return Err(Diagnostic::error(
                    "E0003",
                    "loop state needs one mutable name binding".to_string(),
                    "state changes between loop turns, so its header starts with `name := value`"
                        .to_string(),
                    "write `loop name[: Type] := value; condition { ... }`".to_string(),
                    Some(init.name_span),
                ));
            }
            self.expect(
                TokKind::Semi,
                "after the state initializer",
            )?;
            let cond = self.expr_no_struct_lit()?;
            let step = if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                let step_expr = self.expr()?;
                let step = if matches!(self.peek().kind, TokKind::Eq)
                    || self.peek().kind.compound_op().is_some()
                {
                    let op_tok = self.bump();
                    let op = op_tok.kind.compound_op();
                    let value = self.expr()?;
                    let target = self.expr_to_lvalue(step_expr)?;
                    Stmt::Assign {
                        target,
                        op,
                        op_span: op_tok.span,
                        value,
                    }
                } else {
                    Stmt::Expr(step_expr)
                };
                Some(Box::new(step))
            } else {
                None
            };
            self.expect(TokKind::LBrace, "to open the loop body")?;
            let body = self.block_stmts();
            Ok(Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                span,
                label,
            })
        } else if matches!(&self.peek().kind, TokKind::Ident(_))
            && matches!(&self.peek2().kind, TokKind::Semi | TokKind::Comma)
        {
            // D-LOOP-HEADER2=A: `loop x; source [; stride]` (or map k,v).
            let (var, var_span) = self.expect_ident("as the loop variable")?;
            let mut var2 = None;
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                let (v2, s2) = self.expect_ident("after `,` in `loop key, value; source`")?;
                var2 = Some((v2, s2));
            }
            self.expect(TokKind::Semi, "after the loop source binding")?;
            let first = self.expr_no_struct_lit()?;
            let kind = if matches!(self.peek().kind, TokKind::DotDot) {
                self.bump();
                let end = self.expr_no_struct_lit()?;
                let step = if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    Some(self.expr_no_struct_lit()?)
                } else {
                    None
                };
                ForKind::Range {
                    start: first,
                    end,
                    step,
                }
            } else {
                let step = if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                    Some(self.expr_no_struct_lit()?)
                } else {
                    None
                };
                ForKind::In { collection: first, step }
            };
            self.expect(TokKind::LBrace, "to open the loop body")?;
            let body = self.block_stmts();
            Ok(Stmt::For {
                var,
                var_span,
                var2,
                kind,
                body,
                span,
                label,
            })
        } else {
            // Conditional: loop cond { }
            let cond = self.expr_no_struct_lit()?;
            self.expect(TokKind::LBrace, "to open the loop body")?;
            let body = self.block_stmts();
            Ok(Stmt::While {
                cond,
                body,
                span,
                label,
            })
        }
    }

    /// Parse statements until the closing `}` (consumed). Recovers at
    /// statement boundaries so several problems surface in one run.
    pub(in super::super) fn block_stmts(&mut self) -> Vec<Stmt> {
        let body_start = self.toks[self.pos.saturating_sub(1)].span.end;
        let mut body = Vec::new();
        let body_end = loop {
            match &self.peek().kind {
                TokKind::RBrace => {
                    let end = self.peek().span.start;
                    self.bump();
                    break end;
                }
                // S6-R: a block statement (`if`/`loop`/`#Unsafe`/nested `{}`)
                // ends with `}`, after which the lexer inserts a synthetic
                // terminator. Those statements don't consume their own
                // terminator, so skip a stray one here.
                TokKind::Semi => {
                    self.bump();
                }
                TokKind::Eof => {
                    let end = self.peek().span.start;
                    self.diags.push(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this block, found the end of the file".to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                    break end;
                }
                _ => match self.stmt() {
                    Ok(s) => body.push(s),
                    Err(d) => {
                        self.diags.push(d);
                        self.sync_stmt();
                    }
                },
            }
        };
        self.block_spans.push(Span::new(body_start, body_end));
        body
    }

    pub(super) fn stmt(&mut self) -> Result<Stmt, Diagnostic> {
        match &self.peek().kind {
            // S43 (D-CASING1 follow-on): a `#Test "name" { … }` block in statement
            // position is misplaced — E0601 points at the top level.
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_TEST) =>
            {
                let span = self.peek().span;
                self.bump(); // `#`
                self.bump(); // `Test`
                             // Recovery: consume `("name")` or bare `"name"` (old form) before `{`.
                if matches!(self.peek().kind, TokKind::LParen) {
                    self.bump(); // `(`
                    if matches!(self.peek().kind, TokKind::Str(_)) {
                        self.bump();
                    }
                    if matches!(self.peek().kind, TokKind::RParen) {
                        self.bump(); // `)`
                    }
                } else if matches!(self.peek().kind, TokKind::Str(_)) {
                    self.bump(); // old bare-string form
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
                    "test blocks group checks that `jet test` runs separately from `run`"
                        .to_string(),
                    format!(
                        "move this block to the top level, after your functions: #{} (\"name\") {{ ... }}",
                        Syntax::KW_TEST
                    ),
                    Some(span),
                ))
            }
            TokKind::KwComptime => {
                // D-WHEN1 (ratified 2026-06-19): `comptime if <cond> { … }` is
                // a compile-time conditional — not a binding. Detect by peeking
                // at the second token; `comptime NAME` is always a binding.
                if matches!(self.peek2().kind, TokKind::KwIf) {
                    let stmt = self.comptime_if_stmt()?;
                    return Ok(stmt);
                }
                // D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }` block.
                if matches!(self.peek2().kind, TokKind::LBrace) {
                    let stmt = self.comptime_block_stmt()?;
                    return Ok(stmt);
                }
                let binding = self.comptime_binding()?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Ident(n) if retired_s14_teaching_enabled() && n == Syntax::FOREIGN_MATCH => {
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
            TokKind::Ident(n) if retired_s14_teaching_enabled() && n == Syntax::FOREIGN_SWITCH => {
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
            TokKind::Ident(n) if n == Syntax::KW_DEFER => {
                let defer_span = self.bump().span;
                let close = self.expr()?;
                let valid = matches!(
                    &close,
                    Expr::Call(call)
                        if call.name == Syntax::RESOURCE_CLOSE
                            && call.args.len() == 1
                            && call.args[0].convention == AccessConvention::Move
                            && matches!(call.args[0].expr, Expr::Ident(..))
                );
                if !valid {
                    return Err(Diagnostic::error(
                        "E0003",
                        "`defer` only schedules a consuming resource close".to_string(),
                        "Jet has no general deferred-action mechanism; resource cleanup stays explicit and ownership-checked".to_string(),
                        "write `defer close(^resource)`".to_string(),
                        Some(Span::new(defer_span.start, close.span().end)),
                    ));
                }
                let close_span = close.span();
                self.finish_stmt()?;
                Ok(Stmt::Expr(Expr::Call(Call {
                    name: Syntax::INTERNAL_DEFER_CLOSE.to_string(),
                    name_span: defer_span,
                    args: vec![CallArg {
                        convention: AccessConvention::Read,
                        expr: close,
                        span: close_span,
                        flags: Default::default(),
                        label: None,
                        spread: false,
                    }],
                    range_checked: false,
                })))
            }
            TokKind::Ident(n) if n == Syntax::KW_ASSERT && matches!(self.peek2().kind, TokKind::Ident(_)) => {
                let assert_span = self.bump().span;
                let mut args = Vec::new();
                loop {
                    let (name, span) = self.expect_ident("after `assert`")?;
                    args.push(CallArg {
                        convention: AccessConvention::Read,
                        expr: Expr::Ident(name, span),
                        span,
                        flags: Default::default(),
                        label: None,
                        spread: false,
                    });
                    if !matches!(self.peek().kind, TokKind::Comma) { break; }
                    self.bump();
                }
                self.finish_stmt()?;
                Ok(Stmt::Expr(Expr::Call(Call {
                    name: Syntax::INTERNAL_UNSAFE_ASSERT.to_string(),
                    name_span: assert_span,
                    args,
                    range_checked: false,
                })))
            }
            TokKind::KwYield => {
                let span = self.bump().span;
                let expr = self.expr()?;
                self.finish_stmt()?;
                Ok(Stmt::Yield(expr, span))
            }
            TokKind::KwIf => self.if_or_dispatch(),
            TokKind::KwWhile if retired_s14_teaching_enabled() => {
                // D-S14-PAUSE: `while` teaching is paused.
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
                Ok(Stmt::While {
                    cond,
                    body,
                    span,
                    label: None,
                })
            }
            TokKind::KwFor if retired_s14_teaching_enabled() => {
                // D-S14-PAUSE: `for` teaching is paused.
                let t = self.bump();
                let span = t.span;
                self.diags.push(Diagnostic::error(
                    "E0051",
                    format!(
                        "`{}` is not a keyword; write `{} x; collection {{ }}` instead",
                        Syntax::FOREIGN_FOR,
                        Syntax::KW_LOOP,
                    ),
                    format!(
                        "`{}` has a single loop keyword: `loop x; list {{ }}` for iteration",
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
                    let (v2, s2) = self.expect_ident("after `,` in the loop binding")?;
                    var2 = Some((v2, s2));
                }
                self.expect(TokKind::Semi, "after the loop binding")?;
                let first = self.expr_no_struct_lit()?;
                let kind = if matches!(self.peek().kind, TokKind::DotDot) {
                    self.bump();
                    let end = self.expr_no_struct_lit()?;
                    let step = if matches!(self.peek().kind, TokKind::Semi) {
                        self.bump();
                        Some(self.expr_no_struct_lit()?)
                    } else {
                        None
                    };
                    ForKind::Range {
                        start: first,
                        end,
                        step,
                    }
                } else {
                    ForKind::In { collection: first, step: None }
                };
                self.expect(TokKind::LBrace, "to open the loop body")?;
                let body = self.block_stmts();
                Ok(Stmt::For {
                    var,
                    var_span,
                    var2,
                    kind,
                    body,
                    span,
                    label: None,
                })
            }
            // D-S14-PAUSE: `when` teaching is paused.
            TokKind::KwSwitch if retired_s14_teaching_enabled() => {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0984",
                    format!(
                        "`{}` is no longer a keyword in {}",
                        Syntax::KW_SWITCH,
                        Syntax::LANG_NAME
                    ),
                    format!(
                        "`{}` is the one branching keyword — multi-arm dispatch is `{} subject == {{ arm {} body }}`",
                        Syntax::KW_IF,
                        Syntax::KW_IF,
                        Syntax::OP_ARM_ARROW
                    ),
                    format!(
                        "write `{} subject == {{ value {} body … }}` (an `{} {} body` catch-all)",
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
                // D-LOOPLABEL3=A: retired `break name@` / `break @name`.
                if let TokKind::Ident(_) = &self.peek().kind {
                    if matches!(self.peek2().kind, TokKind::At) {
                        let (name, name_span) = self.expect_ident("for the loop label")?;
                        let end = self.bump().span.end; // `@`
                        self.diags.push(Diagnostic::error(
                            "E0988",
                            "named loop exits use dot calls".to_string(),
                            "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                                .to_string(),
                            format!("write `{name}.break()`"),
                            Some(Span::new(name_span.start, end)),
                        ));
                        self.finish_stmt()?;
                        return Ok(Stmt::BreakLabel(name, span));
                    }
                }
                if matches!(self.peek().kind, TokKind::At) {
                    let at_span = self.peek().span;
                    self.bump();
                    let (name, name_span) = self.expect_ident("after `@` for the loop label")?;
                    self.diags.push(Diagnostic::error(
                        "E0988",
                        "named loop exits use dot calls".to_string(),
                        "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                            .to_string(),
                        format!("write `{name}.break()`"),
                        Some(Span::new(at_span.start, name_span.end)),
                    ));
                    self.finish_stmt()?;
                    return Ok(Stmt::BreakLabel(name, span));
                }
                self.finish_stmt()?;
                Ok(Stmt::Break(span))
            }
            TokKind::Ident(name)
                if name == Syntax::KW_NEXT
                    && (matches!(self.peek2().kind, TokKind::Semi | TokKind::RBrace)
                        || matches!(self.peek2().kind, TokKind::At)
                        || matches!(self.peek2().kind, TokKind::Ident(_))
                            && matches!(self.peek3().kind, TokKind::At)) =>
            {
                let span = self.bump().span;
                // D-LOOPLABEL3=A: retired `next name@` / `next @name`.
                if let TokKind::Ident(_) = &self.peek().kind {
                    if matches!(self.peek2().kind, TokKind::At) {
                        let (name, name_span) = self.expect_ident("for the loop label")?;
                        let end = self.bump().span.end; // `@`
                        self.diags.push(Diagnostic::error(
                            "E0988",
                            "named loop exits use dot calls".to_string(),
                            "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                                .to_string(),
                            format!("write `{name}.next()`"),
                            Some(Span::new(name_span.start, end)),
                        ));
                        self.finish_stmt()?;
                        return Ok(Stmt::ContinueLabel(name, span));
                    }
                }
                if matches!(self.peek().kind, TokKind::At) {
                    let at_span = self.peek().span;
                    self.bump();
                    let (name, name_span) = self.expect_ident("after `@` for the loop label")?;
                    self.diags.push(Diagnostic::error(
                        "E0988",
                        "named loop exits use dot calls".to_string(),
                        "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                            .to_string(),
                        format!("write `{name}.next()`"),
                        Some(Span::new(at_span.start, name_span.end)),
                    ));
                    self.finish_stmt()?;
                    return Ok(Stmt::ContinueLabel(name, span));
                }
                self.finish_stmt()?;
                Ok(Stmt::Continue(span))
            }
            TokKind::Ident(name)
                if name == Syntax::FOREIGN_CONTINUE
                    && matches!(self.peek2().kind, TokKind::Semi | TokKind::RBrace) =>
            {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0003",
                    "Jet spells this loop step `next`, not `continue`".to_string(),
                    "`next` skips the rest of the current loop pass and starts the next one"
                        .to_string(),
                    "write `next`".to_string(),
                    Some(span),
                ));
                self.finish_stmt()?;
                Ok(Stmt::Continue(span))
            }
            // D-LOOPLABEL3=A: named loop exits are statement-shaped dot calls.
            TokKind::Ident(_)
                if matches!(self.peek2().kind, TokKind::Dot)
                    && (matches!(self.peek3().kind, TokKind::KwBreak)
                        || matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::KW_NEXT))
                    && matches!(self.peek4().kind, TokKind::LParen)
                    && matches!(self.peek5().kind, TokKind::RParen) =>
            {
                let (name, name_span) = self.expect_ident("for the loop name")?;
                self.bump(); // `.`
                let method = self.bump();
                self.bump(); // `(`
                let end = self.bump().span.end; // `)`
                self.finish_stmt()?;
                let span = Span::new(name_span.start, end);
                if matches!(method.kind, TokKind::KwBreak) {
                    Ok(Stmt::BreakLabel(name, span))
                } else {
                    Ok(Stmt::ContinueLabel(name, span))
                }
            }
            TokKind::KwLoop => self.loop_stmt(None),
            // D-LOOPLABEL3=A: `name :: loop { }` declares a compile-time loop name.
            TokKind::Ident(_)
                if matches!(self.peek2().kind, TokKind::ColonColon)
                    && matches!(self.peek3().kind, TokKind::KwLoop) =>
            {
                let (label, lspan) = self.expect_ident("for the loop name")?;
                self.bump(); // `::`
                self.loop_stmt(Some((label, lspan)))
            }
            // `name := loop` cannot declare a loop name: labels are compile-time.
            TokKind::Ident(_)
                if matches!(self.peek2().kind, TokKind::ColonEq)
                    && matches!(self.peek3().kind, TokKind::KwLoop) =>
            {
                let (label, lspan) = self.expect_ident("for the loop name")?;
                self.bump(); // `:=`
                let end = self.bump().span.end; // `loop`
                Err(Diagnostic::error(
                    "E0988",
                    "a loop name is compile-time, not mutable state".to_string(),
                    "`:=` creates a runtime binding; a loop name only targets control flow"
                        .to_string(),
                    format!("write `{label} :: loop {{ … }}`"),
                    Some(Span::new(lspan.start, end)),
                ))
            }
            // D-LOOPLABEL3: retired suffix declaration, recovered for one teaching error.
            TokKind::Ident(_)
                if matches!(self.peek2().kind, TokKind::At)
                    && matches!(self.peek3().kind, TokKind::KwLoop | TokKind::KwFor) =>
            {
                let (label, lspan) = self.expect_ident("for the loop label")?;
                let end = self.bump().span.end; // `@`
                self.diags.push(Diagnostic::error(
                    "E0988",
                    "named loops use `::`, not `@`".to_string(),
                    "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                        .to_string(),
                    format!("write `{label} :: loop {{ … }}`"),
                    Some(Span::new(lspan.start, end)),
                ));
                self.loop_stmt(Some((label, lspan)))
            }
            TokKind::Hash if self.at_meta_attr() => {
                let meta = self.parse_meta_attr()?;
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
                if !self.looks_like_sigil_binding() {
                    return Err(self.meta_attr_wrong_place_diag(meta.span, "binding"));
                }
                let mut binding = self.sigil_binding()?;
                binding.meta = Some(meta);
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_TRACK) =>
            {
                let marker_span = self.peek().span;
                self.bump(); // `#`
                let (_, name_span) = self.expect_ident(&format!("`#{}`", Syntax::ATTR_TRACK))?;
                let mut binding = self.sigil_binding()?;
                binding.track = true;
                binding.track_span = Some(Span::new(marker_span.start, name_span.end));
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            // D-BIND1: a sigil binding `name (: type)? (:: | :=) expr` — no
            // leading keyword. Detected before the general Ident statement path.
            _ if self.looks_like_sigil_binding() => {
                let binding = self.sigil_binding()?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            // S58 (E2-M13): bare `unsafe { … }` is the rejected former
            // spelling — point users at the `#Unsafe("…")` form.
            TokKind::Ident(n) if n == Syntax::FOREIGN_UNSAFE => {
                let span = self.bump().span;
                Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "`{}` blocks are written with `#{}`",
                        Syntax::FOREIGN_UNSAFE,
                        Syntax::KW_UNSAFE
                    ),
                    "the expert low-level gate is an attribute marker, never a bare keyword"
                        .to_string(),
                    format!(
                        "write `#{}(\"why this is safe\") {{ … }}`",
                        Syntax::KW_UNSAFE
                    ),
                    Some(span),
                ))
            }
            // D-DOTSCOPE1: a scope-member statement `.name { … }` /
            // `.name(args) { … }`. The ident after the dot separates it from
            // `.{ }` construction (S74) and the required trailing block from a
            // leading-dot enum value (D-ENUMDOT1). Parsed context-free wherever
            // the shape appears; sema resolves it against the enclosing marker's
            // vocabulary (E0614) or rejects it outside a marker block (E0615).
            TokKind::Dot
                if matches!(&self.peek2().kind, TokKind::Ident(_))
                    && matches!(self.peek3().kind, TokKind::LBrace | TokKind::LParen) =>
            {
                return self.scope_member_stmt();
            }
            TokKind::Hash
                if matches!(&self.peek2().kind, TokKind::Ident(n) if matches!(n.as_str(),
                    Syntax::ATTR_AUDIT
                    | Syntax::CTX_BLOCK
                    | Syntax::ATTR_REGION
                    | Syntax::ATTR_POLICY
                    | Syntax::ATTR_LIVE
                    | Syntax::ATTR_NONDETERMINISTIC
                    | Syntax::KW_CAPS
                    | Syntax::KW_GRANT
                    | Syntax::KW_TRANSACT
                    | Syntax::KW_IMPURE
                    | Syntax::KW_SHIELD
                    | Syntax::KW_REACTIVE
                    | Syntax::ATTR_OFF
                    | Syntax::ATTR_DEBUG_ONLY))
                    || matches!(self.peek2().kind, TokKind::KwUnsafe) =>
            {
                if let TokKind::Ident(name) = &self.peek2().kind {
                    if crate::Policy::applied_rule(name).is_some() && !crate::Policy::rule_allows(name, crate::Policy::RuleSite::Block) {
                        return Err(Diagnostic::error("E0355", format!("`#{name}` cannot attach to a block"), "the compiler-owned rule registry gives every applied rule exact attachment sites".to_string(), "move the rule to one of its registered sites".to_string(), Some(self.peek2().span)));
                    }
                }
                // D-CTX1 (ratified 2026-06-22): `#Context(field: value) { … }`.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::CTX_BLOCK) {
                    return self.at_context_stmt();
                }
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_REGION) {
                    return self.at_region_stmt();
                }
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_POLICY) {
                    return self.at_policy_stmt();
                }
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_LIVE) {
                    return self.at_live_stmt();
                }
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_NONDETERMINISTIC) {
                    return self.at_nondeterministic_stmt();
                }
                // D-EFF1 / D-QUAL1: `#Caps(Net, Db) { … }` effect-restriction region.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_CAPS) {
                    return self.at_caps_stmt();
                }
                // D-SCAP1: `#grant(Fs) { caps -> … }` scoped-capability grant region.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_GRANT) {
                    return self.at_grant_stmt();
                }
                // D-TXN1–D-TXN4: `#Transact(name) { … }` transaction block.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_TRANSACT) {
                    return self.at_transact_stmt();
                }
                // D-CTEFFECT1: `#Impure("reason") { … }` comptime effect gate.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_IMPURE) {
                    return self.at_impure_stmt();
                }
                // D-SHIELDNAME1=A: `#Shield { … }` cancellation-shield region.
                // Dispatch on the name alone so `#Shield(...)` still routes here
                // to emit the E0430 teaching error.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_SHIELD) {
                    return self.at_shield_stmt();
                }
                // D-REACTCORE1: `#Reactive { … }` reactive effect scope.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_REACTIVE)
                    && matches!(self.peek3().kind, TokKind::LBrace)
                {
                    return self.at_reactive_stmt();
                }
                // D-CANVASSTATE1=D: statement switch-off attributes.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_OFF) {
                    return self.at_statement_switch_stmt(Syntax::ATTR_OFF);
                }
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::ATTR_DEBUG_ONLY) {
                    return self.at_statement_switch_stmt(Syntax::ATTR_DEBUG_ONLY);
                }
                // D-UNSAFE2: `#Unsafe("reason") { … }` (or retired `#Audit("…") #Unsafe`).
                self.at_unsafe_stmt()
            }
            // D-PERSIST1 (E0145): `#Persist` on a local binding — persistence
            // is keyed by module + name, and a local has no stable identity
            // across a reload. Takes priority over the loop-label-typo arm
            // below for the same reason as the directive-marker guard.
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::CONTRACT_PERSIST) =>
            {
                let t = self.bump(); // `@`
                let name_tok = self.bump(); // `Persist`
                Err(Diagnostic::error(
                    "E0145",
                    "only module-level state can persist across reloads".to_string(),
                    "persistence is keyed by module + name; a local has no stable identity across a reload".to_string(),
                    "move it to module level, or drop `#Persist`".to_string(),
                    Some(Span::new(t.span.start, name_tok.span.end)),
                ))
            }
            TokKind::At => {
                // D-LOOPLABEL3: recover retired `@name loop`.
                if matches!(self.peek2().kind, TokKind::Ident(_))
                    && matches!(self.peek3().kind, TokKind::KwLoop)
                {
                    let at_span = self.peek().span;
                    self.bump(); // `@`
                    let (label, lspan) = self.expect_ident("for the loop label after `@`")?;
                    self.diags.push(Diagnostic::error(
                        "E0988",
                        "named loops use `::`, not `@`".to_string(),
                        "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                            .to_string(),
                        format!("write `{label} :: loop {{ … }}`"),
                        Some(Span::new(at_span.start, lspan.end)),
                    ));
                    return self.loop_stmt(Some((label, lspan)));
                }
                let t = self.bump();
                Err(Diagnostic::error(
                    "E0063",
                    "applied rules use `#`, not `@`".to_string(),
                    "`#` marks attributes, instructions, and properties; `@` marks locations, addresses, and sources (D-VERDICT-732-1)".to_string(),
                    "replace the leading `@` with `#`".to_string(),
                    Some(t.span),
                ))
            }
            // D-TASKSCOPE1=A: `taskgroup g { … }` — structured task scope.
            TokKind::Ident(n)
                if n == Syntax::KW_TASKGROUP
                    && matches!(&self.peek2().kind, TokKind::Ident(_))
                    && matches!(self.peek3().kind, TokKind::LBrace) =>
            {
                let start = self.bump().span; // `taskgroup`
                let (name, name_span) = self.expect_ident("for the task group name")?;
                self.expect(TokKind::LBrace, "after the task group name")?;
                let body = self.block_stmts();
                let end = self.toks[self.pos - 1].span.end;
                return Ok(Stmt::TaskGroup {
                    name,
                    name_span,
                    body,
                    span: Span::new(start.start, end),
                });
            }
            // D-LAYOUT1 / D-LAYOUT-GATES1: `layout form { … }` — a Cassowary-
            // style constraint block. `layout` is a lowercase contextual
            // keyword (D-CASING1): recognized only when followed by `name {`,
            // so a variable named `layout` still works everywhere else.
            // `box.anchor` reads inside the body are desugared here (parse
            // time, purely structural) into `NAME.h(box, anchor)` /
            // `NAME.v(box, anchor)` method calls — GATE 1/2's general
            // HVar/VVar/LengthVar/Constraint machinery does all real
            // checking downstream; no parallel sema path.
            TokKind::Ident(n)
                if n == Syntax::KW_LAYOUT
                    && matches!(&self.peek2().kind, TokKind::Ident(_))
                    && matches!(self.peek3().kind, TokKind::LBrace) =>
            {
                let start = self.bump().span; // `layout`
                let (name, name_span) = self.expect_ident("for the layout name")?;
                self.expect(TokKind::LBrace, "after the layout name")?;
                self.in_layout_body += 1;
                let mut body = self.block_stmts();
                self.in_layout_body -= 1;
                let end = self.toks[self.pos - 1].span.end;
                for stmt in &mut body {
                    desugar_layout_anchors(&name, stmt);
                }
                return Ok(Stmt::Layout {
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
                    // D-CTMARKER1=C: `$name;` as a standalone statement — valid in comptime contexts.
                    | Expr::ComptimeSplice { .. }
                    // S7: `expr?;` propagates a fallible result as a statement (E2-M7).
                    | Expr::Try(_, _, _)
                    | Expr::OrFallback { .. }
                    | Expr::IncDec { .. } => {}
                    // D-LAYOUT1: inside a `layout NAME { … }` body, a bare
                    // `>=`/`<=`/`==` line is a constraint statement — GATE 1
                    // gives it a real side effect (registers into the
                    // solver), so it isn't a no-op the way an ordinary
                    // comparison-as-statement would be. Sema (E2932/E2933)
                    // still enforces that it's actually a valid constraint.
                    Expr::Binary(op, ..)
                        if self.in_layout_body > 0
                            && matches!(op, BinOp::Ge | BinOp::Le | BinOp::Eq) => {}
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
                format!(
                    "expected a call, binding, assignment, or `return`, found {}",
                    describe(other)
                ),
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

}
