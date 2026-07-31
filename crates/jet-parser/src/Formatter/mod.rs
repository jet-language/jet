//! Pretty-printer for Jet source (M6 phase 1, S44).
//!
//! One true style: 4-space indent, same-line `{`, spaces around binary
//! operators, semicolons on statements. Line width is not enforced in v1
//! (S44 width-100 may land with optional org config later). Comments are
//! preserved from the original source and re-attached by span.

mod Expressions;
mod Items;
mod Statements;

use crate::Diagnostics::Span;
use crate::Lexer::{TokKind, Token};
use crate::Syntax;
use crate::AST::{BinOp, Func, Item, LValue, Program, Stmt};

const INDENT: usize = 4;

/// D-FMT1: the width floor (gate d) for keeping a body on one line. A rendered
/// inline body whose final column exceeds this expands instead.
const MAX_WIDTH: usize = 100;

/// D-FMT1: may this statement render inline inside a one-line brace body? Only
/// non-block statements qualify; every block-bearing variant (`if`, loops,
/// `switch`, `#unsafe`, etc.) must expand so fmt never nests a block inline.
fn is_simple_stmt(stmt: &Stmt) -> bool {
    match stmt {
        // Bare `return` must stay multiline: `{ return }` is not a recoverable parse.
        Stmt::Return(None, _) => false,
        Stmt::Expr(_)
        | Stmt::Val(_)
        | Stmt::Assign { .. }
        | Stmt::Return(Some(_), _)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..) => true,
        _ => false,
    }
}

/// Format a parsed program back to canonical Jet source.
pub fn format_program(prog: &Program, src: &str, comment_toks: &[Token]) -> String {
    let (source_toks, _) = crate::Lexer::lex(src);
    format_program_with_tokens(prog, src, comment_toks, &source_toks)
}

