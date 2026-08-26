use super::super::{describe, Diagnostic, Parser, Span, Syntax, TokKind};

impl<'a> Parser<'a> {
    /// D-MIGRATE1 as respelled by D-ARROW-CONTROL1: parse
    /// `migration TypeName { rename a -> b; … }`.
    pub(super) fn migration_decl(&mut self) -> Result<crate::AST::MigrationDecl, Diagnostic> {
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
                    self.expect_unified_arrow("between the old and new field names in `rename`")?;
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
                    self.expect_unified_arrow("between the old and new field types in `change`")?;
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
                            "a `#PublishedSchema` record is keyed by field name, so reordering fields is safe and needs no migration".to_string(),
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

    /// D-STATE-HOME1=A: true when retired `state <TypeName> {` is at the cursor
    /// (contextual), so it can receive the exact nested-form rewrite.
    /// D-VALIDATE1 (ratified 2026-07-12, card #506): true when the cursor is
    /// at `validate {` — a struct's in-body validation block. Token stream:
    /// `validate {`.
    pub(super) fn at_validate_block(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_VALIDATE_BLOCK)
            && matches!(self.peek2().kind, TokKind::LBrace)
    }

    /// D-VALIDATE1: parse `validate { … }` inside a struct body. Reuses the
    /// ordinary statement parser (`block_stmts`) — grammar-wise a rule
    /// statement is just an expression statement; sema restricts what a rule
    /// statement may legally be (`check(cond, at: field, "msg")`,
    /// purity-checked, `field` resolved as a bare sibling reference per
    /// D-FIELDPOL1) and rejects anything else with a teaching diagnostic.
    pub(super) fn validate_block(&mut self) -> Result<(Vec<crate::AST::Stmt>, Span), Diagnostic> {
        let start = self.peek().span;
        self.bump(); // `validate`
        self.expect(TokKind::LBrace, "to open the `validate` block")?;
        let stmts = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok((stmts, Span::new(start.start, end)))
    }

