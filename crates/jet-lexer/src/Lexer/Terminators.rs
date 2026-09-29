//! S6-R statement-terminator insertion: after raw lexing, a post-pass inserts
//! a synthetic `Semi` at each line end that follows a statement-ending token.

use crate::Diagnostics::{Diagnostic, Span};

use super::Scan::{lex_raw, lex_raw_config, lex_raw_generated};
use super::Tokens::{is_comment, TokKind, Token};
use std::cell::RefCell;
use std::sync::Arc;

/// Source-coupled transport for the staged compiler pass, not a public Jet ABI.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawTokenFact {
    pub kind: u16,
    pub span: Span,
    pub uppercase: bool,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminatorEvent {
    InsertSemi { before: usize, at: usize },
    SplitHeader { before: usize, at: usize },
}

#[doc(hidden)]
pub type TerminatorDriver =
    Arc<dyn Fn(&[u8], &[RawTokenFact]) -> Result<Vec<TerminatorEvent>, String> + Send + Sync>;

thread_local! {
    static DRIVER: RefCell<Option<TerminatorDriver>> = const { RefCell::new(None) };
}

struct DriverGuard(Option<TerminatorDriver>);

impl Drop for DriverGuard {
    fn drop(&mut self) {
        DRIVER.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

#[doc(hidden)]
pub fn terminator_driver() -> Option<TerminatorDriver> {
    DRIVER.with(|slot| slot.borrow().clone())
}

#[doc(hidden)]
pub fn with_terminator_driver<R>(driver: Option<TerminatorDriver>, work: impl FnOnce() -> R) -> R {
    let _restore = DriverGuard(DRIVER.with(|slot| slot.replace(driver)));
    work()
}

#[doc(hidden)]
pub const TERMINATOR_PASS_SOURCE: &str = concat!(
    include_str!("../../../../Compiler/JetLexer/Source/Lexer/Scan.jet"),
    "\n",
    include_str!("../../../../Compiler/JetLexer/Source/Lexer/Terminators.jet"),
    "\n",
    include_str!("../../../../Compiler/JetLexer/Source/Lexer/Tokens.jet"),
    "\n",
    include_str!("../../../../Compiler/JetLexer/Source/Lexer/Payload.jet"),
);

/// Every raw kind has a distinct tag. No statement/continuation decision is
/// encoded here, and adding a token kind must update this exhaustive match.
#[doc(hidden)]
pub fn raw_token_fact(token: &Token) -> RawTokenFact {
    let kind = match &token.kind {
        TokKind::Ident(_) => 1,
        TokKind::RawStr(_) => 2,
        TokKind::Str(_) => 3,
        TokKind::Int(..) => 4,
        TokKind::Float(..) => 5,
        TokKind::UnitNumber { .. } => 6,
        TokKind::Char(_) => 7,
        TokKind::KwTrue => 8,
        TokKind::KwFalse => 9,
        TokKind::KwSelf => 10,
        TokKind::KwNull => 11,
        TokKind::KwBreak => 12,
        TokKind::KwReturn => 13,
        TokKind::RParen => 14,
        TokKind::RBracket => 15,
        TokKind::RBrace => 16,
        TokKind::Question => 17,
        TokKind::PlusPlus => 18,
        TokKind::MinusMinus => 19,
        TokKind::Gt => 20,
        TokKind::Shr => 21,
        TokKind::FenceClose => 22,
        TokKind::Dot => 23,
        TokKind::QuestionDot => 24,
        TokKind::AndAnd => 25,
        TokKind::OrOr => 26,
        TokKind::Plus => 27,
        TokKind::Minus => 28,
        TokKind::Star => 29,
        TokKind::Slash => 30,
        TokKind::SlashPercent => 31,
        TokKind::Percent => 32,
        TokKind::PercentPercent => 33,
        TokKind::EqEq => 34,
        TokKind::NotEq => 35,
        TokKind::Lt => 36,
        TokKind::Le => 37,
        TokKind::Ge => 38,
        TokKind::Compare => 39,
        TokKind::Amp => 40,
        TokKind::Pipe => 41,
        TokKind::Caret => 42,
        TokKind::TildePipe => 43,
        TokKind::Shl => 44,
        TokKind::QuestionQuestion => 45,
        TokKind::LBrace => 46,
        TokKind::LParen => 47,
        TokKind::LBracket => 48,
        TokKind::Eof => 49,
        TokKind::Bang => 50,
        TokKind::UnifiedArrow => 51,
        TokKind::LambdaArrow => 53,
        TokKind::Eq => 54,
        TokKind::LineComment(_) => 55,
        TokKind::BlockComment(_) => 56,
        TokKind::KwFn => 57,
        TokKind::KwPub => 58,
        TokKind::KwPriv => 59,
        TokKind::KwIf => 60,
        TokKind::KwElse => 61,
        TokKind::KwIn => 62,
        TokKind::KwMutate => 63,
        TokKind::KwMove => 64,
        TokKind::KwCopy => 65,
        TokKind::KwStruct => 66,
        TokKind::KwEnum => 67,
        TokKind::KwImpl => 68,
        TokKind::KwTrait => 69,
        TokKind::KwTag => 70,
        TokKind::KwEffect => 71,
        TokKind::KwDerive => 72,
        TokKind::KwIt => 73,
        TokKind::KwConst => 74,
        TokKind::KwComptime => 75,
        TokKind::KwLoop => 76,
        TokKind::KwYield => 77,
        TokKind::KwUse => 78,
        TokKind::KwExtern => 79,
        TokKind::KwModule => 80,
        TokKind::FenceOpen => 81,
        TokKind::Colon => 82,
        TokKind::ColonColon => 83,
        TokKind::ColonEq => 84,
        TokKind::Comma => 85,
        TokKind::Semi => 86,
        TokKind::DotDot => 87,
        TokKind::DotDotLt => 88,
        TokKind::DotDotDot => 89,
        TokKind::At => 90,
        TokKind::Tilde => 91,
        TokKind::TildePipeEq => 92,
        TokKind::TildeTilde => 93,
        TokKind::PlusEq => 94,
        TokKind::MinusEq => 95,
        TokKind::StarEq => 96,
        TokKind::SlashEq => 97,
        TokKind::SlashPercentEq => 98,
        TokKind::PercentEq => 99,
        TokKind::PercentPercentEq => 100,
        TokKind::AmpEq => 101,
        TokKind::PipeEq => 102,
        TokKind::CaretEq => 103,
        TokKind::ShlEq => 104,
        TokKind::ShrEq => 105,
        TokKind::Hash => 106,
        TokKind::Dollar => 107,
    };
    RawTokenFact {
        kind,
        span: token.span,
        uppercase: matches!(&token.kind, TokKind::Ident(name)
            if name.chars().next().is_some_and(char::is_uppercase)),
    }
}

fn split_header_spelling(kind: &TokKind) -> Option<&'static str> {
    Some(match kind {
        TokKind::UnifiedArrow => "->",
        TokKind::LambdaArrow => "=>",
        TokKind::Eq | TokKind::MinusMinus => "-[…]>",
        TokKind::LBrace => "{",
        _ => return None,
    })
}

fn split_header_diagnostic(token: &Token) -> Option<Diagnostic> {
    let spelling = split_header_spelling(&token.kind)?;
    Some(Diagnostic::error(
        "E0986",
        format!("`{spelling}` must stay on the same line as the closing `)`"),
        "an arrow, effect row, or opening block continues the header on its line".to_string(),
        format!("move `{spelling}` up to the `)` line"),
        Some(token.span),
    ))
}

fn apply_events(
    src: &str,
    toks: &mut Vec<Token>,
    diags: &mut Vec<Diagnostic>,
    events: Vec<TerminatorEvent>,
) -> Result<(), String> {
    let mut previous = None;
    let mut insertions = 0;
    for event in &events {
        let (before, at) = match *event {
            TerminatorEvent::InsertSemi { before, at }
            | TerminatorEvent::SplitHeader { before, at } => (before, at),
        };
        let token = toks.get(before).ok_or("terminator event has an invalid raw-token index")?;
        if previous.is_some_and(|previous| before <= previous)
            || at > token.span.start
            || !src.is_char_boundary(at)
        {
            return Err("terminator event order or byte position is invalid".to_string());
        }
        if matches!(event, TerminatorEvent::SplitHeader { .. })
            && (at != token.span.start || split_header_spelling(&token.kind).is_none())
        {
            return Err("split-header event does not identify a diagnostic-bearing token".to_string());
        }
        insertions += usize::from(matches!(event, TerminatorEvent::InsertSemi { .. }));
        previous = Some(before);
    }
    if insertions == 0 {
        for event in events {
            if let TerminatorEvent::SplitHeader { before, .. } = event {
                diags.push(split_header_diagnostic(&toks[before]).expect("validated diagnostic"));
            }
        }
        return Ok(());
    }
    let mut output = Vec::with_capacity(toks.len() + insertions);
    let mut events = events.into_iter().peekable();
    for (index, token) in std::mem::take(toks).into_iter().enumerate() {
        if let Some(event) = events.peek() {
            let before = match event {
                TerminatorEvent::InsertSemi { before, .. }
                | TerminatorEvent::SplitHeader { before, .. } => *before,
            };
            if before == index {
                match events.next().expect("peeked event") {
                    TerminatorEvent::InsertSemi { at, .. } => output.push(Token {
                        kind: TokKind::Semi,
                        span: Span::new(at, at),
                    }),
                    TerminatorEvent::SplitHeader { .. } => {
                        diags.push(split_header_diagnostic(&token).expect("validated diagnostic"));
                    }
                }
            }
        }
        output.push(token);
    }
    *toks = output;
    Ok(())
}

fn insert_terminators(src: &str, toks: &mut Vec<Token>, diags: &mut Vec<Diagnostic>) {
    let Some(driver) = terminator_driver() else {
        insert_terminators_reference(src, toks, diags);
        return;
    };
    let facts = toks.iter().map(raw_token_fact).collect::<Vec<_>>();
    let events = driver(src.as_bytes(), &facts)
        .unwrap_or_else(|error| jet_foundation::ice!(None, "Jet terminator pass failed: {error}"));
    apply_events(src, toks, diags, events)
        .unwrap_or_else(|error| jet_foundation::ice!(None, "Jet terminator event ABI failed: {error}"));
}

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
            | TokKind::RawStr(_)
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
            | TokKind::Bang          // D-TYPE-SUFFIX1: contract `SaveError!` at line end
            | TokKind::PlusPlus      // retired D-INCR1: `x++` ends a statement so E0160
            | TokKind::MinusMinus    // can offer its Safe `x += 1` / `x -= 1` edit
            | TokKind::Gt            // generic type close `[Int]` at line end
            | TokKind::Shr // nested generic close `Map<K, List<V>>`
            | TokKind::FenceClose // D-FENCE2: `total += <: 10, 20 :>` at line end
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

fn skip_comment_tokens(toks: &[Token], mut index: usize) -> usize {
    while toks
        .get(index)
        .is_some_and(|token| is_comment(&token.kind))
    {
        index += 1;
    }
    index
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
    let mut j = skip_comment_tokens(toks, i + 1);
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
        j = skip_comment_tokens(toks, j);
        return matches!(
            toks.get(j).map(|t| &t.kind),
            Some(TokKind::UnifiedArrow | TokKind::LambdaArrow)
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
    // D-PATO: each structural or-pattern alternative has the same leading-dot
    // variant/path/payload shape. Keep consuming alternatives before looking
    // for a guard or arrow; otherwise a following arm's leading `.` is
    // mistaken for a fluent chain continuation.
    loop {
        j = skip_comment_tokens(toks, j);
        loop {
            if !matches!(toks.get(j).map(|t| &t.kind), Some(TokKind::Dot)) {
                break;
            }
            let variant = skip_comment_tokens(toks, j + 1);
            if !toks
                .get(variant)
                .map(|t| leading_dot_variant_token(&t.kind))
                .unwrap_or(false)
            {
                break;
            }
            j = variant + 1;
            j = skip_comment_tokens(toks, j);
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
        j = skip_comment_tokens(toks, j);
        if !matches!(toks.get(j).map(|t| &t.kind), Some(TokKind::Pipe)) {
            break;
        }
        j += 1;
        j = skip_comment_tokens(toks, j);
        if !matches!(toks.get(j).map(|t| &t.kind), Some(TokKind::Dot)) {
            return false;
        }
        j = skip_comment_tokens(toks, j + 1);
        if !toks
            .get(j)
            .map(|t| leading_dot_variant_token(&t.kind))
            .unwrap_or(false)
        {
            return false;
        }
        j += 1;
    }
    j = skip_comment_tokens(toks, j);
    // D-IFDIST1: a braceless arm may add a Boolean guard after its complete
    // structural head (`.Key(key) | .Other(key) && key == "b" -> ...`).
    // Scan that guard only on its source line; an unrelated arrow on a later
    // line must not turn a fluent chain into a new arm.
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
                TokKind::UnifiedArrow | TokKind::LambdaArrow if depth == 0 => {
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
        Some(TokKind::UnifiedArrow | TokKind::LambdaArrow)
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

/// D-CAP-RECEIVER1=D: at line start, `&` or `^` written directly against a
/// name (`&buf.append(...)`, `^buf.seal()`, `&self.items.push(x)`) marks the
/// place a call writes or takes, so it begins a new statement. A continued
/// bitwise-and or power line keeps a space after its operator (`& mask`).
fn place_mark_starts_at(toks: &[Token], i: usize) -> bool {
    let (Some(mark), Some(name)) = (toks.get(i), toks.get(i + 1)) else {
        return false;
    };
    matches!(mark.kind, TokKind::Amp | TokKind::Caret)
        && matches!(name.kind, TokKind::Ident(_) | TokKind::KwSelf)
        && mark.span.end == name.span.start
}

/// S6-R post-pass: walk the code tokens (comments are trivia, skipped but kept
/// in the stream) and insert a synthetic `Semi` whenever a statement-ending
/// token is followed — across a line break — by a token that does not continue
/// the line. `->` and `{` never trigger insertion (they must stay on the
/// closing `)` line, S44); a split `-> Type` / `{` is E0986.
fn insert_terminators_reference(src: &str, toks: &mut Vec<Token>, diags: &mut Vec<Diagnostic>) {
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
                        | TokKind::LambdaArrow
                        | TokKind::Eq
                        | TokKind::MinusMinus
                        | TokKind::LBrace
                ) && matches!(prev.kind, TokKind::RParen)
                {
                    diags.push(split_header_diagnostic(cur).expect("checked split-header kind"));
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
                    || dispatch_arm_starts_at(src, toks, i)
                    // D-CAP-RECEIVER1=D: `&name` / `^name` marks a place.
                    || place_mark_starts_at(toks, i))
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