fn format_program_with_tokens(
    prog: &Program,
    src: &str,
    comment_toks: &[Token],
    source_toks: &[Token],
) -> String {
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
        source_toks,
        comments,
        comment_i: 0,
        out: String::new(),
        col: 0,
        at_line_start: true,
        indent: 0,
        pending_blank: false,
        trailing_comment_limit: usize::MAX,
        pub_file: prog.pub_file,
        items: &prog.items,
        policy_declarations: &prog.policy_declarations,
        applied_rules: &prog.applied_rules,
        fenced_statements: &prog.fenced_statements,
    };
    let mut first = true;
    let ordered_file_rules = prog
        .applied_rules
        .iter()
        .filter(|application| application.target.is_none())
        .map(|application| application.marker.clone())
        .collect::<Vec<_>>();
    let has_ordered_file_rules = !ordered_file_rules.is_empty();
    let ordered_file_rules_precede_imports = ordered_file_rules.iter().any(|marker| {
        marker.name == Syntax::MARKER_PUB_FILE || marker.name == Syntax::MARKER_NO_PRELUDE
    });
    let file_rule_count = usize::from(prog.pub_file)
        + usize::from(prog.no_prelude)
        + usize::from(prog.default_target.as_deref() == Some(crate::Syntax::BUILD_TARGET_WEB))
        + usize::from(prog.web_target_ceiling.is_some())
        + usize::from(prog.html_path.is_some());
    let grouped_file_rules = file_rule_count > 1;
    if has_ordered_file_rules && ordered_file_rules_precede_imports {
        let rules = ordered_file_rules.iter().collect::<Vec<_>>();
        f.fmt_marker_group(&rules, Syntax::RULE_PREFIX, true);
        first = false;
    } else if !has_ordered_file_rules && grouped_file_rules {
        let mut rule_index = 0usize;
        f.write("#[");
        let mut write_separator = |f: &mut Fmt<'_>| {
            if rule_index > 0 {
                f.write(", ");
            }
            rule_index += 1;
        };
        if prog.pub_file {
            write_separator(&mut f);
            f.write(Syntax::MARKER_PUB_FILE);
        }
        if prog.no_prelude {
            write_separator(&mut f);
            f.write(Syntax::MARKER_NO_PRELUDE);
        }
        if prog.default_target.as_deref() == Some(crate::Syntax::BUILD_TARGET_WEB) {
            write_separator(&mut f);
            f.write(&format!(
                "{}({})",
                Syntax::ATTR_TARGET,
                Syntax::WEB_TARGET_DEFAULT_WEB
            ));
        }
        if let Some(bucket) = prog.web_target_ceiling {
            write_separator(&mut f);
            f.write(&format!("{}({})", Syntax::ATTR_TARGET, bucket.name()));
        }
        if let Some(html_path) = &prog.html_path {
            write_separator(&mut f);
            f.write(&format!(
                "{}(\"{}\")",
                Syntax::ATTR_HTML,
                escape_str_lit(html_path)
            ));
        }
        f.write("]");
        f.newline();
        first = false;
    }
    // D-VISDEFAULT2: `#PubFile` must precede any `priv`-qualified import in
    // the rendered output — imports are formatted relative to `f.pub_file`
    // (emitting a `priv` prefix when the file is public-by-default), so the
    // literal `#PubFile` marker has to appear *before* the imports loop, not
    // after it. Emitting it post-imports produced output that failed to
    // reparse (`priv use …` with no preceding `#PubFile`) — a real fmt
    // idempotence bug, not just a reordering of independent items.
    if prog.pub_file && !grouped_file_rules && !has_ordered_file_rules {
        first = false;
        f.write(&format!("#{}", Syntax::MARKER_PUB_FILE));
        f.newline();
    }
    // D-PRELUDEX1=A: `#NoPrelude` is a file-level directive; emit before imports
    // so fmt round-trips the opt-out at the top of the file.
    if prog.no_prelude && !grouped_file_rules && !has_ordered_file_rules {
        if !first {
            f.newline();
        }
        first = false;
        f.write(&format!("#{}", Syntax::MARKER_NO_PRELUDE));
        f.newline();
    }
    for imp in &prog.imports {
        if !first {
            f.blank_line_between_items();
        }
        first = false;
        f.emit_leading(imp.span.start);
        f.fmt_import(imp);
        f.emit_trailing(imp.span.end);
    }
    if has_ordered_file_rules && !ordered_file_rules_precede_imports {
        if !first {
            f.blank_line_between_items();
        }
        first = false;
        let rules = ordered_file_rules.iter().collect::<Vec<_>>();
        f.fmt_marker_group(&rules, Syntax::RULE_PREFIX, true);
    }
    // D-WEBDEFAULT1 (ratified 2026-07-01, c134): `#Target(Web)` — the file's
    // default CLI backend. D-WASM1: `#Target(Wasm)`/`#Target(JS)` — the file's
    // web partition ceiling. Neither carries a span (single-instance file
    // markers, same treatment as `#PubFile` above), so this fixed post-import
    // position is canonical, not a preservation of wherever the author
    // originally wrote it in the source. Unlike `#PubFile`, these markers
    // don't gate any import's rendered qualifier, so they have no ordering
    // dependency on the imports loop and keep their original position.
    if prog.default_target.as_deref() == Some(crate::Syntax::BUILD_TARGET_WEB)
        && !grouped_file_rules
        && !has_ordered_file_rules
    {
        if !first {
            f.blank_line_between_items();
        }
        first = false;
        f.write(&format!(
            "#{}({})",
            Syntax::ATTR_TARGET,
            Syntax::WEB_TARGET_DEFAULT_WEB
        ));
        f.newline();
    }
    if let Some(bucket) = prog
        .web_target_ceiling
        .filter(|_| !grouped_file_rules && !has_ordered_file_rules)
    {
        if !first {
            f.blank_line_between_items();
        }
        first = false;
        f.write(&format!("#{}({})", Syntax::ATTR_TARGET, bucket.name()));
        f.newline();
    }
    // D-HTMLPAIR1 (ratified 2026-07-01, c134): `#HTML("path.html")` — the
    // file's explicit companion host page.
    if let Some(html_path) = prog
        .html_path
        .as_ref()
        .filter(|_| !grouped_file_rules && !has_ordered_file_rules)
    {
        if !first {
            f.blank_line_between_items();
        }
        first = false;
        f.write(&format!("#{}(\"{}\")", Syntax::ATTR_HTML, html_path));
        f.newline();
    }
    // D-POLICY-WORD1=A: `#Policy(no_alloc)` — fixed post-import
    // position, same single-instance-marker treatment as `#PubFile`/
    // `#Target(…)`/`#HTML(…)` above (no span to preserve original placement).
    let module_policies = prog.policy_declarations.iter().filter(|d| d.scope == crate::Policy::PolicyScope::Module).cloned().collect::<Vec<_>>();
    if let Some(policy_span) = module_policies.first().map(|d| d.span) {
        if !first {
            f.blank_line_between_items();
        }
        first = false;
        f.emit_leading(policy_span.start);
        f.fmt_policy_declarations(&module_policies);
        f.newline();
        f.emit_trailing(policy_span.end);
    }
    for item in &prog.items {
        if !first {
            f.blank_separator_before_item();
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
    source_toks: &'a [Token],
    comments: Vec<Comment>,
    comment_i: usize,
    out: String,
    col: usize,
    at_line_start: bool,
    indent: usize,
    pending_blank: bool,
    /// Exclusive source boundary for trailing comments while formatting a
    /// nested construct. Prevents an inner statement from claiming a comment
    /// that trails its enclosing expression on the same line.
    trailing_comment_limit: usize,
    /// D-VISDEFAULT2=A: file uses `#PubFile` public-by-default visibility.
    pub_file: bool,
    items: &'a [Item],
    policy_declarations: &'a [crate::Policy::PolicyDeclaration],
    applied_rules: &'a [crate::AST::AppliedRuleApplication],
    /// D-EACH1=C authored forms corresponding to expanded AST statements.
    fenced_statements: &'a [crate::AST::FencedStatement],
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    OrFallback = 0,
    Range = 1,
    Or = 2,
    And = 3,
    Cmp = 4,
    BitOr = 5,
    BitXor = 6,
    BitAnd = 7,
    Shift = 8,
    Add = 9,
    Mul = 10,
    Unary = 11,
    Postfix = 12,
    Primary = 13,
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
    // `pos` is a byte offset that may originate from the formatted output
    // (`is_trailing_comment_at`), so it can land mid-codepoint inside `src`
    // when the source contains multibyte characters (e.g. box-drawing glyphs
    // in a comment). Round down to the nearest char boundary before slicing so
    // a valid input never panics (I2).
    let mut end = pos.min(src.len());
    while end > 0 && !src.is_char_boundary(end) {
        end -= 1;
    }
    src[..end].bytes().filter(|&b| b == b'\n').count()
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
            if c.is_persist {
                src[..c.name_span.start]
                    .rfind(&format!("#{}", Syntax::CONTRACT_PERSIST))
                    .unwrap_or(c.name_span.start)
            } else if c.is_comptime {
                // Prefer force markers, then the live `#Known` marker. Retired
                // keywords remain last-resort recovery starts.
                let before = &src[..c.name_span.start];
                before
                    .rfind("#Static")
                    .or_else(|| before.rfind("#Inline"))
                    .or_else(|| before.rfind(&format!("#{}", Syntax::ATTR_KNOWN)))
                    .or_else(|| before.rfind(Syntax::KW_COMPTIME))
                    .or_else(|| before.rfind(Syntax::KW_CONST))
                    .unwrap_or(c.name_span.start)
            } else {
                c.name_span.start
            }
        }
        Item::Test(t) => src[..t.name_span.start]
            .rfind(&format!("{}{}", Syntax::ATTR_PREFIX, Syntax::KW_TEST))
            .unwrap_or(t.name_span.start),
        Item::Bench(b) => src[..b.name_span.start]
            .rfind(&format!("{}{}", Syntax::ATTR_PREFIX, Syntax::KW_BENCH))
            .unwrap_or(b.name_span.start),
        Item::ExternRust(b) => src[..b.crate_span.start]
            .rfind(Syntax::KW_EXTERN)
            .unwrap_or(b.span.start),
        Item::Trait(t) => type_decl_start(t.is_pub, t.name_span.start, "trait", src),
        // D-QUAL2: tag declarations use their own span.
        Item::Tag(t) => t.span.start,
        Item::EffectDecl(declaration) => declaration.span.start,
        Item::Module(m) => src[..m.name_span.start]
            .rfind(Syntax::KW_MODULE)
            .unwrap_or(m.span.start),
        // S59: the `#Extern`/`#Bindgen` marker precedes the span start.
        Item::CModule(cm) => cm.span.start,
        Item::CodeModule(cm) => src[..cm.name_span.start]
            .rfind(Syntax::KW_MODULE)
            .unwrap_or(cm.span.start),
        // D-DIST1: distinct type declarations use their own span.
        Item::Distinct(d) => d.span.start,
        Item::TypeAlias(a) => a.span.start,
        // D-QUAL3: unit families use their own span.
        Item::UnitFamily(uf) => uf.span.start,
        // D-ERR-CONV: use the from_span (start of `impl Source => Target {}`).
        Item::ErrorConv(ec) => src[..ec.from_span.start]
            .rfind("impl")
            .unwrap_or(ec.from_span.start),
        // D-MIGRATE1: use the migration block's own span.
        Item::Migration(m) => m.span.start,
        // D-STATE-DECL: use the state block's own span.
        Item::StateDecl(s) => s.span.start,
        Item::ProtocolDecl(p) => p.span.start,
        // D-METADERIVE1=A: use the derive block's own span.
        Item::UserDerive(d) => d.span.start,
        // D-GENMOD2=A: generic module template span.
        Item::GenericModule(gm) => gm.span.start,
        // D-GENMOD2=A: module alias span.
        Item::ModuleAlias(ma) => ma.span.start,
    }
}

