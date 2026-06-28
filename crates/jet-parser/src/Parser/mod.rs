//! Parser: tokens -> AST. Hand-written recursive descent with statement-
//! level error recovery (M1): one run reports every parse problem it can.
//!
//! Teaching errors (S14): familiar foreign spellings (`def`, `let`, `set`,
//! `and`, `try`, `match`, …) are recognized here only to emit an error
//! naming the canonical Jet form — then parsing continues as if the
//! canonical form had been written, so one foreign word doesn't hide the
//! rest of the file's problems.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics;
use crate::Lexer::{describe, StrTokPart, TokKind, Token};
use crate::Syntax;
use crate::AST::{
    AccessConvention, BinOp, BindName, BindPattern, Binding, Call, CallArg, CodeModule, ConstAttr,
    ConstDef, Contribution, ElseBranch, EnumDef, Expr, Field, ForKind, Func, IfStmt, ImplDef, Item,
    LValue, Lambda, LambdaBody, LambdaMeta, LambdaParam, Marker, ModuleDecl, Namespace, OrFallback,
    Param, Pattern, Program, Stmt, StrPart, StructDef, SwitchArm, TagDef, TraitDef, TraitImplBlock,
    TraitMethodSig, TryConvert, Type, TypeParam, UnOp, Variant, VariantField, VariantPayload,
};

mod Expressions;
mod Items;
mod Modules;
mod Statements;
mod Types;

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
    let toks = crate::Lexer::without_comments(toks);
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
    let toks = crate::Lexer::without_comments(toks);
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
            format!("write: {} \"path/to/file\";", Syntax::KW_USE),
            None,
        ));
    }
    match &parts[0] {
        StrTokPart::Lit(s) => Ok(s.clone()),
        StrTokPart::Interp(_) => Err(Diagnostic::error(
            "E0003",
            "an import path can't contain `{ }` interpolation".to_string(),
            "file paths are fixed strings".to_string(),
            format!("write: {} \"path/to/file\";", Syntax::KW_USE),
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
            | "E0055"
            | "E0056"
            | "E0057"
            | "E0058"
            | "E0210"
            | "E0984"
            | "E0985"
            | "E0986"
            | "E0320"
            | "E0992"
            | "E0993"
            | "E0994"
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
        "split the nested code into smaller steps with `val` bindings".to_string(),
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
    fn peek(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn peek2(&self) -> &Token {
        &self.toks[(self.pos + 1).min(self.toks.len() - 1)]
    }

    fn peek3(&self) -> &Token {
        &self.toks[(self.pos + 2).min(self.toks.len() - 1)]
    }

    #[allow(dead_code)] // lookahead helpers kept for symmetry with peek6/peek7
    fn peek4(&self) -> &Token {
        &self.toks[(self.pos + 3).min(self.toks.len() - 1)]
    }

    #[allow(dead_code)] // lookahead helpers kept for symmetry with peek6/peek7
    fn peek5(&self) -> &Token {
        &self.toks[(self.pos + 4).min(self.toks.len() - 1)]
    }

    fn peek6(&self) -> &Token {
        &self.toks[(self.pos + 5).min(self.toks.len() - 1)]
    }

    fn peek7(&self) -> &Token {
        &self.toks[(self.pos + 6).min(self.toks.len() - 1)]
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

    /// S6-R (ratified 2026-06-18): statements are terminated by a synthetic
    /// terminator the lexer inserts at line ends — users never type `;`. This
    /// consumes that terminator; an error here means two statements share a line.
    fn finish_stmt(&mut self) -> Result<(), Diagnostic> {
        match &self.peek().kind {
            TokKind::Semi => {
                self.bump();
                Ok(())
            }
            // S6-R (Go's rule part 2): a terminator may be omitted before a
            // closing `}` or EOF — a single-line block `{ stmt }` needs no
            // synthetic terminator. Don't consume the `}`; the block loop closes
            // it. A struct/map literal's `}` is consumed by the expression
            // parser, so it never reaches here — no risk to literals.
            TokKind::RBrace | TokKind::Eof => Ok(()),
            other => Err(Diagnostic::error(
                "E0003",
                format!(
                    "something unexpected appeared after this line, found {}",
                    describe(other)
                ),
                "each line ends where the line break is (no `;` needed)".to_string(),
                "put the next line of code on a new line".to_string(),
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
                name.push_str(Syntax::NAME_SEGMENT_SEP);
                name.push_str(&s);
                end = seg.span.end;
            }
        }
        Ok((name, Span::new(start, end)))
    }
}

/// End byte of a parsed `System` field value, for the field's span.
fn value_end_system(v: &crate::AST::SystemFieldValue) -> usize {
    use crate::AST::SystemFieldValue::*;
    match v {
        Platform { span, .. } => span.end,
        Packages(e) | Other(e) => e.span().end,
        Services(entries) => entries.last().map(|s| s.span.end).unwrap_or(0),
        Options(entries) => entries.last().map(|o| o.span.end).unwrap_or(0),
    }
}

/// End byte of a parsed `Image` field value, for the field's span.
fn value_end_image(v: &crate::AST::ImageFieldValue) -> usize {
    use crate::AST::ImageFieldValue::*;
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
        "a binding is `name {} value` if it never changes, or `name {} value` if it can",
        Syntax::SIGIL_BIND_IMMUT,
        Syntax::SIGIL_BIND_MUT
    )
}

fn pat_span(pat: &Pattern) -> Span {
    pat.span()
}

#[cfg(test)]
mod s61_tests {
    use super::*;
    use crate::Lexer::lex;
    use crate::AST::{BinOp, Expr, Stmt};

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
        // Also a single-line-block regression guard (S6-R Go-rule part 2: a
        // terminator may be omitted before the closing `}`).
        let p = program("fn main() { d @= 5 - 3 }");
        let func = p.items.iter().find_map(|i| match i {
            crate::AST::Item::Func(f) => Some(f),
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
            crate::AST::Item::Module(m) => Some(m),
            _ => None,
        });
        assert_eq!(m.expect("a module").name, "my-host");
    }
}
