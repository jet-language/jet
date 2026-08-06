use super::super::*;
use super::bindings::desugar_layout_anchors;

impl<'a> Parser<'a> {
    /// D-LOOPEVAL1: parse a finite `loop … -> …` as an immediately invoked,
    /// compiler-private collecting closure. This reuses the existing callable
    /// expression pipeline; sema replaces the internal `Yield` markers with a
    /// typed eager List accumulator after it infers the item type.
    pub(in crate::Parser) fn yielding_loop_expr(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.bump().span;
        if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            let body = self.block_stmts();
            let end = self.toks[self.pos - 1].span.end;
            let lambda = crate::AST::Lambda {
                take_names: Vec::new(),
                params: Vec::new(),
                body: crate::AST::LambdaBody::Block(vec![Stmt::Loop {
                    body,
                    span: start,
                    label: None,
                }]),
                span: Span::new(start.start, end),
                meta: crate::AST::LambdaMeta {
                    result_loop: true,
                    ..crate::AST::LambdaMeta::default()
                },
            };
            return Ok(Expr::CallValue {
                callee: Box::new(Expr::Lambda(lambda)),
                args: Vec::new(),
                span: Span::new(start.start, end),
            });
        }
        let finite_header = matches!(self.peek().kind, TokKind::LParen)
            || (matches!(self.peek().kind, TokKind::Ident(_))
                && matches!(
                    self.peek2().kind,
                    TokKind::ColonEq | TokKind::Semi | TokKind::Comma
                ));
        if !finite_header {
            return Err(Diagnostic::error(
                "E0072",
                "this loop cannot yield a List because it has no finite exhaustion edge"
                    .to_string(),
                "bare infinite and condition-only loops do not provide the boundary a yielding loop needs"
                    .to_string(),
                "remove `->`, or iterate a finite source; use `break value` for one final ordinary-loop result"
                    .to_string(),
                Some(start),
            ));
        }
        let mut clauses = Vec::new();
        let mut counted = None;

        if matches!(self.peek().kind, TokKind::Ident(_))
            && matches!(self.peek2().kind, TokKind::ColonEq)
        {
            let init = self.sigil_binding()?;
            self.expect_loop_comma("after the state initializer")?;
            let cond = self.expr_no_struct_lit()?;
            let step = if self.take_loop_comma() {
                let step_expr = self.expr()?;
                let step = if matches!(self.peek().kind, TokKind::Eq)
                    || self.peek().kind.compound_op().is_some()
                {
                    let op_tok = self.bump();
                    let op = op_tok.kind.compound_op();
                    let value = self.expr()?;
                    Stmt::Assign {
                        target: self.expr_to_lvalue(step_expr)?,
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
            counted = Some((init, cond, step));
        } else {
            loop {
                let (var, var_span, var2) = self.loop_source_binding()?;
                self.expect_loop_comma("after the loop source binding")?;
                let first = self.expr_no_struct_lit()?;
                let kind = if let Expr::Range {
                    start,
                    end,
                    exclusive,
                    ..
                } = &first
                {
                    let step = if self.at_yielding_loop_stride() {
                        self.take_loop_comma();
                        Some(self.expr_no_struct_lit()?)
                    } else {
                        None
                    };
                    ForKind::Range {
                        start: (**start).clone(),
                        end: (**end).clone(),
                        step,
                        exclusive: *exclusive,
                    }
                } else if matches!(self.peek().kind, TokKind::DotDot | TokKind::DotDotLt) {
                    let exclusive = matches!(self.peek().kind, TokKind::DotDotLt);
                    self.bump();
                    let end = self.expr_no_struct_lit()?;
                    let step = if self.at_yielding_loop_stride() {
                        self.take_loop_comma();
                        Some(self.expr_no_struct_lit()?)
                    } else {
                        None
                    };
                    ForKind::Range {
                        start: first,
                        end,
                        step,
                        exclusive,
                    }
                } else {
                    let step = if self.at_yielding_loop_stride() {
                        self.take_loop_comma();
                        Some(self.expr_no_struct_lit()?)
                    } else {
                        None
                    };
                    ForKind::In {
                        collection: first,
                        step,
                    }
                };
                clauses.push((var, var_span, var2, kind));
                if !self.at_yielding_loop_clause() {
                    break;
                }
                self.bump();
            }
        }

        let guard = if matches!(self.peek().kind, TokKind::KwIf) {
            self.bump();
            Some(self.expr_no_struct_lit()?)
        } else {
            None
        };
        if !matches!(self.peek().kind, TokKind::Arrow) {
            return Err(Diagnostic::error(
                "E0003",
                "this statement loop is used where a value is required".to_string(),
                "a finite loop produces a value only when its body starts with `->`".to_string(),
                "add `-> value`, or move the effect-only loop out of this expression".to_string(),
                Some(start),
            ));
        }
        self.bump();

        let mut body = if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            let previous_tail_depth = self.callable_tail_block_depth;
            self.callable_tail_block_depth = Some(self.block_depth + 1);
            let body = self.block_stmts();
            self.callable_tail_block_depth = previous_tail_depth;
            body
        } else {
            vec![Stmt::Expr(self.expr()?)]
        };
        let Some(last) = body.pop() else {
            return Err(Diagnostic::error(
                "E0073",
                "this yielding loop path produces no item".to_string(),
                "every accepted iteration must contribute one value unless `next` omits it"
                    .to_string(),
                "add a final value, or remove `->`".to_string(),
                Some(start),
            ));
        };
        let value = match last {
            Stmt::Expr(value) => value,
            other => {
                body.push(other);
                return Err(Diagnostic::error(
                    "E0073",
                    "this yielding loop path produces no item".to_string(),
                    "a multiline yielding body uses its final expression as the item".to_string(),
                    "add a final value, or use `next` to omit this iteration".to_string(),
                    Some(start),
                ));
            }
        };
        // A zero-width span distinguishes this compiler-private collection
        // marker from the user-written `yield` statement used by Stream
        // generators. Formatter and sema remove it before user-facing output.
        body.push(Stmt::Yield(
            value,
            Span::new(start.start, start.start),
        ));
        if let Some(cond) = guard {
            body = vec![Stmt::Switch {
                subject: Expr::Bool(true, start),
                arms: vec![SwitchArm {
                    span: cond.span(),
                    cond,
                    body,
                }],
                else_body: None,
                span: start,
            }];
        }
        // A source comprehension may lower to several nested loops, but source
        // `break` exits the comprehension as one control construct. Give the
        // generated root a private label and retarget only exits owned by the
        // user body; exits inside an explicitly nested user loop remain local.
        let collect_label = format!("__jet_collect_loop_{}", start.start);
        rewrite_collect_root_exits(&mut body, &collect_label, 0);

        let loop_stmt = if let Some((init, cond, step)) = counted {
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                span: start,
                label: Some((collect_label.clone(), start)),
            }
        } else {
            let mut nested = body;
            let clause_count = clauses.len();
            for (index, (var, var_span, var2, kind)) in clauses.into_iter().rev().enumerate() {
                nested = vec![Stmt::For {
                    var,
                    var_span,
                    var2,
                    kind,
                    body: nested,
                    span: start,
                    label: (index + 1 == clause_count)
                        .then(|| (collect_label.clone(), start)),
                }];
            }
            nested.pop().expect("a yielding loop has a source clause")
        };
        let end = loop_stmt.span().end.max(start.end);
        let lambda = crate::AST::Lambda {
            take_names: Vec::new(),
            params: Vec::new(),
            body: crate::AST::LambdaBody::Block(vec![loop_stmt]),
            span: Span::new(start.start, end),
            meta: crate::AST::LambdaMeta {
                collecting_loop: true,
                ..crate::AST::LambdaMeta::default()
            },
        };
        Ok(Expr::CallValue {
            callee: Box::new(Expr::Lambda(lambda)),
            args: Vec::new(),
            span: Span::new(start.start, end),
        })
    }

