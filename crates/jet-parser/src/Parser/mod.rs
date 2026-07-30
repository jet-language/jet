//! Parser: tokens -> AST. Hand-written recursive descent with statement-
//! level error recovery (M1): one run reports every parse problem it can.
//!
//! D-S14-PAUSE / D-TEACHING-LAYER1: broad retired-spelling teaching is paused
//! until post-Epoch 6. Retired S14 spellings fall through to ordinary parse
//! errors. Narrow, separately ratified teaching diagnostics still live at their
//! specific parser sites.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics;
use crate::Lexer::{describe, StrTokPart, TokKind, Token};
use crate::Syntax;
use crate::AST::{
    AccessConvention, BinOp, BindName, BindPattern, Binding, Call, CallArg, CodeModule, ConstAttr,
    ConstDef, Contribution, ElseBranch, EnumDef, EnumLitArg, Expr, Field, ForKind, Func,
    GenericModuleDef, GenericModuleParam, IfStmt, ImplDef, Item, LValue, Lambda, LambdaBody,
    LambdaMeta, LambdaParam, Marker, MetaAttr, MetaField, ModuleAliasDef, ModuleArg, ModuleDecl,
    Namespace, OrFallback, Param, Pattern, Program, Stmt, StrMatchPart, StrPart, StructDef,
    SwitchArm, TagDef, TraitDef, TraitImplBlock, TraitMethodSig, TryConvert, Type, TypeParam,
    TypedLitBody, UnOp, Variant, VariantField, VariantPayload,
};

mod Expressions;
mod Items;
mod Modules;
mod Statements;
mod Types;

pub fn parse(toks: &[Token]) -> Result<Program, Vec<Diagnostic>> {
    parse_inner(toks, false)
}

/// Parse for `jet fmt`: succeeds when the AST is recoverable, even if live
/// teaching diagnostics rewrote retired marker or punctuation forms.
pub fn parse_for_fmt(toks: &[Token]) -> Result<Program, Vec<Diagnostic>> {
    parse_inner(toks, true)
}

/// Parse for editor/LSP check: recover through live teaching errors, return
/// them alongside the AST so sema can still run (M6 phase 4).
pub fn parse_for_check(toks: &[Token]) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    let toks = crate::Lexer::without_comments(toks);
    let (toks, fenced_statements) = crate::FencedNames::expand(&toks)?;
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
        pub_file_default: false,
        in_layout_body: 0,
        adjacent_if_body_depth: 0,
        block_depth: 0,
        callable_tail_block_depth: None,
        module_arg_expr_depth: None,
        policy_declarations: Vec::new(),
        applied_rules: Vec::new(),
        rule_facts: Vec::new(),
        block_spans: Vec::new(),
    };
    let mut prog = p.program();
    prog.fenced_statements = fenced_statements;
    if p.diags.is_empty() {
        Ok((prog, Vec::new()))
    } else if p
        .diags
        .iter()
        .all(|d| d.severity == crate::Diagnostics::Severity::Lint || is_teaching_parse_diag(&d.code))
    {
        Ok((prog, p.diags))
    } else {
        Err(p.diags)
    }
}