    pub(in crate::Parser) fn at_state_section(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_STATE_DECL)
            && matches!(self.peek2().kind, TokKind::LBrace)
    }

    pub(super) fn at_state_block(&self) -> bool {
        if !matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_STATE_DECL) {
            return false;
        }
        if !matches!(self.peek2().kind, TokKind::Ident(_)) {
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

    pub(super) fn state_decl_with_pkg(
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

    /// D-STATE-HOME1=A: parse the sole state section owned by a named struct.
    pub(super) fn state_section(
        &mut self,
        type_name: String,
        type_name_span: Span,
    ) -> Result<crate::AST::StateDecl, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // consume `state`
        self.expect(TokKind::LBrace, "to open the state section")?;
        let mut states = Vec::new();
        while !matches!(&self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(&self.peek().kind, TokKind::Comma | TokKind::Semi) {
                self.bump();
                continue;
            }
            let (name, name_span) = self.expect_ident("a state name inside `state { … }`")?;
            states.push((name, name_span));
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the state section")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::StateDecl {
            is_pub: false,
            is_package_pub: false,
            type_name,
            type_name_span,
            states,
            span: Span::new(start.start, end),
        })
    }

    /// Consume a valid nested-shaped block and report its forbidden owner.
    pub(in crate::Parser) fn reject_state_section(&mut self, owner: &str) -> Diagnostic {
        let start = self.peek().span;
        let section_span = self
            .state_section(String::new(), start)
            .map(|section| section.span)
            .unwrap_or(start);
        Diagnostic::from_row("E0158", &[("owner", owner)], Some(section_span))
    }

    /// D-STATE-HOME1=A: top-level companion declarations are recognized only
    /// to teach the exact move into `struct Type { state { … } }`.
    pub(super) fn reject_top_level_state_decl(
        &mut self,
        is_pub: bool,
        is_package_pub: bool,
    ) -> Result<crate::AST::Item, Diagnostic> {
        let declaration = self.state_decl_with_pkg(is_pub, is_package_pub)?;
        let state_names = declaration
            .states
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let states = format!("{{ {state_names} }}");
        Err(Diagnostic::from_row(
            "E0157",
            &[
                ("type", declaration.type_name.as_str()),
                ("states", states.as_str()),
            ],
            Some(declaration.span),
        ))
    }

    /// D-PROTO1/D-PROTO2: true when `protocol Name {` is at the cursor (contextual).
    pub(super) fn at_protocol_block(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_PROTOCOL)
            && matches!(&self.peek2().kind, TokKind::Ident(_))
            && matches!(&self.peek3().kind, TokKind::LBrace)
    }

    /// D-PROTO1/D-PROTO2: parse `[pub] protocol Name { … }`.
    pub(super) fn protocol_decl(
        &mut self,
        is_pub: bool,
    ) -> Result<crate::AST::ProtocolDecl, Diagnostic> {
        self.protocol_decl_with_pkg(is_pub, false)
    }

    pub(super) fn protocol_decl_with_pkg(
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

    /// D-PROTO2 as cleaned up by D-ARROW-CONTROL1: parse one sender line —
    /// `client: Hello(version: Int)` or `server: Hello(version: Int)`.
    fn protocol_message(&mut self) -> Result<crate::AST::ProtocolMessage, Diagnostic> {
        let start = self.peek().span;
        let (from, _) = self.expect_ident("the sender in `client: Msg(…)`")?;
        let direction = match from.as_str() {
            Syntax::PROTO_CLIENT => crate::AST::ProtocolDirection::ClientToServer,
            Syntax::PROTO_SERVER => crate::AST::ProtocolDirection::ServerToClient,
            other => {
                return Err(Diagnostic::error(
                    "E0154",
                    format!(
                        "protocol messages start with `{}` or `{}`, not `{other}`",
                        Syntax::PROTO_CLIENT,
                        Syntax::PROTO_SERVER
                    ),
                    "each line names the sender; the other endpoint is the receiver".to_string(),
                    format!(
                        "write `{}: …` or `{}: …`",
                        Syntax::PROTO_CLIENT,
                        Syntax::PROTO_SERVER
                    ),
                    Some(start),
                ));
            }
        };
        if self.at_unified_arrow() {
            let arrow = self.expect_unified_arrow("in the retired protocol spelling")?;
            let expected = match direction {
                crate::AST::ProtocolDirection::ClientToServer => Syntax::PROTO_SERVER,
                crate::AST::ProtocolDirection::ServerToClient => Syntax::PROTO_CLIENT,
            };
            let (receiver, receiver_span) =
                self.expect_ident("the receiver in the retired protocol spelling")?;
            if receiver != expected {
                return Err(Diagnostic::error(
                    "E0154",
                    format!("expected receiver `{expected}`, found `{receiver}`"),
                    "the retired endpoint-pair spelling still had to name the opposite endpoint"
                        .to_string(),
                    format!("write `{from}: …`"),
                    Some(receiver_span),
                ));
            }
            self.diags.push(Diagnostic::error(
                "E0154",
                "protocol lines no longer repeat the receiver".to_string(),
                "the sender determines the direction, so the transport arrow adds no information"
                    .to_string(),
                format!("write `{from}: …`; remove `-> {expected}`"),
                Some(arrow.span),
            ));
        }
        self.expect(TokKind::Colon, "after the sender in a protocol message")?;
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
    pub(super) fn user_derive_def(&mut self) -> Result<crate::AST::DeriveDef, Diagnostic> {
        let start = self.peek().span.start;
        self.bump(); // consume `derive`
        let (type_param, type_param_span) = self.expect_ident("after `derive`")?;
        let old_derive_for = matches!(&self.peek().kind, TokKind::Ident(n) if n == "for");
        if old_derive_for {
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
        let body = self.derive_body_items()?;
        if let Some(span) = Self::derive_nested_impl_span(&body) {
            return Err(Diagnostic::error(
                    "E0003",
                    format!("`derive {type_param}.{trait_name}` already names its implementation"),
                    "a derive provider body is the member list; a nested `impl` repeats the target and trait".to_string(),
                    "remove the `impl` wrapper and keep its members directly in the derive body".to_string(),
                    Some(span),
                ));
        }
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::AST::DeriveDef {
            trait_name,
            trait_span,
            type_param,
            body,
            span: Span::new(start, end),
        })
    }

    fn derive_nested_impl_span(body: &[crate::AST::DeriveBodyItem]) -> Option<Span> {
        body.iter().find_map(|body_item| match body_item {
            crate::AST::DeriveBodyItem::Item(item) => match item.as_ref() {
                crate::AST::Item::Impl(implementation) => Some(implementation.span),
                crate::AST::Item::ErrorConv(conversion) => Some(conversion.from_span),
                _ => None,
            },
            crate::AST::DeriveBodyItem::Loop { body, .. } => Self::derive_nested_impl_span(body),
            crate::AST::DeriveBodyItem::Stmt(_) => None,
        })
    }

    /// D-META-CODE1=A / D-META-BODY1=A: parse a derive body as a mixed
    /// sequence of ordinary item templates and compile-time operations.
    /// The item parser is called directly, so the definition is checked
    /// once here and never serialized to source for a second parse.
    pub(crate) fn derive_body_items(
        &mut self,
    ) -> Result<Vec<crate::AST::DeriveBodyItem>, Diagnostic> {
        self.derive_template_depth += 1;
        let result = self.derive_body_items_inner();
        self.derive_template_depth -= 1;
        result
    }

    /// D-META-CODE1=A / D-META-BODY1=A: append the one typed template
    /// argument used by `b.generate`. Both expression parsers call this
    /// helper so the build and derive surfaces cannot drift.
    pub(crate) fn parse_generate_template_arg(
        &mut self,
        member: &str,
        args: &mut Vec<crate::AST::CallArg>,
    ) -> Result<(), Diagnostic> {
        if member != "generate" || !matches!(self.peek().kind, TokKind::LBrace) {
            return Ok(());
        }
        let open = self.bump().span;
        let items = self.derive_body_items()?;
        let end = self.toks[self.pos - 1].span.end;
        let block_span = Span::new(open.start, end);
        args.push(crate::AST::CallArg {
            convention: crate::AST::AccessConvention::Read,
            expr: crate::AST::Expr::Str(Vec::new(), block_span),
            span: block_span,
            flags: crate::AST::CallArgFlags {
                template_items: Some(items),
                ..Default::default()
            },
            label: None,
            spread: false,
        });
        Ok(())
    }

    fn derive_body_items_inner(&mut self) -> Result<Vec<crate::AST::DeriveBodyItem>, Diagnostic> {
        let mut body = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
                continue;
            }
            if matches!(self.peek().kind, TokKind::KwPub | TokKind::KwPriv) {
                let (is_pub, is_package_pub) = self.parse_item_visibility();
                body.push(crate::AST::DeriveBodyItem::Item(Box::new(
                    self.item_after_visibility(is_pub, is_package_pub)?,
                )));
                continue;
            }
            let item = match self.peek().kind {
                TokKind::KwFn => Some(crate::AST::DeriveBodyItem::Item(Box::new(
                    crate::AST::Item::Func(self.func()?),
                ))),
                TokKind::KwImpl => Some(crate::AST::DeriveBodyItem::Item(Box::new(
                    self.impl_or_error_conv()?,
                ))),
                TokKind::KwStruct => Some(crate::AST::DeriveBodyItem::Item(Box::new(
                    crate::AST::Item::Struct(self.struct_def(false)?),
                ))),
                TokKind::KwEnum => Some(crate::AST::DeriveBodyItem::Item(Box::new(
                    crate::AST::Item::Enum(self.enum_def(false)?),
                ))),
                TokKind::Hash if self.at_test_def() => Some(crate::AST::DeriveBodyItem::Item(
                    Box::new(crate::AST::Item::Test(self.test_def()?)),
                )),
                TokKind::Hash if self.at_marker_list() || self.at_single_type_marker() => Some(
                    crate::AST::DeriveBodyItem::Item(Box::new(self.type_def_with_any_markers()?)),
                ),
                TokKind::Hash if self.marker_sequence_leads_to_function() => {
                    Some(crate::AST::DeriveBodyItem::Item(Box::new(
                        crate::AST::Item::Func(self.func_with_marker_list()?),
                    )))
                }
                TokKind::At if matches!(self.peek2().kind, TokKind::KwLoop) => {
                    Some(self.derive_body_loop()?)
                }
                _ => Some(crate::AST::DeriveBodyItem::Stmt(self.stmt()?)),
            };
            if let Some(item) = item {
                body.push(item);
            }
        }
        self.expect(TokKind::RBrace, "to close the derive body")?;
        Ok(body)
    }

    /// D-META-BODY1=A: `@loop field in T.@fields { fn … }` expands item
    /// templates, not runtime statements. This deliberately shares the
    /// ordinary loop source expression grammar; sema evaluates the source
    /// through the comptime interpreter during expansion.
    fn derive_body_loop(&mut self) -> Result<crate::AST::DeriveBodyItem, Diagnostic> {
        let start = self.bump().span.start; // `@`
        self.expect(TokKind::KwLoop, "after `@` in a derive loop")?;
        let (var, var_span) = self.expect_ident("for the derive loop binding")?;
        self.expect_loop_source_separator()?;
        let source = self.expr_no_struct_lit()?;
        self.expect(TokKind::LBrace, "to open the derive loop body")?;
        let body = self.derive_body_items()?;
        let end = self.toks[self.pos.saturating_sub(1)].span.end;
        Ok(crate::AST::DeriveBodyItem::Loop {
            var,
            var_span,
            source,
            body,
            span: Span::new(start, end),
        })
    }

    /// D-STRUCT-ONCE1=A: root declaration position reuses the same
    /// typed loop body as derive and marker templates.
    pub(crate) fn item_template_loop(
        &mut self,
    ) -> Result<crate::AST::ItemTemplateLoop, Diagnostic> {
        let crate::AST::DeriveBodyItem::Loop {
            var,
            var_span,
            source,
            body,
            span,
        } = self.derive_body_loop()?
        else {
            unreachable!("derive_body_loop always returns a loop item")
        };
        Ok(crate::AST::ItemTemplateLoop {
            var,
            var_span,
            source,
            body,
            span,
        })
    }

    /// D-META-NAME1=A / D-MARKER-SITES1=B: true when a canonical marker
    /// declaration (`marker Name(…)`) or a retired declaration spelling is
    /// at the cursor. The retired lookahead keeps its teaching diagnostic in
    /// the marker parser instead of producing a generic item error.
    pub(super) fn at_marker_decl(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_MARKER)
            && matches!(&self.peek2().kind, TokKind::Ident(_))
            && (matches!(&self.peek3().kind, TokKind::LParen)
                || matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::TEXT_HEAD_ON))
    }

    /// D-FACTDECL1=A: `fact Name(params…)` uses the same named-parameter
    /// declaration shape as `marker Name(params…)`.
    pub(super) fn at_fact_decl(&self) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_FACT)
            && matches!(&self.peek2().kind, TokKind::Ident(_))
            && matches!(&self.peek3().kind, TokKind::LParen)
    }

    /// D-META-FORM1=A / D-META-USER1=A / D-MARKER-SITES1=B: parse the one
    /// canonical `marker Name(params…)` declaration. Ordinary rule
    /// arguments and fixed declaration metadata share this named-parameter
    /// list, told apart by `@` marks. Retired checked-text declarations are
    /// rejected before they can become an AST marker declaration.
    pub(super) fn marker_decl(&mut self) -> Result<crate::AST::MarkerDecl, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // consume `marker`
        let (name, name_span) = self.expect_ident("the marker name in `marker Name(params…)`")?;
        if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::TEXT_HEAD_ON) {
            let span = self.peek().span;
            self.skip_marker_trailer_balanced();
            return Err(Diagnostic::error(
                "E0381",
                "the checked-text marker declaration is retired".to_string(),
                "D-MARKER-SITES1=B: every marker declaration uses one named parameter list; `@sites` records legal sites, and checked-text contracts are not a separate marker form".to_string(),
                format!("write `marker {name}(@sites: [.Text])` for a plain rule, or use a built-in typed text head"),
                Some(span),
            ));
        }
        self.expect(TokKind::LParen, "to open the marker's parameter list")?;
        let params = self.marker_decl_param_list()?;
        let mut end = self.toks[self.pos - 1].span.end;
        let body = if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            // D-META-USER1=A / D-META-CODE1=A: a marker body that adds
            // code uses the same typed item-template grammar as a derive.
            // Rejection-only statements remain ordinary body statements;
            // no source string or second parser path exists.
            let body = self.derive_body_items()?;
            end = self.toks[self.pos - 1].span.end;
            Some(body)
        } else if let Some(diagnostic) = self.reject_marker_decl_trailer() {
            self.diags.push(diagnostic);
            end = self.toks[self.pos - 1].span.end;
            None
        } else {
            None
        };
        if matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        Ok(crate::AST::MarkerDecl {
            name,
            name_span,
            params,
            body,
            span: Span::new(start.start, end),
        })
    }

    /// D-FACTDECL1=A: parse one non-code registry row. Fact columns are
    /// ordinary `@`-marked named parameters in the one shared list.
    pub(super) fn fact_decl(&mut self) -> Result<crate::AST::FactDecl, Diagnostic> {
        let start = self.peek().span;
        self.bump(); // consume `fact`
        let (name, name_span) = self.expect_ident("the fact name in `fact Name(params…)`")?;
        self.expect(TokKind::LParen, "to open the fact's parameter list")?;
        let params = self.marker_decl_param_list()?;
        let end = self.toks[self.pos - 1].span.end;
        if matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        Ok(crate::AST::FactDecl {
            name,
            name_span,
            params,
            span: Span::new(start.start, end),
        })
    }

    /// D-META-FORM1=A / D-DEFAULT-SHAPE1=B: the rule's own arguments read
    /// `name: Type{default}`, the same shape as an ordinary function
    /// parameter. A
    /// fact about the rule reads `@name: value` instead — it is a fixed
    /// property of the declaration, not something a use site supplies,
    /// so it carries a value directly rather than a type.
    fn marker_decl_param_list(&mut self) -> Result<Vec<crate::AST::MarkerDeclParam>, Diagnostic> {
        let mut params = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let marked =
                    matches!(&self.peek().kind, TokKind::Ident(n) if Syntax::is_comptime_name(n));
                // D-VERDICT-1455-1: a marker parameter is named by any
                // written word, including one the lexer reads as a keyword
                // (`#Scrub(tag: …)`, `#Layout(tag: …)`).
                let (name, name_span) = match crate::Lexer::keyword_spelling(&self.peek().kind) {
                    Some(word) => (word.to_string(), self.bump().span),
                    None => self.expect_ident(if marked {
                        "a marker fact name"
                    } else {
                        "a marker parameter name"
                    })?,
                };
                self.expect(
                    TokKind::Colon,
                    "between the parameter name and its type or value",
                )?;
                let mut variadic = false;
                let (ty, value) = if marked {
                    (None, Some(Box::new(self.expr_no_struct_lit()?)))
                } else {
                    // D-VARIADIC1: `name: ...T` is the one rest-parameter
                    // spelling, so a marker that takes a list of arguments
                    // (`#FX(Net, FS)`) writes it the same way a function does.
                    if matches!(self.peek().kind, TokKind::DotDotDot) {
                        self.bump();
                        variadic = true;
                    }
                    let (ty, _ty_span) = self.type_()?;
                    let default = if matches!(self.peek().kind, TokKind::LBrace) {
                        self.bump();
                        let default = Box::new(self.expr_no_struct_lit()?);
                        self.expect(TokKind::RBrace, "after a marker parameter default")?;
                        Some(default)
                    } else if matches!(self.peek().kind, TokKind::Eq) {
                        let eq = self.bump().span;
                        self.diags
                            .push(Diagnostic::from_row("E0385", &[], Some(eq)));
                        Some(Box::new(self.expr_no_struct_lit()?))
                    } else {
                        None
                    };
                    (Some(ty), default)
                };
                params.push(crate::AST::MarkerDeclParam {
                    name,
                    name_span,
                    ty,
                    value,
                    variadic,
                });
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between marker parameters")?;
            }
        }
        self.expect(TokKind::RParen, "to close the marker's parameter list")?;
        Ok(params)
    }

    /// D-MARKER-SITES1=B rejects a trailing `on` clause and a second
    /// parameter list. Recognize and consume rejected trailers so the
    /// writer sees one teaching error naming the ratified fix, not a
    /// cascade of unrelated parse errors.
    fn reject_marker_decl_trailer(&mut self) -> Option<Diagnostic> {
        let (code, what, why, fix): (&str, &str, &str, String) = match &self.peek().kind {
                TokKind::Ident(n) if n == Syntax::TEXT_HEAD_ON => (
                    "E0381",
                    "a trailing `on` clause isn't how a marker states a fact",
                    "D-MARKER-SITES1=B: every marker declaration uses one named parameter list; ordinary arguments are typed parameters and `@` names are fixed metadata values — not a clause after the list",
                    "move the sites into the parameter list as `@sites: [.Function, …]`".to_string(),
                ),
                TokKind::LParen => (
                    "E0381",
                    "a second parameter list isn't how a marker states a fact",
                    "D-MARKER-SITES1=B: the rule's own typed arguments and fixed metadata share one named-parameter list, told apart by the compile-time `@` sigil — not two parameter lists",
                    "fold the second list's facts into the first as `@sites: […]`, `@repeatable: true`".to_string(),
                ),

                _ => return None,
            };
        let span = self.peek().span;
        self.skip_marker_trailer_balanced();
        Some(Diagnostic::error(
            code,
            what.to_string(),
            why.to_string(),
            fix,
            Some(span),
        ))
    }

    /// Consume the rejected trailer `reject_marker_decl_trailer` just
    /// identified — the `on` word plus its bracketed list or the second
    /// parameter list — so parsing resumes cleanly after the one teaching
    /// diagnostic.
    fn skip_marker_trailer_balanced(&mut self) {
        let mut depth: i32 = 0;
        let mut opened = false;
        loop {
            match &self.peek().kind {
                TokKind::Eof => return,
                TokKind::LParen | TokKind::LBrace | TokKind::LBracket => {
                    depth += 1;
                    opened = true;
                    self.bump();
                }
                TokKind::RParen | TokKind::RBrace | TokKind::RBracket => {
                    if depth == 0 {
                        return; // don't eat an enclosing close
                    }
                    self.bump();
                    depth -= 1;
                    if depth == 0 {
                        if matches!(self.peek().kind, TokKind::LBrace) {
                            continue;
                        }
                        return;
                    }
                }
                TokKind::Semi if depth == 0 => {
                    self.bump();
                    return;
                }
                _ => {
                    if depth == 0 && opened {
                        return;
                    }
                    self.bump();
                }
            }
        }
    }
}