    pub(in super::super) fn at_meta_attr(&self) -> bool {
        matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_META)
            && matches!(self.peek3().kind, TokKind::LParen)
    }

    /// D-DSLBLOCK1=A: recognize one fixed-whitelist stdlib DSL block without
    /// giving third-party code a grammar hook.
    pub(in super::super) fn at_stdlib_dsl_block(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::Hash) {
            return false;
        }
        let TokKind::Ident(name) = &self.peek2().kind else {
            return false;
        };
        if !Syntax::is_stdlib_dsl_block_marker(name) {
            return false;
        }
        matches!(self.peek3().kind, TokKind::LBrace)
            || (name == Syntax::DSL_BLOCK_SQL
                && matches!(self.peek3().kind, TokKind::Lt)
                && matches!(self.peek4().kind, TokKind::Ident(_))
                && matches!(self.peek5().kind, TokKind::Gt)
                && matches!(self.toks.get(self.pos + 5).map(|token| &token.kind), Some(TokKind::LBrace)))
    }

    pub(in super::super) fn at_stdlib_dsl_block_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        self.expect(TokKind::Hash, "before a stdlib DSL block")?;
        let marker = self.bump();
        let (name, name_span) = match marker.kind {
            TokKind::Ident(name) => (name, marker.span),
            _ => unreachable!("DSL marker lookahead already validated"),
        };
        let mut args = Vec::new();
        let mut args_span = None;
        if matches!(self.peek().kind, TokKind::Lt) {
            let type_start = self.bump().span;
            let type_token = self.bump();
            let (type_name, type_span) = match type_token.kind {
                TokKind::Ident(name) => (name, type_token.span),
                _ => {
                    return Err(Diagnostic::error(
                        "E0617",
                        format!("`#{name}` needs one type name between `<` and `>`"),
                        "the SQL DSL's optional row type is a single compile-time type name".to_string(),
                        "write `#SQL<Row> { … }`".to_string(),
                        Some(type_token.span),
                    ))
                }
            };
            let end = self.peek().span;
            self.expect(TokKind::Gt, "after the DSL type name")?;
            args_span = Some(Span::new(type_start.start, end.end));
            args.push(Expr::Ident(type_name, type_span));
        }
        self.expect(TokKind::LBrace, "after a stdlib DSL marker")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ScopeMember {
            name,
            name_span,
            args,
            args_span,
            body,
            dot_span: start,
            span: Span::new(start.start, end),
        })
    }

    pub(in super::super) fn meta_attr_next_kind(&self) -> Option<&TokKind> {
        self.meta_attr_next_index()
            .and_then(|i| self.toks.get(i).map(|t| &t.kind))
    }

    /// Index of the token immediately after a leading `#Meta(...)` (skipping ASI semis).
    pub(in super::super) fn meta_attr_next_index(&self) -> Option<usize> {
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
        if i < self.toks.len() {
            Some(i)
        } else {
            None
        }
    }

    /// After `#Meta(...)`, is the next item `#Persist …`?
    pub(in super::super) fn at_persist_after_meta(&self) -> bool {
        let Some(i) = self.meta_attr_next_index() else {
            return false;
        };
        matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Hash))
            && matches!(
                self.toks.get(i + 1).map(|t| &t.kind),
                Some(TokKind::Ident(n)) if n == Syntax::MARKER_PERSIST
            )
    }

    /// After `#Meta(...)`, is the next item an explicit compile-time marker?
    pub(in super::super) fn at_comptime_marker_after_meta(&self) -> bool {
        let Some(i) = self.meta_attr_next_index() else {
            return false;
        };
        matches!(self.toks.get(i).map(|t| &t.kind), Some(TokKind::Hash))
            && matches!(
                self.toks.get(i + 1).map(|t| &t.kind),
                Some(TokKind::Ident(n))
                    if matches!(
                        n.as_str(),
                        "Known" | "Static" | "Inline" | "static" | "inline"
                    )
            )
    }

    /// D-LAYOUT-CTOR1: `name (::|:=) Layout .{` — typed-literal constraint body.
    /// `:=` is recognized so we can teach immutable `::` (not silent TypedLit fallthrough).
    fn looks_like_layout_ctor(&self) -> bool {
        matches!(self.peek().kind, TokKind::Ident(_))
            && matches!(
                self.peek2().kind,
                TokKind::ColonColon | TokKind::ColonEq
            )
            && matches!(&self.peek3().kind, TokKind::Ident(n) if n == Syntax::LAYOUT_TYPE)
            && matches!(self.peek4().kind, TokKind::Dot)
            && matches!(self.peek5().kind, TokKind::LBrace)
    }

    /// D-LAYOUT-CTOR1: parse `name :: Layout.{ … }` into `Stmt::Layout`.
    /// Body is a D-DOTCTOR3 element list of `Constraint` exprs (comma/semi),
    /// not a statement block — same separator convention as `[T].{ … }`.
    fn layout_ctor_binding(&mut self) -> Result<Stmt, Diagnostic> {
        let (name, name_span) = self.expect_ident("for the layout binding name")?;
        let mutable = self.expect_bind_sigil()?;
        if mutable {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "a `{}.{{ … }}` binding is immutable",
                    Syntax::LAYOUT_TYPE
                ),
                format!(
                    "`{}` builds a solved layout handle once; mutate suggestions through `.suggest`, not the binding",
                    Syntax::LAYOUT_TYPE
                ),
                format!(
                    "write `{name} {} {}.{{ … }}`",
                    Syntax::SIGIL_BIND_IMMUT,
                    Syntax::LAYOUT_TYPE
                ),
                Some(name_span),
            ));
        }
        let type_tok = self.bump(); // `Layout`
        self.expect(TokKind::Dot, "before the layout constraint body")?;
        self.in_layout_body += 1;
        let lit_body =
            self.typed_lit_body_for_head(&Type::Named(Syntax::LAYOUT_TYPE.to_string()))?;
        self.in_layout_body -= 1;
        let end = self.toks[self.pos - 1].span.end;
        let elems = match lit_body {
            TypedLitBody::Elements(elems) => elems,
            TypedLitBody::Empty => Vec::new(),
            TypedLitBody::Fields(_)
            | TypedLitBody::Entries(_)
            | TypedLitBody::Value(_) => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "a `{}.{{ … }}` body is a comma-separated list of constraints",
                        Syntax::LAYOUT_TYPE
                    ),
                    format!(
                        "write comparisons separated by commas, e.g. `label.width >= 80.0, input.left == label.right`"
                    ),
                    format!(
                        "write `{name} {} {}.{{ constraint, … }}`",
                        Syntax::SIGIL_BIND_IMMUT,
                        Syntax::LAYOUT_TYPE
                    ),
                    Some(name_span),
                ));
            }
        };
        let mut body: Vec<Stmt> = elems.into_iter().map(Stmt::Expr).collect();
        for stmt in &mut body {
            desugar_layout_anchors(&name, stmt);
        }
        Ok(Stmt::Layout {
            name,
            name_span,
            body,
            span: Span::new(name_span.start, end.max(type_tok.span.end)),
        })
    }

    /// D-LAYOUT-CTOR1: retired `layout NAME { … }` — teaching error E2935.
    fn retired_layout_keyword(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.bump().span; // `layout`
        let mut name_hint = String::new();
        let mut end = start.end;
        if let TokKind::Ident(n) = &self.peek().kind {
            name_hint = n.clone();
            end = self.peek().span.end;
            self.bump();
        }
        if matches!(self.peek().kind, TokKind::LBrace) {
            end = self.peek().span.end;
        }
        let fix = if name_hint.is_empty() {
            format!(
                "write `name {} {}.{{ … }}`",
                Syntax::SIGIL_BIND_IMMUT,
                Syntax::LAYOUT_TYPE
            )
        } else {
            format!(
                "write `{name_hint} {} {}.{{ … }}`",
                Syntax::SIGIL_BIND_IMMUT,
                Syntax::LAYOUT_TYPE
            )
        };
        Err(Diagnostic::error(
            "E2935",
            format!("`{}` is retired", Syntax::FOREIGN_LAYOUT_KW),
            format!(
                "constraint layouts use typed-literal construction — `name {} {}.{{ … }}` (D-DOTCTOR3 element body of `Constraint`s)",
                Syntax::SIGIL_BIND_IMMUT,
                Syntax::LAYOUT_TYPE
            ),
            fix,
            Some(Span::new(start.start, end)),
        ))
    }

    /// D-UNINIT-SENTINEL1/2: `#Uninit name: Type` is retired — teaching error
    /// E0426 points at `name := Type.{ uninit }`.
    fn retired_uninit_marker(&mut self) -> Result<Stmt, Diagnostic> {
        let hash_span = self.peek().span;
        self.bump(); // `#`
        let marker = self.bump(); // `Uninit`
        let mut end = marker.span.end;
        let mut name_hint = String::new();
        if let TokKind::Ident(n) = &self.peek().kind {
            name_hint = n.clone();
            end = self.peek().span.end;
        }
        let fix = if name_hint.is_empty() {
            format!(
                "write `name {} Type.{{ {} }}`",
                Syntax::SIGIL_BIND_MUT,
                Syntax::KW_UNINIT
            )
        } else {
            format!(
                "write `{name_hint} {} <Type>.{{ {} }}`",
                Syntax::SIGIL_BIND_MUT,
                Syntax::KW_UNINIT
            )
        };
        Err(Diagnostic::error(
            "E0426",
            format!("`#{}` is retired", Syntax::MARKER_UNINIT),
            format!(
                "uninitialized storage is a fact about the value — it now reads `name {} Type.{{ {} }}`",
                Syntax::SIGIL_BIND_MUT,
                Syntax::KW_UNINIT
            ),
            fix,
            Some(Span::new(hash_span.start, end)),
        ))
    }

    pub(in super::super) fn parse_meta_attr(&mut self) -> Result<MetaAttr, Diagnostic> {
        let marker = self.parse_rule_marker()?;
        self.meta_attr_from_marker(marker)
    }

    pub(in crate::Parser) fn meta_attr_from_marker(
        &mut self,
        marker: Marker,
    ) -> Result<MetaAttr, Diagnostic> {
        let arguments = self.bound_registered_rule_arguments(&marker)?;
        let parameter_indices = (0..marker.args.len())
            .map(|index| arguments.parameter_for_source(index))
            .collect::<Vec<_>>();
        let mut fields = Vec::new();
        for (index, (value, label)) in marker
            .args
            .into_iter()
            .zip(marker.arg_labels)
            .enumerate()
        {
            if label.is_none()
                && matches!(&value, Expr::Ident(name, _) if name == Syntax::META_FIELD_TUNABLE)
            {
                fields.push(MetaField::Tunable { span: value.span() });
                continue;
            }
            let label = label.or_else(|| {
                let parameter_index = parameter_indices[index]?;
                let parameter = crate::Policy::applied_rule(Syntax::MARKER_META)?
                    .signature
                    .params
                    .get(parameter_index)?;
                Some((parameter.name.to_string(), value.span()))
            });
            match label {
                Some((name, field_span)) => {
                    let span = Span::new(field_span.start, value.span().end);
                    if name == Syntax::META_FIELD_CATEGORY {
                        fields.push(MetaField::Category { value, span });
                    } else if name == Syntax::META_FIELD_MATURITY {
                        let valid = matches!(&value,
                            Expr::EnumLit { type_name, variant, args, .. }
                                if type_name.is_empty()
                                    && args.is_empty()
                                    && matches!(variant.as_str(),
                                        Syntax::MARKER_EXPERIMENTAL
                                            | Syntax::MARKER_TESTED
                                            | Syntax::MARKER_HARDENED));
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
                    } else if name == Syntax::META_FIELD_TUNABLE {
                        match value {
                            Expr::Bool(true, _) => fields.push(MetaField::Tunable { span }),
                            Expr::Bool(false, _) => {}
                            value => fields.push(MetaField::Unknown {
                                name,
                                value: Some(value),
                                span: field_span,
                            }),
                        }
                    } else {
                        fields.push(MetaField::Unknown {
                            name,
                            value: Some(value),
                            span: field_span,
                        });
                    }
                }
                None => {
                    let span = value.span();
                    fields.push(MetaField::Unknown {
                        name: match value {
                            Expr::Ident(name, _) => name,
                            _ => "<positional>".to_string(),
                        },
                        value: None,
                        span,
                    });
                }
            }
        }
        Ok(MetaAttr {
            fields,
            span: marker.span,
        })
    }

    pub(in super::super) fn meta_attr_wrong_place_diag(&self, span: Span, target: &str) -> Diagnostic {
        Diagnostic::error(
            "E0349",
            "`#Meta` attaches to a binding or function".to_string(),
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
                TokKind::Ident(n) if n == Syntax::MARKER_OFF || n == Syntax::MARKER_DEBUG_ONLY
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
                    Syntax::MARKER_OFF,
                    Syntax::MARKER_DEBUG_ONLY
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
        if marker == Syntax::MARKER_OFF {
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
    /// D-UNSAFE-REASON1=A: a missing reason is E3112.
    pub(super) fn at_unsafe_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        // E0055: retirement and replacement come from the shared registry;
        // the ordinary marker-call reader consumes the old arguments.
        let retired_audit = match &self.peek2().kind {
            TokKind::Ident(name) if name == Syntax::MARKER_AUDIT => {
                crate::Policy::applied_rule(name).and_then(|row| match row.status {
                    crate::Policy::RuleStatus::Retired { replacement } => Some(replacement),
                    crate::Policy::RuleStatus::Active => None,
                })
            }
            _ => None,
        };
        if matches!(self.peek().kind, TokKind::Hash) && retired_audit.is_some() {
            let replacement = retired_audit.unwrap();
            let audit = self.parse_rule_marker()?;
            // Skip synthetic line terminator between `#Audit(…)` and `#Unsafe`.
            if matches!(self.peek().kind, TokKind::Semi) {
                self.bump();
            }
            self.diags.push(Diagnostic::error(
                "E0055",
                format!("`#{}` is retired", audit.name),
                "D-UNSAFE2 merged the audit reason into the gate itself".to_string(),
                format!("write `{replacement} {{ … }}` and drop the separate audit line"),
                Some(audit.span),
            ));
        }
        // Required `#Unsafe`.
        if !(matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_UNSAFE))
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
        let marker = self.parse_rule_marker()?;
        if marker.args.is_empty() {
            return Err(Diagnostic::error(
                "E3112",
                "an `#Unsafe` block needs a reason".to_string(),
                "every unsafe gate records why its unchecked operations preserve memory safety"
                    .to_string(),
                "write `#Unsafe(\"why this is safe\") { … }`".to_string(),
                Some(marker.span),
            ));
        }
        let arguments = self.bound_registered_rule_arguments(&marker)?;
        // D-UNSAFE-OBLIG1: ordinary call arguments, then semantic validation.
        let audit_expr = arguments.parameter(0).cloned();
        let audit = audit_expr.as_ref().and_then(|argument| match argument {
            Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
                StrPart::Lit(reason) => Some(reason.clone()),
                StrPart::Interp(..) => None,
            },
            _ => None,
        });
        let mut obligation_mode = None;
        if marker.args.len() > 2 {
            return Err(crate::Policy::marker_argument_shape_error(
                Syntax::KW_UNSAFE,
                marker.span,
            ));
        }
        if let Some(value) = arguments.parameter(1) {
            let (mode, mode_span) = match value {
                Expr::EnumLit { type_name, variant, span, args, .. }
                    if type_name.is_empty() && args.is_empty() => (variant, span),
                _ => return Err(crate::Policy::marker_argument_shape_error(Syntax::KW_UNSAFE, value.span())),
            };
            obligation_mode = Some(match mode.as_str() {
                    "Track" => crate::Policy::PolicyValue::UnsafeTrack,
                    "Skip" => crate::Policy::PolicyValue::UnsafeSkip,
                    _ => return Err(Diagnostic::error("E3108", format!("`.{mode}` is not a per-site obligation mode"), "a gate either tracks typed obligations or explicitly skips them when policy permits".to_string(), "write `.Track` or `.Skip`".to_string(), Some(*mode_span))),
                });
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
            audit_expr,
            body,
            span,
        })
    }

    /// D-CTEFFECT1 (ratified 2026-06-25): parse `#Impure("reason") { … }` in
    /// statement position. Mirrors `at_unsafe_stmt`. Missing reason → L3102 in sema.
    pub(super) fn at_impure_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        let marker = self.parse_rule_marker()?;
        let arguments = self.bound_registered_rule_arguments(&marker)?;
        let reason_expr = arguments.parameter(0).cloned();
        let reason = reason_expr.as_ref().and_then(|argument| match argument {
            Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
                StrPart::Lit(reason) => Some(reason.clone()),
                StrPart::Interp(..) => None,
            },
            _ => None,
        });
        self.expect(TokKind::LBrace, "after `#Impure(…)`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Impure {
            reason,
            reason_expr,
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
        let marker = self.parse_rule_marker()?;
        let arguments = self.bound_registered_rule_arguments(&marker)?;
        let Some(Expr::Ident(name, name_span)) = arguments.parameter(0) else {
            return Err(crate::Policy::marker_argument_shape_error(Syntax::MARKER_REGION, marker.span));
        };
        self.expect(TokKind::LBrace, "after `#Region(name)`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Region { name: name.clone(), name_span: *name_span, body, span: Span::new(marker.span.start, end) })
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
        let marker = self.parse_rule_marker()?;
        let arguments = self.bound_registered_rule_arguments(&marker)?;
        let reason_expr = arguments.parameter(0).cloned().expect("bound reason argument");
        let reason = match &reason_expr {
            Expr::Str(parts, _) if parts.len() == 1 => {
                match &parts[0] {
                    StrPart::Lit(reason) => reason.clone(),
                    StrPart::Interp(..) => String::new(),
                }
            }
            _ => String::new(),
        };
        self.expect(TokKind::LBrace, "after `#Nondeterministic(…)`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::AssumeDet {
            reason,
            reason_expr,
            body,
            span: Span::new(marker.span.start, end),
        })
    }

    /// D-CTX1 (ratified 2026-06-22, G2): parse `#Context(field: value, …) { … }`.
    /// Cursor is on the `#` token. Emits E0760 for `=` spelling, E0761 for
    /// unknown fields.
    pub(super) fn at_context_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        if let (TokKind::Ident(field), TokKind::Eq) = (&self.peek4().kind, &self.peek5().kind) {
            return Err(Diagnostic::error(
                "E0760",
                "context fields are set with `:`, not `=`".to_string(),
                "`=` is reassignment (S17); the `name: value` form sets a context field (D-CTX1)"
                    .to_string(),
                format!("write `#{}({field}: …) {{ … }}`", Syntax::CTX_BLOCK),
                Some(self.peek5().span),
            ));
        }
        let marker = self.parse_rule_marker()?;
        let marker_span = marker.span;
        if let Some((field_name, field_name_span)) = marker
            .arg_labels
            .iter()
            .flatten()
            .find(|(field_name, _)| {
                field_name != Syntax::CTX_FIELD_ALLOCATOR
                    && field_name != Syntax::CTX_FIELD_LOGGER
                    && field_name != Syntax::CTX_FIELD_DEADLINE
            })
        {
            return Err(Diagnostic::error(
                "E0761",
                format!("`{field_name}` isn't a context field"),
                "the context bundle holds `allocator`, `logger`, and `deadline`".to_string(),
                format!(
                    "write `#{}(allocator: …)`, `#{}(logger: …)`, or `#{}(deadline: …)`",
                    Syntax::CTX_BLOCK,
                    Syntax::CTX_BLOCK,
                    Syntax::CTX_BLOCK
                ),
                Some(*field_name_span),
            ));
        }
        let arguments = self.bound_registered_rule_arguments(&marker)?;
        let parameter_indices = (0..marker.args.len())
            .map(|index| arguments.parameter_for_source(index))
            .collect::<Vec<_>>();
        let mut fields: Vec<(String, Expr, Span)> = Vec::new();
        for (index, (value, label)) in marker
            .args
            .into_iter()
            .zip(marker.arg_labels)
            .enumerate()
        {
            let (field_name, field_name_span) = match label {
                Some(label) => label,
                None => {
                    let parameter_index =
                        parameter_indices[index].expect("normalized context argument");
                    let parameter = crate::Policy::applied_rule(Syntax::CTX_BLOCK)
                        .and_then(|rule| rule.signature.params.get(parameter_index))
                        .expect("normalized context argument");
                    (parameter.name.to_string(), value.span())
                }
            };
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
                    Some(field_name_span),
                ));
            }
            fields.push((field_name, value.clone(), Span::new(field_name_span.start, value.span().end)));
        }
        self.expect(
            TokKind::LBrace,
            &format!("after `#{}(…)`", Syntax::CTX_BLOCK),
        )?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::ContextBlock {
            fields,
            body,
            span: Span::new(marker_span.start, end),
        })
    }

    /// D-EFF1 / D-QUAL1: parse a `#Caps(Net, DB) { … }` effect-restriction region
    /// in statement position. Cursor is on the `#` token. Effect names are bare
    /// idents; sema validates them against the known effect vocabulary (E0119).
    pub(super) fn at_caps_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let marker = self.parse_rule_marker()?;
        let arguments = self.bound_registered_rule_arguments(&marker)?;
        let mut caps = Vec::with_capacity(marker.args.len());
        for argument in arguments.variadic() {
            let Some(name) = Self::marker_enum_path(argument, "Capability") else {
                return Err(crate::Policy::marker_argument_shape_error(Syntax::KW_CAPS, argument.span()));
            };
            caps.push((name, argument.span()));
        }
        self.expect(TokKind::LBrace, &format!("after `#{}(…)`", Syntax::KW_CAPS))?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Caps {
            caps,
            caps_span: Span::new(marker.name_span.end, marker.span.end),
            body,
            span: Span::new(marker.span.start, end),
        })
    }

    /// D-SCAP1 + D-ARROW-CONTROL1: parse a
    /// `#Grant(caps: FS, Net) { … }` scoped-capability grant region
    /// in statement position. Cursor is on the `#` token. Effect names are bare
    /// idents (sema validates them, E0119); `caps` binds the first-class
    /// capability handle for the block. The dual of `#Caps`: `#Grant` authorizes
    /// the listed effects through the handle, RAII-revoked at scope end.
    pub(super) fn at_grant_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        if matches!(self.peek3().kind, TokKind::LParen)
            && matches!(self.peek4().kind, TokKind::Ident(_))
            && matches!(self.peek5().kind, TokKind::Colon)
        {
            let start = self.bump().span; // `#`
            self.expect_ident(&format!("`#{}`", Syntax::KW_GRANT))?;
            self.expect(TokKind::LParen, "after `#Grant`")?;
            let (binding, binding_span) =
                self.expect_ident("as the scoped capability handle")?;
            self.expect(TokKind::Colon, "after the scoped capability handle")?;
            let mut caps = Vec::new();
            loop {
                let (name, span) = self.expect_effect_path_name("as a granted effect")?;
                caps.push((Self::strip_marker_enum_prefix(name, "Capability"), span));
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between granted effects")?;
            }
            let caps_start = caps.first().map_or(binding_span.end, |(_, span)| span.start);
            self.expect(TokKind::RParen, "to close `#Grant`")?;
            let caps_end = self.toks[self.pos - 1].span.end;
            self.expect(TokKind::LBrace, "after `#Grant(caps: Effects)`")?;
            let body = self.block_stmts();
            let end = self.toks[self.pos - 1].span.end;
            return Ok(Stmt::Grant {
                caps,
                caps_span: Span::new(caps_start, caps_end),
                binding,
                binding_span,
                body,
                span: Span::new(start.start, end),
            });
        }
        let marker = self.parse_rule_marker()?;
        self.diags.push(Diagnostic::error(
            "E0077",
            "this scoped grant uses the retired body binding".to_string(),
            "the capability handle is part of the grant header; `->` is reserved for selected or yielded values"
                .to_string(),
            "write `#Grant(caps: FS, Net) { ... }`".to_string(),
            Some(marker.span),
        ));
        let arguments = self.bound_registered_rule_arguments(&marker)?;
        let mut caps = Vec::with_capacity(marker.args.len());
        for argument in arguments.variadic() {
            let Some(name) = Self::marker_enum_path(argument, "Capability") else {
                return Err(crate::Policy::marker_argument_shape_error(Syntax::KW_GRANT, argument.span()));
            };
            caps.push((name, argument.span()));
        }
        self.expect(
            TokKind::LBrace,
            &format!("after `#{}(…)`", Syntax::KW_GRANT),
        )?;
        // Retired spelling: `{ caps -> … }`.
        let (binding, binding_span) = self.expect_ident("for the capability handle name")?;
        self.expect(
            TokKind::Arrow,
            &format!(
                "after the `#{}` handle name (`#{}(…) {{ caps {} … }}`)",
                Syntax::KW_GRANT,
                Syntax::KW_GRANT,
                "->"
            ),
        )?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Grant {
            caps,
            caps_span: Span::new(marker.name_span.end, marker.span.end),
            binding,
            binding_span,
            body,
            span: Span::new(marker.span.start, end),
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
                                     //   loop x, ... { }        → iteration (was `for`)
                                     //   loop (k, v), ... { }   → key-value iteration
        if matches!(self.peek().kind, TokKind::LBrace) {
            // Infinite loop
            self.bump();
            let body = self.block_stmts();
            Ok(Stmt::Loop { body, span, label })
        } else if matches!(self.peek().kind, TokKind::Ident(_))
            && matches!(self.peek2().kind, TokKind::ColonEq)
        {
            // D-LOOP-HEADER2=A / D-BIND-BARE1: state loop. Only a plain mutable
            // bare binding may initialize state (`name := value`).
            let init = self.sigil_binding()?;
            if !init.mutable || init.pattern.is_some() || init.name.is_empty() {
                return Err(Diagnostic::error(
                    "E0003",
                    "loop state needs one mutable name binding".to_string(),
                    "state changes between loop turns, so its header starts with `name := value`"
                        .to_string(),
                    "write `loop name := value, condition { ... }`".to_string(),
                    Some(init.name_span),
                ));
            }
            self.expect_loop_comma("after the state initializer")?;
            let cond = self.expr_no_struct_lit()?;
            // D-LOOP-HEADER3=D: three-slot C-style counter (`init, cond, step`)
            // retires. Keep two-slot state loops (`name := value, condition`).
            if self.take_loop_comma() {
                let step_span = self.peek().span;
                let _ = self.expr()?;
                if matches!(self.peek().kind, TokKind::Eq)
                    || self.peek().kind.compound_op().is_some()
                {
                    self.bump();
                    let _ = self.expr()?;
                }
                return Err(Diagnostic::error(
                    "E0376",
                    "C-style counter loop headers are retired".to_string(),
                    "a three-slot loop header is binding, source, and step rule — not init, condition, and assignment"
                        .to_string(),
                    "write `loop i, 0..<n { … }` or `loop i, 0..n, 2 { … }`; keep `loop name := value, condition { … }` for mutable state"
                        .to_string(),
                    Some(step_span),
                ));
            }
            let body = self.effect_loop_body()?;
            Ok(Stmt::CountedLoop {
                init,
                cond,
                step: None,
                body,
                span,
                label,
            })
        } else if (matches!(&self.peek().kind, TokKind::Ident(_))
            && matches!(&self.peek2().kind, TokKind::Semi | TokKind::Comma))
            || matches!(&self.peek().kind, TokKind::LParen)
        {
            // D-LOOP-COMMA1=A: `loop x, source [, stride]`; two-name
            // iteration groups the binding as `loop (key, value), source`.
            let (var, var_span, var2) = self.loop_source_binding()?;
            self.expect_loop_comma("after the loop source binding")?;
            let first = self.expr_no_struct_lit()?;
            let kind = if let Expr::Range {
                start,
                end,
                exclusive,
                ..
            } = &first
            {
                let step = if self.take_loop_comma() {
                    Some(self.expr_no_struct_lit()?)
                } else {
                    None
                };
                ForKind::Range {
                    start: (**start).clone(),
                    end: (**end).clone(),
                    step,
                    exclusive: *exclusive,
                }
            } else if matches!(self.peek().kind, TokKind::DotDot | TokKind::DotDotLt) {
                // D-RANGE-EXCL1=C: `..` inclusive (S22); `..<` half-open.
                let exclusive = matches!(self.peek().kind, TokKind::DotDotLt);
                self.bump();
                let end = self.expr_no_struct_lit()?;
                let step = if self.take_loop_comma() {
                    Some(self.expr_no_struct_lit()?)
                } else {
                    None
                };
                ForKind::Range {
                    start: first,
                    end,
                    step,
                    exclusive,
                }
            } else {
                let step = if self.take_loop_comma() {
                    Some(self.expr_no_struct_lit()?)
                } else {
                    None
                };
                ForKind::In { collection: first, step }
            };
            let body = self.effect_loop_body()?;
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
            let body = self.effect_loop_body()?;
            Ok(Stmt::While {
                cond,
                body,
                span,
                label,
            })
        }
    }

    fn effect_loop_body(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
        if matches!(self.peek().kind, TokKind::Arrow) {
            let arrow = self.bump();
            self.diags.push(Diagnostic::error(
                "E0071",
                "this effect-only loop uses a result arrow".to_string(),
                "an arrow says that the loop yields values; this statement discards a result"
                    .to_string(),
                "remove `->` and wrap the body in `{ ... }`".to_string(),
                Some(arrow.span),
            ));
        }
        if matches!(self.peek().kind, TokKind::LBrace) {
            self.bump();
            return Ok(self.block_stmts());
        }
        if matches!(self.peek().kind, TokKind::Semi | TokKind::Eof) {
            return Err(Diagnostic::error(
                "E0003",
                "this loop has no body".to_string(),
                "a loop header must be followed by a braced block".to_string(),
                "add a body written as `{ ... }`".to_string(),
                Some(self.peek().span),
            ));
        }
        if matches!(self.peek().kind, TokKind::KwIf | TokKind::KwLoop) {
            return Err(Diagnostic::error(
                "E0329",
                "a nested one-line control body needs braces".to_string(),
                "an adjacent loop body owns one simple statement".to_string(),
                "wrap the nested control statement in `{ ... }`".to_string(),
                Some(self.peek().span),
            ));
        }
        self.teach_control_braces("loop", self.peek().span);
        Ok(vec![self.stmt()?])
    }

    fn loop_source_binding(&mut self) -> Result<(String, Span, Option<(String, Span)>), Diagnostic> {
        if matches!(self.peek().kind, TokKind::LParen) {
            self.bump();
            let (first, first_span) = self.expect_ident("as the first loop variable")?;
            self.expect(TokKind::Comma, "between the two loop variables")?;
            let second = self.expect_ident("as the second loop variable")?;
            self.expect(TokKind::RParen, "after the two loop variables")?;
            return Ok((first, first_span, Some(second)));
        }

        let (first, first_span) = self.expect_ident("as the loop variable")?;
        // Retired `loop key, value; source`: retain enough structure for fmt
        // to produce `loop (key, value), source`.
        if matches!(self.peek().kind, TokKind::Comma)
            && matches!(self.peek2().kind, TokKind::Ident(_))
            && matches!(self.peek3().kind, TokKind::Semi)
        {
            self.bump();
            let second = self.expect_ident("as the second loop variable")?;
            return Ok((first, first_span, Some(second)));
        }
        Ok((first, first_span, None))
    }

    fn expect_loop_comma(&mut self, context: &str) -> Result<(), Diagnostic> {
        if self.take_loop_comma() {
            Ok(())
        } else {
            self.expect(TokKind::Comma, context)
        }
    }

    fn take_loop_comma(&mut self) -> bool {
        match self.peek().kind {
            TokKind::Comma => {
                self.bump();
                true
            }
            TokKind::Semi if self.peek().span.start < self.peek().span.end => {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0373",
                    "this loop header uses a semicolon".to_string(),
                    "commas separate loop clauses; semicolons separate statements".to_string(),
                    "replace `;` with `,`; `jet fmt` applies this fix".to_string(),
                    Some(span),
                ));
                true
            }
            _ => false,
        }
    }

    fn at_yielding_loop_clause(&self) -> bool {
        matches!(self.peek().kind, TokKind::Comma)
            && (matches!(self.peek2().kind, TokKind::LParen)
                || (matches!(self.peek2().kind, TokKind::Ident(_))
                    && matches!(self.peek3().kind, TokKind::Comma)))
    }

    fn at_yielding_loop_stride(&self) -> bool {
        matches!(self.peek().kind, TokKind::Semi)
            || (matches!(self.peek().kind, TokKind::Comma) && !self.at_yielding_loop_clause())
    }

    /// Parse statements until the closing `}` (consumed). Recovers at
    /// statement boundaries so several problems surface in one run.
    pub(in super::super) fn block_stmts(&mut self) -> Vec<Stmt> {
        self.block_depth += 1;
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
        self.block_depth -= 1;
        body
    }

    pub(super) fn stmt(&mut self) -> Result<Stmt, Diagnostic> {
        match &self.peek().kind {
            TokKind::Hash if self.at_stdlib_dsl_block() => self.at_stdlib_dsl_block_stmt(),
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
                // D-WHEN1 (ratified 2026-06-19): `#Known if <cond> { … }` is
                // a compile-time conditional — not a binding. Detect by peeking
                // at the second token; `comptime NAME` is always a binding.
                if matches!(self.peek2().kind, TokKind::KwIf) {
                    let stmt = self.comptime_if_stmt()?;
                    return Ok(stmt);
                }
                // D-CTMARKER1 (ratified 2026-06-25, piece 2): `#Known { … }` block.
                if matches!(self.peek2().kind, TokKind::LBrace) {
                    let stmt = self.comptime_block_stmt()?;
                    return Ok(stmt);
                }
                let binding = self.comptime_binding()?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Hash if self.at_known_lead() => {
                if matches!(self.peek3().kind, TokKind::KwIf) {
                    return self.comptime_if_stmt();
                }
                if matches!(self.peek3().kind, TokKind::LBrace) {
                    return self.comptime_block_stmt();
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
                    type_args: Vec::new(),
                    args: vec![CallArg {
                        convention: AccessConvention::Read,
                        expr: close,
                        span: close_span,
                        flags: Default::default(),
                        label: None,
                        spread: false,
                    }],
                    resolved_ret: None,
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
                    type_args: Vec::new(),
                    args,
                    resolved_ret: None,
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
                let kind = if let Expr::Range {
                    start,
                    end,
                    exclusive,
                    ..
                } = &first
                {
                    ForKind::Range {
                        start: (**start).clone(),
                        end: (**end).clone(),
                        step: None,
                        exclusive: *exclusive,
                    }
                } else if matches!(self.peek().kind, TokKind::DotDot | TokKind::DotDotLt) {
                    let exclusive = matches!(self.peek().kind, TokKind::DotDotLt);
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
                        exclusive,
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
                // D-ARROW-CONTROL1: the target is an argument of the control
                // operation: `break(name)`.
                if matches!(self.peek().kind, TokKind::LParen) {
                    self.bump();
                    let (name, name_span) =
                        self.expect_ident("as the loop name in `break(name)`")?;
                    if matches!(self.peek().kind, TokKind::Comma) {
                        self.bump();
                        let value = self.expr()?;
                        self.expect(TokKind::RParen, "after the named break value")?;
                        self.finish_stmt()?;
                        return Ok(Stmt::BreakLabelValue(name, name_span, value, span));
                    }
                    self.expect(TokKind::RParen, "after the named break target")?;
                    self.finish_stmt()?;
                    return Ok(Stmt::BreakLabel(name, span));
                }
                // D-LOOPLABEL3=A: retired `break name@` / `break @name`.
                if let TokKind::Ident(_) = &self.peek().kind {
                    if matches!(self.peek2().kind, TokKind::At) {
                        let (name, name_span) = self.expect_ident("for the loop label")?;
                        let end = self.bump().span.end; // `@`
                        self.diags.push(Diagnostic::error(
                            "E0988",
                            "named loop exits use target arguments".to_string(),
                            "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                                .to_string(),
                            format!("write `break({name})`"),
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
                        "named loop exits use target arguments".to_string(),
                        "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                            .to_string(),
                        format!("write `break({name})`"),
                        Some(Span::new(at_span.start, name_span.end)),
                    ));
                    self.finish_stmt()?;
                    return Ok(Stmt::BreakLabel(name, span));
                }
                if matches!(self.peek().kind, TokKind::Semi | TokKind::RBrace | TokKind::Eof) {
                    self.finish_stmt()?;
                    Ok(Stmt::Break(span))
                } else {
                    let value = self.expr()?;
                    self.finish_stmt()?;
                    Ok(Stmt::BreakValue(value, span))
                }
            }
            TokKind::Ident(name)
                if name == Syntax::KW_NEXT
                    && (matches!(self.peek2().kind, TokKind::Semi | TokKind::RBrace)
                        || matches!(self.peek2().kind, TokKind::LParen)
                            && matches!(self.peek3().kind, TokKind::Ident(_))
                            && matches!(self.peek4().kind, TokKind::RParen)
                        || matches!(self.peek2().kind, TokKind::At)
                        || matches!(self.peek2().kind, TokKind::Ident(_))
                            && matches!(self.peek3().kind, TokKind::At)) =>
            {
                let span = self.bump().span;
                if matches!(self.peek().kind, TokKind::LParen) {
                    self.bump();
                    let (name, _) = self.expect_ident("as the loop name in `next(name)`")?;
                    self.expect(TokKind::RParen, "after the named next target")?;
                    self.finish_stmt()?;
                    return Ok(Stmt::ContinueLabel(name, span));
                }
                // D-LOOPLABEL3=A: retired `next name@` / `next @name`.
                if let TokKind::Ident(_) = &self.peek().kind {
                    if matches!(self.peek2().kind, TokKind::At) {
                        let (name, name_span) = self.expect_ident("for the loop label")?;
                        let end = self.bump().span.end; // `@`
                        self.diags.push(Diagnostic::error(
                            "E0988",
                            "named loop exits use target arguments".to_string(),
                            "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                                .to_string(),
                            format!("write `next({name})`"),
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
                        "named loop exits use target arguments".to_string(),
                        "`@` is reserved for locations, addresses, and sources (D-LOOPLABEL3)"
                            .to_string(),
                        format!("write `next({name})`"),
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
            TokKind::KwLoop => self.loop_stmt(None),
            // D-LOOPLABEL3=A: `name :: loop { }` declares a compile-time loop name.
            TokKind::Ident(_)
                if matches!(self.peek2().kind, TokKind::ColonColon)
                    && matches!(self.peek3().kind, TokKind::KwLoop)
                    && !self.named_loop_is_value() =>
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
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_TRACK) =>
            {
                let marker_span = self.peek().span;
                self.bump(); // `#`
                let (_, name_span) = self.expect_ident(&format!("`#{}`", Syntax::MARKER_TRACK))?;
                let mut binding = self.sigil_binding()?;
                binding.track = true;
                binding.track_span = Some(Span::new(marker_span.start, name_span.end));
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Hash
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_LOCAL) =>
            {
                let marker_span = self.peek().span;
                self.bump(); // `#`
                let (_, name_span) = self.expect_ident(&format!("`#{}`", Syntax::MARKER_LOCAL))?;
                let mut binding = self.sigil_binding()?;
                binding.reactive_local = true;
                binding.reactive_local_span =
                    Some(Span::new(marker_span.start, name_span.end));
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Hash
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_SHARED) =>
            {
                let marker_span = self.peek().span;
                self.bump(); // `#`
                let (_, name_span) = self.expect_ident(&format!("`#{}`", Syntax::MARKER_SHARED))?;
                let mut binding = self.sigil_binding()?;
                binding.reactive_shared = true;
                binding.reactive_shared_span =
                    Some(Span::new(marker_span.start, name_span.end));
                binding.reactive_upgrade = true;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            // D-LAYOUT-CTOR1: `name :: Layout.{ … }` before general sigil bindings.
            _ if self.looks_like_layout_ctor() => {
                let stmt = self.layout_ctor_binding()?;
                self.finish_stmt()?;
                Ok(stmt)
            }
            // D-BIND-BARE1: a sigil binding `name (:: | :=) expr` — no leading
            // keyword. Detected before the general Ident statement path.
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
            // D-UNINIT-SENTINEL1/2: `#Uninit name: Type` is retired — teaching
            // error E0426 points at `name := Type.{ uninit }`.
            TokKind::Hash
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_UNINIT) =>
            {
                return self.retired_uninit_marker();
            }
            TokKind::Hash
                if matches!(&self.peek2().kind, TokKind::Ident(n) if matches!(n.as_str(),
                    Syntax::MARKER_AUDIT
                    | Syntax::CTX_BLOCK
                    | Syntax::MARKER_REGION
                    | Syntax::MARKER_POLICY
                    | Syntax::MARKER_LIVE
                    | Syntax::MARKER_NONDETERMINISTIC
                    | Syntax::KW_CAPS
                    | Syntax::KW_GRANT
                    | Syntax::KW_TRANSACT
                    | Syntax::KW_IMPURE
                    | Syntax::KW_SHIELD
                    | Syntax::KW_REACTIVE
                    | Syntax::MARKER_OFF
                    | Syntax::MARKER_DEBUG_ONLY))
                    || matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_UNSAFE) =>
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
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_REGION) {
                    return self.at_region_stmt();
                }
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_POLICY) {
                    return self.at_policy_stmt();
                }
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_LIVE) {
                    return self.at_live_stmt();
                }
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_NONDETERMINISTIC) {
                    return self.at_nondeterministic_stmt();
                }
                // D-EFF1 / D-QUAL1: `#Caps(Net, DB) { … }` effect-restriction region.
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_CAPS) {
                    return self.at_caps_stmt();
                }
                // D-SCAP1: `#grant(FS) { caps -> … }` scoped-capability grant region.
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
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_OFF) {
                    return self.at_statement_switch_stmt(Syntax::MARKER_OFF);
                }
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_DEBUG_ONLY) {
                    return self.at_statement_switch_stmt(Syntax::MARKER_DEBUG_ONLY);
                }
                // D-UNSAFE2: `#Unsafe("reason") { … }` (or retired `#Audit("…") #Unsafe`).
                self.at_unsafe_stmt()
            }
            // D-PERSIST1 (E0145): `#Persist` on a local binding — persistence
            // is keyed by module + name, and a local has no stable identity
            // across a reload. Takes priority over the loop-label-typo arm
            // below for the same reason as the directive-marker guard.
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MARKER_PERSIST) =>
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
            // D-LAYOUT-CTOR1: retired `layout NAME { … }` — teaching error E2935.
            TokKind::Ident(n)
                if n == Syntax::FOREIGN_LAYOUT_KW
                    && matches!(&self.peek2().kind, TokKind::Ident(_))
                    && matches!(self.peek3().kind, TokKind::LBrace) =>
            {
                return self.retired_layout_keyword();
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
                    _ if self.callable_tail_block_depth == Some(self.block_depth)
                        && (matches!(self.peek().kind, TokKind::RBrace)
                            || matches!(self.peek().kind, TokKind::Semi)
                                && matches!(self.peek2().kind, TokKind::RBrace)) => {}
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

    fn named_loop_is_value(&self) -> bool {
        let label = match &self.peek().kind {
            TokKind::Ident(name) => name.as_str(),
            _ => return false,
        };
        let mut braces = 0usize;
        let mut parens = 0usize;
        let mut brackets = 0usize;
        let mut saw_body = false;
        for (index, token) in self.toks.iter().enumerate().skip(self.pos + 3) {
            match token.kind {
                TokKind::LBrace => {
                    braces += 1;
                    saw_body = true;
                }
                TokKind::RBrace if braces == 0 => break,
                TokKind::RBrace => {
                    braces -= 1;
                    if saw_body && braces == 0 {
                        break;
                    }
                }
                TokKind::LParen => parens += 1,
                TokKind::RParen => parens = parens.saturating_sub(1),
                TokKind::LBracket => brackets += 1,
                TokKind::RBracket => brackets = brackets.saturating_sub(1),
                TokKind::Arrow if braces == 0 && parens == 0 && brackets == 0 => return true,
                TokKind::KwBreak if parens == 0 && brackets == 0 => {
                    let next = self.toks.get(index + 1).map(|token| &token.kind);
                    let targets_label = matches!(next, Some(TokKind::LParen))
                        && matches!(
                            self.toks.get(index + 2).map(|token| &token.kind),
                            Some(TokKind::Ident(name)) if name == label
                        )
                        && matches!(
                            self.toks.get(index + 3).map(|token| &token.kind),
                            Some(TokKind::Comma)
                        );
                    let bare_payload = braces == 1
                        && !matches!(
                            next,
                            Some(TokKind::Semi | TokKind::RBrace | TokKind::LParen)
                        );
                    if targets_label || bare_payload {
                        return true;
                    }
                }
                TokKind::Eof if braces == 0 && parens == 0 && brackets == 0 => {
                    break;
                }
                _ => {}
            }
        }
        false
    }

}

