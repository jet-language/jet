//! Raw scanning: `lex_raw` (no terminator insertion), the main `run` loop, and
//! number/char/digit scanning.

use super::Tokens::{TokKind, Token};
use super::{keyword, Lexer};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

fn describe_unrecognized_character(character: char) -> String {
    match character {
        '\0' => "\\0".to_string(),
        '\t' => "\\t".to_string(),
        character
            if character.is_control() || crate::Diagnostics::display_char_width(character) == 0 =>
        {
            format!("U+{:04X}", character as u32)
        }
        character => character.to_string(),
    }
}

/// D-FENCE2=A: a retired `@[` / `]@` fence digraph. The edit respells the one
/// digraph, so applying every edit in a file migrates each fence. A space
/// keeps an adjacent `:` or `<` from gluing onto the new digraph.
fn retired_fence_digraph(span: Span, previous: char, old: &str, new: &str) -> Diagnostic {
    let new_text = if matches!(previous, ':' | '<') {
        format!(" {new}")
    } else {
        new.to_string()
    };
    Diagnostic::from_row("E-FENCE-SPELLING", &[("old", old), ("new", new)], Some(span))
        .with_edit(crate::Diagnostics::TextEdit { span, new_text })
}

/// D-COMPILER-NS1=A / D-META-ROOT3=A / D-BUILD-FACT3=A / D-DECL-META1=A:
/// compiler facts and declaration metadata moved from `@` to `$`. One retired
/// spelling, the text that replaces it, and E0003 with a machine-applicable
/// edit (`respelled` is `None` for a removed query with no mechanical
/// respelling).
fn retired_fact_mark(span: Span, old: &str, respelled: Option<&str>) -> Diagnostic {
    let why = "compiler facts and declaration metadata carry `$`; prefix `@` in code now marks a live link (D-COMPILER-NS1=A)".to_string();
    match respelled {
        Some(new) => Diagnostic::error(
            "E0003",
            format!("`{old}` is retired; write `{new}`"),
            why,
            format!("replace `{old}` with `{new}`"),
            Some(span),
        )
        .with_edit(crate::Diagnostics::TextEdit { span, new_text: new.to_string() }),
        None => Diagnostic::error(
            "E0003",
            format!("`{old}(…)` is retired"),
            format!("{why}; a fact is a `$` member of the thing it describes (D-META-ROOT3=A)"),
            "read the fact on its subject, such as `T.$fields`, `f.$effects` or `value.$origin`"
                .to_string(),
            Some(span),
        ),
    }
}

/// D-NAME-SPLICE1=B: a template name splice carries the fact sigil,
/// `fn $method`, `impl $type_name`, `self.$field`, `.$left`. The retired `@`
/// splice teaches E0388 with a machine-applicable edit and reads as the `$`
/// word, so prefix `@` in code marks only a live link.
fn retired_name_splice(span: Span, old: &str, new: &str) -> Diagnostic {
    Diagnostic::from_row("E0388", &[("old", old), ("new", new)], Some(span)).with_edit(
        crate::Diagnostics::TextEdit {
            span,
            new_text: new.to_string(),
        },
    )
}

/// Declaration heads whose name may be a template splice (`fn $method`).
const SPLICED_DECLARATION_HEADS: &[&str] = &["fn", "impl", "struct", "enum", "trait", "type"];

/// Declaration metadata names written `$name: value` in a `marker` or `fact`
/// parameter list (D-DECL-META1=A).
const DECL_METADATA: &[&str] = &[
    "sites", "repeatable", "holds", "safe", "gates", "decision", "retired", "proved_by",
    "inherits", "companion", "scopes", "resolution", "owns_menu", "identity", "name",
];

/// Fact and reflection members written `subject.$member` (D-META-ROOT3=A)
/// that are not registered fact planes: template reflection members and the
/// retired `track_origin`, which the parser teaches separately. Any other
/// `@name` after `.` is a retired template name splice (D-NAME-SPLICE1=B).
const REFLECTED_MEMBERS: &[&str] = &[
    "name", "fields", "index", "kind", "ty", "base", "unknown", "track_origin", "profile",
];

/// Raw lex with no S6-R terminator insertion. Used for interpolation
/// sub-streams (`{expr}`), which are single expressions and need no terminator.
pub fn lex_raw(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_raw_with_policy_at_depth(src, false, false, 0)
}

pub(super) fn lex_raw_at_depth(
    src: &str,
    interpolation_depth: usize,
) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_raw_with_policy_at_depth(src, false, false, interpolation_depth)
}

pub(super) fn lex_raw_generated(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_raw_with_policy_at_depth(src, true, false, 0)
}

pub(super) fn lex_raw_generated_at_depth(
    src: &str,
    interpolation_depth: usize,
) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_raw_with_policy_at_depth(src, true, false, interpolation_depth)
}

pub(super) fn lex_raw_config(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_raw_with_policy_at_depth(src, false, true, 0)
}

fn lex_raw_with_policy_at_depth(
    src: &str,
    allow_reserved_identifiers: bool,
    config_surface: bool,
    interpolation_depth: usize,
) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer {
        chars: src.char_indices().collect(),
        end: src.len(),
        src,
        i: 0,
        diags: Vec::new(),
        allow_reserved_identifiers,
        config_surface,
        interpolation_depth,
    };
    // D-SCRIPT-ENTRY1=A: the operating system owns a byte-zero `#!/...` line.
    // Advance the real source cursor so every later token keeps its original
    // byte span and line number.
    if src.as_bytes().starts_with(b"#!/") {
        while lx.i < lx.chars.len() && lx.at(lx.i) != '\n' {
            lx.i += 1;
        }
        if lx.i < lx.chars.len() {
            lx.i += 1;
        }
    }
    let mut toks = lx.run();
    toks.push(Token {
        kind: TokKind::Eof,
        span: Span::new(src.len(), src.len()),
    });
    (toks, lx.diags)
}

