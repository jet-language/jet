//! Diagnostics: every user-facing error in the language flows through here.
//!
//! Contract (docs/spec/diagnostics.md): every Diagnostic has a stable code,
//! a `what` (one line, plain language), a `why` (the rule behind it), and
//! a `fix` (a concrete next step, copy-pasteable when possible). Typed rows
//! own those templates and static machine metadata; raise sites supply any
//! source-derived edit, and this module marshals the result into a report.
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
/// (`crates/jet-foundation/src/ExitCodes.rs`) — this macro never touches the exit code, only
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportMoment {
    Compile,
    Run,
    Test,
    Tool,
}

impl ReportMoment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compile => "compile",
            Self::Run => "run",
            Self::Test => "test",
            Self::Tool => "tool",
        }
    }
}

/// D-REPORT-MACHINE1: one machine report schema for every Jet surface.
pub const REPORT_SCHEMA: &str = "jet.report/v1";

/// Source nesting accepted by sema and the canonical TIR evaluator.
pub const MAX_SOURCE_NESTING: usize = 256;

/// How the renderer decides whether to emit ANSI color (E2-M3, D-DX*).
///
/// Resolution order, highest priority first:
///   1. `--color=always|never` (the flag always wins)
///   2. `NO_COLOR` presence forces off; otherwise `FORCE_COLOR` presence forces on
///   3. `auto`: color only when the target stream is a real terminal
///
/// Color never changes the bytes a script parses: it is suppressed whenever
/// the stream is piped, redirected, in CI, or `NO_COLOR` is set.
pub use crate::Terminal::ColorChoice;
use crate::Terminal::Theme;

/// A single-span text replacement (LSP quick-fix / M6 S14 autocorrect).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub span: Span,
    pub new_text: String,
}

/// Closed machine-readable reasons for E2702 (D-CRYPTO-DIAG1=A).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoMisuseReason {
    InvalidLength,
    NonceLength,
    OutputLength,
    SaltLength,
    MemoryCost,
    IterationCount,
    LaneCount,
    MemoryTimeCost,
    RawNonce,
    RawAlgorithm,
    DeterministicEntropy,
}

impl CryptoMisuseReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::InvalidLength => "invalid_length",
            Self::NonceLength => "nonce_length",
            Self::OutputLength => "output_length",
            Self::SaltLength => "salt_length",
            Self::MemoryCost => "memory_cost",
            Self::IterationCount => "iteration_count",
            Self::LaneCount => "lane_count",
            Self::MemoryTimeCost => "memory_time_cost",
            Self::RawNonce => "raw_nonce",
            Self::RawAlgorithm => "raw_algorithm",
            Self::DeterministicEntropy => "deterministic_entropy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructuredDiagnostic {
    CryptoMisuse {
        reason: CryptoMisuseReason,
        operation: &'static str,
        expected: Option<&'static str>,
        actual: Option<i128>,
    },
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub moment: ReportMoment,
    pub severity: Severity,
    pub code: String,
    pub what: String,
    pub why: String,
    pub fix: String,
    pub span: Option<Span>,
    /// Ordered report codes that caused this report. Root reports carry none.
    pub cause: Vec<String>,
    /// Mechanical fix projected from row metadata or authored from a
    /// source-derived fact at the diagnostic raise site (S14).
    pub edit: Option<TextEdit>,
    /// Extra indented detail (e.g. tool output for E0704).
    pub detail: Option<String>,
    /// Decision-owned machine fields. Human prose never gets parsed back into
    /// protocol data.
    pub structured: Option<StructuredDiagnostic>,
}

/// Control-flow sentinels used by the in-process evaluator and comptime
/// bridge. This namespace is internal; these values never become product
/// diagnostics or enter the registered diagnostic-code surface.
#[doc(hidden)]
pub mod internal {
    use super::{Diagnostic, Span};

    pub fn soft_exit(what: String, why: String, span: Option<Span>) -> Diagnostic {
        Diagnostic::internal_sentinel("SOFT_EXIT", what, why, span)
    }

    pub fn task_cancelled(span: Option<Span>) -> Diagnostic {
        Diagnostic::internal_sentinel(
            "TASK_CANCELLED",
            "task cancelled".to_string(),
            "the owning task group stopped this task".to_string(),
            span,
        )
    }
}

impl Diagnostic {
    /// E3403: ambient randomness or wall-clock state in pure evaluation.
    pub fn e3403(what: &str, span: Option<Span>) -> Self {
        Self::from_row("E3403", &[("what", what)], span)
    }

    /// Build a report from a typed row and its named hole values. The row
    /// supplies all product wording and any structured fix metadata.
    pub fn from_row(
        code: impl Into<String>,
        holes: &[(&str, &str)],
        span: Option<Span>,
    ) -> Self {
        let code = code.into();
        let row = crate::Registry::diagnostic(&code)
            .unwrap_or_else(|| panic!("diagnostic `{code}` has no typed row"));
        let rendered = row.render(holes);
        Diagnostic {
            moment: row.moment,
            severity: row.severity,
            code,
            what: rendered.what,
            why: rendered.why,
            fix: rendered.fix,
            span,
            cause: Vec::new(),
            edit: row_edit(row, span),
            detail: None,
            structured: None,
        }
    }

