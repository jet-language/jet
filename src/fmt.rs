//! Pretty-printer for Jet source (M6 phase 1, S44).
//!
//! One true style: 4-space indent, same-line `{`, width 100, spaces around
//! binary operators, semicolons on statements. Comments are preserved from
//! the original source and re-attached by span.

use crate::ast::{
    AccessConvention, BinOp, Binding, Call, CallArg, ConstAttr, ConstDef, ElseBranch, EnumDef,
    EnumLitArg, Expr, Field, ForKind, Func, IfStmt, ImplDef, Item, LValue, OrFallback, Param,
    Pattern, Program, Stmt, StrPart, StructDef, SwitchArm, Type, UnOp, Variant,
    VariantPayload,
};
use crate::diag::Span;
use crate::lexer::{line_comments, Token, TokKind};
use crate::syntax;

const INDENT: usize = 4;
const WIDTH: usize = 100;

/// Format a parsed program back to canonical Jet source.
pub fn format_program(prog: &Program, src: &str, comment_toks: &[Token]) -> String {
    let comments: Vec<Comment> = comment_toks
        .iter()
        .map(|t| Comment {
            text: match &t.kind {
                TokKind::LineComment(s) => s.clone(),
                _ => unreachable!(),
            },
            span: t.span,
        })
        .collect();
    let mut f = Fmt {
        src,
        comments,
        comment_i: 0,
        out: String::new(),
        col: 0,
        at_line_start: true,
        indent: 0,
        pending_blank: false,
    };
    for (i, item) in prog.items.iter().enumerate() {
        if i > 0 {
            f.blank_line_between_items();
        }
        f.emit_leading(item_span_start(item, src));
        f.fmt_item(item);
        f.emit_trailing(item_span_end(item));
    }
    f.emit_remaining_comments();
    if !f.out.ends_with('\n') {
        f.out.push('\n');
    }
    f.out
}

struct Comment {
    text: String,
    span: Span,
}

struct Fmt<'a> {
    src: &'a str,
    comments: Vec<Comment>,
    comment_i: usize,
    out: String,
    col: usize,
    at_line_start: bool,
    indent: usize,
    pending_blank: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    OrFallback = 0,
    Or = 1,
    And = 2,
    Cmp = 3,
    BitOr = 4,
    BitXor = 5,
    BitAnd = 6,
    Shift = 7,
    Add = 8,
    Mul = 9,
    Unary = 10,
    Postfix = 11,
    Primary = 12,
}

impl Prec {
    fn of_bin(op: BinOp) -> Self {
        match op {
            BinOp::Or => Prec::Or,
            BinOp::And => Prec::And,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => Prec::Cmp,
            BinOp::BitOr => Prec::BitOr,
            BinOp::BitXor => Prec::BitXor,
            BinOp::BitAnd => Prec::BitAnd,
            BinOp::Shl | BinOp::Shr => Prec::Shift,
            BinOp::Add | BinOp::Sub => Prec::Add,
            BinOp::Mul | BinOp::Div | BinOp::Rem => Prec::Mul,
        }
    }
}

