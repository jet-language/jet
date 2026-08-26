//! S6-R statement-terminator insertion: after raw lexing, a post-pass inserts
//! a synthetic `Semi` at each line end that follows a statement-ending token.

use crate::Diagnostics::{Diagnostic, Span};

use super::Scan::{lex_raw, lex_raw_config, lex_raw_generated};
use super::Tokens::{is_comment, TokKind, Token};

/// Lex the whole file. Always returns a token stream (ending in Eof) plus
/// every problem found along the way — M1 error recovery.
///
/// S6-R: after raw lexing, a post-pass inserts a synthetic statement
/// terminator (`Semi`) at each line end that follows a statement-ending token.
/// The grammar stays terminator-based; users never type `;`.
pub fn lex(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let (mut toks, mut diags) = lex_raw(src);
    insert_terminators(src, &mut toks, &mut diags);
    (toks, diags)
}

/// Lex a Jetpack config file. The only lexical difference from ordinary Jet
/// source is that `://` inside an unquoted config template is URL punctuation,
/// not the start of a line comment.
pub fn lex_config(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let (mut toks, mut diags) = lex_raw_config(src);
    insert_terminators(src, &mut toks, &mut diags);
    (toks, diags)
}

/// Compiler/tool-generated Jet fragments may use the reserved `__name`
/// namespace. User source must always go through [`lex`].
pub fn lex_generated(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let (mut toks, mut diags) = lex_raw_generated(src);
    insert_terminators(src, &mut toks, &mut diags);
    (toks, diags)
}

/// True when a token of this kind ends a statement, so a following line break
/// should get a synthetic `Semi` (S6-R, Go's rule).
fn ends_statement(kind: &TokKind) -> bool {
    matches!(
        kind,
        TokKind::Ident(_)
            | TokKind::Str(_)
            | TokKind::Int(..)
            | TokKind::Float(..)
            | TokKind::UnitNumber { .. } // D-UNITLIT1: `500ms` ends a line like any literal
            | TokKind::Char(_)
            | TokKind::KwTrue
            | TokKind::KwFalse
            | TokKind::KwSelf
            | TokKind::KwNull
            | TokKind::KwBreak
            | TokKind::KwReturn
            | TokKind::RParen
            | TokKind::RBracket
            | TokKind::RBrace
            | TokKind::Question      // S7: `expr?` trailing propagation
            | TokKind::PlusPlus      // D-INCR1: postfix `x++` / `x--` statement
            | TokKind::MinusMinus    // D-INCR1: postfix `x++` / `x--` statement
            | TokKind::Gt            // generic type close `[Int]` at line end
            | TokKind::Shr // nested generic close `Map<K, List<V>>`
    )
}

/// True when a line that *starts* with this token continues the previous line,
/// so no terminator is inserted before it (S6-R continuation suppression): a
/// leading `.` (S69 method/field chain) or a binary/logical operator.
fn suppresses_terminator(kind: &TokKind) -> bool {
    matches!(
        kind,
        TokKind::FenceClose
            | TokKind::Dot
            | TokKind::QuestionDot
            | TokKind::AndAnd
            | TokKind::OrOr
            | TokKind::Plus
            | TokKind::Minus
            | TokKind::Star
            | TokKind::Slash
            | TokKind::SlashPercent
            | TokKind::Percent
            | TokKind::PercentPercent
            | TokKind::EqEq
            | TokKind::NotEq
            | TokKind::Lt
            | TokKind::Gt
            | TokKind::Le
            | TokKind::Ge
            | TokKind::Compare
            | TokKind::Amp
            | TokKind::Pipe
            | TokKind::Caret
            | TokKind::TildePipe
            | TokKind::Shl
            | TokKind::Shr
            | TokKind::QuestionQuestion // S35/S71 fallback continues the expr
    )
}

/// D-DOTSCOPE1: does the token at `i` (a `.`) begin a scope-member statement —
/// `.ident { … }` or `.ident(args) { … }`? Used to break a fluent chain so the
/// leading-dot member reads as a new statement. `expr.field { }` is never a
/// legal chain, so this reinterpretation is unambiguous for the no-arg form; the
/// arg form wins the scope-member reading at statement position (D-DOTSCOPE1).
fn scope_member_starts_at(toks: &[Token], i: usize) -> bool {
    if !matches!(toks.get(i).map(|t| &t.kind), Some(TokKind::Dot)) {
        return false;
    }
    let mut j = i + 1;
    if !matches!(toks.get(j).map(|t| &t.kind), Some(TokKind::Ident(_))) {
        return false;
    }
    j += 1;
    match toks.get(j).map(|t| &t.kind) {
        Some(TokKind::LBrace) => true,
        Some(TokKind::LParen) => {
            // Scan to the matching `)`, then require a `{` immediately after.
            let mut depth = 0usize;
            while j < toks.len() {
                match &toks[j].kind {
                    TokKind::LParen => depth += 1,
                    TokKind::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            j += 1;
                            break;
                        }
                    }
                    TokKind::Eof => return false,
                    _ => {}
                }
                j += 1;
            }
            matches!(toks.get(j).map(|t| &t.kind), Some(TokKind::LBrace))
        }
        _ => false,
    }
}

