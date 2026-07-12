//! Diagnostics: every user-facing error in the language flows through here.
//!
//! Contract (docs/spec/diagnostics.md): every Diagnostic has a stable code,
//! a `what` (one line, plain language), a `why` (the rule behind it), and
//! a `fix` (a concrete next step, copy-pasteable when possible).
//!
//! Render format uses sentence capitalization — `Error` / `Why:` / `Fix:`
//! (owner, 2026-06-11) — and width-aware caret columns so the underline
//! lines up even when the source line holds wide characters or emoji.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    /// Byte offset into the source, inclusive.
    pub start: usize,
    /// Byte offset into the source, exclusive.
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }
}

/// I2: the single macro every compiler-internal panic site goes through.
/// `Source/CmdCompile.rs` owns the "internal compiler error: the generated
/// Rust did not compile" banner for a *rustc-rejected build*; this macro owns
/// the equivalent "internal compiler error: …" prefix for a bug caught
/// *inside* the front end (sema/codegen reaching a construct it should never
/// reach) — same voice, different trigger, one source of the prefix text
/// each side owns. `panic!` already exits the process with Rust's default
/// unhandled-panic code 101, which is exactly `ExitCodes::ICE`
/// (`Source/ExitCodes.rs`) — this macro never touches the exit code, only
/// normalizes the message so every ICE reads the same.
///
/// `span` is `Option<Span>`; pass `None` when the call site has no source
/// span to attach (most codegen-internal sites: the bug is in the compiler's
/// own construct coverage, not tied to one user source location).
///
/// tests/ban_bare_panic.rs enforces that compiler-crate panic sites use this
/// macro (or are on its explicit vetted-site allowlist) instead of a bare
/// `panic!`.
#[macro_export]
macro_rules! ice {
    ($span:expr, $($arg:tt)*) => {{
        let __ice_msg = format!($($arg)*);
        match ($span as Option<$crate::Diagnostics::Span>) {
            Some(__ice_span) => panic!(
                "internal compiler error: {} (source bytes {}..{})",
                __ice_msg, __ice_span.start, __ice_span.end
            ),
            None => panic!("internal compiler error: {}", __ice_msg),
        }
    }};
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Lint,
}

/// The current `--json` diagnostic schema version. Bumped only on a
/// breaking change to the shape below (D-DX1: stable + versioned). Consumers
/// (LSP, fix engine) gate on this field.
pub const JSON_SCHEMA_VERSION: u32 = 1;

/// How the renderer decides whether to emit ANSI color (E2-M3, D-DX*).
///
/// Resolution order, highest priority first:
///   1. `--color=always|never` (the flag always wins)
///   2. `FORCE_COLOR` (any value) forces on; `NO_COLOR` (any value) forces off
///   3. `auto`: color only when the target stream is a real terminal
///
/// Color never changes the bytes a script parses: it is suppressed whenever
/// the stream is piped, redirected, in CI, or `NO_COLOR` is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}

impl ColorChoice {
    /// Parse a `--color=<value>` argument. Unknown values fall back to `Auto`.
    pub fn parse(value: &str) -> ColorChoice {
        match value {
            "always" => ColorChoice::Always,
            "never" => ColorChoice::Never,
            _ => ColorChoice::Auto,
        }
    }

    /// Resolve to a concrete on/off decision for a given stream's TTY-ness,
    /// honoring `NO_COLOR` / `FORCE_COLOR`. `is_tty` is the caller's
    /// `std::io::IsTerminal` check on the target stream.
    pub fn resolve(self, is_tty: bool) -> bool {
        match self {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => {
                if std::env::var_os("NO_COLOR").is_some() {
                    return false;
                }
                if std::env::var_os("FORCE_COLOR").is_some() {
                    return true;
                }
                is_tty
            }
        }
    }
}

/// ANSI style codes, applied only when color is on.
struct Style {
    on: bool,
}

impl Style {
    fn new(on: bool) -> Self {
        Style { on }
    }
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.on {
            format!("\x1b[{}m{}\x1b[0m", code, s)
        } else {
            s.to_string()
        }
    }
    /// Bold red — error labels.
    fn error(&self, s: &str) -> String {
        self.wrap("1;31", s)
    }
    /// Bold yellow — warning labels.
    fn warn(&self, s: &str) -> String {
        self.wrap("1;33", s)
    }
    /// Bold — headings (`Why:` / `Fix:`).
    fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
    /// Dim — the caret line / location.
    fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
}