fn line_of(src: &str, pos: usize) -> usize {
    src[..pos.min(src.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
}

fn item_span_start(item: &Item, src: &str) -> usize {
    match item {
        Item::Func(f) => func_decl_start(f, src),
        Item::Struct(s) => type_decl_start(s.is_pub, s.name_span.start, "struct", src),
        Item::Enum(e) => type_decl_start(e.is_pub, e.name_span.start, "enum", src),
        Item::Impl(i) => {
            src[..i.type_span.start]
                .rfind("impl")
                .unwrap_or(i.type_span.start)
        }
        Item::Const(c) => {
            src[..c.name_span.start]
                .rfind("const")
                .unwrap_or(c.name_span.start)
        }
    }
}

fn func_decl_start(f: &Func, src: &str) -> usize {
    let before = &src[..f.name_span.start];
    let fn_pos = before.rfind("fn").unwrap_or(f.name_span.start);
    if f.is_pub {
        before[..fn_pos]
            .rfind("pub")
            .unwrap_or(fn_pos)
    } else {
        fn_pos
    }
}

fn type_decl_start(is_pub: bool, name_start: usize, kw: &str, src: &str) -> usize {
    let before = &src[..name_start];
    let kw_pos = before.rfind(kw).unwrap_or(name_start);
    if is_pub {
        before[..kw_pos].rfind("pub").unwrap_or(kw_pos)
    } else {
        kw_pos
    }
}

fn item_span_end(item: &Item) -> usize {
    match item {
        Item::Func(f) => f.body.last().map(stmt_end).unwrap_or(f.name_span.end),
        Item::Struct(s) => s.methods.last().map(|m| m.body.last().map(stmt_end).unwrap_or(m.name_span.end))
            .or_else(|| s.fields.last().map(|fld| fld.ty_span.end))
            .unwrap_or(s.name_span.end),
        Item::Enum(e) => e.methods.last().map(|m| m.body.last().map(stmt_end).unwrap_or(m.name_span.end))
            .or_else(|| e.variants.last().map(|v| v.name_span.end))
            .unwrap_or(e.name_span.end),
        Item::Impl(i) => i.methods.last().map(|m| m.body.last().map(stmt_end).unwrap_or(m.name_span.end))
            .unwrap_or(i.type_span.end),
        Item::Const(c) => c.value.span().end,
    }
}

fn stmt_end(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Expr(e) => e.span().end,
        Stmt::Val(b) => b.init.span().end,
        Stmt::Assign { value, .. } => value.span().end,
        Stmt::Return(e, s) => e.as_ref().map(|x| x.span().end).unwrap_or(s.end),
        Stmt::If(i) => if_end(i),
        Stmt::While { body, .. } => body.last().map(stmt_end).unwrap_or(0),
        Stmt::For { body, .. } => body.last().map(stmt_end).unwrap_or(0),
        Stmt::Switch { else_body, arms, .. } => else_body
            .as_ref()
            .and_then(|b| b.last())
            .map(stmt_end)
            .or_else(|| arms.last().map(|a| a.body.last().map(stmt_end).unwrap_or(a.span.end)))
            .unwrap_or(0),
        Stmt::Break(s) | Stmt::Continue(s) => s.end,
        Stmt::Loop(inner, s) | Stmt::Unsafe(inner, s) => {
            inner.last().map(stmt_end).unwrap_or(s.end)
        }
    }
}

fn if_end(i: &IfStmt) -> usize {
    match &i.else_branch {
        Some(ElseBranch::ElseIf(e)) => if_end(e),
        Some(ElseBranch::Else(body)) => body.last().map(stmt_end).unwrap_or(i.span.end),
        None => i.then_body.last().map(stmt_end).unwrap_or(i.span.end),
    }
}

impl<'a> Fmt<'a> {
    fn blank_line_between_items(&mut self) {
        if !self.pending_blank {
            self.newline();
            self.pending_blank = true;
        }
    }

    fn emit_remaining_comments(&mut self) {
        while self.comment_i < self.comments.len() {
            self.emit_comment_line(&self.comments[self.comment_i].text.clone());
            self.comment_i += 1;
        }
    }

    fn emit_leading(&mut self, pos: usize) {
        while self.comment_i < self.comments.len() {
            let (text, span) = {
                let c = &self.comments[self.comment_i];
                (c.text.clone(), c.span)
            };
            if span.start >= pos {
                break;
            }
            if self.is_trailing_comment_at(span) {
                break;
            }
            self.emit_comment_line(&text);
            self.comment_i += 1;
            self.pending_blank = false;
        }
    }

    fn emit_trailing(&mut self, end: usize) {
        while self.comment_i < self.comments.len() {
            let (text, span) = {
                let c = &self.comments[self.comment_i];
                (c.text.clone(), c.span)
            };
            if span.start >= end && line_of(self.src, span.start) == line_of(self.src, end) {
                self.write("  ");
                self.emit_comment_inline(&text);
                self.comment_i += 1;
            } else {
                break;
            }
        }
    }

    fn is_trailing_comment_at(&self, span: Span) -> bool {
        if self.out.is_empty() {
            return false;
        }
        let last_nl = self.out.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let prev_line = line_of(self.src, last_nl.min(self.src.len()));
        line_of(self.src, span.start) == prev_line && !self.at_line_start
    }

    fn emit_comment_line(&mut self, text: &str) {
        self.newline();
        self.write_indent();
        self.write(text);
        self.newline();
        self.pending_blank = false;
    }

    fn emit_comment_inline(&mut self, text: &str) {
        self.write(text);
        self.pending_blank = false;
    }

    fn write(&mut self, s: &str) {
        if self.at_line_start && !s.is_empty() {
            self.write_indent();
        }
        self.col += s.chars().count();
        self.at_line_start = false;
        self.out.push_str(s);
        self.pending_blank = false;
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.col = self.indent * INDENT;
        self.at_line_start = false;
    }

    fn newline(&mut self) {
        self.out.push('\n');
        self.col = 0;
        self.at_line_start = true;
    }

    fn with_indent<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.indent += 1;
        let r = f(self);
        self.indent -= 1;
        r
    }

    fn fmt_item(&mut self, item: &Item) {
        match item {
            Item::Func(f) => self.fmt_func(f, true),
            Item::Struct(s) => self.fmt_struct(s, true),
            Item::Enum(e) => self.fmt_enum(e, true),
            Item::Impl(i) => self.fmt_impl(i),
            Item::Const(c) => self.fmt_const(c),
        }
    }

    fn fmt_pub(&mut self, is_pub: bool) {
        if is_pub {
            self.write("pub ");
        }
    }

    fn fmt_func(&mut self, f: &Func, top_level: bool) {
        if top_level {
            self.fmt_pub(f.is_pub);
        } else if f.is_pub {
            self.write("pub ");
        }
        self.write("fn ");
        self.write(&f.name);
        self.write("(");
        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.fmt_param(p);
        }
        self.write(")");
        if let Some(ret) = &f.return_type {
            self.write(" -> ");
            if f.is_view_return {
                self.write("view ");
            }
            self.fmt_type(ret);
        }
        self.write(" {");
        self.newline();
        let body = &f.body;
        self.with_indent(|f| f.fmt_block_stmts(body));
        self.end_block();
    }

    fn end_block(&mut self) {
        self.newline();
        self.write("}");
    }

    fn fmt_param(&mut self, p: &Param) {
        match p.convention {
            AccessConvention::Read => {}
            AccessConvention::Mutate => self.write("mut "),
            AccessConvention::Move => self.write("take "),
        }
        self.write(&p.name);
        if p.name != syntax::KW_SELF || !p.ty.name().is_empty() {
            self.write(": ");
            self.fmt_type(&p.ty);
        }
    }

    fn fmt_struct(&mut self, s: &StructDef, top_level: bool) {
        if top_level {
            self.fmt_pub(s.is_pub);
        }
        self.write("struct ");
        self.write(&s.name);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (i, field) in s.fields.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.fmt_field(field);
                f.write(";");
            }
            for (i, m) in s.methods.iter().enumerate() {
                if i > 0 || !s.fields.is_empty() {
                    f.newline();
                    f.newline();
                }
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_enum(&mut self, e: &EnumDef, top_level: bool) {
        if top_level {
            self.fmt_pub(e.is_pub);
        }
        self.write("enum ");
        self.write(&e.name);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (i, v) in e.variants.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.fmt_variant(v);
                f.write(";");
            }
            for (i, m) in e.methods.iter().enumerate() {
                if i > 0 || !e.variants.is_empty() {
                    f.newline();
                    f.newline();
                }
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_variant(&mut self, v: &Variant) {
        self.write(&v.name);
        match &v.payload {
            VariantPayload::Unit => {}
            VariantPayload::Single(ty, _) => {
                self.write("(");
                self.fmt_type(ty);
                self.write(")");
            }
            VariantPayload::Named(fields) => {
                self.write("(");
                for (i, fld) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&fld.name);
                    self.write(": ");
                    self.fmt_type(&fld.ty);
                }
                self.write(")");
            }
        }
    }

    fn fmt_impl(&mut self, i: &ImplDef) {
        self.write("impl ");
        self.write(&i.type_name);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (idx, m) in i.methods.iter().enumerate() {
                if idx > 0 {
                    f.newline();
                    f.newline();
                }
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_const(&mut self, c: &ConstDef) {
        for attr in &c.attrs {
            match attr {
                ConstAttr::ForceStatic => self.write("@static "),
                ConstAttr::ForceInline => self.write("@inline "),
            }
        }
        self.write("const ");
        self.write(&c.name);
        self.write(" = ");
        self.fmt_expr(&c.value, Prec::OrFallback);
        self.write(";");
    }

    fn fmt_field(&mut self, field: &Field) {
        if field.is_stored_ref {
            self.write("ref");
            if let Some(label) = &field.stored_ref_label {
                self.write("[");
                self.write(label);
                self.write("]");
            }
            self.write(" ");
        }
        self.write(&field.name);
        self.write(": ");
        self.fmt_type(&field.ty);
    }

    fn fmt_block_stmts(&mut self, body: &[Stmt]) {
        for (i, stmt) in body.iter().enumerate() {
            if i > 0 {
                self.newline();
            }
            self.emit_leading(stmt_start(stmt));
            self.fmt_stmt(stmt);
            self.emit_trailing(stmt_end(stmt));
        }
    }

    fn fmt_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expr(e) => {
                self.fmt_expr(e, Prec::OrFallback);
                self.write(";");
            }
            Stmt::Val(b) => {
                self.fmt_binding(b);
                self.write(";");
            }
            Stmt::Assign {
                target,
                op,
                value,
                ..
            } => {
                self.fmt_lvalue(target);
                if let Some(op) = op {
                    self.write(compound_spell(*op));
                } else {
                    self.write(" =");
                }
                self.write(" ");
                self.fmt_expr(value, Prec::OrFallback);
                self.write(";");
            }
            Stmt::Return(expr, _) => {
                self.write("return");
                if let Some(e) = expr {
                    self.write(" ");
                    self.fmt_expr(e, Prec::OrFallback);
                }
                self.write(";");
            }
            Stmt::If(i) => self.fmt_if(i),
            Stmt::While { cond, body, .. } => {
                self.write("while ");
                self.fmt_expr(cond, Prec::OrFallback);
                self.write(" {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            Stmt::For {
                var,
                var2,
                kind,
                body,
                ..
            } => {
                self.write("for ");
                self.write(var);
                if let Some((v2, _)) = var2 {
                    self.write(", ");
                    self.write(v2);
                }
                self.write(" in ");
                match kind {
                    ForKind::Range { start, end } => {
                        self.fmt_expr(start, Prec::OrFallback);
                        self.write("..");
                        self.fmt_expr(end, Prec::OrFallback);
                    }
                    ForKind::In { collection } => {
                        self.fmt_expr(collection, Prec::OrFallback);
                    }
                }
                self.write(" {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                self.write("switch ");
                self.fmt_expr(subject, Prec::OrFallback);
                self.write(" {");
                self.newline();
                self.with_indent(|f| {
                    for arm in arms {
                        f.fmt_switch_arm(arm);
                        f.newline();
                    }
                    if let Some(else_b) = else_body {
                        f.write("else -> {");
                        f.newline();
                        f.with_indent(|f| f.fmt_block_stmts(else_b));
                        f.end_block();
                        f.write(";");
                    }
                });
                self.end_block();
            }
            Stmt::Break(_) => self.write("break;"),
            Stmt::Continue(_) => self.write("continue;"),
            Stmt::Loop(inner, _) => {
                self.write("loop {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(inner));
                self.end_block();
            }
            Stmt::Unsafe(inner, _) => {
                self.write("unsafe {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(inner));
                self.end_block();
            }
        }
    }

    fn fmt_if(&mut self, i: &IfStmt) {
        self.write("if ");
        self.fmt_expr(&i.cond, Prec::OrFallback);
        self.write(" {");
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(&i.then_body));
        self.end_block();
        if let Some(else_b) = &i.else_branch {
            self.write(" else ");
            match else_b {
                ElseBranch::ElseIf(inner) => self.fmt_if(inner),
                ElseBranch::Else(body) => {
                    self.write("{");
                    self.newline();
                    self.with_indent(|f| f.fmt_block_stmts(body));
                    self.end_block();
                }
            }
        }
    }

    fn fmt_switch_arm(&mut self, arm: &SwitchArm) {
        self.fmt_expr(&arm.cond, Prec::OrFallback);
        self.write(" -> {");
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(&arm.body));
        self.end_block();
        self.write(";");
    }

    fn fmt_binding(&mut self, b: &Binding) {
        self.write(if b.mutable {
            syntax::KW_VAR
        } else {
            syntax::KW_VAL
        });
        self.write(" ");
        self.write(&b.name);
        if let Some(ty) = &b.ty {
            self.write(": ");
            self.fmt_type(ty);
        }
        self.write(" = ");
        self.fmt_expr(&b.init, Prec::OrFallback);
    }

    fn fmt_lvalue(&mut self, lv: &LValue) {
        match lv {
            LValue::Local { name, .. } => self.write(name),
            LValue::Index { base, index, .. } => {
                self.fmt_expr(base, Prec::Postfix);
                self.write("[");
                self.fmt_expr(index, Prec::OrFallback);
                self.write("]");
            }
        }
    }

    fn fmt_type(&mut self, ty: &Type) {
        match ty {
            Type::Int => self.write(syntax::TYPE_INT),
            Type::Float => self.write(syntax::TYPE_FLOAT),
            Type::Bool => self.write(syntax::TYPE_BOOL),
            Type::String => self.write(syntax::TYPE_STRING),
            Type::Char => self.write(syntax::TYPE_CHAR),
            Type::List(inner) => {
                self.write("List<");
                self.fmt_type(inner);
                self.write(">");
            }
            Type::Map { key, value } => {
                self.write("Map<");
                self.fmt_type(key);
                self.write(", ");
                self.fmt_type(value);
                self.write(">");
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
                self.write("Result<");
                self.fmt_type(ok);
                self.write(", ");
                self.fmt_type(err);
                self.write(">");
            }
            Type::Named(n) => self.write(n),
        }
    }

    fn fmt_expr(&mut self, expr: &Expr, prec: Prec) {
        match expr {
            Expr::Str(parts, _) => self.fmt_str(parts),
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
            Expr::Slice { base, start, end, .. } => {
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
            Expr::Field(base, field, _) => {
                self.fmt_expr(base, Prec::Postfix);
                self.write(".");
                self.write(field);
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                ..
            } => {
                self.fmt_expr(receiver, Prec::Postfix);
                self.write(".");
                self.write(method);
                self.write("(");
                self.fmt_call_args(args);
                self.write(")");
            }
            Expr::StructLit {
                type_name,
                fields,
                ..
            } => {
                self.write(type_name);
                self.write(" {");
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
            Expr::PatternTest { subject, pattern, .. } => {
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
            Expr::Try(inner, _) => {
                self.fmt_expr(inner, Prec::Postfix);
                self.write("?");
            }
            Expr::OrFallback {
                value,
                fallback,
                ..
            } => {
                if prec > Prec::OrFallback {
                    self.write("(");
                }
                self.fmt_expr(value, Prec::OrFallback.add_rhs());
                self.write(" or ");
                self.fmt_or_fallback(fallback);
                if prec > Prec::OrFallback {
                    self.write(")");
                }
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

    fn fmt_pattern(&mut self, pat: &Pattern) {
        match pat {
            Pattern::Variant { variant, bindings, .. } => {
                self.write(variant);
                if !bindings.is_empty() {
                    self.write("(");
                    for (i, b) in bindings.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.write(b);
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
            match arg.convention {
                AccessConvention::Read => {}
                AccessConvention::Mutate => {
                    self.write("mut ");
                }
                AccessConvention::Move => {
                    self.write("take ");
                }
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
}

impl Prec {
    fn add_rhs(self) -> Self {
        match self {
            Prec::OrFallback => Prec::OrFallback,
            Prec::Or => Prec::And,
            Prec::And => Prec::Cmp,
            Prec::Cmp => Prec::BitOr,
            Prec::BitOr => Prec::BitXor,
            Prec::BitXor => Prec::BitAnd,
            Prec::BitAnd => Prec::Shift,
            Prec::Shift => Prec::Add,
            Prec::Add => Prec::Mul,
            Prec::Mul => Prec::Unary,
            Prec::Unary => Prec::Postfix,
            Prec::Postfix => Prec::Primary,
            Prec::Primary => Prec::Primary,
        }
    }
}

fn stmt_start(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Expr(e) => e.span().start,
        Stmt::Val(b) => b.name_span.start,
        Stmt::Assign { target, .. } => match target {
            LValue::Local { name_span, .. } => name_span.start,
            LValue::Index { span, .. } => span.start,
        },
        Stmt::Return(_, s) => s.start,
        Stmt::If(i) => i.span.start,
        Stmt::While { span, .. } | Stmt::For { span, .. } | Stmt::Switch { span, .. } => span.start,
        Stmt::Break(s) | Stmt::Continue(s) => s.start,
        Stmt::Loop(_, s) | Stmt::Unsafe(_, s) => s.start,
    }
}

fn compound_spell(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+=",
        BinOp::Sub => "-=",
        BinOp::Mul => "*=",
        BinOp::Div => "/=",
        BinOp::Rem => "%=",
        BinOp::BitAnd => "&=",
        BinOp::BitOr => "|=",
        BinOp::BitXor => "^=",
        BinOp::Shl => "<<=",
        BinOp::Shr => ">>=",
        _ => unreachable!("compound assignment uses arithmetic/bit ops only"),
    }
}

fn fmt_float(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{:.1}", v)
    } else {
        format!("{}", v)
    }
}

fn fmt_char(c: char) -> String {
    match c {
        '\'' => "'\\''".to_string(),
        '\\' => "'\\\\'".to_string(),
        '\n' => "'\\n'".to_string(),
        '\t' => "'\\t'".to_string(),
        '\r' => "'\\r'".to_string(),
        c if c.is_ascii() && !c.is_control() => format!("'{}'", c),
        c => format!("'\\u{{{:x}}}'", c as u32),
    }
}

fn escape_str_lit(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c == '{' || c == '}' => {
                out.push(c);
                out.push(c);
            }
            c if c.is_control() => out.push_str(&format!("\\u{{{}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Lex + parse + format. Parse errors propagate; sema is not required.
pub fn format_source(src: &str) -> Result<String, Vec<crate::diag::Diagnostic>> {
    let (toks, lex_diags) = crate::lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let comments = line_comments(&toks);
    let prog = crate::parser::parse(&toks)?;
    Ok(format_program(&prog, src, &comments))
}

/// Simple unified diff for `jet fmt --check`.
pub fn unified_diff(path: &str, old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut out = format!("--- {path}\n+++ {path}\n");
    let mut i = 0usize;
    let mut j = 0usize;
    while i < old_lines.len() || j < new_lines.len() {
        if i < old_lines.len() && j < new_lines.len() && old_lines[i] == new_lines[j] {
            i += 1;
            j += 1;
            continue;
        }
        let start_i = i;
        let start_j = j;
        while i < old_lines.len()
            && (j >= new_lines.len() || old_lines[i] != new_lines.get(j).copied().unwrap_or(""))
        {
            i += 1;
        }
        while j < new_lines.len()
            && (i >= old_lines.len() || new_lines[j] != old_lines.get(i).copied().unwrap_or(""))
        {
            j += 1;
        }
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            start_i + 1,
            i - start_i,
            start_j + 1,
            j - start_j
        ));
        for k in start_i..i {
            out.push_str(&format!("-{}\n", old_lines[k]));
        }
        for k in start_j..j {
            out.push_str(&format!("+{}\n", new_lines[k]));
        }
    }
    out
}