impl<'a> Lexer<'a> {
    /// D-TIME-IN1=C: `in` stays a keyword everywhere except immediately after
    /// a postfix dot. Reclassifying only that position keeps the parser's
    /// source-loop diagnostic while making `value.in(...)` an ordinary member
    /// token for lexer clients and downstream parsers.
    fn after_postfix_dot(toks: &[Token]) -> bool {
        toks.iter()
            .rev()
            .find(|token| {
                !matches!(
                    &token.kind,
                    TokKind::LineComment(_) | TokKind::BlockComment(_)
                )
            })
            .is_some_and(|token| matches!(&token.kind, TokKind::Dot | TokKind::QuestionDot))
    }
    fn significant_lookbehind<const N: usize>(toks: &[Token]) -> [Option<&Token>; N] {
        let mut lookbehind: [Option<&Token>; N] = [None; N];
        let mut count = 0;
        for token in toks.iter().rev() {
            if matches!(
                &token.kind,
                TokKind::LineComment(_) | TokKind::BlockComment(_)
            ) {
                continue;
            }
            if count == N {
                break;
            }
            lookbehind[count] = Some(token);
            count += 1;
        }
        lookbehind
    }

    /// D-BOUND-RAW1=A: the quote immediately follows the opening brace of a
    /// typed head body. Only that body's literal substream changes the
    /// backslash rule; brace lexing and interpolation substreams remain
    /// ordinary Jet lexer input.
    ///
    /// D-LIT-DOT1=B spells that head `Regex{"\d+"}`, so the `.{` that used to
    /// mark it is gone and the head name itself is the signal. A PascalCase
    /// identifier before the brace is always a type, never a value: S54 /
    /// D-SHAPE-CASE1=C makes casing a machine-enforced law with zero
    /// exceptions, so a block subject like `if flag {` is snake_case and
    /// cannot reach this rule. The `Dot` arm reads the retired spelling and
    /// leaves with the rest of the migration shim.
    fn starts_typed_head_body(toks: &[Token]) -> bool {
        let [last, previous] = Self::significant_lookbehind::<2>(toks);
        matches!(
            last.map(|token| &token.kind),
            Some(TokKind::LBrace)
        ) && match previous.map(|token| &token.kind) {
            Some(TokKind::Dot) => true,
            Some(TokKind::Ident(name)) => {
                name.starts_with(|first: char| first.is_ascii_uppercase())
            }
            _ => false,
        }
    }

    /// D-BYTELIT1=B: `[U8]{ "..." }` and `[U8#N]{ "..." }` use the same
    /// quoted token as ordinary text, but their body grammar owns `\xNN`.
    fn starts_byte_typed_lit_body(toks: &[Token]) -> bool {
        let lookbehind = Self::significant_lookbehind::<6>(toks);
        if !matches!(
            lookbehind[0].map(|token| &token.kind),
            Some(TokKind::LBrace)
        ) || !matches!(
            lookbehind[1].map(|token| &token.kind),
            Some(TokKind::RBracket)
        ) {
            return false;
        }
        let is_u8 = |token: Option<&Token>| {
            matches!(
                token,
                Some(token)
                    if matches!(&token.kind, TokKind::Ident(name) if name == Syntax::TYPE_U8)
            )
        };
        if matches!(
            lookbehind[3].map(|token| &token.kind),
            Some(TokKind::LBracket)
        ) && is_u8(lookbehind[2])
        {
            return true;
        }
        matches!(
            lookbehind[5].map(|token| &token.kind),
            Some(TokKind::LBracket)
        ) && is_u8(lookbehind[4])
            && matches!(
                lookbehind[3].map(|token| &token.kind),
                Some(TokKind::Hash)
            )
    }

