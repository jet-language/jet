use super::*;

impl<'a> Parser<'a> {
    /// S58 (E2-M13): parse the tail of `alias.Ptr<T>.from_addr(addr)`, with the
    /// cursor at the `<`. The `alias`/`alias_span` are the already-parsed
    /// module alias and `.Ptr` member.
    fn ptr_from_addr(&mut self, alias: String, alias_span: Span) -> Result<Expr, Diagnostic> {
        self.expect_type_args_open(Syntax::TYPE_PTR)?;
        let (elem, _) = self.type_()?;
        if matches!(self.peek().kind, TokKind::Comma) {
            return Err(Diagnostic::error(
                "E0003",
                format!("`{}<…>` takes exactly one element type", Syntax::TYPE_PTR),
                "a pointer points at a single element type".to_string(),
                format!("write `{}.{}<Int>.{}(addr)`", alias, Syntax::TYPE_PTR, Syntax::MEM_FROM_ADDR),
                Some(self.peek().span),
            ));
        }
        self.expect_type_args_close(&format!("after `{}<…>`", Syntax::TYPE_PTR))?;
        self.expect(TokKind::Dot, &format!("after `{}<…>`", Syntax::TYPE_PTR))?;
        let (method, method_span) = self.expect_field_name()?;
        if method != Syntax::MEM_FROM_ADDR {
            return Err(Diagnostic::error(
                "E0003",
                format!("`{}<…>` has no static method `{}`", Syntax::TYPE_PTR, method),
                "a typed pointer is built from an address".to_string(),
                format!("write `{}.{}<Int>.{}(addr)`", alias, Syntax::TYPE_PTR, Syntax::MEM_FROM_ADDR),
                Some(method_span),
            ));
        }
        self.expect(TokKind::LParen, &format!("after `{}`", Syntax::MEM_FROM_ADDR))?;
        let addr = self.expr()?;
        self.expect(TokKind::RParen, "to finish the call")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(Expr::PtrFromAddr {
            alias,
            alias_span,
            elem,
            addr: Box::new(addr),
            span: Span::new(alias_span.start, end),
        })
    }

    /// S73: `( name : … )` — named tuple literal or type, not grouping.
    /// When `lparen_consumed` is true, `self.pos` is already on the first member name.
    pub(super) fn looks_like_named_tuple(&self, lparen_consumed: bool) -> bool {
        let i = if lparen_consumed {
            self.pos
        } else {
            self.pos + 1
        };
        matches!(
            self.toks.get(i).map(|t| &t.kind),
            Some(TokKind::Ident(_))
        ) && matches!(
            self.toks.get(i + 1).map(|t| &t.kind),
            Some(TokKind::Colon)
        )
    }

    fn emit_positional_tuple_error(&mut self, span: Span) {
        self.diags.push(Diagnostic::error(
            "E0048",
            format!(
                "{} tuples name every member — positional `(1, 2)` isn't allowed (S73)",
                Syntax::LANG_NAME
            ),
            "named members make field access obvious and avoid `.0`, which collides with decimal numbers"
                .to_string(),
            "write named members: `(x: 1, y: 2)` and use `p.x`, not `p.0`".to_string(),
            Some(span),
        ));
    }

    fn emit_numeric_field_error(&mut self, span: Span) {
        self.diags.push(Diagnostic::error(
            "E0049",
            format!(
                "{} doesn't use numeric field access like `.0` (S73)",
                Syntax::LANG_NAME
            ),
            "`.0` looks like the start of a decimal number, so tuple members must have names"
                .to_string(),
            "name the members when you build the tuple: `(x: 1, y: 2)`, then read `p.x`"
                .to_string(),
            Some(span),
        ));
    }

    /// S73: reject `.0` / `.1` field access before `expect_ident`.
    fn expect_field_name(&mut self) -> Result<(String, Span), Diagnostic> {
        if matches!(
            self.peek().kind,
            TokKind::Int(_) | TokKind::Float(_)
        ) {
            let span = self.peek().span;
            self.bump();
            self.emit_numeric_field_error(span);
            return Ok(("0".to_string(), span));
        }
        // D-ITER1: `take` is `KwMove` in the lexer but is valid as a method name
        // in dot position (`xs.take(n)`). Accept it as an identifier here.
        if matches!(self.peek().kind, TokKind::KwMove) {
            let span = self.peek().span;
            self.bump();
            return Ok((Syntax::KW_MOVE.to_string(), span));
        }
        self.expect_ident("after `.`")
    }

    fn sync_to_rparen(&mut self) {
        let mut depth = 0;
        while self.pos < self.toks.len() {
            match self.peek().kind {
                TokKind::LParen => depth += 1,
                TokKind::RParen if depth == 0 => {
                    self.bump();
                    return;
                }
                TokKind::RParen => depth -= 1,
                TokKind::Semi | TokKind::RBrace if depth == 0 => return,
                _ => {}
            }
            self.bump();
        }
    }

    /// S73: `(x: expr, y: expr)` after the opening `(`.
    fn parse_tuple_lit(&mut self, open: Span) -> Result<Expr, Diagnostic> {
        let mut fields = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (name, _) = self.expect_ident("for a tuple member name")?;
                self.expect(TokKind::Colon, "after each tuple member name")?;
                let value = self.expr()?;
                fields.push((name, value));
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
        self.expect(TokKind::RParen, "to close this tuple")?;
        if fields.len() < 2 {
            return Err(Diagnostic::error(
                "E0003",
                "a tuple needs at least two named members".to_string(),
                "a single `(name: value)` would be ambiguous with grouping — use a one-field `struct` instead"
                    .to_string(),
                "add another member: `(x: 1, y: 2)`".to_string(),
                Some(Span::new(open.start, close.end)),
            ));
        }
        Ok(Expr::TupleLit(fields, Span::new(open.start, close.end), None))
    }

