//! Parser: tokens -> AST. Hand-written recursive descent with statement-
//! level error recovery (M1): one run reports every parse problem it can.
//!
//! Teaching errors (S14): familiar foreign spellings (`def`, `let`, `set`,
//! `and`, `try`, `match`, …) are recognized here only to emit an error
//! naming the canonical Jet form — then parsing continues as if the
//! canonical form had been written, so one foreign word doesn't hide the
//! rest of the file's problems.

use crate::ast::{
    AccessConvention, BinOp, Binding, Call, CallArg, ConstAttr, ConstDef, ElseBranch, EnumDef,
    EnumLitArg, Expr, Field, Func, IfStmt, ImplDef, Item, Param, Pattern, Program, Stmt, StrPart,
    StructDef, SwitchArm, Type, UnOp, Variant, VariantField, VariantPayload,
};
use crate::diag::{Diagnostic, Span};
use crate::lexer::{describe, StrTokPart, TokKind, Token};
use crate::syntax;

pub fn parse(toks: &[Token]) -> Result<Program, Vec<Diagnostic>> {
    let mut p = Parser {
        toks,
        pos: 0,
        diags: Vec::new(),
    };
    let prog = p.program();
    if p.diags.is_empty() {
        Ok(prog)
    } else {
        Err(p.diags)
    }
}

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    diags: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn peek2(&self) -> &Token {
        &self.toks[(self.pos + 1).min(self.toks.len() - 1)]
    }

    fn bump(&mut self) -> Token {
        let t = self.peek().clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn peek_is_ident(&self, name: &str) -> bool {
        matches!(&self.peek().kind, TokKind::Ident(n) if n == name)
    }

    // --- recovery ------------------------------------------------------

    /// After a failed top-level item: skip to the next plausible item start.
    fn sync_top(&mut self) {
        loop {
            match self.peek().kind {
                TokKind::Eof
                | TokKind::KwFn
                | TokKind::KwPub
                | TokKind::KwStruct
                | TokKind::KwEnum
                | TokKind::KwImpl
                | TokKind::KwConst => return,
                _ => {
                    self.bump();
                }
            }
        }
    }

    /// After a failed statement: skip to just past the next `;` at this
    /// brace depth, or stop before the block's closing `}`.
    fn sync_stmt(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.peek().kind {
                TokKind::Eof => return,
                TokKind::LBrace => {
                    depth += 1;
                    self.bump();
                }
                TokKind::RBrace => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                    self.bump();
                }
                TokKind::Semi => {
                    self.bump();
                    if depth == 0 {
                        return;
                    }
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    // --- items ----------------------------------------------------------

    /// S16 (ratified, staged M6): parse and reject `import` with E0019.
    fn import_staged(&mut self) {
        let start = self.bump().span;
        while !matches!(self.peek().kind, TokKind::Semi | TokKind::Eof) {
            self.bump();
        }
        if matches!(self.peek().kind, TokKind::Semi) {
            self.bump();
        }
        self.diags.push(Diagnostic::error(
            "E0019",
            format!("`{}` doesn't work yet", syntax::KW_IMPORT),
            "multi-file programs arrive in M6 — the import forms are already decided (S16)"
                .to_string(),
            format!(
                "keep everything in one file for now; later: `{} \"path\";`, `{} name;`, or add `{} alias`",
                syntax::KW_IMPORT,
                syntax::KW_IMPORT,
                syntax::KW_AS
            ),
            Some(start),
        ));
    }

    fn program(&mut self) -> Program {
        let mut items = Vec::new();
        loop {
            let r = match &self.peek().kind {
                TokKind::Eof => break,
                TokKind::KwFn | TokKind::KwPub => self.func().map(Item::Func),
                TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                TokKind::KwImpl => self.impl_def().map(Item::Impl),
                TokKind::KwConst | TokKind::At => self.const_def().map(Item::Const),
                TokKind::Ident(name) if name == syntax::FOREIGN_CLASS => {
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0021",
                        format!(
                            "types are written with `{}`, not `{}`",
                            syntax::KW_STRUCT,
                            syntax::FOREIGN_CLASS
                        ),
                        format!(
                            "{} uses exactly one spelling for each thing, so all code reads the same",
                            syntax::LANG_NAME
                        ),
                        format!(
                            "replace `{}` with `{}`",
                            syntax::FOREIGN_CLASS,
                            syntax::KW_STRUCT
                        ),
                        Some(t.span),
                    ));
                    self.struct_def(false).map(Item::Struct)
                }
                TokKind::Ident(name)
                    if name == syntax::FOREIGN_INTERFACE || name == syntax::FOREIGN_TRAIT =>
                {
                    let t = self.bump();
                    let foreign = if let TokKind::Ident(n) = &t.kind {
                        n.clone()
                    } else {
                        unreachable!()
                    };
                    self.diags.push(Diagnostic::error(
                        "E0022",
                        format!("`{}` doesn't work yet", foreign),
                        "traits and interfaces arrive in M9 — for now, use structs and enums"
                            .to_string(),
                        format!(
                            "remove `{}` for now, or define a `struct` / `enum` instead",
                            foreign
                        ),
                        Some(t.span),
                    ));
                    self.sync_top();
                    continue;
                }
                TokKind::Ident(name)
                    if name == syntax::FOREIGN_DEF || name == syntax::FOREIGN_FUNC =>
                {
                    // S14 teaching error E0008, then parse as if `fn`.
                    let t = self.bump();
                    let foreign = if let TokKind::Ident(n) = &t.kind {
                        n.clone()
                    } else {
                        unreachable!()
                    };
                    self.diags.push(Diagnostic::error(
                        "E0008",
                        format!(
                            "functions are written with `{}`, not `{}`",
                            syntax::KW_FN,
                            foreign
                        ),
                        "Jet has exactly one spelling for each thing, so all code reads the same"
                            .to_string(),
                        format!("replace `{}` with `{}`", foreign, syntax::KW_FN),
                        Some(t.span),
                    ));
                    self.func_after_fn(false).map(Item::Func)
                }
                TokKind::Ident(name) if name == syntax::FOREIGN_USE => {
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0015",
                        format!("{} does not use `{}`", syntax::LANG_NAME, syntax::FOREIGN_USE),
                        format!(
                            "other files are brought in with `{} \"path\"` or `{} name` (S16; M6)",
                            syntax::KW_IMPORT,
                            syntax::KW_IMPORT
                        ),
                        format!(
                            "replace with `{} \"path\";`, `{} name;`, or `{} \"path\" {} alias;`",
                            syntax::KW_IMPORT,
                            syntax::KW_IMPORT,
                            syntax::KW_IMPORT,
                            syntax::KW_AS
                        ),
                        Some(t.span),
                    ));
                    self.sync_stmt();
                    continue;
                }
                TokKind::KwImport => {
                    self.import_staged();
                    continue;
                }
                other => {
                    let d = Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}`, `{}`, or `{}` here, found {}",
                            syntax::KW_FN,
                            syntax::KW_STRUCT,
                            syntax::KW_CONST,
                            describe(other)
                        ),
                        "at the top level of a file, only definitions can appear".to_string(),
                        format!(
                            "define a function ({} main() {{ ... }}), struct, or const",
                            syntax::KW_FN
                        ),
                        Some(self.peek().span),
                    );
                    self.diags.push(d);
                    self.bump();
                    self.sync_top();
                    continue;
                }
            };
            match r {
                Ok(item) => items.push(item),
                Err(d) => {
                    self.diags.push(d);
                    self.sync_top();
                }
            }
        }
        Program { items }
    }

    fn func(&mut self) -> Result<Func, Diagnostic> {
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "to start a function definition")?;
        self.func_after_fn(is_pub)
    }

    fn func_after_fn(&mut self, is_pub: bool) -> Result<Func, Diagnostic> {
        let (name, name_span) = self.expect_ident("after `fn`")?;
        self.expect(TokKind::LParen, "after the function name")?;
        let mut params = Vec::new();
        if !matches!(self.peek().kind, TokKind::RParen) {
            loop {
                params.push(self.param()?);
                if matches!(self.peek().kind, TokKind::RParen) {
                    break;
                }
                self.expect(TokKind::Comma, "between parameters")?;
            }
        }
        self.expect(TokKind::RParen, "to close the parameter list")?;

        let mut return_type = None;
        let mut is_view_return = false;
        if matches!(self.peek().kind, TokKind::Arrow) {
            self.bump();
            if matches!(self.peek().kind, TokKind::KwView) {
                is_view_return = true;
                self.bump();
            }
            let (ty, _) = self.type_()?;
            return_type = Some(ty);
        }

        self.expect(TokKind::LBrace, "to open the function body")?;
        let body = self.block_stmts();
        Ok(Func {
            is_pub,
            name,
            name_span,
            params,
            return_type,
            is_view_return,
            body,
        })
    }

    fn param(&mut self) -> Result<Param, Diagnostic> {
        let convention = self.parse_access_prefix();
        let (name, name_span) = if matches!(self.peek().kind, TokKind::KwSelf) {
            let span = self.bump().span;
            (syntax::KW_SELF.to_string(), span)
        } else {
            self.expect_ident("for a parameter name")?
        };
        let (ty, ty_span) = if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            self.type_()?
        } else if name == syntax::KW_SELF {
            // S27: receiver type is the owning struct/enum; sema fills it in.
            (Type::Named(String::new()), name_span)
        } else {
            return Err(Diagnostic::error(
                "E0003",
                format!("expected `:` after the parameter `{}`", name),
                "every parameter except `self` needs a type after its name".to_string(),
                format!("write `{}: Type`", name),
                Some(name_span),
            ));
        };
        Ok(Param {
            convention,
            name,
            name_span,
            ty,
            ty_span,
        })
    }

    fn struct_def(&mut self, nested: bool) -> Result<StructDef, Diagnostic> {
        let is_pub = if nested {
            false
        } else {
            matches!(self.peek().kind, TokKind::KwPub)
        };
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwStruct, "to start a struct definition")?;
        let (name, name_span) = self.expect_ident("after `struct`")?;
        self.expect(TokKind::LBrace, "to open the struct body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::KwFn | TokKind::KwPub) {
                methods.push(self.method_in_type()?);
            } else {
                fields.push(self.field()?);
                if matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                    self.bump();
                }
            }
        }
        self.bump(); // }
        Ok(StructDef {
            is_pub,
            name,
            name_span,
            fields,
            methods,
        })
    }

    fn enum_def(&mut self, nested: bool) -> Result<EnumDef, Diagnostic> {
        let is_pub = if nested {
            false
        } else {
            matches!(self.peek().kind, TokKind::KwPub)
        };
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwEnum, "to start an enum definition")?;
        let (name, name_span) = self.expect_ident("after `enum`")?;
        self.expect(TokKind::LBrace, "to open the enum body")?;
        let mut variants = Vec::new();
        let mut methods = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::KwFn | TokKind::KwPub) {
                methods.push(self.method_in_type()?);
            } else {
                variants.push(self.variant()?);
                if matches!(self.peek().kind, TokKind::Semi) {
                    self.bump();
                }
            }
        }
        self.bump();
        Ok(EnumDef {
            is_pub,
            name,
            name_span,
            variants,
            methods,
        })
    }

    fn variant(&mut self) -> Result<Variant, Diagnostic> {
        let (name, name_span) = self.expect_ident("for a variant name")?;
        let payload = if matches!(self.peek().kind, TokKind::LParen) {
            self.bump();
            let payload = self.variant_payload()?;
            self.expect(TokKind::RParen, "after a variant's payload")?;
            payload
        } else {
            VariantPayload::Unit
        };
        Ok(Variant {
            name,
            name_span,
            payload,
        })
    }

    fn variant_payload(&mut self) -> Result<VariantPayload, Diagnostic> {
        if matches!(self.peek().kind, TokKind::Ident(_)) {
            let peek2 = self.peek2().kind.clone();
            if matches!(peek2, TokKind::Colon) {
                let mut fields = Vec::new();
                loop {
                    let (name, name_span) = self.expect_ident("for a variant field name")?;
                    self.expect(TokKind::Colon, "after a variant field name")?;
                    let (ty, ty_span) = self.type_()?;
                    fields.push(VariantField {
                        name,
                        name_span,
                        ty,
                        ty_span,
                    });
                    if !matches!(self.peek().kind, TokKind::Comma) {
                        break;
                    }
                    self.bump();
                }
                Ok(VariantPayload::Named(fields))
            } else {
                let (ty, ty_span) = self.type_()?;
                Ok(VariantPayload::Single(ty, ty_span))
            }
        } else {
            let (ty, ty_span) = self.type_()?;
            Ok(VariantPayload::Single(ty, ty_span))
        }
    }

    fn impl_def(&mut self) -> Result<ImplDef, Diagnostic> {
        self.expect_kw(TokKind::KwImpl, "to start an `impl` block")?;
        let (type_name, type_span) = self.expect_ident("after `impl`")?;
        self.expect(TokKind::LBrace, "to open the `impl` body")?;
        let mut methods = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            methods.push(self.method_in_type()?);
        }
        self.bump();
        Ok(ImplDef {
            type_name,
            type_span,
            methods,
        })
    }

    /// S27: method inside a type body or `impl` block.
    fn method_in_type(&mut self) -> Result<Func, Diagnostic> {
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "to start a method")?;
        self.func_after_fn(is_pub)
    }

    fn field(&mut self) -> Result<Field, Diagnostic> {
        let mut is_stored_ref = false;
        let mut stored_ref_label = None;
        if matches!(self.peek().kind, TokKind::KwStored) {
            is_stored_ref = true;
            self.bump();
            if matches!(self.peek().kind, TokKind::LBracket) {
                self.bump();
                let (label, _) = self.expect_ident("inside `ref[...]`")?;
                stored_ref_label = Some(label);
                self.expect(TokKind::RBracket, "after a ref label")?;
            }
        }
        let (name, name_span) = self.expect_ident("for a field name")?;
        self.expect(TokKind::Colon, "after a field name")?;
        let (ty, ty_span) = self.type_()?;
        Ok(Field {
            is_stored_ref,
            stored_ref_label,
            name,
            name_span,
            ty,
            ty_span,
        })
    }

    fn const_def(&mut self) -> Result<ConstDef, Diagnostic> {
        let mut attrs = Vec::new();
        while matches!(self.peek().kind, TokKind::At) {
            self.bump();
            let (attr_name, _) = self.expect_ident("after `@`")?;
            match attr_name.as_str() {
                "static" => attrs.push(ConstAttr::ForceStatic),
                "inline" => attrs.push(ConstAttr::ForceInline),
                other => {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!("`@{}` isn't a known attribute on a const", other),
                        "only `@static` and `@inline` are supported on const declarations"
                            .to_string(),
                        "remove the attribute or use `@static` or `@inline`".to_string(),
                        Some(self.peek().span),
                    ));
                }
            }
        }
        self.expect_kw(TokKind::KwConst, "to start a const declaration")?;
        let (name, name_span) = self.expect_ident("after `const`")?;
        self.expect(TokKind::Eq, "after the const name")?;
        let value = self.expr()?;
        self.expect(TokKind::Semi, "after a const value")?;
        Ok(ConstDef {
            name,
            name_span,
            value,
            attrs,
            rust_kind: crate::ast::RustConstKind::Const,
        })
    }

    // --- statements ------------------------------------------------------

    /// Parse statements until the closing `}` (consumed). Recovers at
    /// statement boundaries so several problems surface in one run.
    fn block_stmts(&mut self) -> Vec<Stmt> {
        let mut body = Vec::new();
        loop {
            match &self.peek().kind {
                TokKind::RBrace => {
                    self.bump();
                    break;
                }
                TokKind::Eof => {
                    self.diags.push(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this block, found the end of the file".to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                    break;
                }
                _ => match self.stmt() {
                    Ok(s) => body.push(s),
                    Err(d) => {
                        self.diags.push(d);
                        self.sync_stmt();
                    }
                },
            }
        }
        body
    }

    fn stmt(&mut self) -> Result<Stmt, Diagnostic> {
        match &self.peek().kind {
            TokKind::KwVal | TokKind::KwVar => {
                let binding = self.binding()?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Ident(n) if n == syntax::FOREIGN_LET => {
                // S14 teaching error E0009, then parse as a binding.
                let t = self.bump();
                let is_mut = matches!(self.peek().kind, TokKind::KwMutate);
                if is_mut {
                    self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0009",
                        format!("{} does not use `{}`", syntax::LANG_NAME, syntax::FOREIGN_LET_MUT),
                        binding_why(),
                        format!("replace `{}` with `{}`", syntax::FOREIGN_LET_MUT, syntax::KW_VAR),
                        Some(t.span),
                    ));
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0009",
                        format!("{} does not use `{}`", syntax::LANG_NAME, syntax::FOREIGN_LET),
                        binding_why(),
                        format!("replace `{}` with `{}`", syntax::FOREIGN_LET, syntax::KW_VAL),
                        Some(t.span),
                    ));
                }
                let binding = self.binding_after_kw(is_mut)?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Ident(n)
                if n == syntax::FOREIGN_SET && matches!(self.peek2().kind, TokKind::Ident(_)) =>
            {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0010",
                    format!("{} does not use `{}`", syntax::LANG_NAME, syntax::FOREIGN_SET),
                    binding_why(),
                    format!("replace `{}` with `{}`", syntax::FOREIGN_SET, syntax::KW_VAL),
                    Some(t.span),
                ));
                let binding = self.binding_after_kw(false)?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Ident(n) if n == syntax::FOREIGN_MATCH => {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0016",
                    format!("{} does not use `{}`", syntax::LANG_NAME, syntax::FOREIGN_MATCH),
                    format!(
                        "choosing one branch from many is written with `{}`",
                        syntax::KW_SWITCH
                    ),
                    format!("replace `{}` with `{}`", syntax::FOREIGN_MATCH, syntax::KW_SWITCH),
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
            TokKind::KwIf => Ok(Stmt::If(self.if_stmt()?)),
            TokKind::KwWhile => {
                let span = self.bump().span;
                let cond = self.expr_no_struct_lit()?;
                self.expect(TokKind::LBrace, "to open the `while` body")?;
                let body = self.block_stmts();
                Ok(Stmt::While { cond, body, span })
            }
            TokKind::KwFor => {
                let span = self.bump().span;
                let (var, var_span) = self.expect_ident("after `for`")?;
                self.expect_kw(TokKind::KwIn, "after the loop name")?;
                let start = self.expr_no_struct_lit()?;
                if !matches!(self.peek().kind, TokKind::DotDot) {
                    return Err(Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}` in this `for` range, found {}",
                            syntax::OP_RANGE,
                            describe(&self.peek().kind)
                        ),
                        "`for` counts over a range with two ends, like `1..10` (both ends included)"
                            .to_string(),
                        format!("write `{} {} {} 1..10 {{ ... }}`", syntax::KW_FOR, var, syntax::KW_IN),
                        Some(self.peek().span),
                    ));
                }
                self.bump(); // ..
                let end = self.expr_no_struct_lit()?;
                self.expect(TokKind::LBrace, "to open the `for` body")?;
                let body = self.block_stmts();
                Ok(Stmt::For {
                    var,
                    var_span,
                    start,
                    end,
                    body,
                    span,
                })
            }
            TokKind::KwSwitch => {
                let span = self.bump().span;
                self.switch_after_kw(span)
            }
            TokKind::KwBreak => {
                let span = self.bump().span;
                self.finish_stmt()?;
                Ok(Stmt::Break(span))
            }
            TokKind::KwContinue => {
                let span = self.bump().span;
                self.finish_stmt()?;
                Ok(Stmt::Continue(span))
            }
            TokKind::KwLoop => {
                let span = self.bump().span;
                self.expect(TokKind::LBrace, "after `loop`")?;
                let inner = self.block_stmts();
                Ok(Stmt::Loop(inner, span))
            }
            TokKind::KwUnsafe => {
                let span = self.bump().span;
                self.expect(TokKind::LBrace, "after `unsafe`")?;
                let inner = self.block_stmts();
                Ok(Stmt::Unsafe(inner, span))
            }
            TokKind::Ident(_) => {
                // Assignment (`x = e;`, `x += e;`) or a call statement.
                if let TokKind::Ident(name) = self.peek().kind.clone() {
                    let next = &self.peek2().kind;
                    if matches!(next, TokKind::Eq) || next.compound_op().is_some() {
                        let name_span = self.bump().span;
                        let op_tok = self.bump();
                        let op = op_tok.kind.compound_op();
                        let value = self.expr()?;
                        self.finish_stmt()?;
                        return Ok(Stmt::Assign {
                            name,
                            name_span,
                            op,
                            op_span: op_tok.span,
                            value,
                        });
                    }
                }
                let expr = self.expr()?;
                match &expr {
                    Expr::Call(_)
                    | Expr::Field(_, _, _)
                    | Expr::MethodCall { .. } => {}
                    other => {
                        return Err(Diagnostic::error(
                            "E0003",
                            "this line computes a value but doesn't do anything with it"
                                .to_string(),
                            "a statement must have an effect: a call, a binding, an assignment, or `return`".to_string(),
                            format!(
                                "use the value, e.g. `{} x = ...;` or `{}(...)`",
                                syntax::KW_VAL,
                                syntax::BUILTIN_PRINT
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
                format!("expected a statement, found {}", describe(other)),
                "inside a function body, write a call, binding, assignment, or `return`"
                    .to_string(),
                format!(
                    "e.g. {}(\"hello\"); or {} x = 1;",
                    syntax::BUILTIN_PRINT,
                    syntax::KW_VAL
                ),
                Some(self.peek().span),
            )),
        }
    }

    fn if_stmt(&mut self) -> Result<IfStmt, Diagnostic> {
        let span = self.bump().span; // `if`
        let cond = self.expr_no_struct_lit()?;
        self.expect(TokKind::LBrace, "to open the `if` body")?;
        let then_body = self.block_stmts();
        let mut else_branch = None;
        if matches!(self.peek().kind, TokKind::KwElse) {
            self.bump();
            if matches!(self.peek().kind, TokKind::KwIf) {
                else_branch = Some(ElseBranch::ElseIf(Box::new(self.if_stmt()?)));
            } else {
                self.expect(TokKind::LBrace, "to open the `else` body")?;
                else_branch = Some(ElseBranch::Else(self.block_stmts()));
            }
        }
        Ok(IfStmt {
            cond,
            then_body,
            else_branch,
            span,
        })
    }

    /// `switch` body, after the keyword (S24): condition arms with `->`,
    /// each arm block followed by `;`, and a required `else` arm.
    fn switch_after_kw(&mut self, span: Span) -> Result<Stmt, Diagnostic> {
        let subject = self.expr_no_struct_lit()?;
        self.expect(TokKind::LBrace, "to open the `switch` body")?;
        let mut arms = Vec::new();
        let mut else_body: Option<Vec<Stmt>> = None;
        loop {
            match &self.peek().kind {
                TokKind::RBrace => {
                    self.bump();
                    break;
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this `switch`, found the end of the file"
                            .to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                TokKind::Ident(name)
                    if name == syntax::FOREIGN_CASE || name == syntax::FOREIGN_DEFAULT =>
                {
                    let t = self.bump();
                    let foreign = if let TokKind::Ident(n) = &t.kind {
                        n.clone()
                    } else {
                        unreachable!()
                    };
                    self.diags.push(Diagnostic::error(
                        "E0023",
                        format!(
                            "`{}` arms are written `condition {} {{ ... }};`, not `{}`",
                            syntax::KW_SWITCH,
                            syntax::OP_ARM_ARROW,
                            foreign
                        ),
                        format!(
                            "choosing one branch from many uses `{}` with `->` arms (S24)",
                            syntax::KW_SWITCH
                        ),
                        format!(
                            "replace `{}` with a condition and `{}`, like `x == 1 {} {{ ... }};`",
                            foreign,
                            syntax::OP_ARM_ARROW,
                            syntax::OP_ARM_ARROW
                        ),
                        Some(t.span),
                    ));
                    self.sync_stmt();
                    continue;
                }
                TokKind::KwElse => {
                    self.bump();
                    self.expect(TokKind::Arrow, "after `else` in a `switch`")?;
                    self.expect(TokKind::LBrace, "to open the `else` arm")?;
                    let body = self.block_stmts();
                    self.expect(TokKind::Semi, "after a `switch` arm's closing `}`")?;
                    else_body = Some(body);
                }
                _ => {
                    let arm_start = self.peek().span;
                    let cond = self.expr_no_struct_lit()?;
                    self.expect(TokKind::Arrow, "after a `switch` arm's condition")?;
                    self.expect(TokKind::LBrace, "to open the arm's body")?;
                    let body = self.block_stmts();
                    self.expect(TokKind::Semi, "after a `switch` arm's closing `}`")?;
                    arms.push(SwitchArm {
                        cond,
                        body,
                        span: arm_start,
                    });
                }
            }
        }
        Ok(Stmt::Switch {
            subject,
            arms,
            else_body,
            span,
        })
    }

    fn binding(&mut self) -> Result<Binding, Diagnostic> {
        let mutable = match self.peek().kind {
            TokKind::KwVar => {
                self.bump();
                true
            }
            TokKind::KwVal => {
                self.bump();
                false
            }
            _ => unreachable!(),
        };
        self.binding_after_kw(mutable)
    }

    fn binding_after_kw(&mut self, mutable: bool) -> Result<Binding, Diagnostic> {
        let (name, name_span) = self.expect_ident("after a binding keyword")?;
        let (ty, ty_span) = if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            let (t, s) = self.type_()?;
            (Some(t), Some(s))
        } else {
            (None, None)
        };
        self.expect(TokKind::Eq, "in a binding")?;
        let init = self.expr()?;
        Ok(Binding {
            mutable,
            name,
            name_span,
            ty,
            ty_span,
            init,
        })
    }

    // --- expressions -----------------------------------------------------

    fn expr(&mut self) -> Result<Expr, Diagnostic> {
        self.expr_or(true)
    }

    fn expr_no_struct_lit(&mut self) -> Result<Expr, Diagnostic> {
        self.expr_or(false)
    }

    fn expr_or(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let mut lhs = self.expr_and(allow_struct_lit)?;
        loop {
            let is_or = match &self.peek().kind {
                TokKind::OrOr => true,
                TokKind::Ident(n) if n == syntax::FOREIGN_OR => {
                    self.foreign_logic_error(syntax::FOREIGN_OR, syntax::OP_OR);
                    true
                }
                _ => false,
            };
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
                TokKind::Ident(n) if n == syntax::FOREIGN_AND => {
                    self.foreign_logic_error(syntax::FOREIGN_AND, syntax::OP_AND);
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
            TokKind::EqEq | TokKind::NotEq | TokKind::Lt | TokKind::Gt | TokKind::Le
            | TokKind::Ge => Some(self.peek().span),
            _ => None,
        } {
            return Err(Diagnostic::error(
                "E0003",
                "comparisons can't be chained".to_string(),
                format!(
                    "`a < b < c` doesn't compare all three; check each pair and join with `{}`",
                    syntax::OP_AND
                ),
                format!("write `a < b {} b < c`", syntax::OP_AND),
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
            TokKind::Ident(n) if n == syntax::FOREIGN_NOT && self.starts_expr(&self.peek2().kind) => {
                self.foreign_logic_error(syntax::FOREIGN_NOT, syntax::OP_NOT);
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Unary(UnOp::Not, Box::new(inner), full))
            }
            TokKind::Ident(n) if n == syntax::FOREIGN_TRY && self.starts_expr(&self.peek2().kind) => {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0014",
                    format!("{} does not use `{}`", syntax::LANG_NAME, syntax::FOREIGN_TRY),
                    format!(
                        "a call that can fail is marked with `{}` after it, like `parse(x){}` (error handling arrives in M4 — until then, no call can fail)",
                        syntax::OP_TRY_SUFFIX,
                        syntax::OP_TRY_SUFFIX
                    ),
                    format!("remove `{}`", syntax::FOREIGN_TRY),
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
                    self.bump();
                    let (member, member_span) = self.expect_ident("after `.`")?;
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
                        };
                    } else {
                        expr = Expr::Field(Box::new(expr), member, member_span);
                    }
                }
                TokKind::Question => {
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0006",
                        format!("`{}` doesn't do anything yet", syntax::OP_TRY_SUFFIX),
                        "errors as values (and `?` to pass them along) arrive in M4".to_string(),
                        format!("remove the `{}` for now", syntax::OP_TRY_SUFFIX),
                        Some(t.span),
                    ));
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn expr_primary(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        match self.peek().kind.clone() {
            TokKind::KwValue => {
                let span = self.bump().span;
                self.expect(TokKind::LParen, "after `value`")?;
                let inner = self.expr()?;
                self.expect(TokKind::RParen, "after the value inside `value(...)`")?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Present(Box::new(inner), full))
            }
            TokKind::KwNull => {
                let span = self.bump().span;
                Ok(Expr::Absent(span))
            }
            TokKind::Ident(name)
                if matches!(
                    name.as_str(),
                    syntax::FOREIGN_NONE
                        | syntax::FOREIGN_SOME
                        | syntax::FOREIGN_NIL
                        | syntax::FOREIGN_NONE_LOWER
                        | syntax::FOREIGN_SOME_LOWER
                ) =>
            {
                let t = self.bump();
                let foreign = if let TokKind::Ident(n) = &t.kind {
                    n.clone()
                } else {
                    unreachable!()
                };
                let (canonical, fix) = match foreign.as_str() {
                    syntax::FOREIGN_NONE | syntax::FOREIGN_NONE_LOWER | syntax::FOREIGN_NIL => {
                        (syntax::LIT_NULL, syntax::LIT_NULL)
                    }
                    _ => (syntax::LIT_VALUE, syntax::LIT_VALUE),
                };
                self.diags.push(Diagnostic::error(
                    "E0020",
                    format!(
                        "optional values use `{}` and `{}`, not `{}`",
                        syntax::LIT_VALUE,
                        syntax::LIT_NULL,
                        foreign
                    ),
                    format!(
                        "{} uses exactly one spelling for each thing, so all code reads the same",
                        syntax::LANG_NAME
                    ),
                    format!("replace `{}` with `{}`", foreign, fix),
                    Some(t.span),
                ));
                if canonical == syntax::LIT_NULL {
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
                Ok(Expr::Int(n, span))
            }
            TokKind::Float(v) => {
                let span = self.bump().span;
                Ok(Expr::Float(v, span))
            }
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
                Ok(Expr::Ident(syntax::KW_SELF.to_string(), span))
            }
            TokKind::LParen => {
                self.bump();
                let inner = self.expr()?;
                self.expect(TokKind::RParen, "to close this `(`")?;
                Ok(inner)
            }
            TokKind::Ident(name) => {
                let span = self.bump().span;
                if allow_struct_lit && matches!(self.peek().kind, TokKind::LBrace) {
                    return self.struct_lit_after_name(name, span);
                }
                if matches!(self.peek().kind, TokKind::Dot) {
                    self.bump();
                    let (member, member_span) = self.expect_ident("after `.`")?;
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
                            receiver: Box::new(Expr::Ident(name, span)),
                            method: member,
                            method_span: member_span,
                            args,
                        });
                    }
                    return Ok(Expr::Field(
                        Box::new(Expr::Ident(name, span)),
                        member,
                        member_span,
                    ));
                }
                if matches!(self.peek().kind, TokKind::LParen) {
                    let call = self.call_after_name(name, span)?;
                    return Ok(Expr::Call(call));
                }
                Ok(Expr::Ident(name, span))
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

    fn struct_lit_after_name(
        &mut self,
        type_name: String,
        start_span: Span,
    ) -> Result<Expr, Diagnostic> {
        self.expect(TokKind::LBrace, "to open a struct literal")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            let (field, field_span) = self.expect_ident("for a field name")?;
            self.expect(TokKind::Colon, "after a field name in a struct literal")?;
            let value = self.expr()?;
            fields.push((field, field_span, value));
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.bump();
        Ok(Expr::StructLit {
            type_name,
            fields,
            span: Span::new(start_span.start, end),
        })
    }

    fn enum_lit_args(&mut self) -> Result<Vec<EnumLitArg>, Diagnostic> {
        let mut args = Vec::new();
        if matches!(self.peek().kind, TokKind::RParen) {
            return Ok(args);
        }
        loop {
            if matches!(self.peek().kind, TokKind::Ident(_)) {
                let name = if let TokKind::Ident(n) = self.peek().kind.clone() {
                    n
                } else {
                    unreachable!()
                };
                if matches!(self.peek2().kind, TokKind::Colon) {
                    self.bump();
                    self.bump();
                    let expr = self.expr()?;
                    args.push(EnumLitArg::Named { label: name, expr });
                } else {
                    args.push(EnumLitArg::Positional(self.expr()?));
                }
            } else {
                args.push(EnumLitArg::Positional(self.expr()?));
            }
            if matches!(self.peek().kind, TokKind::RParen) {
                break;
            }
            self.expect(TokKind::Comma, "between enum variant arguments")?;
        }
        Ok(args)
    }

    /// S31: try to parse a pattern on the right of `==`.
    fn try_pattern_rhs(&mut self) -> Result<Option<Pattern>, Diagnostic> {
        match &self.peek().kind {
            TokKind::KwNull => {
                let span = self.bump().span;
                return Ok(Some(Pattern::Absent(span)));
            }
            TokKind::KwValue => {
                let start = self.bump().span;
                self.expect(TokKind::LParen, "after `value`")?;
                let (binding, binding_span) = self.expect_ident("inside `value(...)`")?;
                self.expect(TokKind::RParen, "after the binding in `value(...)`")?;
                return Ok(Some(Pattern::Present {
                    binding,
                    span: Span::new(start.start, binding_span.end),
                }));
            }
            TokKind::Ident(variant) => {
                let variant = variant.clone();
                let span = self.peek().span;
                self.bump();
                let bindings = if matches!(self.peek().kind, TokKind::LParen) {
                    self.bump();
                    let mut bindings = Vec::new();
                    if !matches!(self.peek().kind, TokKind::RParen) {
                        loop {
                            let (b, _) = self.expect_ident("for a pattern binding")?;
                            bindings.push(b);
                            if matches!(self.peek().kind, TokKind::RParen) {
                                break;
                            }
                            self.expect(TokKind::Comma, "between pattern bindings")?;
                        }
                    }
                    self.expect(TokKind::RParen, "after pattern bindings")?;
                    bindings
                } else {
                    Vec::new()
                };
                let end = self.toks[self.pos.saturating_sub(1)].span.end;
                return Ok(Some(Pattern::Variant {
                    variant,
                    bindings,
                    span: Span::new(span.start, end),
                }));
            }
            _ => Ok(None),
        }
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
        let convention = self.parse_access_prefix();
        let span = self.peek().span;
        let expr = self.expr()?;
        Ok(CallArg {
            convention,
            expr,
            span,
            flags: Default::default(),
        })
    }

    fn parse_access_prefix(&mut self) -> AccessConvention {
        if let TokKind::Ident(name) = self.peek().kind.clone() {
            match name.as_str() {
                syntax::FOREIGN_READ => {
                    let span = self.peek().span;
                    self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0017",
                        format!(
                            "shared access is written with no word in front — not `{}`",
                            syntax::FOREIGN_READ
                        ),
                        "Jet has exactly one spelling for each thing, so all code reads the same"
                            .to_string(),
                        format!(
                            "remove `{}` and write `name: Type`",
                            syntax::FOREIGN_READ
                        ),
                        Some(span),
                    ));
                    return AccessConvention::Read;
                }
                syntax::FOREIGN_WRITE => {
                    let span = self.peek().span;
                    self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0018",
                        format!(
                            "changeable access is written `{}`, not `{}`",
                            syntax::KW_MUTATE,
                            syntax::FOREIGN_WRITE
                        ),
                        "Jet has exactly one spelling for each thing, so all code reads the same"
                            .to_string(),
                        format!(
                            "replace `{}` with `{}`",
                            syntax::FOREIGN_WRITE,
                            syntax::KW_MUTATE
                        ),
                        Some(span),
                    ));
                    return AccessConvention::Mutate;
                }
                _ => {}
            }
        }
        match self.peek().kind {
            TokKind::KwMutate => {
                self.bump();
                AccessConvention::Mutate
            }
            TokKind::KwMove => {
                self.bump();
                AccessConvention::Move
            }
            _ => AccessConvention::Read,
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
                | TokKind::LParen
                | TokKind::Minus
                | TokKind::Bang
        )
    }

    fn foreign_logic_error(&mut self, foreign: &str, canonical: &str) {
        self.diags.push(Diagnostic::error(
            "E0012",
            format!("{} writes \"{}\" as `{}`", syntax::LANG_NAME, foreign, canonical),
            format!(
                "logic uses the symbols `{}`, `{}`, and `{}`",
                syntax::OP_AND,
                syntax::OP_OR,
                syntax::OP_NOT
            ),
            format!("replace `{}` with `{}`", foreign, canonical),
            Some(self.peek().span),
        ));
    }

    fn type_(&mut self) -> Result<(Type, Span), Diagnostic> {
        let start = self.peek().span;
        let base = match self.peek().kind.clone() {
            TokKind::Ident(name) => {
                self.bump();
                match name.as_str() {
                    syntax::TYPE_INT => Type::Int,
                    syntax::TYPE_FLOAT => Type::Float,
                    syntax::TYPE_BOOL => Type::Bool,
                    syntax::TYPE_STRING => Type::String,
                    syntax::FOREIGN_TEXT => {
                        // S14 teaching error E0013; recover as String.
                        self.diags.push(Diagnostic::error(
                            "E0013",
                            format!(
                                "the text type is called `{}`, not `{}`",
                                syntax::TYPE_STRING,
                                syntax::FOREIGN_TEXT
                            ),
                            format!("`{}` is the one and only text type", syntax::TYPE_STRING),
                            format!(
                                "replace `{}` with `{}`",
                                syntax::FOREIGN_TEXT,
                                syntax::TYPE_STRING
                            ),
                            Some(start),
                        ));
                        Type::String
                    }
                    syntax::TYPE_LIST => {
                        self.expect(TokKind::LBracket, "after `List`")?;
                        let (inner, _) = self.type_()?;
                        self.expect(TokKind::RBracket, "after a list element type")?;
                        Type::List(Box::new(inner))
                    }
                    syntax::TYPE_SHARED => {
                        self.expect(TokKind::LBracket, "after `Shared`")?;
                        let (inner, _) = self.type_()?;
                        self.expect(TokKind::RBracket, "after a shared element type")?;
                        Type::Shared(Box::new(inner))
                    }
                    other => Type::Named(other.to_string()),
                }
            }
            other => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!("expected a type name, found {}", describe(&other)),
                    "types look like `Int`, `String`, or `List[Int]`".to_string(),
                    "e.g. `x: Int` or `items: List[String]`".to_string(),
                    Some(self.peek().span),
                ));
            }
        };
        if matches!(self.peek().kind, TokKind::Question) {
            let qspan = self.bump().span;
            if matches!(self.peek().kind, TokKind::Question) {
                return Err(Diagnostic::error(
                    "E0309",
                    "`??` isn't allowed on a type".to_string(),
                    "an optional value is written `T?` once — there's no optional optional"
                        .to_string(),
                    "use a single `?`, like `Int?`".to_string(),
                    Some(qspan),
                ));
            }
            return Ok((Type::Option(Box::new(base)), start));
        }
        Ok((base, start))
    }

    /// S6 (ratified): every statement ends with `;` — no exceptions, not
    /// even before a closing `}`.
    fn finish_stmt(&mut self) -> Result<(), Diagnostic> {
        match &self.peek().kind {
            TokKind::Semi => {
                self.bump();
                Ok(())
            }
            other => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected `{}` after this statement, found {}",
                    syntax::STMT_SEP,
                    describe(other)
                ),
                format!(
                    "every statement ends with `{}` — including the last one in a block",
                    syntax::STMT_SEP
                ),
                format!("add `{}` after the statement", syntax::STMT_SEP),
                Some(self.peek().span),
            )),
        }
    }

    fn expect_kw(&mut self, want: TokKind, where_: &str) -> Result<(), Diagnostic> {
        if std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(&want) {
            self.bump();
            Ok(())
        } else {
            Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected {} {}, found {}",
                    describe(&want),
                    where_,
                    describe(&self.peek().kind)
                ),
                "the structure here isn't what the compiler expected".to_string(),
                format!("use {} {}", describe(&want), where_),
                Some(self.peek().span),
            ))
        }
    }

    fn expect(&mut self, want: TokKind, where_: &str) -> Result<(), Diagnostic> {
        self.expect_kw(want, where_)
    }

    fn expect_ident(&mut self, where_: &str) -> Result<(String, Span), Diagnostic> {
        match self.bump() {
            Token {
                kind: TokKind::Ident(name),
                span,
            } => Ok((name, span)),
            t => Err(Diagnostic::error(
                "E0003",
                format!("expected a name {}, found {}", where_, describe(&t.kind)),
                "names start with a letter or `_`".to_string(),
                "e.g. `main`, `count`, `_tmp`".to_string(),
                Some(t.span),
            )),
        }
    }
}

fn binding_why() -> String {
    format!(
        "a binding is `{}` if it never changes, or `{}` if it can",
        syntax::KW_VAL,
        syntax::KW_VAR
    )
}

fn pat_span(pat: &Pattern) -> Span {
    match pat {
        Pattern::Variant { span, .. } | Pattern::Present { span, .. } | Pattern::Absent(span) => {
            *span
        }
    }
}
