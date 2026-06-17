//! Parser: tokens -> AST. Hand-written recursive descent with statement-
//! level error recovery (M1): one run reports every parse problem it can.
//!
//! Teaching errors (S14): familiar foreign spellings (`def`, `let`, `set`,
//! `and`, `try`, `match`, …) are recognized here only to emit an error
//! naming the canonical Jet form — then parsing continues as if the
//! canonical form had been written, so one foreign word doesn't hide the
//! rest of the file's problems.

use crate::ast::{
    AccessConvention, BinOp, Binding, BindName, BindPattern, Call, CallArg, ConstAttr, ConstDef,
    Contribution, ElseBranch, EnumDef,
    EnumLitArg, Expr, Field, ForKind, Func, IfStmt, ImplDef, Item, LValue, Lambda, LambdaBody,
    ModuleDecl, Namespace,
    LambdaMeta, LambdaParam, OrFallback, Param, Pattern, Program, Stmt, StrPart, StructDef,
    SwitchArm, TraitDef, TraitImplBlock, TraitMethodSig, Type, TypeParam, UnOp, Variant,
    VariantField, VariantPayload,
};
use crate::diag::{Diagnostic, Span};
use crate::generics;
use crate::lexer::{describe, StrTokPart, TokKind, Token};
use crate::syntax;

pub fn parse(toks: &[Token]) -> Result<Program, Vec<Diagnostic>> {
    parse_inner(toks, false)
}

/// Parse for `jet fmt`: succeeds when the AST is recoverable, even if S14
/// teaching diagnostics were emitted (foreign spellings already lowered in
/// the tree as `val`, `fn`, `&&`, …).
pub fn parse_for_fmt(toks: &[Token]) -> Result<Program, Vec<Diagnostic>> {
    parse_inner(toks, true)
}

/// Parse for editor/LSP check: recover through S14 teaching errors, return them
/// alongside the AST so sema can still run (M6 phase 4).
pub fn parse_for_check(toks: &[Token]) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    let toks = crate::lexer::without_comments(toks);
    check_token_nesting(&toks)?;
    let mut p = Parser {
        toks: &toks,
        pos: 0,
        diags: Vec::new(),
        pending_type_gt: false,
        depth: 0,
        type_generic_depth: 0,
        type_generic_chain: Vec::new(),
        type_generic_truncated: false,
    };
    let prog = p.program();
    if p.diags.is_empty() {
        Ok((prog, Vec::new()))
    } else if p.diags.iter().all(|d| is_teaching_parse_diag(d.code)) {
        Ok((prog, p.diags))
    } else {
        Err(p.diags)
    }
}

fn parse_inner(toks: &[Token], for_fmt: bool) -> Result<Program, Vec<Diagnostic>> {
    let toks = crate::lexer::without_comments(toks);
    check_token_nesting(&toks)?;
    let mut p = Parser {
        toks: &toks,
        pos: 0,
        diags: Vec::new(),
        pending_type_gt: false,
        depth: 0,
        type_generic_depth: 0,
        type_generic_chain: Vec::new(),
        type_generic_truncated: false,
    };
    let prog = p.program();
    if p.diags.is_empty() {
        return Ok(prog);
    }
    if for_fmt && p.diags.iter().all(|d| is_teaching_parse_diag(d.code)) {
        Ok(prog)
    } else {
        Err(p.diags)
    }
}

fn string_literal_value(parts: &[StrTokPart]) -> Result<String, Diagnostic> {
    if parts.len() != 1 {
        return Err(Diagnostic::error(
            "E0003",
            "an import path must be one piece of quoted text".to_string(),
            "file paths can't contain `{ }` interpolation".to_string(),
            format!("write: {} \"path/to/file\";", syntax::KW_USE),
            None,
        ));
    }
    match &parts[0] {
        StrTokPart::Lit(s) => Ok(s.clone()),
        StrTokPart::Interp(_) => Err(Diagnostic::error(
            "E0003",
            "an import path can't contain `{ }` interpolation".to_string(),
            "file paths are fixed strings".to_string(),
            format!("write: {} \"path/to/file\";", syntax::KW_USE),
            None,
        )),
    }
}

/// S14: recovered in the AST; fmt may rewrite to canon.
fn is_teaching_parse_diag(code: &str) -> bool {
    matches!(
        code,
        "E0008"
            | "E0009"
            | "E0010"
            | "E0012"
            | "E0013"
            | "E0014"
            | "E0015"
            | "E0016"
            | "E0017"
            | "E0018"
            | "E0020"
            | "E0021"
            | "E0023"
            | "E0024"
            | "E0025"
            | "E0026"
            | "E0027"
            | "E0028"
            | "E0030"
            | "E0031"
            | "E0034"
            | "E0036"
            | "E0044"
            | "E0045"
            | "E0048"
            | "E0049"
            | "E0050"
            | "E0051"
    )
}

/// Hard cap on recursive parser nesting (parentheses, unary chains,
/// literals-with-expressions, generic types). Recursive descent here — and
/// the recursive passes downstream (sema, codegen, fmt) — use the call
/// stack, so unbounded nesting in adversarial input would abort the compiler
/// with a stack overflow instead of a diagnostic (E0035).
const MAX_NESTING: usize = 128;

struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    diags: Vec<Diagnostic>,
    /// S33: when `>>` is split while closing nested `Type<…>`.
    pending_type_gt: bool,
    /// Current recursive parser nesting depth.
    depth: usize,
    /// Nesting depth inside generic type arguments (M9 E0909).
    type_generic_depth: usize,
    type_generic_chain: Vec<String>,
    /// Set when generic depth exceeds the limit mid-parse.
    type_generic_truncated: bool,
}

fn too_deep(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0035",
        "this code is nested too deeply".to_string(),
        format!(
            "the compiler keeps things simple by allowing at most {} levels of nesting",
            MAX_NESTING
        ),
        "split the expression into smaller steps with `val` bindings".to_string(),
        Some(span),
    )
}