/// A single-span text replacement (LSP quick-fix / M6 S14 autocorrect).
#[derive(Debug, Clone)]
pub struct TextEdit {
    pub span: Span,
    pub new_text: String,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: String,
    pub what: String,
    pub why: String,
    pub fix: String,
    pub span: Option<Span>,
    /// Mechanical fix when the `fix` line is a simple replace/remove (S14).
    pub edit: Option<TextEdit>,
    /// Extra indented detail (e.g. tool output for E0704).
    pub detail: Option<String>,
}

impl Diagnostic {
    /// If `fix` is `replace \`from\` with \`to\`` or `remove \`tok\` …`, attach
    /// a span edit for LSP/CLI autocorrect (M6 phase 4).
    pub fn attach_teaching_edit(&mut self) {
        if self.edit.is_some() {
            return;
        }
        let span = match self.span {
            Some(s) => s,
            None => return,
        };
        let new_text = if let Some(rest) = self.fix.strip_prefix("replace `") {
            if let Some((_from, rest2)) = rest.split_once("` with `") {
                rest2.strip_suffix('`').map(str::to_string)
            } else {
                None
            }
        } else if let Some(rest) = self.fix.strip_prefix("remove `") {
            rest.split_once('`').map(|_| String::new())
        } else {
            None
        };
        if let Some(text) = new_text {
            self.edit = Some(TextEdit {
                span,
                new_text: text,
            });
        }
    }
}

impl Diagnostic {
    pub fn error(
        code: impl Into<String>,
        what: String,
        why: String,
        fix: String,
        span: Option<Span>,
    ) -> Self {
        let mut d = Diagnostic {
            severity: Severity::Error,
            code: code.into(),
            what,
            why,
            fix,
            span,
            edit: None,
            detail: None,
        };
        d.attach_teaching_edit();
        d
    }

