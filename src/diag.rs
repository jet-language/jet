//! Diagnostics: every user-facing error in the language flows through here.
//!
//! Contract (docs/04-diagnostics.md): every Diagnostic has a stable code,
//! a `what` (one line, plain language), a `why` (the rule behind it), and
//! a `fix` (a concrete next step, copy-pasteable when possible).
//!
//! Render format uses sentence capitalization — `Error` / `Why:` / `Fix:`
//! (owner, 2026-06-11) — and width-aware caret columns so the underline
//! lines up even when the source line holds wide characters or emoji.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Lint,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub fix: String,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn error(
        code: &'static str,
        what: String,
        why: String,
        fix: String,
        span: Option<Span>,
    ) -> Self {
        Diagnostic {
            severity: Severity::Error,
            code,
            what,
            why,
            fix,
            span,
        }
    }

    pub fn lint(
        code: &'static str,
        what: String,
        why: String,
        fix: String,
        span: Option<Span>,
    ) -> Self {
        Diagnostic {
            severity: Severity::Lint,
            code,
            what,
            why,
            fix,
            span,
        }
    }

    /// Render in the exact format specified by docs/04-diagnostics.md.
    /// The ui snapshot tests pin this format; change it deliberately.
    pub fn render(&self, file: &str, src: &str) -> String {
        let mut out = String::new();
        let label = match self.severity {
            Severity::Error => "Error",
            Severity::Lint => "Warning",
        };
        out.push_str(&format!("{} [{}]: {}\n", label, self.code, self.what));
        if let Some(span) = self.span {
            let (line, col) = line_col(src, span.start);
            out.push_str(&format!("  --> {}:{}:{}\n", file, line, col));
            let line_text = src.lines().nth(line - 1).unwrap_or("");
            out.push_str("    |\n");
            out.push_str(&format!("{:>3} | {}\n", line, line_text));

            // Width-aware underline: pad by the display width of everything
            // before the span, then draw carets as wide as the spanned text.
            let prefix: String = line_text.chars().take(col - 1).collect();
            let pad_width = display_width(&prefix);
            let snippet = src.get(span.start..span.end.min(src.len())).unwrap_or("");
            let snippet_first_line: String =
                snippet.chars().take_while(|&c| c != '\n').collect();
            let avail = display_width(line_text).saturating_sub(pad_width);
            let mut caret_len = display_width(&snippet_first_line).max(1);
            if avail > 0 {
                caret_len = caret_len.min(avail);
            }
            out.push_str("    | ");
            for _ in 0..pad_width {
                out.push(' ');
            }
            for _ in 0..caret_len {
                out.push('^');
            }
            out.push('\n');
        }
        out.push_str(&format!(" Why: {}\n", self.why));
        out.push_str(&format!(" Fix: {}\n", self.fix));
        out
    }
}

/// 1-based (line, column). Columns count characters, not bytes.
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
fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

fn char_width(c: char) -> usize {
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