fn check_token_nesting(toks: &[Token]) -> Result<(), Vec<Diagnostic>> {
    let mut depth = 0usize;
    for t in toks {
        match t.kind {
            TokKind::LParen | TokKind::LBracket | TokKind::LBrace => {
                depth += 1;
                if depth > MAX_NESTING {
                    return Err(vec![too_deep(t.span)]);
                }
            }
            TokKind::RParen | TokKind::RBracket | TokKind::RBrace => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    Ok(())
}

impl<'a> Parser<'a> {
    fn enter_generic_type_layer(&mut self, label: &str, span: Span) -> bool {
        self.type_generic_depth += 1;
        self.type_generic_chain.push(label.to_string());
        if self.type_generic_depth > generics::MAX_GENERIC_DEPTH {
            let chain = self.type_generic_chain.join(" → ");
            self.type_generic_depth = self.type_generic_depth.saturating_sub(1);
            self.type_generic_chain.pop();
            self.diags.push(generics::e0909(&chain, span));
            self.type_generic_truncated = true;
            return false;
        }
        true
    }

    fn leave_generic_type_layer(&mut self) {
        self.type_generic_depth = self.type_generic_depth.saturating_sub(1);
        self.type_generic_chain.pop();
    }

    fn type_generic_arg(&mut self, label: &str) -> Result<Type, Diagnostic> {
        let span = self.peek().span;
        if !self.enter_generic_type_layer(label, span) {
            self.sync_type_arg();
            return Ok(Type::Int);
        }
        let (inner, _) = self.type_()?;
        self.leave_generic_type_layer();
        Ok(inner)
    }

    fn type_starts_here(&self) -> bool {
        matches!(
            self.peek().kind,
            TokKind::KwFn | TokKind::Ident(_) | TokKind::LParen | TokKind::LBracket
        )
    }

    fn return_type(&mut self) -> Result<(Type, Span), Diagnostic> {
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
                        err: Box::new(Type::Named(syntax::TYPE_ERROR.to_string())),
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

    fn peek(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn peek2(&self) -> &Token {
        &self.toks[(self.pos + 1).min(self.toks.len() - 1)]
    }

    fn peek3(&self) -> &Token {
        &self.toks[(self.pos + 2).min(self.toks.len() - 1)]
    }

    /// End byte of the most recently consumed token (the one before the cursor).
    fn prev_end(&self) -> usize {
        self.toks[self.pos.saturating_sub(1)].span.end
    }

    fn bump(&mut self) -> Token {
        let t = self.peek().clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn with_nesting<T>(
        &mut self,
        span: Span,
        f: impl FnOnce(&mut Self) -> Result<T, Diagnostic>,
    ) -> Result<T, Diagnostic> {
        if self.depth >= MAX_NESTING {
            return Err(too_deep(span));
        }
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
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
                | TokKind::KwConst
                | TokKind::KwComptime => return,
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

    /// S16 (M6): `import "path" [as alias];` or `import name [as alias];`
    fn import_decl(&mut self) -> Result<crate::ast::ImportDecl, Diagnostic> {
        let start = self.bump().span;
        let (kind, alias_default) = match &self.peek().kind {
            TokKind::Str(parts) => {
                let path = string_literal_value(parts)?;
                let span = self.bump().span;
                (
                    crate::ast::ImportKind::File(path.clone(), span),
                    path.rsplit('/').next().unwrap_or("module").to_string(),
                )
            }
            TokKind::Ident(_) => {
                let (module_name, span) = self.module_path()?;
                let alias_default = module_name
                    .rsplit('.')
                    .next()
                    .unwrap_or(module_name.as_str())
                    .to_string();
                (
                    crate::ast::ImportKind::Module(module_name.clone(), span),
                    alias_default,
                )
            }
            other => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected a file path in quotes or a module name after `{}`, found {}",
                        syntax::KW_USE,
                        describe(other)
                    ),
                    format!(
                        "write `{} \"path/to/file\";` or `{} module_name;`",
                        syntax::KW_USE,
                        syntax::KW_USE
                    ),
                    format!(
                        "e.g. `{} \"util/helpers\";` or `{} scoring;`",
                        syntax::KW_USE,
                        syntax::KW_USE
                    ),
                    Some(self.peek().span),
                ));
            }
        };
        if matches!(self.peek().kind, TokKind::LBrace) {
            return Err(Diagnostic::error(
                "E0003",
                "selective imports aren't part of Jet".to_string(),
                "modules keep their namespace so call sites show where a library function comes from"
                    .to_string(),
                "import the module with `as`, then call items through the alias: `use core.math as math; math.clamp(x, lo, hi);`"
                    .to_string(),
                Some(self.peek().span),
            ));
        }
        let (alias, alias_span) = if matches!(
            &self.peek().kind,
            TokKind::Ident(n) if n == syntax::KW_AS
        ) {
            self.bump();
            let (name, span) = self.expect_ident("after `as`")?;
            (name, span)
        } else {
            (alias_default, start)
        };
        self.expect(TokKind::Semi, "after an import")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::ast::ImportDecl {
            kind,
            alias,
            alias_span,
            span: Span::new(start.start, end),
        })
    }

    fn module_path(&mut self) -> Result<(String, Span), Diagnostic> {
        let (first, first_span) = self.expect_ident("after `import`")?;
        let mut name = first;
        let mut end = first_span.end;
        while matches!(self.peek().kind, TokKind::Dot) {
            self.bump();
            let (part, span) = self.expect_ident("after `.` in an import")?;
            name.push('.');
            name.push_str(&part);
            end = span.end;
        }
        Ok((name, Span::new(first_span.start, end)))
    }

    fn program(&mut self) -> Program {
        let mut imports = Vec::new();
        let mut items = Vec::new();
        loop {
            let r = match &self.peek().kind {
                TokKind::Eof => break,
                TokKind::KwUse => match self.import_decl() {
                    Ok(imp) => {
                        imports.push(imp);
                        continue;
                    }
                    Err(d) => {
                        self.diags.push(d);
                        self.sync_stmt();
                        continue;
                    }
                },
                TokKind::KwUnsafe => {
                    let t = self.bump();
                    let ffi_attempt = matches!(&self.peek().kind, TokKind::KwExtern);
                    self.diags.push(Diagnostic::error(
                        "E0031",
                        format!(
                            "{} doesn't use `{}` to call Rust crates",
                            syntax::LANG_NAME,
                            syntax::KW_UNSAFE
                        ),
                        "foreign Rust functions live in whole `extern rust` blocks — callers never write `unsafe`"
                            .to_string(),
                        format!(
                            "write: {} {} \"crate@version\" {{ fn name(...) -> T = \"rust::path\"; }}",
                            syntax::KW_EXTERN,
                            syntax::KW_RUST
                        ),
                        Some(t.span),
                    ));
                    if ffi_attempt {
                        self.extern_rust_block().map(Item::ExternRust)
                    } else {
                        self.sync_top();
                        continue;
                    }
                }
                TokKind::KwExtern => self.extern_rust_block().map(Item::ExternRust),
                TokKind::KwFn => self.func().map(Item::Func),
                TokKind::KwPub => match self.peek2().kind {
                    TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                    TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                    TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
                    _ => self.func().map(Item::Func),
                },
                TokKind::KwTest => self.test_def().map(Item::Test),
                TokKind::KwModule => self.module_decl().map(Item::Module),
                TokKind::KwStruct => self.struct_def(false).map(Item::Struct),
                TokKind::KwEnum => self.enum_def(false).map(Item::Enum),
                TokKind::KwTrait => self.trait_def(false).map(Item::Trait),
                TokKind::KwImpl => self.impl_def().map(Item::Impl),
                TokKind::At if self.at_c_module() => self.c_module().map(Item::CModule),
                TokKind::At if self.at_unsafe_fn() => self.unsafe_fn().map(Item::Func),
                TokKind::KwConst | TokKind::At => self.const_def().map(Item::Const),
                TokKind::KwComptime => self.comptime_def().map(Item::Const),
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
                TokKind::Ident(name) if name == syntax::FOREIGN_INTERFACE => {
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0022",
                        format!(
                            "`{}` is spelled `{}` in {}",
                            syntax::FOREIGN_INTERFACE,
                            syntax::KW_TRAIT,
                            syntax::LANG_NAME
                        ),
                        format!(
                            "traits are written with `{}` — see docs for `trait Name {{ … }}`",
                            syntax::KW_TRAIT
                        ),
                        format!(
                            "replace `{}` with `{}`",
                            syntax::FOREIGN_INTERFACE,
                            syntax::KW_TRAIT
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
                    self.func_after_fn(false, false).map(Item::Func)
                }
                TokKind::Ident(name) if name == syntax::FOREIGN_IMPORT => {
                    let t = self.bump();
                    self.diags.push(Diagnostic::error(
                        "E0015",
                        format!(
                            "{} uses `{}`, not `{}`",
                            syntax::LANG_NAME,
                            syntax::KW_USE,
                            syntax::FOREIGN_IMPORT
                        ),
                        format!(
                            "other files are brought in with `{} \"path\"` or `{} name` (S16; M6)",
                            syntax::KW_USE,
                            syntax::KW_USE
                        ),
                        format!(
                            "replace with `{} \"path\";`, `{} name;`, or `{} \"path\" {} alias;`",
                            syntax::KW_USE,
                            syntax::KW_USE,
                            syntax::KW_USE,
                            syntax::KW_AS
                        ),
                        Some(t.span),
                    ));
                    self.sync_stmt();
                    continue;
                }
                other => {
                    let d = Diagnostic::error(
                        "E0003",
                        format!(
                            "expected `{}`, `{}`, `{}`, or `{}` here, found {}",
                            syntax::KW_FN,
                            syntax::KW_TEST,
                            syntax::KW_STRUCT,
                            syntax::KW_CONST,
                            describe(other)
                        ),
                        "at the top level of a file, only definitions can appear".to_string(),
                        format!(
                            "define a function ({} main() {{ ... }}), {} block, struct, or const",
                            syntax::KW_FN,
                            syntax::KW_TEST
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
        Program { imports, items }
    }

    fn test_def(&mut self) -> Result<crate::ast::TestDef, Diagnostic> {
        self.expect_kw(TokKind::KwTest, "to start a test block")?;
        let (name, name_span) = self.expect_test_name()?;
        self.expect(TokKind::LBrace, "to open the test body")?;
        let body = self.block_stmts();
        Ok(crate::ast::TestDef {
            name,
            name_span,
            body,
        })
    }

    /// Test names are plain string literals — no interpolation (S43).
    fn expect_test_name(&mut self) -> Result<(String, Span), Diagnostic> {
        let parts = match &self.peek().kind {
            TokKind::Str(parts) => parts.clone(),
            other => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected a test name in quotes after `{}`, found {}",
                        syntax::KW_TEST,
                        describe(other)
                    ),
                    "each test block needs a name so failures are easy to find".to_string(),
                    format!(
                        "write: {} \"describes what this checks\" {{ ... }}",
                        syntax::KW_TEST
                    ),
                    Some(self.peek().span),
                ));
            }
        };
        let span = self.bump().span;
        if parts.len() != 1 {
            return Err(Diagnostic::error(
                "E0003",
                "a test name must be one piece of quoted text".to_string(),
                "test names are labels, not interpolated messages".to_string(),
                format!("write: {} \"my test name\" {{ ... }}", syntax::KW_TEST),
                Some(span),
            ));
        }
        match &parts[0] {
            StrTokPart::Lit(s) => Ok((s.clone(), span)),
            StrTokPart::Interp(_) => Err(Diagnostic::error(
                "E0003",
                "a test name can't contain `{ }` interpolation".to_string(),
                "test names are fixed labels".to_string(),
                format!("write: {} \"my test name\" {{ ... }}", syntax::KW_TEST),
                Some(span),
            )),
        }
    }

    /// S50 (M7): `extern rust "crate@version" { fn … = "rust::path"; }`
    fn extern_rust_block(&mut self) -> Result<crate::ast::ExternRustBlock, Diagnostic> {
        let start = self.bump().span;
        if !matches!(&self.peek().kind, TokKind::Ident(n) if n == syntax::KW_RUST) {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected `{}` after `{}`, found {}",
                    syntax::KW_RUST,
                    syntax::KW_EXTERN,
                    describe(&self.peek().kind)
                ),
                format!(
                    "foreign Rust functions are declared in `{} {} \"crate@version\" {{ … }}`",
                    syntax::KW_EXTERN,
                    syntax::KW_RUST
                ),
                format!(
                    "write: {} {} \"std\" {{ fn name() -> Int = \"std::path\"; }}",
                    syntax::KW_EXTERN,
                    syntax::KW_RUST
                ),
                Some(self.peek().span),
            ));
        }
        self.bump();
        let (crate_spec, crate_span) = self.expect_plain_string(
            "after `extern rust`",
            "the crate name must be one piece of quoted text",
            "write: extern rust \"base64@0.22\" { ... }",
        )?;
        self.expect(TokKind::LBrace, "to open the extern block")?;
        let mut functions = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            functions.push(self.extern_fn()?);
        }
        self.expect(TokKind::RBrace, "to close the extern block")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::ast::ExternRustBlock {
            crate_spec,
            crate_span,
            functions,
            span: Span::new(start.start, end),
        })
    }

    fn extern_fn(&mut self) -> Result<crate::ast::ExternFn, Diagnostic> {
        let fn_span = self.peek().span;
        self.expect_kw(TokKind::KwFn, "to declare a foreign function")?;
        let fn_start = fn_span.start;
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
            let (ty, _) = self.return_type()?;
            return_type = Some(ty);
        }

        self.expect(TokKind::Eq, "before the Rust path")?;
        let (rust_path, rust_path_span) = self.expect_plain_string(
            "after `=`",
            "the Rust path must be one piece of quoted text",
            "write: = \"crate::function\"",
        )?;
        self.expect(TokKind::Semi, "after the foreign path")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::ast::ExternFn {
            name,
            name_span,
            params,
            return_type,
            is_view_return,
            rust_path,
            rust_path_span,
            span: Span::new(fn_start, end),
        })
    }

    /// S58 (E2-M13): is the cursor at `@unsafe fn …`? The whole-function
    /// unsafe contract — `@` then the `unsafe` keyword then `fn`/`pub fn`.
    fn at_unsafe_fn(&self) -> bool {
        matches!(self.peek().kind, TokKind::At)
            && matches!(self.peek2().kind, TokKind::KwUnsafe)
            && matches!(self.peek3().kind, TokKind::KwFn | TokKind::KwPub)
    }

    /// S58 (E2-M13): parse `@unsafe fn name(...) { ... }`. The body is checked
    /// like any other; the contract is enforced at call sites (E3103).
    fn unsafe_fn(&mut self) -> Result<Func, Diagnostic> {
        self.expect(TokKind::At, "before `unsafe`")?;
        self.expect_kw(TokKind::KwUnsafe, "to mark a whole-function contract")?;
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "after `@unsafe`")?;
        self.func_after_fn(is_pub, true)
    }

    /// S58 (E2-M13, D-LL2): parse a `@unsafe { … }` audited region in
    /// statement position, with an optional `@audit("…")` reason on the line
    /// above. The reason is required at runtime by lint L3101, not by the
    /// grammar, so a missing `@audit` parses fine and is flagged in sema.
    fn at_unsafe_stmt(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.peek().span;
        // Optional `@audit("…")`.
        let mut audit = None;
        if matches!(self.peek().kind, TokKind::At)
            && matches!(&self.peek2().kind, TokKind::Ident(n) if n == syntax::ATTR_AUDIT)
        {
            self.bump(); // `@`
            self.bump(); // `audit`
            self.expect(TokKind::LParen, "after `@audit`")?;
            let (reason, _) = self.expect_plain_string(
                "for the audit reason",
                "`@audit` takes one piece of quoted text explaining why the block is safe",
                "write: @audit(\"index checked against len\")",
            )?;
            self.expect(TokKind::RParen, "after the audit reason")?;
            audit = Some(reason);
        }
        // Required `@unsafe { … }`.
        if !(matches!(self.peek().kind, TokKind::At)
            && matches!(self.peek2().kind, TokKind::KwUnsafe))
        {
            return Err(Diagnostic::error(
                "E0003",
                format!("`@{}` must be followed by an `@{}` block", syntax::ATTR_AUDIT, syntax::KW_UNSAFE),
                "an audit reason annotates the gated region it sits above".to_string(),
                format!(
                    "write `@{}(\"…\") @{} {{ … }}`",
                    syntax::ATTR_AUDIT,
                    syntax::KW_UNSAFE
                ),
                Some(self.peek().span),
            ));
        }
        self.bump(); // `@`
        self.bump(); // `unsafe`
        self.expect(TokKind::LBrace, "after `@unsafe`")?;
        let body = self.block_stmts();
        let end = self.toks[self.pos - 1].span.end;
        Ok(Stmt::Unsafe {
            audit,
            body,
            span: Span::new(start.start, end),
        })
    }

    /// S58 (E2-M13): parse the tail of `alias.Ptr<T>.from_addr(addr)`, with the
    /// cursor at the `<`. The `alias`/`alias_span` are the already-parsed
    /// module alias and `.Ptr` member.
    fn ptr_from_addr(&mut self, alias: String, alias_span: Span) -> Result<Expr, Diagnostic> {
        self.expect_type_args_open(syntax::TYPE_PTR)?;
        let (elem, _) = self.type_()?;
        if matches!(self.peek().kind, TokKind::Comma) {
            return Err(Diagnostic::error(
                "E0003",
                format!("`{}<…>` takes exactly one element type", syntax::TYPE_PTR),
                "a pointer points at a single element type".to_string(),
                format!("write `{}.{}<Int>.{}(addr)`", alias, syntax::TYPE_PTR, syntax::MEM_FROM_ADDR),
                Some(self.peek().span),
            ));
        }
        self.expect_type_args_close(&format!("after `{}<…>`", syntax::TYPE_PTR))?;
        self.expect(TokKind::Dot, &format!("after `{}<…>`", syntax::TYPE_PTR))?;
        let (method, method_span) = self.expect_field_name()?;
        if method != syntax::MEM_FROM_ADDR {
            return Err(Diagnostic::error(
                "E0003",
                format!("`{}<…>` has no static method `{}`", syntax::TYPE_PTR, method),
                "a typed pointer is built from an address".to_string(),
                format!("write `{}.{}<Int>.{}(addr)`", alias, syntax::TYPE_PTR, syntax::MEM_FROM_ADDR),
                Some(method_span),
            ));
        }
        self.expect(TokKind::LParen, &format!("after `{}`", syntax::MEM_FROM_ADDR))?;
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

    /// S59 (E2-M14): is the cursor at the start of a C FFI module — `@extern
    /// module …` or `@bindgen module …`? (Distinguishes from `@static const`,
    /// and from bare `extern rust`.)
    fn at_c_module(&self) -> bool {
        if !matches!(self.peek().kind, TokKind::At) {
            return false;
        }
        let intro_is_c = match &self.peek2().kind {
            TokKind::KwExtern => true,
            TokKind::Ident(n) => n == syntax::ATTR_BINDGEN,
            _ => false,
        };
        intro_is_c && matches!(self.peek3().kind, TokKind::KwModule)
    }

    /// S59 (E2-M14): parse `@extern module c.<lib> { … }` (overlay) or
    /// `@bindgen module c.<lib>.__bindgen__ { … }` (generated cache). Body
    /// declarations share the `extern_fn` shape (`fn name(args) -> T = "Sym";`).
    fn c_module(&mut self) -> Result<crate::ast::CModule, Diagnostic> {
        use crate::ast::CModuleKind;
        let start = self.bump().span; // `@`
        let kind = match &self.peek().kind {
            TokKind::KwExtern => {
                self.bump();
                CModuleKind::Extern
            }
            TokKind::Ident(n) if n == syntax::ATTR_BINDGEN => {
                self.bump();
                CModuleKind::Bindgen
            }
            other => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected `{}` or `{}` after `@`, found {}",
                        syntax::ATTR_EXTERN_MODULE,
                        syntax::ATTR_BINDGEN,
                        describe(other)
                    ),
                    "a C FFI module begins with `@extern module c.<lib>` or `@bindgen module c.<lib>.__bindgen__`".to_string(),
                    "write: @extern module c.raylib { fn init_window(w: Int, h: Int, title: String) = \"InitWindow\"; }".to_string(),
                    Some(self.peek().span),
                ));
            }
        };
        self.expect_kw(TokKind::KwModule, "to declare a C FFI module")?;

        // Parse the dotted module path: `c` `.` `<lib>` [ `.` `__bindgen__` ].
        let path_start = self.peek().span;
        let (root, _) = self.expect_ident("after `module`")?;
        if root != syntax::C_MODULE_ROOT {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "a C FFI module path starts with `{}.`, found `{}`",
                    syntax::C_MODULE_ROOT, root
                ),
                "C libraries live under the `c.` module root — `c.raylib`, `c.sqlite3`".to_string(),
                format!("write: {} module {}.<lib> {{ … }}",
                    match kind { CModuleKind::Extern => "@extern", CModuleKind::Bindgen => "@bindgen" },
                    syntax::C_MODULE_ROOT),
                Some(path_start),
            ));
        }
        self.expect(TokKind::Dot, "after `c` in a C FFI module path")?;
        let (lib, lib_span) = self.expect_ident("for the C library name")?;
        let mut has_bindgen_seg = false;
        let mut path_end = lib_span.end;
        if matches!(self.peek().kind, TokKind::Dot) {
            self.bump();
            let (seg, seg_span) = self.expect_ident("after `.` in a C FFI module path")?;
            path_end = seg_span.end;
            if seg == syntax::C_BINDGEN_SEGMENT {
                has_bindgen_seg = true;
            } else {
                return Err(Diagnostic::error(
                    "E0003",
                    format!("a C FFI module path can't have a `.{}` segment", seg),
                    "the only legal third segment is the reserved `__bindgen__` on a generated cache module".to_string(),
                    format!("write: @extern module {}.{} {{ … }}", syntax::C_MODULE_ROOT, lib),
                    Some(seg_span),
                ));
            }
        }
        let path_span = Span::new(path_start.start, path_end);

        // E3206: a user overlay must not name the reserved `__bindgen__` segment.
        if kind == CModuleKind::Extern && has_bindgen_seg {
            return Err(Diagnostic::error(
                "E3206",
                format!(
                    "module path `{}.{}.{}` uses the reserved segment `{}`",
                    syntax::C_MODULE_ROOT, lib, syntax::C_BINDGEN_SEGMENT, syntax::C_BINDGEN_SEGMENT
                ),
                format!(
                    "autogen lives in `{}.<lib>.{}`; users declare overlays as `@{} module {}.<lib>` only",
                    syntax::C_MODULE_ROOT, syntax::C_BINDGEN_SEGMENT, syntax::ATTR_EXTERN_MODULE, syntax::C_MODULE_ROOT
                ),
                format!(
                    "drop `{}` from your module path, or use `@{} module {}.{} {{ … }}`",
                    syntax::C_BINDGEN_SEGMENT, syntax::ATTR_EXTERN_MODULE, syntax::C_MODULE_ROOT, lib
                ),
                Some(path_span),
            ));
        }
        // A `@bindgen` module must carry the `__bindgen__` segment (it is the
        // generated surface). Without it the path is malformed.
        if kind == CModuleKind::Bindgen && !has_bindgen_seg {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "a `@bindgen` module path must end in `.{}`",
                    syntax::C_BINDGEN_SEGMENT
                ),
                "the compiler generates `@bindgen module c.<lib>.__bindgen__` cache files".to_string(),
                format!(
                    "write: @bindgen module {}.{}.{} {{ … }}",
                    syntax::C_MODULE_ROOT, lib, syntax::C_BINDGEN_SEGMENT
                ),
                Some(path_span),
            ));
        }

        self.expect(TokKind::LBrace, "to open the C FFI module body")?;
        let mut functions = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            functions.push(self.extern_fn()?);
        }
        self.expect(TokKind::RBrace, "to close the C FFI module body")?;
        let end = self.toks[self.pos - 1].span.end;
        Ok(crate::ast::CModule {
            kind,
            lib,
            path_span,
            functions,
            span: Span::new(start.start, end),
        })
    }

    fn expect_plain_string(
        &mut self,
        context: &str,
        why_interp: &str,
        fix: &str,
    ) -> Result<(String, Span), Diagnostic> {
        let parts = match &self.peek().kind {
            TokKind::Str(parts) => parts.clone(),
            other => {
                return Err(Diagnostic::error(
                    "E0003",
                    format!(
                        "expected a piece of quoted text {}, found {}",
                        context,
                        describe(other)
                    ),
                    why_interp.to_string(),
                    fix.to_string(),
                    Some(self.peek().span),
                ));
            }
        };
        let span = self.bump().span;
        if parts.len() != 1 {
            return Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected a piece of quoted text {}, found interpolation",
                    context
                ),
                why_interp.to_string(),
                fix.to_string(),
                Some(span),
            ));
        }
        match &parts[0] {
            StrTokPart::Lit(s) => Ok((s.clone(), span)),
            StrTokPart::Interp(_) => Err(Diagnostic::error(
                "E0003",
                format!(
                    "expected a piece of quoted text {}, found interpolation",
                    context
                ),
                why_interp.to_string(),
                fix.to_string(),
                Some(span),
            )),
        }
    }

    fn func(&mut self) -> Result<Func, Diagnostic> {
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "to start a function definition")?;
        self.func_after_fn(is_pub, false)
    }

    fn func_after_fn(&mut self, is_pub: bool, is_unsafe: bool) -> Result<Func, Diagnostic> {
        let (name, name_span) = self.expect_ident("after `fn`")?;
        let type_params = self.parse_opt_type_params()?;
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
            let (ty, _) = self.return_type()?;
            return_type = Some(ty);
        }

        self.expect(TokKind::LBrace, "to open the function body")?;
        let body = self.block_stmts();
        Ok(Func {
            is_pub,
            name,
            name_span,
            type_params,
            params,
            return_type,
            is_view_return,
            is_unsafe,
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
        let type_params = self.parse_opt_type_params()?;
        self.expect(TokKind::LBrace, "to open the struct body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut trait_impls = Vec::new();
        let mut derives = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::KwDerive) {
                derives.push(self.derive_line()?);
            } else if matches!(self.peek().kind, TokKind::KwImpl) {
                trait_impls.push(self.trait_impl_block()?);
            } else {
                let is_method = matches!(self.peek().kind, TokKind::KwFn)
                    || (matches!(self.peek().kind, TokKind::KwPub)
                        && matches!(self.peek2().kind, TokKind::KwFn));
                if is_method {
                    methods.push(self.method_in_type()?);
                } else {
                    fields.push(self.field()?);
                    if matches!(self.peek().kind, TokKind::Comma | TokKind::Semi) {
                        self.bump();
                    }
                }
            }
        }
        self.bump(); // }
        Ok(StructDef {
            is_pub,
            name,
            name_span,
            type_params,
            fields,
            methods,
            trait_impls,
            derives,
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
        let type_params = self.parse_opt_type_params()?;
        self.expect(TokKind::LBrace, "to open the enum body")?;
        let mut variants = Vec::new();
        let mut methods = Vec::new();
        let mut trait_impls = Vec::new();
        let mut derives = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            if matches!(self.peek().kind, TokKind::KwDerive) {
                derives.push(self.derive_line()?);
            } else if matches!(self.peek().kind, TokKind::KwImpl) {
                trait_impls.push(self.trait_impl_block()?);
            } else if matches!(self.peek().kind, TokKind::KwFn | TokKind::KwPub) {
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
            type_params,
            variants,
            methods,
            trait_impls,
            derives,
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
        let (type_name, type_span) = self.parse_type_path("after `impl`")?;
        let (trait_name, trait_span) = if matches!(self.peek().kind, TokKind::Colon) {
            self.bump();
            let (t, ts) = self.expect_ident("after `:` in `impl Type: Trait`")?;
            (Some(t), Some(ts))
        } else {
            (None, None)
        };
        self.expect(TokKind::LBrace, "to open the `impl` body")?;
        let mut methods = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            methods.push(self.method_in_type()?);
        }
        self.bump();
        Ok(ImplDef {
            type_name,
            type_span,
            trait_name,
            trait_span,
            methods,
        })
    }

    /// S28: `impl Trait { … }` inside a struct/enum body.
    fn trait_impl_block(&mut self) -> Result<TraitImplBlock, Diagnostic> {
        self.expect_kw(TokKind::KwImpl, "to start a trait impl block")?;
        let (trait_name, trait_span) = self.expect_ident("after `impl`")?;
        self.expect(TokKind::LBrace, "to open the trait impl body")?;
        let mut methods = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            methods.push(self.method_in_type()?);
        }
        self.bump();
        Ok(TraitImplBlock {
            trait_name,
            trait_span,
            methods,
        })
    }

    /// S55: `derive Comparable;` inside a type body.
    fn derive_line(&mut self) -> Result<(String, Span), Diagnostic> {
        let start = self.bump().span;
        let (trait_name, _) = self.expect_ident("after `derive`")?;
        self.finish_stmt()?;
        Ok((trait_name, start))
    }

    /// S28: top-level `trait Name { fn sig(self) -> T; … }`.
    fn trait_def(&mut self, nested: bool) -> Result<TraitDef, Diagnostic> {
        let is_pub = if nested {
            false
        } else {
            matches!(self.peek().kind, TokKind::KwPub)
        };
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwTrait, "to start a trait definition")?;
        let (name, name_span) = self.expect_ident("after `trait`")?;
        self.expect(TokKind::LBrace, "to open the trait body")?;
        let mut methods = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            methods.push(self.trait_method_sig()?);
        }
        self.bump();
        Ok(TraitDef {
            is_pub,
            name,
            name_span,
            methods,
        })
    }

    fn trait_method_sig(&mut self) -> Result<TraitMethodSig, Diagnostic> {
        let start = self.peek().span;
        self.expect_kw(TokKind::KwFn, "to start a trait method signature")?;
        let (name, name_span) = self.expect_ident("after `fn`")?;
        self.expect(TokKind::LParen, "after the method name")?;
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
            let (ty, _) = self.return_type()?;
            return_type = Some(ty);
        }
        let end = self.peek().span.end;
        self.finish_stmt()?;
        Ok(TraitMethodSig {
            name,
            name_span,
            params,
            return_type,
            is_view_return,
            span: Span::new(start.start, end),
        })
    }

    fn parse_opt_type_params(&mut self) -> Result<Vec<TypeParam>, Diagnostic> {
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

    fn parse_trait_bounds(&mut self) -> Result<Vec<String>, Diagnostic> {
        let mut bounds = Vec::new();
        loop {
            let (name, _) = self.expect_ident("for a trait bound")?;
            bounds.push(name);
            if matches!(self.peek().kind, TokKind::Plus) {
                self.bump();
                continue;
            }
            break;
        }
        Ok(bounds)
    }

    fn parse_type_path(&mut self, where_: &str) -> Result<(String, Span), Diagnostic> {
        let (first, span) = self.expect_ident(where_)?;
        let mut name = first;
        while matches!(self.peek().kind, TokKind::Dot) {
            self.bump();
            let (part, _) = self.expect_ident("after `.` in a type path")?;
            name = format!("{name}.{part}");
        }
        Ok((name, span))
    }

    /// S27: method inside a type body or `impl` block.
    fn method_in_type(&mut self) -> Result<Func, Diagnostic> {
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
        self.expect_kw(TokKind::KwFn, "to start a method")?;
        self.func_after_fn(is_pub, false)
    }

    fn field(&mut self) -> Result<Field, Diagnostic> {
        let is_pub = matches!(self.peek().kind, TokKind::KwPub);
        if is_pub {
            self.bump();
        }
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
            is_pub,
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
            is_comptime: false,
            ct: None,
        })
    }

    /// S57 (M9.5): `comptime NAME = expr;` — a compile-time constant binding.
    fn comptime_def(&mut self) -> Result<ConstDef, Diagnostic> {
        let kw = self.peek().span;
        self.expect_kw(TokKind::KwComptime, "to start a comptime binding")?;
        // E0954: `comptime val` / `comptime var` — one keyword suffices.
        if matches!(self.peek().kind, TokKind::KwVal | TokKind::KwVar) {
            let extra = self.peek().span;
            return Err(Diagnostic::error(
                "E0954",
                format!(
                    "write `{} NAME = ...`, not `{} {} NAME = ...`",
                    syntax::KW_COMPTIME,
                    syntax::KW_COMPTIME,
                    if matches!(self.peek().kind, TokKind::KwVal) {
                        syntax::KW_VAL
                    } else {
                        syntax::KW_VAR
                    }
                ),
                format!(
                    "`{}` is already the binding keyword, and a comptime value is always a constant",
                    syntax::KW_COMPTIME
                ),
                format!("remove the extra keyword: `{} NAME = ...`", syntax::KW_COMPTIME),
                Some(Span::new(kw.start, extra.end)),
            ));
        }
        let (name, name_span) = self.expect_ident("after `comptime`")?;
        self.expect(TokKind::Eq, "after the comptime name")?;
        let value = self.expr()?;
        self.expect(TokKind::Semi, "after a comptime value")?;
        Ok(ConstDef {
            name,
            name_span,
            value,
            attrs: Vec::new(),
            rust_kind: crate::ast::RustConstKind::Const,
            is_comptime: true,
            ct: None,
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
            TokKind::KwTest => {
                let span = self.peek().span;
                self.bump();
                if matches!(self.peek().kind, TokKind::Str(_)) {
                    self.bump();
                }
                if matches!(self.peek().kind, TokKind::LBrace) {
                    self.bump();
                    let _ = self.block_stmts();
                } else {
                    self.sync_stmt();
                }
                Err(Diagnostic::error(
                    "E0601",
                    format!("`{}` blocks only belong at the top of a file", syntax::KW_TEST),
                    "test blocks group checks that `jet test` runs separately from `main`"
                        .to_string(),
                    format!(
                        "move this block to the top level, after your functions: {} \"name\" {{ ... }}",
                        syntax::KW_TEST
                    ),
                    Some(span),
                ))
            }
            TokKind::KwVal | TokKind::KwVar => {
                let binding = self.binding()?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::KwComptime => {
                let binding = self.comptime_binding()?;
                self.finish_stmt()?;
                Ok(Stmt::Val(binding))
            }
            TokKind::Ident(n) if n == syntax::FOREIGN_LET => {
                // S14 teaching error E0009, then parse as a binding.
                let t = self.bump();
                let is_mut = matches!(self.peek().kind, TokKind::KwMutate);
                if is_mut {
                    let mut_tok = self.bump();
                    let full_span = Span::new(t.span.start, mut_tok.span.end);
                    self.diags.push(Diagnostic::error(
                        "E0009",
                        format!(
                            "{} does not use `{}`",
                            syntax::LANG_NAME,
                            syntax::FOREIGN_LET_MUT
                        ),
                        binding_why(),
                        format!(
                            "replace `{}` with `{}`",
                            syntax::FOREIGN_LET_MUT,
                            syntax::KW_VAR
                        ),
                        Some(full_span),
                    ));
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0009",
                        format!(
                            "{} does not use `{}`",
                            syntax::LANG_NAME,
                            syntax::FOREIGN_LET
                        ),
                        binding_why(),
                        format!(
                            "replace `{}` with `{}`",
                            syntax::FOREIGN_LET,
                            syntax::KW_VAL
                        ),
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
                    format!(
                        "{} does not use `{}`",
                        syntax::LANG_NAME,
                        syntax::FOREIGN_SET
                    ),
                    binding_why(),
                    format!(
                        "replace `{}` with `{}`",
                        syntax::FOREIGN_SET,
                        syntax::KW_VAL
                    ),
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
                    format!(
                        "{} does not use `{}`",
                        syntax::LANG_NAME,
                        syntax::FOREIGN_MATCH
                    ),
                    format!(
                        "choosing one branch from many is written with `{}`",
                        syntax::KW_SWITCH
                    ),
                    format!(
                        "replace `{}` with `{}`",
                        syntax::FOREIGN_MATCH,
                        syntax::KW_SWITCH
                    ),
                    Some(t.span),
                ));
                self.switch_after_kw(t.span)
            }
            TokKind::Ident(n) if n == syntax::FOREIGN_SWITCH => {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0044",
                    format!(
                        "{} renamed `{}` to `{}`",
                        syntax::LANG_NAME,
                        syntax::FOREIGN_SWITCH,
                        syntax::KW_SWITCH
                    ),
                    format!(
                        "choosing one branch from many is written with `{}` (S24)",
                        syntax::KW_SWITCH
                    ),
                    format!(
                        "replace `{}` with `{}`",
                        syntax::FOREIGN_SWITCH,
                        syntax::KW_SWITCH
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
            TokKind::KwIf => Ok(Stmt::If(self.if_stmt()?)),
            TokKind::KwWhile => {
                // S19-amend (E0050): `while` is now a teaching error; use `loop cond { }`.
                let t = self.bump();
                let span = t.span;
                self.diags.push(Diagnostic::error(
                    "E0050",
                    format!(
                        "`{}` is not a keyword; write `{}` instead",
                        syntax::FOREIGN_WHILE,
                        syntax::KW_LOOP,
                    ),
                    format!(
                        "`{}` has a single loop keyword: `loop cond {{ }}` for conditional loops",
                        syntax::LANG_NAME,
                    ),
                    format!(
                        "replace `{}` with `{}`",
                        syntax::FOREIGN_WHILE,
                        syntax::KW_LOOP,
                    ),
                    Some(span),
                ));
                let cond = self.expr_no_struct_lit()?;
                self.expect(TokKind::LBrace, "to open the loop body")?;
                let body = self.block_stmts();
                Ok(Stmt::While { cond, body, span })
            }
            TokKind::KwFor => {
                // S19-amend (E0051): `for` is now a teaching error; use `loop x in ... { }`.
                let t = self.bump();
                let span = t.span;
                self.diags.push(Diagnostic::error(
                    "E0051",
                    format!(
                        "`{}` is not a keyword; write `{} x in collection {{ }}` instead",
                        syntax::FOREIGN_FOR,
                        syntax::KW_LOOP,
                    ),
                    format!(
                        "`{}` has a single loop keyword: `loop x in list {{ }}` for iteration",
                        syntax::LANG_NAME,
                    ),
                    format!(
                        "replace `{}` with `{}`",
                        syntax::FOREIGN_FOR,
                        syntax::KW_LOOP,
                    ),
                    Some(span),
                ));
                let (var, var_span) = self.expect_ident("after the loop variable name")?;
                let mut var2 = None;
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    let (v2, s2) = self.expect_ident("after `,` in `loop key, value in`")?;
                    var2 = Some((v2, s2));
                }
                self.expect_kw(TokKind::KwIn, "after the loop name")?;
                let first = self.expr_no_struct_lit()?;
                let kind = if matches!(self.peek().kind, TokKind::DotDot) {
                    self.bump();
                    let end = self.expr_no_struct_lit()?;
                    let step = if matches!(&self.peek().kind, TokKind::Ident(n) if n == syntax::KW_RANGE_STEP)
                    {
                        self.bump();
                        Some(self.expr_no_struct_lit()?)
                    } else {
                        None
                    };
                    ForKind::Range { start: first, end, step }
                } else {
                    ForKind::In { collection: first }
                };
                self.expect(TokKind::LBrace, "to open the loop body")?;
                let body = self.block_stmts();
                Ok(Stmt::For { var, var_span, var2, kind, body, span })
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
                // S19-amend: `loop` handles all three loop forms by header.
                //   loop { }               → infinite
                //   loop cond { }          → conditional (was `while`)
                //   loop x in ... { }      → iteration (was `for`)
                //   loop k, v in ... { }   → key-value iteration
                if matches!(self.peek().kind, TokKind::LBrace) {
                    // Infinite loop
                    self.bump();
                    let inner = self.block_stmts();
                    Ok(Stmt::Loop(inner, span))
                } else if matches!(&self.peek().kind, TokKind::Ident(_))
                    && matches!(
                        &self.peek2().kind,
                        TokKind::KwIn | TokKind::Comma
                    )
                {
                    // Iteration: loop x in ... { } or loop k, v in ... { }
                    let (var, var_span) = self.expect_ident("as the loop variable")?;
                    let mut var2 = None;
                    if matches!(self.peek().kind, TokKind::Comma) {
                        self.bump();
                        let (v2, s2) = self.expect_ident("after `,` in `loop key, value in`")?;
                        var2 = Some((v2, s2));
                    }
                    self.expect_kw(TokKind::KwIn, "after the loop variable")?;
                    let first = self.expr_no_struct_lit()?;
                    let kind = if matches!(self.peek().kind, TokKind::DotDot) {
                        self.bump();
                        let end = self.expr_no_struct_lit()?;
                        let step = if matches!(&self.peek().kind, TokKind::Ident(n) if n == syntax::KW_RANGE_STEP)
                        {
                            self.bump();
                            Some(self.expr_no_struct_lit()?)
                        } else {
                            None
                        };
                        ForKind::Range { start: first, end, step }
                    } else {
                        ForKind::In { collection: first }
                    };
                    self.expect(TokKind::LBrace, "to open the loop body")?;
                    let body = self.block_stmts();
                    Ok(Stmt::For { var, var_span, var2, kind, body, span })
                } else {
                    // Conditional: loop cond { }
                    let cond = self.expr_no_struct_lit()?;
                    self.expect(TokKind::LBrace, "to open the loop body")?;
                    let body = self.block_stmts();
                    Ok(Stmt::While { cond, body, span })
                }
            }
            // S58 (E2-M13): the audit + unsafe gate is `@audit("…")` then
            // `@unsafe { … }`. Bare `unsafe { … }` is the rejected former
            // spelling — point users at the `@` form.
            TokKind::KwUnsafe => {
                let span = self.bump().span;
                Err(Diagnostic::error(
                    "E0003",
                    format!("`{}` blocks are written with `@`", syntax::KW_UNSAFE),
                    "the expert low-level gate is an attribute marker, never a bare keyword"
                        .to_string(),
                    format!(
                        "write `@{}(\"why this is safe\") @{} {{ … }}`",
                        syntax::ATTR_AUDIT,
                        syntax::KW_UNSAFE
                    ),
                    Some(span),
                ))
            }
            TokKind::At => self.at_unsafe_stmt(),
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
                    Expr::Call(_) | Expr::Field(_, _, _) | Expr::MethodCall { .. } => {}
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

    /// `switch` body, after the keyword (S24): either legacy condition arms
    /// with `->`, or pipe arms where bare terms mean `subject == term`.
    /// S68 (D-SG2): parse an `if` expression — `if cond { … value } else { … }`.
    /// Each branch is a value block; `else` is required (an `if` with no value
    /// is a statement, parsed elsewhere).
    fn parse_if_expr(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.bump().span; // `if`
        let cond = self.expr_no_struct_lit()?;
        let (then_body, then_value) = self.parse_value_block()?;
        if !matches!(self.peek().kind, TokKind::KwElse) {
            return Err(Diagnostic::error(
                "E0003",
                "an `if` used as a value needs an `else` branch".to_string(),
                "in expression position both outcomes must produce a value (S68)".to_string(),
                "add `else { … }` so every path has a value".to_string(),
                Some(self.peek().span),
            ));
        }
        self.bump(); // `else`
        // `else if …` nests as the else branch's value.
        let (else_body, else_value) = if matches!(self.peek().kind, TokKind::KwIf) {
            let e = self.parse_if_expr()?;
            (Vec::new(), e)
        } else {
            self.parse_value_block()?
        };
        let span = Span::new(start.start, else_value.span().end);
        Ok(Expr::If {
            cond: Box::new(cond),
            then_body,
            then_value: Box::new(then_value),
            else_body,
            else_value: Box::new(else_value),
            span,
        })
    }

    /// S68 (D-SG2): parse `{ stmt* tail-expr }` where the trailing expression
    /// (no `;`) is the block's value. Leading statements use the ordinary
    /// statement grammar; the tail is detected by speculatively parsing an
    /// expression and checking for the closing `}`.
    fn parse_value_block(&mut self) -> Result<(Vec<Stmt>, Expr), Diagnostic> {
        self.expect(TokKind::LBrace, "to open this `if` branch")?;
        let mut stmts = Vec::new();
        loop {
            match &self.peek().kind {
                TokKind::RBrace => {
                    let span = self.peek().span;
                    self.bump();
                    return Err(Diagnostic::error(
                        "E0003",
                        "this `if` branch is empty but is used as a value".to_string(),
                        "an `if` in expression position must end each branch with a value (S68)"
                            .to_string(),
                        "put a value as the last line, like `{ x }`".to_string(),
                        Some(span),
                    ));
                }
                TokKind::Eof => {
                    return Err(Diagnostic::error(
                        "E0003",
                        "expected `}` to close this `if` branch, found the end of the file"
                            .to_string(),
                        "every `{` needs a matching `}`".to_string(),
                        "add a closing `}`".to_string(),
                        Some(self.peek().span),
                    ));
                }
                _ => {}
            }
            // Try the current position as the trailing value expression.
            let save = self.pos;
            let saved_diags = self.diags.len();
            if let Ok(e) = self.expr() {
                if matches!(self.peek().kind, TokKind::RBrace) {
                    self.bump();
                    return Ok((stmts, e));
                }
            }
            // Not the tail value — rewind and parse an ordinary statement.
            self.pos = save;
            self.diags.truncate(saved_diags);
            match self.stmt() {
                Ok(s) => stmts.push(s),
                Err(d) => {
                    self.diags.push(d);
                    self.sync_stmt();
                }
            }
        }
    }

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
                TokKind::Pipe => {
                    let arm_start = self.bump().span;
                    if matches!(self.peek().kind, TokKind::KwElse) {
                        self.bump();
                        self.expect(TokKind::LBrace, "to open the `else` arm")?;
                        let body = self.block_stmts();
                        if matches!(self.peek().kind, TokKind::Semi) {
                            self.bump();
                        }
                        else_body = Some(body);
                    } else {
                        let raw_cond = self.expr_no_struct_lit()?;
                        let cond = Self::switch_pipe_cond(subject.clone(), raw_cond);
                        self.expect(TokKind::LBrace, "to open the arm's body")?;
                        let body = self.block_stmts();
                        let end = self.peek().span.end;
                        if matches!(self.peek().kind, TokKind::Semi) {
                            self.bump();
                        }
                        arms.push(SwitchArm {
                            cond,
                            body,
                            span: Span::new(arm_start.start, end),
                        });
                    }
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
                    // Capture the `;` end so SwitchArm.span covers the full arm.
                    let semi_end = self.peek().span.end;
                    self.expect(TokKind::Semi, "after a `switch` arm's closing `}`")?;
                    arms.push(SwitchArm {
                        cond,
                        body,
                        span: Span::new(arm_start.start, semi_end),
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

    fn switch_pipe_cond(subject: Expr, cond: Expr) -> Expr {
        match cond {
            Expr::Binary(BinOp::And, lhs, rhs, span) => Expr::Binary(
                BinOp::And,
                Box::new(Self::switch_pipe_cond(subject.clone(), *lhs)),
                Box::new(Self::switch_pipe_cond(subject, *rhs)),
                span,
            ),
            Expr::Binary(BinOp::Or, lhs, rhs, span) => Expr::Binary(
                BinOp::Or,
                Box::new(Self::switch_pipe_cond(subject.clone(), *lhs)),
                Box::new(Self::switch_pipe_cond(subject, *rhs)),
                span,
            ),
            Expr::Binary(op, lhs, rhs, span) if op.is_comparison() => {
                Expr::Binary(op, lhs, rhs, span)
            }
            Expr::PatternTest { .. } | Expr::Bool(_, _) => cond,
            other => {
                let span = Span::new(subject.span().start, other.span().end);
                Expr::Binary(BinOp::Eq, Box::new(subject), Box::new(other), span)
            }
        }
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
        // S74: a destructuring target — `[ … ]` for a list, `Ident { … }` for a
        // struct — instead of a plain `name`.
        if let Some(pattern) = self.try_bind_pattern()? {
            self.expect(TokKind::Eq, "in a binding")?;
            let init = self.expr()?;
            return Ok(Binding {
                mutable,
                name: String::new(),
                name_span: pattern.span(),
                pattern: Some(pattern),
                ty: None,
                ty_span: None,
                init,
                is_comptime: false,
                ct: None,
            });
        }
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
            pattern: None,
            ty,
            ty_span,
            init,
            is_comptime: false,
            ct: None,
        })
    }

    /// S74: parse a `val`/`var` destructuring target if one starts here.
    /// `[ a, b ]` is a list pattern; `Ident { x, y }` is a struct pattern.
    /// A bare `name` (followed by `=` or `:`) is not a pattern.
    fn try_bind_pattern(&mut self) -> Result<Option<BindPattern>, Diagnostic> {
        match &self.peek().kind {
            TokKind::LBracket => {
                let start = self.bump().span;
                let mut elems = Vec::new();
                if !matches!(self.peek().kind, TokKind::RBracket) {
                    loop {
                        let (name, span) = self.expect_ident("for a list-pattern binding")?;
                        elems.push(BindName { name, span });
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                }
                let end = self.peek().span;
                self.expect(TokKind::RBracket, "to close the list pattern")?;
                Ok(Some(BindPattern::List {
                    elems,
                    span: Span::new(start.start, end.end),
                }))
            }
            TokKind::Ident(_) if matches!(self.peek2().kind, TokKind::LBrace) => {
                let (type_name, type_span) = self.expect_ident("for a struct pattern")?;
                self.expect(TokKind::LBrace, "to open the struct pattern")?;
                let mut fields = Vec::new();
                if !matches!(self.peek().kind, TokKind::RBrace) {
                    loop {
                        let (name, span) = self.expect_ident("for a struct-pattern field")?;
                        fields.push(BindName { name, span });
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                }
                let end = self.peek().span;
                self.expect(TokKind::RBrace, "to close the struct pattern")?;
                Ok(Some(BindPattern::Struct {
                    type_name,
                    type_span,
                    fields,
                    span: Span::new(type_span.start, end.end),
                }))
            }
            TokKind::LParen if !self.looks_like_named_tuple(false) => {
                let start = self.bump().span;
                let mut elems = Vec::new();
                if !matches!(self.peek().kind, TokKind::RParen) {
                    loop {
                        let (name, span) =
                            self.expect_ident("for a tuple-pattern binding")?;
                        elems.push(BindName { name, span });
                        if matches!(self.peek().kind, TokKind::Comma) {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                }
                let end = self.peek().span;
                self.expect(TokKind::RParen, "to close the tuple pattern")?;
                Ok(Some(BindPattern::Tuple {
                    elems,
                    span: Span::new(start.start, end.end),
                }))
            }
            _ => Ok(None),
        }
    }

    /// S73: `( name : … )` — named tuple literal or type, not grouping.
    /// When `lparen_consumed` is true, `self.pos` is already on the first member name.
    fn looks_like_named_tuple(&self, lparen_consumed: bool) -> bool {
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
                syntax::LANG_NAME
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
                syntax::LANG_NAME
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
            crate::ast::canonicalize_tuple_fields(fields)
                .into_iter()
                .map(|(n, t)| (n, Box::new(t)))
                .collect(),
        ))
    }

    fn parse_paren_primary(&mut self, allow_struct_lit: bool) -> Result<Expr, Diagnostic> {
        let open = self.bump().span;
        if self.looks_like_named_tuple(true) {
            return self.parse_tuple_lit(open);
        }
        if self.after_lparen_is_positional_tuple() {
            self.emit_positional_tuple_error(open);
            self.sync_to_rparen();
            return Ok(Expr::Int(0, open));
        }
        let inner = self.expr()?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.emit_positional_tuple_error(open);
            self.sync_to_rparen();
            return Ok(Expr::Int(0, open));
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

    fn comptime_binding(&mut self) -> Result<Binding, Diagnostic> {
        let kw = self.peek().span;
        self.expect_kw(TokKind::KwComptime, "to start a comptime binding")?;
        if matches!(self.peek().kind, TokKind::KwVal | TokKind::KwVar) {
            let extra = self.peek().span;
            return Err(Diagnostic::error(
                "E0954",
                format!(
                    "write `{} NAME = ...`, not `{} {} NAME = ...`",
                    syntax::KW_COMPTIME,
                    syntax::KW_COMPTIME,
                    if matches!(self.peek().kind, TokKind::KwVal) {
                        syntax::KW_VAL
                    } else {
                        syntax::KW_VAR
                    }
                ),
                format!(
                    "`{}` is already the binding keyword, and a comptime value is always a constant",
                    syntax::KW_COMPTIME
                ),
                format!("remove the extra keyword: `{} NAME = ...`", syntax::KW_COMPTIME),
                Some(Span::new(kw.start, extra.end)),
            ));
        }
        let (name, name_span) = self.expect_ident("after `comptime`")?;
        self.expect(TokKind::Eq, "in a comptime binding")?;
        let init = self.expr()?;
        Ok(Binding {
            mutable: false,
            name,
            name_span,
            pattern: None,
            ty: None,
            ty_span: None,
            init,
            is_comptime: true,
            ct: None,
        })
    }

    // --- expressions -----------------------------------------------------

    fn expr(&mut self) -> Result<Expr, Diagnostic> {
        let span = self.peek().span;
        self.with_nesting(span, |p| p.expr_or_fallback(true))
    }

    fn expr_no_struct_lit(&mut self) -> Result<Expr, Diagnostic> {
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
                TokKind::Ident(n) if n == syntax::FOREIGN_OR_FALLBACK => {
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
            if call.name == syntax::BUILTIN_PANIC {
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
                if n == syntax::FOREIGN_NOT && self.starts_expr(&self.peek2().kind) =>
            {
                self.foreign_logic_error(syntax::FOREIGN_NOT, syntax::OP_NOT);
                let span = self.bump().span;
                let inner = self.expr_unary(allow_struct_lit)?;
                let full = Span::new(span.start, inner.span().end);
                Ok(Expr::Unary(UnOp::Not, Box::new(inner), full))
            }
            TokKind::Ident(n)
                if n == syntax::FOREIGN_TRY && self.starts_expr(&self.peek2().kind) =>
            {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0014",
                    format!(
                        "{} does not use `{}`",
                        syntax::LANG_NAME,
                        syntax::FOREIGN_TRY
                    ),
                    format!(
                        "a call that can fail is marked with `{}` after it, like `parse(x){}`",
                        syntax::OP_TRY_SUFFIX,
                        syntax::OP_TRY_SUFFIX
                    ),
                    format!("write `parse(x){}` instead", syntax::OP_TRY_SUFFIX),
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
                    if member == syntax::TYPE_PTR
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
                        };
                    } else {
                        expr = Expr::Field(Box::new(expr), member, member_span);
                    }
                }
                TokKind::Question => {
                    let qspan = self.bump().span;
                    let full = Span::new(expr.span().start, qspan.end);
                    expr = Expr::Try(Box::new(expr), full);
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
                            kind: crate::ast::IndexKind::Unknown,
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

    fn expr_to_lvalue(&mut self, expr: Expr) -> Result<LValue, Diagnostic> {
        match expr {
            Expr::Ident(name, name_span) => Ok(LValue::Local { name, name_span }),
            Expr::Index {
                base, index, span, ..
            } => Ok(LValue::Index {
                base,
                index,
                span,
                kind: crate::ast::IndexKind::Unknown,
            }),
            other => Err(Diagnostic::error(
                "E0003",
                "this value can't be assigned to".to_string(),
                "only a name or an indexed slot like `items[0]` can appear on the left of `=`"
                    .to_string(),
                format!("use `{} name = ...` or `map[key] = ...`", syntax::KW_VAR),
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
                Ok(Expr::Ident(syntax::KW_IT.to_string(), span))
            }
            TokKind::Ident(name)
                if name == syntax::LIT_VALUE
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
            TokKind::Ident(name)
                if matches!(name.as_str(), syntax::FOREIGN_THROW | syntax::FOREIGN_RAISE) =>
            {
                let t = self.bump();
                let foreign = name.clone();
                self.diags.push(Diagnostic::error(
                    "E0026",
                    format!("{} doesn't use `{}`", syntax::LANG_NAME, foreign),
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
                    syntax::FOREIGN_CATCH | syntax::FOREIGN_EXCEPT
                ) =>
            {
                let t = self.bump();
                let foreign = name.clone();
                self.diags.push(Diagnostic::error(
                    "E0024",
                    format!("{} doesn't use `{}`", syntax::LANG_NAME, foreign),
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
                    syntax::FOREIGN_UNWRAP | syntax::FOREIGN_EXPECT
                ) =>
            {
                let t = self.bump();
                let foreign = name.clone();
                self.diags.push(Diagnostic::error(
                    "E0025",
                    format!("{} doesn't use `{}`", syntax::LANG_NAME, foreign),
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
                Ok(Expr::Int(n, span))
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
                Ok(Expr::Ident(syntax::KW_SELF.to_string(), span))
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
                    format!("{} doesn't use `|` pipes for lambdas", syntax::LANG_NAME),
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
            TokKind::Ident(name) if name == syntax::FOREIGN_LAMBDA => {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0032",
                    format!(
                        "{} doesn't use the `{}` keyword for short functions",
                        syntax::LANG_NAME,
                        syntax::FOREIGN_LAMBDA
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
                    syntax::FOREIGN_VEC | syntax::FOREIGN_HASHMAP | syntax::FOREIGN_DICT
                ) =>
            {
                let t = self.bump();
                let foreign = name.clone();
                let canonical = if foreign == syntax::FOREIGN_VEC {
                    syntax::TYPE_LIST
                } else {
                    syntax::TYPE_MAP
                };
                self.diags.push(Diagnostic::error(
                    "E0028",
                    format!(
                        "{} uses `{}`, not `{}`",
                        syntax::LANG_NAME,
                        canonical,
                        foreign
                    ),
                    format!("`{}` is the built-in collection type", canonical),
                    format!("replace `{}` with `{}`", foreign, canonical),
                    Some(t.span),
                ));
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name) if name == syntax::FOREIGN_AS => {
                let t = self.bump();
                self.diags.push(Diagnostic::error(
                    "E0030",
                    format!(
                        "{} doesn't use `{}` for conversions",
                        syntax::LANG_NAME,
                        syntax::FOREIGN_AS
                    ),
                    "convert with methods like `.to_float()` or `.to_string()`".to_string(),
                    "e.g. `x.to_float()` instead of `x as Float`".to_string(),
                    Some(t.span),
                ));
                return self.expr_primary(allow_struct_lit);
            }
            TokKind::Ident(name) if name == syntax::FOREIGN_APPEND => {
                let span = self.bump().span;
                self.diags.push(Diagnostic::error(
                    "E0027",
                    format!("lists use `{}`, not `{}`", "push", syntax::FOREIGN_APPEND),
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
                    if member == syntax::TYPE_PTR && matches!(self.peek().kind, TokKind::Lt) {
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

    /// U3 (unified-ecosystem §4): `module name { contributions… }`. Many
    /// modules may share a file; a leading-`_` name disables one. The body is a
    /// list of typed namespace contributions (`env.dev: Env { … }`).
    fn module_decl(&mut self) -> Result<ModuleDecl, Diagnostic> {
        let start = self.bump().span; // `module`
        // S84: module names may be kebab-case (a module is the package the
        // payload manifest discovers by name).
        let (name, name_span) = self.expect_dashed_name("for the module name")?;
        let disabled = name.starts_with('_');
        self.expect(TokKind::LBrace, "to open the module body")?;
        let mut sources = Vec::new();
        let mut imports = Vec::new();
        let mut contributions = Vec::new();
        // A module body holds three kinds of entry (U3/U8): `sources:` and
        // `imports:` fields, and typed `namespace.path: Value` contributions.
        // The first two are distinguished by their reserved name followed by
        // `:`; contributions begin with a namespace name followed by `.`.
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            match &self.peek().kind {
                TokKind::Ident(n)
                    if n == syntax::MODULE_FIELD_SOURCES
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    sources.extend(self.module_sources()?);
                }
                TokKind::Ident(n)
                    if n == syntax::MODULE_FIELD_IMPORTS
                        && matches!(self.peek2().kind, TokKind::Colon) =>
                {
                    imports.push(self.module_import()?);
                }
                _ => contributions.push(self.contribution()?),
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close the module body")?;
        Ok(ModuleDecl {
            name,
            name_span,
            disabled,
            sources,
            imports,
            contributions,
            span: Span::new(start.start, end),
        })
    }

    /// U8: a module's `sources: { name: provider@target, … }` block. Each ref is
    /// not a single token (it carries `@`, `/`, `-`, `.`), so we record its
    /// source span and leave validation to modeval (`classify_provider_ref`).
    fn module_sources(&mut self) -> Result<Vec<crate::ast::SourceDecl>, Diagnostic> {
        self.bump(); // `sources`
        self.expect(TokKind::Colon, "after `sources`")?;
        self.expect(TokKind::LBrace, "to open the sources block")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            let (name, name_span) = self.expect_ident("for a source name")?;
            self.expect(TokKind::Colon, "after a source name")?;
            // Consume the `provider@target` ref tokens up to the next `,`/`}`;
            // the recovered span slices back to the exact written text.
            let ref_start = self.peek().span;
            let mut ref_end = ref_start.end;
            if matches!(self.peek().kind, TokKind::Comma | TokKind::RBrace | TokKind::Eof) {
                return Err(Diagnostic::error(
                    "E0003",
                    "a source needs a `provider@target` ref".to_string(),
                    "every named source resolves to an upstream, e.g. `default: github@NixOS/nixpkgs/nixos-24.05`"
                        .to_string(),
                    "write the ref after the `:`".to_string(),
                    Some(ref_start),
                ));
            }
            while !matches!(
                self.peek().kind,
                TokKind::Comma | TokKind::RBrace | TokKind::Eof
            ) {
                ref_end = self.peek().span.end;
                self.bump();
            }
            out.push(crate::ast::SourceDecl {
                name,
                name_span,
                ref_span: Span::new(ref_start.start, ref_end),
                span: Span::new(name_span.start, ref_end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the sources block")?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(out)
    }

    /// U8: a module's `imports: find("./modules")` directive. The value is an
    /// ordinary call expression; the `find` walk itself lands with U4 discovery.
    fn module_import(&mut self) -> Result<Expr, Diagnostic> {
        self.bump(); // `imports`
        self.expect(TokKind::Colon, "after `imports`")?;
        let value = self.expr()?;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(value)
    }

    /// U3 (unified-ecosystem §5): one typed namespace contribution,
    /// `namespace.path: Value`, e.g. `env.dev: Env { … }`. The value reuses the
    /// ordinary expression parser (struct literals, lists, strings).
    fn contribution(&mut self) -> Result<Contribution, Diagnostic> {
        let (ns_name, ns_span) =
            self.expect_ident("for a namespace (`env`, `system`, or `image`)")?;
        let namespace = match ns_name.as_str() {
            syntax::NS_ENV => Namespace::Env,
            syntax::NS_SYSTEM => Namespace::System,
            syntax::NS_IMAGE => Namespace::Image,
            _ => {
                return Err(Diagnostic::error(
                    "E0960",
                    format!("`{}` is not a module namespace", ns_name),
                    format!(
                        "a module contributes to exactly three reserved namespaces: `{}` (a dev environment), `{}` (a whole machine), and `{}` (a disk image)",
                        syntax::NS_ENV, syntax::NS_SYSTEM, syntax::NS_IMAGE
                    ),
                    format!(
                        "begin the contribution with `{}`, `{}`, or `{}`",
                        syntax::NS_ENV, syntax::NS_SYSTEM, syntax::NS_IMAGE
                    ),
                    Some(ns_span),
                ));
            }
        };
        self.expect(TokKind::Dot, "after the namespace name")?;
        // S84: contribution names (`system.<name>`, `image.<name>`, `env.<name>`)
        // may be kebab-case, e.g. `image.halcyon-iso`.
        let (path, path_span) = self.expect_dashed_name("for the contribution name")?;
        self.expect(TokKind::Colon, "after the contribution name")?;
        // U11/U14/U18: `system.<name>:` and `image.<name>:` parse into dedicated
        // typed literals (the U13 `options` list, the typed `target` value, the
        // U12 `Service` map, and U18 bare `{ … }` don't fit the ordinary
        // expression grammar). `env.<name>:` keeps the ordinary expression parser.
        let value = match namespace {
            Namespace::Env => crate::ast::ContribValue::Expr(self.expr()?),
            Namespace::System => crate::ast::ContribValue::System(self.system_lit()?),
            Namespace::Image => crate::ast::ContribValue::Image(self.image_lit()?),
        };
        let end = value.span().end;
        if matches!(self.peek().kind, TokKind::Comma) {
            self.bump();
        }
        Ok(Contribution {
            namespace,
            path,
            path_span,
            value,
            span: Span::new(ns_span.start, end),
        })
    }

    /// U11/U18: parse a `System { … }` or bare `{ … }` record. The type name
    /// `System` is optional (U18 inferred constructor); when present it is
    /// recorded so modeval can keep allowing the explicit form (S29).
    fn system_lit(&mut self) -> Result<crate::ast::SystemLit, Diagnostic> {
        let start = self.peek().span.start;
        let explicit_type = self.opt_record_type(syntax::TYPE_SYSTEM)?;
        self.expect(TokKind::LBrace, "to open a `System` record")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            fields.push(self.system_field()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close a `System` record")?;
        Ok(crate::ast::SystemLit {
            explicit_type,
            fields,
            span: Span::new(start, end),
        })
    }

    /// U18: an optional record type name before `{`. Returns its span when the
    /// author wrote it (`System { … }` / `Image { … }` / `Service { … }`), `None`
    /// for a bare `{ … }`. A type name other than `expected` is left for `{` to
    /// reject, so the message stays about the record shape.
    fn opt_record_type(&mut self, expected: &str) -> Result<Option<Span>, Diagnostic> {
        if self.peek_is_ident(expected) && matches!(self.peek2().kind, TokKind::LBrace) {
            Ok(Some(self.bump().span))
        } else {
            Ok(None)
        }
    }

    /// U11/U12/U13: one field inside a `System { … }` record.
    fn system_field(&mut self) -> Result<crate::ast::SystemField, Diagnostic> {
        let (name, name_span) = self.expect_ident("for a `System` field name")?;
        self.expect(TokKind::Colon, "after a `System` field name")?;
        let value = match name.as_str() {
            syntax::SYSTEM_FIELD_TARGET => {
                let (os, arch, span) = self.platform_value()?;
                crate::ast::SystemFieldValue::Platform { os, arch, span }
            }
            syntax::SYSTEM_FIELD_PACKAGES => {
                crate::ast::SystemFieldValue::Packages(self.expr()?)
            }
            syntax::SYSTEM_FIELD_SERVICES => {
                crate::ast::SystemFieldValue::Services(self.services_map()?)
            }
            syntax::SYSTEM_FIELD_OPTIONS => {
                crate::ast::SystemFieldValue::Options(self.options_list()?)
            }
            _ => crate::ast::SystemFieldValue::Other(self.expr()?),
        };
        let end = value_end_system(&value);
        Ok(crate::ast::SystemField {
            name,
            name_span,
            value,
            span: Span::new(name_span.start, end),
        })
    }

    /// U13: a dotted typed platform value — `linux.x64`. Two name segments joined
    /// by `.`; modeval checks they name a known platform.
    fn platform_value(&mut self) -> Result<(String, String, Span), Diagnostic> {
        let (os, os_span) = self.expect_ident("for a platform, e.g. `linux`")?;
        self.expect(TokKind::Dot, "between the platform and its architecture")?;
        let (arch, arch_span) = self.expect_ident("for an architecture, e.g. `x64`")?;
        Ok((os, arch, Span::new(os_span.start, arch_span.end)))
    }

    /// U12/U18: a `services: { name: { … }, … }` map — each entry is a service
    /// name and an inferred (or explicit) `Service` record.
    fn services_map(&mut self) -> Result<Vec<crate::ast::ServiceEntry>, Diagnostic> {
        self.expect(TokKind::LBrace, "to open the `services` map")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            let (name, name_span) = self.expect_ident("for a service name")?;
            self.expect(TokKind::Colon, "after a service name")?;
            let explicit_type = self.opt_record_type(syntax::TYPE_SERVICE)?;
            self.expect(TokKind::LBrace, "to open a `Service` record")?;
            let mut fields = Vec::new();
            while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
                let (field, field_span) = self.expect_ident("for a `Service` field name")?;
                self.expect(TokKind::Colon, "after a `Service` field name")?;
                let value = self.expr()?;
                fields.push((field, field_span, value));
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                }
            }
            let rec_end = self.peek().span.end;
            self.expect(TokKind::RBrace, "to close a `Service` record")?;
            out.push(crate::ast::ServiceEntry {
                name,
                name_span,
                explicit_type,
                fields,
                span: Span::new(name_span.start, rec_end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBrace, "to close the `services` map")?;
        Ok(out)
    }

    /// U13: an `options: [ dotted.key: value, … ]` ordered list. Each entry is a
    /// dotted key path and a value expression (bare identifier, dotted typed
    /// value, list, or quoted free-form string).
    fn options_list(&mut self) -> Result<Vec<crate::ast::OptionEntry>, Diagnostic> {
        self.expect(TokKind::LBracket, "to open the `options` list")?;
        let mut out = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBracket | TokKind::Eof) {
            let (mut key, key_start) = self.expect_ident("for an option key, e.g. `net.hostName`")?;
            let mut key_end = key_start.end;
            while matches!(self.peek().kind, TokKind::Dot) {
                self.bump();
                let (seg, seg_span) = self.expect_ident("for the next part of the option key")?;
                key.push('.');
                key.push_str(&seg);
                key_end = seg_span.end;
            }
            let key_span = Span::new(key_start.start, key_end);
            self.expect(TokKind::Colon, "after an option key")?;
            let value_start = self.peek().span.start;
            let value = self.expr()?;
            // Record the value's full written span from the first token of the
            // value to the last token consumed (the token before the cursor) —
            // robust for dotted typed values like `default.fish` whose `Expr`
            // span covers only the final member.
            let value_end = self.prev_end();
            out.push(crate::ast::OptionEntry {
                key,
                key_span,
                value,
                value_span: Span::new(value_start, value_end),
                span: Span::new(key_start.start, value_end),
            });
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        self.expect(TokKind::RBracket, "to close the `options` list")?;
        Ok(out)
    }

    /// U14/U18: parse an `Image { … }` or bare `{ … }` record.
    fn image_lit(&mut self) -> Result<crate::ast::ImageLit, Diagnostic> {
        let start = self.peek().span.start;
        let explicit_type = self.opt_record_type(syntax::TYPE_IMAGE)?;
        self.expect(TokKind::LBrace, "to open an `Image` record")?;
        let mut fields = Vec::new();
        while !matches!(self.peek().kind, TokKind::RBrace | TokKind::Eof) {
            fields.push(self.image_field()?);
            if matches!(self.peek().kind, TokKind::Comma) {
                self.bump();
            }
        }
        let end = self.peek().span.end;
        self.expect(TokKind::RBrace, "to close an `Image` record")?;
        Ok(crate::ast::ImageLit {
            explicit_type,
            fields,
            span: Span::new(start, end),
        })
    }

    /// U14: one field inside an `Image { … }` record.
    fn image_field(&mut self) -> Result<crate::ast::ImageField, Diagnostic> {
        let (name, name_span) = self.expect_ident("for an `Image` field name")?;
        self.expect(TokKind::Colon, "after an `Image` field name")?;
        let value = match name.as_str() {
            syntax::IMAGE_FIELD_FROM => {
                // `from: system.<name>` — the `system` keyword then the name.
                let (kw, kw_span) = self.expect_ident("for `system`, e.g. `system.halcyon`")?;
                if kw != syntax::NS_SYSTEM {
                    return Err(image_from_not_system(kw_span));
                }
                self.expect(TokKind::Dot, "after `system`")?;
                // S84: `from: system.<name>` may reference a kebab-case System
                // name; must read the same way the definition does so the E0978
                // cross-check still string-matches.
                let (sys, sys_span) = self.expect_dashed_name("for the system name")?;
                crate::ast::ImageFieldValue::From {
                    system: sys,
                    span: Span::new(kw_span.start, sys_span.end),
                }
            }
            syntax::IMAGE_FIELD_FORMAT => {
                let (word, span) = self.expect_ident("for a format, e.g. `iso`")?;
                crate::ast::ImageFieldValue::Format { word, span }
            }
            syntax::SYSTEM_FIELD_TARGET => {
                let (os, arch, span) = self.platform_value()?;
                crate::ast::ImageFieldValue::Platform { os, arch, span }
            }
            _ => crate::ast::ImageFieldValue::Other(self.expr()?),
        };
        let end = value_end_image(&value);
        Ok(crate::ast::ImageField {
            name,
            name_span,
            value,
            span: Span::new(name_span.start, end),
        })
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
            type_args: Vec::new(),
            import_ns: Some(alias),
            as_trait: None,
            fields,
            span: Span::new(start, end),
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
            TokKind::Ident(name) if name == syntax::LIT_VALUE => {
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
                        Stmt::Break(s) | Stmt::Continue(s) => s.end,
                        Stmt::If(i) => i.span.end,
                        Stmt::While { span, .. }
                        | Stmt::For { span, .. }
                        | Stmt::Switch { span, .. } => span.end,
                        Stmt::Val(b) => b.init.span().end,
                        Stmt::Assign { value, .. } => value.span().end,
                        Stmt::Loop(_, s) => s.end,
                        Stmt::Unsafe { span, .. } => span.end,
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
                        format!("remove `{}` and write `name: Type`", syntax::FOREIGN_READ),
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
                // `take(names) () =>` is a lambda take-prefix, not an arg convention.
                // Only consume `take` as an arg convention when NOT followed by `(`.
                let is_lambda_take = matches!(
                    self.toks.get(self.pos + 1).map(|t| &t.kind),
                    Some(TokKind::LParen)
                );
                if is_lambda_take {
                    AccessConvention::Read
                } else {
                    self.bump();
                    AccessConvention::Move
                }
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
                syntax::LANG_NAME,
                foreign,
                canonical
            ),
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

    /// S33: open `Type<…>` — teach square brackets used for value lists.
    fn expect_type_args_open(&mut self, type_name: &str) -> Result<(), Diagnostic> {
        match &self.peek().kind {
            TokKind::Lt => {
                self.bump();
                Ok(())
            }
            TokKind::LBracket => Err(Diagnostic::error(
                "E0034",
                format!("`{type_name}[...]` isn't how Jet writes generic types"),
                "square brackets start collection types like `[Int]` or `[String, Int]`, and collection values like `[1, 2]`"
                    .to_string(),
                format!("write `{type_name}<...>`, or use `[Int]` for a list type"),
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
    fn maybe_close_type_args(&mut self, context: &str) -> Result<(), Diagnostic> {
        if self.type_generic_truncated {
            Ok(())
        } else {
            self.expect_type_args_close(context)
        }
    }

    fn expect_type_args_close(&mut self, context: &str) -> Result<(), Diagnostic> {
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

    fn type_(&mut self) -> Result<(Type, Span), Diagnostic> {
        let span = self.peek().span;
        self.with_nesting(span, |p| p.type_inner())
    }

    fn type_inner(&mut self) -> Result<(Type, Span), Diagnostic> {
        let start = self.peek().span;
        let base = match self.peek().kind.clone() {
            TokKind::KwFn => {
                self.bump();
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
                Type::Fn { params, ret }
            }
            TokKind::LBracket => {
                self.bump();
                let first = self.type_generic_arg("list/map type")?;
                if matches!(self.peek().kind, TokKind::Comma) {
                    self.bump();
                    let value = self.type_generic_arg("map value")?;
                    self.expect(TokKind::RBracket, "after the value type in `[K, V]`")?;
                    Type::Map {
                        key: Box::new(first),
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
                    Type::FixedList { elem: Box::new(first), len }
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
                        self.expect_type_args_open("List")?;
                        let inner = self.type_generic_arg("List")?;
                        if !self.type_generic_truncated {
                            self.maybe_close_type_args("after a list element type")?;
                        }
                        Type::List(Box::new(inner))
                    }
                    syntax::TYPE_MAP => {
                        self.expect_type_args_open("Map")?;
                        let key = self.type_generic_arg("Map key")?;
                        self.expect(TokKind::Comma, "between the two types in `Map<K, V>`")?;
                        let value = self.type_generic_arg("Map value")?;
                        self.maybe_close_type_args("after the value type in `Map<K, V>`")?;
                        Type::Map {
                            key: Box::new(key),
                            value: Box::new(value),
                        }
                    }
                    syntax::TYPE_CHAR => Type::Char,
                    syntax::FOREIGN_DYN => {
                        self.diags.push(generics::e0036(syntax::FOREIGN_DYN, start));
                        let (trait_name, _) = self.expect_ident("after `dyn`")?;
                        Type::TraitObject(trait_name)
                    }
                    syntax::FOREIGN_BOX => {
                        self.diags.push(generics::e0036(syntax::FOREIGN_BOX, start));
                        if matches!(self.peek().kind, TokKind::Lt) {
                            self.expect_type_args_open("Box")?;
                            if matches!(self.peek().kind, TokKind::Ident(ref n) if n == syntax::FOREIGN_DYN)
                            {
                                self.bump();
                                let (trait_name, _) =
                                    self.expect_ident("after `dyn` in `Box<dyn …>`")?;
                                self.maybe_close_type_args("after `Box<dyn …>`")?;
                                Type::TraitObject(trait_name)
                            } else {
                                let (inner, _) = self.type_()?;
                                self.maybe_close_type_args("after `Box<…>`")?;
                                inner
                            }
                        } else {
                            Type::Named("Box".to_string())
                        }
                    }
                    syntax::FOREIGN_VEC => {
                        self.diags.push(Diagnostic::error(
                            "E0028",
                            format!(
                                "{} uses `{}`, not `{}`",
                                syntax::LANG_NAME,
                                syntax::TYPE_LIST,
                                syntax::FOREIGN_VEC
                            ),
                            format!("`{}` is the list type", syntax::TYPE_LIST),
                            format!(
                                "replace `{}` with `{}<...>`",
                                syntax::FOREIGN_VEC,
                                syntax::TYPE_LIST
                            ),
                            Some(start),
                        ));
                        self.expect_type_args_open("List")?;
                        let inner = self.type_generic_arg("List")?;
                        self.maybe_close_type_args("after a list element type")?;
                        Type::List(Box::new(inner))
                    }
                    syntax::FOREIGN_HASHMAP | syntax::FOREIGN_DICT => {
                        let foreign = name.clone();
                        self.diags.push(Diagnostic::error(
                            "E0028",
                            format!(
                                "{} uses `{}`, not `{}`",
                                syntax::LANG_NAME,
                                syntax::TYPE_MAP,
                                foreign
                            ),
                            format!("`{}` is the map type", syntax::TYPE_MAP),
                            format!("replace `{}` with `{}<K, V>`", foreign, syntax::TYPE_MAP),
                            Some(start),
                        ));
                        self.expect_type_args_open("Map")?;
                        let key = self.type_generic_arg("Map key")?;
                        self.expect(TokKind::Comma, "between the two types in `Map<K, V>`")?;
                        let value = self.type_generic_arg("Map value")?;
                        self.maybe_close_type_args("after the value type in `Map<K, V>`")?;
                        Type::Map {
                            key: Box::new(key),
                            value: Box::new(value),
                        }
                    }
                    syntax::TYPE_SHARED => {
                        self.expect_type_args_open("Shared")?;
                        let inner = self.type_generic_arg("Shared")?;
                        self.maybe_close_type_args("after a shared element type")?;
                        Type::Shared(Box::new(inner))
                    }
                    syntax::TYPE_RESULT => {
                        self.diags.push(Diagnostic::error(
                            "E0406",
                            "`Result<T, E>` is old Jet error syntax".to_string(),
                            "fallible Jet types are written as `T ? E`".to_string(),
                            "write the return type as `T ? E`, or `T ?` for the default Error type"
                                .to_string(),
                            Some(start),
                        ));
                        self.expect_type_args_open("Result")?;
                        let ok_ty = self.type_generic_arg("Result ok")?;
                        self.expect(
                            TokKind::Comma,
                            "between the two types in old `Result<T, E>` syntax",
                        )?;
                        let err_ty = self.type_generic_arg("Result err")?;
                        self.maybe_close_type_args(
                            "after the error type in old `Result<T, E>` syntax",
                        )?;
                        Type::Result {
                            ok: Box::new(ok_ty),
                            err: Box::new(err_ty),
                        }
                    }
                    other => {
                        let name = other.to_string();
                        if matches!(self.peek().kind, TokKind::Lt) {
                            self.expect_type_args_open(&name)?;
                            let mut args = Vec::new();
                            loop {
                                args.push(self.type_generic_arg(&name)?);
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
                "an optional value is written `T?` once — there's no optional optional"
                    .to_string(),
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

    /// S84 (ratified 2026-06-16): a *dashed name* — `ident (-ident)*` — for the
    /// kebab-case naming positions (package / module / system / image / env
    /// names), matching nixpkgs/npm package-name convention. A `-` only joins
    /// when it is **span-adjacent** to both neighbours (`prev.end == minus.start`
    /// and `minus.end == next.start`); a spaced `a - b` therefore stays
    /// subtraction (no lexer change, no expression-grammar change). No leading,
    /// trailing, or doubled hyphen — `image.-iso` / `image.a--b` fall through to
    /// the ordinary teaching diagnostic via `expect_ident`.
    fn expect_dashed_name(&mut self, where_: &str) -> Result<(String, Span), Diagnostic> {
        let (mut name, first_span) = self.expect_ident(where_)?;
        let start = first_span.start;
        let mut end = first_span.end;
        // Join `-<segment>` while the hyphen and the following ident are both
        // glued (no intervening whitespace) to the preceding segment.
        while matches!(self.peek().kind, TokKind::Minus)
            && self.peek().span.start == end
            && matches!(self.peek2().kind, TokKind::Ident(_))
            && self.peek2().span.start == self.peek().span.end
        {
            self.bump(); // `-`
            let seg = self.bump(); // the adjacent ident
            if let TokKind::Ident(s) = seg.kind {
                name.push_str(syntax::NAME_SEGMENT_SEP);
                name.push_str(&s);
                end = seg.span.end;
            }
        }
        Ok((name, Span::new(start, end)))
    }
}

/// End byte of a parsed `System` field value, for the field's span.
fn value_end_system(v: &crate::ast::SystemFieldValue) -> usize {
    use crate::ast::SystemFieldValue::*;
    match v {
        Platform { span, .. } => span.end,
        Packages(e) | Other(e) => e.span().end,
        Services(entries) => entries.last().map(|s| s.span.end).unwrap_or(0),
        Options(entries) => entries.last().map(|o| o.span.end).unwrap_or(0),
    }
}

/// End byte of a parsed `Image` field value, for the field's span.
fn value_end_image(v: &crate::ast::ImageFieldValue) -> usize {
    use crate::ast::ImageFieldValue::*;
    match v {
        From { span, .. } | Format { span, .. } | Platform { span, .. } => span.end,
        Other(e) => e.span().end,
    }
}

/// U14: `from:` must be written `system.<name>`.
fn image_from_not_system(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0003",
        "an image's `from:` must name a system".to_string(),
        "U14: an `Image` is built from a `System`, written `from: system.<name>`".to_string(),
        "write `from: system.<name>`, e.g. `from: system.halcyon`".to_string(),
        Some(span),
    )
}

fn binding_why() -> String {
    format!(
        "a binding is `{}` if it never changes, or `{}` if it can",
        syntax::KW_VAL,
        syntax::KW_VAR
    )
}

fn pat_span(pat: &Pattern) -> Span {
    pat.span()
}

#[cfg(test)]
mod s61_tests {
    use super::*;
    use crate::ast::{BinOp, Expr, Stmt};
    use crate::lexer::lex;

    fn program(src: &str) -> Program {
        let (toks, errs) = lex(src);
        assert!(errs.is_empty(), "lex errors: {errs:?}");
        parse(&toks).unwrap_or_else(|d| panic!("parse errors: {d:?}"))
    }

    /// S84 (regression): spaced `a - b` is still subtraction. The dashed-name
    /// reader only fires in name positions and only on span-adjacent hyphens, so
    /// the expression grammar is untouched.
    #[test]
    fn spaced_minus_is_subtraction() {
        let p = program("fn main() { val d = 5 - 3; }");
        let func = p.items.iter().find_map(|i| match i {
            crate::ast::Item::Func(f) => Some(f),
            _ => None,
        });
        let func = func.expect("a main function");
        let val = func.body.iter().find_map(|s| match s {
            Stmt::Val(b) => Some(b),
            _ => None,
        });
        let val = val.expect("a val binding");
        assert!(
            matches!(val.init, Expr::Binary(BinOp::Sub, _, _, _)),
            "expected `5 - 3` to parse as subtraction, got {:?}",
            val.init
        );
    }

    /// S84: a kebab-case module name (`my-host`) joins span-adjacent hyphens into
    /// one name.
    #[test]
    fn dashed_module_name_joins() {
        let p = program("module my-host { }");
        let m = p.items.iter().find_map(|i| match i {
            crate::ast::Item::Module(m) => Some(m),
            _ => None,
        });
        assert_eq!(m.expect("a module").name, "my-host");
    }
}
