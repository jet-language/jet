//! Pretty-printer for Jet source (M6 phase 1, S44).
//!
//! One true style: 4-space indent, same-line `{`, spaces around binary
//! operators, semicolons on statements. Line width is not enforced in v1
//! (S44 width-100 may land with optional org config later). Comments are
//! preserved from the original source and re-attached by span.

use crate::ast::{
    AccessConvention, BinOp, BindPattern, Binding, Call, CallArg, ConstAttr, ConstDef, ElseBranch,
    EnumDef,
    EnumLitArg, Expr, ExternFn, ExternRustBlock, Field, ForKind, Func, IfStmt, ImplDef, ImportDecl,
    ImportKind, Item, LValue, OrFallback, Param, Pattern, Program, Stmt, StrPart, StructDef,
    SwitchArm, TraitImplBlock, Type, TypeParam, UnOp, Variant, VariantPayload,
};
use crate::diag::Span;
use crate::lexer::{comments, TokKind, Token};
use crate::syntax;

const INDENT: usize = 4;

/// Format a parsed program back to canonical Jet source.
pub fn format_program(prog: &Program, src: &str, comment_toks: &[Token]) -> String {
    let comments: Vec<Comment> = comment_toks
        .iter()
        .map(|t| Comment {
            text: match &t.kind {
                TokKind::LineComment(s) | TokKind::BlockComment(s) => s.clone(),
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
    let mut first = true;
    for imp in &prog.imports {
        if !first {
            f.blank_line_between_items();
        }
        first = false;
        f.emit_leading(imp.span.start);
        f.fmt_import(imp);
        f.emit_trailing(imp.span.end);
    }
    for item in &prog.items {
        if !first {
            f.blank_line_between_items();
        }
        first = false;
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
        Item::Impl(i) => src[..i.type_span.start]
            .rfind("impl")
            .unwrap_or(i.type_span.start),
        Item::Const(c) => {
            let kw = if c.is_comptime {
                syntax::KW_COMPTIME
            } else {
                syntax::KW_CONST
            };
            src[..c.name_span.start]
                .rfind(kw)
                .unwrap_or(c.name_span.start)
        }
        Item::Test(t) => src[..t.name_span.start]
            .rfind(syntax::KW_TEST)
            .unwrap_or(t.name_span.start),
        Item::ExternRust(b) => src[..b.crate_span.start]
            .rfind(syntax::KW_EXTERN)
            .unwrap_or(b.span.start),
        Item::Trait(t) => type_decl_start(t.is_pub, t.name_span.start, "trait", src),
        Item::Module(m) => src[..m.name_span.start]
            .rfind(syntax::KW_MODULE)
            .unwrap_or(m.span.start),
        // S59: the `@extern`/`@bindgen` attribute precedes the span start.
        Item::CModule(cm) => cm.span.start,
        Item::CodeModule(cm) => src[..cm.name_span.start]
            .rfind(syntax::KW_MODULE)
            .unwrap_or(cm.span.start),
    }
}

fn func_decl_start(f: &Func, src: &str) -> usize {
    let before = &src[..f.name_span.start];
    let fn_pos = before.rfind("fn").unwrap_or(f.name_span.start);
    if f.is_pub {
        before[..fn_pos].rfind("pub").unwrap_or(fn_pos)
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
        Item::Struct(s) => s
            .methods
            .last()
            .map(|m| m.body.last().map(stmt_end).unwrap_or(m.name_span.end))
            .or_else(|| s.fields.last().map(|fld| fld.ty_span.end))
            .unwrap_or(s.name_span.end),
        Item::Enum(e) => e
            .methods
            .last()
            .map(|m| m.body.last().map(stmt_end).unwrap_or(m.name_span.end))
            .or_else(|| e.variants.last().map(|v| v.name_span.end))
            .unwrap_or(e.name_span.end),
        Item::Impl(i) => i
            .methods
            .last()
            .map(|m| m.body.last().map(stmt_end).unwrap_or(m.name_span.end))
            .unwrap_or(i.type_span.end),
        Item::Const(c) => c.value.span().end,
        Item::Test(t) => t.body.last().map(stmt_end).unwrap_or(t.name_span.end),
        Item::ExternRust(b) => b.span.end,
        Item::Trait(t) => t
            .methods
            .last()
            .map(|m| m.span.end)
            .unwrap_or(t.name_span.end),
        Item::Module(m) => m.span.end,
        Item::CModule(cm) => cm.span.end,
        Item::CodeModule(cm) => cm.span.end,
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
        Stmt::Switch {
            else_body, arms, ..
        } => else_body
            .as_ref()
            .and_then(|b| b.last())
            .map(stmt_end)
            .or_else(|| {
                arms.last()
                    .map(|a| a.body.last().map(stmt_end).unwrap_or(a.span.end))
            })
            .unwrap_or(0),
        Stmt::Break(s) | Stmt::Continue(s) | Stmt::BreakLabel(_, s) | Stmt::ContinueLabel(_, s) => s.end,
        Stmt::Loop { body: inner, span: s, .. } => inner.last().map(stmt_end).unwrap_or(s.end),
        Stmt::Unsafe { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
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
        // Start the line without a spurious leading newline at BOF (S44 has no
        // "blank line before first item" rule) and without doubling the break
        // when a previous comment already left us at the line start.
        if !self.out.is_empty() && !self.at_line_start {
            self.newline();
        }
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

    /// S69 (D-SG3): did the author put a line break before this chain step's
    /// `.`? `from` is the receiver's end offset, `to` is the method/field's
    /// start offset; a `\n` in between means the chain was broken on purpose.
    fn chain_break_between(&self, from: usize, to: usize) -> bool {
        from <= to
            && self
                .src
                .get(from..to)
                .is_some_and(|s| s.contains('\n'))
    }

    fn fmt_item(&mut self, item: &Item) {
        match item {
            Item::Func(f) => self.fmt_func(f, true),
            Item::Struct(s) => self.fmt_struct(s, true),
            Item::Enum(e) => self.fmt_enum(e, true),
            Item::Impl(i) => self.fmt_impl(i),
            Item::Const(c) => self.fmt_const(c),
            Item::Test(t) => self.fmt_test(t),
            Item::ExternRust(b) => self.fmt_extern_rust(b),
            Item::Trait(t) => self.fmt_trait(t),
            // Stage 1a: modules are emitted verbatim (non-destructive). A
            // canonical module formatter lands with the eval pipeline.
            Item::Module(m) => {
                let text = self.src[m.span.start..m.span.end].to_string();
                self.write(&text);
            }
            // S59: C FFI modules are emitted verbatim (non-destructive). A
            // canonical formatter can land alongside the bind backend.
            Item::CModule(cm) => {
                let text = self.src[cm.span.start..cm.span.end].to_string();
                self.write(&text);
            }
            // Code modules are emitted verbatim pending a dedicated formatter.
            Item::CodeModule(cm) => {
                let text = self.src[cm.span.start..cm.span.end].to_string();
                self.write(&text);
            }
        }
    }

    fn fmt_trait(&mut self, t: &crate::ast::TraitDef) {
        if t.is_pub {
            self.write("pub ");
        }
        self.write("trait ");
        self.write(&t.name);
        self.write(" ");
        self.write(syntax::BLOCK_OPEN);
        self.newline();
        self.with_indent(|f| {
            for m in &t.methods {
                f.write("fn ");
                f.write(&m.name);
                f.write("(");
                for (i, p) in m.params.iter().enumerate() {
                    if i > 0 {
                        f.write(", ");
                    }
                    f.fmt_param(p);
                }
                f.write(")");
                if let Some(ret) = &m.return_type {
                    f.write(" -> ");
                    f.fmt_return_type(ret);
                }
                f.newline();
            }
        });
        self.write(syntax::BLOCK_CLOSE);
        self.newline();
        self.newline();
    }

    fn fmt_extern_rust(&mut self, block: &ExternRustBlock) {
        self.write(syntax::KW_EXTERN);
        self.write(" ");
        self.write(syntax::KW_RUST);
        self.write(" \"");
        self.write(&block.crate_spec);
        self.write("\" ");
        self.write(syntax::BLOCK_OPEN);
        self.newline();
        self.with_indent(|f| {
            for ef in &block.functions {
                f.fmt_extern_fn(ef);
            }
        });
        self.end_block();
    }

    fn fmt_extern_fn(&mut self, ef: &ExternFn) {
        self.write("fn ");
        self.write(&ef.name);
        self.write("(");
        for (i, p) in ef.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.fmt_param(p);
        }
        self.write(")");
        if let Some(ret) = &ef.return_type {
            self.write(" -> ");
            if ef.is_view_return {
                self.write("view ");
            }
            self.fmt_return_type(ret);
        }
        self.write(" = \"");
        self.write(&ef.rust_path);
        self.write("\"");
        self.write(syntax::STMT_SEP);
        self.newline();
    }

    fn fmt_test(&mut self, t: &crate::ast::TestDef) {
        self.write(syntax::KW_TEST);
        self.write(" ");
        self.write("\"");
        self.write(&t.name.replace('\\', "\\\\").replace('"', "\\\""));
        self.write("\"");
        self.write(" ");
        self.write(syntax::BLOCK_OPEN);
        self.newline();
        self.with_indent(|f| {
            for stmt in &t.body {
                f.fmt_stmt(stmt);
            }
        });
        self.end_block();
    }

    fn fmt_pub(&mut self, is_pub: bool) {
        if is_pub {
            self.write("pub ");
        }
    }

    fn fmt_type_params(&mut self, params: &[TypeParam]) {
        self.write(&crate::generics::format_type_params(params));
    }

    fn fmt_derive_line(&mut self, trait_name: &str) {
        self.write("derive ");
        self.write(trait_name);
    }

    fn fmt_trait_impl_block(&mut self, block: &TraitImplBlock) {
        self.write("impl ");
        self.write(&block.trait_name);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (i, m) in block.methods.iter().enumerate() {
                if i > 0 {
                    f.newline();
                    f.newline();
                }
                f.fmt_func(m, false);
            }
        });
        self.end_block();
    }

    fn fmt_func(&mut self, f: &Func, top_level: bool) {
        // S58 (E2-M13): `@unsafe` whole-function contract sits on its own line.
        if f.is_unsafe {
            self.write(&format!("@{}", syntax::KW_UNSAFE));
            self.newline();
        }
        if top_level {
            self.fmt_pub(f.is_pub);
        } else if f.is_pub {
            self.write("pub ");
        }
        // S60 (E2-M16): `pure fn` modifier.
        if f.is_pure {
            self.write("pure ");
        }
        self.write("fn ");
        self.write(&f.name);
        self.fmt_type_params(&f.type_params);
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
            self.fmt_return_type(ret);
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
        self.fmt_type_params(&s.type_params);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (i, field) in s.fields.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.fmt_field(field);
            }
            for (i, (trait_name, _)) in s.derives.iter().enumerate() {
                if i > 0 || !s.fields.is_empty() {
                    f.newline();
                }
                f.fmt_derive_line(trait_name);
            }
            for (i, block) in s.trait_impls.iter().enumerate() {
                if i > 0 || !s.fields.is_empty() || !s.derives.is_empty() {
                    f.newline();
                    f.newline();
                }
                f.fmt_trait_impl_block(block);
            }
            for (i, m) in s.methods.iter().enumerate() {
                if i > 0
                    || !s.fields.is_empty()
                    || !s.derives.is_empty()
                    || !s.trait_impls.is_empty()
                {
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
        self.fmt_type_params(&e.type_params);
        self.write(" {");
        self.newline();
        self.with_indent(|f| {
            for (i, v) in e.variants.iter().enumerate() {
                if i > 0 {
                    f.newline();
                }
                f.fmt_variant(v);
            }
            for (i, (trait_name, _)) in e.derives.iter().enumerate() {
                if i > 0 || !e.variants.is_empty() {
                    f.newline();
                }
                f.fmt_derive_line(trait_name);
            }
            for (i, block) in e.trait_impls.iter().enumerate() {
                if i > 0 || !e.variants.is_empty() || !e.derives.is_empty() {
                    f.newline();
                    f.newline();
                }
                f.fmt_trait_impl_block(block);
            }
            for (i, m) in e.methods.iter().enumerate() {
                if i > 0
                    || !e.variants.is_empty()
                    || !e.derives.is_empty()
                    || !e.trait_impls.is_empty()
                {
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
        if let Some(tr) = &i.trait_name {
            self.write(": ");
            self.write(tr);
        }
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
        if c.is_comptime {
            self.write(syntax::KW_COMPTIME);
            self.write(" ");
            self.write(&c.name);
            self.write(" = ");
            self.fmt_expr(&c.value, Prec::OrFallback);
            return;
        }
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
    }

    fn fmt_import(&mut self, imp: &ImportDecl) {
        if imp.is_pub {
            self.write("pub ");
        }
        self.write(syntax::KW_USE);
        self.write(" ");
        match &imp.kind {
            ImportKind::File(path, _) => {
                self.write("\"");
                self.write(path);
                self.write("\"");
                let default_alias = path.rsplit('/').next().unwrap_or("module");
                if imp.alias != default_alias {
                    self.write(" ");
                    self.write(syntax::KW_AS);
                    self.write(" ");
                    self.write(&imp.alias);
                }
            }
            ImportKind::Module(name, _) => {
                self.write(name);
                let default_alias = name.rsplit('.').next().unwrap_or(name.as_str());
                if imp.alias != default_alias {
                    self.write(" ");
                    self.write(syntax::KW_AS);
                    self.write(" ");
                    self.write(&imp.alias);
                }
            }
            ImportKind::Unqualified { module_alias, items, .. } => {
                if items.len() == 1 {
                    self.write(module_alias);
                    self.write(".");
                    self.write(&items[0]);
                } else {
                    self.write(module_alias);
                    self.write(".{");
                    self.write(&items.join(", "));
                    self.write("}");
                }
            }
        }
    }

    fn fmt_field(&mut self, field: &Field) {
        if field.is_pub {
            self.write("pub ");
        }
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
            }
            Stmt::Val(b) => {
                self.fmt_binding(b);
            }
            Stmt::Assign {
                target, op, value, ..
            } => {
                self.fmt_lvalue(target);
                if let Some(op) = op {
                    self.write(compound_spell(*op));
                } else {
                    self.write(" =");
                }
                self.write(" ");
                self.fmt_expr(value, Prec::OrFallback);
            }
            Stmt::Return(expr, _) => {
                self.write("return");
                if let Some(e) = expr {
                    self.write(" ");
                    self.fmt_expr(e, Prec::OrFallback);
                }
            }
            Stmt::If(i) => self.fmt_if(i),
            Stmt::While { cond, body, label, .. } => {
                // S19: the canonical loop keyword is `loop` (a parsed `loop cond`
                // becomes a `While` node). D-LABEL1: print the `@name` label.
                if let Some((_n, _)) = label {
                    self.write(&format!("@{} ", _n));
                }
                self.write("loop ");
                self.fmt_cond(cond);
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
                label,
                ..
            } => {
                if let Some((_n, _)) = label {
                    self.write(&format!("@{} ", _n));
                }
                self.write("loop ");
                self.write(var);
                if let Some((v2, _)) = var2 {
                    self.write(", ");
                    self.write(v2);
                }
                self.write(" in ");
                match kind {
                    ForKind::Range { start, end, step } => {
                        self.fmt_expr(start, Prec::OrFallback);
                        self.write("..");
                        self.fmt_expr(end, Prec::OrFallback);
                        if let Some(step) = step {
                            self.write(&format!(" {} ", syntax::KW_RANGE_STEP));
                            self.fmt_expr(step, Prec::OrFallback);
                        }
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
            // D-IF1: multi-arm dispatch renders as `if subject { head -> body }`
            // (the `Stmt::Switch` IR is shared with the retired `when`).
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                self.write(syntax::KW_IF);
                self.write(" ");
                self.fmt_expr(subject, Prec::OrFallback);
                self.write(" {");
                self.newline();
                self.with_indent(|f| {
                    for arm in arms {
                        f.fmt_switch_arm(subject, arm);
                        f.newline();
                    }
                    if let Some(else_b) = else_body {
                        f.write(syntax::KW_ELSE);
                        f.write(" ");
                        f.write(syntax::OP_ARM_ARROW);
                        f.write(" {");
                        f.newline();
                        f.with_indent(|f| f.fmt_block_stmts(else_b));
                        f.end_block();
                    }
                });
                self.end_block();
            }
            Stmt::Break(_) => self.write("break"),
            Stmt::Continue(_) => self.write("continue"),
            Stmt::BreakLabel(name, _) => self.write(&format!("break @{}", name)),
            Stmt::ContinueLabel(name, _) => self.write(&format!("continue @{}", name)),
            Stmt::Loop { body: inner, label, .. } => {
                if let Some((_n, _)) = label {
                    self.write(&format!("@{} ", _n));
                }
                self.write("loop {");
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(inner));
                self.end_block();
            }
            Stmt::Unsafe { audit, body, .. } => {
                if let Some(reason) = audit {
                    self.write(&format!("@{}(\"{}\")", syntax::ATTR_AUDIT, reason));
                    self.newline();
                }
                self.write(&format!("@{} {{", syntax::KW_UNSAFE));
                self.newline();
                self.with_indent(|f| f.fmt_block_stmts(body));
                self.end_block();
            }
        }
    }

    /// S68 (D-SG2): render an `if`/`while` condition without wrapping the
    /// outermost expression in redundant parens. Precedence-required parens on
    /// nested sub-expressions are preserved by the normal `fmt_expr` rules.
    fn fmt_cond(&mut self, cond: &Expr) {
        self.fmt_expr(cond, Prec::Primary);
    }

    /// S68 (D-SG2): render an `if`-expression branch `{ stmts… value }`.
    fn fmt_value_block(&mut self, stmts: &[Stmt], value: &Expr) {
        self.write("{");
        self.newline();
        self.with_indent(|f| {
            f.fmt_block_stmts(stmts);
            if !stmts.is_empty() {
                f.newline();
            }
            f.fmt_expr(value, Prec::OrFallback);
        });
        self.end_block();
    }

    fn fmt_if(&mut self, i: &IfStmt) {
        self.write("if ");
        // S68 (D-SG2): conditions use the no-paren house style — the outer
        // redundant parens of `if (cond)` are stripped.
        self.fmt_cond(&i.cond);
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

    /// D-IF1: render one arm as `head -> { body }`. A bare-value arm
    /// (`subject == value`) prints just the value; a full condition prints as
    /// written. A single-statement body could be braceless, but fmt always uses
    /// a block for a stable, idempotent shape.
    fn fmt_switch_arm(&mut self, subject: &Expr, arm: &SwitchArm) {
        self.fmt_switch_cond(subject, &arm.cond, Prec::OrFallback);
        self.write(" ");
        self.write(syntax::OP_ARM_ARROW);
        self.write(" {");
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(&arm.body));
        self.end_block();
    }

    fn fmt_switch_cond(&mut self, subject: &Expr, cond: &Expr, prec: Prec) {
        match cond {
            Expr::Binary(op @ (BinOp::And | BinOp::Or), lhs, rhs, _) => {
                let my_prec = Prec::of_bin(*op);
                let needs_paren = prec > my_prec;
                if needs_paren {
                    self.write("(");
                }
                self.fmt_switch_cond(subject, lhs, my_prec);
                self.write(" ");
                self.write(op.spell());
                self.write(" ");
                self.fmt_switch_cond(subject, rhs, my_prec.add_rhs());
                if needs_paren {
                    self.write(")");
                }
            }
            Expr::Binary(BinOp::Eq, lhs, rhs, _) if self.same_subject(lhs, subject) => {
                self.fmt_expr(rhs, Prec::Cmp);
            }
            Expr::PatternTest {
                subject: lhs,
                pattern,
                ..
            } => {
                // A pattern arm keeps its `subject == pattern` shape: the bare
                // pattern would re-parse as a value comparison and drop the
                // binding names (e.g. `| ok(n)` wouldn't bind `n`). When the arm
                // repeats the subject, collapse it to `it` so the subject is
                // evaluated once and stays exhaustiveness-checkable (S24).
                if self.same_subject(lhs, subject) {
                    self.write(syntax::KW_IT);
                } else {
                    self.fmt_expr(lhs, Prec::Cmp);
                }
                self.write(" == ");
                self.fmt_pattern(pattern);
            }
            _ => self.fmt_expr(cond, prec),
        }
    }

    /// True when `a` denotes the `when` subject: either the `it` placeholder or
    /// an expression with byte-for-byte the same source text as `subject`.
    fn same_subject(&self, a: &Expr, subject: &Expr) -> bool {
        if let Expr::Ident(name, _) = a {
            if name == syntax::KW_IT {
                return true;
            }
        }
        let a_src = self.src.get(a.span().start..a.span().end);
        let subj_src = self.src.get(subject.span().start..subject.span().end);
        matches!((a_src, subj_src), (Some(x), Some(y)) if x == y)
    }

    fn fmt_binding(&mut self, b: &Binding) {
        // S57: comptime stays keyword-led (`comptime NAME = …`). D-BIND1: ordinary
        // bindings are sigil-led (`name :: …` / `name := …`), no leading keyword.
        if b.is_comptime {
            self.write(syntax::KW_COMPTIME);
            self.write(" ");
            self.write(&b.name);
            self.write(" = ");
            self.fmt_expr(&b.init, Prec::OrFallback);
            return;
        }
        if let Some(pat) = &b.pattern {
            // S74: a destructuring target stands in for the name.
            self.fmt_bind_pattern(pat);
        } else {
            self.write(&b.name);
            if let Some(ty) = &b.ty {
                self.write(": ");
                self.fmt_type(ty);
            }
        }
        self.write(" ");
        self.write(if b.mutable {
            syntax::SIGIL_BIND_MUT
        } else {
            syntax::SIGIL_BIND_IMMUT
        });
        self.write(" ");
        self.fmt_expr(&b.init, Prec::OrFallback);
    }

    fn fmt_bind_pattern(&mut self, pat: &BindPattern) {
        match pat {
            BindPattern::Struct {
                type_name, fields, ..
            } => {
                self.write(type_name);
                self.write(" { ");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&f.name);
                }
                self.write(" }");
            }
            BindPattern::List { elems, .. } => {
                self.write("[");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&e.name);
                }
                self.write("]");
            }
            BindPattern::Tuple { elems, .. } => {
                self.write("(");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(&e.name);
                }
                self.write(")");
            }
        }
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

    fn fmt_return_type(&mut self, ty: &Type) {
        if let Type::Result { ok, err } = ty {
            self.fmt_type(ok);
            self.write(" ?");
            if !matches!(**err, Type::Named(ref n) if n == syntax::TYPE_ERROR) {
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

    fn fmt_expr(&mut self, expr: &Expr, prec: Prec) {
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
                self.write(&format!(".{}<", syntax::TYPE_PTR));
                self.fmt_type(elem);
                self.write(&format!(">.{}(", syntax::MEM_FROM_ADDR));
                self.fmt_expr(addr, Prec::OrFallback);
                self.write(")");
            }
        }
    }

    fn fmt_lambda(&mut self, lam: &crate::ast::Lambda) {
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
            crate::ast::LambdaBody::Expr(e) => self.fmt_expr(e, Prec::OrFallback.add_rhs()),
            crate::ast::LambdaBody::Block(stmts) => {
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

    fn fmt_pattern(&mut self, pat: &Pattern) {
        match pat {
            Pattern::Variant {
                variant, bindings, ..
            } => {
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
        Stmt::Break(s) | Stmt::Continue(s) | Stmt::BreakLabel(_, s) | Stmt::ContinueLabel(_, s) => s.start,
        Stmt::Loop { span: s, .. } => s.start,
        Stmt::Unsafe { span, .. } => span.start,
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

/// Escape a single content line of a triple-quoted string (S70). Newlines are
/// real line breaks here, so only `\`, the interpolation braces, and other
/// control characters need escaping; quotes and tabs stay literal.
fn escape_str_multiline(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push('\t'),
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
    let comments = comments(&toks);
    let prog = crate::parser::parse_for_fmt(&toks)?;
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
