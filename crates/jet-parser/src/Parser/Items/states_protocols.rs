use super::super::{Diagnostic, Parser, Span, Syntax, TokKind, describe};

impl<'a> Parser<'a> {
        /// D-MIGRATE1 as respelled by D-ARROW-CONTROL1: parse
        /// `migration TypeName { rename a => b; … }`.
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
                    // D-MIGRATE1: `rename old => new`.
                    TokKind::Ident(kw) if kw == Syntax::KW_RENAME => {
                        let (from, from_span) = self.expect_ident("as the field to rename")?;
                        self.expect(
                            TokKind::LambdaArrow,
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
                    // D-MIGRATE2E: `change field: Old => New [via { expr }]`.
                    TokKind::Ident(kw) if kw == Syntax::KW_CHANGE => {
                        let (field, field_span) =
                            self.expect_ident("as the field whose type changes")?;
                        self.expect(TokKind::Colon, "after the changed field name")?;
                        let (from_ty, from_span) = self.type_()?;
                        self.expect(
                            TokKind::LambdaArrow,
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
                            "use `rename old => new`, `add f: T = default`, `remove f`, or `change f: Old => New via { … }`".to_string(),
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
                            "write `rename fieldA => fieldB` (or `add` / `remove` / `change`)".to_string(),
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

        pub(super) fn at_state_block(&self) -> bool {
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
        pub(super) fn state_decl(&mut self, is_pub: bool) -> Result<crate::AST::StateDecl, Diagnostic> {
            self.state_decl_with_pkg(is_pub, false)
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
    
        /// D-PROTO1/D-PROTO2: true when `protocol Name {` is at the cursor (contextual).
        pub(super) fn at_protocol_block(&self) -> bool {
            matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_PROTOCOL)
                && matches!(&self.peek2().kind, TokKind::Ident(_))
                && matches!(&self.peek3().kind, TokKind::LBrace)
        }
    
        /// D-PROTO1/D-PROTO2: parse `[pub] protocol Name { … }`.
        pub(super) fn protocol_decl(&mut self, is_pub: bool) -> Result<crate::AST::ProtocolDecl, Diagnostic> {
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
            if matches!(self.peek().kind, TokKind::Arrow) {
                let arrow = self.bump();
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
            self.expect(
                TokKind::Colon,
                "after the sender in a protocol message",
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

        /// D-META-NAME1=A: true when `marker Name(` is at the cursor (contextual,
        /// like `state`/`protocol`).
        pub(super) fn at_marker_decl(&self) -> bool {
            matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_MARKER)
                && matches!(&self.peek2().kind, TokKind::Ident(_))
                && matches!(&self.peek3().kind, TokKind::LParen)
        }

        /// D-FACTDECL1=A: `fact Name(params…)` uses the same named-parameter
        /// declaration shape as `marker Name(params…)`.
        pub(super) fn at_fact_decl(&self) -> bool {
            matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_FACT)
                && matches!(&self.peek2().kind, TokKind::Ident(_))
                && matches!(&self.peek3().kind, TokKind::LParen)
        }

        /// D-META-FORM1=A / D-META-USER1=A: parse `marker Name(params…)`
        /// with an optional checked body. The rule's own arguments and facts
        /// still share one named-parameter list, told apart by `$` marks.
        pub(super) fn marker_decl(&mut self) -> Result<crate::AST::MarkerDecl, Diagnostic> {
            let start = self.peek().span;
            self.bump(); // consume `marker`
            let (name, name_span) =
                self.expect_ident("the marker name in `marker Name(params…)`")?;
            self.expect(TokKind::LParen, "to open the marker's parameter list")?;
            let params = self.marker_decl_param_list()?;
            let mut end = self.toks[self.pos - 1].span.end;
            let body = if matches!(self.peek().kind, TokKind::LBrace) {
                self.bump();
                let body = self.block_stmts();
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
        /// ordinary `$`-marked named parameters in the one shared list.
        pub(super) fn fact_decl(&mut self) -> Result<crate::AST::FactDecl, Diagnostic> {
            let start = self.peek().span;
            self.bump(); // consume `fact`
            let (name, name_span) =
                self.expect_ident("the fact name in `fact Name(params…)`")?;
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

        /// D-META-FORM1=A: the rule's own arguments read `name: Type [=
        /// default]`, the same shape as an ordinary function parameter. A
        /// fact about the rule reads `$name: value` instead — it is a fixed
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
                    let (name, name_span) = match crate::Lexer::keyword_spelling(&self.peek().kind)
                    {
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
                        // (`#Caps(Net, FS)`) writes it the same way a function does.
                        if matches!(self.peek().kind, TokKind::DotDotDot) {
                            self.bump();
                            variadic = true;
                        }
                        let (ty, _ty_span) = self.type_()?;
                        let default = if matches!(self.peek().kind, TokKind::Eq) {
                            self.bump();
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

        /// D-META-FORM1=A rejects a trailing `on` clause and a second
        /// parameter list. A scope body is the checked code form from
        /// D-META-USER1=A and is consumed by `marker_decl` above. Recognize
        /// and consume rejected trailers so the writer sees one teaching error,
        /// so the writer sees one teaching error naming the ratified fix,
        /// not a cascade of unrelated parse errors.
        fn reject_marker_decl_trailer(&mut self) -> Option<Diagnostic> {
            let (code, what, why, fix): (&str, &str, &str, String) = match &self.peek().kind {
                TokKind::Ident(n) if n == "on" => (
                    "E0381",
                    "a trailing `on` clause isn't how a marker states a fact",
                    "D-META-FORM1=A: a fact about the rule (its legal sites, whether it repeats) is an ordinary named parameter in the same list, marked with the compile-time `$` sigil — not a clause after the list",
                    "move the sites into the parameter list as `$sites: [.Function, …]`".to_string(),
                ),
                TokKind::LParen => (
                    "E0381",
                    "a second parameter list isn't how a marker states a fact",
                    "D-META-FORM1=A: the rule's own arguments and facts about the rule share one named-parameter list, told apart by the compile-time `$` sigil — not two parameter lists",
                    "fold the second list's facts into the first as `$sites: […]`, `$repeatable: true`".to_string(),
                ),
                _ => return None,
            };
            let span = self.peek().span;
            self.skip_marker_trailer_balanced();
            Some(Diagnostic::error(code, what.to_string(), why.to_string(), fix, Some(span)))
        }

        /// Consume the rejected trailer `reject_marker_decl_trailer` just
        /// identified — the `on` word plus its bracketed list, the second
        /// parameter list, or the scope block — so parsing resumes cleanly
        /// after the one teaching diagnostic.
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
    use crate::{AST, Lexer, Parser};

    /// D-META-NAME1=A / D-META-FORM1=A: the ratified shape from the ballot's
    /// own worked example — named parameters, a fact marked with `$`.
    #[test]
    fn ratified_named_parameter_form_parses_with_a_dollar_marked_fact() {
        let source = "marker Inline(mode: InlineMode, $sites: [.Function, .Method, .Constant])\nfn run() {}\n";
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
        assert_eq!(decl.params[1].name, "$sites");
    }

    /// D-FACTDECL1=A: fact declarations reuse the marker parameter AST shape.
    #[test]
    fn fact_declaration_parses_its_law_columns() {
        let source = concat!(
            "fact Exactness($holds: .Value, $safe: .Gain, ",
            "$gates: [approx, raw], $decision: \"D-TEST\")\n",
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
            ["$holds", "$safe", "$gates", "$decision"]
        );
        assert!(decl
            .params
            .iter()
            .all(|param| param.ty.is_none() && param.value.is_some()));
    }

    /// D-META-FORM1=A: `$repeatable` is a named parameter like every other
    /// fact about a rule, never a trailing word. A new fact about rules is a
    /// new named parameter, so the list stays open-ended and the grammar does
    /// not grow. `$sites` takes `[Site]`, the eighteen-member menu published in
    /// `core.lang` (`Policy::SITE_VARIANTS`).
    #[test]
    fn a_fact_about_the_rule_is_one_more_named_parameter() {
        let source = "marker Pre(condition: String, message: String, $sites: [.Function, .Method], $repeatable: true)\nfn run() {}\n";
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
        let names: Vec<&str> = decl.params.iter().map(|param| param.name.as_str()).collect();
        assert_eq!(names, ["condition", "message", "$sites", "$repeatable"]);

        // A fact about the rule carries a value, not a type; an argument the
        // use site supplies carries a type.
        for param in &decl.params {
            if param.name.starts_with('$') {
                assert!(param.ty.is_none(), "{}", param.name);
                assert!(param.value.is_some(), "{}", param.name);
            } else {
                assert!(param.ty.is_some(), "{}", param.name);
            }
        }

        // The site names `$sites` may hold are exactly the published menu.
        for site in jet_foundation::Policy::RuleSite::ALL {
            assert!(jet_foundation::Policy::SITE_VARIANTS.contains(&site.name()));
        }
    }

    /// D-META-FORM1=A rejects a trailing `on` clause and a second parameter
    /// list in favor of `$`-marked named parameters in the declaration's own
    /// list. D-META-USER1=A makes the scope body the checked code form.
    #[test]
    fn rejected_spellings_each_teach_the_ratified_named_parameter_form() {
        let source = r#"
marker Inline(mode: String) on [.Function]

marker Pre(condition: String)(sites: [.Function])

fn run() {}
"#;
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
                diagnostic.fix.contains('$'),
                "fix must name the ratified $-marked named-parameter form: {diagnostic:?}"
            );
        }
    }
}