    pub fn lint(
        code: impl Into<String>,
        what: String,
        why: String,
        fix: String,
        span: Option<Span>,
    ) -> Self {
        Diagnostic {
            severity: Severity::Lint,
            code: code.into(),
            what,
            why,
            fix,
            span,
            edit: None,
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: String) -> Self {
        self.detail = Some(detail);
        self
    }

    /// Render in the exact format specified by docs/spec/diagnostics.md, plain
    /// (no color). The ui snapshot tests pin this format; change it
    /// deliberately. This is the byte-stable form scripts and CI parse.
    pub fn render(&self, file: &str, src: &str) -> String {
        self.render_styled(file, src, false)
    }

    /// Color-aware render. When `color` is false the output is byte-for-byte
    /// identical to [`render`]; color only adds ANSI styling around the same
    /// text, so it never changes what a script reads.
    pub fn render_colored(&self, file: &str, src: &str, color: bool) -> String {
        self.render_styled(file, src, color)
    }

    /// Like [`render_colored`], but on a supporting terminal the `--> file:line`
    /// location is wrapped in an OSC 8 hyperlink (a `file://` URL) so editors and
    /// terminals can jump to it (D-DX6). `hyperlinks` must only be true when the
    /// stream is a real TTY *and* color is on; with it off the bytes are
    /// byte-for-byte identical to [`render_colored`], so piped/CI output and the
    /// existing snapshots are unaffected.
    pub fn render_linked(&self, file: &str, src: &str, color: bool, hyperlinks: bool) -> String {
        self.render_inner(file, src, color, hyperlinks)
    }

    fn render_styled(&self, file: &str, src: &str, color: bool) -> String {
        self.render_inner(file, src, color, false)
    }

    fn render_inner(&self, file: &str, src: &str, color: bool, hyperlinks: bool) -> String {
        let st = Style::new(color);
        let mut out = String::new();
        let label = match self.severity {
            Severity::Error => st.error("Error"),
            Severity::Lint => st.warn("Warning"),
        };
        out.push_str(&format!("{} [{}]: {}\n", label, self.code, self.what));
        if let Some(span) = self.span {
            let (line, col) = line_col(src, span.start);
            let loc = format!("--> {}:{}:{}", file, line, col);
            let loc = if hyperlinks {
                osc8(&file_url(file, line, col), &loc)
            } else {
                loc
            };
            out.push_str(&format!("  {}\n", st.dim(&loc)));
            let line_text = src.lines().nth(line - 1).unwrap_or("");
            out.push_str("    |\n");
            out.push_str(&format!("{:>3} | {}\n", line, line_text));

            // Width-aware underline: pad by the display width of everything
            // before the span, then draw carets as wide as the spanned text.
            let prefix: String = line_text.chars().take(col - 1).collect();
            let pad_width = display_width(&prefix);
            let snippet = src.get(span.start..span.end.min(src.len())).unwrap_or("");
            let snippet_first_line: String = snippet.chars().take_while(|&c| c != '\n').collect();
            let avail = display_width(line_text).saturating_sub(pad_width);
            let mut caret_len = display_width(&snippet_first_line).max(1);
            if avail > 0 {
                caret_len = caret_len.min(avail);
            }
            let mut carets = String::new();
            for _ in 0..pad_width {
                carets.push(' ');
            }
            let color_start = carets.len();
            for _ in 0..caret_len {
                carets.push('^');
            }
            // Color only the caret glyphs, not the leading pad (keeps columns).
            if color {
                let (pad, marks) = carets.split_at(color_start);
                out.push_str(&format!("    | {}{}\n", pad, st.error(marks)));
            } else {
                out.push_str(&format!("    | {}\n", carets));
            }
        }
        out.push_str(&format!(" {} {}\n", st.bold("Why:"), self.why));
        out.push_str(&format!(" {} {}\n", st.bold("Fix:"), self.fix));
        if let Some(detail) = &self.detail {
            out.push_str(detail);
            if !detail.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }

    /// Render this diagnostic as one JSON object in the stable `--json` schema
    /// (D-DX1). Hand-rolled (invariant I6: no serde). The shape is:
    ///
    /// ```json
    /// {
    ///   "schema_version": 1,
    ///   "code": "E0102", "severity": "error",
    ///   "message": "…", "why": "…", "fix": "…",
    ///   "detail": "…" | null,
    ///   "file": "a.jet", "line": 2, "col": 5,
    ///   "span": { "start": 12, "end": 17 } | null,
    ///   "edit": { "span": {…}, "new_text": "…" } | null
    /// }
    /// ```
    ///
    /// `edit` is the machine-applicable fix the LSP / fix engine consumes.
    pub fn to_json(&self, file: &str, src: &str) -> String {
        let mut o = String::from("{");
        o.push_str(&format!("\"schema_version\":{}", JSON_SCHEMA_VERSION));
        o.push_str(&format!(",\"code\":{}", json_str(&self.code)));
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Lint => "warning",
        };
        o.push_str(&format!(",\"severity\":{}", json_str(sev)));
        o.push_str(&format!(",\"message\":{}", json_str(&self.what)));
        o.push_str(&format!(",\"why\":{}", json_str(&self.why)));
        o.push_str(&format!(",\"fix\":{}", json_str(&self.fix)));
        match &self.detail {
            Some(d) => o.push_str(&format!(",\"detail\":{}", json_str(d))),
            None => o.push_str(",\"detail\":null"),
        }
        o.push_str(&format!(",\"file\":{}", json_str(file)));
        match self.span {
            Some(span) => {
                let (line, col) = line_col(src, span.start);
                o.push_str(&format!(",\"line\":{},\"col\":{}", line, col));
                o.push_str(&format!(
                    ",\"span\":{{\"start\":{},\"end\":{}}}",
                    span.start, span.end
                ));
            }
            None => {
                o.push_str(",\"line\":null,\"col\":null,\"span\":null");
            }
        }
        match &self.edit {
            Some(e) => {
                o.push_str(&format!(
                    ",\"edit\":{{\"span\":{{\"start\":{},\"end\":{}}},\"new_text\":{}}}",
                    e.span.start,
                    e.span.end,
                    json_str(&e.new_text)
                ));
            }
            None => o.push_str(",\"edit\":null"),
        }
        o.push('}');
        o
    }
}

/// Escape a string as a JSON string literal (RFC 8259), std-only (I6).
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Render a batch of diagnostics as a single JSON document in the `--json`
/// schema: `{ "schema_version": 1, "diagnostics": [ … ] }`.
pub fn render_all_json(file: &str, src: &str, diags: &[Diagnostic]) -> String {
    let mut out = String::from("{");
    out.push_str(&format!("\"schema_version\":{}", JSON_SCHEMA_VERSION));
    out.push_str(",\"diagnostics\":[");
    for (i, d) in diags.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&d.to_json(file, src));
    }
    out.push_str("]}\n");
    out
}