fn rewrite_collect_root_exits(stmts: &mut [Stmt], target: &str, nested_loop_depth: usize) {
    for stmt in stmts {
        match stmt {
            Stmt::Break(span) if nested_loop_depth == 0 => {
                *stmt = Stmt::BreakLabel(target.to_string(), *span);
            }
            Stmt::BreakValue(value, span) if nested_loop_depth == 0 => {
                let value = std::mem::replace(value, Expr::Absent(*span));
                *stmt = Stmt::BreakLabelValue(target.to_string(), *span, value, *span);
            }
            Stmt::Switch {
                arms, else_body, ..
            }
            | Stmt::ComptimeSwitch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    rewrite_collect_root_exits(&mut arm.body, target, nested_loop_depth);
                }
                if let Some(body) = else_body {
                    rewrite_collect_root_exits(body, target, nested_loop_depth);
                }
            }
            Stmt::Loop { .. }
            | Stmt::While { .. }
            | Stmt::For { .. }
            | Stmt::CountedLoop { .. } => {}
            Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Off { body, .. }
            | Stmt::DebugOnly { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::ScopeMember { body, .. } => {
                rewrite_collect_root_exits(body, target, nested_loop_depth)
            }
            Stmt::ComptimeIf {
                then_body,
                else_body,
                ..
            } => {
                rewrite_collect_root_exits(then_body, target, nested_loop_depth);
                if let Some(body) = else_body {
                    rewrite_collect_root_exits(body, target, nested_loop_depth);
                }
            }
            _ => {}
        }
    }
}