    /// Attach a dynamic edit whose kind is authorized by the typed row.
    /// Emitters may supply only the source-derived replacement text and span;
    /// the row still owns whether this report has a generated fix channel.
    pub fn set_structured_edit(&mut self, edit: TextEdit) {
        let row = crate::Registry::diagnostic(&self.code)
            .unwrap_or_else(|| panic!("diagnostic `{}` has no typed row", self.code));
        assert!(
            matches!(
                row.structured_fix,
                Some(
                    crate::Registry::StructuredFix::GeneratedMarkerGroup
                        | crate::Registry::StructuredFix::GeneratedMissingArms
                        | crate::Registry::StructuredFix::GeneratedScriptRun
                )
            ),
            "diagnostic `{}` has no row-owned generated structured fix",
            self.code
        );
        self.edit = Some(edit);
    }

    /// Build a report from the one typed row. The supplied strings are the
    /// row's already-filled dynamic values; code, severity, moment, and any
    /// row-declared structured fix still come from the typed row.
    pub fn error(
        code: impl Into<String>,
        what: String,
        why: String,
        fix: String,
        span: Option<Span>,
    ) -> Self {
        let code = code.into();
        let row = crate::Registry::diagnostic(&code)
            .unwrap_or_else(|| panic!("diagnostic `{code}` has no typed row"));
        Diagnostic {
            moment: row.moment,
            severity: row.severity,
            code,
            what,
            why,
            fix,
            span,
            cause: Vec::new(),
            edit: row_edit(row, span),
            detail: None,
            structured: None,
        }
    }

    /// Attach a source-derived edit authored by the checker at the diagnostic
    /// raise site. Human fix prose is presentation only.
    pub fn with_edit(mut self, edit: TextEdit) -> Self {
        self.edit = Some(edit);
        self
    }

