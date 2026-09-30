//! User-facing syntax dictionary.
//!
//! Rows start with `JET_HIGHLIGHT_TOKENS`, which already references the
//! canonical `Syntax` constants. Decision IDs come from those constants'
//! source comments or the applied-rule registry; no second token list exists.

use std::sync::LazyLock;

use super::{highlighted_tokens_sorted, HighlightClass, HighlightToken, JET_KEYWORD_LIST};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SyntaxDictionaryKind {
    Keyword,
    Marker,
    Operator,
    Sigil,
    Literal,
    Type,
    Builtin,
}

/// One meaning of a token at one kind of position. A sigil or operator can
/// mean different things before a name, between two values, or inside a
/// signature; each meaning carries its own example and what to expect after.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxMeaning {
    /// Where the token sits, in plain words ("Before a name"). Empty for
    /// tokens whose meaning does not depend on position.
    pub position: String,
    pub text: String,
    /// A short example, one statement per line.
    pub example: String,
    /// What the reader should expect after this use. May be empty.
    pub afterwards: String,
    pub related_codes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxDictionaryRow {
    pub token: String,
    /// Internal constant name; shown only on request (`jet explain --verbose`).
    pub name: String,
    /// Owning decision ID; shown only on request (`jet explain --verbose`).
    pub decision: String,
    pub meanings: Vec<SyntaxMeaning>,
    pub kind: SyntaxDictionaryKind,
}

static ROWS: LazyLock<Vec<SyntaxDictionaryRow>> = LazyLock::new(build_rows);

pub fn rows() -> &'static [SyntaxDictionaryRow] {
    &ROWS
}

pub fn lookup(query: &str) -> Option<&'static SyntaxDictionaryRow> {
    let query = query.trim();
    rows().iter().find(|row| {
        row.token.eq_ignore_ascii_case(query)
            || display(row).eq_ignore_ascii_case(query)
            || row.kind == SyntaxDictionaryKind::Marker
                && row
                    .token
                    .eq_ignore_ascii_case(query.strip_prefix('#').unwrap_or(query))
    })
}

pub fn nearest(query: &str) -> Option<&'static SyntaxDictionaryRow> {
    let query = query.trim();
    rows().iter().min_by_key(|row| {
        levenshtein(
            &query.to_ascii_lowercase(),
            &display(row).to_ascii_lowercase(),
        )
    })
}

pub fn looks_like_query(query: &str) -> bool {
    let query = query.trim();
    if query.starts_with("package-overlay:") {
        return false;
    }
    if is_package_ref(query) {
        return false;
    }
    query.starts_with('@')
        || query.starts_with('#')
        || query.starts_with("::")
        || query.starts_with(":=")
        || query
            .chars()
            .any(|ch| !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '.' | '-' | '/' | '@'))
}

fn is_package_ref(query: &str) -> bool {
    if query.contains('@') && !query.starts_with('@') {
        return true;
    }
    query.split_once(':').is_some_and(|(prefix, suffix)| {
        !prefix.is_empty()
            && !suffix.is_empty()
            && prefix
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
            && suffix.chars().any(|ch| ch.is_ascii_alphanumeric())
    })
}

fn build_rows() -> Vec<SyntaxDictionaryRow> {
    let decisions = source_decisions();
    let acronym_decision = source_header_decision(include_str!("acronyms.rs"));
    let mut rows = Vec::new();
    let mut tokens = highlighted_tokens_sorted();
    for text in [
        super::MARKER_REGION,
        super::MARKER_LIVE,
        super::MARKER_NONDETERMINISTIC,
    ] {
        if !tokens.iter().any(|token| token.text == text) {
            tokens.push(HighlightToken {
                text,
                class: HighlightClass::MarkerRule,
            });
        }
    }
    for &text in JET_KEYWORD_LIST {
        if !tokens.iter().any(|token| token.text == text) {
            tokens.push(HighlightToken {
                text,
                class: HighlightClass::KeywordOther,
            });
        }
    }
    for row in crate::Registry::rows()
        .iter()
        .filter(|row| row.kind() == crate::Registry::RowKind::Marker)
    {
        if !tokens.iter().any(|token| token.text == row.name) {
            tokens.push(HighlightToken {
                text: row.name,
                class: HighlightClass::MarkerRule,
            });
        }
    }
    for token in tokens {
        if rows
            .iter()
            .any(|row: &SyntaxDictionaryRow| row.token == token.text)
        {
            continue;
        }
        let kind = kind(token.class);
        let (name, decision) = metadata(&decisions, token.text, kind, acronym_decision.as_deref());
        rows.push(SyntaxDictionaryRow {
            token: token.text.to_string(),
            name,
            decision,
            meanings: meanings(token.text, kind),
            kind,
        });
    }
    rows
}

