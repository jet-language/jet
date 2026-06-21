use super::*;
use crate::AST::{
    AccessConvention, Call, CallArg, EnumLitArg, Expr, OrFallback, Pattern, StrPart, Type, UnOp,
};

impl<'a> Fmt<'a> {
    pub(super) fn fmt_type(&mut self, ty: &Type) {
        match ty {
            Type::Int => self.write(Syntax::TYPE_INT),
            Type::Float => self.write(Syntax::TYPE_FLOAT),
            Type::Bool => self.write(Syntax::TYPE_BOOL),
            Type::String => self.write(Syntax::TYPE_STRING),
            Type::Char => self.write(Syntax::TYPE_CHAR),
            Type::List(inner) => {
                self.write("[");
                self.fmt_type(inner);
                self.write("]");
            }
            Type::Map { key, value } => {
                self.write("[");
                self.fmt_type(key);
                self.write(", ");
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
            Type::Fn { params, ret } => {
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
                self.write(t);
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
            Type::FixedList { elem, len } => {
                self.write("[");
                self.fmt_type(elem);
                self.write(&format!("#{}", len));
                self.write("]");
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
            Expr::If {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
                ..
            } => {
                self.write("if ");
                self.fmt_cond(cond);
                self.write(" ");
                self.fmt_value_block(then_body, then_value);
                self.write(" else ");
                if else_body.is_empty() && matches!(else_value.as_ref(), Expr::If { .. }) {
                    self.fmt_expr(else_value, Prec::OrFallback);
                } else {
                    self.fmt_value_block(else_body, else_value);
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
            Expr::Int(n, _) => self.write(&n.to_string()),
            Expr::Float(v, _) => self.write(&fmt_float(*v)),
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
                self.write("[");
                if pairs.is_empty() {
                    self.write(":");
                } else {
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
            Expr::Call(c) => self.fmt_call(c),
            Expr::Unary(op, inner, _) => {
                let inner_prec = Prec::Unary;
                if prec <= inner_prec {
                    self.write("(");
                }
                match op {
                    UnOp::Neg => self.write("-"),
                    UnOp::Not => self.write("!"),
                }
                self.fmt_expr(inner, inner_prec);
                if prec <= inner_prec {
                    self.write(")");
                }
            }
            Expr::Binary(op, lhs, rhs, _) => {
                let op_prec = Prec::of_bin(*op);
                if prec <= op_prec {
                    self.write("(");
                }
                self.fmt_expr(lhs, op_prec);
                self.write(" ");
                self.write(op.spell());
                self.write(" ");
                self.fmt_expr(rhs, op_prec.add_rhs());
                if prec <= op_prec {
                    self.write(")");
                }
            }
            Expr::Deref(inner, _) => {
                self.write("*");
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
                        f.write("(");
                        f.fmt_call_args(args);
                        f.write(")");
                    });
                } else {
                    self.write(".");
                    self.write(method);
                    self.write("(");
                    self.fmt_call_args(args);
                    self.write(")");
                }
            }
            Expr::StructLit {
                type_name,
                type_args,
                import_ns,
                fields,
                ..
            } => {
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
                self.write("{");
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
                    self.write("(");
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
                    self.write(")");
                }
            }
            Expr::Present(inner, _) => {
                self.write("value(");
                self.fmt_expr(inner, Prec::OrFallback);
                self.write(")");
            }
            Expr::Absent(_) => self.write("null"),
            Expr::Todo { .. } => self.write("todo"),
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
                self.write("(");
                self.fmt_call_args(args);
                self.write(")");
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
        }
    }

    pub(super) fn fmt_pattern(&mut self, pat: &Pattern) {
        use crate::AST::PatSlot;
        match pat {
            Pattern::Variant {
                variant, bindings, ..
            } => {
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
                self.write("value(");
                self.write(binding);
                self.write(")");
            }
            Pattern::Absent(_) => self.write("null"),
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
        }
    }

    fn fmt_call(&mut self, c: &Call) {
        self.write(&c.name);
        self.write("(");
        self.fmt_call_args(&c.args);
        self.write(")");
    }

    fn fmt_call_args(&mut self, args: &[CallArg]) {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            // Convention prefix comes first: the parser reads `mut`/`take`
            // before the label (`f(mut x: v)`), so fmt must emit it in that
            // order or the output won't re-parse.
            match arg.convention {
                AccessConvention::Read => {}
                AccessConvention::Mutate => {
                    self.write("mut ");
                }
                AccessConvention::Move => {
                    self.write("take ");
                }
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
                StrPart::Interp(e) => {
                    self.write("{");
                    self.fmt_expr(e, Prec::OrFallback);
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
                StrPart::Interp(e) => {
                    self.write("{");
                    self.fmt_expr(e, Prec::OrFallback);
                    self.write("}");
                }
            }
        }
        self.newline();
        self.write("\"\"\"");
    }
}
