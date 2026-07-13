use super::*;
use crate::AST::{
    AccessConvention, Call, CallArg, EnumLitArg, Expr, OrFallback, Pattern, StrPart, Type, UnOp,
};

impl<'a> Fmt<'a> {
    /// D-FMT1: render an `if`-expression chain with a shared `inline` decision.
    /// `expr` must be `Expr::If`.
    fn fmt_if_expr(&mut self, expr: &Expr, inline: bool) {
        let Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } = expr
        else {
            unreachable!("fmt_if_expr called on non-if expression");
        };
        self.write("if ");
        self.fmt_cond(cond);
        self.write(" ");
        self.fmt_value_block(then_body, then_value, inline);
        self.write(" else ");
        if else_body.is_empty() && matches!(else_value.as_ref(), Expr::If { .. }) {
            self.fmt_if_expr(else_value, inline);
        } else {
            self.fmt_value_block(else_body, else_value, inline);
        }
    }

    /// D-FMT1: every branch of an `if`-expression chain is inline-eligible (gates
    /// a–d, author wrote each on one line). A nested `else if` recurses.
    fn if_expr_chain_inlineable(&self, expr: &Expr) -> bool {
        let Expr::If {
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } = expr
        else {
            return false;
        };
        if !self.value_block_inlineable(then_body, then_value) {
            return false;
        }
        if else_body.is_empty() && matches!(else_value.as_ref(), Expr::If { .. }) {
            self.if_expr_chain_inlineable(else_value)
        } else {
            self.value_block_inlineable(else_body, else_value)
        }
    }

    pub(super) fn fmt_type(&mut self, ty: &Type) {
        match ty {
            Type::Int => self.write(Syntax::TYPE_INT),
            Type::Float => self.write(Syntax::TYPE_FLOAT),
            Type::IntN { signed, bits } => self.write(&crate::AST::int_spelling(*signed, *bits)),
            Type::Float32 => self.write("F32"),
            Type::Bool => self.write(Syntax::TYPE_BOOL),
            Type::String => self.write(Syntax::TYPE_STRING),
            Type::Char => self.write(Syntax::TYPE_CHAR),
            Type::List(inner) => {
                self.write("[");
                self.fmt_type(inner);
                self.write("]");
            }
            Type::Map { key, value, .. } => {
                self.write("[");
                self.fmt_type(key);
                self.write(": ");
                self.fmt_type(value);
                self.write("]");
            }
            Type::Shared(inner) => {
                self.write("Shared<");
                self.fmt_type(inner);
                self.write(">");
            }
            Type::Option(inner) => {
                self.fmt_type(inner);
                self.write("?");
            }
            Type::Result { ok, err } => {
                self.fmt_type(ok);
                self.write(" ? ");
                self.fmt_type(err);
            }
            Type::Fn {
                params,
                ret,
                effect_bound,
            } => {
                // D-EFF2: render the callback effect bound prefix — `@Pure ` for an
                // empty bound, `#(E1, E2) ` for a listed one. `@Pure` is a
                // contract marker (`@`-plane, D-MARKER-FAMILY1) — it was
                // being written with the `#` directive-plane prefix instead,
                // silently downgrading ratified `@Pure` callback bounds back
                // to the retired `#Pure` spelling on every fmt pass (a real
                // regression caught reformatting examples/features/effects/effect_levers.jet).
                if let Some(bound) = effect_bound {
                    if bound.is_empty() {
                        self.write(crate::Syntax::CONTRACT_PREFIX);
                        self.write(crate::Syntax::KW_PURE);
                        self.write(" ");
                    } else {
                        self.write("#(");
                        for (i, (name, _)) in bound.iter().enumerate() {
                            if i > 0 {
                                self.write(", ");
                            }
                            self.write(name);
                        }
                        self.write(") ");
                    }
                }
                self.write("fn(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_type(p);
                }
                self.write(")");
                if let Some(r) = ret {
                    self.write(" -> ");
                    self.fmt_type(r);
                }
            }
            Type::Named(n) => self.write(n),
            // D-CAP9: the raw-pointer type renders as the canonical `*T`, never
            // the deprecated `Ptr<T>` alias.
            Type::Apply { name, args } if name == Syntax::TYPE_PTR && args.len() == 1 => {
                self.write("*");
                self.fmt_type(&args[0]);
            }
            Type::Apply { name, args } => {
                self.write(name);
                self.write("<");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_type(a);
                }
                self.write(">");
            }
            Type::TraitObject(t) => {
                // Only the parser's `dyn`/`Box<dyn>` teaching-error recovery paths
                // ever construct this AST-facing arm with more than a formatting
                // concern in mind, and those are always single-name; join
                // defensively rather than assume (never emit "" silently, I2).
                self.write(&t.join(" + "));
            }
            Type::Tuple(fields) => {
                self.write("(");
                for (i, (name, ty)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(name);
                    self.write(": ");
                    self.fmt_type(ty);
                }
                self.write(")");
            }
            Type::FixedList { elem, len, len_symbol } => {
                self.write("[");
                self.fmt_type(elem);
                self.write(&format!("#{}", len_symbol.as_ref().map(|v| v.0.as_str()).map_or_else(|| len.to_string(), str::to_string)));
                self.write("]");
            }
            // D-QUAL4=A: `#Marker Type` — prefix value-tag.
            Type::Tagged { marker, inner } => {
                self.write("#");
                self.write(marker);
                self.write(" ");
                self.fmt_type(inner);
            }
        }
    }

    pub(super) fn fmt_return_type(&mut self, ty: &Type) {
        if let Type::Result { ok, err } = ty {
            self.fmt_type(ok);
            self.write(" ?");
            if !matches!(**err, Type::Named(ref n) if n == Syntax::TYPE_ERROR) {
                self.write(" ");
                self.fmt_type(err);
            }
        } else if matches!(ty, Type::Option(_)) {
            self.write("(");
            self.fmt_type(ty);
            self.write(")");
        } else {
            self.fmt_type(ty);
        }
    }

    pub(super) fn fmt_expr(&mut self, expr: &Expr, prec: Prec) {
        match expr {
            // S68 (D-SG2): `if` used as a value.
            Expr::If { .. } => {
                // D-FMT1: the whole if-expression chain shares one line shape —
                // inline only when every branch is inline-eligible.
                let inline = self.if_expr_chain_inlineable(expr);
                if inline {
                    let saved_out = self.out.len();
                    let saved_col = self.col;
                    let saved_line_start = self.at_line_start;
                    let saved_pending_blank = self.pending_blank;
                    let saved_comment_i = self.comment_i;
                    self.fmt_if_expr(expr, true);
                    // A later branch can cross MAX_WIDTH even when every
                    // source branch was authored inline. Never leave a mixed
                    // inline/expanded chain: roll back and expand all branches
                    // so the first pass is already byte-stable.
                    if self.out[saved_out..].contains('\n') {
                        self.out.truncate(saved_out);
                        self.col = saved_col;
                        self.at_line_start = saved_line_start;
                        self.pending_blank = saved_pending_blank;
                        self.comment_i = saved_comment_i;
                        self.fmt_if_expr(expr, false);
                    }
                } else {
                    self.fmt_if_expr(expr, false);
                }
            }
            Expr::Str(parts, span) => {
                // S70 (D-SG5): re-derive the triple-quoted shape from the source.
                if self.src.get(span.start..span.start + 3) == Some("\"\"\"") {
                    self.fmt_str_multiline(parts);
                } else {
                    self.fmt_str(parts);
                }
            }
            // D-SHIFT1 (c7shift): `cursor.take_pattern("…")`'s pattern-literal
            // argument — same rendering as a `Pattern::StrMatch` (D-PARSESTR1).
            Expr::StrMatchLit(parts, _) => {
                self.fmt_str_match_parts(parts);
            }
            // D-BINPAT1 (card #506 follow-up): `reader.take_pattern(b"…")`'s
            // pattern-literal argument — same rendering as a
            // `Pattern::BinMatch` (byte-mode sibling of the arm above).
            Expr::BinMatchLit(parts, _) => {
                self.fmt_bin_match_parts(parts);
            }
            // S34/S67: keep the author's radix (`0x2a`/`0o17`/`0b1010`),
            // digit separators (`1_000_000`), and hex-digit case. The AST
            // stores only the value, so fmt was rewriting every hex/octal/
            // binary literal to decimal — destroying ratified spelling (same
            // failure class as a dropped token). Re-emit the original source
            // slice, but only when it round-trips to the same value: some
            // Int nodes are synthesized with a borrowed nearby span whose
            // text isn't a number at all.
            Expr::Int(n, span, _) => {
                let text = int_literal_spelling(self.src, *span, *n);
                self.write(&text);
            }
            Expr::Float(v, _, _) => self.write(&fmt_float(*v)),
            // D-UNITLIT1: `500ms` — no space between the number and the suffix.
            Expr::UnitLit {
                raw, suffix, ..
            } => {
                self.write(raw);
                self.write(suffix);
            }
            Expr::Bool(b, _) => self.write(if *b { "true" } else { "false" }),
            Expr::Char(c, _) => self.write(&fmt_char(*c)),
            Expr::ListLit(elems, _) => {
                self.write("[");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(e, Prec::OrFallback);
                }
                self.write("]");
            }
            Expr::TupleLit(fields, _, _) => {
                self.write("(");
                for (i, (name, e)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(name);
                    self.write(": ");
                    self.fmt_expr(e, Prec::OrFallback);
                }
                self.write(")");
            }
            Expr::MapLit(pairs, _) => {
                // D-EMPTYLIT1: an empty map is spelled `[]`, same as an empty
                // list — sema (not the formatter) is the only thing that
                // knows an empty `MapLit` node came from that shared spelling.
                self.write("[");
                if !pairs.is_empty() {
                    for (i, (k, v)) in pairs.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.fmt_expr(k, Prec::OrFallback);
                        self.write(": ");
                        self.fmt_expr(v, Prec::OrFallback);
                    }
                }
                self.write("]");
            }
            Expr::Index { base, index, .. } => {
                self.fmt_expr(base, Prec::Postfix);
                self.write("[");
                self.fmt_expr(index, Prec::OrFallback);
                self.write("]");
            }
            Expr::Slice {
                base, start, end, ..
            } => {
                self.fmt_expr(base, Prec::Postfix);
                self.write("[");
                self.fmt_expr(start, Prec::OrFallback);
                self.write("..");
                self.fmt_expr(end, Prec::OrFallback);
                self.write("]");
            }
            Expr::Ident(name, _) => self.write(name),
            Expr::Call(c)
                if matches!(
                    c.name.as_str(),
                    Syntax::TYPED_TEXT_SQL_PREFIX_CALL | Syntax::TYPED_TEXT_HTML_PREFIX_CALL
                ) =>
            {
                self.write(if c.name == Syntax::TYPED_TEXT_SQL_PREFIX_CALL {
                    "sql"
                } else {
                    "html"
                });
                if let Some(arg) = c.args.first() {
                    if let Expr::Str(parts, _) = &arg.expr {
                        self.fmt_str(parts);
                    }
                }
            }
            Expr::Call(c) => self.fmt_call(c),
            Expr::Unary(op, inner, _) => {
                let inner_prec = Prec::Unary;
                // Wrap only when the surrounding slot binds tighter than this
                // operator (e.g. a unary expr used as a `.method()` receiver).
                if prec > inner_prec {
                    self.write("(");
                }
                match op {
                    UnOp::Neg => self.write("-"),
                    UnOp::Not => self.write("!"),
                }
                self.fmt_expr(inner, inner_prec);
                if prec > inner_prec {
                    self.write(")");
                }
            }
            Expr::IncDec {
                op,
                operand,
                postfix,
                ..
            } => {
                if *postfix {
                    if prec > Prec::Postfix {
                        self.write("(");
                    }
                    self.fmt_expr(operand, Prec::Postfix);
                    self.write(match op {
                        crate::AST::IncDecOp::Inc => "++",
                        crate::AST::IncDecOp::Dec => "--",
                    });
                    if prec > Prec::Postfix {
                        self.write(")");
                    }
                } else {
                    let inner_prec = Prec::Unary;
                    if prec > inner_prec {
                        self.write("(");
                    }
                    self.write(match op {
                        crate::AST::IncDecOp::Inc => "++",
                        crate::AST::IncDecOp::Dec => "--",
                    });
                    self.fmt_expr(operand, inner_prec);
                    if prec > inner_prec {
                        self.write(")");
                    }
                }
            }
            Expr::Binary(op, lhs, rhs, _) => {
                let op_prec = Prec::of_bin(*op);
                // Wrap only when the surrounding slot binds tighter than this
                // operator (e.g. `(a + b).method()`, `(a + b) * c`); equal-prec
                // right-hand nesting is handled by `add_rhs` on the rhs slot.
                if prec > op_prec {
                    self.write("(");
                }
                self.fmt_expr(lhs, op_prec);
                self.write(" ");
                self.write(op.spell());
                self.write(" ");
                self.fmt_expr(rhs, op_prec.add_rhs());
                if prec > op_prec {
                    self.write(")");
                }
            }
            // D-CHAINCMP1: `0 <= sev < 10` — same-direction relational chain.
            // Single spaces around each operator, no parens between pairs
            // (chain reads left to right at uniform precedence).
            Expr::CompareChain { operands, ops, .. } => {
                let op_prec = Prec::of_bin(ops[0]);
                if prec > op_prec {
                    self.write("(");
                }
                self.fmt_expr(&operands[0], op_prec);
                for (op, operand) in ops.iter().zip(operands.iter().skip(1)) {
                    self.write(" ");
                    self.write(op.spell());
                    self.write(" ");
                    self.fmt_expr(operand, op_prec.add_rhs());
                }
                if prec > op_prec {
                    self.write(")");
                }
            }
            // D-CAP9: postfix `p.*` deref.
            Expr::Deref(inner, _) => {
                self.fmt_expr(inner, Prec::Postfix);
                self.write(".*");
            }
            // D-CAP9: prefix `*x` raw-pointer-of.
            Expr::RawOf(inner, _) => {
                self.write("*");
                self.fmt_expr(inner, Prec::Unary);
            }
            // D-CAP2 (D-MEM1/S4): prefix `copy x` — the one copy verb.
            Expr::Copy(inner, _) => {
                self.write(Syntax::KW_COPY);
                self.write(" ");
                self.fmt_expr(inner, Prec::Unary);
            }
            Expr::Field(base, field, span) => {
                self.fmt_expr(base, Prec::Postfix);
                // S69 (D-SG3): keep an author-placed break before `.field`.
                if self.chain_break_between(base.span().end, span.end) {
                    // The receiver's own trailing comment (e.g. `.step()  // note`)
                    // stays on its line, before we break to this step.
                    self.emit_trailing(base.span().end);
                    self.with_indent(|f| {
                        f.newline();
                        f.write(".");
                        f.write(field);
                    });
                } else {
                    self.write(".");
                    self.write(field);
                }
            }
            Expr::OptField { base, member, .. } => {
                self.fmt_expr(base, Prec::Postfix);
                self.write("?.");
                self.write(member);
            }
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                type_args,
                args,
                ..
            } => {
                self.fmt_expr(receiver, Prec::Postfix);
                // S69 (D-SG3): keep an author-placed break before `.method(...)`.
                if self.chain_break_between(receiver.span().end, method_span.start) {
                    // The receiver's own trailing comment (e.g. `.step()  // note`)
                    // stays on its line, before we break to this step.
                    self.emit_trailing(receiver.span().end);
                    self.with_indent(|f| {
                        f.newline();
                        f.write(".");
                        f.write(method);
                        f.fmt_method_type_args(type_args);
                        f.fmt_view_or_call_args(method, args);
                    });
                } else {
                    self.write(".");
                    self.write(method);
                    self.fmt_method_type_args(type_args);
                    self.fmt_view_or_call_args(method, args);
                }
            }
            Expr::StructLit {
                type_name,
                type_args,
                import_ns,
                fields,
                inferred,
                ..
            } => {
                // D-DOTCTOR1: emit `Type.{ … }` (named) or `.{ … }` (inferred).
                // The formatter is also the auto-fixer for E0320: any old `Type { … }`
                // (recovered with `inferred: false`) is re-emitted in the new form.
                if *inferred {
                    // `.{ field: val, … }` — type inferred from context.
                } else {
                    if let Some(ns) = import_ns {
                        self.write(ns.as_str());
                        self.write(".");
                    }
                    self.write(type_name);
                    if !type_args.is_empty() {
                        self.write("<");
                        for (i, a) in type_args.iter().enumerate() {
                            if i > 0 {
                                self.write(", ");
                            }
                            self.fmt_type(a);
                        }
                        self.write(">");
                    }
                }
                self.write(".{");
                for (i, (name, _, expr)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(name);
                    self.write(": ");
                    self.fmt_expr(expr, Prec::OrFallback);
                }
                self.write("}");
            }
            Expr::EnumLit {
                type_name,
                variant,
                args,
                ..
            } => {
                self.write(type_name);
                self.write(".");
                self.write(variant);
                if !args.is_empty() {
                    // D-UITREE1/D-DOTCTOR1: named-payload variants use the struct
                    // dot-brace spelling (`.Variant.{ field: val }`); positional
                    // (single-payload, S30) variants keep the paren call form.
                    let named = matches!(args.first(), Some(EnumLitArg::Named { .. }));
                    self.write(if named { ".{" } else { "(" });
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        match arg {
                            EnumLitArg::Positional(e) => self.fmt_expr(e, Prec::OrFallback),
                            EnumLitArg::Named { label, expr } => {
                                self.write(label);
                                self.write(": ");
                                self.fmt_expr(expr, Prec::OrFallback);
                            }
                        }
                    }
                    self.write(if named { "}" } else { ")" });
                }
            }
            // D-TAINT1: `#Tainted expr` — prefix value-fact tag, space-separated.
            Expr::Tainted(inner, _) => {
                self.write(&format!("#{} ", Syntax::KW_TAINTED));
                self.fmt_expr(inner, Prec::Unary);
            }
            Expr::Present(inner, _) => {
                self.write(Syntax::LIT_VALUE);
                self.write("(");
                self.fmt_expr(inner, Prec::OrFallback);
                self.write(")");
            }
            Expr::Absent(_) => self.write(Syntax::LIT_NULL),
            // D-SIMD2: a reduce-op marker `#Add`/`#Mul`/`#Min`/`#Max` (inside `.reduce(…)`).
            Expr::ReduceMarker(name, _) => self.write(&format!("#{}", name)),
            Expr::Todo { .. } => self.write(&format!("#{}", Syntax::KW_TODO)),
            Expr::PatternTest {
                subject, pattern, ..
            } => {
                self.fmt_expr(subject, Prec::Cmp);
                self.write(" == ");
                self.fmt_pattern(pattern);
            }
            Expr::Ok(inner, _) => {
                self.write("ok(");
                self.fmt_expr(inner, Prec::OrFallback);
                self.write(")");
            }
            Expr::Err(inner, _) => {
                self.write("err(");
                self.fmt_expr(inner, Prec::OrFallback);
                self.write(")");
            }
            Expr::Try(inner, _, _) => {
                self.fmt_expr(inner, Prec::Postfix);
                self.write("?");
            }
            Expr::OrFallback {
                value, fallback, ..
            } => {
                if prec > Prec::OrFallback {
                    self.write("(");
                }
                self.fmt_expr(value, Prec::OrFallback.add_rhs());
                self.write(" ?? ");
                self.fmt_or_fallback(fallback);
                if prec > Prec::OrFallback {
                    self.write(")");
                }
            }
            Expr::Lambda(lam) => self.fmt_lambda(lam),
            Expr::CallValue { callee, args, .. } => {
                if prec > Prec::Postfix {
                    self.write("(");
                }
                self.fmt_expr(callee, Prec::Postfix);
                self.fmt_call_args_or_trailing_block(args);
                if prec > Prec::Postfix {
                    self.write(")");
                }
            }
            Expr::FanOut { callee, items, .. } => {
                self.fmt_expr(callee, Prec::Postfix);
                self.write(".[");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(item, Prec::OrFallback);
                }
                self.write("]");
            }
            // S58 (E2-M13): `alias.Ptr<T>.from_addr(addr)`.
            Expr::PtrFromAddr {
                alias, elem, addr, ..
            } => {
                self.write(alias);
                self.write(&format!(".{}<", Syntax::TYPE_PTR));
                self.fmt_type(elem);
                self.write(&format!(">.{}(", Syntax::MEM_FROM_ADDR));
                self.fmt_expr(addr, Prec::OrFallback);
                self.write(")");
            }
            // D-CTMARKER1=C: `$name` comptime splice expression.
            Expr::ComptimeSplice { name, .. } => {
                self.write(&format!("${}", name));
            }
            // D-FMTPARENS1=A: author-written grouping parens are always re-emitted.
            Expr::Paren(inner, _) => {
                self.write("(");
                self.fmt_expr(inner, Prec::OrFallback);
                self.write(")");
            }
            // D-VARIADIC1: spread call argument — emit verbatim from source when possible.
            Expr::Spread(inner, _) => {
                self.write("...");
                self.fmt_expr(inner, Prec::Postfix);
            }
        }
    }

    fn fmt_lambda(&mut self, lam: &crate::AST::Lambda) {
        if !lam.take_names.is_empty() {
            self.write("take(");
            for (i, (n, _)) in lam.take_names.iter().enumerate() {
                if i > 0 {
                    self.write(", ");
                }
                self.write(n);
            }
            self.write(") ");
        }
        self.write("(");
        for (i, p) in lam.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&p.name);
            if let Some(ty) = &p.ty {
                self.write(": ");
                self.fmt_type(ty);
            }
        }
        self.write(") => ");
        match &lam.body {
            crate::AST::LambdaBody::Expr(e) => self.fmt_expr(e, Prec::OrFallback.add_rhs()),
            crate::AST::LambdaBody::Block(stmts) => {
                self.write("{");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(stmts));
                self.end_block();
            }
        }
    }

    fn fmt_or_fallback(&mut self, fb: &OrFallback) {
        match fb {
            OrFallback::Value(e) => self.fmt_expr(e, Prec::OrFallback),
            OrFallback::Return(expr, _) => {
                self.write("return");
                if let Some(e) = expr {
                    self.write(" ");
                    self.fmt_expr(e, Prec::OrFallback);
                }
            }
            OrFallback::Panic { args, .. } => {
                self.write("panic(");
                self.fmt_call_args(args);
                self.write(")");
            }
            OrFallback::Break(_) => self.write("break"),
            OrFallback::Continue(_) => self.write("continue"),
        }
    }

    pub(super) fn fmt_pattern(&mut self, pat: &Pattern) {
        use crate::AST::PatSlot;
        match pat {
            Pattern::Variant {
                variant, bindings, ..
            } => {
                // D-ENUMDOT1: leading dot is canonical for variant patterns.
                self.write(".");
                self.write(variant);
                if !bindings.is_empty() {
                    self.write("(");
                    for (i, slot) in bindings.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        match slot {
                            PatSlot::Bind(name) => self.write(name),
                            PatSlot::Wildcard => self.write("_"),
                            PatSlot::Range { lo, hi } => {
                                self.write(&lo.to_string());
                                self.write("..");
                                self.write(&hi.to_string());
                            }
                        }
                    }
                    self.write(")");
                }
            }
            Pattern::Present { binding, .. } => {
                self.write(Syntax::LIT_VALUE);
                self.write("(");
                self.write(binding);
                self.write(")");
            }
            Pattern::Absent(_) => self.write(Syntax::LIT_NULL),
            Pattern::Ok { binding, .. } => {
                self.write("ok(");
                self.write(binding);
                self.write(")");
            }
            Pattern::Err { binding, .. } => {
                self.write("err(");
                self.write(binding);
                self.write(")");
            }
            // D-PATR: range pattern at arm-head level.
            Pattern::Range { lo, hi, .. } => {
                self.write(&lo.to_string());
                self.write("..");
                self.write(&hi.to_string());
            }
            // D-PATO: or-pattern `A(x) | B(x)`.
            Pattern::Or(alts, _) => {
                for (i, alt) in alts.iter().enumerate() {
                    if i > 0 {
                        self.write(" | ");
                    }
                    self.fmt_pattern(alt);
                }
            }
            // D-DESTRUCT1: struct-shaped dispatch arm head.
            Pattern::Struct { fields, rest, .. } => {
                self.write(".{");
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    match field {
                        crate::AST::StructPatField::Bind { field, local, .. } if field == local => {
                            self.write(field)
                        }
                        crate::AST::StructPatField::Bind { field, local, .. } => {
                            self.write(field);
                            self.write(": ");
                            self.write(local);
                        }
                        crate::AST::StructPatField::Value { field, value, .. } => {
                            self.write(field);
                            self.write(": ");
                            self.fmt_expr(value, Prec::OrFallback);
                        }
                    }
                }
                if rest.is_some() {
                    if !fields.is_empty() {
                        self.write(", ");
                    }
                    self.write("..");
                }
                self.write("}");
            }
            // D-PARSESTR1: an interpolation literal used as a pattern —
            // formats identically to the same text as a format literal, plus
            // a `:Type` suffix on typed holes (which format literals never
            // have).
            Pattern::StrMatch { parts, .. } => {
                self.fmt_str_match_parts(parts);
            }
            // D-BINPAT1 (card #506): a `b"…"` binary pattern.
            Pattern::BinMatch { parts, .. } => {
                self.fmt_bin_match_parts(parts);
            }
        }
    }

    /// D-BINPAT1: render a `BinMatchPart` list as a `b"…"` literal —
    /// fixed bytes as text, holes as `{name:U<width>[be|le]}` / `{name:...}`.
    pub(super) fn fmt_bin_match_parts(&mut self, parts: &[crate::AST::BinMatchPart]) {
        use crate::AST::{BinEndian, BinMatchPart, BinSpec};
        self.write("b\"");
        for part in parts {
            match part {
                BinMatchPart::Lit(bytes) => {
                    self.write(&String::from_utf8_lossy(bytes));
                }
                BinMatchPart::Hole { name, spec, .. } => {
                    self.write("{");
                    self.write(name);
                    self.write(":");
                    match spec {
                        BinSpec::Rest => self.write("..."),
                        BinSpec::Bits { width, endian } => {
                            self.write(&format!("U{}", width));
                            match endian {
                                BinEndian::Big => self.write(Syntax::BINPAT_ENDIAN_BIG),
                                BinEndian::Little => self.write(Syntax::BINPAT_ENDIAN_LITTLE),
                                BinEndian::None => {}
                            }
                        }
                    }
                    self.write("}");
                }
            }
        }
        self.write("\"");
    }

    /// D-PARSESTR1 (shared with `Expr::StrMatchLit` — D-SHIFT1's
    /// `take_pattern` argument): render a `StrMatchPart` list the same way a
    /// format literal renders, plus a `:Type` suffix on typed holes.
    pub(super) fn fmt_str_match_parts(&mut self, parts: &[crate::AST::StrMatchPart]) {
        self.write("\"");
        for part in parts {
            match part {
                crate::AST::StrMatchPart::Lit(s) => self.write(&escape_str_lit(s)),
                crate::AST::StrMatchPart::Hole { name, ty, .. } => {
                    self.write("{");
                    self.write(name);
                    if let Some(t) = ty {
                        self.write(":");
                        self.fmt_type(t);
                    }
                    self.write("}");
                }
            }
        }
        self.write("\"");
    }

    fn fmt_call(&mut self, c: &Call) {
        self.write(&c.name);
        self.fmt_call_args_or_trailing_block(&c.args);
    }

    /// D-SERDE6: call-site turbofish `<T, …>` on a method call (`decode<Order>(…)`).
    /// No-op when the call carries no type arguments.
    fn fmt_method_type_args(&mut self, type_args: &[crate::AST::Type]) {
        if type_args.is_empty() {
            return;
        }
        self.write("<");
        for (i, t) in type_args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.fmt_type(t);
        }
        self.write(">");
    }

    /// D-TRAILBLOCK1: emit `(args) { … }` — or bare `{ … }` when the block is
    /// the call's only argument — when the LAST arg is the desugared
    /// trailing-block lambda; otherwise the ordinary `(args)`. Shared by
    /// `Expr::Call`, `Expr::MethodCall`, and `Expr::CallValue` so all three
    /// call shapes round-trip the sugar identically.
    /// D-DYNARRAY1: `.view(a..b)` parses its two args from `start .. end`, not
    /// a comma list — round-trip that shape here, or `jet fmt` would silently
    /// rewrite `.view(0..9)` into the unparseable `.view(0, 9)` (own-memory
    /// rule: new syntax needs a formatter round-trip, not just a parser).
    fn fmt_view_or_call_args(&mut self, method: &str, args: &[CallArg]) {
        if method == Syntax::METHOD_VIEW && args.len() == 2 {
            self.write("(");
            self.fmt_expr(&args[0].expr, Prec::OrFallback);
            self.write("..");
            self.fmt_expr(&args[1].expr, Prec::OrFallback);
            self.write(")");
            return;
        }
        self.fmt_call_args_or_trailing_block(args);
    }

    fn fmt_call_args_or_trailing_block(&mut self, args: &[CallArg]) {
        if let Some((last, init)) = args.split_last() {
            if last.flags.is_trailing_block {
                if !init.is_empty() {
                    self.write("(");
                    self.fmt_call_args(init);
                    self.write(")");
                }
                self.write(" ");
                if let Expr::Lambda(lam) = &last.expr {
                    if let crate::AST::LambdaBody::Block(stmts) = &lam.body {
                        self.write("{");
                        self.newline();
                        self.with_indent(|f| f.fmt_block_stmts(stmts));
                        self.end_block();
                        return;
                    }
                }
                // Defensive: not the shape the parser produces, fall through.
            }
        }
        self.write("(");
        self.fmt_call_args(args);
        self.write(")");
    }

    fn fmt_call_args(&mut self, args: &[CallArg]) {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            // D-MEM1: the call-site capability is a sigil that attaches to the
            // argument with no space (`^x`, `&x`). The parser reads it
            // before the label, so fmt emits it in that order to round-trip.
            // `Read` is unmarked; `Raw` (`*`) is handled apart.
            match arg.convention {
                AccessConvention::Read | AccessConvention::Raw => {}
                AccessConvention::Write => self.write(Syntax::SIGIL_WRITE),
                AccessConvention::Move => self.write(Syntax::SIGIL_MOVE),
                AccessConvention::Share => self.write(Syntax::SIGIL_WRITE),
            }
            // D-VARIADIC1: `f(...xs)` call spread — the parser reads this
            // between the access-convention sigil and the optional label
            // (see `call_arg` in Parser/Expressions.rs), so fmt re-emits it
            // in that same position. Dropping this silently changed a spread
            // call into a plain one (real behavior change, not just style —
            // caught as a genuine fmt-stability regression on
            // examples/features/basics/variadics_spread.jet).
            if arg.spread {
                self.write("...");
            }
            // S61: preserve the call-site argument label `name:` (canonical
            // `name: value` spacing, matching struct-literal field init).
            if let Some((name, _)) = &arg.label {
                self.write(name);
                self.write(": ");
            }
            self.fmt_expr(&arg.expr, Prec::OrFallback);
        }
    }

    fn fmt_str(&mut self, parts: &[StrPart]) {
        self.write("\"");
        for part in parts {
            match part {
                StrPart::Lit(s) => self.write(&escape_str_lit(s)),
                StrPart::Interp(e, fmt) => {
                    self.write("{");
                    self.fmt_expr(e, Prec::OrFallback);
                    if *fmt == crate::AST::StrFormat::Debug {
                        self.write("@");
                        self.write(crate::Syntax::INTERP_SELECTOR_DEBUG);
                    }
                    self.write("}");
                }
            }
        }
        self.write("\"");
    }

    /// S70 (D-SG5): re-emit a triple-quoted string. Content sits at the current
    /// indent, so re-lexing strips exactly the same prefix the closing `"""`
    /// sets — keeping `jet fmt` idempotent.
    fn fmt_str_multiline(&mut self, parts: &[StrPart]) {
        self.write("\"\"\"");
        self.newline();
        for part in parts {
            match part {
                StrPart::Lit(s) => {
                    for (i, line) in s.split('\n').enumerate() {
                        if i > 0 {
                            self.newline();
                        }
                        if !line.is_empty() {
                            self.write(&escape_str_multiline(line));
                        }
                    }
                }
                StrPart::Interp(e, fmt) => {
                    self.write("{");
                    self.fmt_expr(e, Prec::OrFallback);
                    if *fmt == crate::AST::StrFormat::Debug {
                        self.write("@");
                        self.write(crate::Syntax::INTERP_SELECTOR_DEBUG);
                    }
                    self.write("}");
                }
            }
        }
        self.newline();
        self.write("\"\"\"");
    }
}