fn metadata(
    decisions: &[(String, (String, String))],
    token: &str,
    kind: SyntaxDictionaryKind,
    acronym_decision: Option<&str>,
) -> (String, String) {
    let mut candidates = decisions
        .iter()
        .filter(|(value, (_, decision))| value.as_str() == token && decision != "Syntax.rs");
    let candidate = candidates
        .find(|(_, (name, _))| constant_matches_kind(name, kind))
        .or_else(|| {
            decisions
                .iter()
                .find(|(value, (_, decision))| value.as_str() == token && decision != "Syntax.rs")
        });
    if let Some((_, metadata)) = candidate {
        return metadata.clone();
    }
    if let Some(row) = crate::Registry::row(token) {
        return (token.to_string(), row.decision.to_string());
    }
    if is_acronym(token) {
        if let Some(decision) = acronym_decision {
            return (token.to_string(), decision.to_string());
        }
    }
    (token.to_string(), "Syntax.rs".to_string())
}

fn constant_matches_kind(name: &str, kind: SyntaxDictionaryKind) -> bool {
    match kind {
        SyntaxDictionaryKind::Keyword => {
            name.starts_with("KW_") || name.starts_with("LIT_") || name.starts_with("BUILTIN_")
        }
        SyntaxDictionaryKind::Marker => name.starts_with("MARKER_") || name.starts_with("RULE_"),
        SyntaxDictionaryKind::Operator => name.starts_with("OP_"),
        SyntaxDictionaryKind::Sigil => {
            name.starts_with("SIGIL_")
                || name.ends_with("_PREFIX")
                || name.ends_with("_MARK")
                || name == "TYPE_FIXED_SIZE_SEP"
        }
        SyntaxDictionaryKind::Literal => name.starts_with("LIT_"),
        SyntaxDictionaryKind::Type => name.starts_with("TYPE_") || name.ends_with("_TYPE"),
        SyntaxDictionaryKind::Builtin => name.starts_with("BUILTIN_"),
    }
}

fn source_header_decision(source: &str) -> Option<String> {
    source
        .lines()
        .take(24)
        .find_map(decision_id)
        .map(str::to_string)
}

fn is_acronym(token: &str) -> bool {
    token.chars().filter(|ch| ch.is_ascii_uppercase()).count() >= 2
        && token.chars().any(|ch| ch.is_ascii_lowercase())
}

fn kind(class: HighlightClass) -> SyntaxDictionaryKind {
    match class {
        HighlightClass::KeywordControl
        | HighlightClass::KeywordDeclaration
        | HighlightClass::KeywordOwnership
        | HighlightClass::KeywordOther => SyntaxDictionaryKind::Keyword,
        HighlightClass::MarkerRule => SyntaxDictionaryKind::Marker,
        HighlightClass::Operator => SyntaxDictionaryKind::Operator,
        HighlightClass::Sigil => SyntaxDictionaryKind::Sigil,
        HighlightClass::Literal => SyntaxDictionaryKind::Literal,
        HighlightClass::TypeBuiltin => SyntaxDictionaryKind::Type,
        HighlightClass::Builtin => SyntaxDictionaryKind::Builtin,
    }
}

