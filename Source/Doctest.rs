//! D-TEST4 (ratified 2026-06-22, option A): doctests — runnable examples inside
//! `///` doc comments (S49). A fenced ```` ```jet ```` block runs as a test under
//! `jet test`; an expected value is a `// =>` trailing comment on the line that
//! produces it. A mismatch fires **E2901**. Reuses the `//` comment marker (S5);
//! no new tokens.
//!
//! Discovery is purely textual (the lexer already preserves doc comments as
//! `LineComment` tokens, but scanning the source directly is simpler and keeps
//! the producing-line numbers exact). Each block is compiled and run as a
//! self-contained Jet program: setup lines are emitted verbatim inside
//! `fn main()`, and each `EXPR // => VALUE` line is run as `print("{EXPR}")` so
//! its `JetShow` rendering can be compared against the claimed `VALUE`.

use crate::Diagnostics::{Diagnostic, Span};

/// One `EXPR // => VALUE` expectation inside a doctest block.
#[derive(Debug, Clone)]
pub struct DocExpect {
    /// The expression text to the left of `// =>` (trimmed).
    pub expr: String,
    /// The expected `JetShow` rendering, to the right of `// =>` (trimmed).
    pub expected: String,
    /// 1-based source line of the producing line, for the E2901 span message.
    pub line: usize,
}

/// One ```` ```jet ```` fenced block found inside a run of `///` doc comments.
#[derive(Debug, Clone)]
pub struct DocBlock {
    /// 1-based source line of the opening fence.
    pub fence_line: usize,
    /// Lines that are not `// =>` expectations — setup/statements, verbatim.
    pub setup: Vec<String>,
    /// The `// =>` expectations, in source order. Interleaving with setup is
    /// preserved by emitting setup first then the expectation `print`s in order;
    /// for the simple "value on the producing line" model this matches author
    /// intent (each expectation prints exactly once, after all setup).
    pub expects: Vec<DocExpect>,
}

/// Scan `src` for doctest blocks: ```` ```jet ```` fences inside `///` doc-comment
/// runs. Lines that don't begin with `///` interrupt a doc-comment run, so a
/// fence only counts when every line of the block is a doc comment.
pub fn discover(src: &str) -> Vec<DocBlock> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        // A fence opens with `/// ```jet` (any indentation before `///`).
        if is_doc_line(trimmed) {
            let inner = doc_inner(trimmed);
            if inner.trim_start().starts_with("```") && fence_lang_is_jet(inner.trim_start()) {
                let fence_line = i + 1;
                let mut setup = Vec::new();
                let mut expects = Vec::new();
                i += 1;
                // Collect until the closing fence or the doc-comment run ends.
                while i < lines.len() {
                    let t = lines[i].trim_start();
                    if !is_doc_line(t) {
                        break; // doc run interrupted before close — drop block
                    }
                    let content = doc_inner(t);
                    if content.trim_start().starts_with("```") {
                        // closing fence
                        blocks.push(DocBlock { fence_line, setup, expects });
                        i += 1;
                        break;
                    }
                    if let Some(idx) = find_expect_marker(content) {
                        let expr = content[..idx].trim().to_string();
                        let expected = content[idx + EXPECT_MARKER.len()..].trim().to_string();
                        if !expr.is_empty() {
                            expects.push(DocExpect {
                                expr,
                                expected,
                                line: i + 1,
                            });
                        }
                    } else if !content.trim().is_empty() {
                        setup.push(content.trim_end().to_string());
                    }
                    i += 1;
                }
                continue;
            }
        }
        i += 1;
    }
    blocks
}

const EXPECT_MARKER: &str = "// =>";

/// Find the `// =>` marker, ignoring one inside a string literal. A doctest line
/// is short and simple; track whether we're inside a `"…"` to avoid a false hit.
fn find_expect_marker(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut in_str = false;
    let mut i = 0;
    while i + EXPECT_MARKER.len() <= bytes.len() {
        let c = bytes[i];
        if c == b'"' && (i == 0 || bytes[i - 1] != b'\\') {
            in_str = !in_str;
        }
        if !in_str && &s[i..i + EXPECT_MARKER.len()] == EXPECT_MARKER {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_doc_line(trimmed_line: &str) -> bool {
    trimmed_line.starts_with("///")
}

/// The text after the `///` prefix (with the single following space removed).
fn doc_inner(trimmed_line: &str) -> &str {
    let rest = &trimmed_line[3..];
    rest.strip_prefix(' ').unwrap_or(rest)
}

/// A `` ```jet `` (or bare `` ``` ``) opening fence is a Jet code fence. We accept
/// `jet` explicitly; a bare fence is also treated as Jet inside a `.jet` file.
fn fence_lang_is_jet(fence: &str) -> bool {
    let lang = fence.trim_start_matches('`').trim();
    lang.is_empty() || lang.eq_ignore_ascii_case("jet")
}

/// Build the synthetic Jet program for one doctest block: setup lines verbatim
/// inside `fn main()`, then one `print("{EXPR}")` per expectation (source order).
/// The program's stdout is one line per expectation, compared to the expected
/// values by the runner.
pub fn synth_program(block: &DocBlock) -> String {
    let mut out = String::new();
    out.push_str("fn main() {\n");
    for line in &block.setup {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    for e in &block.expects {
        // Render the expression through string interpolation so the output is
        // exactly its `JetShow` form — what `// =>` claims.
        out.push_str(&format!("    print(\"{{{}}}\")\n", e.expr));
    }
    out.push_str("}\n");
    out
}

/// E2901: a doctest expectation didn't match the produced value. `span` points at
/// the producing line in the original source so the report underlines the right
/// `// =>`.
pub fn mismatch_diag(file: &str, e: &DocExpect, actual: &str, span: Option<Span>) -> Diagnostic {
    let _ = file;
    Diagnostic::error(
        "E2901",
        format!(
            "doctest output mismatch. Expected: `{}` Got: `{}`",
            e.expected, actual
        ),
        "the example in the doc comment claims a different result from what the code produces; docs cannot lie (D-TEST4)".to_string(),
        "run `jet test --update-snapshots` to update the claimed output, or fix the code to match it".to_string(),
        span,
    )
}