    fn starts_inline_foreign_body(toks: &[Token]) -> bool {
        if !matches!(toks.last().map(|t| &t.kind), Some(TokKind::LBrace)) {
            return false;
        }
        let Some(fn_pos) = toks.iter().rposition(|t| matches!(t.kind, TokKind::KwFn)) else {
            return false;
        };
        let significant = toks[..fn_pos]
            .iter()
            .filter(|token| {
                !matches!(
                    token.kind,
                    TokKind::LineComment(_)
                        | TokKind::BlockComment(_)
                        | TokKind::KwPub
                        | TokKind::Semi
                )
            })
            .collect::<Vec<_>>();
        let is_marker_name = |token: &&Token| matches!(token.kind, TokKind::Ident(_));
        let is_ffi = |token: &&Token| matches!(&token.kind, TokKind::Ident(name) if jet_foundation::Registry::is_inline_foreign_marker(name));
        let mut cursor = significant.len();
        let mut saw_ffi = false;
        loop {
            if cursor >= 2
                && matches!(significant[cursor - 2].kind, TokKind::Hash)
                && is_marker_name(&significant[cursor - 1])
            {
                saw_ffi |= is_ffi(&significant[cursor - 1]);
                cursor -= 2;
                continue;
            }
            if cursor > 0 && matches!(significant[cursor - 1].kind, TokKind::RParen) {
                let mut depth = 0usize;
                let mut open = None;
                for index in (0..cursor).rev() {
                    match significant[index].kind {
                        TokKind::RParen => depth += 1,
                        TokKind::LParen => {
                            depth -= 1;
                            if depth == 0 {
                                open = Some(index);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(open) = open {
                    if open >= 2
                        && matches!(significant[open - 2].kind, TokKind::Hash)
                        && is_marker_name(&significant[open - 1])
                    {
                        saw_ffi |= is_ffi(&significant[open - 1]);
                        cursor = open - 2;
                        continue;
                    }
                }
            }
            if cursor > 0 && matches!(significant[cursor - 1].kind, TokKind::RBracket) {
                let mut depth = 0usize;
                let mut open = None;
                for index in (0..cursor).rev() {
                    match significant[index].kind {
                        TokKind::RBracket => depth += 1,
                        TokKind::LBracket => {
                            depth -= 1;
                            if depth == 0 {
                                open = Some(index);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(open) = open {
                    if open > 0 && matches!(significant[open - 1].kind, TokKind::Hash) {
                        let mut paren_depth = 0usize;
                        let mut bracket_depth = 0usize;
                        let mut expects_name = true;
                        for token in &significant[open + 1..cursor - 1] {
                            if expects_name && paren_depth == 0 && bracket_depth == 0 {
                                saw_ffi |= is_ffi(token);
                                expects_name = false;
                            }
                            match token.kind {
                                TokKind::LParen => paren_depth += 1,
                                TokKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                                TokKind::LBracket => bracket_depth += 1,
                                TokKind::RBracket => {
                                    bracket_depth = bracket_depth.saturating_sub(1);
                                }
                                TokKind::Comma if paren_depth == 0 && bracket_depth == 0 => {
                                    expects_name = true;
                                }
                                _ => {}
                            }
                        }
                        cursor = open - 1;
                        continue;
                    }
                }
            }
            break;
        }
        saw_ffi
    }

    pub(super) fn at(&self, i: usize) -> char {
        if i < self.chars.len() {
            self.chars[i].1
        } else {
            '\0'
        }
    }

    pub(super) fn pos(&self, i: usize) -> usize {
        if i < self.chars.len() {
            self.chars[i].0
        } else {
            self.end
        }
    }

    fn run(&mut self) -> Vec<Token> {
        let mut toks = Vec::new();
        while self.i < self.chars.len() {
            let c = self.at(self.i);

            if c.is_whitespace() {
                self.i += 1;
                continue;
            }

            // D-SCRIPT-ENTRY1=A: only a `#!/...` line is launch metadata. The
            // existing `#!Name` type-marker form remains Jet syntax.
            if c == '#' && self.at(self.i + 1) == '!' && self.at(self.i + 2) == '/' {
                let start = self.pos(self.i);
                self.diags.push(Diagnostic::from_row(
                    "E0042",
                    &[],
                    Some(Span::new(start, self.pos(self.i + 2))),
                ));
                while self.i < self.chars.len() && self.at(self.i) != '\n' {
                    self.i += 1;
                }
                continue;
            }

            // Line comments (decision S5) — retained for fmt (M6/S44).
            if c == '/'
                && self.at(self.i + 1) == '/'
                && !(self.config_surface
                    && self.pos(self.i) > 0
                    && self.src.as_bytes().get(self.pos(self.i) - 1) == Some(&b':'))
            {
                let comment_start = self.pos(self.i);
                while self.i < self.chars.len() && self.at(self.i) != '\n' {
                    self.i += 1;
                }
                let text = self.src[comment_start..self.pos(self.i)].to_string();
                toks.push(Token {
                    kind: TokKind::LineComment(text),
                    span: Span::new(comment_start, self.pos(self.i)),
                });
                continue;
            }

            // Config templates may use an unquoted URL such as
            // `postgres://{USER}@{HOST}`. In that one config lexer mode, the
            // adjacent slashes after a scheme colon are URL punctuation, not
            // the ordinary line-comment opener.
            if c == '/' && self.at(self.i + 1) == '/' && self.config_surface {
                let start = self.pos(self.i);
                let middle = self.pos(self.i + 1);
                let end = self.pos(self.i + 2);
                toks.push(Token {
                    kind: TokKind::Slash,
                    span: Span::new(start, middle),
                });
                toks.push(Token {
                    kind: TokKind::Slash,
                    span: Span::new(middle, end),
                });
                self.i += 2;
                continue;
            }

            // Block comments (decision S5) — nest, so a region containing other
            // comments can always be commented out. Retained for fmt (M6/S44).
            if c == '/' && self.at(self.i + 1) == '*' {
                let comment_start = self.pos(self.i);
                self.i += 2;
                let mut depth = 1usize;
                while self.i < self.chars.len() && depth > 0 {
                    if self.at(self.i) == '/' && self.at(self.i + 1) == '*' {
                        depth += 1;
                        self.i += 2;
                    } else if self.at(self.i) == '*' && self.at(self.i + 1) == '/' {
                        depth -= 1;
                        self.i += 2;
                    } else {
                        self.i += 1;
                    }
                }
                let end = self.pos(self.i);
                if depth > 0 {
                    self.diags.push(Diagnostic::error(
                        "E0002",
                        "this `/*` comment never gets a closing `*/`".to_string(),
                        "a block comment starts with `/*` and runs until a matching `*/`"
                            .to_string(),
                        "add a `*/` to close the comment".to_string(),
                        Some(Span::new(comment_start, end)),
                    ));
                }
                let text = self.src[comment_start..end].to_string();
                toks.push(Token {
                    kind: TokKind::BlockComment(text),
                    span: Span::new(comment_start, end),
                });
                continue;
            }

            let start = self.pos(self.i);
            let simple = |lx: &mut Self, kind: TokKind, len: usize| {
                let tok = Token {
                    kind,
                    span: Span::new(start, lx.pos(lx.i + len)),
                };
                lx.i += len;
                tok
            };

            let next = self.at(self.i + 1);
            let next2 = self.at(self.i + 2);
            match c {
                '(' => toks.push(simple(self, TokKind::LParen, 1)),
                ')' => toks.push(simple(self, TokKind::RParen, 1)),
                '{' => toks.push(simple(self, TokKind::LBrace, 1)),
                '}' => toks.push(simple(self, TokKind::RBrace, 1)),
                '[' => toks.push(simple(self, TokKind::LBracket, 1)),
                // D-FENCE2=A: the retired `]@` fence close teaches `:>` and
                // still closes the fence so parsing recovers.
                ']' if next == '@' => {
                    self.diags.push(retired_fence_digraph(
                        Span::new(start, self.pos(self.i + 2)),
                        self.at(self.i.saturating_sub(1)),
                        Syntax::RETIRED_FENCE_CLOSE,
                        Syntax::SIGIL_FENCE_CLOSE,
                    ));
                    toks.push(simple(self, TokKind::FenceClose, 2))
                }
                ']' => toks.push(simple(self, TokKind::RBracket, 1)),
                // D-FENCE2=A: `:>` closes a statement-expansion fence
                // (longest match before `::`, `:=`, and `:`). D-BIND4: `:=`
                // mutable binding sigil.
                ':' if next == '>' => toks.push(simple(self, TokKind::FenceClose, 2)),
                ':' if next == ':' => toks.push(simple(self, TokKind::ColonColon, 2)),
                ':' if next == '=' => toks.push(simple(self, TokKind::ColonEq, 2)),
                ':' => toks.push(simple(self, TokKind::Colon, 1)),
                ',' => toks.push(simple(self, TokKind::Comma, 1)),
                ';' => toks.push(simple(self, TokKind::Semi, 1)),
                // D-ONCE-AT1=D: prefix `@name` is one marked identifier, but
                // an adjacent `name@source` remains the package-source ref.
                // D-FENCE2=A: the retired `@[` fence open teaches `<:`.
                '@' if next == '[' => {
                    self.diags.push(retired_fence_digraph(
                        Span::new(start, self.pos(self.i + 2)),
                        self.at(self.i.saturating_sub(1)),
                        Syntax::RETIRED_FENCE_OPEN,
                        Syntax::SIGIL_FENCE_OPEN,
                    ));
                    toks.push(simple(self, TokKind::FenceOpen, 2))
                }
                '@' if (next.is_alphabetic() || next == '_')
                    && !self.at(self.i.saturating_sub(1)).is_alphanumeric()
                    && self.at(self.i.saturating_sub(1)) != '_' =>
                {
                    let mut j = self.i + 1;
                    let mut name = String::from("@");
                    while j < self.chars.len() {
                        let ch = self.at(j);
                        if ch.is_alphanumeric() || ch == '_' {
                            name.push(ch);
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if keyword(&name[1..]).is_some() {
                        toks.push(simple(self, TokKind::At, 1));
                    } else {
                        let rest = name[1..].to_string();
                        let prev = self.at(self.i.saturating_sub(1));
                        let prev2 = self.at(self.i.saturating_sub(2));
                        let member = self.i > 0
                            && prev == '.'
                            && prev2 != '.'
                            && (REFLECTED_MEMBERS.contains(&rest.as_str())
                                || Syntax::fact_read_kind(&format!(
                                    "{}{rest}",
                                    Syntax::COMPTIME_MARK
                                ))
                                .is_some());
                        let mut colon = j;
                        while self.at(colon) == ' ' {
                            colon += 1;
                        }
                        let metadata = DECL_METADATA.contains(&rest.as_str())
                            && self.at(colon) == ':'
                            && self.at(colon + 1) != ':';
                        let word_ends = |at: char| !(at.is_alphanumeric() || at == '_');
                        let package_member = rest == "build"
                            && ".package".chars().enumerate().all(|(k, ch)| self.at(j + k) == ch)
                            && word_ends(self.at(j + 8));
                        let empty_call = self.at(j) == '(' && self.at(j + 1) == ')';
                        // D-NAME-SPLICE1=B: `@name` after `.` that is not a
                        // fact (`self.@field`, `.@left`), or naming a
                        // declaration (`fn @method`, `impl @type_name`), is
                        // a retired template name splice.
                        let mut head_end = self.i;
                        while head_end > 0 && self.at(head_end - 1) == ' ' {
                            head_end -= 1;
                        }
                        let mut head_start = head_end;
                        while head_start > 0 && !word_ends(self.at(head_start - 1)) {
                            head_start -= 1;
                        }
                        let head: String = (head_start..head_end).map(|k| self.at(k)).collect();
                        let splice = !member
                            && ((self.i > 0 && prev == '.' && prev2 != '.')
                                || (head_end < self.i
                                    && SPLICED_DECLARATION_HEADS.contains(&head.as_str())));
                        // (end of the retired text, its respelling)
                        let respelled: Option<(usize, String)> =
                            if member || metadata || rest == "irreversible" {
                                Some((j, format!("{}{rest}", Syntax::COMPTIME_MARK)))
                            } else if splice {
                                None
                            } else {
                                match Syntax::retired_fact_spelling(&name) {
                                    Some(Syntax::RetiredFactSpelling::Root(_)) if package_member => {
                                        Some((j + 8, Syntax::FACT_ROOT_PACKAGE.to_string()))
                                    }
                                    Some(Syntax::RetiredFactSpelling::Root(root))
                                        if rest != "build" && empty_call =>
                                    {
                                        Some((j + 2, root.to_string()))
                                    }
                                    Some(Syntax::RetiredFactSpelling::Root(root)) => {
                                        Some((j, root.to_string()))
                                    }
                                    Some(Syntax::RetiredFactSpelling::SubjectMember) => {
                                        self.diags.push(retired_fact_mark(
                                            Span::new(start, self.pos(j)),
                                            &name,
                                            None,
                                        ));
                                        None
                                    }
                                    None => None,
                                }
                            };
                        let (end, text) = match respelled {
                            Some((end, new)) => {
                                let span = Span::new(start, self.pos(end));
                                let old: String = (self.i..end).map(|k| self.at(k)).collect();
                                self.diags.push(retired_fact_mark(span, &old, Some(&new)));
                                (end, new)
                            }
                            None if splice => {
                                let new = format!("{}{rest}", Syntax::COMPTIME_MARK);
                                self.diags.push(retired_name_splice(
                                    Span::new(start, self.pos(j)),
                                    &name,
                                    &new,
                                ));
                                (j, new)
                            }
                            None => (j, name),
                        };
                        self.i = end;
                        let span = Span::new(start, self.pos(self.i));
                        toks.push(Token {
                            kind: TokKind::Ident(text),
                            span,
                        });
                    }
                }
                '@' => toks.push(simple(self, TokKind::At, 1)),
                '#' => toks.push(simple(self, TokKind::Hash, 1)),
                // D-COMPILER-NS1=A: prefix `$name` is one compiler-fact
                // identifier (`$build`, `T.$layout`, `$sites:`). Config
                // surfaces read the same token as a `$NAME` environment read
                // (D-ONCE-DOLLAR1=B). A lone `$` stays its own token.
                '$' if (next.is_alphabetic() || next == '_')
                    && !self.at(self.i.saturating_sub(1)).is_alphanumeric()
                    && self.at(self.i.saturating_sub(1)) != '_' =>
                {
                    let mut j = self.i + 1;
                    let mut name = String::from(Syntax::COMPTIME_MARK);
                    while j < self.chars.len() {
                        let ch = self.at(j);
                        if ch.is_alphanumeric() || ch == '_' {
                            name.push(ch);
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    self.i = j;
                    let span = Span::new(start, self.pos(self.i));
                    toks.push(Token {
                        kind: TokKind::Ident(name),
                        span,
                    });
                }
                '$' => toks.push(simple(self, TokKind::Dollar, 1)),
                '?' if next == '?' => toks.push(simple(self, TokKind::QuestionQuestion, 2)),
                '?' if next == '.' => toks.push(simple(self, TokKind::QuestionDot, 2)),
                '?' => toks.push(simple(self, TokKind::Question, 1)),
                '.' if next == '.' && next2 == '.' => {
                    toks.push(simple(self, TokKind::DotDotDot, 3))
                }
                // D-RANGE-EXCL1=C: `..<` before plain `..` (longest match).
                '.' if next == '.' && next2 == '<' => toks.push(simple(self, TokKind::DotDotLt, 3)),
                '.' if next == '.' => toks.push(simple(self, TokKind::DotDot, 2)),
                '.' => toks.push(simple(self, TokKind::Dot, 1)),
                '=' if next == '=' => toks.push(simple(self, TokKind::EqEq, 2)),
                '=' if next == '>' => toks.push(simple(self, TokKind::LambdaArrow, 2)),
                '=' => toks.push(simple(self, TokKind::Eq, 1)),
                '!' if next == '=' => toks.push(simple(self, TokKind::NotEq, 2)),
                '!' => toks.push(simple(self, TokKind::Bang, 1)),
                '+' if next == '+' => toks.push(simple(self, TokKind::PlusPlus, 2)),
                '+' if next == '=' => toks.push(simple(self, TokKind::PlusEq, 2)),
                '+' => toks.push(simple(self, TokKind::Plus, 1)),
                // D-ARROW-RESPELL1=A: canonical callable/control arrow.
                '-' if next == '>' => toks.push(simple(self, TokKind::UnifiedArrow, 2)),
                '-' if next == '-' => toks.push(simple(self, TokKind::MinusMinus, 2)),
                '-' if next == '=' => toks.push(simple(self, TokKind::MinusEq, 2)),
                '-' => toks.push(simple(self, TokKind::Minus, 1)),
                '*' if next == '=' => toks.push(simple(self, TokKind::StarEq, 2)),
                '*' => toks.push(simple(self, TokKind::Star, 1)),
                // D-FLOORDIV1=A: `/%=` before `/%` before `/=` before `/`
                // (longest match). Comments are consumed earlier, so `//` and
                // `/*` never reach here.
                '/' if next == '%' && next2 == '=' => {
                    toks.push(simple(self, TokKind::SlashPercentEq, 3))
                }
                '/' if next == '%' => toks.push(simple(self, TokKind::SlashPercent, 2)),
                '/' if next == '=' => toks.push(simple(self, TokKind::SlashEq, 2)),
                '/' => toks.push(simple(self, TokKind::Slash, 1)),
                // D-MODSEM1=A: `%%=` before `%%` before `%=` before `%`
                // (longest match).
                '%' if next == '%' && next2 == '=' => {
                    toks.push(simple(self, TokKind::PercentPercentEq, 3))
                }
                '%' if next == '%' => toks.push(simple(self, TokKind::PercentPercent, 2)),
                '%' if next == '=' => toks.push(simple(self, TokKind::PercentEq, 2)),
                '%' => toks.push(simple(self, TokKind::Percent, 1)),
                '^' if next == '=' => toks.push(simple(self, TokKind::CaretEq, 2)),
                '^' => toks.push(simple(self, TokKind::Caret, 1)),
                // D-SHAPE-COPY1=A: `~` is the copy sigil. `~~` is longest-match
                // lexed first so the parser can still emit the retired
                // external-method connector diagnostic (E0325).
                '~' if next == '~' => toks.push(simple(self, TokKind::TildeTilde, 2)),
                // D-XORSPELL1=A: `~|=` before `~|` before `~` (longest match).
                '~' if next == '|' && next2 == '=' => {
                    toks.push(simple(self, TokKind::TildePipeEq, 3))
                }
                '~' if next == '|' => toks.push(simple(self, TokKind::TildePipe, 2)),
                '~' => toks.push(simple(self, TokKind::Tilde, 1)),
                '&' if next == '&' => toks.push(simple(self, TokKind::AndAnd, 2)),
                '&' if next == '=' => toks.push(simple(self, TokKind::AmpEq, 2)),
                '&' => toks.push(simple(self, TokKind::Amp, 1)),
                '|' if next == '|' => toks.push(simple(self, TokKind::OrOr, 2)),
                '|' if next == '=' => toks.push(simple(self, TokKind::PipeEq, 2)),
                '|' => toks.push(simple(self, TokKind::Pipe, 1)),
                '<' if next == '<' && next2 == '=' => toks.push(simple(self, TokKind::ShlEq, 3)),
                '<' if next == '<' => toks.push(simple(self, TokKind::Shl, 2)),
                // D-CMP3WAY1=B: `<=>` must win over `<=` (longest match).
                '<' if next == '=' && next2 == '>' => toks.push(simple(self, TokKind::Compare, 3)),
                '<' if next == '=' => toks.push(simple(self, TokKind::Le, 2)),
                // D-FENCE2=A: `<:` opens a statement-expansion fence.
                '<' if next == ':' => toks.push(simple(self, TokKind::FenceOpen, 2)),
                '<' => toks.push(simple(self, TokKind::Lt, 1)),
                '>' if next == '>' && next2 == '=' => toks.push(simple(self, TokKind::ShrEq, 3)),
                '>' if next == '>' => toks.push(simple(self, TokKind::Shr, 2)),
                '>' if next == '=' => toks.push(simple(self, TokKind::Ge, 2)),
                '>' => toks.push(simple(self, TokKind::Gt, 1)),
                '"' => {
                    let raw_head = Self::starts_typed_head_body(&toks);
                    let byte_head = Self::starts_byte_typed_lit_body(&toks);
                    let tok = if next == '"' && next2 == '"' {
                        if Self::starts_inline_foreign_body(&toks) {
                            self.raw_foreign_string(start)
                        } else {
                            self.triple_string(start, raw_head, byte_head)
                        }
                    } else {
                        self.string(start, raw_head, byte_head)
                    };
                    if let Some(tok) = tok {
                        toks.push(tok);
                    }
                }
                '`' => {
                    if let Some(tok) = self.raw_string(start) {
                        toks.push(tok);
                    }
                }
                '\'' => {
                    if let Some(tok) = self.char_lit(start) {
                        toks.push(tok);
                    }
                }
                c if c.is_ascii_digit() => toks.push(self.number(start)),
                c if c.is_alphabetic() || c == '_' => {
                    let mut name = String::new();
                    while self.i < self.chars.len() {
                        let ch = self.at(self.i);
                        if ch.is_alphanumeric() || ch == '_' {
                            name.push(ch);
                            self.i += 1;
                        } else {
                            break;
                        }
                    }
                    let span = Span::new(start, self.pos(self.i));
                    if matches!(name.as_str(), "r" | "raw") && self.at(self.i) == '"' {
                        self.diags.push(Diagnostic::error(
                            "E0003",
                            format!("`{name}\"…\"` is not a Jet raw-string prefix"),
                            "backtick fences are the one raw ordinary-String spelling; `r` and `raw` are ordinary names"
                                .to_string(),
                            "write the text as ``…`` instead".to_string(),
                            Some(span),
                        ));
                    }
                    if !self.allow_reserved_identifiers
                        && Syntax::classify_identifier(&name) == Syntax::IdentifierClass::Reserved
                    {
                        self.diags.push(Diagnostic::error(
                            "E0067",
                            format!("`{name}` is reserved for Jet"),
                            "double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools"
                                .to_string(),
                            "rename it without the second leading underscore".to_string(),
                            Some(span),
                        ));
                    }
                    let kind = if name == Syntax::KW_IN && Self::after_postfix_dot(&toks) {
                        TokKind::Ident(name)
                    } else {
                        keyword(&name).unwrap_or(TokKind::Ident(name))
                    };
                    toks.push(Token { kind, span });
                }
                other => {
                    self.diags.push(Diagnostic::error(
                        "E0001",
                        format!(
                            "the character `{}` doesn't mean anything here (yet)",
                            describe_unrecognized_character(other)
                        ),
                        "unprintable characters are shown as `\\0`, `\\t`, or `U+XXXX` so you can identify them"
                            .to_string(),
                        "remove it, or use supported syntax".to_string(),
                        Some(Span::new(start, self.pos(self.i + 1))),
                    ));
                    self.i += 1; // skip it and keep lexing (error recovery)
                }
            }
        }
        toks
    }

    /// Lex digits, with an optional decimal part (S11 Float).
    /// S34: `_` digit separators (stripped), `0x`/`0o`/`0b` base prefixes, and
    /// a `e`/`E` exponent on floats. `1..10` stays Int DotDot Int: a `.` only
    /// starts the decimal part when a digit follows it.
    fn number(&mut self, start: usize) -> Token {
        // Base-prefixed integers: 0x / 0o / 0b (S34).
        if self.at(self.i) == '0' {
            let radix = match self.at(self.i + 1) {
                'x' | 'X' => Some(16u32),
                'o' | 'O' => Some(8),
                'b' | 'B' => Some(2),
                _ => None,
            };
            if let Some(radix) = radix {
                self.i += 2; // consume `0x` / `0o` / `0b`
                let mut digits = String::new();
                while self.i < self.chars.len() {
                    let ch = self.at(self.i);
                    if ch == Syntax::DIGIT_SEPARATOR {
                        self.i += 1;
                    } else if ch.to_digit(radix).is_some() {
                        digits.push(ch);
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                let span = Span::new(start, self.pos(self.i));
                if digits.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0001",
                        "this number prefix has no digits after it".to_string(),
                        "`0x`, `0o`, and `0b` must be followed by digits, e.g. `0xFF`, `0o17`, `0b1010`"
                            .to_string(),
                        "add digits after the base prefix".to_string(),
                        Some(span),
                    ));
                    return Token {
                        kind: TokKind::Int(0, self.src[span.start..span.end].to_string()),
                        span,
                    };
                }
                return match i64::from_str_radix(&digits, radix) {
                    Ok(n) => Token {
                        kind: TokKind::Int(n, self.src[span.start..span.end].to_string()),
                        span,
                    },
                    Err(_) => Token {
                        kind: TokKind::Int(0, self.src[span.start..span.end].to_string()),
                        span,
                    },
                };
            }
        }

        let mut text = String::new();
        self.lex_digits(&mut text);
        let mut is_float = false;
        if self.at(self.i) == '.' && self.at(self.i + 1).is_ascii_digit() {
            is_float = true;
            text.push('.');
            self.i += 1;
            self.lex_digits(&mut text);
        }
        // Exponent (S34): `e`/`E`, an optional sign, then digits — makes a Float.
        if matches!(self.at(self.i), 'e' | 'E') {
            let after = self.at(self.i + 1);
            let exp_ok = after.is_ascii_digit()
                || ((after == '+' || after == '-') && self.at(self.i + 2).is_ascii_digit());
            if exp_ok {
                is_float = true;
                text.push('e');
                self.i += 1;
                if matches!(self.at(self.i), '+' | '-') {
                    text.push(self.at(self.i));
                    self.i += 1;
                }
                self.lex_digits(&mut text);
            }
        }
        // D-UNITLIT1: a trailing identifier run right after the number is a
        // unit-suffix candidate — `500ms`, `12.50usd`. The `e`/`E`+digits
        // exponent form was already consumed above (`UNIT_SUFFIX_EXPONENT_RESERVED`),
        // so anything reaching here is never a float exponent; a bare `e`/`E`
        // with no following digit falls through and IS eligible as a suffix.
        let suffix = self.lex_unit_suffix();

        if is_float {
            // digits '.' digits (with optional exponent) always parses as f64.
            let v: f64 = text.parse().unwrap_or(0.0);
            if let Some(suffix) = suffix {
                let span = Span::new(start, self.pos(self.i));
                return Token {
                    kind: TokKind::UnitNumber {
                        raw: text,
                        int: None,
                        float: Some(v),
                        suffix,
                    },
                    span,
                };
            }
            let span = Span::new(start, self.pos(self.i));
            return Token {
                kind: TokKind::Float(v, text),
                span,
            };
        }
        match text.parse::<i64>() {
            Ok(n) => {
                if let Some(suffix) = suffix {
                    let span = Span::new(start, self.pos(self.i));
                    return Token {
                        kind: TokKind::UnitNumber {
                            raw: text,
                            int: Some(n),
                            float: None,
                            suffix,
                        },
                        span,
                    };
                }
                let span = Span::new(start, self.pos(self.i));
                Token {
                    kind: TokKind::Int(n, self.src[span.start..span.end].to_string()),
                    span,
                }
            }
            Err(_) => {
                let span = Span::new(start, self.pos(self.i));
                Token {
                    kind: TokKind::Int(0, self.src[span.start..span.end].to_string()),
                    span,
                }
            }
        }
    }

    /// D-UNITLIT1: greedily read a trailing identifier run (ASCII letter/`_`
    /// start, alphanumeric/`_` continue) right after a numeric literal, with
    /// no space between. Returns `None` when the next char doesn't start an
    /// identifier (the common case — a plain number).
    fn lex_unit_suffix(&mut self) -> Option<String> {
        let c = self.at(self.i);
        if !(c.is_ascii_alphabetic() || c == '_') {
            return None;
        }
        let mut suffix = String::new();
        while self.i < self.chars.len() {
            let ch = self.at(self.i);
            if ch.is_alphanumeric() || ch == '_' {
                suffix.push(ch);
                self.i += 1;
            } else {
                break;
            }
        }
        Some(suffix)
    }

    /// Consume a run of decimal digits, skipping `_` separators (S34).
    fn lex_digits(&mut self, text: &mut String) {
        while self.i < self.chars.len() {
            let ch = self.at(self.i);
            if ch.is_ascii_digit() {
                text.push(ch);
                self.i += 1;
            } else if ch == Syntax::DIGIT_SEPARATOR {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    /// S41: `'a'` or `'\n'` — exactly one Unicode scalar.
    fn char_lit(&mut self, start: usize) -> Option<Token> {
        self.i += 1; // opening quote
        if self.i >= self.chars.len() {
            return None;
        }
        let mut ch = self.at(self.i);
        if ch == '\\' {
            let esc = self.at(self.i + 1);
            if let Some(&(_, decoded)) = Syntax::ESCAPES.iter().find(|&&(e, _)| e == esc) {
                ch = decoded;
                self.i += 2;
            } else {
                self.diags.push(Diagnostic::error(
                    "E0001",
                    format!("`\\{}` isn't an escape Jet knows", esc),
                    "inside a character literal, `\\` starts an escape: `\\n`, `\\t`, `\\'`, `\\\\`"
                        .to_string(),
                    "use a supported escape or a plain character".to_string(),
                    Some(Span::new(self.pos(self.i), self.pos(self.i + 2))),
                ));
                self.i += 2;
                ch = '?';
            }
        } else {
            self.i += 1;
        }
        if self.at(self.i) != '\'' {
            self.diags.push(Diagnostic::error(
                "E0002",
                "a character literal must be exactly one character".to_string(),
                "write `'x'` with a single character between the quotes".to_string(),
                "use a String for longer text".to_string(),
                Some(Span::new(start, self.pos(self.i))),
            ));
            while self.i < self.chars.len() && self.at(self.i) != '\'' {
                self.i += 1;
            }
        }
        if self.at(self.i) == '\'' {
            self.i += 1;
        }
        Some(Token {
            kind: TokKind::Char(ch),
            span: Span::new(start, self.pos(self.i)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::lex_raw;
    use crate::Lexer::{StrTokPart, TokKind};

    #[test]
    fn raw_foreign_body_marker_does_not_capture_a_later_function() {
        let source = r#"#FFI(c) fn foreign() -> Int {
    """int64_t foreign(void) { return 1; }"""
}

fn ordinary(value: Int) {
    text :: """
value={value}
"""
}"#;
        let (tokens, diagnostics) = lex_raw(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let strings = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokKind::Str(parts) => Some(parts),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(strings.len(), 2);
        assert!(matches!(strings[0].as_slice(), [StrTokPart::Lit(_)]));
        assert!(
            strings[1]
                .iter()
                .any(|part| matches!(part, StrTokPart::Interp(_))),
            "{strings:?}"
        );
    }

    #[test]
    fn grouped_ffi_marker_keeps_foreign_body_raw() {
        let source = r#"#[Unsafe("scalar registers"), FFI(asm)]
fn add(a: Int, b: Int) -> Int {
    """add {a}, {b} ; -> return"""
}"#;
        let (tokens, diagnostics) = lex_raw(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let body = tokens.iter().find_map(|token| match &token.kind {
            TokKind::Str(parts)
                if matches!(parts.as_slice(), [StrTokPart::Lit(text)] if text.contains("-> return")) =>
            {
                Some(parts)
            }
            _ => None,
        });
        assert!(body.is_some(), "{tokens:?}");
    }

    #[test]
    fn adjacent_ffi_markers_keep_foreign_body_raw_in_both_orders() {
        for source in [
            "#Unsafe(\"registers\") #FFI(asm) fn add() { \"\"\"add {a}, {b}\"\"\" }",
            "#FFI(asm) #Unsafe(\"registers\") fn add() { \"\"\"add {a}, {b}\"\"\" }",
        ] {
            let (tokens, diagnostics) = lex_raw(source);
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            assert!(tokens.iter().any(|token| matches!(
                &token.kind,
                TokKind::Str(parts)
                    if matches!(parts.as_slice(), [StrTokPart::Lit(text)] if text == "add {a}, {b}")
            )), "{tokens:?}");
        }
    }

    #[test]
    fn grouped_ffi_argument_name_does_not_make_an_ordinary_body_raw() {
        let source =
            "#[Meta(category: FFI(asm)), Task] fn work() { text :: \"\"\"\nvalue={value}\n\"\"\" }";
        let (tokens, diagnostics) = lex_raw(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(
            tokens.iter().any(|token| matches!(
                &token.kind,
                TokKind::Str(parts)
                    if parts.iter().any(|part| matches!(part, StrTokPart::Interp(_)))
            )),
            "{tokens:?}"
        );
    }

    #[test]
    fn typed_head_bodies_keep_backslashes_and_shared_holes_but_plain_strings_decode_them() {
        let source = r#"plain :: "\n"
pattern :: Regex.{"\n{{literal}}-{value}"}
multiline :: Path.{"""
\logs\app
"""}"#;
        let (tokens, diagnostics) = lex_raw(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let strings = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokKind::Str(parts) => Some(parts),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(strings.len(), 3);
        assert!(matches!(
            strings[0].as_slice(),
            [StrTokPart::Lit(text)] if text == "\n"
        ));
        assert!(matches!(
            strings[1].as_slice(),
            [StrTokPart::Lit(text), StrTokPart::Interp(_)] if text == "\\n{literal}-"
        ));
        assert!(matches!(
            strings[2].as_slice(),
            [StrTokPart::Lit(text)] if text == "\\logs\\app"
        ));
    }

    #[test]
    fn typed_head_lookbehind_preserves_comments_incomplete_and_byte_heads() {
        let prefix_count = 128;
        let mut source = String::new();
        for index in 0..prefix_count {
            source.push_str(&format!("prefix{index} :: \"prefix\"; "));
        }
        source.push_str(
            r#"
ordinary :: "\n"
raw_dot :: Regex /* block */ . // line comment
{ /* block */ "\n" }
raw_name :: Regex /* block */ { // line comment
"\n" }
ordinary_lower :: regex /* block */ { /* block */ "\n" }
incomplete_raw :: Regex /* block */ . /* block */ "\n"
byte :: [ /* block */ U8 // line comment
] /* block */ { /* block */ "\x41" }
byte_fixed :: [ /* block */ U8 /* block */ # /* block */ 8 /* block */ ] /* block */ { /* block */ "\x42" }
"#,
        );
        let (tokens, diagnostics) = lex_raw(&source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let strings = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokKind::Str(parts) => Some(parts),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(strings.len(), prefix_count + 7);
        let tail = &strings[prefix_count..];
        assert!(matches!(
            tail[0].as_slice(),
            [StrTokPart::Lit(text)] if text == "\n"
        ));
        assert!(matches!(
            tail[1].as_slice(),
            [StrTokPart::Lit(text)] if text == "\\n"
        ));
        assert!(matches!(
            tail[2].as_slice(),
            [StrTokPart::Lit(text)] if text == "\\n"
        ));
        assert!(matches!(
            tail[3].as_slice(),
            [StrTokPart::Lit(text)] if text == "\n"
        ));
        assert!(matches!(
            tail[4].as_slice(),
            [StrTokPart::Lit(text)] if text == "\n"
        ));
        assert!(matches!(tail[5].as_slice(), [StrTokPart::Byte(0x41)]));
        assert!(matches!(tail[6].as_slice(), [StrTokPart::Byte(0x42)]));
    }

    #[test]
    fn at_distinguishes_prefix_package_ref_and_fence() {
        let (tokens, diagnostics) =
            lex_raw("foo@bar @baz T.@layout <:0, 1:> $build T.$layout a$b $[x]$");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let kinds = tokens
            .into_iter()
            .map(|token| token.kind)
            .collect::<Vec<_>>();
        assert!(matches!(kinds[0], TokKind::Ident(ref name) if name == "foo"));
        assert!(matches!(kinds[1], TokKind::At));
        assert!(matches!(kinds[2], TokKind::Ident(ref name) if name == "bar"));
        assert!(matches!(kinds[3], TokKind::Ident(ref name) if name == "@baz"));
        assert!(matches!(kinds[4], TokKind::Ident(ref name) if name == "T"));
        assert!(matches!(kinds[5], TokKind::Dot));
        assert!(matches!(kinds[6], TokKind::Ident(ref name) if name == "@layout"));
        assert!(matches!(kinds[7], TokKind::FenceOpen));
        assert!(matches!(kinds[11], TokKind::FenceClose));
        assert!(matches!(kinds[12], TokKind::Ident(ref name) if name == "$build"));
        assert!(matches!(kinds[13], TokKind::Ident(ref name) if name == "T"));
        assert!(matches!(kinds[14], TokKind::Dot));
        assert!(matches!(kinds[15], TokKind::Ident(ref name) if name == "$layout"));
        // An adjacent `a$b` is not a fact read: `$` stays its own token.
        assert!(matches!(kinds[16], TokKind::Ident(ref name) if name == "a"));
        assert!(matches!(kinds[17], TokKind::Dollar));
        assert!(matches!(kinds[18], TokKind::Ident(ref name) if name == "b"));
        assert!(matches!(kinds[19], TokKind::Dollar));
        assert!(matches!(kinds[20], TokKind::LBracket));
        assert!(matches!(kinds[22], TokKind::RBracket));
        assert!(matches!(kinds[23], TokKind::Dollar));
    }

    #[test]
    fn fence_digraphs_are_longest_match_beside_comparisons_and_generics() {
        let (tokens, diagnostics) =
            lex_raw("<: a :> x < y x <= y x <=> y x > y List<Int> a::b c:=d e:f");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let kinds = tokens
            .into_iter()
            .map(|token| token.kind)
            .filter(|kind| !matches!(kind, TokKind::Ident(_) | TokKind::Eof))
            .collect::<Vec<_>>();
        assert!(
            matches!(
                kinds.as_slice(),
                [
                    TokKind::FenceOpen,
                    TokKind::FenceClose,
                    TokKind::Lt,
                    TokKind::Le,
                    TokKind::Compare,
                    TokKind::Gt,
                    TokKind::Lt,
                    TokKind::Gt,
                    TokKind::ColonColon,
                    TokKind::ColonEq,
                    TokKind::Colon,
                ]
            ),
            "{kinds:?}"
        );
    }

    #[test]
    fn retired_fence_digraphs_teach_the_new_spelling_with_edits() {
        let (tokens, diagnostics) = lex_raw("print(@[ a, b ]@) x:]@");
        let edits = diagnostics
            .iter()
            .map(|diagnostic| {
                assert_eq!(diagnostic.code, "E-FENCE-SPELLING");
                diagnostic.edit.as_ref().expect("fence respelling edit").new_text.clone()
            })
            .collect::<Vec<_>>();
        assert_eq!(edits, ["<:", ":>", " :>"]);
        assert!(matches!(tokens[2].kind, TokKind::FenceOpen));
        assert!(matches!(tokens[6].kind, TokKind::FenceClose));
    }
}