#[cfg(test)]
mod marker_decl_tests {
    use crate::{Lexer, Parser, AST};

    /// D-META-NAME1=A / D-META-FORM1=A: the ratified shape from the ballot's
    /// own worked example — named parameters, a fact marked with `@`.
    #[test]
    fn ratified_named_parameter_form_parses_with_an_at_marked_fact() {
        let source = "marker Inline(mode: InlineMode, @sites: [.Function, .Method, .Constant])\nfn run() {}\n";
        let (tokens, lex_diags) = Lexer::lex(source);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let program = Parser::parse(&tokens).expect("ratified marker declaration must parse");
        let decl = program
            .items
            .iter()
            .find_map(|item| match item {
                AST::Item::MarkerDecl(decl) => Some(decl),
                _ => None,
            })
            .expect("a MarkerDecl item");
        assert_eq!(decl.name, "Inline");
        assert_eq!(decl.params.len(), 2);
        assert_eq!(decl.params[0].name, "mode");
        assert_eq!(decl.params[1].name, "@sites");
    }

    /// D-MARKER-SITES1=B: an empty site value is still a fixed metadata value,
    /// and every published site name uses the same list grammar.
    #[test]
    fn empty_and_every_published_site_value_parse_in_the_same_list() {
        let no_parameters = "marker NoParameters()\nfn run() {}\n";
        let (tokens, lex_diags) = Lexer::lex(no_parameters);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let program = Parser::parse(&tokens).expect("zero declaration parameters must parse");
        let decl = program
            .items
            .iter()
            .find_map(|item| match item {
                AST::Item::MarkerDecl(decl) => Some(decl),
                _ => None,
            })
            .expect("a NoParameters marker declaration");
        assert!(decl.params.is_empty());

        let empty = "marker Empty(@sites: [], @repeatable: false)\nfn run() {}\n";
        let (tokens, lex_diags) = Lexer::lex(empty);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let program = Parser::parse(&tokens).expect("empty metadata list must parse");
        let decl = program
            .items
            .iter()
            .find_map(|item| match item {
                AST::Item::MarkerDecl(decl) => Some(decl),
                _ => None,
            })
            .expect("an Empty marker declaration");
        assert_eq!(decl.params.len(), 2);
        assert!(decl.params.iter().all(|param| param.ty.is_none()));
        assert!(decl.params.iter().all(|param| param.value.is_some()));

        let many = "marker Many(@sites: [.Type, .Field], @repeatable: true, @inherits: false, @scopes: [.Type, .Field], @resolution: .Merge)\nfn run() {}\n";
        let (tokens, lex_diags) = Lexer::lex(many);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let program = Parser::parse(&tokens).expect("many metadata values must parse");
        let decl = program
            .items
            .iter()
            .find_map(|item| match item {
                AST::Item::MarkerDecl(decl) => Some(decl),
                _ => None,
            })
            .expect("a Many marker declaration");
        assert_eq!(decl.params.len(), 5);
        assert!(decl.params.iter().all(|param| param.ty.is_none()));
        assert!(decl.params.iter().all(|param| param.value.is_some()));

        for site in jet_foundation::Policy::RuleSite::ALL {
            let source = format!(
                "marker Uses(@sites: [.{site}])\nfn run() {{}}\n",
                site = site.name()
            );
            let (tokens, lex_diags) = Lexer::lex(&source);
            assert!(lex_diags.is_empty(), "{site:?}: {lex_diags:?}");
            Parser::parse(&tokens).unwrap_or_else(|diagnostics| {
                panic!("{site:?} must use the canonical @sites list: {diagnostics:?}")
            });
        }
    }