fn func_decl_start(f: &Func, src: &str) -> usize {
    let before = &src[..f.name_span.start];
    let fn_pos = before.rfind("fn").unwrap_or(f.name_span.start);
    let pos = if f.is_pub {
        before[..fn_pos].rfind("pub").unwrap_or(fn_pos)
    } else {
        fn_pos
    };
    pos
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
        Item::Bench(b) => b.body.last().map(stmt_end).unwrap_or(b.name_span.end),
        Item::ExternRust(b) => b.span.end,
        Item::Trait(t) => t
            .methods
            .last()
            .map(|m| m.span.end)
            .unwrap_or(t.name_span.end),
        Item::Tag(t) => t.span.end,
        Item::EffectDecl(declaration) => declaration.span.end,
        Item::Module(m) => m.span.end,
        Item::CModule(cm) => cm.span.end,
        Item::CodeModule(cm) => cm.span.end,
        Item::Distinct(d) => d.span.end,
        Item::TypeAlias(a) => a.span.end,
        Item::UnitFamily(uf) => uf.span.end,
        // D-ERR-CONV: body_span.end is after the closing `}`.
        Item::ErrorConv(ec) => ec.body_span.end,
        // D-MIGRATE1: use the migration block's own span end.
        Item::Migration(m) => m.span.end,
        // D-STATE-DECL: use the state block's own span end.
        Item::StateDecl(s) => s.span.end,
        Item::ProtocolDecl(p) => p.span.end,
        // D-METADERIVE1=A: use the derive block's own span end.
        Item::UserDerive(d) => d.span.end,
        // D-GENMOD2=A: generic module template span end.
        Item::GenericModule(gm) => gm.span.end,
        // D-GENMOD2=A: module alias span end.
        Item::ModuleAlias(ma) => ma.span.end,
    }
}