fn parse_inner(toks: &[Token], for_fmt: bool) -> Result<Program, Vec<Diagnostic>> {
    let toks = crate::Lexer::without_comments(toks);
    let (toks, fenced_statements) = crate::FencedNames::expand(&toks)?;
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
        pub_file_default: false,
        in_layout_body: 0,
        adjacent_if_body_depth: 0,
        block_depth: 0,
        callable_tail_block_depth: None,
        module_arg_expr_depth: None,
        policy_declarations: Vec::new(),
        applied_rules: Vec::new(),
        rule_facts: Vec::new(),
        block_spans: Vec::new(),
    };
    let mut prog = p.program();
    prog.fenced_statements = fenced_statements;
    if p.diags.is_empty() {
        return Ok(prog);
    }
    let mut errors = p
        .diags
        .iter()
        .filter(|d| d.severity == crate::Diagnostics::Severity::Error);
    if errors.clone().next().is_none()
        || (for_fmt
            && errors.all(|d| {
                is_teaching_parse_diag(&d.code)
                    || (d.code == "E0927" && d.what.contains("retired"))
            }))
    {
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

/// Live teaching diagnostics that recover in the AST; fmt may rewrite to canon.
fn is_teaching_parse_diag(code: &str) -> bool {
    matches!(
        code,
        "E0031"
            | "E0034"
            | "E0048"
            | "E0049"
            | "E0055"
            | "E0057"
            | "E0066"
            | "E0070"
            | "E0071"
            | "E0077"
            | "E0146"
            | "E0154"
            | "E0210"
            | "E0986"
            | "E0320"
            | "E0992"
            | "E0994"
            | "E0366"
            | "E0999"
            | "E0412"
            | "E0413"
            | "E0414"
            | "E0415"
            | "E0416"
            | "E0417"
            | "E0418"
            | "E0998"
    )
}

fn retired_s14_teaching_enabled() -> bool {
    false
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
    /// D-VISDEFAULT2=A: when true, top-level items default to public unless `priv`.
    pub_file_default: bool,
    /// D-LAYOUT1 / D-LAYOUT-CTOR1: >0 while parsing a `Layout.{ … }` body. The
    /// general "bare expression statement" rule (E0003) only allows
    /// calls/field reads/assignments as a statement — a plain `>=`/`<=`/`==`
    /// comparison is normally a no-op. Inside a layout body it's a constraint
    /// element with a real side effect (GATE 1 desugars it to a
    /// solver-registering call), so the parser lets `Expr::Binary` through
    /// here; sema (E2932/E2933) enforces that it's actually a valid
    /// constraint, not the parser.
    in_layout_body: usize,
    /// >0 while a one-line effect-only `if` body is being parsed. The body's
    /// statement may end at an adjacent `else` instead of a line terminator.
    adjacent_if_body_depth: usize,
    /// Current statement-block nesting. An explicit callable result contract
    /// may admit one final value expression only at its own body depth.
    block_depth: usize,
    callable_tail_block_depth: Option<usize>,
    /// While parsing a value in `Template<...>`, a top-level `>` closes the
    /// application instead of becoming a comparison. Nested expressions can
    /// still use `>` normally.
    module_arg_expr_depth: Option<usize>,
    policy_declarations: Vec<crate::Policy::PolicyDeclaration>,
    applied_rules: Vec<crate::AST::AppliedRuleApplication>,
    rule_facts: Vec<crate::AST::AppliedRuleApplication>,
    block_spans: Vec<Span>,
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
    fn span_has_authored_line_break(&self, start: usize, end: usize) -> bool {
        self.toks[start..end].iter().any(|token| {
            matches!(token.kind, TokKind::Semi) && token.span.start == token.span.end
        })
    }

    fn prefer_arm_table_lint(&mut self, span: Span) {
        self.diags.push(Diagnostic::lint(
            "L0507",
            "prefer an ordered arm table for this branch".to_string(),
            "one ordered arm table is Jet's normal form for multi-line and chained choices"
                .to_string(),
            "write `if { condition -> body else -> body }`".to_string(),
            Some(span),
        ));
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

    #[allow(dead_code)]
    fn peek4(&self) -> &Token {
        &self.toks[(self.pos + 3).min(self.toks.len() - 1)]
    }

    #[allow(dead_code)]
    fn peek5(&self) -> &Token {
        &self.toks[(self.pos + 4).min(self.toks.len() - 1)]
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
            TokKind::KwElse if self.adjacent_if_body_depth > 0 => Ok(()),
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

    /// D-EFFTREE1: an *effect path* — `ident (.ident)*` — for effect-list
    /// entries (`FS`, `FS.Read`, `Net.HTTP.Get`). The root is validated in
    /// sema against the closed ten-name D-EFF4/5 vocabulary; further segments
    /// are an open, user-chosen leaf path (mirrors D-TAG1's tag-tree dotted
    /// paths) — not validated here, and with no depth limit.
    fn expect_effect_path_name(&mut self, where_: &str) -> Result<(String, Span), Diagnostic> {
        let (mut name, first_span) = self.expect_ident(where_)?;
        let mut end = first_span.end;
        while matches!(self.peek().kind, TokKind::Dot) {
            self.bump(); // `.`
            let (seg, seg_span) = self.expect_ident("for an effect path segment after `.`")?;
            name.push('.');
            name.push_str(&seg);
            end = seg_span.end;
        }
        Ok((name, Span::new(first_span.start, end)))
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

/// U14/D-JPK-IMAGE1: `from:` must be written `system.<name>` (the `.Iso` disk-image
/// tier) or `packages.<name>` (the `.Oci` container tier).
fn image_from_not_system(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0003",
        "an image's `from:` must name a system or a package".to_string(),
        "an `Image` is built either from a `System` (`from: system.<name>`, disk images) or a `Package` (`from: packages.<name>`, OCI containers)".to_string(),
        "write `from: system.<name>` or `from: packages.<name>`".to_string(),
        Some(span),
    )
}

/// U15: a fleet host value must be written `system.<name>[.{ overrides }]`.
fn fleet_host_not_system(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0003",
        "a fleet host must name a system".to_string(),
        "U15: each `hosts:` entry maps a host to a `System`, written `<host>: system.<name>`"
            .to_string(),
        "write `<host>: system.<name>`, e.g. `web1: system.web`".to_string(),
        Some(span),
    )
}

/// U15: an unterminated `.{ … }` override record on a fleet host.
fn fleet_unterminated_override(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0003",
        "unterminated host override record".to_string(),
        "U15: a `.{ … }` copy-with-update override on a host must be closed with `}`".to_string(),
        "close the override record with `}`".to_string(),
        Some(span),
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
        let p = program("fn run() { d :: 5 - 3 }");
        let func = p.items.iter().find_map(|i| match i {
            crate::AST::Item::Func(f) => Some(f),
            _ => None,
        });
        let func = func.expect("a run function");
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

    /// D-TASKSCOPE1=A: `g.task => { … }` parses as a scoped spawn call.
    #[test]
    fn taskgroup_task_callable_body_parses_as_spawn() {
        let p =
            program("fn run() {\n    taskgroup g {\n        h :: g.task => { return 1 }\n    }\n}\n");
        let run = p.items.iter().find_map(|i| match i {
            crate::AST::Item::Func(f) if f.name == "run" => Some(f),
            _ => None,
        });
        let body = &run.expect("run").body;
        let taskgroup = body.iter().find_map(|s| match s {
            Stmt::TaskGroup { body, .. } => Some(body),
            _ => None,
        });
        let bind = taskgroup.expect("taskgroup").iter().find_map(|s| match s {
            Stmt::Val(b) => Some(b),
            _ => None,
        });
        match &bind.expect("binding").init {
            Expr::MethodCall { method, args, .. } => {
                assert_eq!(method, Syntax::TASKGROUP_SPAWN_METHOD);
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0].expr, Expr::Lambda(_)));
            }
            other => panic!("expected g.task => {{ … }} MethodCall, got {other:?}"),
        }
    }

    /// D-FFI-INLINE1=A / D-FFI-RAWBODY1=A (card #501): `#FFI(c) fn` parses into an inline foreign
    /// tier function — the signature is an ordinary Jet signature, the body is
    /// captured as foreign source and the statement body is empty.
    #[test]
    fn ffi_c_inline_tier_parses() {
        let src = "#FFI(c) fn add(a: Int, b: Int) => Int {\n    \"\"\"long add(long a, long b) { return a + b; }\\n\"quoted\"\n\"\"\"\n}\n";
        let p = program(src);
        let func = p
            .items
            .iter()
            .find_map(|i| match i {
                crate::AST::Item::Func(f) if f.name == "add" => Some(f),
                _ => None,
            })
            .expect("add function");
        let inl = func.inline_foreign.as_ref().expect("inline_foreign set");
        assert_eq!(inl.lang, "c");
        assert_eq!(
            inl.source,
            "long add(long a, long b) { return a + b; }\\n\"quoted\"\n"
        );
        assert!(
            func.body.is_empty(),
            "statement body cleared for inline foreign"
        );
        assert_eq!(func.params.len(), 2);
    }

    /// D-FFI-ASM1=A (card #501): the gated `#Unsafe("…") #FFI(asm) fn` form
    /// parses with both the unsafe contract and the inline foreign payload.
    #[test]
    fn ffi_asm_inline_tier_with_unsafe_gate_parses() {
        let src = "use core.mem\n#[Unsafe(\"cycle counter\"), FFI(asm)] fn rdtsc() => U64 {\n    \"\"\"rdtsc\nshl rdx, 32\nor rax, rdx        ; -> return\n; clobbers rdx\"\"\"\n}\n";
        let p = program(src);
        let func = p
            .items
            .iter()
            .find_map(|i| match i {
                crate::AST::Item::Func(f) if f.name == "rdtsc" => Some(f),
                _ => None,
            })
            .expect("rdtsc function");
        assert!(func.is_unsafe, "unsafe gate recorded");
        assert_eq!(func.unsafe_reason.as_deref(), Some("cycle counter"));
        let inl = func.inline_foreign.as_ref().expect("inline_foreign set");
        assert_eq!(inl.lang, "asm");
        assert!(
            inl.source.contains("; -> return"),
            "source: {:?}",
            inl.source
        );
    }

    #[test]
    fn grouped_ffi_keeps_unsafe_gate_and_raw_payload() {
        let src = "use core.mem\n#[Unsafe(\"scalar registers\"), FFI(asm)]\nfn add(a: Int, b: Int) => Int {\n    \"\"\"add {a}, {b} ; -> return\"\"\"\n}\n";
        let p = program(src);
        let func = p.items.iter().find_map(|item| match item {
            crate::AST::Item::Func(func) if func.name == "add" => Some(func),
            _ => None,
        }).expect("add function");
        assert_eq!(func.unsafe_reason.as_deref(), Some("scalar registers"));
        assert_eq!(
            func.inline_foreign.as_ref().map(|inline| inline.source.as_str()),
            Some("add {a}, {b} ; -> return")
        );
    }

    #[test]
    fn grouped_function_rules_keep_meta_and_policy_semantics() {
        let src = "#[Policy(no_alloc), Meta(category: \"api\", maturity: .Tested), Task]\nfn sync() {}\n";
        let p = program(src);
        let func = p.items.iter().find_map(|item| match item {
            crate::AST::Item::Func(func) if func.name == "sync" => Some(func),
            _ => None,
        }).expect("sync function");
        assert!(func.is_task);
        assert!(func.meta.is_some());
        assert_eq!(func.maturity, Some(crate::AST::MaturityTag::Tested));
        assert!(p.policy_declarations.iter().any(|declaration| {
            declaration.key == crate::Policy::PolicyKey::NoAlloc
                && declaration.scope == crate::Policy::PolicyScope::Function
                && declaration.target == Some(func.span)
        }));
    }

    #[test]
    fn named_rule_arguments_bind_by_signature_without_reordering_source() {
        let src = "#[Transition(to: Closed, from: Open), Meta(maturity: .Hardened, tunable: false)]\nfn work() {}\n";
        let parsed = program(src);
        let function = parsed
            .items
            .iter()
            .find_map(|item| match item {
                crate::AST::Item::Func(function) if function.name == "work" => Some(function),
                _ => None,
            })
            .expect("work function");
        let transition = function
            .state_transition
            .as_ref()
            .expect("bound transition");
        assert_eq!(transition.from.as_deref(), Some("Open"));
        assert_eq!(transition.to, "Closed");
        assert_eq!(
            function.maturity,
            Some(crate::AST::MaturityTag::Hardened)
        );
        assert!(
            !function.meta.as_ref().expect("meta").facts().tunable,
            "explicit false keeps the default"
        );
        let source_transition = parsed
            .applied_rules
            .iter()
            .find(|application| application.marker.name == Syntax::KW_TRANSITION)
            .expect("source transition");
        assert_eq!(
            source_transition
                .marker
                .arg_labels
                .iter()
                .map(|label| label.as_ref().map(|(name, _)| name.as_str()))
                .collect::<Vec<_>>(),
            vec![Some("to"), Some("from")]
        );

        let family = program(
            "#UnitFamily(base: meter, family: Length) {\n    meter\n}\n",
        );
        let family = family
            .items
            .iter()
            .find_map(|item| match item {
                crate::AST::Item::UnitFamily(family) => Some(family),
                _ => None,
            })
            .expect("unit family");
        assert_eq!(family.family, "Length");
        assert_eq!(family.base.as_ref().map(|(name, _)| name.as_str()), Some("meter"));

        let formatted = crate::Formatter::format_source(src).expect("format reordered labels");
        assert!(
            formatted.contains(
                "#[Transition(to: Closed, from: Open), Meta(maturity: .Hardened, tunable: false)]"
            ),
            "{formatted}"
        );

        let invalid = "#Transition(to: Closed, Open)\nfn work() {}\n";
        let (tokens, lex_diagnostics) = lex(invalid);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
        let diagnostics = parse(&tokens).expect_err("positional after named is E0930");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0930"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn marker_stack_fix_preserves_reordered_named_arguments() {
        let src = "#Transition(to: Closed, from: Open)\n#Meta(maturity: .Tested, category: \"state\")\nfn work() {}\n";
        let (tokens, lex_diagnostics) = lex(src);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
        let diagnostics = parse(&tokens).expect_err("adjacent markers are E0999");
        let edit = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "E0999")
            .and_then(|diagnostic| diagnostic.edit.as_ref())
            .expect("marker stack edit");
        assert_eq!(
            edit.new_text,
            "#[Transition(to: Closed, from: Open), Meta(maturity: .Tested, category: \"state\")]"
        );
    }

    #[test]
    fn argument_marker_stack_fix_keeps_payloads_and_order() {
        let src = "#Codable\n#RenameAll(camel)\nstruct Particle { x: Float }\n";
        let (tokens, lex_diagnostics) = lex(src);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
        let diagnostics = parse(&tokens).expect_err("bare stack is E0999");
        let edit = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "E0999")
            .and_then(|diagnostic| diagnostic.edit.as_ref())
            .unwrap_or_else(|| panic!("machine-applicable stack edit: {diagnostics:?}"));
        assert_eq!(edit.new_text, "#[Codable, RenameAll(camel)]");
    }

    #[test]
    fn adjacent_marker_stacks_share_one_ordered_e0999_rewrite() {
        for (src, expected) in [
            (
                "#Job\n#Every(1s)\nfn tick() {}\n",
                "#[Task, Every(1s)]",
            ),
            (
                "use core.mem\n#Unsafe(\"register ABI\")\n#FFI(c)\nfn add() { \"\"\"void add(void) {}\"\"\" }\n",
                "#[Unsafe(\"register ABI\"), FFI(c)]",
            ),
            (
                "#Target(Web)\n#HTML(\"index.html\")\nfn main() {}\n",
                "#[Target(Web), HTML(\"index.html\")]",
            ),
            (
                "#PubFile\n#NoPrelude\nfn main() {}\n",
                "#[PubFile, NoPrelude]",
            ),
            (
                "#Codable\n#RenameAll(camel)\nstruct Particle { x: Float }\n",
                "#[Codable, RenameAll(camel)]",
            ),
        ] {
            let (tokens, lex_diagnostics) = lex(src);
            assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
            let diagnostics = parse(&tokens).expect_err("adjacent markers are E0999");
            let edit = diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code == "E0999")
                .and_then(|diagnostic| diagnostic.edit.as_ref())
                .unwrap_or_else(|| panic!("machine-applicable stack edit: {diagnostics:?}"));
            assert_eq!(edit.new_text, expected, "{src}");
        }
    }

    #[test]
    fn ordered_function_and_file_groups_format_idempotently() {
        use crate::Formatter::format_source;
        for (src, expected_group) in [
            (
                "use core.mem\n#[Policy(no_alloc), Meta(category: \"ffi\"), Task, Unsafe(\"register ABI\", obligations: .None), FFI(c)]\nfn add() {\n    \"\"\"void add(void) {}\"\"\"\n}\n",
                "#[Policy(no_alloc), Meta(category: \"ffi\"), Task, Unsafe(\"register ABI\", obligations: .None), FFI(c)]",
            ),
            (
                "#[HTML(\"index.html\"), PubFile, Target(Web), NoPrelude]\nfn main() {}\n",
                "#[HTML(\"index.html\"), PubFile, Target(Web), NoPrelude]",
            ),
        ] {
            let once = format_source(src).expect("format once");
            assert!(once.contains(expected_group), "{once}");
            let twice = format_source(&once).expect("format twice");
            assert_eq!(once, twice, "{once}");
        }
    }

    #[test]
    fn grouped_retired_function_markers_keep_known_teaching() {
        for (src, code) in [
            ("#[Pure, Task]\nfn work() {}\n", "E0066"),
            ("#[InlineAlways, Task]\nfn work() {}\n", "E0927"),
            ("#[Pure(), Task]\nfn work() {}\n", "E0066"),
            ("#[InlineAlways(), Task]\nfn work() {}\n", "E0927"),
        ] {
            let (tokens, lex_diagnostics) = lex(src);
            assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
            let diagnostics = parse(&tokens).expect_err("retired marker is teaching");
            assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == code), "{diagnostics:?}");
            assert!(!diagnostics.iter().any(|diagnostic| diagnostic.code == "E0930" || diagnostic.code == "E0355"), "{diagnostics:?}");
        }
    }

    #[test]
    fn abi_function_markers_route_to_the_c_declaration_diagnostic() {
        for src in [
            "#[ABI(C), MustUse]\nfn work() {}\n",
            "#ABI(C) fn work() {}\n",
        ] {
            let (tokens, lex_diagnostics) = lex(src);
            assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
            let diagnostics = parse(&tokens).expect_err("ordinary functions reject C ABI selection");
            assert!(
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == "E3212"
                        && diagnostic.what.contains("only applies to C declarations")
                }),
                "{diagnostics:?}"
            );
        }
    }

    #[test]
    fn abi_lowers_to_the_c_declaration_field_and_groups_reject_extra_rules() {
        let program = program(
            "#Extern module c.demo {\n    #ABI(sysv64) fn ping(x: I32) => I32 = \"ping\"\n}\n",
        );
        let function = program
            .items
            .iter()
            .find_map(|item| match item {
                crate::AST::Item::CModule(module) => module.functions.first(),
                _ => None,
            })
            .expect("C declaration");
        assert_eq!(
            function.abi.as_ref().map(|(name, _)| name.as_str()),
            Some("sysv64")
        );

        let src =
            "#Extern module c.demo {\n    #[ABI(sysv64), MustUse] fn ping() = \"ping\"\n}\n";
        let (tokens, lex_diagnostics) = lex(src);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
        let diagnostics =
            parse(&tokens).expect_err("C declarations reject non-ABI function markers");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E3212"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn every_active_function_rule_has_an_applicator() {
        for rule in crate::Policy::applied_rule_registry().iter().filter(|rule| {
            matches!(rule.status, crate::Policy::RuleStatus::Active)
                && rule.sites.contains(&crate::Policy::RuleSite::Function)
        }) {
            assert!(
                Parser::function_marker_has_applicator(rule.name),
                "active function rule `{}` has no applicator",
                rule.name
            );
        }
    }

    #[test]
    fn doc_on_a_function_requires_task() {
        let task_src = "#[Doc(\"task text\"), Task] fn work() {}\n";
        let (task_tokens, task_lex_diagnostics) = lex(task_src);
        assert!(
            task_lex_diagnostics.is_empty(),
            "{task_lex_diagnostics:?}"
        );
        parse(&task_tokens).expect("#Doc may attach to a #Job function");

        let src = "#Doc(\"helper text\") fn helper() {}\n";
        let (tokens, lex_diagnostics) = lex(src);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
        let diagnostics = parse(&tokens).expect_err("#Doc alone must not attach to a function");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0355"),
            "{diagnostics:?}"
        );
    }

    /// D-FFI-INLINE1=A (card #501): `jet fmt` round-trips `#FFI` fns idempotently
    /// (formatter-roundtrip-required-for-new-syntax).
    #[test]
    fn ffi_inline_tier_formats_idempotently() {
        use crate::Formatter::format_source;
        for src in [
            "#FFI(c) fn add(a: Int, b: Int) => Int {\n    \"\"\"long add(long a, long b) { return a + b; }\n\"\"\"\n}\n",
            "use core.mem\n#[Unsafe(\"cycle counter\"), FFI(asm)] fn rdtsc() => U64 {\n    \"\"\"rdtsc ; -> return\n\"\"\"\n}\n",
        ] {
            let once = format_source(src).expect("format once");
            assert!(once.contains("FFI("), "formatted output keeps the FFI marker: {once}");
            let twice = format_source(&once).expect("format twice");
            assert_eq!(once, twice, "jet fmt is idempotent for #FFI fns");
        }
    }

    /// D-VISDEFAULT2=A: `#PubFile` flips default top-level visibility; `priv` opts out.
    #[test]
    fn pub_file_marker_sets_default_visibility() {
        let src = r#"#PubFile

fn greet() => String {
    return "hi"
}

priv fn secret() => Int {
    return 0
}

fn run() {
    return
}"#;
        let p = program(src);
        assert!(p.pub_file);
        let mut funcs: Vec<_> = p
            .items
            .iter()
            .filter_map(|i| match i {
                crate::AST::Item::Func(f) => Some(f),
                _ => None,
            })
            .collect();
        funcs.sort_by_key(|f| f.name.as_str());
        let greet = funcs.iter().find(|f| f.name == "greet").expect("greet");
        let secret = funcs.iter().find(|f| f.name == "secret").expect("secret");
        let run = funcs.iter().find(|f| f.name == "run").expect("run");
        assert!(greet.is_pub);
        assert!(!secret.is_pub);
        assert!(run.is_pub);
    }

    #[test]
    fn pub_file_section_label_emits_e0415() {
        let src = "#PubFile\n\npriv:\nfn run() { return }\n";
        let (toks, errs) = lex(src);
        assert!(errs.is_empty(), "lex errors: {errs:?}");
        let toks = crate::Lexer::without_comments(&toks);
        let mut p = Parser {
            toks: &toks,
            pos: 0,
            diags: Vec::new(),
            pending_type_gt: false,
            depth: 0,
            type_generic_depth: 0,
            type_generic_chain: Vec::new(),
            type_generic_truncated: false,
            pub_file_default: false,
            in_layout_body: 0,
            adjacent_if_body_depth: 0,
            block_depth: 0,
            callable_tail_block_depth: None,
            module_arg_expr_depth: None,
            policy_declarations: Vec::new(),
            applied_rules: Vec::new(),
            rule_facts: Vec::new(),
            block_spans: Vec::new(),
        };
        let _prog = p.program();
        assert!(
            p.diags.iter().any(|d| d.code == "E0415"),
            "expected E0415, got {:?}",
            p.diags
        );
    }

    #[test]
    fn arrow_control_law_formats_canonical_forms_idempotently() {
        use crate::Formatter::format_source;

        let src = r#"protocol Exchange {
    client: Hello(id: Int)
    server: Ready()
}

