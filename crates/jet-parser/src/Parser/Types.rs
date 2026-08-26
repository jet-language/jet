use super::*;

impl<'a> Parser<'a> {
    fn enter_generic_type_layer(&mut self, label: &str, span: Span) -> bool {
        self.type_generic_depth += 1;
        self.type_generic_chain.push(label.to_string());
        if self.type_generic_depth > Generics::MAX_GENERIC_DEPTH {
            let chain = self.type_generic_chain.join(" → ");
            self.type_generic_depth = self.type_generic_depth.saturating_sub(1);
            self.type_generic_chain.pop();
            self.diags.push(Generics::e0909(&chain, span));
            self.type_generic_truncated = true;
            return false;
        }
        true
    }

    fn leave_generic_type_layer(&mut self) {
        self.type_generic_depth = self.type_generic_depth.saturating_sub(1);
        self.type_generic_chain.pop();
    }

    fn type_generic_arg(&mut self, label: &str) -> Result<(Type, Span), Diagnostic> {
        let span = self.peek().span;
        // D-TYPE2-MEASURE1=A: `Vec<N>` and `Matrix<M, N>` use the shared
        // declared-measure parser, not a private shape representation.
        if matches!(label.rsplit('.').next(), Some("Vec" | "Matrix")) {
            return Ok((Type::Measure(self.parse_declared_measure("shape")?), span));
        }
        if !self.enter_generic_type_layer(label, span) {
            self.sync_type_arg();
            return Ok((Type::Int, span));
        }
        // A collection-type probe may fail while parsing an expression (for
        // example, `[.Function, .Method]` is a value list, not `[T]`). Keep
        // the generic-depth accounting balanced on that error path too.
        let parsed = self.type_();
        self.leave_generic_type_layer();
        Ok(parsed?)
    }

    /// Parse the closed measure language. Literals, module value parameters,
    /// and the declared additive rule are typeable; calls and other user code
    /// are not.
    fn parse_declared_measure(&mut self, kind: &str) -> Result<crate::AST::Measure, Diagnostic> {
        let measure = match self.peek().kind.clone() {
            TokKind::Int(value, _) if value >= 0 => {
                self.bump();
                crate::AST::Measure::literal(kind, value as u64)
            }
            TokKind::Minus
                if kind == "length" && matches!(self.peek2().kind, TokKind::Int(_, _)) =>
            {
                let start = self.bump().span.start;
                let end = match self.peek().kind {
                    TokKind::Int(_, _) => self.bump().span.end,
                    _ => start,
                };
                return Err(Diagnostic::error(
                    "E0963",
                    "a fixed-size list length is outside the supported range".to_string(),
                    "the list length must fit the target's array-size representation".to_string(),
                    "use a non-negative comptime integer within the supported range".to_string(),
                    Some(Span::new(start, end)),
                ));
            }
            TokKind::Float(..) if kind == "length" => {
                let span = self.bump().span;
                return Err(Diagnostic::error(
                    "E0963",
                    "a fixed-size list length must be an integer, got Float (an approximate binary number)"
                        .to_string(),
                    "a fixed-size list needs one known number of elements".to_string(),
                    "use an integer literal or a compile-time expression that produces Int"
                        .to_string(),
                    Some(span),
                ));
            }
            TokKind::Ident(name) => {
                self.bump();
                crate::AST::Measure::symbol(kind, name)
            }
            TokKind::LParen => {
                self.bump();
                let left = self.parse_declared_measure(kind)?;
                // D-META-CONST1: `(@lanes * 2)` is shipped in
                // examples/features/comptime/computed_constants.jet, so the
                // declared rules are addition and scaling, not addition alone.
                let rule = match self.peek().kind {
                    TokKind::Star => {
                        self.bump();
                        crate::AST::MeasureRule::Mul
                    }
                    _ => {
                        self.expect(TokKind::Plus, "in the declared measure rule")?;
                        crate::AST::MeasureRule::Add
                    }
                };
                let right = self.parse_declared_measure(kind)?;
                self.expect(TokKind::RParen, "after the declared measure rule")?;
                left.combine(&right, rule)
                    .expect("measure operands use the same kind")
            }
            _ => {
                let span = self.peek().span;
                return Err(Diagnostic::error(
                    "E0963",
                    "a type measure must be a declared integer".to_string(),
                    "type measures accept literals, module value parameters, and declared combination rules; user code cannot compute a type".to_string(),
                    "write an integer literal, a module value parameter, or a declared measure such as `(N + M)` or `(N * 2)`".to_string(),
                    Some(span),
                ));
            }
        };
        Ok(measure)
    }