/// True when `kind` can start a leading-dot enum/group pattern (D-ENUMDOT1 /
/// D-TAG1): PascalCase ident or `null`.
fn leading_dot_variant_token(kind: &TokKind) -> bool {
    match kind {
        TokKind::Ident(name) => name.chars().next().is_some_and(char::is_uppercase),
        TokKind::KwNull => true,
        _ => false,
    }
}

/// D-IF3 / D-ENUMDOT1: does the token at `i` (a `.`) begin a dispatch arm head
/// — `.Variant ->`, `.Variant(payload) ->`, `.Group.Leaf ->`, or
/// `.{ … } ->` — rather than a fluent chain step? Without a terminator, a
/// braceless prior arm body would glue onto the next `.Variant` as a field
/// access and then choke on `->`.
fn dispatch_arm_starts_at(src: &str, toks: &[Token], i: usize) -> bool {
    if !matches!(toks.get(i).map(|t| &t.kind), Some(TokKind::Dot)) {
        return false;
    }
    let mut j = i + 1;
    // D-DESTRUCT1: `.{ … } -> …` — `expr\n.{` is never a legal chain.
    if matches!(toks.get(j).map(|t| &t.kind), Some(TokKind::LBrace)) {
        let mut depth = 0usize;
        while j < toks.len() {
            match &toks[j].kind {
                TokKind::LBrace => depth += 1,
                TokKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        j += 1;
                        break;
                    }
                }
                TokKind::Eof => return false,
                _ => {}
            }
            j += 1;
        }
        return matches!(
            toks.get(j).map(|t| &t.kind),
            Some(TokKind::UnifiedArrow | TokKind::Arrow | TokKind::LambdaArrow)
        );
    }
    if !toks
        .get(j)
        .map(|t| leading_dot_variant_token(&t.kind))
        .unwrap_or(false)
    {
        return false;
    }
    j += 1;
    // D-TAG1: `.Fire.Burn` leaf path.
    while matches!(toks.get(j).map(|t| &t.kind), Some(TokKind::Dot))
        && toks
            .get(j + 1)
            .map(|t| leading_dot_variant_token(&t.kind))
            .unwrap_or(false)
    {
        j += 2;
    }
    if matches!(toks.get(j).map(|t| &t.kind), Some(TokKind::LParen)) {
        let mut depth = 0usize;
        while j < toks.len() {
            match &toks[j].kind {
                TokKind::LParen => depth += 1,
                TokKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        j += 1;
                        break;
                    }
                }
                TokKind::Eof => return false,
                _ => {}
            }
            j += 1;
        }
    }
    // D-IFDIST1: a braceless arm may add a Boolean guard after its variant
    // payload (`.Key(key) && key == "b" -> ...`). Scan that guard only on its
    // source line; an unrelated arrow on a later line must not turn a fluent
    // chain into a new arm.
    if matches!(
        toks.get(j).map(|t| &t.kind),
        Some(TokKind::AndAnd | TokKind::OrOr)
    ) {
        let guard_start = toks[j].span.start;
        let mut depth = 0usize;
        while let Some(token) = toks.get(j) {
            match &token.kind {
                TokKind::LParen | TokKind::LBracket | TokKind::LBrace => depth += 1,
                TokKind::RParen | TokKind::RBracket | TokKind::RBrace => {
                    depth = depth.saturating_sub(1);
                }
                kind if matches!(
                    kind,
                    TokKind::UnifiedArrow | TokKind::Arrow | TokKind::LambdaArrow
                ) && depth == 0 =>
                {
                    return src
                        .get(guard_start..token.span.start)
                        .is_some_and(|guard| !guard.contains('\n'));
                }
                _ => {}
            }
            j += 1;
        }
        return false;
    }
    matches!(
        toks.get(j).map(|t| &t.kind),
        Some(TokKind::UnifiedArrow | TokKind::Arrow | TokKind::LambdaArrow)
    )
}

/// D-RESULT-DECON2=B: a compact Result handler may put its failure separator
/// on the next line. Only `! name ->` suppresses the line terminator; a normal
/// leading unary `!` keeps the ordinary statement boundary.
fn result_handler_failure_starts_at(toks: &[Token], i: usize) -> bool {
    matches!(toks.get(i).map(|t| &t.kind), Some(TokKind::Bang))
        && matches!(toks.get(i + 1).map(|t| &t.kind), Some(TokKind::Ident(_)))
        && toks
            .get(i + 2)
            .is_some_and(|token| matches!(token.kind, TokKind::UnifiedArrow))
}

