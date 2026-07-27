use super::super::{
    BinOp, Diagnostic, EnumLitArg, Expr, OrFallback, Parser, Span, Syntax, TokKind, UnOp,
    pat_span, retired_s14_teaching_enabled,
};

fn write_window_at_maximal_place(expr: Expr, start: usize) -> Expr {
    match expr {
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            type_args,
            args,
            recv_type,
            resolved_ret,
        } => Expr::MethodCall {
            receiver: Box::new(write_window_at_maximal_place(*receiver, start)),
            method,
            method_span,
            type_args,
            args,
            recv_type,
            resolved_ret,
        },
        place => {
            let span = Span::new(start, place.span().end);
            Expr::Place(Box::new(place), crate::AST::PlaceAccess::Write, span)
        }
    }
}

fn leading_dot_variant(kind: &TokKind) -> Option<String> {
    match kind {
        TokKind::Ident(name) if name.chars().next().is_some_and(char::is_uppercase) => {
            Some(name.clone())
        }
        TokKind::KwNull => Some(Syntax::LIT_NULL.to_string()),
        _ => None,
    }
}

impl<'a> Parser<'a> {
        /// S35/S71: the `??` fallback binds looser than `&&` / `||`.
        pub(super) fn expr_or_fallback(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
            let mut lhs = self.expr_or(allow_struct_lit)?;
            loop {
                match &self.peek().kind {
                    TokKind::QuestionQuestion => {}
                    // S71 (D-SG6): the retired word `or` — teach `??`, then recover.
                    TokKind::Ident(n)
                        if retired_s14_teaching_enabled() && n == Syntax::FOREIGN_OR_FALLBACK =>
                    {
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
                    OrFallback::Break(s)
                    | OrFallback::Continue(s)
                    | OrFallback::BreakLabel(_, s)
                    | OrFallback::ContinueLabel(_, s) => s.end,
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
            if matches!(self.peek().kind, TokKind::LBrace) {
                return Err(Diagnostic::error(
                    "E0003",
                    "a `??` fallback cannot be a bare block".to_string(),
                    "`??` accepts one fallback value or a control action, not a block with several steps"
                        .to_string(),
                    "write `value ?? fallback`, `value ?? return`, or `value ?? make_fallback()`"
                        .to_string(),
                    Some(self.peek().span),
                ));
            }
            if matches!(self.peek().kind, TokKind::KwReturn) {
                let span = self.bump().span;
                if self.starts_expr(&self.peek().kind) {
                    let e = self.expr_or(allow_struct_lit)?;
                    return Ok(OrFallback::Return(Some(Box::new(e)), span));
                }
                return Ok(OrFallback::Return(None, span));
            }
            // D-LOOP-CONTROLWORD1=B: bare `?? next` is contextual control;
            // `?? (next)` remains an ordinary value fallback.
            if matches!(self.peek().kind, TokKind::KwBreak) {
                if matches!(self.peek2().kind, TokKind::LParen) {
                    let start = self.bump().span;
                    self.bump(); // `(`
                    let (name, _) = self.expect_ident("as the loop name in `break(name)`")?;
                    let end = self.peek().span.end;
                    self.expect(TokKind::RParen, "after the named break target")?;
                    return Ok(OrFallback::BreakLabel(name, Span::new(start.start, end)));
                }
                return Ok(OrFallback::Break(self.bump().span));
            }
            if matches!(&self.peek().kind, TokKind::Ident(n) if n == Syntax::KW_NEXT)
                && (!matches!(self.peek2().kind, TokKind::LParen)
                    || matches!(self.peek3().kind, TokKind::Ident(_))
                        && matches!(self.peek4().kind, TokKind::RParen))
            {
                if matches!(self.peek2().kind, TokKind::LParen) {
                    let start = self.bump().span;
                    self.bump(); // `(`
                    let (name, _) = self.expect_ident("as the loop name in `next(name)`")?;
                    let end = self.peek().span.end;
                    self.expect(TokKind::RParen, "after the named next target")?;
                    return Ok(OrFallback::ContinueLabel(name, Span::new(start.start, end)));
                }
                return Ok(OrFallback::Continue(self.bump().span));
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
                    TokKind::Ident(n) if retired_s14_teaching_enabled() && n == Syntax::FOREIGN_AND => {
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
    
        /// D-CHAINCMP1: a run of same-direction relational operators (`<`/`<=`/
        /// `>`/`>=`) chains into `Expr::CompareChain`, any length. `==`/`!=` never
        /// chain (with each other or with a relational op) — `a == b == c` and
        /// `a < b == c` both stay the pre-existing "comparisons can't be chained"
        /// error (E0003). A mixed-direction relational chain (`a < b > c`) is
        /// E0333, naming the direction break.
        pub(in crate::Parser) fn expr_cmp(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
            let lhs = self.expr_bitxor(allow_struct_lit)?;
            let op = match &self.peek().kind {
                TokKind::EqEq => Some(BinOp::Eq),
                TokKind::NotEq => Some(BinOp::Ne),
                TokKind::Lt => Some(BinOp::Lt),
                TokKind::Gt if self.module_arg_expr_depth != Some(self.depth) => Some(BinOp::Gt),
                TokKind::Le => Some(BinOp::Le),
                TokKind::Ge => Some(BinOp::Ge),
                _ => None,
            };
            let Some(op) = op else { return Ok(lhs) };
            let op_span = self.bump().span;
            if op == BinOp::Eq {
                if let Some(pat) = self.try_pattern_rhs()? {
                    let span = Span::new(lhs.span().start, pat_span(&pat).end.max(op_span.end));
                    return Ok(Expr::PatternTest {
                        subject: Box::new(lhs),
                        pattern: pat,
                        span,
                    });
                }
            }
            let rhs = self.expr_bitxor(allow_struct_lit)?;
    
            // `==`/`!=` never chain — D-CHAINCMP1 excludes them. Reproduce the
            // pre-existing behavior exactly: any further relational/equality
            // token after an `==`/`!=` pair is E0003.
            if op == BinOp::Eq || op == BinOp::Ne {
                let span = Span::new(lhs.span().start, rhs.span().end.max(op_span.end));
                let cmp = Expr::Binary(op, Box::new(lhs), Box::new(rhs), span);
                if let Some(second) = self.peek_cmp_span() {
                    return Err(self.chained_eq_error(second));
                }
                return Ok(cmp);
            }
    
            // Relational op: collect a same-direction chain.
            let ascending = matches!(op, BinOp::Lt | BinOp::Le);
            let mut operands = vec![lhs, rhs];
            let mut ops = vec![op];
            loop {
                let next = match &self.peek().kind {
                    TokKind::Lt => Some(BinOp::Lt),
                    TokKind::Le => Some(BinOp::Le),
                    TokKind::Gt => Some(BinOp::Gt),
                    TokKind::Ge => Some(BinOp::Ge),
                    TokKind::EqEq | TokKind::NotEq => {
                        // `==`/`!=` can't extend a relational chain either.
                        let bad_span = self.peek().span;
                        return Err(self.chained_eq_error(bad_span));
                    }
                    _ => None,
                };
                let Some(next_op) = next else { break };
                let next_ascending = matches!(next_op, BinOp::Lt | BinOp::Le);
                if next_ascending != ascending {
                    let bad_span = self.peek().span;
                    return Err(Diagnostic::error(
                        "E0333",
                        "this comparison chain changes direction".to_string(),
                        "a chain like `0 <= sev < 10` reads in one direction; mixing `<` and `>` in one chain is almost always a mistake and has no single meaning".to_string(),
                        "split it into two comparisons joined with `&&`".to_string(),
                        Some(bad_span),
                    ));
                }
                self.bump();
                let next_rhs = self.expr_bitxor(allow_struct_lit)?;
                operands.push(next_rhs);
                ops.push(next_op);
            }
    
            let span = Span::new(
                operands.first().unwrap().span().start,
                operands.last().unwrap().span().end,
            );
            if ops.len() == 1 {
                let rhs = operands.pop().unwrap();
                let lhs = operands.pop().unwrap();
                return Ok(Expr::Binary(ops[0], Box::new(lhs), Box::new(rhs), span));
            }
            Ok(Expr::CompareChain {
                hooks: vec![false; ops.len()],
                operands,
                ops,
                span,
            })
        }
    
        fn peek_cmp_span(&self) -> Option<Span> {
            match &self.peek().kind {
                TokKind::EqEq
                | TokKind::NotEq
                | TokKind::Lt
                | TokKind::Gt
                | TokKind::Le
                | TokKind::Ge => Some(self.peek().span),
                _ => None,
            }
        }
    
        fn chained_eq_error(&self, second: Span) -> Diagnostic {
            Diagnostic::error(
                "E0003",
                "comparisons can't be chained".to_string(),
                format!(
                    "`a < b < c` doesn't compare all three; check each pair and join with `{}`",
                    Syntax::OP_AND
                ),
                format!("write `a < b {} b < c`", Syntax::OP_AND),
                Some(second),
            )
        }
    
        pub(in crate::Parser) fn expr_bitxor(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
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
                // D-INCR1: prefix `++` / `--` on a mutable integer lvalue.
                TokKind::PlusPlus | TokKind::MinusMinus => {
                    let op_tok = self.bump();
                    let op = match op_tok.kind {
                        TokKind::PlusPlus => crate::AST::IncDecOp::Inc,
                        TokKind::MinusMinus => crate::AST::IncDecOp::Dec,
                        _ => unreachable!(),
                    };
                    let inner = self.expr_unary(allow_struct_lit)?;
                    let full = Span::new(op_tok.span.start, inner.span().end);
                    Ok(Expr::IncDec {
                        op,
                        operand: Box::new(inner),
                        postfix: false,
                        span: full,
                    })
                }
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
                    if retired_s14_teaching_enabled()
                        && n == Syntax::FOREIGN_NOT
                        && self.starts_expr(&self.peek2().kind) =>
                {
                    self.foreign_logic_error(Syntax::FOREIGN_NOT, Syntax::OP_NOT);
                    let span = self.bump().span;
                    let inner = self.expr_unary(allow_struct_lit)?;
                    let full = Span::new(span.start, inner.span().end);
                    Ok(Expr::Unary(UnOp::Not, Box::new(inner), full))
                }
                TokKind::Ident(n)
                    if retired_s14_teaching_enabled()
                        && n == Syntax::FOREIGN_TRY
                        && self.starts_expr(&self.peek2().kind) =>
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
                    // D-CAP9: prefix `*x` is raw-pointer-of (take a raw pointer to
                    // `x`), gated to `#Unsafe`. Dereference moved to postfix `p.*`.
                    let span = self.bump().span;
                    let inner = self.expr_unary(allow_struct_lit)?;
                    let full = Span::new(span.start, inner.span().end);
                    Ok(Expr::RawOf(Box::new(inner), full))
                }
                // D-SHAPE-PLACE1=A: `&place` is an expression-level exclusive
                // write window. It is distinct from call-argument convention;
                // sema proves the operand is a maximal place.
                TokKind::Amp => {
                    let span = self.bump().span;
                    let inner = self.expr_unary(allow_struct_lit)?;
                    Ok(write_window_at_maximal_place(inner, span.start))
                }
                // D-SHAPE-COPY1=A: `~x` — the one copy sigil, a prefix-verb
                // expression form. Legal on any expression; most useful on a named
                // binding. `.clone()` is not user-typable Jet syntax (I8).
                TokKind::Tilde => {
                    let span = self.bump().span;
                    let inner = self.expr_unary(allow_struct_lit)?;
                    let full = Span::new(span.start, inner.span().end);
                    Ok(Expr::Copy(Box::new(inner), full))
                }
                // D-SHAPE-COPY1=A: the `copy` word is retired — copy is now the
                // `~` sigil (was D-CAP2/S4). Teach, then recover as Expr::Copy so
                // parsing continues.
                TokKind::KwCopy => {
                    let span = self.bump().span;
                    self.diags.push(Diagnostic::error(
                        "E0991",
                        format!("`{}` is now the `{}` sigil", Syntax::KW_COPY, Syntax::SIGIL_COPY),
                        format!(
                            "Jet has exactly one spelling for a copy — the `{}` sigil \
                             (D-SHAPE-COPY1) — so all code reads the same",
                            Syntax::SIGIL_COPY
                        ),
                        format!(
                            "write `{}name` in place of `{} name`",
                            Syntax::SIGIL_COPY,
                            Syntax::KW_COPY
                        ),
                        Some(span),
                    ));
                    let inner = self.expr_unary(allow_struct_lit)?;
                    let full = Span::new(span.start, inner.span().end);
                    Ok(Expr::Copy(Box::new(inner), full))
                }
                // D-DOTCTOR1: `.{ … }` inferred struct literal (type from context).
                // A leading `.` immediately followed by `{` is unambiguous — it is not
                // valid as a field access (no receiver) or any other production.
                TokKind::Dot if allow_struct_lit && matches!(self.peek2().kind, TokKind::LBrace) => {
                    let dot_start = self.bump().span.start; // consume `.`
                    self.struct_lit_inferred(dot_start)
                }
                // D-SHAPE3a=A: `.new(...)` leaves only the static receiver implicit.
                // Sema fills the empty, unspellable identifier from the ordinary
                // expected type; downstream lowering then sees the same MethodCall as
                // an explicit `Type.new(...)`.
                TokKind::Dot if matches!(&self.peek2().kind, TokKind::Ident(n) if n == Syntax::MEM_ALLOC_NEW) => {
                    let dot = self.bump().span;
                    let (method, method_span) = self.expect_field_name()?;
                    self.expect(TokKind::LParen, "after `.new`")?;
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
                    Ok(Expr::MethodCall {
                        receiver: Box::new(Expr::Ident(String::new(), dot)),
                        method,
                        method_span,
                        type_args: Vec::new(),
                        args,
                        recv_type: None,
                        resolved_ret: None,
                    })
                }
                // D-ENUMDOT2=A: `.Variant`, `.Variant(arg)`, or `.Variant.{ field: val }` in
                // value position. An uppercase ident after `.` with no receiver is a
                // leading-dot enum literal. type_name="" is the unresolved sentinel;
                // sema fills it in via expected_type.
                TokKind::Dot if leading_dot_variant(&self.peek2().kind).is_some() =>
                {
                    let dot_start = self.bump().span.start; // consume `.`
                    let variant_token = self.bump();
                    let mut variant = leading_dot_variant(&variant_token.kind)
                        .expect("leading-dot variant guard and token must agree");
                    let mut variant_span = variant_token.span;
                    // D-TAG1: a dotted leaf path (`.Fire.Burn`) — further uppercase
                    // segments extend the variant path. `.{` never matches (its second
                    // token is `{`, not an ident), so named-payload construction below
                    // still sees its `.` intact.
                    while matches!(self.peek().kind, TokKind::Dot)
                        && matches!(&self.peek2().kind, TokKind::Ident(n) if n.chars().next().map_or(false, |c| c.is_uppercase()))
                    {
                        self.bump(); // consume `.`
                        let (seg, seg_span) =
                            self.expect_ident("after `.` in a leading-dot enum variant")?;
                        variant = format!("{variant}.{seg}");
                        variant_span = seg_span;
                    }
                    // D-UITREE1/D-DOTCTOR1: `.Variant.{ field: val, … }` — named-payload
                    // construction reuses the struct dot-brace spelling (one leading-dot
                    // rule for every inferred construction, structs and enums alike).
                    let (args, end) = if allow_struct_lit
                        && matches!(self.peek().kind, TokKind::Dot)
                        && matches!(self.peek2().kind, TokKind::LBrace)
                    {
                        self.bump(); // consume `.`
                        self.enum_lit_named_fields()?
                    } else if matches!(self.peek().kind, TokKind::LParen) {
                        self.bump(); // consume `(`
                        let mut args = Vec::new();
                        if !matches!(self.peek().kind, TokKind::RParen) {
                            loop {
                                let e = self.expr()?;
                                args.push(EnumLitArg::Positional(e));
                                if matches!(self.peek().kind, TokKind::RParen) {
                                    break;
                                }
                                self.expect(TokKind::Comma, "between enum variant arguments")?;
                            }
                        }
                        self.expect(TokKind::RParen, "to close the enum variant arguments")?;
                        let close_end = self.toks[self.pos - 1].span.end;
                        (args, close_end)
                    } else {
                        (Vec::new(), variant_span.end)
                    };
                    Ok(Expr::EnumLit {
                        type_name: String::new(),
                        variant,
                        args,
                        span: Span::new(dot_start, end),
                    })
                }
                _ => self.expr_postfix(allow_struct_lit),
            }
        }
    
}