    pub(in crate::Parser) fn type_starts_here(&self) -> bool {
        matches!(
            self.peek().kind,
            TokKind::Question
                | TokKind::Bang
                | TokKind::KwFn
                | TokKind::Ident(_)
                | TokKind::LParen
                | TokKind::LBracket
                | TokKind::Star
        )
        // D-EFF2/D-VERDICT-732-1 (formerly D-MARKERMOVE2): `fn(…) :[]>` — a pure-bounded function type
        // (G1: the one carve-out where a contract marker prefixes a TYPE, not a
        // declaration). Retired `fn(…) --[]->` remains recognized here so the
        // callback-bound parser can teach E0062. `fn(…) --[E]->` — the retired general
        // effect-bound list — stays on `#`.
        || (matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_PURE)
            && matches!(self.peek().kind, TokKind::Hash))
        || (matches!(self.peek().kind, TokKind::Hash)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if !n.is_empty() && n.chars().next().map_or(false, |c| c.is_uppercase())))
        || (matches!(self.peek().kind, TokKind::Hash) && matches!(self.peek2().kind, TokKind::LParen))
    }

    pub(super) fn return_type(&mut self) -> Result<(Type, Span), Diagnostic> {
        let start = self.peek().span.start;
        let (ty, _) = self.return_type_inner()?;
        let end = self.toks[self.pos.saturating_sub(1)].span.end;
        Ok((ty, Span::new(start, end)))
    }

    /// D-FAILURE-FOUNDATION1=A: a unit-fallible declaration writes its
    /// contract as a prefix (`fn save() !IOError`).
    pub(in crate::Parser) fn parse_unit_fallible_return(
        &mut self,
    ) -> Result<Option<(Type, Span)>, Diagnostic> {
        if !matches!(self.peek().kind, TokKind::Bang) {
            return Ok(None);
        }
        let sigil = self.bump().span;
        let start = sigil.start;
        let err = if self.type_starts_here()
            && self.peek().span.start == sigil.end
            && !self.next_field_starts_here()
        {
            self.type_()?.0
        } else {
            Type::Named(Syntax::TYPE_ERR.to_string())
        };
        let end = self.toks[self.pos.saturating_sub(1)].span.end;
        Ok(Some((
            Type::Result {
                ok: Box::new(Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())),
                err: Box::new(err),
            },
            Span::new(start, end),
        )))
    }

    pub(in crate::Parser) fn is_unit_fallible_type(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Result { ok, .. }
                if matches!(ok.as_ref(), Type::Named(name) if name == Syntax::INTERNAL_UNIT_TYPE)
        )
    }

    pub(in crate::Parser) fn return_type_has_value(ty: &Type) -> bool {
        !matches!(ty, Type::Named(name) if name == Syntax::INTERNAL_UNIT_TYPE)
            && !Self::is_unit_fallible_type(ty)
    }

    pub(in crate::Parser) fn retired_unit_fallible_signature(span: Span) -> Diagnostic {
        Diagnostic::error(
            "E0003",
            "this unit-fallible signature uses the retired arrow-and-unit form".to_string(),
            "a function that can fail but returns no value has no result payload for an arrow to introduce".to_string(),
            "write `fn save(path: String) !IOError`, or `fn sync() !` for the default error".to_string(),
            Some(span),
        )
    }

    fn return_type_inner(&mut self) -> Result<(Type, Span), Diagnostic> {
        // D-FAILURE-FOUNDATION1=A: return types use the same `?T !E` /
        // `T !(E1 | E2)` rules as every other type position. Parentheses only group.
        self.type_()
    }

    /// Skip tokens until the enclosing `Type<…>` or `[T]` argument ends.
    fn sync_type_arg(&mut self) {
        while self.pos < self.toks.len() {
            match &self.peek().kind {
                TokKind::Eq
                | TokKind::Semi
                | TokKind::Comma
                | TokKind::RParen
                | TokKind::RBrace
                | TokKind::RBracket => {
                    break;
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    pub(super) fn parse_opt_type_params(&mut self) -> Result<Vec<TypeParam>, Diagnostic> {
        if !matches!(self.peek().kind, TokKind::Lt) {
            return Ok(Vec::new());
        }
        self.parse_type_params()
    }

    fn parse_type_params(&mut self) -> Result<Vec<TypeParam>, Diagnostic> {
        self.expect_type_args_open("type")?;
        let mut params = Vec::new();
        loop {
            let (name, name_span) = self.expect_ident("for a type parameter name")?;
            let mut bounds = Vec::new();
            if matches!(self.peek().kind, TokKind::Colon) {
                self.bump();
                bounds = self.parse_trait_bounds()?;
            }
            params.push(TypeParam {
                name,
                name_span,
                bounds,
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_type_args_close("after type parameters")?;
        Ok(params)
    }

    /// S45 multi-trait bounds, D-VARARGBOUND1-amended: a consistent list form
    /// everywhere — `<T: [A, B]>`, never `<T: A + B>`. Single-trait bounds stay
    /// bare (`<T: A>`).
    pub(super) fn parse_trait_bounds(&mut self) -> Result<Vec<String>, Diagnostic> {
        if matches!(self.peek().kind, TokKind::LBracket) {
            let (bounds, _) = self.parse_bracket_trait_bound_list()?;
            Ok(bounds)
        } else {
            let (name, _) = self.expect_ident("for a trait bound")?;
            if name == Syntax::BOUND_QUANTITY && matches!(self.peek().kind, TokKind::Lt) {
                self.expect_type_args_open("quantity bound")?;
                let (dimension, _) = self.expect_ident("for a quantity dimension")?;
                self.expect(TokKind::Comma, "after the quantity dimension")?;
                self.expect(TokKind::Dot, "before the quantity kind")?;
                let (kind, kind_span) = self.expect_ident("for a quantity kind")?;
                if !matches!(kind.as_str(), "Linear" | "Point" | "Delta") {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("`.{kind}` is not a quantity kind"),
                        "Quantity bounds distinguish linear values, affine points, and affine deltas".to_string(),
                        "use `.Linear`, `.Point`, or `.Delta`".to_string(),
                        Some(kind_span),
                    ));
                }
                self.expect_type_args_close("after the quantity bound")?;
                Ok(vec![crate::Generics::quantity_bound(&dimension, &kind)])
            } else {
                Ok(vec![name])
            }
        }
    }

    /// D-VARARGBOUND1: `[TraitA, TraitB, …]` — the bracketed multi-trait-bound
    /// list, the cursor at `[`. Shared by `<T: [A, B]>` (via `parse_trait_bounds`)
    /// and the variadic bound position `...[A, B]` (D-ANY-JAI1, `Items.rs::param`).
    pub(super) fn parse_bracket_trait_bound_list(
        &mut self,
    ) -> Result<(Vec<String>, Span), Diagnostic> {
        let start = self.peek().span;
        self.expect(TokKind::LBracket, "to open a trait-bound list")?;
        let mut bounds = Vec::new();
        loop {
            let (name, _) = self.expect_ident("for a trait bound")?;
            bounds.push(name);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        let end = self.peek().span;
        self.expect(TokKind::RBracket, "to close the trait-bound list")?;
        Ok((bounds, Span::new(start.start, end.end)))
    }

    pub(super) fn parse_type_path(&mut self, where_: &str) -> Result<(String, Span), Diagnostic> {
        let (first, span) = self.expect_ident(where_)?;
        let mut name = first;
        while matches!(self.peek().kind, TokKind::Dot) {
            self.bump();
            let (part, _) = self.expect_ident("after `.` in a type path")?;
            name = format!("{name}.{part}");
        }
        Ok((name, span))
    }

    /// D-PROTO1/D-PROTO2: parse a type name that may contain `.` (e.g. `Payment.Client`).
    pub(super) fn parse_dotted_type_name(
        &mut self,
        where_: &str,
    ) -> Result<(String, Span), Diagnostic> {
        self.parse_type_path(where_)
    }

    /// S73: `(x: Type, y: Type)` in type position after the opening `(`.
    fn parse_tuple_type(&mut self, open: Span) -> Result<Type, Diagnostic> {
        let mut fields = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (name, _) = self.expect_ident("for a tuple member name")?;
                self.expect(TokKind::Colon, "after each tuple member name")?;
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
        }
        let close = self.peek().span;
        self.expect(TokKind::RParen, "to close this tuple type")?;
        if fields.len() < 2 {
            return Err(Diagnostic::error(
                "E0003",
                "a tuple type needs at least two named members".to_string(),
                "a single `(name: Type)` would be ambiguous with a grouped type — use a one-field `struct` instead"
                    .to_string(),
                "add another member: `(x: Int, y: Int)`".to_string(),
                Some(Span::new(open.start, close.end)),
            ));
        }
        Ok(Type::Tuple(
            crate::AST::canonicalize_tuple_fields(fields)
                .into_iter()
                .map(|(n, t)| (n, Box::new(t)))
                .collect(),
        ))
    }

    /// S33: open `Type<…>` — teach square brackets used for value lists.
    pub(super) fn expect_type_args_open(&mut self, type_name: &str) -> Result<(), Diagnostic> {
        match &self.peek().kind {
            TokKind::Lt => {
                self.bump();
                Ok(())
            }
            TokKind::LBracket => Err(Diagnostic::error(
                "E0034",
                format!("`{type_name}[...]` isn't how Jet writes generic types"),
                "square brackets start collection types like `[Int]` or `[String:Int]`, and collection values like `[1, 2]`"
                    .to_string(),
                "write angle brackets for generic arguments, or use `[Int]` for a list type".to_string(),
                Some(self.peek().span),
            )),
            other => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected `<` after `{type_name}`, found {}",
                    describe(other)
                ),
                format!("generic types use angle brackets, like `{type_name}<Int>`"),
                format!("write `{type_name}<` here"),
                Some(self.peek().span),
            )),
        }
    }

    /// S33: close `Type<…>`; splits `>>` when nested generics end with `>`.
    /// D-GENERIC-CALL1=A: true when the cursor (on `<`) begins explicit call type
    /// arguments `<T, …>(`. Both angle-bracket edges must be adjacent to the call
    /// name, so spaced comparisons such as `f < T > (x)` keep their meaning.
    pub(super) fn at_turbofish(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::Lt) {
            return false;
        }
        if self.pos == 0 || self.toks[self.pos - 1].span.end != self.peek().span.start {
            return false;
        }
        let mut depth = 0i32;
        let mut i = self.pos;
        while i < self.toks.len() {
            match self.toks[i].kind {
                TokKind::Lt => depth += 1,
                TokKind::Gt => {
                    depth -= 1;
                    if depth == 0 {
                        return self.toks.get(i + 1).is_some_and(|next| {
                            matches!(next.kind, TokKind::LParen)
                                && self.toks[i].span.end == next.span.start
                        });
                    }
                }
                TokKind::Shr => {
                    depth -= 2;
                    if depth <= 0 {
                        return self.toks.get(i + 1).is_some_and(|next| {
                            matches!(next.kind, TokKind::LParen)
                                && self.toks[i].span.end == next.span.start
                        });
                    }
                }
                // Tokens that never appear inside a type-argument list → not a turbofish.
                TokKind::Semi | TokKind::LBrace | TokKind::RBrace | TokKind::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// D-GENERIC-CALL1=A: parse call-site type arguments `<T, …>` (cursor on `<`).
    /// Callers must first confirm [`Self::at_turbofish`].
    pub(super) fn parse_turbofish(&mut self) -> Result<Vec<crate::AST::Type>, Diagnostic> {
        self.expect_type_args_open("for type arguments")?;
        let mut args = Vec::new();
        loop {
            let (t, _) = self.type_()?;
            args.push(t);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
                continue;
            }
            break;
        }
        self.expect_type_args_close("after call type arguments")?;
        Ok(args)
    }

    fn maybe_close_type_args(&mut self, context: &str) -> Result<(), Diagnostic> {
        if self.type_generic_truncated {
            Ok(())
        } else {
            self.expect_type_args_close(context)
        }
    }

    pub(super) fn expect_type_args_close(&mut self, context: &str) -> Result<(), Diagnostic> {
        if self.pending_type_gt {
            self.pending_type_gt = false;
            return Ok(());
        }
        match &self.peek().kind {
            TokKind::Gt => {
                self.bump();
                Ok(())
            }
            TokKind::Shr => {
                self.bump();
                self.pending_type_gt = true;
                Ok(())
            }
            other => Err(Diagnostic::error(
                "E0003",
                format!("expected `>` {context}, found {}", describe(other)),
                "close a generic type with `>` — nested types may end with `>>`".to_string(),
                "add `>` here".to_string(),
                Some(self.peek().span),
            )),
        }
    }

    pub(super) fn type_(&mut self) -> Result<(Type, Span), Diagnostic> {
        let first = self.peek().span;
        let (ty, _) = self.with_nesting(first, |p| p.type_inner())?;
        let end = self.toks[self.pos.saturating_sub(1)].span.end;
        Ok((ty, Span::new(first.start, end)))
    }

    fn type_inner(&mut self) -> Result<(Type, Span), Diagnostic> {
        self.type_inner_with_failure_contract(true)
    }

    /// Parse one type. `allow_failure_contract_tail` is false only for the success
    /// half of a prefix contract (`?T !E` / `T !E`); otherwise the `!E` would
    /// be consumed as the contract tail before the prefix parser sees it.
    fn type_inner_with_failure_contract(
        &mut self,
        allow_failure_contract_tail: bool,
    ) -> Result<(Type, Span), Diagnostic> {
        let start = self.peek().span;
        let base = match self.peek().kind.clone() {
            // D-FAILURE-FOUNDATION1=A: `!Error` is the expert unit-success
            // failure contract. Bare `!` remains readable during the source
            // cutover and means the default `Err` domain.
            TokKind::Bang => {
                let bang = self.bump().span;
                let err = if self.type_starts_here()
                    && self.peek().span.start == bang.end
                    && !self.next_field_starts_here()
                {
                    self.type_inner_with_failure_contract(false)?.0
                } else {
                    Type::Named(Syntax::TYPE_ERR.to_string())
                };
                Type::Result {
                    ok: Box::new(Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())),
                    err: Box::new(err),
                }
            }
            // D-FAILURE-FOUNDATION1=A: `?Success` is the optional-success
            // prefix. A following adjacent `!Error` turns it into the one
            // structured result carrier; without it this is ordinary Option.
            TokKind::Question => {
                self.bump();
                let success = self.type_inner_with_failure_contract(false)?.0;
                if matches!(self.peek().kind, TokKind::Bang) {
                    let bang = self.bump().span;
                    let err = if self.type_starts_here()
                        && self.peek().span.start == bang.end
                        && !self.next_field_starts_here()
                    {
                        self.type_inner_with_failure_contract(false)?.0
                    } else {
                        Type::Named(Syntax::TYPE_ERR.to_string())
                    };
                    Type::Result {
                        ok: Box::new(Type::Option(Box::new(success))),
                        err: Box::new(err),
                    }
                } else {
                    Type::Option(Box::new(success))
                }
            }
            // D-CAP9: `*T` is the canonical raw-pointer type. Lowers to the same
            // internal `Ptr<T>` (`Type::Apply { name: "Ptr", … }`). `Ptr<T>` is
            // a deprecated alias that teaches `*T` (see the `TYPE_PTR` arm).
            TokKind::Star => {
                self.bump();
                let (elem, _) = self.type_()?;
                Type::Apply {
                    name: Syntax::TYPE_PTR.to_string(),
                    args: vec![elem],
                }
            }
            // D-SOA2C: reserve the per-container prefix spelling `columnar [T]` (a
            // future per-use layout override). Parse-and-reserve only — nothing
            // ships; reject with a clear "reserved" message.
            TokKind::Ident(n)
                if n == Syntax::LAYOUT_COLUMNAR
                    && matches!(self.peek2().kind, TokKind::LBracket) =>
            {
                let kw_span = self.peek().span;
                return Err(Diagnostic::error(
                    "E1107",
                    "the `columnar [T]` per-container layout form is reserved".to_string(),
                    "a per-use columnar override isn't built yet — only the whole-struct form ships".to_string(),
                    "put `#Layout(columnar)` on the `struct` declaration instead".to_string(),
                    Some(kw_span),
                ));
            }
            TokKind::KwFn => self.fn_type(None)?,
            // Retired pre-D-SHAPE8 callback bounds remain recognized only so a
            // teaching diagnostic can be emitted; they are not accepted aliases.
            TokKind::Hash if matches!(self.peek2().kind, TokKind::LParen) => {
                let bound = self.parse_fn_type_effect_bound()?;
                self.diags.push(Self::retired_effect_syntax(start));
                self.fn_type(Some(bound))?
            }
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_PURE) =>
            {
                let bound = self.parse_fn_type_effect_bound()?;
                self.diags.push(Self::retired_effect_syntax(start));
                self.fn_type(Some(bound))?
            }
            // D-QUAL4=A: `#TagName Type` — a value-tag prefix on a type. The marker
            // must be a PascalCase ident (not `Pure`/`(` which are fn-effect bounds).
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if !n.is_empty() && n.chars().next().map_or(false, |c| c.is_uppercase())) =>
            {
                self.bump(); // `@`
                let (marker, _) = self.expect_ident("after `@` in type-tag position")?;
                let (inner, _) = self.type_()?;
                Type::Tagged {
                    marker: TagMarker::User(marker),
                    inner: Box::new(inner),
                }
            }
            TokKind::LBracket => {
                self.bump();
                let (first, first_span) = self.type_generic_arg("list/map type")?;
                if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    let (value, _) = self.type_generic_arg("map value")?;
                    self.expect(TokKind::RBracket, "after the value type in `[K:V]`")?;
                    Type::Map {
                        key: Box::new(first),
                        key_span: Some(first_span),
                        value: Box::new(value),
                    }
                } else if matches!(self.peek().kind, TokKind::Hash) {
                    // D-TYPE2-MEASURE1=A: fixed lengths use the same declared
                    // measure grammar as shapes.
                    self.bump(); // consume `#`
                    let len = self.parse_declared_measure("length")?;
                    self.expect(TokKind::RBracket, "after the size in `[T#N]`")?;
                    Type::FixedList {
                        elem: Box::new(first),
                        len,
                    }
                } else {
                    self.expect(TokKind::RBracket, "after the element type in `[T]`")?;
                    Type::List(Box::new(first))
                }
            }
            TokKind::LParen if self.looks_like_named_tuple(false) => {
                self.bump();
                self.parse_tuple_type(start)?
            }
            // D-VOID1: the empty tuple spelling is the one public
            // no-information result type. It lowers to the existing internal
            // unit value; non-empty tuple syntax is unchanged.
            TokKind::LParen if matches!(self.peek2().kind, TokKind::RParen) => {
                self.bump();
                self.bump();
                Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())
            }
            TokKind::LParen => {
                self.bump();
                let (inner, _) = self.type_()?;
                self.expect(TokKind::RParen, "to close this parenthesized type")?;
                inner
            }
            TokKind::Ident(name) => {
                self.bump();
                match name.as_str() {
                    Syntax::TYPE_INT => Type::Int,
                    Syntax::TYPE_FLOAT => Type::Float,
                    // D-SG9/S42: explicit fixed-width numeric spellings. `I64` is
                    // a fixed-width cell, while bare `Int` is exact and may spill.
                    Syntax::TYPE_I64 => Type::IntN {
                        signed: true,
                        bits: 64,
                    },
                    Syntax::TYPE_F64 => Type::Float,
                    Syntax::TYPE_I8 => Type::IntN {
                        signed: true,
                        bits: 8,
                    },
                    Syntax::TYPE_I16 => Type::IntN {
                        signed: true,
                        bits: 16,
                    },
                    Syntax::TYPE_I32 => Type::IntN {
                        signed: true,
                        bits: 32,
                    },
                    Syntax::TYPE_U8 => Type::IntN {
                        signed: false,
                        bits: 8,
                    },
                    Syntax::TYPE_U16 => Type::IntN {
                        signed: false,
                        bits: 16,
                    },
                    Syntax::TYPE_U32 => Type::IntN {
                        signed: false,
                        bits: 32,
                    },
                    Syntax::TYPE_U64 => Type::IntN {
                        signed: false,
                        bits: 64,
                    },
                    Syntax::TYPE_F32 => Type::Float32,
                    Syntax::TYPE_BOOL => Type::Bool,
                    Syntax::TYPE_STRING => Type::String,
                    Syntax::FOREIGN_TEXT if false => {
                        // D-S14-PAUSE: `Text` teaching is paused.
                        self.diags.push(Diagnostic::error(
                            "E0013",
                            format!(
                                "the text type is called `{}`, not `{}`",
                                Syntax::TYPE_STRING,
                                Syntax::FOREIGN_TEXT
                            ),
                            format!("`{}` is the one and only text type", Syntax::TYPE_STRING),
                            format!(
                                "replace `{}` with `{}`",
                                Syntax::FOREIGN_TEXT,
                                Syntax::TYPE_STRING
                            ),
                            Some(start),
                        ));
                        Type::String
                    }
                    Syntax::TYPE_CHAR => Type::Char,
                    Syntax::RETIRED_TYPE_ERROR => {
                        self.diags.push(Diagnostic::error(
                            "E0432",
                            "`Error` is retired".to_string(),
                            "the default error type and its constructor now use the same name"
                                .to_string(),
                            "replace `Error` with `Err`".to_string(),
                            Some(start),
                        ));
                        Type::Named(Syntax::TYPE_ERR.to_string())
                    }
                    Syntax::RETIRED_TYPE_VOID => {
                        self.diags.push(Diagnostic::error(
                            "E0431",
                            "`Void` is retired".to_string(),
                            "Jet uses `()` for a result with no information; non-returning paths are compiler facts under D-NEVER1".to_string(),
                            "replace `Void` with `()`".to_string(),
                            Some(start),
                        ));
                        Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())
                    }
                    Syntax::FOREIGN_DYN if false => {
                        self.diags.push(Generics::e0036(Syntax::FOREIGN_DYN, start));
                        let (trait_name, _) = self.expect_ident("after `dyn`")?;
                        Type::TraitObject(vec![trait_name])
                    }
                    Syntax::FOREIGN_BOX if false => {
                        self.diags.push(Generics::e0036(Syntax::FOREIGN_BOX, start));
                        if matches!(self.peek().kind, TokKind::Lt) {
                            self.expect_type_args_open("Box")?;
                            if matches!(self.peek().kind, TokKind::Ident(ref n) if n == Syntax::FOREIGN_DYN)
                            {
                                self.bump();
                                let (trait_name, _) =
                                    self.expect_ident("after `dyn` in `Box<dyn …>`")?;
                                self.maybe_close_type_args("after `Box<dyn …>`")?;
                                Type::TraitObject(vec![trait_name])
                            } else {
                                let (inner, _) = self.type_()?;
                                self.maybe_close_type_args("after `Box<…>`")?;
                                inner
                            }
                        } else {
                            Type::Named("Box".to_string())
                        }
                    }
                    Syntax::TYPE_SHARED => {
                        // D-SHARED-CYCLE1=C: `Shared.Weak<T>` is the expert weak
                        // handle; ordinary `Shared<T>` stays the strong form.
                        if matches!(self.peek().kind, TokKind::Dot) {
                            self.bump();
                            let (seg, seg_span) = self.expect_ident("after `Shared.`")?;
                            if seg != "Weak" {
                                self.diags.push(Diagnostic::error(
                                    "E0107",
                                    format!("nothing named `Shared.{seg}` exists here"),
                                    "`Shared` only names the strong handle `Shared<T>` and the expert weak handle `Shared.Weak<T>`"
                                        .to_string(),
                                    "write `Shared.Weak<T>` for a weak shared handle".to_string(),
                                    Some(seg_span),
                                ));
                            }
                            self.expect_type_args_open(Syntax::TYPE_SHARED_WEAK)?;
                            let (inner, _) = self.type_generic_arg(Syntax::TYPE_SHARED_WEAK)?;
                            self.maybe_close_type_args("after a shared weak element type")?;
                            Type::Apply {
                                name: Syntax::TYPE_SHARED_WEAK.to_string(),
                                args: vec![inner],
                            }
                        } else {
                            self.expect_type_args_open("Shared")?;
                            let (inner, _) = self.type_generic_arg("Shared")?;
                            self.maybe_close_type_args("after a shared element type")?;
                            Type::Shared(Box::new(inner))
                        }
                    }
                    // D-CAP9: `Ptr<T>` is a deprecated alias for the canonical
                    // raw-pointer type `*T`. Still parses to the same internal
                    // type, but teaches the new spelling (E0210).
                    Syntax::TYPE_PTR => {
                        self.expect_type_args_open(Syntax::TYPE_PTR)?;
                        let (inner, _) = self.type_generic_arg(Syntax::TYPE_PTR)?;
                        self.maybe_close_type_args(&format!("after `{}<…>`", Syntax::TYPE_PTR))?;
                        self.diags.push(Diagnostic::error(
                            "E0210",
                            format!("`{}<T>` is the old name for the raw-pointer type", Syntax::TYPE_PTR),
                            format!(
                                "the raw-pointer type is now written `*T` — `{}<T>` is a deprecated alias",
                                Syntax::TYPE_PTR
                            ),
                            format!("write `*{}` instead of `{}<{}>`", inner.name(), Syntax::TYPE_PTR, inner.name()),
                            Some(start),
                        ));
                        Type::Apply {
                            name: Syntax::TYPE_PTR.to_string(),
                            args: vec![inner],
                        }
                    }
                    Syntax::TYPE_RESULT => {
                        let diagnostic = Diagnostic::error(
                            "E0406",
                            "`Result<T, E>` is old Jet error syntax".to_string(),
                            "fallible Jet types use a success type and a prefixed error contract".to_string(),
                            "write `?T !E`, `T !(E1 | E2)`, or `!` for the default Err type"
                                .to_string(),
                            Some(start),
                        );
                        self.expect_type_args_open("Result")?;
                        let _ok_ty = self.type_generic_arg("Result ok")?;
                        self.expect(
                            TokKind::Comma,
                            "between the two types in old `Result<T, E>` syntax",
                        )?;
                        let _err_ty = self.type_generic_arg("Result err")?;
                        self.maybe_close_type_args(
                            "after the error type in old `Result<T, E>` syntax",
                        )?;
                        return Err(diagnostic);
                    }
                    other => {
                        let mut name = other.to_string();
                        while matches!(self.peek().kind, TokKind::Dot) {
                            self.bump();
                            let (part, _) = self.expect_ident("after `.` in a type path")?;
                            name = format!("{name}.{part}");
                        }
                        if matches!(self.peek().kind, TokKind::Lt) {
                            self.expect_type_args_open(&name)?;
                            let mut args = Vec::new();
                            loop {
                                args.push(self.type_generic_arg(&name)?.0);
                                if matches!(self.peek().kind, TokKind::Comma) {
                                    self.bump();
                                    continue;
                                }
                                break;
                            }
                            self.maybe_close_type_args(&format!("after `{name}<…>`"))?;
                            Type::Apply { name, args }
                        } else {
                            if self.derive_template_depth == 0
                                && name.split('.').any(Syntax::is_comptime_name)
                            {
                                self.diags.push(Diagnostic::error(
                                    "E0119",
                                    format!("`{name}` is a fact value, not a type"),
                                    "facts classify values at compile time; they do not mint or select types"
                                        .to_string(),
                                    "read the fact in a compile-time value position instead".to_string(),
                                    Some(start),
                                ));
                            }
                            Type::Named(name)
                        }
                    }
                }
            }
            other => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!("expected a type name, found {}", describe(&other)),
                    "types look like `Int`, `String`, or `[Int]`".to_string(),
                    "e.g. `x: Int` or `items: [String]`".to_string(),
                    Some(self.peek().span),
                ));
            }
        };
        // D-TYPE2-SPELL1 / card #1549: an inline range is the same literal-only
        // interval grammar used by a named distinct range, but may appear in
        // every type position. The carrier is intentionally fixed to `Int`.
        // A consumed `>>` leaves one virtual outer `>` pending. The following
        // `(` belongs to the enclosing call, not to an inline range.
        let base = if !self.pending_type_gt
            && matches!(self.peek().kind, TokKind::LParen)
            && matches!(self.peek2().kind, TokKind::Int(_, _) | TokKind::Minus)
        {
            let open = self.bump().span;
            let (lo, lo_span) = self.expect_range_bound_int("as the range's lower bound")?;
            self.expect(TokKind::DotDot, "between the range's bounds")?;
            let (hi, hi_span) = self.expect_range_bound_int("as the range's upper bound")?;
            let close = self.peek().span;
            self.expect(TokKind::RParen, "to close the range constraint")?;
            if lo > hi {
                self.diags.push(Diagnostic::error(
                    "E0137",
                    format!("this range is empty — {lo} is after {hi}"),
                    "a range's low bound must not be greater than its high bound".to_string(),
                    format!("write `{hi}..{lo}` (swap the bounds), or fix the values"),
                    Some(Span::new(lo_span.start, hi_span.end)),
                ));
            }
            if base != Type::Int {
                self.diags.push(Diagnostic::error(
                    "E0137",
                    format!("inline value ranges apply to `Int`, not `{}`", base.name()),
                    "an inline range carries an interval on the default integer carrier".to_string(),
                    "write `Int(lo..hi)` here, or declare a named distinct type for another carrier".to_string(),
                    Some(Span::new(open.start, close.end)),
                ));
                base
            } else {
                Type::InlineRange {
                    base: Box::new(base),
                    lo,
                    hi,
                }
            }
        } else {
            base
        };
        if matches!(self.peek().kind, TokKind::QuestionQuestion) {
            let qspan = self.peek().span;
            return Err(Diagnostic::error(
                "E0309",
                "`??` isn't allowed on a type".to_string(),
                "an optional value is written `?T` once — there's no optional optional".to_string(),
                "use a single `?`, like `?Int`".to_string(),
                Some(qspan),
            ));
        }
        if !allow_failure_contract_tail {
            let member = base;
            if matches!(self.peek().kind, TokKind::Pipe) {
                self.bump();
                let (right, _) = self.type_inner_with_failure_contract(false)?;
                return Ok((crate::AST::canonicalize_union(vec![member, right]), start));
            }
            return Ok((member, start));
        }
        // D-FAILURE-FOUNDATION1=A: an error contract owns the `!` prefix.
        // Prefix contracts are the only accepted failure surface.
        let member = if matches!(self.peek().kind, TokKind::Bang) {
            let bang = self.bump().span;
            let err = if self.type_starts_here()
                && self.peek().span.start == bang.end
                && !self.next_field_starts_here()
            {
                self.type_inner_with_failure_contract(false)?.0
            } else {
                Type::Named(Syntax::TYPE_ERR.to_string())
            };
            Type::Result {
                ok: Box::new(base),
                err: Box::new(err),
            }
        } else {
            base
        };
        // D-UNIONTYPE1=A: a contract-owned error union is parenthesized, so
        // the outer union parser never steals its members.
        if matches!(self.peek().kind, TokKind::Pipe) {
            self.bump();
            let (right, _) = self.type_()?;
            return Ok((crate::AST::canonicalize_union(vec![member, right]), start));
        }
        Ok((member, start))
    }

    fn next_field_starts_here(&self) -> bool {
        (matches!(self.peek().kind, TokKind::Ident(_))
            && matches!(self.peek2().kind, TokKind::Colon))
            || (matches!(self.peek().kind, TokKind::KwFn)
                && matches!(self.peek2().kind, TokKind::Ident(_)))
    }

    /// Parse a function type `fn(T1, …) R -[E]>`, the cursor at `fn`.
    /// `effect_bound` is non-None only while recovering retired prefix syntax.
    /// D-MEMPROVENANCE3=A: optional `name: Type` params and a trailing `from`
    /// after the return type populate `return_view_provenance` (names resolve
    /// here and are not stored on the type).
    fn fn_type(
        &mut self,
        mut effect_bound: Option<Vec<(String, Span)>>,
    ) -> Result<Type, Diagnostic> {
        self.expect(TokKind::KwFn, "to start a function type")?;
        self.expect(TokKind::LParen, "after `fn` in a function type")?;
        let mut params = Vec::new();
        let mut param_names: Vec<Option<String>> = Vec::new();
        // D-APILABEL1=A: a function type reuses the parameter grammar, so it
        // carries the same zone separators. Public labels and zones are part of
        // callable identity; local names and default bodies are not.
        let mut param_zones: Vec<crate::AST::ParamZone> = Vec::new();
        let mut zone = crate::AST::ParamZone::Either;
        let mut saw_slash = false;
        let mut saw_star = false;
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                if matches!(self.peek().kind, TokKind::Slash) {
                    let span = self.bump().span;
                    if saw_slash || saw_star || params.is_empty() {
                        self.diags.push(Self::fn_type_zone_error(span));
                    } else {
                        saw_slash = true;
                        for slot in param_zones.iter_mut() {
                            *slot = crate::AST::ParamZone::PositionalOnly;
                        }
                    }
                } else if matches!(self.peek().kind, TokKind::Star) {
                    let span = self.bump().span;
                    if saw_star {
                        self.diags.push(Self::fn_type_zone_error(span));
                    } else {
                        saw_star = true;
                        zone = crate::AST::ParamZone::LabelOnly;
                    }
                } else {
                    let named = matches!(
                        (&self.peek().kind, &self.peek2().kind),
                        (TokKind::Ident(_), TokKind::Colon)
                    );
                    let name = if named {
                        let TokKind::Ident(name) = self.bump().kind else {
                            unreachable!("peek matched Ident");
                        };
                        self.expect(TokKind::Colon, "after a named function-type parameter")?;
                        Some(name)
                    } else {
                        None
                    };
                    let (pty, _) = self.type_()?;
                    params.push(pty);
                    param_names.push(name);
                    param_zones.push(zone);
                }
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between parameter types in `fn(...)`")?;
            }
        }
        self.expect(TokKind::RParen, "after parameter types in `fn(...)`")?;
        if saw_star && !param_zones.contains(&crate::AST::ParamZone::LabelOnly) {
            self.diags.push(Self::fn_type_zone_error(self.peek().span));
        }
        // D-APILABEL1=A: a label-only parameter needs a label to be called by,
        // so an unnamed one after `*` could never receive an argument.
        if param_names
            .iter()
            .zip(param_zones.iter())
            .any(|(name, zone)| name.is_none() && *zone == crate::AST::ParamZone::LabelOnly)
        {
            self.diags.push(Diagnostic::error(
                "E0763",
                format!(
                    "a parameter after `{}` in this function type has no label",
                    Syntax::PARAM_ZONE_LABEL_ONLY
                ),
                "a label-only parameter is reached by writing its label, so it has to have one"
                    .to_string(),
                "name it, as in `fn(*, force: Bool) Int`".to_string(),
                Some(self.peek().span),
            ));
        }
        // Identity only exists when the type actually declares one; an
        // unannotated `fn(Int) Int` keeps its bare structural meaning.
        let param_contract: Option<Vec<(String, crate::AST::ParamZone)>> =
            (saw_slash || saw_star || param_names.iter().any(Option::is_some)).then(|| {
                param_names
                    .iter()
                    .zip(param_zones.iter())
                    .map(|(name, zone)| (name.clone().unwrap_or_default(), *zone))
                    .collect()
            });
        let canonical_effect = matches!(self.peek().kind, TokKind::Minus)
            && matches!(self.peek2().kind, TokKind::LBracket);
        let retired_colon = matches!(self.peek().kind, TokKind::Colon)
            && matches!(self.peek2().kind, TokKind::LBracket);
        let retired_eq = matches!(self.peek().kind, TokKind::Eq)
            && matches!(self.peek2().kind, TokKind::LBracket);
        let retired_double = matches!(self.peek().kind, TokKind::MinusMinus);
        let prefix_effect_span = (canonical_effect || retired_colon).then(|| self.peek().span);
        if canonical_effect || retired_colon || retired_eq || retired_double {
            if retired_colon || retired_eq || retired_double {
                self.diags
                    .push(Self::retired_effect_syntax(self.peek().span));
            }
            effect_bound = Some(self.parse_effect_arrow_row(
                canonical_effect,
                retired_colon,
                retired_eq,
                retired_double,
            )?);
        }
        let mut arrow_return = false;
        let mut return_type_span = None;
        let ret = if canonical_effect || retired_colon {
            if let Some((ty, span)) = self.parse_unit_fallible_return()? {
                self.diags.push(Self::retired_signature_shape(
                    prefix_effect_span.unwrap_or(span),
                ));
                return_type_span = Some(span);
                Some(Box::new(ty))
            } else if self.type_starts_here() {
                arrow_return = true;
                let (r, span) = self.type_()?;
                return_type_span = Some(span);
                self.diags.push(Self::retired_signature_shape(
                    prefix_effect_span.unwrap_or(span),
                ));
                Some(Box::new(r))
            } else {
                None
            }
        } else if retired_eq || retired_double || self.at_unified_arrow() {
            let arrow = self.at_unified_arrow().then(|| self.bump());
            arrow_return = arrow.is_some() || retired_eq || retired_double;
            if !retired_eq && !retired_double {
                if arrow.is_none() {
                    self.expect_unified_arrow("before a callable result type")?;
                }
            }
            if let Some((r, span)) = self.parse_unit_fallible_return()? {
                return_type_span = Some(span);
                if arrow.is_some() {
                    self.diags.push(Self::retired_signature_shape(
                        arrow.as_ref().map(|token| token.span).unwrap_or(span),
                    ));
                }
                Some(Box::new(r))
            } else if self.type_starts_here() {
                let (r, span) = self.type_()?;
                return_type_span = Some(span);
                if arrow.is_some() {
                    self.diags.push(Self::retired_signature_shape(
                        arrow.as_ref().map(|token| token.span).unwrap_or(span),
                    ));
                }
                Some(Box::new(r))
            } else {
                None
            }
        } else if let Some((ty, span)) = self.parse_unit_fallible_return()? {
            return_type_span = Some(span);
            Some(Box::new(ty))
        } else if self.type_starts_here() {
            let (r, span) = self.type_()?;
            return_type_span = Some(span);
            Some(Box::new(r))
        } else {
            None
        };
        if arrow_return
            && ret
                .as_deref()
                .is_some_and(|ty| Self::is_unit_fallible_type(ty))
        {
            self.diags.push(Self::retired_unit_fallible_signature(
                return_type_span.unwrap_or(self.peek().span),
            ));
        }
        // Synthetic params so `from line` reuses the same resolver as fn results.
        let from_params: Vec<crate::AST::Param> = param_names
            .into_iter()
            .zip(params.iter())
            .enumerate()
            .map(|(index, (name, ty))| crate::AST::Param {
                convention: crate::AST::AccessConvention::Read,
                root: false,
                name: name.unwrap_or_else(|| format!("_{index}")),
                name_span: crate::Diagnostics::Span::new(0, 0),
                ty: ty.clone(),
                ty_span: crate::Diagnostics::Span::new(0, 0),
                default: None,
                variadic: false,
                variadic_bound_list: None,
                declared_view_from_names: None,
                public_label: None,
                zone: crate::AST::ParamZone::Either,
            })
            .collect();
        let return_view_provenance = self.parse_opt_declared_view_from(&from_params);
        if effect_bound.is_none() && self.func_effect_starts_here() {
            let canonical = matches!(self.peek().kind, TokKind::Minus)
                && matches!(self.peek2().kind, TokKind::LBracket);
            let retired_colon = matches!(self.peek().kind, TokKind::Colon)
                && matches!(self.peek2().kind, TokKind::LBracket);
            let retired_eq = matches!(self.peek().kind, TokKind::Eq)
                && matches!(self.peek2().kind, TokKind::LBracket);
            let retired_double = matches!(self.peek().kind, TokKind::MinusMinus);
            if retired_colon || retired_eq || retired_double {
                self.diags
                    .push(Self::retired_effect_syntax(self.peek().span));
            }
            effect_bound = Some(self.parse_effect_arrow_row(
                canonical,
                retired_colon,
                retired_eq,
                retired_double,
            )?);
        }
        let call_metadata = Some(crate::AST::FunctionCallMetadata {
            names: from_params.iter().map(|param| param.name.clone()).collect(),
            defaults: from_params
                .iter()
                .map(|param| param.default.as_deref().cloned())
                .collect(),
            variadic: from_params.iter().map(|param| param.variadic).collect(),
            conventions: from_params.iter().map(|param| param.convention).collect(),
            policies: crate::AST::CallablePolicyChain::default(),
        });
        Ok(Type::Fn {
            params,
            ret,
            effect_bound,
            param_contract,
            call_metadata,
            return_view_provenance,
        })
    }

    fn parse_effect_arrow_row(
        &mut self,
        canonical: bool,
        retired_colon: bool,
        retired_eq: bool,
        retired_double: bool,
    ) -> Result<Vec<(String, Span)>, Diagnostic> {
        self.expect(
            if canonical {
                TokKind::Minus
            } else if retired_colon {
                TokKind::Colon
            } else if retired_eq {
                TokKind::Eq
            } else {
                debug_assert!(retired_double);
                TokKind::MinusMinus
            },
            "to start an effect arrow",
        )?;
        self.expect(TokKind::LBracket, "to start an effect row")?;
        let mut effects = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBracket) {
            if matches!(self.peek().kind, TokKind::DotDot) {
                self.bump();
                let (name, span) = self.expect_ident("for an open effect-row name")?;
                effects.push((format!("..{name}"), span));
                if matches!(self.peek().kind, TokKind::RBracket) {
                    break;
                }
                self.expect(TokKind::Comma, "between effects in the row")?;
                continue;
            }
            let prohibited = matches!(self.peek().kind, TokKind::Bang);
            if prohibited {
                self.bump();
            }
            let (name, span) = self.expect_effect_path_name("for an effect name")?;
            if !prohibited && name.contains('(') {
                return Err(Diagnostic::error(
                    "E0119",
                    format!("`{name}` is only valid as a memory denial"),
                    "the `above: Bytes` argument parameterizes a prohibition, not a positive effect bound".to_string(),
                    format!("write `-[!{name}]>`"),
                    Some(span),
                ));
            }
            effects.push((if prohibited { format!("!{name}") } else { name }, span));
            if matches!(self.peek().kind, TokKind::RBracket) {
                break;
            }
            self.expect(TokKind::Comma, "between effects in the row")?;
        }
        self.expect(TokKind::RBracket, "to close the effect row")?;
        self.expect(
            if canonical {
                TokKind::Gt
            } else if retired_colon {
                TokKind::Gt
            } else if retired_eq {
                TokKind::LambdaArrow
            } else {
                TokKind::UnifiedArrow
            },
            "after the effect row",
        )?;
        Ok(effects)
    }

    /// D-EFF2: parse the effect bound on the front of a function type, the cursor
    /// at `#`. `#Pure` yields the empty set (`Some([])`); `#(E1, E2, …)` yields the
    /// listed names (validated against the effect vocabulary in sema, not here).
    /// The caller has confirmed via lookahead that a `fn` follows.
    /// D-EFF2/D-VERDICT-732-1 (formerly D-MARKERMOVE2, G1): parse a retired
    /// callback effect-bound prefix. Canonical function types carry their
    /// effect row after the result as `fn(…) R -[E]>` or `fn(…) R -[]>`;
    /// `#Pure` and `#(E1, E2, …)` remain recognized only to teach the retired
    /// spelling, including the old `fn(…) --[Net]->` form.
    fn parse_fn_type_effect_bound(&mut self) -> Result<Vec<(String, Span)>, Diagnostic> {
        if matches!(self.peek().kind, TokKind::Hash) {
            self.bump();
        } else {
            self.expect(TokKind::Hash, "to start a callback effect bound")?;
        };
        if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_PURE) {
            self.bump(); // `Pure`
            return Ok(Vec::new());
        }
        // Effect-list form: only ever reached via `#`, since `type_starts_here`
        // and the `fn_type` dispatch only route `@` here when the name is `Pure`.
        self.expect(TokKind::LParen, "after `#` to start a callback effect list")?;
        let mut effects = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (name, span) = self.expect_effect_path_name("for an effect name")?;
                effects.push((name, span));
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between effects in the list")?;
            }
        }
        self.expect(TokKind::RParen, "to close the callback effect list")?;
        Ok(effects)
    }
}

impl<'a> Parser<'a> {
    /// D-APILABEL1=A: a function type's zone separators follow the same rule as
    /// a declaration's — one `/`, one `*`, `/` before `*`, and neither may mark
    /// an empty zone.
    pub(in crate::Parser) fn fn_type_zone_error(span: Span) -> Diagnostic {
        Diagnostic::error(
            "E0763",
            "this function type's parameter zones are out of order".to_string(),
            "the zones read left to right: positional-only, then either, then label-only"
                .to_string(),
            format!(
                "write at most one `{}` before at most one `{}`, each with parameters on its side",
                Syntax::PARAM_ZONE_POSITIONAL_ONLY,
                Syntax::PARAM_ZONE_LABEL_ONLY
            ),
            Some(span),
        )
    }
}