/// Color-aware batch render, blank line between each. Plain when `color` is
/// false (byte-identical to [`render_all`]).
pub fn render_all_colored(file: &str, src: &str, diags: &[Diagnostic], color: bool) -> String {
    let rendered: Vec<String> = diags
        .iter()
        .map(|d| d.render_colored(file, src, color))
        .collect();
    rendered.join("\n")
}

/// Color-aware batch render that may add OSC 8 hyperlinks (D-DX6). `hyperlinks`
/// must only be true on a real TTY with color on; with it off this is identical
/// to [`render_all_colored`].
pub fn render_all_linked(
    file: &str,
    src: &str,
    diags: &[Diagnostic],
    color: bool,
    hyperlinks: bool,
) -> String {
    let rendered: Vec<String> = diags
        .iter()
        .map(|d| d.render_linked(file, src, color, hyperlinks))
        .collect();
    rendered.join("\n")
}

/// Wrap `text` in an OSC 8 terminal hyperlink to `url`.
/// Format: `ESC ] 8 ; ; URL ST  text  ESC ] 8 ; ; ST`, with ST = `ESC \`.
pub fn osc8(url: &str, text: &str) -> String {
    format!("\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\", url, text)
}

/// Build a `file://` URL for a path + line/col, absolutized when possible so
/// the link resolves regardless of the terminal's working directory.
fn file_url(file: &str, line: usize, col: usize) -> String {
    let abs = std::fs::canonicalize(file)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| file.to_string());
    format!("file://{}:{}:{}", abs, line, col)
}

/// 1-based (line, column). Columns count characters, not bytes.
pub fn span_line_col(src: &str, offset: usize) -> (usize, usize) {
    line_col(src, offset)
}

fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in src.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Terminal display width of a string (std-only, invariant I6): combining
/// marks take no column; East Asian wide/fullwidth chars and emoji take two.
pub fn display_width(s: &str) -> usize {
    s.chars().map(display_char_width).sum()
}

/// Terminal display width of one Unicode scalar.
pub fn display_char_width(c: char) -> usize {
    let cp = c as u32;
    // Combining marks and zero-width characters.
    if matches!(
        cp,
        0x0300..=0x036F      // combining diacritics
        | 0x1AB0..=0x1AFF    // combining diacritics extended
        | 0x1DC0..=0x1DFF    // combining diacritics supplement
        | 0x20D0..=0x20FF    // combining marks for symbols
        | 0xFE00..=0xFE0F    // variation selectors
        | 0xFE20..=0xFE2F    // combining half marks
        | 0x200B..=0x200F    // zero-width space/joiners/marks
    ) {
        return 0;
    }
    // East Asian Wide / Fullwidth, plus common emoji blocks.
    if matches!(
        cp,
        0x1100..=0x115F      // Hangul Jamo
        | 0x2E80..=0x303E    // CJK radicals, punctuation
        | 0x3041..=0x33FF    // kana, CJK symbols
        | 0x3400..=0x4DBF    // CJK ext A
        | 0x4E00..=0x9FFF    // CJK unified
        | 0xA000..=0xA4CF    // Yi
        | 0xAC00..=0xD7A3    // Hangul syllables
        | 0xF900..=0xFAFF    // CJK compatibility
        | 0xFE30..=0xFE4F    // CJK compatibility forms
        | 0xFF00..=0xFF60    // fullwidth forms
        | 0xFFE0..=0xFFE6    // fullwidth signs
        | 0x1F300..=0x1F64F  // emoji & pictographs
        | 0x1F680..=0x1F6FF  // transport emoji
        | 0x1F900..=0x1FAFF  // supplemental emoji
        | 0x20000..=0x3FFFD  // CJK ext B+
    ) {
        return 2;
    }
    1
}

/// Render a batch of diagnostics, blank line between each.
pub fn render_all(file: &str, src: &str, diags: &[Diagnostic]) -> String {
    let rendered: Vec<String> = diags.iter().map(|d| d.render(file, src)).collect();
    rendered.join("\n")
}