/// The plain-words default text for a row: every meaning, each with its
/// example, what happens afterwards, and related diagnostic codes. `jet
/// explain`, the REPL `?` and LSP hover all show this text; internal
/// constant names and decision IDs are not part of it.
pub fn plain_text(row: &SyntaxDictionaryRow) -> String {
    let mut out = String::new();
    for (index, meaning) in row.meanings.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let indent = if meaning.position.is_empty() {
            ""
        } else {
            out.push_str(&meaning.position);
            out.push_str(":\n");
            "  "
        };
        out.push_str(&format!("{indent}{}\n{indent}Example:\n", meaning.text));
        for line in meaning.example.lines() {
            out.push_str(&format!("{indent}  {line}\n"));
        }
        if !meaning.afterwards.is_empty() {
            out.push_str(&format!("{indent}Afterwards: {}\n", meaning.afterwards));
        }
        if !meaning.related_codes.is_empty() {
            out.push_str(&format!(
                "{indent}Related: {}\n",
                meaning.related_codes.join(", ")
            ));
        }
    }
    out
}

/// `(position, text, example, afterwards, related codes)`.
type Curated = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
);

const COMPOUND_AFTER: &str =
    "The name must be mutable (`:=`) or a place marked `&`; its new value is used from here on.";

/// Plain-words meanings for every registered sigil and operator. Rows are the
/// only home for this text.
fn curated(token: &str) -> &'static [Curated] {
    use super::*;
    match token {
        SIGIL_BIND_IMMUT => &[(
            "Between a name and a value",
            "Binds the name to a value that never changes.",
            "answer :: 42\nprint(answer)",
            "`answer` keeps this value for the rest of its block; assigning to it again is an error.",
            &["E0111"],
        )],
        SIGIL_BIND_MUT => &[(
            "Between a name and a value",
            "Binds the name to a value that can change later with `=`.",
            "count := 0\ncount = count + 1",
            "Change it with `=` or a compound form such as `+=`. A `:=` name that never changes gets a hint to use `::`.",
            &["L0528"],
        )],
        COMPTIME_MARK => &[(
            "Before a name",
            "Marks a fact the compiler supplies: a `$` member of the thing it describes, or one of the roots `$build`, `$package`, `$phase` and `$program`. Programs read facts; they never declare a `$` name.",
            "print(Point.$name)\nprep if $build.os == {\n    .Linux -> { print(\"linux\") }\n    else -> { print(\"other\") }\n}",
            "A fact is known while building, so reading it costs nothing at run time. Marker and fact declarations mark their compiler metadata the same way: `$sites:`.",
            &[],
        )],
        SIGIL_FENCE_OPEN => &[(
            "At the start of a list of entries",
            "Opens a fence: the whole statement repeats once for each entry between `<:` and `:>`.",
            "<: a, b, c :> :: 0\nprint(<: a, b, c :>)",
            "No list is created; each entry gets its own copy of the statement. Fences in one statement advance together, so they need the same number of entries.",
            &["E0368", "E0370"],
        )],
        SIGIL_FENCE_CLOSE => &[(
            "At the end of a list of entries",
            "Closes a fence opened with `<:`.",
            "<: low, high :> :: 1\nprint(<: low, high :>)",
            "The statement around the fence runs once per entry.",
            &["E0371"],
        )],
        SIGIL_MOVE => &[
            (
                "Before a name (move)",
                "Moves the value out of the name and hands it to where the mark is written. Jet already moves a name at its last use, so the mark is optional; it makes the move exact and visible.",
                "names :: [\"ada\", \"grace\"]\narchive(^names)",
                "`names` can no longer be used: a later use is error E0121. To keep using it, pass a copy with `~names` instead.",
                &["E0121", "E0225"],
            ),
            (
                "Before a parameter's type",
                "`name: ^T` says the function takes ownership of that argument.",
                "fn archive(name: ^String) -> String { name }\nsaved :: archive(^title)",
                "The caller's value moves into the function; the caller can't use it after the call.",
                &["E0209"],
            ),
            (
                "Between two numbers (power)",
                "Raises the left number to the power of the right one.",
                "side :: 3\narea :: side ^ 2",
                "Both numbers are unchanged and the result is a new number (here 9). `x ^= 2` raises `x` in place.",
                &[],
            ),
        ],
        SIGIL_WRITE => &[
            (
                "Before a name or a call on it",
                "Marks the place that this call changes, so every write is visible where it happens.",
                "score := 41\nbump(&score)\n&items.push(4)",
                "The change shows up in `score` and `items` afterwards. A call that writes without the mark is error E0202.",
                &["E0202", "E0213"],
            ),
            (
                "Before a parameter's type",
                "`name: &T` lets the function change the caller's value.",
                "fn bump(n: &Int) { n += 1 }\nbump(&score)",
                "Callers must mark the argument with `&`.",
                &["E0202"],
            ),
            (
                "Between two whole numbers",
                "Bitwise AND: each result bit is 1 only where both numbers have a 1.",
                "flags :: 6\nlow :: flags & 3",
                "Both numbers are unchanged; here `low` is 2.",
                &[],
            ),
        ],
        SIGIL_COPY => &[(
            "Before a value",
            "Makes an independent copy of the value.",
            "items := [1, 2, 3]\nbackup :: ~items",
            "Changing one does not change the other, and the original stays usable.",
            &["E0211"],
        )],
        SIGIL_SPREAD => &[
            (
                "Before a list in a call or list literal",
                "Spreads the list's items in place, as if each were written out.",
                "both :: [...first, 0, ...second]\nshow(...names)",
                "The spread list itself is unchanged.",
                &["E1311", "E1312"],
            ),
            (
                "Before the last parameter's type",
                "`name: ...T` accepts any number of arguments and collects them into a list.",
                "fn total(numbers: ...Int) -> Int { … }\ntotal(1, 2, 3)",
                "Only the last parameter can collect arguments, and it can't have a default.",
                &["E1310"],
            ),
        ],
        OP_MEMBER_SPREAD => &[(
            "After a value or module name",
            "Picks several members at once.",
            "checks :: v.[length, charset]\nuse numeric.[clamp, abs]",
            "On a value you get a list of those members; after `use` each name comes into scope.",
            &["E0961"],
        )],
        OP_TRY_SUFFIX => &[
            (
                "After a type",
                "`T?` is an optional value: it holds a `T` or nothing (`None`).",
                "fn find(id: Int) -> Entry? { … }\nentry :: find(7) ?? return",
                "Handle the missing case with `??`, `?.` or an `if` check before using the value.",
                &["E0308", "E0047"],
            ),
            (
                "After a call, followed by a note in parentheses",
                "Adds a note to any failure that passes through this call.",
                "raw :: read_raw()?(\"reading the config\")\nreturn Ok(raw)",
                "On success nothing changes; on failure the caller sees the failure with the note attached.",
                &[],
            ),
        ],
        OP_RANGE => &[(
            "Between two whole numbers",
            "A range from the first number to the last, both included.",
            "loop i in 1..3 { print(i) }",
            "The loop sees 1, 2 and 3. Use `..<` to leave out the last number.",
            &["E0364"],
        )],
        OP_RANGE_EXCLUSIVE => &[(
            "Between two whole numbers",
            "A range that stops just before the last number.",
            "loop i in 0..<3 { print(i) }",
            "The loop sees 0, 1 and 2. The range is empty when the start is not less than the end.",
            &[],
        )],
        OP_UNIFIED_ARROW => &[(
            "After parameters, a pattern, or a loop head",
            "Points to the result: a function's return type, or the value or statement of a lambda, match arm, or loop body.",
            "fn twice(n: Int) -> Int { n * 2 }\nloop value in values -> print(value)",
            "Write `-[IO]>` instead of `->` to state which effects a function may use.",
            &[],
        )],
        OP_PLUS => &[(
            "Between two numbers",
            "Adds the numbers.",
            "total :: price + tax",
            "Both numbers are unchanged; the sum is a new value.",
            &[],
        )],
        OP_MINUS => &[
            (
                "Between two numbers",
                "Subtracts the right number from the left one.",
                "change :: paid - price",
                "Both numbers are unchanged.",
                &[],
            ),
            (
                "Before a number",
                "Negates the number.",
                "below :: -5\nflipped :: -below",
                "",
                &[],
            ),
        ],
        OP_STAR => &[
            (
                "Between two numbers",
                "Multiplies the numbers.",
                "area :: width * height",
                "Both numbers are unchanged.",
                &[],
            ),
            (
                "Alone in a parameter list",
                "Starts the label-only parameters: callers must name each argument after it.",
                "fn draw(size: Int, *, color: String) { … }\ndraw(3, color: \"red\")",
                "",
                &[],
            ),
        ],
        OP_SLASH => &[
            (
                "Between two numbers",
                "Divides the left number by the right one. Whole numbers drop the fraction (7 / 2 is 3); `/%` rounds down instead.",
                "half :: total / 2",
                "Both numbers are unchanged. `%%` gives the matching remainder.",
                &[],
            ),
            (
                "Alone in a parameter list",
                "Ends the position-only parameters: callers pass them without names.",
                "fn show(n: Int, /) { print(\"{n}\") }\nshow(5)",
                "",
                &[],
            ),
        ],
        OP_SLASH_PERCENT => &[(
            "Between two whole numbers",
            "Divides and rounds down, toward negative infinity.",
            "rows :: -7 /% 2",
            "Here `rows` is -4. `%` gives the matching remainder.",
            &[],
        )],
        OP_PERCENT => &[(
            "Between two whole numbers",
            "Modulo: the answer takes the divisor's sign, so -7 % 2 is 1.",
            "slot :: index % size",
            "It pairs with `/%`.",
            &[],
        )],
        OP_PERCENT_PERCENT => &[(
            "Between two whole numbers",
            "Remainder: the answer takes the dividend's sign, so -7 %% 2 is -1.",
            "left_over :: total %% 3",
            "It pairs with `/`.",
            &[],
        )],
        OP_PIPE => &[
            (
                "Between types or patterns",
                "Either alternative is accepted.",
                "fn load(path: String) -> Config (IOError | ParseError)! { … }",
                "Code that handles the result must cover every alternative.",
                &[],
            ),
            (
                "Between two whole numbers",
                "Bitwise OR: each result bit is 1 where either number has a 1.",
                "mode :: READ | WRITE",
                "Both numbers are unchanged.",
                &[],
            ),
        ],
        OP_SHL => &[(
            "Between two whole numbers",
            "Shifts the left number's bits left by the right number of places.",
            "eight :: 1 << 3",
            "",
            &[],
        )],
        OP_SHR => &[(
            "Between two whole numbers",
            "Shifts the left number's bits right by the right number of places.",
            "two :: 16 >> 3",
            "",
            &[],
        )],
        OP_AND => &[(
            "Between two conditions",
            "True when both are true. The right side runs only when the left is true.",
            "if ready && count > 0 { start() }",
            "",
            &[],
        )],
        OP_OR => &[(
            "Between two conditions",
            "True when either is true. The right side runs only when the left is false.",
            "if empty || closed { return }",
            "",
            &[],
        )],
        OP_NOT => &[
            (
                "Before a condition",
                "Flips true and false.",
                "if !done { keep_going() }",
                "",
                &[],
            ),
            (
                "After an error type in a signature",
                "`E!` marks the errors a function can fail with; `(A | B)!` lists several.",
                "fn parse(text: String) -> Int ParseError! { … }\nvalue :: parse(input) ?? 0",
                "A failure passes to the caller on its own; handle it where you want with `??`.",
                &["E0405"],
            ),
        ],
        OP_EQ => &[(
            "Between two values",
            "True when the values are equal.",
            "if answer == 42 { print(\"found\") }",
            "`=` assigns; `==` only compares.",
            &[],
        )],
        OP_NE => &[(
            "Between two values",
            "True when the values differ.",
            "if value != None { use(value) }",
            "",
            &[],
        )],
        OP_LT => &[
            (
                "Between two values",
                "True when the left value is smaller.",
                "if age < 18 { print(\"minor\") }",
                "",
                &[],
            ),
            (
                "After a type or function name",
                "Opens a list of type arguments, closed by `>`.",
                "names :: List<String>.new()\n(sender, receiver) :: channel<Int>()",
                "",
                &[],
            ),
        ],
        OP_GT => &[
            (
                "Between two values",
                "True when the left value is larger.",
                "if score > best { best = score }",
                "",
                &[],
            ),
            (
                "After type arguments",
                "Closes a list of type arguments opened by `<`.",
                "cache :: Map<String, Int>.new()",
                "",
                &[],
            ),
        ],
        OP_LE => &[(
            "Between two values",
            "True when the left value is smaller or equal.",
            "if count <= limit { add() }",
            "",
            &[],
        )],
        OP_GE => &[(
            "Between two values",
            "True when the left value is larger or equal.",
            "if total >= 100 { print(\"free shipping\") }",
            "",
            &[],
        )],
        OP_COMPARE => &[(
            "Between two values",
            "Compares in one step and gives an `Ordering`: `.Less`, `.Equal` or `.Greater`.",
            "order :: left <=> right",
            "Match on the `Ordering` to handle each case.",
            &[],
        )],
        OP_PLUS_EQ => &[(
            "After a mutable name",
            "Adds to the name in place: `x += 1` is `x = x + 1`.",
            "count := 0\ncount += 1",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_MINUS_EQ => &[(
            "After a mutable name",
            "Subtracts from the name in place: `x -= 1` is `x = x - 1`.",
            "lives := 3\nlives -= 1",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_STAR_EQ => &[(
            "After a mutable name",
            "Multiplies the name in place: `x *= 2` is `x = x * 2`.",
            "size := 4\nsize *= 2",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_SLASH_EQ => &[(
            "After a mutable name",
            "Divides the name in place: `x /= 2` is `x = x / 2`.",
            "size := 8\nsize /= 2",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_SLASH_PERCENT_EQ => &[(
            "After a mutable name",
            "Divides the name in place, rounding down: `x /%= 2` is `x = x /% 2`.",
            "step := -7\nstep /%= 2",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_PERCENT_EQ => &[(
            "After a mutable name",
            "Replaces the name with its modulo: `x %= 3` is `x = x % 3`.",
            "slot := 10\nslot %= 3",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_PERCENT_PERCENT_EQ => &[(
            "After a mutable name",
            "Replaces the name with its remainder: `x %%= 3` is `x = x %% 3`.",
            "rest := -7\nrest %%= 3",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_AMP_EQ => &[(
            "After a mutable name",
            "Bitwise AND in place: `x &= m` is `x = x & m`.",
            "flags := 7\nflags &= 3",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_PIPE_EQ => &[(
            "After a mutable name",
            "Bitwise OR in place: `x |= m` is `x = x | m`.",
            "flags := 1\nflags |= 4",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_CARET_EQ => &[(
            "After a mutable name",
            "Raises the name to a power in place: `x ^= 2` is `x = x ^ 2`.",
            "side := 3\nside ^= 2",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_TILDE_PIPE => &[(
            "Between two whole numbers",
            "Bitwise exclusive OR: each result bit is 1 where exactly one number has a 1.",
            "toggled :: flags ~| 1",
            "Both numbers are unchanged. Prefix `~` alone still means copy.",
            &[],
        )],
        OP_TILDE_PIPE_EQ => &[(
            "After a mutable name",
            "Bitwise exclusive OR in place: `x ~|= m` is `x = x ~| m`.",
            "flags := 5\nflags ~|= 1",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_SHL_EQ => &[(
            "After a mutable name",
            "Shifts the name's bits left in place: `x <<= 1` is `x = x << 1`.",
            "bits := 1\nbits <<= 3",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_SHR_EQ => &[(
            "After a mutable name",
            "Shifts the name's bits right in place: `x >>= 1` is `x = x >> 1`.",
            "bits := 16\nbits >>= 3",
            COMPOUND_AFTER,
            &["E0111"],
        )],
        OP_FALLBACK => &[(
            "After an optional or fallible value",
            "Supplies what to use when the left side is missing (`None`) or failed: a value, or a way out such as `return`, `break`, `next` or `panic(…)`.",
            "port :: env_port() ?? 8080\nuser :: load_user() ?? return",
            "The result is a plain value, no longer optional. Inside a fallback after a failure, `err` names the failure.",
            &["E0405", "E0408"],
        )],
        OP_OPTIONAL_CHAIN => &[(
            "After an optional value",
            "Reads a field or calls a method only when the value is there; it stops at the first missing link.",
            "city :: user?.address?.city\nname :: user?.display_name()",
            "The result is optional (`T?`), so handle the missing case with `??`.",
            &["E0047"],
        )],
        OP_NAMED_CTOR => &[
            (
                "After a type name",
                "Builds a value by naming its fields. Without a type name, the type comes from context.",
                "origin :: Point{x: 0, y: 0}\nmove_to({x: 3, y: 4})",
                "",
                &[],
            ),
            (
                "After a field or parameter type",
                "`T{value}` gives the field or parameter a default.",
                "fn greet(name: String{\"world\"}) { print(\"hello, {name}\") }\ngreet()",
                "Callers may leave that argument out.",
                &["E2414"],
            ),
        ],
        TYPE_FIXED_SIZE_SEP => &[
            (
                "Before a name",
                "Applies a marker to what follows, such as `#Test`; `#[A, B]` applies several.",
                "#Test(\"adds\") {\n    assert_eq(1 + 1, 2)\n}",
                "Run `jet explain #Name` to read what a marker does.",
                &[],
            ),
            (
                "Inside a list type",
                "`[T#N]` is a list of exactly N items.",
                "fn mix(color: [Int#3]) -> Int { color[0] }",
                "The length is fixed: `push` and `pop` are errors, and a constant index past the end is caught before the program runs.",
                &["E0964", "E0965"],
            ),
            (
                "After a package name",
                "Pins an exact package version.",
                "json#1.2.0",
                "",
                &[],
            ),
        ],
        _ => &[],
    }
}

fn meanings(token: &str, kind: SyntaxDictionaryKind) -> Vec<SyntaxMeaning> {
    let curated = curated(token);
    if !curated.is_empty() {
        return curated
            .iter()
            .map(|(position, text, example, afterwards, codes)| SyntaxMeaning {
                position: position.to_string(),
                text: text.to_string(),
                example: example.to_string(),
                afterwards: afterwards.to_string(),
                related_codes: codes.iter().map(|code| code.to_string()).collect(),
            })
            .collect();
    }
    let (text, example) = match token {
        super::MARKER_LIVE => (
            "Marks a block that takes input directly from the terminal.".to_string(),
            "#Live { input() }".to_string(),
        ),
        _ => match kind {
            SyntaxDictionaryKind::Keyword => {
                (format!("The Jet keyword `{token}`."), format!("{token} …"))
            }
            SyntaxDictionaryKind::Marker => {
                (format!("The marker `#{token}`."), format!("#{token} …"))
            }
            SyntaxDictionaryKind::Operator => {
                (format!("The operator `{token}`."), format!("left {token} right"))
            }
            SyntaxDictionaryKind::Sigil => {
                (format!("The sigil `{token}`."), format!("name {token} value"))
            }
            SyntaxDictionaryKind::Literal => (format!("The literal `{token}`."), token.to_string()),
            SyntaxDictionaryKind::Type => {
                (format!("The built-in type `{token}`."), token.to_string())
            }
            SyntaxDictionaryKind::Builtin => (format!("The built-in `{token}`."), token.to_string()),
        },
    };
    vec![SyntaxMeaning {
        position: String::new(),
        text,
        example,
        afterwards: String::new(),
        related_codes: Vec::new(),
    }]
}

pub fn display(row: &SyntaxDictionaryRow) -> String {
    if row.kind == SyntaxDictionaryKind::Marker {
        format!("#{}", row.token)
    } else {
        row.token.clone()
    }
}

fn source_decisions() -> Vec<(String, (String, String))> {
    const SOURCES: &[&str] = &[
        include_str!("../Syntax.rs"),
        include_str!("core_surface.rs"),
        include_str!("effects_surface.rs"),
        include_str!("math_layout.rs"),
        include_str!("markers.rs"),
        include_str!("package_files.rs"),
        include_str!("jetpack_config.rs"),
        include_str!("predicates.rs"),
    ];
    let mut out = Vec::new();
    for source in SOURCES {
        let mut comments = String::new();
        let mut inherited_comments = String::new();
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                comments.push_str(trimmed);
                comments.push('\n');
                continue;
            }
            let Some(rest) = trimmed.strip_prefix("pub const ") else {
                comments.clear();
                inherited_comments.clear();
                continue;
            };
            let Some((name, rhs)) = rest.split_once(':') else {
                comments.clear();
                continue;
            };
            let Some(value) = rhs
                .split_once("= \"")
                .and_then(|(_, rest)| rest.split_once('"'))
            else {
                comments.clear();
                continue;
            };
            let decision = decision_id(trimmed)
                .or_else(|| comment_decision(&comments))
                .or_else(|| decision_id(&inherited_comments))
                .unwrap_or("Syntax.rs")
                .to_string();
            if !comments.is_empty() {
                inherited_comments.clone_from(&comments);
            }
            out.push((value.0.to_string(), (name.trim().to_string(), decision)));
            comments.clear();
        }
    }
    out
}

fn decision_id(text: &str) -> Option<&str> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '-')
        .find(|word| {
            word.starts_with("D-")
                || word.starts_with('S') && word[1..].chars().all(|ch| ch.is_ascii_digit())
                || word.starts_with('M') && word[1..].chars().all(|ch| ch.is_ascii_digit())
                || word.starts_with('U') && word[1..].chars().all(|ch| ch.is_ascii_digit())
        })
}

fn comment_decision(text: &str) -> Option<&str> {
    text.lines().rev().find_map(decision_id)
}

fn levenshtein(left: &str, right: &str) -> usize {
    let mut row: Vec<usize> = (0..=right.chars().count()).collect();
    for (i, left) in left.chars().enumerate() {
        let mut next = vec![i + 1];
        for (j, right) in right.chars().enumerate() {
            next.push(if left == right {
                row[j]
            } else {
                1 + row[j].min(row[j + 1]).min(next[j])
            });
        }
        row = next;
    }
    row[right.chars().count()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_covers_highlights_and_keywords_without_foreign_words() {
        for token in crate::Syntax::JET_HIGHLIGHT_TOKENS {
            assert!(
                lookup(token.text).is_some(),
                "missing highlight token `{}`",
                token.text
            );
        }
        for token in JET_KEYWORD_LIST {
            assert!(lookup(token).is_some(), "missing keyword `{token}`");
        }

        let constants = source_decisions();
        for row in rows() {
            assert!(
                constants.iter().any(|(token, _)| token == &row.token)
                    || crate::Registry::row(&row.token).is_some(),
                "dictionary token `{}` is not a Syntax constant or registry marker",
                row.token
            );
            assert_ne!(
                row.decision, "Syntax.rs",
                "missing decision ID for `{}`",
                row.token
            );
            assert!(
                !row.meanings.is_empty()
                    && row.meanings.iter().all(|meaning| !meaning.example.is_empty()),
                "missing example for `{}`",
                row.token
            );
        }
    }
}