fn stmt_end(stmt: &Stmt) -> usize {
    match stmt {
        Stmt::Expr(e) => e.span().end,
        Stmt::Val(b) => b.init.span().end,
        Stmt::Assign { value, .. } => value.span().end,
        Stmt::Return(e, s) => e.as_ref().map(|x| x.span().end).unwrap_or(s.end),
        Stmt::Yield(e, _) => e.span().end,
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
        Stmt::Break(s) | Stmt::Continue(s) | Stmt::BreakLabel(_, s) | Stmt::ContinueLabel(_, s) => {
            s.end
        }
        Stmt::BreakValue(value, _) | Stmt::BreakLabelValue(_, _, value, _) => value.span().end,
        Stmt::CountedLoop { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Loop {
            body: inner,
            span: s,
            ..
        } => inner.last().map(stmt_end).unwrap_or(s.end),
        Stmt::Unsafe { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Impure { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Reactive { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Shield { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Off { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::DebugOnly { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Region { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Policy { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::TaskGroup { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Layout { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Caps { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Grant { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::ComptimeBlock { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::ComptimeIf {
            else_body,
            then_body,
            span,
            ..
        } => else_body
            .as_ref()
            .and_then(|b| b.last())
            .or_else(|| then_body.last())
            .map(stmt_end)
            .unwrap_or(span.end),
        Stmt::ComptimeSwitch { span, .. } => span.end,
        Stmt::ContextBlock { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        // D-TERM1 (ratified 2026-06-22): `live { … }` — use span end.
        Stmt::Live { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        // D-DOTSCOPE1: `.name { … }` scope member — use body/span end.
        Stmt::ScopeMember { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        // D-DET1: `assume_deterministic { … }` — use body/span end.
        Stmt::AssumeDet { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        // D-TXN1–D-TXN4: `#Transact(name) { … }` — use body/span end.
        Stmt::Transact { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
    }
}

impl<'a> Fmt<'a> {
    fn write_loop_continuation_indent(&mut self) {
        self.indent += 1;
        self.write_indent();
        self.indent -= 1;
    }

    fn loop_clause_separator(&mut self, next_start: usize, wrap: bool) {
        self.write(",");
        let mut broke = false;
        while self.comment_i < self.comments.len()
            && self.comments[self.comment_i].span.start < next_start
        {
            let text = self.comments[self.comment_i].text.clone();
            self.write("  ");
            self.write(&text);
            self.comment_i += 1;
            if text.starts_with("//") {
                self.newline();
                self.write_loop_continuation_indent();
                broke = true;
            }
        }
        if wrap && !broke {
            self.newline();
            self.write_loop_continuation_indent();
        } else if !broke {
            self.write(" ");
        }
    }

    fn blank_line_between_items(&mut self) {
        if !self.pending_blank {
            self.newline();
            self.pending_blank = true;
        }
    }

    /// Top-level definitions (types, functions, etc.) are separated by exactly
    /// one blank line (owner, 2026-06-22): end the current line, then guarantee a
    /// single empty line before the next item — never cut it, never double it.
    fn blank_separator_before_item(&mut self) {
        if self.out.is_empty() {
            return;
        }
        if !self.at_line_start {
            self.newline();
        }
        if !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
        self.pending_blank = true;
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
            if span.start >= self.trailing_comment_limit {
                break;
            }
            if span.start >= end && line_of(self.src, span.start) == line_of(self.src, end) {
                self.write("  ");
                self.emit_comment_inline(&text);
                self.comment_i += 1;
            } else {
                break;
            }
        }
    }

    /// Several item kinds (derive/module/tag/migration/… blocks) are emitted
    /// **verbatim** — the raw source slice is copied straight to output
    /// instead of being walked comment-by-comment. Any comment whose span
    /// falls inside that slice is therefore already visible in the copied
    /// text, but `comment_i` never advanced past it. Left alone, the next
    /// `emit_leading`/`emit_remaining_comments` call finds it "unconsumed"
    /// and re-emits it before the following item — a real duplication bug,
    /// not just a stale index. Call this right after writing a verbatim
    /// item's text, with that item's span end, to skip past its internal
    /// comments without re-printing them.
    pub(super) fn skip_verbatim_comments(&mut self, end: usize) {
        while self.comment_i < self.comments.len() && self.comments[self.comment_i].span.start < end
        {
            self.comment_i += 1;
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

    fn with_trailing_comment_limit<R>(
        &mut self,
        limit: usize,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = self.trailing_comment_limit;
        self.trailing_comment_limit = previous.min(limit);
        let result = f(self);
        self.trailing_comment_limit = previous;
        result
    }

    /// The AST intentionally stores only the name span for calls and method
    /// calls. Comment attachment needs the whole source statement, including
    /// call/lambda closing delimiters. Walk lexer tokens to the first
    /// top-level terminator instead of mistaking an inner expression span for
    /// the statement end.
    fn statement_source_end(&self, stmt: &Stmt) -> usize {
        if !matches!(
            stmt,
            Stmt::Expr(_) | Stmt::Val(_) | Stmt::Assign { .. } | Stmt::Return(..) | Stmt::Yield(..)
        ) {
            return stmt_end(stmt);
        }

        let start = stmt_start(stmt);
        let mut parens = 0usize;
        let mut brackets = 0usize;
        let mut braces = 0usize;
        let mut last_end = stmt_end(stmt);
        let first = self
            .source_toks
            .partition_point(|token| token.span.start < start);
        for token in &self.source_toks[first..] {
            match token.kind {
                TokKind::LineComment(_) | TokKind::BlockComment(_) => continue,
                TokKind::Semi if parens == 0 && brackets == 0 && braces == 0 => return last_end,
                TokKind::Eof => return last_end,
                TokKind::LParen => parens += 1,
                TokKind::RParen if parens == 0 => return last_end,
                TokKind::RParen => parens -= 1,
                TokKind::LBracket => brackets += 1,
                TokKind::RBracket if brackets == 0 => return last_end,
                TokKind::RBracket => brackets -= 1,
                TokKind::LBrace => braces += 1,
                TokKind::RBrace if braces == 0 => return last_end,
                TokKind::RBrace => braces -= 1,
                _ => {}
            }
            last_end = token.span.end;
        }
        last_end
    }

    /// S69 (D-SG3): did the author put a line break before this chain step's
    /// `.`? `from` is the receiver's end offset, `to` is the method/field's
    /// start offset; a `\n` in between means the chain was broken on purpose.
    fn chain_break_between(&self, from: usize, to: usize) -> bool {
        from <= to && self.src.get(from..to).is_some_and(|s| s.contains('\n'))
    }

    /// Return the exact type spelling through its next top-level terminator.
    /// The AST canonicalizes union members during parsing, so this source
    /// slice is the only retained order fact.
    fn source_type_spelling(&self, start: usize) -> Option<&str> {
        let first = self
            .source_toks
            .partition_point(|token| token.span.start < start);
        let mut parens = 0usize;
        let mut brackets = 0usize;
        let mut angles = 0usize;
        let mut end = start;
        for token in &self.source_toks[first..] {
            let top_level = parens == 0 && brackets == 0 && angles == 0;
            if top_level
                && matches!(
                    token.kind,
                    TokKind::Comma
                        | TokKind::Semi
                        | TokKind::LambdaArrow
                        | TokKind::RParen
                        | TokKind::RBrace
                        | TokKind::Eof
                )
            {
                break;
            }
            match token.kind {
                TokKind::LParen => parens += 1,
                TokKind::RParen => parens = parens.saturating_sub(1),
                TokKind::LBracket => brackets += 1,
                TokKind::RBracket => brackets = brackets.saturating_sub(1),
                TokKind::Lt => angles += 1,
                TokKind::Gt => angles = angles.saturating_sub(1),
                TokKind::Shr => angles = angles.saturating_sub(2),
                _ => {}
            }
            end = token.span.end;
        }
        (end > start).then(|| &self.src[start..end])
    }

    fn end_block(&mut self) {
        self.newline();
        self.write("}");
    }

    /// True if any tracked comment falls inside `open..close` (gate c).
    fn span_has_comment(&self, open: usize, close: usize) -> bool {
        self.comments
            .iter()
            .any(|c| c.span.start >= open && c.span.start < close)
    }

    /// D-FMT1 / D-FMTCOLLAPSE1=B: render one brace body. Collapse any fitting
    /// simple body to one line (comments and over-width stay multiline).
    fn fmt_body(&mut self, body: &[Stmt]) {
        self.fmt_control_body(body);
    }

    /// The expanded brace-body shape: newline, indented statements, closing `}`.
    /// The caller has already emitted ` {`.
    fn fmt_body_expanded(&mut self, body: &[Stmt]) {
        self.newline();
        self.with_indent(|f| f.fmt_block_stmts(body));
        self.end_block();
    }

    /// Locate the `{ … }` that brackets a lone body statement: `open` is the
    /// offset just after `{`, `close` the offset of `}`. Returns `None` if the
    /// braces can't be found (defensive; the body then expands).
    fn single_stmt_braces(&self, stmt: &Stmt) -> Option<(usize, usize)> {
        let start = stmt_start(stmt);
        let end = stmt_end(stmt);
        let open = self.src.get(..start)?.rfind('{')? + 1;
        let close = end + self.src.get(end..)?.find('}')?;
        (open <= close).then_some((open, close))
    }

    /// Locate the `{ … }` bracketing an if-expression branch whose only content
    /// is `value`. `open` is the offset just after `{`, `close` the offset of `}`.
    fn value_block_braces(&self, value: &crate::AST::Expr) -> Option<(usize, usize)> {
        let start = value.span().start;
        let end = value.span().end;
        let open = self.src.get(..start)?.rfind('{')? + 1;
        let close = end + self.src.get(end..)?.find('}')?;
        (open <= close).then_some((open, close))
    }

    /// D-FMT1: attempt the inline if-expression branch `{ value }`. Renders into
    /// `self.out`, enforces the width-100 floor, rolls back on overflow. The
    /// caller has already emitted the opening `{`.
    fn try_inline_value_block(&mut self, stmts: &[Stmt], value: &crate::AST::Expr) -> bool {
        if !self.value_block_inlineable(stmts, value) {
            return false;
        }
        let saved_out = self.out.len();
        let saved_col = self.col;
        let saved_line_start = self.at_line_start;
        self.write(" ");
        self.fmt_expr(value, Prec::OrFallback);
        self.write(" }");
        if self.col <= MAX_WIDTH {
            return true;
        }
        self.out.truncate(saved_out);
        self.col = saved_col;
        self.at_line_start = saved_line_start;
        false
    }

    fn fmt_control_body(&mut self, body: &[Stmt]) {
        if let [statement] = body {
            let comment_free = self
                .single_stmt_braces(statement)
                .map_or(true, |(open, close)| !self.span_has_comment(open, close));
            if is_simple_stmt(statement) && comment_free {
                let saved_out = self.out.len();
                let saved_col = self.col;
                let saved_line_start = self.at_line_start;
                let saved_pending_blank = self.pending_blank;
                let saved_comment_i = self.comment_i;
                self.write(" ");
                self.fmt_stmt_inline(statement);
                self.write(" }");
                if self.col <= MAX_WIDTH {
                    return;
                }
                self.out.truncate(saved_out);
                self.col = saved_col;
                self.at_line_start = saved_line_start;
                self.pending_blank = saved_pending_blank;
                self.comment_i = saved_comment_i;
            }
        }
        self.fmt_body_expanded(body);
    }
}

impl Prec {
    fn add_rhs(self) -> Self {
        match self {
            Prec::OrFallback => Prec::OrFallback,
            Prec::Range => Prec::Or,
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
            LValue::Field { base, .. } => base.span().start,
        },
        Stmt::Return(_, s) => s.start,
        Stmt::Yield(_, s) => s.start,
        Stmt::While { span, .. } | Stmt::For { span, .. } | Stmt::Switch { span, .. } => span.start,
        Stmt::Break(s) | Stmt::Continue(s) | Stmt::BreakLabel(_, s) | Stmt::ContinueLabel(_, s) => {
            s.start
        }
        Stmt::BreakValue(_, span) | Stmt::BreakLabelValue(_, _, _, span) => span.start,
        Stmt::Loop { span: s, .. } | Stmt::CountedLoop { span: s, .. } => s.start,
        Stmt::Unsafe { span, .. } => span.start,
        Stmt::Impure { span, .. } => span.start,
        Stmt::Reactive { span, .. } => span.start,
        Stmt::Shield { span, .. } => span.start,
        Stmt::Off { span, .. } => span.start,
        Stmt::DebugOnly { span, .. } => span.start,
        Stmt::Region { span, .. } => span.start,
        Stmt::Policy { span, .. } => span.start,
        Stmt::TaskGroup { span, .. } => span.start,
        Stmt::Layout { span, .. } => span.start,
        Stmt::Caps { span, .. } => span.start,
        Stmt::Grant { span, .. } => span.start,
        Stmt::ComptimeBlock { span, .. } => span.start,
        Stmt::ComptimeIf { span, .. } => span.start,
        Stmt::ComptimeSwitch { span, .. } => span.start,
        Stmt::ContextBlock { span, .. } => span.start,
        // D-TERM1 (ratified 2026-06-22): `live { … }` — use span start.
        Stmt::Live { span, .. } => span.start,
        // D-DOTSCOPE1: `.name { … }` scope member — use span start.
        Stmt::ScopeMember { span, .. } => span.start,
        // D-DET1: `assume_deterministic { … }` — use span start.
        Stmt::AssumeDet { span, .. } => span.start,
        // D-TXN1–D-TXN4: `#Transact(name) { … }` — use span start.
        Stmt::Transact { span, .. } => span.start,
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

/// S34/S67: re-emit an integer literal exactly as the author spelled it —
/// radix prefix (`0x`/`0o`/`0b`), digit separators (`_`), and hex-digit case
/// included. The AST stores only the value, so the spelling must come from
/// the source slice at the literal's span. Falls back to plain decimal when
/// the slice doesn't round-trip to the same value (synthesized Int nodes
/// borrow a nearby span whose text isn't a number).
pub(crate) fn int_literal_spelling(src: &str, span: Span, n: i64) -> String {
    let Some(slice) = src.get(span.start..span.end) else {
        return n.to_string();
    };
    let cleaned: String = slice.chars().filter(|c| *c != '_').collect();
    let parsed = if let Some(digits) = cleaned.strip_prefix("0x").or(cleaned.strip_prefix("0X")) {
        i64::from_str_radix(digits, 16)
    } else if let Some(digits) = cleaned.strip_prefix("0o").or(cleaned.strip_prefix("0O")) {
        i64::from_str_radix(digits, 8)
    } else if let Some(digits) = cleaned.strip_prefix("0b").or(cleaned.strip_prefix("0B")) {
        i64::from_str_radix(digits, 2)
    } else {
        cleaned.parse::<i64>()
    };
    match parsed {
        Ok(v) if v == n => slice.to_string(),
        _ => n.to_string(),
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
        // The lexer has no `\u{...}` escape (that spelling would be new
        // user-typeable syntax), so every other char must round-trip raw.
        c => format!("'{}'", c),
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
            // No `\u{...}` escape exists in the lexer; raw round-trip keeps
            // the formatter lossless on control characters.
            c => out.push(c),
        }
    }
    out
}

pub(super) fn escape_str_lit(s: &str) -> String {
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
            // No `\u{...}` escape exists in the lexer; raw round-trip keeps
            // the formatter lossless on control characters.
            c => out.push(c),
        }
    }
    out
}

/// Lex + parse + format. Parse errors propagate; sema is not required.
pub fn format_source(src: &str) -> Result<String, Vec<crate::Diagnostics::Diagnostic>> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let prog = crate::Parser::parse_for_fmt(&toks)?;
    let comment_toks: Vec<_> = toks
        .iter()
        .filter(|token| {
            matches!(
                token.kind,
                TokKind::LineComment(_) | TokKind::BlockComment(_)
            )
        })
        .cloned()
        .collect();
    Ok(format_program_with_tokens(&prog, src, &comment_toks, &toks))
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

#[cfg(test)]
mod tests {
    use super::format_source;

    #[test]
    fn trailing_call_comment_stays_outside_lambda_block() {
        let source = r#"fn transfer(from: Shared<Account>, amount: Int) {
    #Transact(tx) {
        from.edit(a => { a.balance -= amount })  // both land, or neither
    }
}
"#;
        let formatted = format_source(source).expect("source should format");
        let close = formatted.find("})").expect("lambda call should close");
        let comment = formatted
            .find("// both land, or neither")
            .expect("trailing comment should survive");
        assert!(close < comment, "comment moved into lambda:\n{formatted}");
        assert_eq!(
            formatted,
            format_source(&formatted).expect("formatted source should re-format")
        );
    }

    #[test]
    fn inner_comment_stays_before_enclosing_block_close() {
        let source = "fn run() {\n    if ready { launch() /* go */ }\n}\n";
        let formatted = format_source(source).expect("source should format");
        let comment = formatted.find("/* go */").expect("comment should survive");
        let inner_close = formatted.find('}').expect("if body should close");
        assert!(comment < inner_close, "comment left its block:\n{formatted}");
        assert_eq!(
            formatted,
            format_source(&formatted).expect("formatted source should re-format")
        );
    }
}