    /// Build a compile-time error emitted by a programmable build rule.
    ///
    /// Build rules may use project-owned codes instead of the compiler's
    /// registry. Keep that escape checked here so the runtime bridge never
    /// constructs a report by hand.
    pub fn project_error(
        code: String,
        what: String,
        why: String,
        fix: String,
        span: Option<Span>,
    ) -> Result<Self, &'static str> {
        if code.is_empty() {
            return Err("custom diagnostic code must not be empty");
        }
        if code.chars().any(char::is_control) {
            return Err("custom diagnostic code must not contain control characters");
        }
        let row = crate::Registry::diagnostic(&code)
            .unwrap_or_else(|| panic!("diagnostic `{code}` has no typed row"));
        Ok(Self {
            moment: row.moment,
            severity: row.severity,
            code,
            what,
            why,
            fix,
            span,
            cause: Vec::new(),
            edit: row_edit(row, span),
            detail: None,
            structured: None,
        })
    }

    /// The TIR interpreter's internal control-flow sentinels: each unwinds a
    /// `Result<_, Diagnostic>` cleanly out of `eval_expr`/a task worker without
    /// ever meaning to reach a renderer (`SOFT_EXIT`: a `panic`/`require`/
    /// contract failure or an `E3005` trap, after already writing the rendered
    /// message and exit code into the shared `DevSink` — Source/Interpreter.rs
    /// and jet-jit/src/jit/deopt.rs both special-case `code == "SOFT_EXIT"`;
    /// `TASK_CANCELLED`: a task's wait point observed a pending cancel).
    /// Deliberately NOT registered rows — they must never surface as a real
    /// user-facing diagnostic — so this skips `error()`'s registry lookup
    /// instead of panicking on the code every one of them would otherwise
    /// trigger by construction.
    fn internal_sentinel(code: &'static str, what: String, why: String, span: Option<Span>) -> Self {
        Diagnostic {
            moment: ReportMoment::Run,
            severity: Severity::Error,
            code: code.to_string(),
            what,
            why,
            fix: String::new(),
            span,
            cause: Vec::new(),
            edit: None,
            detail: None,
            structured: None,
        }
    }

    /// See `internal_sentinel`.
    pub fn soft_exit(what: String, why: String, span: Option<Span>) -> Self {
        Self::internal_sentinel("SOFT_EXIT", what, why, span)
    }

    /// See `internal_sentinel`.
    pub fn task_cancelled(span: Option<Span>) -> Self {
        Self::internal_sentinel(
            "TASK_CANCELLED",
            "task cancelled".to_string(),
            "the owning task group stopped this task".to_string(),
            span,
        )
    }

    /// D-LAYOUT-FACTS1=B: physical byte values are optional facts. Tooling
    /// uses this registered diagnostic when the typed layout model carries a
    /// clean absence instead of printing an unexplained `unknown` value.
    pub fn layout_byte_facts_unavailable(
        type_name: &str,
        member: &str,
        span: Option<Span>,
    ) -> Self {
        Self::from_row(
            "E0959",
            &[("type", type_name), ("member", member)],
            span,
        )
    }

    pub fn source_nesting_exceeded(depth: usize, span: Span) -> Self {
        let depth = depth.to_string();
        Self::from_row(
            "E1403",
            &[("depth", &depth)],
            Some(span),
        )
    }

    pub fn lint(
        code: impl Into<String>,
        what: String,
        why: String,
        fix: String,
        span: Option<Span>,
    ) -> Self {
        let code = code.into();
        let row = crate::Registry::diagnostic(&code)
            .unwrap_or_else(|| panic!("diagnostic `{code}` has no typed row"));
        Diagnostic {
            moment: row.moment,
            severity: row.severity,
            code,
            what,
            why,
            fix,
            span,
            cause: Vec::new(),
            edit: row_edit(row, span),
            detail: None,
            structured: None,
        }
    }

    pub fn with_detail(mut self, detail: String) -> Self {
        self.detail = Some(detail);
        self
    }

    /// Attach the ordered report-code chain that caused this report.
    pub fn with_causes(mut self, cause: Vec<String>) -> Self {
        self.cause = cause;
        self
    }

    /// Put the nearest wrapped report first, followed by its own causes.
    pub fn caused_by(mut self, cause: &Self) -> Self {
        self.cause.reserve(cause.cause.len() + 1);
        self.cause.push(cause.code.clone());
        self.cause.extend(cause.cause.iter().cloned());
        self
    }

    pub fn crypto_misuse(
        why: String,
        fix: String,
        span: Span,
        reason: CryptoMisuseReason,
        operation: &'static str,
        expected: &'static str,
        actual: i128,
    ) -> Self {
        let mut diagnostic = Self::from_row(
            "E2702",
            &[("why", why.as_str()), ("fix", fix.as_str())],
            Some(span),
        );
        assert_eq!(
            crate::Registry::diagnostic("E2702")
                .and_then(|row| row.structured_fix),
            Some(crate::Registry::StructuredFix::CryptoMisuse),
            "E2702 must keep its typed crypto structured fix"
        );
        diagnostic.structured = Some(StructuredDiagnostic::CryptoMisuse {
            reason,
            operation,
            expected: Some(expected),
            actual: Some(actual),
        });
        diagnostic
    }

    pub fn crypto_misuse_fact(
        why: String,
        fix: String,
        span: Span,
        reason: CryptoMisuseReason,
        operation: &'static str,
    ) -> Self {
        let mut diagnostic = Self::from_row(
            "E2702",
            &[("why", why.as_str()), ("fix", fix.as_str())],
            Some(span),
        );
        assert_eq!(
            crate::Registry::diagnostic("E2702")
                .and_then(|row| row.structured_fix),
            Some(crate::Registry::StructuredFix::CryptoMisuse),
            "E2702 must keep its typed crypto structured fix"
        );
        diagnostic.structured = Some(StructuredDiagnostic::CryptoMisuse {
            reason,
            operation,
            expected: None,
            actual: None,
        });
        diagnostic
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
        let theme = Theme::new(color);
        let mut out = String::new();
        let label = match self.severity {
            Severity::Error => theme.error("Error"),
            Severity::Lint => theme.warn("Warning"),
        };
        if self.severity == Severity::Lint {
            if let Some(name) = crate::LintPolicy::name_for_code(&self.code) {
                out.push_str(&format!("{} [{}] ({}): {}\n", label, self.code, name, self.what));
            } else {
                out.push_str(&format!("{} [{}]: {}\n", label, self.code, self.what));
            }
        } else {
            out.push_str(&format!("{} [{}]: {}\n", label, self.code, self.what));
        }
        if let Some(span) = self.span {
            let (line, col) = line_col(src, span.start);
            let loc = format!("--> {}:{}:{}", file, line, col);
            let loc = if hyperlinks {
                osc8(&file_url(file, line, col), &loc)
            } else {
                loc
            };
            out.push_str(&format!("  {}\n", theme.dim(&loc)));
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
                out.push_str(&format!("    | {}{}\n", pad, theme.error(marks)));
            } else {
                out.push_str(&format!("    | {}\n", carets));
            }
        }
        out.push_str(&format!(" {} {}\n", theme.bold("Why:"), self.why));
        out.push_str(&format!(" {} {}\n", theme.bold("Fix:"), self.fix));
        if let Some(detail) = &self.detail {
            out.push_str(detail);
            if !detail.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }

    /// Render this diagnostic as one `jet.report/v1` JSON object.
    /// Hand-rolled (invariant I6: no serde). The shape is:
    ///
    /// ```json
    /// {
    ///   "schema": "jet.report/v1", "moment": "compile",
    ///   "severity": "error", "code": "E0102", "what": "…",
    ///   "why": "…", "fix": "…",
    ///   "detail": "…" | null,
    ///   "file": "a.jet", "line": 2, "col": 5,
    ///   "span": { "start": 12, "end": 17 } | null,
    ///   "fix_edits": [{ "file": "a.jet", "span": {…}, "new_text": "…" }],
    ///   "cause": ["E0109", "E0108"],
    ///   "clears": 2
    /// }
    /// ```
    ///
    /// `fix_edits` holds the machine-applicable fix the LSP / fix engine consumes.
    pub fn to_json(&self, file: &str, src: &str) -> String {
        self.to_json_with_clears(file, src, 0)
    }

    /// Render one report with its batch-derived dependent count.
    pub fn to_json_with_clears(&self, file: &str, src: &str, clears: usize) -> String {
        let report_file = file;
        let mut o = String::from("{");
        o.push_str(&format!("\"schema\":{}", json_str(REPORT_SCHEMA)));
        o.push_str(&format!(",\"moment\":{}", json_str(self.moment.as_str())));
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Lint => "warning",
        };
        o.push_str(&format!(",\"severity\":{}", json_str(sev)));
        o.push_str(&format!(",\"code\":{}", json_str(&self.code)));
        o.push_str(&format!(",\"what\":{}", json_str(&self.what)));
        o.push_str(&format!(",\"why\":{}", json_str(&self.why)));
        o.push_str(&format!(",\"fix\":{}", json_str(&self.fix)));
        match &self.detail {
            Some(d) => o.push_str(&format!(",\"detail\":{}", json_str(d))),
            None => o.push_str(",\"detail\":null"),
        }
        if report_file.is_empty() {
            o.push_str(",\"file\":null");
        } else {
            o.push_str(&format!(",\"file\":{}", json_str(&report_file)));
        }
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
        o.push_str(",\"fix_edits\":[");
        match &self.edit {
            Some(e) => {
                o.push_str(&format!(
                    "{{\"file\":{},\"span\":{{\"start\":{},\"end\":{}}},\"new_text\":{}}}",
                    json_str(&report_file),
                    e.span.start,
                    e.span.end,
                    json_str(&e.new_text)
                ));
            }
            None => {}
        }
        o.push_str("],\"cause\":[");
        for (index, cause) in self.cause.iter().enumerate() {
            if index > 0 {
                o.push(',');
            }
            o.push_str(&json_str(cause));
        }
        o.push(']');
        o.push_str(&format!(",\"clears\":{clears}"));
        if let Some(StructuredDiagnostic::CryptoMisuse {
            reason,
            operation,
            expected,
            actual,
        }) = &self.structured
        {
            o.push_str(&format!(",\"reason\":{}", json_str(reason.as_str())));
            o.push_str(&format!(",\"operation\":{}", json_str(operation)));
            if let Some(expected) = expected {
                o.push_str(&format!(",\"expected\":{}", json_str(expected)));
            }
            if let Some(actual) = actual {
                o.push_str(&format!(",\"actual\":{actual}"));
            }
        }
        o.push('}');
        o
    }
}