    fn parse_paren_primary(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let open = self.bump().span;
        if self.looks_like_named_tuple(true) {
            return self.parse_tuple_lit(open);
        }
        if self.after_lparen_is_positional_tuple() {
            self.emit_positional_tuple_error(open);
            self.sync_to_rparen();
            return Ok(Expr::Int(0, open, None));
        }
        let inner = self.expr()?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.emit_positional_tuple_error(open);
            self.sync_to_rparen();
            return Ok(Expr::Int(0, open, None));
        }
        self.expect(TokKind::RParen, "to close this `(`")?;
        Ok(inner)
    }

    /// True when `(` starts `( expr , … )` without member names — rejected (S73).
    /// Call with `self.pos` on the first token inside `(`.
    fn after_lparen_is_positional_tuple(&self) -> bool {
        let mut i = self.pos;
        if i >= self.toks.len() {
            return false;
        }
        if matches!(self.toks[i].kind, TokKind::RParen) {
            return false;
        }
        if matches!(self.toks[i].kind, TokKind::Ident(_))
            && self
                .toks
                .get(i + 1)
                .is_some_and(|t| matches!(t.kind, TokKind::Colon))
        {
            return false;
        }
        loop {
            match &self.toks[i].kind {
                TokKind::RParen => return false,
                TokKind::Comma => return true,
                TokKind::Colon => return false,
                TokKind::LParen | TokKind::LBrace | TokKind::LBracket => {
                    i += 1;
                    let mut depth = 1;
                    while i < self.toks.len() && depth > 0 {
                        match self.toks[i].kind {
                            TokKind::LParen | TokKind::LBrace | TokKind::LBracket => depth += 1,
                            TokKind::RParen | TokKind::RBrace | TokKind::RBracket => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                }
                _ => i += 1,
            }
            if i >= self.toks.len() {
                return false;
            }
        }
    }

    pub(super) fn expr(&mut self) -> Result<Expr, Diagnostic> {
        let span = self.peek().span;
        self.with_nesting(span, |p| p.expr_or_fallback(true))
    }

    pub(super) fn expr_no_struct_lit(&mut self) -> Result<Expr, Diagnostic> {
        let span = self.peek().span;
        self.with_nesting(span, |p| p.expr_or_fallback(false))
    }

    /// S35/S71: the `??` fallback binds looser than `&&` / `||`.
    fn expr_or_fallback(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_or(allow_struct_lit)?;
        loop {
            match &self.peek().kind {
                TokKind::QuestionQuestion => {}
                // S71 (D-SG6): the retired word `or` — teach `??`, then recover.
                TokKind::Ident(n) if n == Syntax::FOREIGN_OR_FALLBACK => {
                    let span = self.peek().span;
                    self.diags.push(Diagnostic::error(
                        "E0045",
                        "Jet writes the fallback as `??`, not `or`".to_string(),
                        "`??` supplies a value when a `T?` is absent or a `T ? E` failed — `count ?? 0`, `read() ?? return`"
                            .to_string(),
                        "replace `or` with `??`".to_string(),
                        Some(span),
                    ));
                }
                _ => break,
            }
            let op_span = self.bump().span;
            let fallback = self.parse_or_fallback(allow_struct_lit)?;
            let end = match &fallback {
                OrFallback::Value(e) => e.span().end,
                OrFallback::Return(_, s) => s.end,
                OrFallback::Panic { name_span, .. } => name_span.end,
            };
            let span = Span::new(lhs.span().start, end.max(op_span.end));
            lhs = Expr::OrFallback {
                value: Box::new(lhs),
                fallback,
                is_option: false,
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_or_fallback(&mut self, allow_struct_lit: bool) -> Result<OrFallback, Diagnostic> {
        if matches!(self.peek().kind, TokKind::KwReturn) {
            let span = self.bump().span;
            if self.starts_expr(&self.peek().kind) {
                let e = self.expr_or(allow_struct_lit)?;
                return Ok(OrFallback::Return(Some(Box::new(e)), span));
            }
            return Ok(OrFallback::Return(None, span));
        }
        let e = self.expr_or(allow_struct_lit)?;
        if let Expr::Call(call) = &e {
            if call.name == Syntax::BUILTIN_PANIC {
                return Ok(OrFallback::Panic {
                    name_span: call.name_span,
                    args: call.args.clone(),
                });
            }
        }
        Ok(OrFallback::Value(Box::new(e)))
    }

    fn expr_or(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_and(allow_struct_lit)?;
        loop {
            let is_or = matches!(self.peek().kind, TokKind::OrOr);
            if !is_or {
                break;
            }
            let op_span = self.bump().span;
            let rhs = self.expr_and(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_and(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_cmp(allow_struct_lit)?;
        loop {
            let is_and = match &self.peek().kind {
                TokKind::AndAnd => true,
                TokKind::Ident(n) if n == Syntax::FOREIGN_AND => {
                    self.foreign_logic_error(Syntax::FOREIGN_AND, Syntax::OP_AND);
                    true
                }
                _ => false,
            };
            if !is_and {
                break;
            }
            let op_span = self.bump().span;
            let rhs = self.expr_cmp(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::And, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    /// Comparisons don't chain: `a < b < c` is a parse error with guidance.
    fn expr_cmp(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let lhs = self.expr_bitor(allow_struct_lit)?;
        let op = match &self.peek().kind {
            TokKind::EqEq => Some(BinOp::Eq),
            TokKind::NotEq => Some(BinOp::Ne),
            TokKind::Lt => Some(BinOp::Lt),
            TokKind::Gt => Some(BinOp::Gt),
            TokKind::Le => Some(BinOp::Le),
            TokKind::Ge => Some(BinOp::Ge),
            _ => None,
        };
        let Some(op) = op else { return Ok(lhs) };
        let op_span = self.bump().span;
        let rhs = if op == BinOp::Eq {
            if let Some(pat) = self.try_pattern_rhs()? {
                let span = Span::new(lhs.span().start, pat_span(&pat).end.max(op_span.end));
                return Ok(Expr::PatternTest {
                    subject: Box::new(lhs),
                    pattern: pat,
                    span,
                });
            }
            self.expr_bitor(allow_struct_lit)?
        } else {
            self.expr_bitor(allow_struct_lit)?
        };
        let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
        let cmp = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
        if let Some(second) = match &self.peek().kind {
            TokKind::EqEq
            | TokKind::NotEq
            | TokKind::Lt
            | TokKind::Gt
            | TokKind::Le
            | TokKind::Ge => Some(self.peek().span),
            _ => None,
        } {
            return Err(Diagnostic::error(
                "E0003",
                "comparisons can't be chained".to_string(),
                format!(
                    "`a < b < c` doesn't compare all three; check each pair and join with `{}`",
                    Syntax::OP_AND
                ),
                format!("write `a < b {} b < c`", Syntax::OP_AND),
                Some(second),
            ));
        }
        Ok(cmp)
    }

    fn expr_bitor(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_bitxor(allow_struct_lit)?;
        while matches!(self.peek().kind, TokKind::Pipe) {
            let op_span = self.bump().span;
            let rhs = self.expr_bitxor(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::BitOr, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_bitxor(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_bitand(allow_struct_lit)?;
        while matches!(self.peek().kind, TokKind::Caret) {
            let op_span = self.bump().span;
            let rhs = self.expr_bitand(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::BitXor, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_bitand(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_shift(allow_struct_lit)?;
        while matches!(self.peek().kind, TokKind::Amp) {
            let op_span = self.bump().span;
            let rhs = self.expr_shift(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(BinOp::BitAnd, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_shift(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_add(allow_struct_lit)?;
        loop {
            let op = match &self.peek().kind {
                TokKind::Shl => BinOp::Shl,
                TokKind::Shr => BinOp::Shr,
                _ => break,
            };
            let op_span = self.bump().span;
            let rhs = self.expr_add(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_add(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_mul(allow_struct_lit)?;
        loop {
            let op = match &self.peek().kind {
                TokKind::Plus => BinOp::Add,
                TokKind::Minus => BinOp::Sub,
                _ => break,
            };
            let op_span = self.bump().span;
            let rhs = self.expr_mul(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_mul(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_unary(allow_struct_lit)?;
        loop {
            let op = match &self.peek().kind {
                TokKind::Star => BinOp::Mul,
                TokKind::Slash => BinOp::Div,
                TokKind::Percent => BinOp::Rem,
                _ => break,
            };
            let op_span = self.bump().span;
            let rhs = self.expr_unary(allow_struct_lit)?;
            let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
        }
        Ok(lhs)
    }

    fn expr_unary(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let span = self.peek().span;
        self.with_nesting(span, |p| p.expr_unary_inner(allow_struct_lit))
    }

    fn expr_unary_inner(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        match &self.peek().kind {
            TokKind::Minus => {
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Unary(UnOp::Neg, Box::new(inner), full))
            }
            TokKind::Bang => {
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Unary(UnOp::Not, Box::new(inner), full))
            }
            TokKind::Ident(n)
                if n == Syntax::FOREIGN_NOT && self.starts_expr(&self.peek2().kind) =>
            {
                self.foreign_logic_error(Syntax::FOREIGN_NOT, Syntax::OP_NOT);
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Unary(UnOp::Not, Box::new(inner), full))
            }
            TokKind::Ident(n)
                if n == Syntax::FOREIGN_TRY && self.starts_expr(&self.peek2().kind) =>
            {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0014",
                    format!(
                        "{} does not use `{}`",
                        Syntax::LANG_NAME,
                        Syntax::FOREIGN_TRY
                    ),
                    format!(
                        "a call that can fail is marked with `{}` after it, like `parse(x){}`",
                        Syntax::OP_TRY_SUFFIX,
                        Syntax::OP_TRY_SUFFIX
                    ),
                    format!("write `parse(x){}` instead", Syntax::OP_TRY_SUFFIX),
                    Some(t.span),
                ));
                self.expr_unary(allow_struct_lit)
            }
            TokKind::Star => {
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                Ok(Expr::Deref(Box::new(inner), span))
            }
            _ => self.expr_postfix(allow_struct_lit),
        }
    }

    fn expr_postfix(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut expr = self.expr_primary(allow_struct_lit)?;
        loop {
            match &self.peek().kind {
                TokKind::Dot => {
                    let dot = self.bump().span;
                    // S75 (2026-06-16): `f.[a, b, c]` fan-out — `.` immediately followed by `[`
                    if matches!(self.peek().kind, TokKind::LBracket) {
                        expr = self.parse_fan_out_bracket(Box::new(expr), dot)?;
                        continue;
                    }
                    let (member, member_span) = self.expect_field_name()?;
                    // S58 (E2-M13): `alias.Ptr<T>.from_addr(addr)` — a typed
                    // pointer constructor through a `core.mem` alias. Recognise
                    // the `<…>` here (postfix position) so `<` is read as a
                    // type-arg list, not a comparison.
                    if member == Syntax::TYPE_PTR
                        && matches!(self.peek().kind, TokKind::Lt)
                    {
                        if let Expr::Ident(alias, alias_span) = &expr {
                            let alias = alias.clone();
                            let alias_span = *alias_span;
                            expr = self.ptr_from_addr(alias, alias_span)?;
                            continue;
                        }
                    }
                    if matches!(self.peek().kind, TokKind::LParen) {
                        self.bump();
                        let mut args = Vec::new();
                        if !matches!(self.peek().kind, TokKind::RParen) {
                            loop {
                                args.push(self.call_arg()?);
                                if matches!(self.peek().kind, TokKind::RParen) {
                                    break;
                                }
                                self.expect(TokKind::Comma, "between arguments")?;
                            }
                        }
                        self.expect(TokKind::RParen, "to finish the call")?;
                        expr = Expr::MethodCall {
                            receiver: Box::new(expr),
                            method: member,
                            method_span: member_span,
                            args,
                            recv_type: None,
                            resolved_ret: None,
                        };
                    } else {
                        expr = Expr::Field(Box::new(expr), member, member_span);
                    }
                }
                TokKind::Question => {
                    let qspan = self.bump().span;
                    let full = Span::new(expr.span().start, qspan.end);
                    expr = Expr::Try(Box::new(expr), full, TryConvert::None);
                }
                // S71 (D-SG6): `base?.field` optional chaining.
                TokKind::QuestionDot => {
                    self.bump();
                    let (member, member_span) = self.expect_ident("after `?.`")?;
                    if matches!(self.peek().kind, TokKind::LParen) {
                        return Err(Diagnostic::error(
                            "E0046",
                            "optional chaining `?.` only reaches fields, not methods".to_string(),
                            "`a?.b` short-circuits a `T?` to absent; calling through `?.` isn't in yet"
                                .to_string(),
                            "unwrap first, e.g. `(a ?? return).method()`, or test with `== present`"
                                .to_string(),
                            Some(member_span),
                        ));
                    }
                    let span = Span::new(expr.span().start, member_span.end);
                    expr = Expr::OptField {
                        base: Box::new(expr),
                        member,
                        member_span,
                        flatten: false,
                        span,
                    };
                }
                TokKind::LParen => {
                    let open = self.bump().span;
                    let mut args = Vec::new();
                    if !matches!(self.peek().kind, TokKind::RParen) {
                        loop {
                            args.push(self.call_arg()?);
                            if matches!(self.peek().kind, TokKind::RParen) {
                                break;
                            }
                            self.expect(TokKind::Comma, "between arguments")?;
                        }
                    }
                    self.expect(TokKind::RParen, "to finish the call")?;
                    let close = self.toks[self.pos - 1].span;
                    let span = Span::new(open.start, close.end);
                    expr = Expr::CallValue {
                        callee: Box::new(expr),
                        args,
                        span,
                    };
                }
                TokKind::LBrace => {
                    // In a control-flow header (`for … in expr {`, `if cond {`, …)
                    // the `{` opens the body, never a struct literal — even after a
                    // field chain like `recv.field`. Only treat `expr.Type { … }` as
                    // an import-namespace struct literal when struct literals are
                    // allowed in this position.
                    let import_lit = if !allow_struct_lit {
                        None
                    } else if let Expr::Field(inner, type_name, _) = &expr {
                        if let Expr::Ident(alias, _) = inner.as_ref() {
                            Some((alias.clone(), type_name.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some((alias, type_name)) = import_lit {
                        let start = expr.span().start;
                        expr = self.struct_lit_after_import(alias, type_name, start)?;
                    } else {
                        break;
                    }
                }
                TokKind::LBracket => {
                    let open = self.bump().span;
                    let start = self.expr()?;
                    if matches!(self.peek().kind, TokKind::DotDot) {
                        self.bump();
                        let end = self.expr()?;
                        self.expect(TokKind::RBracket, "after a slice range")?;
                        let close = self.toks[self.pos - 1].span;
                        let span = Span::new(open.start, close.end);
                        expr = Expr::Slice {
                            base: Box::new(expr),
                            start: Box::new(start),
                            end: Box::new(end),
                            span,
                        };
                    } else {
                        self.expect(TokKind::RBracket, "after an index")?;
                        let close = self.toks[self.pos - 1].span;
                        let span = Span::new(open.start, close.end);
                        expr = Expr::Index {
                            base: Box::new(expr),
                            index: Box::new(start),
                            span,
                            kind: crate::AST::IndexKind::Unknown,
                        };
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    /// S75 (2026-06-16): parse `.[item, …]` after the `.` has already been consumed.
    /// `dot_span` is the span of the consumed `.`. Called from both `expr_primary`
    /// (for `ident.[…]`) and `expr_postfix` (for chained `expr.[…]`).
    fn parse_fan_out_bracket(
        &mut self,
        callee: Box<Expr>,
        dot_span: Span,
    ) -> Result<Expr, Diagnostic> {
        self.bump(); // consume `[`
        let mut items = Vec::new();
        if !matches!(self.peek().kind, TokKind::RBracket) {
            loop {
                items.push(self.expr()?);
                if matches!(self.peek().kind, TokKind::RBracket) {
                    break;
                }
                self.expect(TokKind::Comma, "between fan-out items")?;
                if matches!(self.peek().kind, TokKind::RBracket) {
                    break; // trailing comma
                }
            }
        }
        self.expect(TokKind::RBracket, "to close the fan-out `.[`")?;
        let close = self.toks[self.pos - 1].span;
        let span = Span::new(dot_span.start, close.end);
        Ok(Expr::FanOut { callee, items, span })
    }

    pub(super) fn expr_to_lvalue(&mut self, expr: Expr) -> Result<LValue, Diagnostic> {
        match expr {
            Expr::Ident(name, name_span) => Ok(LValue::Local { name, name_span }),
            Expr::Index {
                base, index, span, ..
            } => Ok(LValue::Index {
                base,
                index,
                span,
                kind: crate::AST::IndexKind::Unknown,
            }),
            // D-MUTSELF1: a field-access target `place.field = v` (the headline being
            // `self.field = v` in a `mut self` method). Sema gates whether the root is
            // mutable (E0205); the parser only records the place.
            Expr::Field(base, field, span) => Ok(LValue::Field { base, field, span }),
            other => Err(Diagnostic::error(
                "E0003",
                "this value can't be assigned to".to_string(),
                "only a name or an indexed slot like `items[0]` can appear on the left of `=`"
                    .to_string(),
                format!("use `name {} ...` or `map[key] = ...`", Syntax::SIGIL_BIND_MUT),
                Some(other.span()),
            )),
        }
    }

    fn expr_primary(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        match self.peek().kind.clone() {
            TokKind::KwOk => {
                let span = self.bump().span;
                self.expect(TokKind::LParen, "after `ok`")?;
                let inner = self.expr()?;
                self.expect(TokKind::RParen, "after the value inside `ok(...)`")?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Ok(Box::new(inner), full))
            }
            TokKind::KwErr => {
                let span = self.bump().span;
                self.expect(TokKind::LParen, "after `err`")?;
                let inner = self.expr()?;
                self.expect(TokKind::RParen, "after the value inside `err(...)`")?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Err(Box::new(inner), full))
            }
            TokKind::KwIt => {
                let span = self.bump().span;
                Ok(Expr::Ident(Syntax::KW_IT.to_string(), span))
            }
            TokKind::Ident(name)
                if name == Syntax::LIT_VALUE
                    && matches!(
                        self.toks.get(self.pos + 1).map(|t| &t.kind),
                        Some(TokKind::LParen)
                    ) =>
            {
                let span = self.bump().span;
                self.expect(TokKind::LParen, "after `value`")?;
                let inner = self.expr()?;
                self.expect(TokKind::RParen, "after the value inside `value(...)`")?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Present(Box::new(inner), full))
            }
            TokKind::KwNull => {
                let span = self.bump().span;
                return Ok(Expr::Absent(span));
            }
            TokKind::Hash
                if matches!(
                    self.toks.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokKind::Ident(n)) if n == Syntax::KW_TODO
                ) =>
            {
                // D-TOOL2 (D-CASING1 follow-on): `#Todo` typed hole — valid in any
                // expression position; sema fills `expected_type`; codegen emits a
                // panic.
                let start = self.bump().span.start; // `#`
                let span = Span::new(start, self.bump().span.end); // `Todo`
                return Ok(Expr::Todo { span, expected_type: None });
            }
            TokKind::Ident(name) if name == Syntax::FOREIGN_TODO => {
                // S14: bare lowercase `todo` is the retired spelling (E0054).
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0054",
                    format!(
                        "the typed hole is written `#{}`, not bare `{}`",
                        Syntax::KW_TODO,
                        Syntax::FOREIGN_TODO
                    ),
                    format!(
                        "`#{}` is a marker, like every other `#`-tag, so an unfinished spot draws the eye",
                        Syntax::KW_TODO
                    ),
                    format!("write: #{}", Syntax::KW_TODO),
                    Some(t.span),
                ));
                return Ok(Expr::Todo { span: t.span, expected_type: None });
            }
            TokKind::Ident(name)
                if matches!(name.as_str(), Syntax::FOREIGN_THROW | Syntax::FOREIGN_RAISE) =>
            {
                let t = self.bump();
                let foreign = name.clone();
                self.diags.push(Diagnostic::error(
                    "E0026",
                    format!("{} doesn't use `{}`", Syntax::LANG_NAME, foreign),
                    "a function that can fail returns `T ? E` and signals failure with `err(...)`"
                        .to_string(),
                    format!("return `err(...)` instead of `{}`", foreign),
                    Some(t.span),
                ));
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name)
                if matches!(
                    name.as_str(),
                    Syntax::FOREIGN_CATCH | Syntax::FOREIGN_EXCEPT
                ) =>
            {
                let t = self.bump();
                let foreign = name.clone();
                self.diags.push(Diagnostic::error(
                    "E0024",
                    format!("{} doesn't use `{}`", Syntax::LANG_NAME, foreign),
                    "handle a failure with `or` for a fallback, or test with `== err(...)`"
                        .to_string(),
                    format!(
                        "write `parse(x) or 0` or `if x == err(e) {{ ... }}` instead of `{}`",
                        foreign
                    ),
                    Some(t.span),
                ));
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name)
                if matches!(
                    name.as_str(),
                    Syntax::FOREIGN_UNWRAP | Syntax::FOREIGN_EXPECT
                ) =>
            {
                let t = self.bump();
                let foreign = name.clone();
                self.diags.push(Diagnostic::error(
                    "E0025",
                    format!("{} doesn't use `{}`", Syntax::LANG_NAME, foreign),
                    "when failure should stop the program, use `or panic(\"…\")`".to_string(),
                    format!(
                        "write `parse(x) or panic(\"…\")` instead of `.{}()`",
                        foreign
                    ),
                    Some(t.span),
                ));
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name)
                if matches!(
                    name.as_str(),
                    Syntax::FOREIGN_NONE
                        | Syntax::FOREIGN_SOME
                        | Syntax::FOREIGN_NIL
                        | Syntax::FOREIGN_NONE_LOWER
                        | Syntax::FOREIGN_SOME_LOWER
                ) =>
            {
                let t = self.bump();
                let foreign = if let TokKind::Ident(n) = &t.kind {
                    n.clone()
                } else {
                    unreachable!()
                };
                let (canonical, fix) = match foreign.as_str() {
                    Syntax::FOREIGN_NONE | Syntax::FOREIGN_NONE_LOWER | Syntax::FOREIGN_NIL => {
                        (Syntax::LIT_NULL, Syntax::LIT_NULL)
                    }
                    _ => (Syntax::LIT_VALUE, Syntax::LIT_VALUE),
                };
                self.diags.push(Diagnostic::error(
                    "E0020",
                    format!(
                        "optional values use `{}` and `{}`, not `{}`",
                        Syntax::LIT_VALUE,
                        Syntax::LIT_NULL,
                        foreign
                    ),
                    format!(
                        "{} uses exactly one spelling for each thing, so all code reads the same",
                        Syntax::LANG_NAME
                    ),
                    format!("replace `{}` with `{}`", foreign, fix),
                    Some(t.span),
                ));
                if canonical == Syntax::LIT_NULL {
                    Ok(Expr::Absent(t.span))
                } else {
                    self.expect(TokKind::LParen, "after `value`")?;
                    let inner = self.expr()?;
                    self.expect(TokKind::RParen, "after the value inside `value(...)`")?;
                    let full = Span::new(t.span.start, inner.span().end);
                    Ok(Expr::Present(Box::new(inner), full))
                }
            }
            TokKind::Str(parts) => {
                let span = self.bump().span;
                let mut out = Vec::new();
                for part in parts {
                    match part {
                        StrTokPart::Lit(s) => out.push(StrPart::Lit(s)),
                        StrTokPart::Interp(toks) => {
                            let mut sub = Parser {
                                toks: &toks,
                                pos: 0,
                                diags: Vec::new(),
                                pending_type_gt: false,
                                depth: self.depth,
                                type_generic_depth: 0,
                                type_generic_chain: Vec::new(),
                                type_generic_truncated: false,
                            };
                            let e = sub.expr()?;
                            if !sub.diags.is_empty() {
                                let mut ds = sub.diags;
                                let first = ds.remove(0);
                                self.diags.extend(ds);
                                return Err(first);
                            }
                            if !matches!(sub.peek().kind, TokKind::Eof) {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    format!(
                                        "unexpected {} inside this interpolated `{{ }}`",
                                        describe(&sub.peek().kind)
                                    ),
                                    "the braces hold exactly one value".to_string(),
                                    "keep one value per `{ }`, e.g. \"{a} and {b}\"".to_string(),
                                    Some(sub.peek().span),
                                ));
                            }
                            out.push(StrPart::Interp(e));
                        }
                    }
                }
                Ok(Expr::Str(out, span))
            }
            TokKind::Int(n) => {
                let span = self.bump().span;
                Ok(Expr::Int(n, span, None))
            }
            TokKind::Float(v) => {
                let span = self.bump().span;
                Ok(Expr::Float(v, span))
            }
            TokKind::Char(ch) => {
                let span = self.bump().span;
                Ok(Expr::Char(ch, span))
            }
            TokKind::LBracket => self.list_or_map_lit(),
            TokKind::KwTrue => {
                let span = self.bump().span;
                Ok(Expr::Bool(true, span))
            }
            TokKind::KwFalse => {
                let span = self.bump().span;
                Ok(Expr::Bool(false, span))
            }
            TokKind::KwSelf => {
                let span = self.bump().span;
                Ok(Expr::Ident(Syntax::KW_SELF.to_string(), span))
            }
            // S68 (D-SG2): `if` used as a value. Statement-position `if` is
            // handled earlier in `stmt`, so reaching here means expression use.
            TokKind::KwIf => self.parse_if_expr(),
            TokKind::KwMove
                if matches!(
                    self.toks.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokKind::LParen)
                ) =>
            {
                let takes = self.parse_lambda_takes()?;
                Ok(Expr::Lambda(self.parse_lambda(takes)?))
            }
            TokKind::LParen if self.after_lparen_is_lambda() => {
                Ok(Expr::Lambda(self.parse_lambda(vec![])?))
            }
            TokKind::LParen => self.parse_paren_primary(allow_struct_lit),
            TokKind::Pipe => {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0033",
                    format!("{} doesn't use `|` pipes for lambdas", Syntax::LANG_NAME),
                    "a short function is written with parentheses and `=>`".to_string(),
                    "write `(x) => x + 1` instead of `|x| x + 1`".to_string(),
                    Some(span),
                ));
                while !matches!(self.peek().kind, TokKind::Pipe | TokKind::Eof) {
                    self.bump();
                }
                if matches!(self.peek().kind, TokKind::Pipe) {
                    self.bump();
                }
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name) if name == Syntax::FOREIGN_LAMBDA => {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0032",
                    format!(
                        "{} doesn't use the `{}` keyword for short functions",
                        Syntax::LANG_NAME,
                        Syntax::FOREIGN_LAMBDA
                    ),
                    "write a lambda with parentheses and `=>` instead".to_string(),
                    "e.g. `(x) => x + 1` instead of `lambda x { ... }`".to_string(),
                    Some(span),
                ));
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name)
                if matches!(
                    name.as_str(),
                    Syntax::FOREIGN_VEC | Syntax::FOREIGN_HASHMAP | Syntax::FOREIGN_DICT
                ) =>
            {
                let t = self.bump();
                let foreign = name.clone();
                let canonical = if foreign == Syntax::FOREIGN_VEC {
                    Syntax::TYPE_LIST
                } else {
                    Syntax::TYPE_MAP
                };
                self.diags.push(Diagnostic::error(
                    "E0028",
                    format!(
                        "{} uses `{}`, not `{}`",
                        Syntax::LANG_NAME,
                        canonical,
                        foreign
                    ),
                    format!("`{}` is the built-in collection type", canonical),
                    format!("replace `{}` with `{}`", foreign, canonical),
                    Some(t.span),
                ));
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name) if name == Syntax::FOREIGN_AS => {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0030",
                    format!(
                        "{} doesn't use `{}` for conversions",
                        Syntax::LANG_NAME,
                        Syntax::FOREIGN_AS
                    ),
                    "convert with methods like `.to_float()` or `.to_string()`".to_string(),
                    "e.g. `x.to_float()` instead of `x as Float`".to_string(),
                    Some(t.span),
                ));
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name) if name == Syntax::FOREIGN_APPEND => {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0027",
                    format!("lists use `{}`, not `{}`", "push", Syntax::FOREIGN_APPEND),
                    "add an item to the end of a list with `.push(value)`".to_string(),
                    "e.g. `items.push(x)`".to_string(),
                    Some(span),
                ));
                if matches!(self.peek().kind, TokKind::LParen) {
                    self.bump();
                    let _ = self.expr();
                    let _ = self.expect(TokKind::RParen, "after append args");
                }
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name) => {
                let span = self.bump().span;
                let type_name = name.clone();
                let mut type_args = Vec::new();
                if allow_struct_lit
                    && matches!(self.peek().kind, TokKind::Lt)
                    && type_name.chars().next().is_some_and(|c| c.is_uppercase())
                {
                    self.expect_type_args_open(&type_name)?;
                    loop {
                        let (arg, _) = self.type_()?;
                        type_args.push(arg);
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                    self.expect_type_args_close(&format!("after `{type_name}<…>`"))?;
                }
                if allow_struct_lit && matches!(self.peek().kind, TokKind::LBrace) {
                    return self.struct_lit_after_name(type_name, type_args, span);
                }
                if matches!(self.peek().kind, TokKind::Dot) {
                    let dot_span = self.bump().span;
                    // S75 (2026-06-16): `ident.[a, b, c]` fan-out
                    if matches!(self.peek().kind, TokKind::LBracket) {
                        let callee = Box::new(Expr::Ident(type_name, span));
                        return self.parse_fan_out_bracket(callee, dot_span);
                    }
                    let (member, member_span) = self.expect_field_name()?;
                    // S58 (E2-M13): `alias.Ptr<T>.from_addr(addr)` — a typed
                    // pointer constructor through a `core.mem` alias. Recognise
                    // the `<…>` here (primary position, where `alias.Member` is
                    // consumed) so `<` is read as a type-arg list, not a
                    // comparison. Mirrors the postfix-position trigger.
                    if member == Syntax::TYPE_PTR && matches!(self.peek().kind, TokKind::Lt) {
                        return self.ptr_from_addr(type_name, span);
                    }
                    if matches!(self.peek().kind, TokKind::LParen) {
                        self.bump();
                        let mut args = Vec::new();
                        if !matches!(self.peek().kind, TokKind::RParen) {
                            loop {
                                args.push(self.call_arg()?);
                                if matches!(self.peek().kind, TokKind::RParen) {
                                    break;
                                }
                                self.expect(TokKind::Comma, "between arguments")?;
                            }
                        }
                        self.expect(TokKind::RParen, "to finish the call")?;
                        return Ok(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident(type_name, span)),
                            method: member,
                            method_span: member_span,
                            args,
                            recv_type: None,
                            resolved_ret: None,
                        });
                    }
                    return Ok(Expr::Field(
                        Box::new(Expr::Ident(type_name, span)),
                        member,
                        member_span,
                    ));
                }
                if matches!(self.peek().kind, TokKind::LParen) {
                    let call = self.call_after_name(type_name, span)?;
                    return Ok(Expr::Call(call));
                }
                Ok(Expr::Ident(type_name, span))
            }
            other => Err(Diagnostic::error(
                "E0003",
                format!("expected a value, found {}", describe(&other)),
                "a value can be a name, a number, quoted text, `true`/`false`, or a call"
                    .to_string(),
                "e.g. `x`, `42`, `3.5`, or `\"hello\"`".to_string(),
                Some(self.peek().span),
            )),
        }
    }

    /// S37/S38: `[a, b]` or `["k": v]` or `[]` / `[:]`.
    fn list_or_map_lit(&mut self) -> Result<Expr, Diagnostic> {
        let open = self.bump().span;
        if matches!(self.peek().kind, TokKind::RBracket) {
            let close = self.bump().span;
            return Ok(Expr::ListLit(Vec::new(), Span::new(open.start, close.end)));
        }
        if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            self.expect(TokKind::RBracket, "after `[:]`")?;
            let close = self.toks[self.pos - 1].span;
            return Ok(Expr::MapLit(Vec::new(), Span::new(open.start, close.end)));
        }
        let first = self.expr()?;
        if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            let value = self.expr()?;
            let mut entries = vec![(first, value)];
            while matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                self.bump();
                if matches!(self.peek().kind, TokKind::RBracket) {
                    break;
                }
                let key = self.expr()?;
                self.expect(TokKind::Colon, "between a map key and its value")?;
                let val = self.expr()?;
                entries.push((key, val));
            }
            self.expect(TokKind::RBracket, "to close the map literal")?;
            let close = self.toks[self.pos - 1].span;
            return Ok(Expr::MapLit(entries, Span::new(open.start, close.end)));
        }
        let mut elems = vec![first];
        while matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
            self.bump();
            if matches!(self.peek().kind, TokKind::RBracket) {
                break;
            }
            elems.push(self.expr()?);
        }
        self.expect(TokKind::RBracket, "to close the list literal")?;
        let close = self.toks[self.pos - 1].span;
        Ok(Expr::ListLit(elems, Span::new(open.start, close.end)))
    }

    fn struct_lit_after_name(
        &mut self,
        type_name: String,
        type_args: Vec<Type>,
        start_span: Span,
    ) -> Result<Expr, Diagnostic> {
        self.expect(TokKind::LBrace, "to open a struct literal")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            let (field, field_span) = self.expect_ident("for a field name")?;
            let value = if matches!(self.peek().kind, TokKind::Colon) {
                self.bump();
                self.expr()?
            } else {
                // S77: field punning — `{ name }` means `{ name: name }`.
                Expr::Ident(field.clone(), field_span)
            };
            fields.push((field, field_span, value));
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.bump();
        Ok(Expr::StructLit {
            type_name,
            type_args,
            import_ns: None,
            as_trait: None,
            fields,
            span: Span::new(start_span.start, end),
        })
    }

    fn struct_lit_after_import(
        &mut self,
        alias: String,
        type_name: String,
        start: usize,
    ) -> Result<Expr, Diagnostic> {
        self.expect(TokKind::LBrace, "to open a struct literal")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::Semi) { self.bump(); continue; }
            let (field, field_span) = self.expect_ident("for a field name")?;
            let value = if matches!(self.peek().kind, TokKind::Colon) {
                self.bump();
                self.expr()?
            } else {
                // S77: field punning — `{ name }` means `{ name: name }`.
                Expr::Ident(field.clone(), field_span)
            };
            fields.push((field, field_span, value));
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.bump();
        Ok(Expr::StructLit {
            type_name,
            type_args: Vec::new(),
            import_ns: Some(alias),
            as_trait: None,
            fields,
            span: Span::new(start, end),
        })
    }

    /// S31: try to parse a pattern on the right of `==`.
    ///
    /// Only unambiguous pattern spellings: `null`, `value(n)`, and
    /// `Variant(bindings)`. A bare identifier is ordinary value equality
    /// (`a == b`); unit-variant tests like `light == Red` are resolved in
    /// sema when `Red` is not a variable but is a variant on the subject.
    fn try_pattern_rhs(&mut self) -> Result<Option<Pattern>, Diagnostic> {
        match &self.peek().kind {
            TokKind::KwNull => {
                let span = self.bump().span;
                return Ok(Some(Pattern::Absent(span)));
            }
            TokKind::KwOk => {
                let start = self.bump().span;
                self.expect(TokKind::LParen, "after `ok`")?;
                let (binding, binding_span) = self.expect_ident("inside `ok(...)`")?;
                self.expect(TokKind::RParen, "after the binding in `ok(...)`")?;
                return Ok(Some(Pattern::Ok {
                    binding,
                    span: Span::new(start.start, binding_span.end),
                }));
            }
            TokKind::KwErr => {
                let start = self.bump().span;
                self.expect(TokKind::LParen, "after `err`")?;
                let (binding, binding_span) = self.expect_ident("inside `err(...)`")?;
                self.expect(TokKind::RParen, "after the binding in `err(...)`")?;
                return Ok(Some(Pattern::Err {
                    binding,
                    span: Span::new(start.start, binding_span.end),
                }));
            }
            TokKind::Ident(name) if name == Syntax::LIT_VALUE => {
                let start = self.bump().span;
                self.expect(TokKind::LParen, "after `value`")?;
                let (binding, binding_span) = self.expect_ident("inside `value(...)`")?;
                self.expect(TokKind::RParen, "after the binding in `value(...)`")?;
                return Ok(Some(Pattern::Present {
                    binding,
                    span: Span::new(start.start, binding_span.end),
                }));
            }
            TokKind::Ident(variant)
                if matches!(
                    self.toks.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokKind::LParen)
                ) =>
            {
                let variant = variant.clone();
                let span = self.peek().span;
                self.bump();
                self.bump();
                let mut bindings: Vec<crate::AST::PatSlot> = Vec::new();
                if !matches!(self.peek().kind, TokKind::RParen) {
                    loop {
                        // D-PATW: `_` in payload slot = wildcard (ignore field, bind nothing).
                        let slot = if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::PAT_WILDCARD_SLOT) {
                            self.bump();
                            crate::AST::PatSlot::Wildcard
                        } else if let TokKind::Int(lo_val) = &self.peek().kind.clone() {
                            // D-PATR: `lo..hi` range in payload slot.
                            let lo = *lo_val;
                            self.bump();
                            if matches!(self.peek().kind, TokKind::DotDot) {
                                self.bump(); // consume `..`
                                if let TokKind::Int(hi_val) = &self.peek().kind.clone() {
                                    let hi = *hi_val;
                                    self.bump();
                                    crate::AST::PatSlot::Range { lo, hi }
                                } else {
                                    return Err(Diagnostic::error(
                                        "E0003",
                                        "expected an integer after `..` in a range pattern".to_string(),
                                        "range patterns need both ends: `lo..hi`".to_string(),
                                        "write `0..100` for an inclusive range".to_string(),
                                        Some(self.peek().span),
                                    ));
                                }
                            } else {
                                return Err(Diagnostic::error(
                                    "E0003",
                                    "expected `..` after the lower bound of a range pattern".to_string(),
                                    "range patterns need `lo..hi` syntax".to_string(),
                                    "write `0..100` for an inclusive range".to_string(),
                                    Some(self.peek().span),
                                ));
                            }
                        } else {
                            let (b, _) = self.expect_ident("for a pattern binding")?;
                            crate::AST::PatSlot::Bind(b)
                        };
                        bindings.push(slot);
                        if matches!(self.peek().kind, TokKind::RParen) {
                            break;
                        }
                        self.expect(TokKind::Comma, "between pattern bindings")?;
                    }
                }
                self.expect(TokKind::RParen, "after pattern bindings")?;
                let end = self.toks[self.pos.saturating_sub(1)].span.end;
                // D-PATO: check for `| AltVariant(bindings)` after this pattern.
                let base = Pattern::Variant {
                    variant,
                    bindings,
                    span: Span::new(span.start, end),
                };
                if matches!(self.peek().kind, TokKind::Pipe) {
                    // Collect or-pattern alternatives.
                    let or_start = span;
                    let mut alts = vec![base];
                    while matches!(self.peek().kind, TokKind::Pipe) {
                        self.bump(); // consume `|`
                        // Parse the next alternative (must be a Variant pattern).
                        if let Some(alt) = self.try_pattern_rhs()? {
                            alts.push(alt);
                        } else {
                            return Err(Diagnostic::error(
                                "E0003",
                                "expected a variant pattern after `|` in an or-pattern".to_string(),
                                "or-patterns join two variant patterns: `A(x) | B(x)`".to_string(),
                                "write a variant name with bindings after `|`".to_string(),
                                Some(self.peek().span),
                            ));
                        }
                    }
                    let or_end = self.toks[self.pos.saturating_sub(1)].span.end;
                    return Ok(Some(Pattern::Or(alts, Span::new(or_start.start, or_end))));
                }
                return Ok(Some(base));
            }
            _ => Ok(None),
        }
    }

    /// S46: `(` … `) =>` without scanning nested `(` for the `=>` probe.
    fn after_lparen_is_lambda(&self) -> bool {
        let mut i = self.pos + 1;
        let mut depth = 1usize;
        while i < self.toks.len() {
            match &self.toks[i].kind {
                TokKind::LParen => depth += 1,
                TokKind::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return matches!(
                            self.toks.get(i + 1).map(|t| &t.kind),
                            Some(TokKind::LambdaArrow)
                        );
                    }
                }
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// S47: `take(a, b)` prefix on a lambda.
    fn parse_lambda_takes(&mut self) -> Result<Vec<(String, Span)>, Diagnostic> {
        self.expect(TokKind::KwMove, "before the capture list")?;
        self.expect(TokKind::LParen, "after `take` in a capture list")?;
        let mut names = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (name, span) = self.expect_ident("in the capture list")?;
                names.push((name, span));
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between captured names")?;
            }
        }
        self.expect(TokKind::RParen, "after the capture list")?;
        Ok(names)
    }

    fn parse_lambda(&mut self, take_names: Vec<(String, Span)>) -> Result<Lambda, Diagnostic> {
        let open = self.peek().span;
        self.expect(TokKind::LParen, "before lambda parameters")?;
        let mut params = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                let (name, name_span) = self.expect_ident("as a lambda parameter")?;
                let (ty, ty_span) = if matches!(self.peek().kind, TokKind::Colon) {
                    self.bump();
                    let (t, ts) = self.type_()?;
                    (Some(t), Some(ts))
                } else {
                    (None, None)
                };
                params.push(LambdaParam {
                    name,
                    name_span,
                    ty,
                    ty_span,
                });
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between lambda parameters")?;
            }
        }
        let close_paren = self.peek().span;
        self.expect(TokKind::RParen, "after lambda parameters")?;
        self.expect(TokKind::LambdaArrow, "after `)` in a lambda")?;
        let body = if matches!(self.peek().kind, TokKind::LBrace) {
            self.expect(TokKind::LBrace, "to open the lambda body")?;
            LambdaBody::Block(self.block_stmts())
        } else {
            LambdaBody::Expr(Box::new(self.expr()?))
        };
        let end = match &body {
            LambdaBody::Expr(e) => e.span().end,
            LambdaBody::Block(stmts) => {
                if let Some(last) = stmts.last() {
                    match last {
                        Stmt::Expr(e) => e.span().end,
                        Stmt::Return(_, s) => s.end,
                        Stmt::Break(s) | Stmt::Continue(s) | Stmt::BreakLabel(_, s) | Stmt::ContinueLabel(_, s) => s.end,
                        Stmt::If(i) => i.span.end,
                        Stmt::While { span, .. }
                        | Stmt::For { span, .. }
                        | Stmt::Switch { span, .. } => span.end,
                        Stmt::Val(b) => b.init.span().end,
                        Stmt::Assign { value, .. } => value.span().end,
                        Stmt::Loop { span: s, .. } => s.end,
                        Stmt::Unsafe { span, .. } => span.end,
                        Stmt::Region { span, .. } => span.end,
                        Stmt::Caps { span, .. } => span.end,
                        Stmt::ComptimeIf { span, .. } => span.end,
                        Stmt::ContextBlock { span, .. } => span.end,
                        // D-TERM1 (ratified 2026-06-22): live block span end.
                        Stmt::Live { span, .. } => span.end,
                    }
                } else {
                    close_paren.end
                }
            }
        };
        Ok(Lambda {
            take_names,
            params,
            body,
            span: Span::new(open.start, end),
            meta: LambdaMeta::default(),
        })
    }

    fn call_after_name(&mut self, name: String, name_span: Span) -> Result<Call, Diagnostic> {
        self.expect(TokKind::LParen, &format!("after `{}` to call it", name))?;
        let mut args = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                args.push(self.call_arg()?);
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between arguments")?;
            }
        }
        self.expect(TokKind::RParen, "to finish the call")?;
        Ok(Call {
            name,
            name_span,
            args,
        })
    }

    fn call_arg(&mut self) -> Result<CallArg, Diagnostic> {
        // D-CAP8: capability *inference* is for parameter definitions, not call sites.
        // An unmarked argument is a plain read at the call site (the caller isn't
        // requesting a stronger capability), so normalize `Infer` → `Read` here. Only
        // parameters carry `Infer` into Sema::Capability for resolution.
        let convention = match self.parse_access_prefix() {
            AccessConvention::Infer => AccessConvention::Read,
            c => c,
        };
        let span = self.peek().span;
        // S61: detect `name: expr` label at call site — an ident followed by `:` that is
        // NOT `::` (a Rust path). We must not consume it yet if it is just a variable name.
        let label = if matches!(self.peek().kind, TokKind::Ident(_))
            && matches!(self.peek2().kind, TokKind::Colon)
        {
            let lbl_tok = self.bump();
            let lbl_name = match lbl_tok.kind {
                TokKind::Ident(n) => n,
                _ => unreachable!(),
            };
            self.bump(); // consume `:`
            Some((lbl_name, lbl_tok.span))
        } else {
            None
        };
        let expr = self.expr()?;
        Ok(CallArg {
            convention,
            expr,
            span,
            flags: Default::default(),
            label,
        })
    }

    /// D-CAP7: consume a leading capability sigil `~`/`^`/`&` → Write/Move/Share.
    /// Returns `None` when no sigil is present. Position-disambiguated: infix `^`
    /// (xor) and `&` (BitAnd) are parsed inside expressions and never reach the
    /// start of a parameter/argument or a type. `*` (raw) is D-CAP9, handled apart.
    pub(super) fn parse_capability_sigil(&mut self) -> Option<AccessConvention> {
        let cap = match self.peek().kind {
            TokKind::Tilde => AccessConvention::Write,
            TokKind::Caret => AccessConvention::Move,
            TokKind::Amp => AccessConvention::Share,
            _ => return None,
        };
        self.bump();
        Some(cap)
    }

    pub(super) fn parse_access_prefix(&mut self) -> AccessConvention {
        // D-CAP7 sigils take precedence over the (migrating) keyword forms.
        if let Some(cap) = self.parse_capability_sigil() {
            return cap;
        }
        if let TokKind::Ident(name) = self.peek().kind.clone() {
            match name.as_str() {
                Syntax::FOREIGN_READ => {
                    let span = self.peek().span;
                    self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0017",
                        format!(
                            "shared access is written with no word in front — not `{}`",
                            Syntax::FOREIGN_READ
                        ),
                        "Jet has exactly one spelling for each thing, so all code reads the same"
                            .to_string(),
                        format!("remove `{}` and write `name: Type`", Syntax::FOREIGN_READ),
                        Some(span),
                    ));
                    return AccessConvention::Read;
                }
                Syntax::FOREIGN_WRITE => {
                    let span = self.peek().span;
                    self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0018",
                        format!(
                            "changeable access is written `{}`, not `{}`",
                            Syntax::KW_MUTATE,
                            Syntax::FOREIGN_WRITE
                        ),
                        "Jet has exactly one spelling for each thing, so all code reads the same"
                            .to_string(),
                        format!(
                            "replace `{}` with `{}`",
                            Syntax::FOREIGN_WRITE,
                            Syntax::KW_MUTATE
                        ),
                        Some(span),
                    ));
                    return AccessConvention::Write;
                }
                _ => {}
            }
        }
        match self.peek().kind {
            TokKind::KwMutate => {
                self.bump();
                AccessConvention::Write
            }
            TokKind::KwMove => {
                // `take(names) () =>` is a lambda take-prefix, not an arg convention.
                // Only consume `take` as an arg convention when NOT followed by `(`.
                let is_lambda_take = matches!(
                    self.toks.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokKind::LParen)
                );
                if is_lambda_take {
                    // `take(names)` lambda prefix is not a capability marker — the value
                    // here is unmarked, so it infers (D-CAP8).
                    AccessConvention::Infer
                } else {
                    self.bump();
                    AccessConvention::Move
                }
            }
            // D-CAP8 (= C): an unmarked parameter/argument starts as `Infer` and is
            // resolved from body usage by Sema::Capability before checks/codegen.
            _ => AccessConvention::Infer,
        }
    }

    fn starts_expr(&self, kind: &TokKind) -> bool {
        matches!(
            kind,
            TokKind::Ident(_)
                | TokKind::Int(_)
                | TokKind::Float(_)
                | TokKind::Str(_)
                | TokKind::KwTrue
                | TokKind::KwFalse
                | TokKind::KwNull
                | TokKind::KwOk
                | TokKind::KwErr
                | TokKind::KwIt
                | TokKind::LParen
                | TokKind::Minus
                | TokKind::Bang
        )
    }

    fn foreign_logic_error(&mut self, foreign: &str, canonical: &str) {
        self.diags.push(Diagnostic::error(
            "E0012",
            format!(
                "{} writes \"{}\" as `{}`",
                Syntax::LANG_NAME,
                foreign,
                canonical
            ),
            format!(
                "logic uses the symbols `{}`, `{}`, and `{}`",
                Syntax::OP_AND,
                Syntax::OP_OR,
                Syntax::OP_NOT
            ),
            format!("replace `{}` with `{}`", foreign, canonical),
            Some(self.peek().span),
        ));
    }

}