fn classify(score: Int) => Grade = if {
    score >= 90 -> .A
    score >= 80 -> .B
    else -> .C
}

fn notify(ready: Bool) =[Net]=> Void {
    if ready send() else skip()
    loop item; items audit(item)
    outer :: loop {
        next(outer)
    }
    taskgroup group {
        task :: group.task => fetch()
    }
    #Grant(caps: FS, Net) {
        use_caps(caps)
    }
}
"#;

        let once = format_source(src).expect("canonical arrow/control syntax formats");
        assert!(once.contains("fn classify(score: Int) => Grade = if {"), "{once}");
        assert!(once.contains("score >= 90 -> .A"), "{once}");
        assert!(once.contains("fn notify(ready: Bool) =[Net]=> Void"), "{once}");
        assert!(once.contains("if ready send() else skip()"), "{once}");
        assert!(once.contains("loop item; items audit(item)"), "{once}");
        assert!(once.contains("next(outer)"), "{once}");
        assert!(once.contains("task :: group.task => fetch()"), "{once}");
        assert!(once.contains("#Grant(caps: FS, Net)"), "{once}");
        let twice = format_source(&once).expect("canonical arrow/control syntax reformats");
        assert_eq!(once, twice);
    }

    #[test]
    fn named_effect_loop_stops_value_lookahead_at_its_body() {
        use crate::Formatter::format_source;

        let src = r#"fn run() {
    outer :: loop {
        break
    }
    values :: loop item; [1, 2] -> item
    state :: if ready -> 1 else -> 2
}
"#;
        format_source(src).expect("later value arrows must not reclassify the named effect loop");
    }

    #[test]
    fn multiline_callable_tail_preserves_its_source_expression() {
        let p = program(
            "fn double(value: Int) => Int {\n    adjusted :: value + 1\n    adjusted * 2\n}\n",
        );
        let func = p.items.iter().find_map(|item| match item {
            crate::AST::Item::Func(func) if func.name == "double" => Some(func),
            _ => None,
        });
        assert!(
            matches!(
                func.expect("double").body.last(),
                Some(Stmt::Expr(_))
            ),
            "parser must preserve the final expression; sema lowers it to the callable result"
        );
    }

    #[test]
    fn retired_arrow_control_forms_have_specific_teaching_diagnostics() {
        for (src, code) in [
            ("fn old() -> Int { return 1 }\n", "E0070"),
            ("fn old() --[FS]-> Int { return 1 }\n", "E0066"),
            ("fn run() { if ready -> send() }\n", "E0071"),
            ("fn run() { #Grant(FS) { caps -> use_caps(caps) } }\n", "E0077"),
            (
                "protocol Old { client -> server: Hello(id: Int) }\n",
                "E0154",
            ),
            (
                "fn run() { f :: take(value) (x: Int) => x + value }\n",
                "E0057",
            ),
        ] {
            let (tokens, lex_diagnostics) = lex(src);
            assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:?}");
            let diagnostics = parse(&tokens).expect_err("retired syntax must teach");
            assert!(
                diagnostics.iter().any(|diagnostic| diagnostic.code == code),
                "expected {code}, got {diagnostics:?}"
            );
        }
    }

    #[test]
    fn retired_s14_teaching_is_paused() {
        const RETIRED_CODES: &[&str] = &[
            "E0008", "E0012", "E0013", "E0014", "E0015", "E0016", "E0017", "E0018",
            "E0021", "E0022", "E0023", "E0024", "E0025", "E0026", "E0027", "E0028", "E0030",
            "E0032", "E0033", "E0036", "E0044", "E0045", "E0050", "E0051", "E0052", "E0053",
            "E0054", "E0056", "E0057", "E0984",
        ];
        let cases = [
            "def run() { return }",
            "func run() { return }",
            "import core.files",
            "fn run() { while true { return } }",
            "fn run() { for x in xs { return } }",
            "fn run() { match x { 1 -> return } }",
            "fn run() { switch x { 1 -> return } }",
            "fn run() { x :: a or b }",
            "fn run() { x :: a and b }",
            "fn run() { x :: not ok }",
            "fn run(x: Text) { return }",
            "fn run() { todo }",
            "fn run() { y :: Some(1) }",
            "fn run() { y :: lambda x { x } }",
            "fn run() { y :: append(xs, 1) }",
            "class Point { x: Int }",
            "interface Shape { fn draw(self) }",
            "fn use_item(mut item: Item) { return }",
        ];

        for src in cases {
            let (toks, lex_errs) = lex(src);
            assert!(lex_errs.is_empty(), "lex errors for {src:?}: {lex_errs:?}");
            let diags = match parse_for_check(&toks) {
                Ok((_program, recovered)) => recovered,
                Err(diags) => diags,
            };
            let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
            assert!(
                codes.iter().all(|code| !RETIRED_CODES.contains(code)),
                "retired teaching code leaked for {src:?}: {codes:?}"
            );
        }

        for src in [
            "fn run() { y :: |x| x + 1 }",
            "fn run() { y :: 1 | 2 }",
            "fn run() { y :: 1 |> print }",
        ] {
            let (toks, lex_errs) = lex(src);
            assert!(lex_errs.is_empty(), "lex errors for {src:?}: {lex_errs:?}");
            let diags = match parse_for_check(&toks) {
                Ok((_program, recovered)) => recovered,
                Err(diags) => diags,
            };
            let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
            assert!(codes.contains(&"E0003"), "bar shape must use ordinary E0003: {src:?}: {codes:?}");
            assert!(!codes.contains(&"E0033"), "reserved E0033 leaked for {src:?}: {codes:?}");
        }

        for word in [Syntax::FOREIGN_WHILE, Syntax::FOREIGN_FOR] {
            let (toks, errs) = lex(word);
            assert!(errs.is_empty(), "lex errors for {word}: {errs:?}");
            assert!(
                matches!(&toks[0].kind, TokKind::Ident(n) if n == word),
                "`{word}` should lex as an ordinary name while D-S14-PAUSE is active, got {:?}",
                toks[0].kind
            );
        }
    }

    /// D-RESULT-OPTION-CANON1: tight `T?` is Optional; spaced `T ?` is fallible.
    #[test]
    fn return_type_question_spacing_disambiguates_option_vs_result() {
        let opt = program("fn a() => Int? { return None }\nfn run() {}\n");
        let a = opt.items.iter().find_map(|i| match i {
            crate::AST::Item::Func(f) if f.name == "a" => Some(f),
            _ => None,
        });
        assert!(
            matches!(a.expect("a").return_type, Some(crate::AST::Type::Option(_))),
            "tight `Int?` must be Optional"
        );

        let res = program("fn b() => Int ? { return Ok(1) }\nfn run() {}\n");
        let b = res.items.iter().find_map(|i| match i {
            crate::AST::Item::Func(f) if f.name == "b" => Some(f),
            _ => None,
        });
        assert!(
            matches!(
                b.expect("b").return_type,
                Some(crate::AST::Type::Result { .. })
            ),
            "spaced `Int ?` must be Result"
        );

        let paren = program("fn c() => (Int?) { return None }\nfn run() {}\n");
        let c = paren.items.iter().find_map(|i| match i {
            crate::AST::Item::Func(f) if f.name == "c" => Some(f),
            _ => None,
        });
        assert!(
            matches!(c.expect("c").return_type, Some(crate::AST::Type::Option(_))),
            "parenthesized `(Int?)` stays Optional"
        );
    }
}
