// Shared machine-report value model.
//
// This file is dependency-free because the AOT test harness includes the
// same source. CLI, LSP, Foundation JSON, and generated test reports all
// serialize this envelope instead of maintaining separate protocol shapes.

pub const REPORT_SCHEMA: &str = "jet.report/v1";

/// D-REPORT-FIXGRADE1=D: closed safety classes for machine edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixSafety {
    Formatting,
    BehaviorPreserving,
    ApiChanging,
    TargetChanging,
    NeedsReview,
}

impl FixSafety {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Formatting => "formatting",
            Self::BehaviorPreserving => "behavior-preserving",
            Self::ApiChanging => "api-changing",
            Self::TargetChanging => "target-changing",
            Self::NeedsReview => "needs-review",
        }
    }

    pub const fn auto_apply(self) -> bool {
        matches!(self, Self::Formatting | Self::BehaviorPreserving)
    }
}

/// D-REPORT-FIXGRADE1=D: the promise attached to a machine-applicable edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixApplicability {
    /// Applying every edit with this grade is proved to make report progress.
    Safe,
    /// The edit is useful to show, but a person must choose whether to apply it.
    Suggested,
}

impl FixApplicability {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Suggested => "suggested",
        }
    }
}

/// A source identity suitable for a report consumer to open from its process.
///
/// Relative disk paths are made absolute lexically. No manifest or `.git`
/// search is involved, and the source does not need to exist yet. Synthetic
/// labels such as `<cli>` remain labels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportPath(String);

impl ReportPath {
    fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn from_process(value: &str) -> Self {
        if value.is_empty() || value.starts_with('<') {
            return Self::new(value);
        }
        Self::from_path(std::path::Path::new(value))
    }

    pub fn from_path(path: &std::path::Path) -> Self {
        let display = path.to_string_lossy();
        if display.is_empty() || display.starts_with('<') {
            return Self::new(display.into_owned());
        }
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            match std::env::current_dir() {
                Ok(cwd) => cwd.join(path),
                Err(_) => return Self::new(display.into_owned()),
            }
        };
        Self::new(
            lexically_normalize(&absolute)
                .to_string_lossy()
                .into_owned(),
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl AsRef<str> for ReportPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for ReportPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn lexically_normalize(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportSpan {
    pub start: usize,
    pub end: usize,
}

impl ReportSpan {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportEdit {
    pub file: ReportPath,
    pub span: ReportSpan,
    pub new_text: String,
    pub safety: FixSafety,
}

impl ReportEdit {
    pub fn new(
        file: ReportPath,
        span: ReportSpan,
        new_text: impl Into<String>,
        safety: FixSafety,
    ) -> Self {
        Self {
            file,
            span,
            new_text: new_text.into(),
            safety,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReportExtension {
    Crypto {
        reason: String,
        operation: String,
        expected: Option<String>,
        actual: Option<i128>,
    },
}

/// The one `jet.report/v1` envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportEnvelope {
    pub moment: String,
    pub severity: String,
    pub code: String,
    pub what: String,
    pub why: String,
    pub fix: String,
    pub applicability: Option<FixApplicability>,
    pub detail: Option<String>,
    pub file: Option<ReportPath>,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub span: Option<ReportSpan>,
    pub fix_edits: Vec<ReportEdit>,
    pub cause: Vec<String>,
    pub clears: usize,
    pub extension: Option<ReportExtension>,
}

impl ReportEnvelope {
    pub fn new(
        moment: impl Into<String>,
        severity: impl Into<String>,
        code: impl Into<String>,
        what: impl Into<String>,
        why: impl Into<String>,
        fix: impl Into<String>,
    ) -> Self {
        Self {
            moment: moment.into(),
            severity: severity.into(),
            code: code.into(),
            what: what.into(),
            why: why.into(),
            fix: fix.into(),
            applicability: None,
            detail: None,
            file: None,
            line: None,
            col: None,
            span: None,
            fix_edits: Vec::new(),
            cause: Vec::new(),
            clears: 0,
            extension: None,
        }
    }

    pub fn json(&self) -> String {
        let mut out = String::from("{");
        out.push_str("\"schema\":");
        out.push_str(&report_json_string(REPORT_SCHEMA));
        out.push_str(",\"moment\":");
        out.push_str(&report_json_string(&self.moment));
        out.push_str(",\"severity\":");
        out.push_str(&report_json_string(&self.severity));
        out.push_str(",\"code\":");
        out.push_str(&report_json_string(&self.code));
        out.push_str(",\"what\":");
        out.push_str(&report_json_string(&self.what));
        out.push_str(",\"why\":");
        out.push_str(&report_json_string(&self.why));
        out.push_str(",\"fix\":");
        out.push_str(&report_json_string(&self.fix));
        if let Some(applicability) = self.applicability {
            out.push_str(",\"applicability\":");
            out.push_str(&report_json_string(applicability.as_str()));
        }
        out.push_str(",\"detail\":");
        match &self.detail {
            Some(detail) => out.push_str(&report_json_string(detail)),
            None => out.push_str("null"),
        }
        out.push_str(",\"file\":");
        match &self.file {
            Some(file) if !file.is_empty() => out.push_str(&report_json_string(file.as_str())),
            _ => out.push_str("null"),
        }
        out.push_str(",\"line\":");
        push_optional_usize(&mut out, self.line);
        out.push_str(",\"col\":");
        push_optional_usize(&mut out, self.col);
        out.push_str(",\"span\":");
        match self.span {
            Some(span) => out.push_str(&format!(
                "{{\"start\":{},\"end\":{}}}",
                span.start, span.end
            )),
            None => out.push_str("null"),
        }
        out.push_str(",\"fix_edits\":[");
        for (index, edit) in self.fix_edits.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"file\":{},\"span\":{{\"start\":{},\"end\":{}}},\"new_text\":{},\"safety\":{}}}",
                report_json_string(edit.file.as_str()),
                edit.span.start,
                edit.span.end,
                report_json_string(&edit.new_text),
                report_json_string(edit.safety.as_str()),
            ));
        }
        out.push_str("],\"cause\":[");
        for (index, cause) in self.cause.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&report_json_string(cause));
        }
        out.push_str("]");
        out.push_str(&format!(",\"clears\":{}", self.clears));
        if let Some(ReportExtension::Crypto {
            reason,
            operation,
            expected,
            actual,
        }) = &self.extension
        {
            out.push_str(",\"reason\":");
            out.push_str(&report_json_string(reason));
            out.push_str(",\"operation\":");
            out.push_str(&report_json_string(operation));
            if let Some(expected) = expected {
                out.push_str(",\"expected\":");
                out.push_str(&report_json_string(expected));
            }
            if let Some(actual) = actual {
                out.push_str(&format!(",\"actual\":{actual}"));
            }
        }
        out.push('}');
        out
    }
}

fn push_optional_usize(out: &mut String, value: Option<usize>) {
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn report_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            character if (character as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}
