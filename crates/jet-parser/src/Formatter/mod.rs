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
use crate::Lexer::{comments, TokKind, Token};
use crate::Syntax;
use crate::AST::{BinOp, ElseBranch, Func, IfStmt, Item, LValue, Program, Stmt};

const INDENT: usize = 4;

/// D-FMT1: the width floor (gate d) for keeping a body on one line. A rendered
/// inline body whose final column exceeds this expands instead.
const MAX_WIDTH: usize = 100;

/// D-FMT1: may this statement render inline inside a one-line brace body? Only
/// non-block statements qualify; every block-bearing variant (`if`, loops,
/// `switch`, `#unsafe`, etc.) must expand so fmt never nests a block inline.
fn is_simple_stmt(stmt: &Stmt) -> bool {
    matches!(
        stmt,
        Stmt::Expr(_)
            | Stmt::Val(_)
            | Stmt::Assign { .. }
            | Stmt::Return(..)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(..)
            | Stmt::ContinueLabel(..)
    )
}

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
        pub_file: prog.pub_file,
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
    if prog.pub_file {
        if !first {
            f.blank_line_between_items();
        }
        first = false;
        f.write(&format!("#{}", Syntax::MARKER_PUB_FILE));
        f.newline();
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
    comments: Vec<Comment>,
    comment_i: usize,
    out: String,
    col: usize,
    at_line_start: bool,
    indent: usize,
    pending_blank: bool,
    /// D-VISDEFAULT2=A: file uses `#PubFile` public-by-default visibility.
    pub_file: bool,
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
            let kw = if c.is_comptime {
                Syntax::KW_COMPTIME
            } else {
                Syntax::KW_CONST
            };
            src[..c.name_span.start]
                .rfind(kw)
                .unwrap_or(c.name_span.start)
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
        // D-ERR-CONV: use the from_span (start of `impl Source -> Target {}`).
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
    // S60 (D-CASING1 follow-on) / D-MARKERMOVE2: the `@Pure` marker precedes
    // `pub`/`fn`. The retired `@Pure` spelling still parses (E0062) so it's
    // searched too, preferring whichever match sits closer to `pos`.
    if f.is_pure {
        let at_pos =
            before[..pos].rfind(&format!("{}{}", Syntax::CONTRACT_PREFIX, Syntax::KW_PURE));
        let hash_pos = before[..pos].rfind(&format!("{}{}", Syntax::ATTR_PREFIX, Syntax::KW_PURE));
        match (at_pos, hash_pos) {
            (Some(a), Some(h)) => a.max(h),
            (Some(a), None) => a,
            (None, Some(h)) => h,
            (None, None) => pos,
        }
    } else {
        pos
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
        Item::Bench(b) => b.body.last().map(stmt_end).unwrap_or(b.name_span.end),
        Item::ExternRust(b) => b.span.end,
        Item::Trait(t) => t
            .methods
            .last()
            .map(|m| m.span.end)
            .unwrap_or(t.name_span.end),
        Item::Tag(t) => t.span.end,
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
        Stmt::Break(s) | Stmt::Continue(s) | Stmt::BreakLabel(_, s) | Stmt::ContinueLabel(_, s) => {
            s.end
        }
        Stmt::CountedLoop { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Loop {
            body: inner,
            span: s,
            ..
        } => inner.last().map(stmt_end).unwrap_or(s.end),
        Stmt::Unsafe { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Impure { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Reactive { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::SuppressMustUse { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        Stmt::Region { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
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
        Stmt::ContextBlock { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        // D-TERM1 (ratified 2026-06-22): `live { … }` — use span end.
        Stmt::Live { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        // D-DET1: `assume_deterministic { … }` — use body/span end.
        Stmt::AssumeDet { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
        // D-TXN1–D-TXN4: `#Transact(name) { … }` — use body/span end.
        Stmt::Transact { body, span, .. } => body.last().map(stmt_end).unwrap_or(span.end),
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
        from <= to && self.src.get(from..to).is_some_and(|s| s.contains('\n'))
    }

    fn end_block(&mut self) {
        self.newline();
        self.write("}");
    }

    /// D-FMT1 (revises S44): is the brace body the author placed between `open`
    /// (the offset just after `{`) and `close` (the offset of `}`) eligible to
    /// stay on one line? Mirrors the S69 dot-chain author-intent mechanism
    /// (`chain_break_between`): the author's line-count choice is preserved, fmt
    /// only normalizes spacing within it. Gates: exactly one statement, that
    /// statement is simple (no nested block), no comment inside the braces, and
    /// the author wrote the whole body on a single source line. The width-100
    /// floor is enforced after rendering (see `fmt_body`).
    fn body_inline_eligible(&self, body: &[Stmt], open: usize, close: usize) -> bool {
        open <= close
            && body.len() == 1
            && is_simple_stmt(&body[0])
            && !self.span_has_comment(open, close)
            && self.src.get(open..close).is_some_and(|s| !s.contains('\n'))
    }

    /// True if any tracked comment falls inside `open..close` (gate c).
    fn span_has_comment(&self, open: usize, close: usize) -> bool {
        self.comments
            .iter()
            .any(|c| c.span.start >= open && c.span.start < close)
    }

    /// D-FMT1: render one brace body, choosing inline vs expanded by author
    /// intent. The caller has already emitted ` {`. On the inline path this emits
    /// ` <stmt> }`; on the expand path it emits the original newline-indented form
    /// and the closing `}` via `end_block`. The body's brace offsets are located
    /// from the lone statement's source span (only single-statement bodies are
    /// inline-eligible, so an empty/multi body always expands without offsets).
    fn fmt_body(&mut self, body: &[Stmt]) {
        if body.len() == 1 {
            if let Some((open, close)) = self.single_stmt_braces(&body[0]) {
                if self.try_inline_body(body, open, close) {
                    return;
                }
            }
        }
        self.fmt_body_expanded(body);
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

    /// Attempt the inline shape `<stmt> }`. Renders into `self.out`, then checks
    /// the width-100 floor (gate d); if it overflows, rolls the output back and
    /// returns `false` so the caller expands. The caller has already emitted the
    /// opening `{` (with its leading space).
    fn try_inline_body(&mut self, body: &[Stmt], open: usize, close: usize) -> bool {
        if !self.body_inline_eligible(body, open, close) {
            return false;
        }
        let saved_out = self.out.len();
        let saved_col = self.col;
        let saved_line_start = self.at_line_start;
        self.write(" ");
        self.fmt_stmt_inline(&body[0]);
        self.write(" }");
        if self.col <= MAX_WIDTH {
            return true;
        }
        // Width floor failed: roll back to the state right after the `{`.
        self.out.truncate(saved_out);
        self.col = saved_col;
        self.at_line_start = saved_line_start;
        false
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
            LValue::Field { base, .. } => base.span().start,
        },
        Stmt::Return(_, s) => s.start,
        Stmt::Yield(_, s) => s.start,
        Stmt::If(i) => i.span.start,
        Stmt::While { span, .. } | Stmt::For { span, .. } | Stmt::Switch { span, .. } => span.start,
        Stmt::Break(s) | Stmt::Continue(s) | Stmt::BreakLabel(_, s) | Stmt::ContinueLabel(_, s) => {
            s.start
        }
        Stmt::Loop { span: s, .. } | Stmt::CountedLoop { span: s, .. } => s.start,
        Stmt::Unsafe { span, .. } => span.start,
        Stmt::Impure { span, .. } => span.start,
        Stmt::Reactive { span, .. } => span.start,
        Stmt::SuppressMustUse { span, .. } => span.start,
        Stmt::Region { span, .. } => span.start,
        Stmt::TaskGroup { span, .. } => span.start,
        Stmt::Layout { span, .. } => span.start,
        Stmt::Caps { span, .. } => span.start,
        Stmt::Grant { span, .. } => span.start,
        Stmt::ComptimeBlock { span, .. } => span.start,
        Stmt::ComptimeIf { span, .. } => span.start,
        Stmt::ContextBlock { span, .. } => span.start,
        // D-TERM1 (ratified 2026-06-22): `live { … }` — use span start.
        Stmt::Live { span, .. } => span.start,
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
pub fn format_source(src: &str) -> Result<String, Vec<crate::Diagnostics::Diagnostic>> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let comments = comments(&toks);
    let prog = crate::Parser::parse_for_fmt(&toks)?;
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