    /// D-FACTDECL1=A: fact declarations reuse the marker parameter AST shape.
    #[test]
    fn fact_declaration_parses_its_law_columns() {
        let source = concat!(
            "fact Exactness(@holds: .Value, @safe: .Gain, ",
            "@gates: [approx, raw], @decision: \"D-TEST\")\n",
            "fn run() {}\n"
        );
        let (tokens, lex_diags) = Lexer::lex(source);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let program = Parser::parse(&tokens).expect("fact declaration must parse");
        let decl = program
            .items
            .iter()
            .find_map(|item| match item {
                AST::Item::FactDecl(decl) => Some(decl),
                _ => None,
            })
            .expect("a FactDecl item");
        assert_eq!(decl.name, "Exactness");
        assert_eq!(
            decl.params
                .iter()
                .map(|param| param.name.as_str())
                .collect::<Vec<_>>(),
            ["@holds", "@safe", "@gates", "@decision"]
        );
        assert!(decl
            .params
            .iter()
            .all(|param| param.ty.is_none() && param.value.is_some()));
    }

    #[test]
    fn former_text_contract_is_rejected() {
        let source = concat!(
            "\nmarker Selector",
            " on [.Text] {\n",
            "    check @body\n",
            "    hole @value\n",
            "}\nfn run() {}\n"
        );
        let (tokens, lex_diags) = Lexer::lex(source);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let diagnostics =
            Parser::parse(&tokens).expect_err("retired marker form must be rejected");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "E0381");
        assert!(diagnostics[0].what.contains("retired"));
        assert!(diagnostics[0].fix.contains("@sites"));
    }

    /// D-META-FORM1=A: `@repeatable` is a named parameter like every other
    /// fact about a rule, never a trailing word. A new fact about rules is a
    /// new named parameter, so the list stays open-ended and the grammar does
    /// not grow. `@sites` takes `[Site]`, the eighteen-member menu published in
    /// `core.compiler.lang` (`Policy::SITE_VARIANTS`).
    #[test]
    fn a_fact_about_the_rule_is_one_more_named_parameter() {
        let source = "marker Pre(condition: String, message: String, @sites: [.Function, .Method], @repeatable: true)\nfn run() {}\n";
        let (tokens, lex_diags) = Lexer::lex(source);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let program = Parser::parse(&tokens).expect("ratified marker declaration must parse");
        let decl = program
            .items
            .iter()
            .find_map(|item| match item {
                AST::Item::MarkerDecl(decl) => Some(decl),
                _ => None,
            })
            .expect("a MarkerDecl item");
        let names: Vec<&str> = decl
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect();
        assert_eq!(names, ["condition", "message", "@sites", "@repeatable"]);

        // A fact about the rule carries a value, not a type; an argument the
        // use site supplies carries a type.
        for param in &decl.params {
            if param.name.starts_with('@') {
                assert!(param.ty.is_none(), "{}", param.name);
                assert!(param.value.is_some(), "{}", param.name);
            } else {
                assert!(param.ty.is_some(), "{}", param.name);
            }
        }

        // The site names `@sites` may hold are exactly the published menu.
        for site in jet_foundation::Policy::RuleSite::ALL {
            assert!(jet_foundation::Policy::SITE_VARIANTS.contains(&site.name()));
        }
    }

    /// D-META-FORM1=A rejects a trailing `on` clause and a second parameter
    /// list in favor of `@`-marked named parameters in the declaration's own
    /// list. D-MARKER-SITES1=B also retires the former checked-text branch.
    #[test]
    fn rejected_spellings_each_teach_the_ratified_named_parameter_form() {
        let source = concat!(
            "\nmarker Inline(mode: String)",
            " on [.Function]\n\n",
            "marker Pre(condition: String)",
            "(sites: [.Function])\n\nfn run() {}\n"
        );
        let (tokens, lex_diags) = Lexer::lex(source);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let diagnostics = match Parser::parse(&tokens) {
            Ok(_) => Vec::new(),
            Err(diagnostics) => diagnostics,
        };
        let codes: Vec<_> = diagnostics.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(codes, ["E0381", "E0381"], "{diagnostics:?}");
        for diagnostic in &diagnostics {
            assert!(
                diagnostic.fix.contains('@'),
                "fix must name the ratified @-marked named-parameter form: {diagnostic:?}"
            );
        }
    }
}