fn row_edit(row: &crate::Registry::DiagnosticRow, span: Option<Span>) -> Option<TextEdit> {
    let span = span?;
    match row.structured_fix? {
        crate::Registry::StructuredFix::Replace { to, .. } => Some(TextEdit {
            span,
            new_text: to.to_string(),
        }),
        crate::Registry::StructuredFix::Remove { .. } => Some(TextEdit {
            span,
            new_text: String::new(),
        }),
        crate::Registry::StructuredFix::CryptoMisuse
        | crate::Registry::StructuredFix::GeneratedMarkerGroup
        | crate::Registry::StructuredFix::GeneratedMissingArms
        | crate::Registry::StructuredFix::GeneratedScriptRun => None,
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

/// Render reports as JSON Lines. An empty batch emits no bytes.
pub fn render_all_json(file: &str, src: &str, diags: &[Diagnostic]) -> String {
    let mut out = String::new();
    let clears = report_clear_counts(diags);
    for (d, clears) in diags.iter().zip(clears) {
        out.push_str(&d.to_json_with_clears(file, src, clears));
        out.push('\n');
    }
    out
}

/// Render the explicit success result for a clean --json check.
pub fn render_success_json(file: &str) -> String {
    format!(
        "{{\"schema\":{},\"moment\":\"compile\",\"status\":\"ok\",\"ok\":true,\"diagnostics\":[],\"file\":{}}}\n",
        json_str(REPORT_SCHEMA),
        json_str(file),
    )
}

/// Count reports whose explicit cause chain names each report. A transitive
/// dependent counts once, so a consumer can rank root reports without
/// rebuilding the graph from report text.
pub fn report_clear_counts(diags: &[Diagnostic]) -> Vec<usize> {
    diags
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            diags
                .iter()
                .enumerate()
                .filter(|(dependent_index, dependent)| {
                    *dependent_index != index
                        && dependent
                            .cause
                            .iter()
                            .any(|cause| cause == &diagnostic.code)
                })
                .count()
        })
        .collect()
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

fn unicode_range_value(table: &[(u32, u32, u8)], cp: u32) -> u8 {
    table
        .binary_search_by(|&(start, end, _)| {
            if cp < start {
                std::cmp::Ordering::Greater
            } else if cp > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .map(|at| table[at].2)
        .unwrap_or(0)
}

fn unicode_range_contains(table: &[(u32, u32)], cp: u32) -> bool {
    table
        .binary_search_by(|&(start, end)| {
            if cp < start {
                std::cmp::Ordering::Greater
            } else if cp > end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn unicode_general_category(cp: u32) -> u8 {
    unicode_range_value(
        crate::generated::UnicodeTables::UNICODE_GENERAL_CATEGORY,
        cp,
    )
}

fn unicode_grapheme_class(cp: u32) -> u8 {
    unicode_range_value(
        crate::generated::UnicodeTables::UNICODE_GRAPHEME_BREAK,
        cp,
    )
}

fn unicode_grapheme_break(previous: u8, current: u8) -> bool {
    const CR: u8 = 1;
    const LF: u8 = 2;
    const CONTROL: u8 = 3;
    const EXTEND: u8 = 4;
    const ZWJ: u8 = 5;
    const PREPEND: u8 = 7;
    const SPACING_MARK: u8 = 8;
    const L: u8 = 9;
    const V: u8 = 10;
    const T: u8 = 11;
    const LV: u8 = 12;
    const LVT: u8 = 13;

    if previous == CR && current == LF {
        return false;
    }
    if matches!(previous, CR | LF | CONTROL) || matches!(current, CR | LF | CONTROL) {
        return true;
    }
    if previous == L && matches!(current, L | V | LV | LVT) {
        return false;
    }
    if matches!(previous, LV | V) && matches!(current, V | T) {
        return false;
    }
    if matches!(previous, LVT | T) && current == T {
        return false;
    }
    !matches!(current, EXTEND | ZWJ | SPACING_MARK) && previous != PREPEND
}

fn diagnostic_graphemes(s: &str) -> Vec<&str> {
    const ZWJ: u8 = 5;
    const RI: u8 = 6;
    const EXTEND: u8 = 4;
    const INCB_LINKER: u8 = 1;
    const INCB_CONSONANT: u8 = 2;
    const INCB_EXTEND: u8 = 3;

    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let Some(&(_, first)) = chars.first() else {
        return Vec::new();
    };
    let is_pictographic = |cp| {
        unicode_range_contains(
            crate::generated::UnicodeTables::UNICODE_EXTENDED_PICTOGRAPHIC,
            cp,
        )
    };
    let incb = |cp| {
        unicode_range_value(crate::generated::UnicodeTables::UNICODE_INCB, cp)
    };
    let mut starts = vec![0];
    let mut ri_run = usize::from(unicode_grapheme_class(first as u32) == RI);
    let mut saw_pictographic = is_pictographic(first as u32);
    let mut incb_pending = incb(first as u32) == INCB_CONSONANT;
    let mut incb_linker = false;
    for index in 1..chars.len() {
        let (_, previous) = chars[index - 1];
        let (byte, current) = chars[index];
        let previous_class = unicode_grapheme_class(previous as u32);
        let current_class = unicode_grapheme_class(current as u32);
        let is_break = if previous_class == ZWJ
            && saw_pictographic
            && is_pictographic(current as u32)
        {
            false
        } else if previous_class == RI && current_class == RI {
            ri_run % 2 == 0
        } else if incb_pending
            && incb_linker
            && incb(current as u32) == INCB_CONSONANT
        {
            false
        } else {
            unicode_grapheme_break(previous_class, current_class)
        };
        if is_break {
            starts.push(byte);
        }
        ri_run = if current_class == RI { ri_run + 1 } else { 0 };
        saw_pictographic = if matches!(current_class, EXTEND | ZWJ) {
            saw_pictographic
        } else {
            is_pictographic(current as u32)
        };
        match incb(current as u32) {
            INCB_CONSONANT => {
                incb_pending = true;
                incb_linker = false;
            }
            INCB_LINKER if incb_pending => incb_linker = true,
            INCB_EXTEND | INCB_LINKER => {}
            _ => {
                incb_pending = false;
                incb_linker = false;
            }
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| &s[*start..starts.get(index + 1).copied().unwrap_or(s.len())])
        .collect()
}

fn diagnostic_cluster_width(cluster: &str) -> usize {
    let codepoints: Vec<char> = cluster.chars().collect();
    let Some(&base) = codepoints.first() else {
        return 0;
    };
    if unicode_general_category(base as u32) == 0 {
        return 0;
    }
    let keycap = matches!(
        codepoints.as_slice(),
        ['0'..='9' | '#' | '*', '\u{20E3}']
            | ['0'..='9' | '#' | '*', '\u{FE0F}', '\u{20E3}']
    );
    let ri_pair = codepoints.len() >= 2
        && unicode_grapheme_class(codepoints[0] as u32) == 6
        && unicode_grapheme_class(codepoints[1] as u32) == 6;
    let emoji_style = cluster.contains('\u{FE0F}')
        && codepoints.iter().any(|c| {
            unicode_range_contains(
                crate::generated::UnicodeTables::UNICODE_EMOJI,
                *c as u32,
            )
        });
    let wide = codepoints.iter().any(|c| {
        unicode_range_value(
            crate::generated::UnicodeTables::UNICODE_EAST_ASIAN_WIDTH,
            *c as u32,
        ) == 2
            || unicode_range_contains(
                crate::generated::UnicodeTables::UNICODE_EMOJI_PRESENTATION,
                *c as u32,
            )
    });
    if keycap || ri_pair || emoji_style || wide {
        return 2;
    }
    if codepoints.iter().all(|c| {
        matches!(unicode_general_category(*c as u32), 2 | 3)
            || unicode_range_contains(
                crate::generated::UnicodeTables::UNICODE_DEFAULT_IGNORABLE,
                *c as u32,
            )
    }) {
        return 0;
    }
    1
}

/// Terminal display width using the pinned Unicode release and core.text's
/// portable-default grapheme-cluster policy (D-TEXTWIDTH1=B).
pub fn display_width(s: &str) -> usize {
    diagnostic_graphemes(s)
        .into_iter()
        .map(diagnostic_cluster_width)
        .sum()
}

/// Portable-default width of one Unicode scalar from the same pinned tables.
pub fn display_char_width(c: char) -> usize {
    let cp = c as u32;
    if unicode_general_category(cp) == 0
        || matches!(unicode_general_category(cp), 2 | 3)
        || unicode_range_contains(
            crate::generated::UnicodeTables::UNICODE_DEFAULT_IGNORABLE,
            cp,
        )
    {
        return 0;
    }
    if unicode_range_value(
        crate::generated::UnicodeTables::UNICODE_EAST_ASIAN_WIDTH,
        cp,
    ) == 2
        || unicode_range_contains(
            crate::generated::UnicodeTables::UNICODE_EMOJI_PRESENTATION,
            cp,
        )
    {
        return 2;
    }
    1
}

/// Render a batch of diagnostics, blank line between each.
pub fn render_all(file: &str, src: &str, diags: &[Diagnostic]) -> String {
    let rendered: Vec<String> = diags.iter().map(|d| d.render(file, src)).collect();
    rendered.join("\n")
}

/// I2: the one home for the branded ICE report. Every internal-compiler-error
/// site — the uncaught-panic hook (`install_ice_panic_hook`) and every
/// `Source/CmdCompile.rs` site that used to hand-type its own banner —
/// renders through here, so there is exactly one phrasing.
///
/// `what` is the one-line description that follows the fixed
/// "internal compiler error: " prefix. `detail` is optional extra context
/// (a generated-file path, rustc's stderr, a panic location) appended below
/// the fixed body; pass `""` to omit it. `generated_file_attached` says
/// whether a generated Rust file actually exists on disk for the user to
/// attach — only rustc-rejection call sites (which write the file before
/// invoking rustc) may pass `true`; call sites with no codegen output yet,
/// or an in-process panic with no generated file at all, pass `false` so
/// the report never promises an attachment that doesn't exist.
pub fn render_ice_report(what: &str, detail: &str, generated_file_attached: bool) -> String {
    let file_clause = if generated_file_attached {
        " and the generated file below"
    } else {
        ""
    };
    let mut report = format!(
        "internal compiler error: {what}\n\
         This is a bug in {bin}, NOT in your program. Please report it,\n\
         attaching your source file{file_clause}.",
        bin = crate::Syntax::BINARY_NAME,
    );
    if !detail.is_empty() {
        report.push('\n');
        report.push_str(detail);
    }
    report
}

/// I2: installs the one process-wide panic hook so an uncaught panic
/// anywhere in the jet binary prints the branded `render_ice_report` output
/// instead of Rust's raw panic text (no `thread 'main' panicked at`), then
/// exits 101 (`ExitCodes::ICE`) via Rust's default unhandled-panic behavior —
/// this never touches the exit code, only the message. Call this first thing
/// in `main()`, before any other work. When `RUST_BACKTRACE` is set, a
/// captured backtrace is still printed below the branded report, so the env
/// var keeps working even though the default hook is replaced.
///
/// `JET_ICE_SELF_TEST=1` deliberately panics right after the hook installs,
/// so `tests/ice_report_single_home.rs` can prove the hook's real output on
/// the actual `jet` binary. Gated to debug builds only — a release binary
/// must never ship a live panic-on-env-var backdoor.
pub fn install_ice_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let backtrace_wanted = std::env::var_os("RUST_BACKTRACE").is_some_and(|v| v != "0");
        let backtrace = backtrace_wanted.then(std::backtrace::Backtrace::force_capture);
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        // The `ice!` macro (this module) already prefixes its message with
        // "internal compiler error: "; strip it so it isn't doubled.
        let what = what
            .strip_prefix("internal compiler error: ")
            .map(str::to_string)
            .unwrap_or(what);
        let mut detail = info
            .location()
            .map(|loc| format!("  at {}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_default();
        if let Some(bt) = backtrace {
            if !detail.is_empty() {
                detail.push('\n');
            }
            detail.push_str(&bt.to_string());
        }
        eprintln!("{}", render_ice_report(&what, &detail, false));
    }));

    #[cfg(debug_assertions)]
    if std::env::var_os("JET_ICE_SELF_TEST").is_some() {
        panic!("ICE self-test triggered by JET_ICE_SELF_TEST");
    }
}

#[cfg(test)]
mod unicode_display_width_tests {
    use super::*;

    #[test]
    fn diagnostics_use_pinned_grapheme_display_width() {
        assert_eq!(display_width("e\u{301}"), 1);
        assert_eq!(display_width("🇺🇸"), 2);
        assert_eq!(display_width("👨‍👩‍👧‍👦"), 2);
        assert_eq!(display_width("1️⃣"), 2);
        assert_eq!(display_width("©️"), 2);
        assert_eq!(display_width("\u{301}\u{200D}"), 0);
        assert_eq!(display_width("界"), 2);
    }
}

#[cfg(test)]
mod crypto_diagnostic_contract_tests {
    use super::*;

    #[test]
    fn e2702_json_is_one_closed_redacted_object() {
        let diagnostic = Diagnostic::crypto_misuse(
            "HKDF-SHA256 output length is 8161 bytes; this operation requires 0..8160".into(),
            "pass an output length from 0 through 8160 bytes".into(),
            Span::new(4, 8),
            CryptoMisuseReason::OutputLength,
            "hkdf_sha256",
            "0..8160",
            8161,
        );
        let json = render_all_json("secret-name.jet", "xxxx8161", &[diagnostic]);
        assert_eq!(
            json,
            concat!(
                "{\"schema\":\"jet.report/v1\",\"moment\":\"compile\",",
                "\"severity\":\"error\",\"code\":\"E2702\",",
                "\"what\":\"crypto API misuse\",",
                "\"why\":\"HKDF-SHA256 output length is 8161 bytes; this operation requires 0..8160\",",
                "\"fix\":\"pass an output length from 0 through 8160 bytes\",",
                "\"detail\":null,\"file\":\"secret-name.jet\",\"line\":1,\"col\":5,",
                "\"span\":{\"start\":4,\"end\":8},\"fix_edits\":[],\"cause\":[],\"clears\":0,",
                "\"reason\":\"output_length\",\"operation\":\"hkdf_sha256\",",
                "\"expected\":\"0..8160\",\"actual\":8161}\n"
            )
        );
        for forbidden in ["password", "plaintext", "ciphertext", "backend", "rustc", "dependency"] {
            assert!(!json.contains(forbidden), "leaked `{forbidden}`: {json}");
        }
    }

    #[test]
    fn ordinary_diagnostics_use_report_json_lines() {
        let diagnostic = Diagnostic::error(
            "E0001",
            "bad token".into(),
            "grammar".into(),
            "remove it".into(),
            None,
        );
        let json = render_all_json("x.jet", "", &[diagnostic]);
        assert!(json.starts_with("{\"schema\":\"jet.report/v1\",\"moment\":\"compile\""));
        assert_eq!(json.lines().count(), 1);
        assert!(crate::JSON::parse_json(json.trim_end()).is_ok());
        assert_eq!(render_all_json("x.jet", "", &[]), "");
    }

    #[test]
    fn report_json_preserves_a_multi_link_cause_chain() {
        let root = Diagnostic::error(
            "E0109",
            "root".into(),
            "test".into(),
            "fix root".into(),
            None,
        );
        let middle = Diagnostic::error(
            "E0108",
            "middle".into(),
            "test".into(),
            "fix middle".into(),
            None,
        )
        .caused_by(&root);
        let leaf = Diagnostic::error(
            "E0107",
            "leaf".into(),
            "test".into(),
            "fix leaf".into(),
            None,
        )
        .caused_by(&middle);
        let sibling = Diagnostic::error(
            "E0001",
            "sibling".into(),
            "test".into(),
            "fix sibling".into(),
            None,
        )
        .caused_by(&root);
        let json = render_all_json("x.jet", "", &[root, middle, leaf, sibling]);
        let lines = json.lines().collect::<Vec<_>>();
        assert!(lines[0].contains("\"cause\":[],\"clears\":3"), "{json}");
        assert!(lines[1].contains("\"cause\":[\"E0109\"],\"clears\":1"), "{json}");
        assert!(
            lines[2].contains("\"cause\":[\"E0108\",\"E0109\"],\"clears\":0"),
            "{json}"
        );
        assert!(lines[3].contains("\"cause\":[\"E0109\"],\"clears\":0"), "{json}");
    }

    #[test]
    fn constructors_take_metadata_from_the_typed_row() {
        let error = Diagnostic::from_row("E0102", &[], None);
        assert_eq!(error.severity, Severity::Error);
        assert_eq!(error.moment, ReportMoment::Compile);

        let lint = Diagnostic::from_row("L2001", &[], None);
        assert_eq!(lint.severity, Severity::Lint);
        assert_eq!(lint.moment, ReportMoment::Compile);
    }

    #[test]
    fn structured_edit_comes_from_the_row_not_fix_prose() {
        let diagnostic = Diagnostic::from_row("E0373", &[], Some(Span::new(3, 4)));
        assert_eq!(
            diagnostic.edit.as_ref().map(|edit| edit.new_text.as_str()),
            Some(",")
        );
    }

    #[test]
    fn construction_edit_survives_fix_rewording() {
        let span = Span::new(4, 9);
        let edit = TextEdit {
            span,
            new_text: "print".to_string(),
        };
        let original = Diagnostic::error(
            "E0102",
            "nothing named `pirnt` exists here".to_string(),
            "only known functions can be called".to_string(),
            "did you mean `print`?".to_string(),
            Some(span),
        )
        .with_edit(edit.clone());
        let reworded = Diagnostic::error(
            "E0102",
            "nothing named `pirnt` exists here".to_string(),
            "only known functions can be called".to_string(),
            "try `print` instead".to_string(),
            Some(span),
        )
        .with_edit(edit);
        assert_eq!(original.edit, reworded.edit);
    }

    #[test]
    fn migrated_rows_render_named_holes_without_a_second_message() {
        let lint = Diagnostic::from_row(
            "L0503",
            &[("place", "total"), ("op", "+=")],
            Some(Span::new(0, 5)),
        );
        assert_eq!(
            lint.what,
            "prefer `total += …` instead of repeating the left side"
        );
        assert_eq!(lint.fix, "write `total += …`");

        let unsupported = crate::Prelude::jet_e0956_unsupported("a compiler fact", Span::new(0, 1));
        assert_eq!(unsupported.what, "`a compiler fact` can't run at compile time yet");
    }

    #[test]
    fn every_crypto_reason_is_closed_and_batches_keep_structured_fields() {
        let reasons = [
            (CryptoMisuseReason::InvalidLength, "invalid_length"),
            (CryptoMisuseReason::NonceLength, "nonce_length"),
            (CryptoMisuseReason::OutputLength, "output_length"),
            (CryptoMisuseReason::SaltLength, "salt_length"),
            (CryptoMisuseReason::MemoryCost, "memory_cost"),
            (CryptoMisuseReason::IterationCount, "iteration_count"),
            (CryptoMisuseReason::LaneCount, "lane_count"),
            (CryptoMisuseReason::MemoryTimeCost, "memory_time_cost"),
            (CryptoMisuseReason::RawNonce, "raw_nonce"),
            (CryptoMisuseReason::RawAlgorithm, "raw_algorithm"),
            (CryptoMisuseReason::DeterministicEntropy, "deterministic_entropy"),
        ];
        for (reason, spelling) in reasons {
            assert_eq!(reason.as_str(), spelling);
        }

        let diagnostic = || {
            Diagnostic::crypto_misuse(
                "invalid length".into(),
                "pass the required length".into(),
                Span::new(0, 1),
                CryptoMisuseReason::InvalidLength,
                "x25519",
                "exactly 32",
                31,
            )
        };
        let json = render_all_json("x.jet", "x", &[diagnostic(), diagnostic()]);
        assert_eq!(json.lines().count(), 2);
        assert_eq!(json.matches("\"schema\":\"jet.report/v1\"").count(), 2);
        assert_eq!(json.matches("\"reason\":\"invalid_length\"").count(), 2);
    }

    #[test]
    fn mixed_batches_preserve_order_and_one_schema() {
        let generic = Diagnostic::error(
            "E0001",
            "bad token".into(),
            "grammar".into(),
            "remove it".into(),
            None,
        );
        let crypto = Diagnostic::crypto_misuse(
            "invalid length".into(),
            "pass the required length".into(),
            Span::new(0, 1),
            CryptoMisuseReason::InvalidLength,
            "x25519",
            "exactly 32",
            31,
        );
        let json = render_all_json("x.jet", "x", &[crypto, generic]);
        let lines = json.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2, "{json}");
        assert!(lines.iter().all(|line| crate::JSON::parse_json(line).is_ok()));
        assert!(lines[0].contains("\"schema\":\"jet.report/v1\"") && lines[0].contains("\"code\":\"E2702\""));
        assert!(lines[1].contains("\"schema\":\"jet.report/v1\"") && lines[1].contains("\"code\":\"E0001\""));
    }
}
