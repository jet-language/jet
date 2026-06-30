//! S6-R statement-terminator insertion: after raw lexing, a post-pass inserts
//! a synthetic `Semi` at each line end that follows a statement-ending token.

use crate::Diagnostics::{Diagnostic, Span};

use super::Scan::lex_raw;
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

/// True when a token of this kind ends a statement, so a following line break
/// should get a synthetic `Semi` (S6-R, Go's rule).
fn ends_statement(kind: &TokKind) -> bool {
    matches!(
        kind,
        TokKind::Ident(_)
            | TokKind::Str(_)
            | TokKind::Int(_)
            | TokKind::Float(_)
            | TokKind::Char(_)
            | TokKind::KwTrue
            | TokKind::KwFalse
            | TokKind::KwSelf
            | TokKind::KwNull
            | TokKind::KwBreak
            | TokKind::KwContinue
            | TokKind::KwReturn
            | TokKind::RParen
            | TokKind::RBracket
            | TokKind::RBrace
            | TokKind::Question      // S7: `expr?` trailing propagation
            | TokKind::PlusPlus      // D-INCR1: postfix `x++` / `x--` statement
            | TokKind::MinusMinus    // D-INCR1: postfix `x++` / `x--` statement
            | TokKind::Gt            // generic type close `List<Int>` at line end
            | TokKind::Shr // nested generic close `Map<K, List<V>>`
    )
}

/// True when a line that *starts* with this token continues the previous line,
/// so no terminator is inserted before it (S6-R continuation suppression): a
/// leading `.` (S69 method/field chain) or a binary/logical operator.
fn suppresses_terminator(kind: &TokKind) -> bool {
    matches!(
        kind,
        TokKind::Dot
            | TokKind::QuestionDot
            | TokKind::AndAnd
            | TokKind::OrOr
            | TokKind::Plus
            | TokKind::Minus
            | TokKind::Star
            | TokKind::Slash
            | TokKind::Percent
            | TokKind::EqEq
            | TokKind::NotEq
            | TokKind::Lt
            | TokKind::Gt
            | TokKind::Le
            | TokKind::Ge
            | TokKind::Amp
            | TokKind::Pipe
            | TokKind::Caret
            | TokKind::Shl
            | TokKind::Shr
            | TokKind::QuestionQuestion // S35/S71 fallback continues the expr
    )
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
                // E0986: `->` or `{` split onto the next line from a `)`.
                if matches!(cur.kind, TokKind::Arrow | TokKind::LBrace)
                    && matches!(prev.kind, TokKind::RParen)
                {
                    diags.push(Diagnostic::error(
                        "E0986",
                        format!(
                            "`{}` must stay on the same line as the closing `)`",
                            if matches!(cur.kind, TokKind::Arrow) { "->" } else { "{" }
                        ),
                        "a return type `-> Type` and an opening `{` follow the parameter list on its line (S44)".to_string(),
                        format!(
                            "move the `{}` up to the `)` line",
                            if matches!(cur.kind, TokKind::Arrow) { "->" } else { "{" }
                        ),
                        Some(cur.span),
                    ));
                    // Do not insert a terminator; let the parser keep going.
                } else if !suppresses_terminator(&cur.kind)
                    // A closing `)` / `]` on its own line never begins a
                    // statement, so a terminator before it is never grammatical
                    // (multi-line call args / list / map). Suppress it. A `}` is
                    // NOT suppressed: a block close legitimately ends a statement.
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
