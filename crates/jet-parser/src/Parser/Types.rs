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
        if !self.enter_generic_type_layer(label, span) {
            self.sync_type_arg();
            return Ok((Type::Int, span));
        }
        let parsed = self.type_()?;
        self.leave_generic_type_layer();
        Ok(parsed)
    }

    fn type_starts_here(&self) -> bool {
        matches!(
            self.peek().kind,
            TokKind::KwFn | TokKind::Ident(_) | TokKind::LParen | TokKind::LBracket
        )
        // D-EFF2/D-MARKERMOVE2: `@Pure fn(…)` — a pure-bounded function type
        // (G1: the one carve-out where a contract marker prefixes a TYPE, not a
        // declaration). Retired `@Pure fn(…)` still recognized here so the
        // callback-bound parser can teach E0062. `#(E) fn(…)` — the general
        // effect-bound list — stays on `#`.
        || (matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_PURE)
            && matches!(self.peek().kind, TokKind::Hash | TokKind::At))
        || (matches!(self.peek().kind, TokKind::Hash) && matches!(self.peek2().kind, TokKind::LParen))
    }

    pub(super) fn return_type(&mut self) -> Result<(Type, Span), Diagnostic> {
        let start = self.peek().span.start;
        let (ty, _) = self.return_type_inner()?;
        let end = self.toks[self.pos.saturating_sub(1)].span.end;
        Ok((ty, Span::new(start, end)))
    }

    fn return_type_inner(&mut self) -> Result<(Type, Span), Diagnostic> {
        if matches!(self.peek().kind, TokKind::LParen) {
            let start = self.bump().span;
            if self.looks_like_named_tuple(true) {
                let ty = self.parse_tuple_type(start)?;
                return Ok((ty, start));
            }
            let (ty, _) = self.type_()?;
            self.expect(TokKind::RParen, "to close this parenthesized return type")?;
            return Ok((ty, start));
        }

        let (ty, span) = self.type_()?;
        if let Type::Option(ok_ty) = ty {
            if self.type_starts_here() {
                let (err_ty, _) = self.type_()?;
                Ok((
                    Type::Result {
                        ok: ok_ty,
                        err: Box::new(err_ty),
                    },
                    span,
                ))
            } else {
                Ok((
                    Type::Result {
                        ok: ok_ty,
                        err: Box::new(Type::Named(Syntax::TYPE_ERROR.to_string())),
                    },
                    span,
                ))
            }
        } else {
            Ok((ty, span))
        }
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
            Ok(vec![name])
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
                "square brackets start collection types like `[Int]` or `[String: Int]`, and collection values like `[1, 2]`"
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
    /// D-SERDE6 (= C): true when the cursor (on `<`) begins a call-site turbofish
    /// `<T, …>(` — a balanced `<…>` immediately followed by `(`. Used to read `<`
    /// as type arguments on a call (`decode<Order>(s)`) rather than a comparison.
    pub(super) fn at_turbofish(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::Lt) {
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
                        return matches!(
                            self.toks.get(i + 1).map(|t| &t.kind),
                            Some(TokKind::LParen)
                        );
                    }
                }
                TokKind::Shr => {
                    depth -= 2;
                    if depth <= 0 {
                        return matches!(
                            self.toks.get(i + 1).map(|t| &t.kind),
                            Some(TokKind::LParen)
                        );
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

    /// D-SERDE6: parse a call-site turbofish `<T, …>` (cursor on `<`). Callers must
    /// first confirm [`Self::at_turbofish`].
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
        let start = self.peek().span;
        let base = match self.peek().kind.clone() {
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
            // D-EFF2/D-MARKERMOVE2 (G1): a callback effect bound rides the front
            // of a function type — `@Pure fn(T) -> U` (the callback must be pure;
            // the retired `@Pure fn(T) -> U` spelling teaches E0062) or
            // `#(Net) fn(T) -> U` (at most the listed effects, directive-only,
            // `#` never `@`). Anything else is not a type and falls through to
            // the normal "expected a type" error.
            TokKind::Hash
                if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_PURE)
                    || matches!(self.peek2().kind, TokKind::LParen) =>
            {
                let bound = self.parse_fn_type_effect_bound()?;
                self.fn_type(Some(bound))?
            }
            TokKind::At if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::KW_PURE) =>
            {
                let bound = self.parse_fn_type_effect_bound()?;
                self.fn_type(Some(bound))?
            }
            // D-QUAL4=A: `#Marker Type` — a value-tag prefix on a type. The marker
            // must be a PascalCase ident (not `Pure`/`(` which are fn-effect bounds).
            TokKind::Hash if matches!(&self.peek2().kind, TokKind::Ident(n) if !n.is_empty() && n.chars().next().map_or(false, |c| c.is_uppercase())) =>
            {
                self.bump(); // `#`
                let (marker, _) = self.expect_ident("after `#` in type-tag position")?;
                let (inner, _) = self.type_()?;
                Type::Tagged {
                    marker,
                    inner: Box::new(inner),
                }
            }
            TokKind::LBracket => {
                self.bump();
                let (first, first_span) = self.type_generic_arg("list/map type")?;
                if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    let (value, _) = self.type_generic_arg("map value")?;
                    self.expect(TokKind::RBracket, "after the value type in `[K: V]`")?;
                    Type::Map {
                        key: Box::new(first),
                        key_span: Some(first_span),
                        value: Box::new(value),
                    }
                } else if matches!(self.peek().kind, TokKind::Hash) {
                    // S76 (2026-06-16): `[T#N]` fixed-size list.
                    self.bump(); // consume `#`
                    let len = match &self.peek().kind {
                        TokKind::Int(n) => {
                            let n = *n;
                            self.bump();
                            n as u64
                        }
                        _ => {
                            let sp = self.peek().span;
                            self.diags.push(Diagnostic::error(
                                "E0963",
                                "expected a literal integer size after `#` in `[T#N]`".to_string(),
                                "the size must be a non-negative integer literal".to_string(),
                                "write `[T#4]` for a fixed-size list of 4 elements".to_string(),
                                Some(sp),
                            ));
                            0
                        }
                    };
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
                    // D-SG9/S42: explicit fixed-width numeric spellings. `I64`/`F64`
                    // are the same types as `Int`/`Float` and canonicalise here so
                    // they stay fully interchangeable; the rest are distinct widths.
                    Syntax::TYPE_I64 => Type::Int,
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
                    Syntax::FOREIGN_TEXT if retired_s14_teaching_enabled() => {
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
                    Syntax::FOREIGN_DYN if retired_s14_teaching_enabled() => {
                        self.diags.push(Generics::e0036(Syntax::FOREIGN_DYN, start));
                        let (trait_name, _) = self.expect_ident("after `dyn`")?;
                        Type::TraitObject(vec![trait_name])
                    }
                    Syntax::FOREIGN_BOX if retired_s14_teaching_enabled() => {
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
                        self.expect_type_args_open("Shared")?;
                        let (inner, _) = self.type_generic_arg("Shared")?;
                        self.maybe_close_type_args("after a shared element type")?;
                        Type::Shared(Box::new(inner))
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
                        self.diags.push(Diagnostic::error(
                            "E0406",
                            "`Result<T, E>` is old Jet error syntax".to_string(),
                            "fallible Jet types are written as `T ? E`".to_string(),
                            "write the return type as `T ? E`, or `T ?` for the default Error type"
                                .to_string(),
                            Some(start),
                        ));
                        self.expect_type_args_open("Result")?;
                        let (ok_ty, _) = self.type_generic_arg("Result ok")?;
                        self.expect(
                            TokKind::Comma,
                            "between the two types in old `Result<T, E>` syntax",
                        )?;
                        let (err_ty, _) = self.type_generic_arg("Result err")?;
                        self.maybe_close_type_args(
                            "after the error type in old `Result<T, E>` syntax",
                        )?;
                        Type::Result {
                            ok: Box::new(ok_ty),
                            err: Box::new(err_ty),
                        }
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
        if matches!(self.peek().kind, TokKind::QuestionQuestion) {
            let qspan = self.peek().span;
            return Err(Diagnostic::error(
                "E0309",
                "`??` isn't allowed on a type".to_string(),
                "an optional value is written `T?` once — there's no optional optional".to_string(),
                "use a single `?`, like `Int?`".to_string(),
                Some(qspan),
            ));
        }
        if matches!(self.peek().kind, TokKind::Question) {
            self.bump();
            if self.type_starts_here() {
                let (err_ty, _) = self.type_()?;
                return Ok((
                    Type::Result {
                        ok: Box::new(base),
                        err: Box::new(err_ty),
                    },
                    start,
                ));
            }
            return Ok((Type::Option(Box::new(base)), start));
        }
        Ok((base, start))
    }

    /// Parse a function type `fn(T1, …) -> R`, the cursor at `fn`. `effect_bound`
    /// is the D-EFF2 callback bound parsed off the front (`@Pure` → `Some([])`,
    /// `#(E,…)` → `Some([(name, span), …])`), or `None` for an unbounded type.
    fn fn_type(&mut self, effect_bound: Option<Vec<(String, Span)>>) -> Result<Type, Diagnostic> {
        self.expect(TokKind::KwFn, "to start a function type")?;
        self.expect(TokKind::LParen, "after `fn` in a function type")?;
        let mut params = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (pty, _) = self.type_()?;
                params.push(pty);
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between parameter types in `fn(...)`")?;
            }
        }
        self.expect(TokKind::RParen, "after parameter types in `fn(...)`")?;
        let ret = if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            let (r, _) = self.type_()?;
            Some(Box::new(r))
        } else {
            None
        };
        Ok(Type::Fn {
            params,
            ret,
            effect_bound,
        })
    }

    /// D-EFF2: parse the effect bound on the front of a function type, the cursor
    /// at `#`. `@Pure` yields the empty set (`Some([])`); `#(E1, E2, …)` yields the
    /// listed names (validated against the effect vocabulary in sema, not here).
    /// The caller has confirmed via lookahead that a `fn` follows.
    /// D-EFF2/D-MARKERMOVE2 (G1): parse a callback effect bound. `@Pure fn(…)`
    /// is the one carve-out where a contract marker prefixes a function TYPE
    /// instead of a declaration — the retired `@Pure fn(…)` spelling still
    /// parses here so it can teach E0062. The general effect-list form,
    /// `#(Net) fn(…)`, is a directive and stays on `#` only.
    fn parse_fn_type_effect_bound(&mut self) -> Result<Vec<(String, Span)>, Diagnostic> {
        let sigil = if matches!(self.peek().kind, TokKind::Hash | TokKind::At) {
            self.bump()
        } else {
            self.expect(TokKind::Hash, "to start a callback effect bound")?;
            unreachable!()
        };
        if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_PURE) {
            let name_tok = self.bump(); // `Pure`
            if matches!(sigil.kind, TokKind::Hash) {
                self.diags.push(Self::e0062_contract_on_hash(
                    Syntax::KW_PURE,
                    Span::new(sigil.span.start, name_tok.span.end),
                ));
            }
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