#[cfg(test)]
mod state_section_tests {
    use crate::Diagnostics::Span;
    use crate::{AST, Lexer, Parser};

    #[test]
    fn nested_state_section_is_stored_on_struct_not_as_an_item() {
        let source = "struct Door {\n    state { Open, Closed }\n    value: Int\n}\nfn run() {}\n";
        let (tokens, lex_diags) = Lexer::lex(source);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let program = Parser::parse(&tokens).expect("nested state section must parse");
        let definition = program
            .items
            .iter()
            .find_map(|item| match item {
                AST::Item::Struct(definition) => Some(definition),
                _ => None,
            })
            .expect("the struct item");
        let state = definition.state.as_ref().expect("the struct-owned state set");
        assert_eq!(
            state.states.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(),
            ["Open", "Closed"]
        );
    }

    #[test]
    fn top_level_state_companion_is_rejected_with_a_spanned_rewrite() {
        let source = "state Door { Open, Closed }\nstruct Door { value: Int }\nfn run() {}\n";
        let (tokens, lex_diags) = Lexer::lex(source);
        assert!(lex_diags.is_empty(), "{lex_diags:?}");
        let diagnostics = Parser::parse(&tokens).expect_err("the retired companion must fail");
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].code, "E0157");
        assert!(diagnostics[0].span.is_some());
        assert!(diagnostics[0].fix.contains("struct Door"));
        assert!(diagnostics[0].fix.contains("state { Open, Closed }"));
    }

    #[test]
    fn ineligible_state_section_uses_one_registered_parser_diagnostic() {
        for (source, owner) in [
            ("enum Door { state { Open } }\nfn run() {}\n", "enum"),
            ("trait Door { state { Open } }\nfn run() {}\n", "trait"),
            ("alias Door :: state { Open };\nfn run() {}\n", "alias"),
            ("impl Door { state { Open } }\nfn run() {}\n", "impl"),
            ("module Door<T> { state { Open } }\nfn run() {}\n", "module"),
            (
                "fn run() { value :: { state { Open } } }\n",
                "anonymous shape",
            ),
        ] {
            let (tokens, lex_diags) = Lexer::lex(source);
            assert!(lex_diags.is_empty(), "{owner}: {lex_diags:?}");
            let diagnostics = Parser::parse(&tokens).expect_err(owner);
            assert_eq!(diagnostics.len(), 1, "{owner}: {diagnostics:?}");
            assert_eq!(diagnostics[0].code, "E0158", "{owner}: {diagnostics:?}");
            assert!(diagnostics[0].what.contains(owner), "{owner}: {diagnostics:?}");
            let start = source.find("state {").expect("state section start");
            let end = start + source[start..].find('}').expect("state section end") + 1;
            assert_eq!(
                diagnostics[0].span,
                Some(Span::new(start, end)),
                "{owner}: {diagnostics:?}"
            );
        }
    }
}