/// S6-R post-pass: walk the code tokens (comments are trivia, skipped but kept
/// in the stream) and insert a synthetic `Semi` whenever a statement-ending
/// token is followed — across a line break — by a token that does not continue
/// the line. `->` and `{` never trigger insertion (they must stay on the
/// closing `)` line, S44); a split `-> Type` / `{` is E0986.
fn insert_terminators(src: &str, toks: &mut Vec<Token>, diags: &mut Vec<Diagnostic>) {
    let bytes = src.as_bytes();
    let has_newline_between = |a: usize, b: usize| -> bool {
        a <= b && a <= bytes.len() && b <= bytes.len() && bytes[a..b].contains(&b'\n')
    };

    let mut out: Vec<Token> = Vec::with_capacity(toks.len() + 8);
    let mut last_code: Option<usize> = None; // index into `toks` of last code token

    let mut i = 0;
    while i < toks.len() {
        let cur = &toks[i];
        if is_comment(&cur.kind) {
            out.push(cur.clone());
            i += 1;
            continue;
        }
        // S6-R: at EOF, terminate a final statement that ends right before the
        // end of the file (covers a file with no trailing newline). A block
        // close `}` instead relies on the line-break rule below — a real
        // statement always sits on its own line above the `}`, while a
        // single-line struct/map literal `{ x: 1 }` must NOT get a terminator.
        if matches!(cur.kind, TokKind::Eof) {
            if let Some(prev_idx) = last_code {
                let prev = &toks[prev_idx].kind;
                // A trailing `}` (a closed block/item) needs no terminator
                // before EOF; only a bare final expression/value does.
                if ends_statement(prev) && !matches!(prev, TokKind::RBrace) {
                    let at = toks[prev_idx].span.end;
                    out.push(Token {
                        kind: TokKind::Semi,
                        span: Span::new(at, at),
                    });
                }
            }
            out.push(cur.clone());
            i += 1;
            continue;
        }

        // Decide whether to insert a terminator BEFORE this token, based on the
        // previous code token and an intervening line break.
        if let Some(prev_idx) = last_code {
            let prev = &toks[prev_idx];
            let crossed_line = has_newline_between(prev.span.end, cur.span.start);
            if crossed_line && ends_statement(&prev.kind) {
                // E0986: a callable/control arrow, effect row, or `{` split
                // onto the next line from a `)`.
                if matches!(
                    cur.kind,
                    TokKind::UnifiedArrow
                        | TokKind::Arrow
                        | TokKind::LambdaArrow
                        | TokKind::Eq
                        | TokKind::MinusMinus
                        | TokKind::LBrace
                ) && matches!(prev.kind, TokKind::RParen)
                {
                    let spelling = match &cur.kind {
                        TokKind::UnifiedArrow => "->",
                        TokKind::Arrow => ":>",
                        TokKind::LambdaArrow => "=>",
                        TokKind::Eq => "-[…]>",
                        TokKind::MinusMinus => "-[…]>",
                        _ => "{",
                    };
                    diags.push(Diagnostic::error(
                        "E0986",
                        format!("`{spelling}` must stay on the same line as the closing `)`"),
                        "an arrow, effect row, or opening block continues the header on its line"
                            .to_string(),
                        format!("move `{spelling}` up to the `)` line"),
                        Some(cur.span),
                    ));
                    // Do not insert a terminator; let the parser keep going.
                } else if (!suppresses_terminator(&cur.kind)
                    // D-DOTSCOPE1: a leading `.` normally continues a fluent chain
                    // (suppressed), but `.name { … }` / `.name(args) { … }` at the
                    // start of a line is a scope-member statement, not a chain —
                    // `expr.field { }` is never legal (E0335), so breaking the chain
                    // here is unambiguous. Insert the terminator so the parser sees
                    // a fresh statement.
                    || scope_member_starts_at(toks, i)
                    // D-IF3 / D-ENUMDOT1: `.Variant ->` / `.Variant(x) ->` /
                    // `.{ … } ->` at line start is the next dispatch arm, not a
                    // field chain off the previous braceless arm body.
                    || dispatch_arm_starts_at(src, toks, i))
                    // A closing `)` / `]` on its own line never begins a
                    // statement, so a terminator before it is never grammatical
                    // (multi-line call args / list / map). Suppress it. A `}` is
                    // NOT suppressed: a block close legitimately ends a statement.
                    // D-RESULT-DECON2=B: the second compact handler arm may
                    // begin on its own line without becoming a new statement.
                    && !result_handler_failure_starts_at(toks, i)
                    && !matches!(cur.kind, TokKind::RParen | TokKind::RBracket)
                {
                    out.push(Token {
                        kind: TokKind::Semi,
                        span: Span::new(prev.span.end, prev.span.end),
                    });
                }
            }
        }

        out.push(cur.clone());
        last_code = Some(i);
        i += 1;
    }

    *toks = out;
}
