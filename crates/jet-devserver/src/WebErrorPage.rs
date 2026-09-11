//! Typed, bounded projections for a local development-service failure.
//!
//! The page is an adapter at the web boundary.  It consumes facts that were
//! already recorded by the terminal/runtime error path; it never parses a
//! rendered diagnostic and it never changes failure classification or
//! lifecycle.  HTML and JSON are two renderings of one `WebErrorFacts` value,
//! so their identity and digest cannot drift.

use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::JSON::json_escape;
use jet_foundation::Outcome::{JetErrorContextFrame, JetErrorJourneyFrame, JetErrorReport};
use jet_foundation::SHA256::sha256_hex;
use std::fmt;

pub const WEB_ERROR_PAGE_SCHEMA: &str = "jet.dev.error/v1";
pub const MAX_WEB_ERROR_ID_BYTES: usize = 256;
pub const MAX_WEB_ERROR_TEXT_BYTES: usize = 8 * 1024;
pub const MAX_WEB_ERROR_SOURCE_BYTES: usize = 16 * 1024;
pub const MAX_WEB_ERROR_LINK_BYTES: usize = 2 * 1024;
pub const MAX_WEB_ERROR_ACCEPT_BYTES: usize = 8 * 1024;
pub const MAX_WEB_ERROR_SOURCE_EXCERPTS: usize = 32;
pub const MAX_WEB_ERROR_ACTION_LINKS: usize = 16;
pub const MAX_WEB_ERROR_EDITS: usize = 64;
pub const MAX_WEB_ERROR_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_WEB_ERROR_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_WEB_ERROR_SPAN_BYTES: usize = 1024 * 1024;

/// Errors are rejected before rendering.  In particular, malformed identity,
/// source spans, and links never become a best-effort page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebErrorPageError {
    EmptyField(&'static str),
    InvalidIdentity(&'static str),
    InvalidSpan,
    InvalidStatus,
    InvalidLink,
    InvalidAccept,
    ControlCharacter(&'static str),
    UnsafeContent(&'static str),
    TextTooLarge(&'static str),
    TooManyItems(&'static str),
    InputTooLarge,
    OutputTooLarge,
}

impl fmt::Display for WebErrorPageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "{field} must not be empty"),
            Self::InvalidIdentity(field) => write!(formatter, "invalid {field} identity"),
            Self::InvalidSpan => formatter.write_str("invalid source span"),
            Self::InvalidStatus => formatter.write_str("web error status must be between 400 and 599"),
            Self::InvalidLink => formatter.write_str("action link is not a safe local or HTTPS URL"),
            Self::InvalidAccept => formatter.write_str("invalid or oversized Accept header"),
            Self::ControlCharacter(field) => write!(formatter, "{field} contains a control character"),
            Self::UnsafeContent(field) => write!(formatter, "{field} contains unpublished sensitive content"),
            Self::TextTooLarge(field) => write!(formatter, "{field} exceeds its byte budget"),
            Self::TooManyItems(field) => write!(formatter, "too many {field}"),
            Self::InputTooLarge => formatter.write_str("web error facts exceed the input byte budget"),
            Self::OutputTooLarge => formatter.write_str("web error projection exceeds the output byte budget"),
        }
    }
}

impl std::error::Error for WebErrorPageError {}

/// An HTTP representation selected from the request's `Accept` header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebErrorContentType {
    Html,
    Json,
}

impl WebErrorContentType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Html => "text/html; charset=utf-8",
            Self::Json => "application/json; charset=utf-8",
        }
    }

    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Html => "text/html",
            Self::Json => "application/json",
        }
    }
}

impl fmt::Display for WebErrorContentType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Capability input for [`WebErrorPage::project`].  A page is local only when
/// the caller explicitly grants local details and the build is not release.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebErrorCapabilities {
    pub local: bool,
    pub release: bool,
}

impl WebErrorCapabilities {
    pub const fn new(local: bool, release: bool) -> Self {
        Self { local, release }
    }

    pub const fn local() -> Self {
        Self {
            local: true,
            release: false,
        }
    }

    pub const fn redacted() -> Self {
        Self {
            local: false,
            release: true,
        }
    }

    fn permits_local_details(self) -> bool {
        self.local && !self.release
    }
}

impl Default for WebErrorCapabilities {
    fn default() -> Self {
        Self::redacted()
    }
}

/// Allows callers to pass capabilities by value, by reference, or the simple
/// `bool` local-development switch without making a second page API.
pub trait WebErrorCapabilityInput {
    fn local_details(&self) -> bool;
    fn release_build(&self) -> bool;
}

impl WebErrorCapabilityInput for WebErrorCapabilities {
    fn local_details(&self) -> bool {
        self.permits_local_details()
    }

    fn release_build(&self) -> bool {
        self.release
    }
}

impl<T: WebErrorCapabilityInput + ?Sized> WebErrorCapabilityInput for &T {
    fn local_details(&self) -> bool {
        (*self).local_details()
    }

    fn release_build(&self) -> bool {
        (*self).release_build()
    }
}

impl WebErrorCapabilityInput for bool {
    fn local_details(&self) -> bool {
        *self
    }

    fn release_build(&self) -> bool {
        false
    }
}

/// Stable correlation identity carried by every projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebErrorPageIdentity {
    pub request_id: String,
    pub session_id: String,
    pub source_id: String,
    pub build_id: String,
    pub revision: String,
}

impl WebErrorPageIdentity {
    pub fn new(
        request_id: impl Into<String>,
        session_id: impl Into<String>,
        source_id: impl Into<String>,
        build_id: impl Into<String>,
        revision: impl Into<String>,
    ) -> Result<Self, WebErrorPageError> {
        let identity = Self {
            request_id: normalize_identity(request_id.into(), "request id")?,
            session_id: normalize_identity(session_id.into(), "session id")?,
            source_id: normalize_source_identity(source_id.into())?,
            build_id: normalize_identity(build_id.into(), "build id")?,
            revision: normalize_identity(revision.into(), "revision")?,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), WebErrorPageError> {
        validate_identity(&self.request_id, "request id")?;
        validate_identity(&self.session_id, "session id")?;
        validate_source_identity(&self.source_id)?;
        validate_identity(&self.build_id, "build id")?;
        validate_identity(&self.revision, "revision")?;
        Ok(())
    }

    fn json(&self) -> String {
        format!(
            "{{\"request_id\":{},\"session_id\":{},\"source_id\":{},\"build_id\":{},\"revision\":{}}}",
            json_string(&self.request_id),
            json_string(&self.session_id),
            json_string(&self.source_id),
            json_string(&self.build_id),
            json_string(&self.revision),
        )
    }
}

/// A byte range into a source file.  Equal endpoints are allowed for an
/// insertion point; reversed or unbounded ranges fail closed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebErrorSpan {
    pub start: usize,
    pub end: usize,
}

impl WebErrorSpan {
    pub fn new(start: usize, end: usize) -> Result<Self, WebErrorPageError> {
        let span = Self { start, end };
        span.validate()?;
        Ok(span)
    }

    pub fn validate(&self) -> Result<(), WebErrorPageError> {
        if self.end < self.start
            || self.start > MAX_WEB_ERROR_SPAN_BYTES
            || self.end > MAX_WEB_ERROR_SPAN_BYTES
        {
            return Err(WebErrorPageError::InvalidSpan);
        }
        Ok(())
    }

    fn json(self) -> String {
        format!("{{\"start\":{},\"end\":{}}}", self.start, self.end)
    }
}

impl From<WebErrorSpan> for Span {
    fn from(span: WebErrorSpan) -> Self {
        Span::new(span.start, span.end)
    }
}

/// One source frame or excerpt.  An absent `span` or `excerpt` is an explicit
/// unavailable fact, not a guessed value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebErrorSourceExcerpt {
    pub source_id: String,
    pub label: String,
    pub span: Option<WebErrorSpan>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub excerpt: Option<String>,
}

impl WebErrorSourceExcerpt {
    pub fn new(
        source_id: impl Into<String>,
        label: impl Into<String>,
        start: usize,
        end: usize,
        excerpt: Option<String>,
    ) -> Result<Self, WebErrorPageError> {
        let span = Some(WebErrorSpan::new(start, end)?);
        Self::with_parts(source_id.into(), label.into(), span, None, None, excerpt)
    }

    pub fn without_span(
        source_id: impl Into<String>,
        label: impl Into<String>,
        excerpt: Option<String>,
    ) -> Result<Self, WebErrorPageError> {
        Self::with_parts(
            source_id.into(),
            label.into(),
            None,
            None,
            None,
            excerpt,
        )
    }

    pub fn with_location(
        mut self,
        line: Option<u32>,
        column: Option<u32>,
    ) -> Result<Self, WebErrorPageError> {
        if line.is_some_and(|value| value == 0) || column.is_some_and(|value| value == 0) {
            return Err(WebErrorPageError::InvalidSpan);
        }
        self.line = line;
        self.column = column;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), WebErrorPageError> {
        validate_source_identity(&self.source_id)?;
        validate_identity(&self.label, "source label")?;
        if let Some(span) = self.span {
            span.validate()?;
        }
        if self.line.is_some_and(|value| value == 0) || self.column.is_some_and(|value| value == 0) {
            return Err(WebErrorPageError::InvalidSpan);
        }
        validate_optional_text(&self.excerpt, "source excerpt", MAX_WEB_ERROR_SOURCE_BYTES)?;
        Ok(())
    }

    fn with_parts(
        source_id: String,
        label: String,
        span: Option<WebErrorSpan>,
        line: Option<u32>,
        column: Option<u32>,
        excerpt: Option<String>,
    ) -> Result<Self, WebErrorPageError> {
        let item = Self {
            source_id: normalize_source_identity(source_id)?,
            label: normalize_identity(label, "source label")?,
            span,
            line,
            column,
            excerpt: normalize_optional_text(excerpt, "source excerpt", MAX_WEB_ERROR_SOURCE_BYTES)?,
        };
        item.validate()?;
        Ok(item)
    }

    fn json(&self) -> String {
        let mut fields = vec![
            format!("\"source_id\":{}", json_string(&self.source_id)),
            format!("\"label\":{}", json_string(&self.label)),
        ];
        if let Some(span) = self.span {
            fields.push(format!("\"span\":{}", span.json()));
        }
        if let Some(line) = self.line {
            fields.push(format!("\"line\":{line}"));
        }
        if let Some(column) = self.column {
            fields.push(format!("\"column\":{column}"));
        }
        if let Some(excerpt) = &self.excerpt {
            fields.push(format!("\"excerpt\":{}", json_string(excerpt)));
        }
        format!("{{{}}}", fields.join(","))
    }
}

/// A safe action link.  Only local paths, HTTPS/HTTP, and the Jet inspection
/// scheme are accepted; script and credential-bearing URLs are rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebErrorActionLink {
    pub label: String,
    pub href: String,
}

impl WebErrorActionLink {
    pub fn new(
        label: impl Into<String>,
        href: impl Into<String>,
    ) -> Result<Self, WebErrorPageError> {
        let link = Self {
            label: normalize_identity(label.into(), "action label")?,
            href: normalize_href(href.into())?,
        };
        link.validate()?;
        Ok(link)
    }

    pub fn validate(&self) -> Result<(), WebErrorPageError> {
        validate_identity(&self.label, "action label")?;
        validate_href(&self.href)
    }

    fn json(&self) -> String {
        format!(
            "{{\"label\":{},\"href\":{}}}",
            json_string(&self.label),
            json_string(&self.href),
        )
    }
}

/// One typed source edit attached to a terminal diagnostic.
///
/// The edit remains a fact owned by the diagnostic.  Its source identity and
/// grade are carried into the web projection without parsing terminal output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebErrorEdit {
    pub source_id: String,
    pub span: WebErrorSpan,
    pub new_text: String,
    pub applicability: Option<String>,
    pub safety: Option<String>,
}

impl WebErrorEdit {
    pub fn new(
        source_id: impl Into<String>,
        span: WebErrorSpan,
        new_text: impl Into<String>,
        applicability: Option<String>,
        safety: Option<String>,
    ) -> Result<Self, WebErrorPageError> {
        let edit = Self {
            source_id: normalize_source_identity(source_id.into())?,
            span,
            new_text: normalize_text(new_text.into(), "fix edit", MAX_WEB_ERROR_TEXT_BYTES)?,
            applicability: applicability
                .map(|value| normalize_identity(value, "fix applicability"))
                .transpose()?,
            safety: safety
                .map(|value| normalize_identity(value, "fix safety"))
                .transpose()?,
        };
        edit.validate()?;
        Ok(edit)
    }

    fn from_diagnostic(
        source_id: &str,
        edit: &jet_foundation::Diagnostics::TextEdit,
        applicability: Option<&str>,
        safety: Option<&str>,
    ) -> Result<Self, WebErrorPageError> {
        Self::new(
            source_id,
            web_span_from_span(edit.span)?,
            edit.new_text.clone(),
            applicability.map(str::to_string),
            safety.map(str::to_string),
        )
    }

    pub fn validate(&self) -> Result<(), WebErrorPageError> {
        validate_source_identity(&self.source_id)?;
        self.span.validate()?;
        if self.new_text.len() > MAX_WEB_ERROR_TEXT_BYTES {
            return Err(WebErrorPageError::TextTooLarge("fix edit"));
        }
        if self.new_text.chars().any(|character| {
            character.is_control()
                && !matches!(character, '\n' | '\r' | '\t')
        }) {
            return Err(WebErrorPageError::ControlCharacter("fix edit"));
        }
        if contains_sensitive_marker(&self.new_text) {
            return Err(WebErrorPageError::UnsafeContent("fix edit"));
        }
        if let Some(value) = &self.applicability {
            validate_identity(value, "fix applicability")?;
        }
        if let Some(value) = &self.safety {
            validate_identity(value, "fix safety")?;
        }
        Ok(())
    }

    fn json(&self) -> String {
        let mut fields = vec![
            format!("\"source_id\":{}", json_string(&self.source_id)),
            format!("\"span\":{}", self.span.json()),
            format!("\"new_text\":{}", json_string(&self.new_text)),
        ];
        if let Some(value) = &self.applicability {
            fields.push(format!("\"applicability\":{}", json_string(value)));
        }
        if let Some(value) = &self.safety {
            fields.push(format!("\"safety\":{}", json_string(value)));
        }
        format!("{{{}}}", fields.join(","))
    }
}

/// A typed compiler or host failure that is not a user-authored diagnostic.
///
/// Internal failures are rendered through the same bounded page, but retain a
/// separate wire kind so an ICE can never be mistaken for a registered
/// diagnostic row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebErrorInternalFact {
    pub code: String,
    pub message: String,
}

impl WebErrorInternalFact {
    pub fn new(
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<Self, WebErrorPageError> {
        let fact = Self {
            code: normalize_code(code.into())?,
            message: normalize_required_text(
                message.into(),
                "internal error message",
                MAX_WEB_ERROR_TEXT_BYTES,
            )?,
        };
        fact.validate()?;
        Ok(fact)
    }

    fn validate(&self) -> Result<(), WebErrorPageError> {
        validate_code(&self.code)?;
        if self.message.is_empty() {
            return Err(WebErrorPageError::EmptyField("internal error message"));
        }
        validate_optional_text(
            &Some(self.message.clone()),
            "internal error message",
            MAX_WEB_ERROR_TEXT_BYTES,
        )
    }

    fn json(&self) -> String {
        format!(
            "{{\"code\":{},\"message\":{}}}",
            json_string(&self.code),
            json_string(&self.message)
        )
    }
}

/// Real facts read from `DevStatus`'s live snapshot. `WebHost` owns
/// `DevStatus` privately, so this is the minimal typed handoff instead of a
/// visibility change or a module cycle. Fields mirror `DevStatusSnapshot`
/// exactly: `code`/`diagnostic` are empty for every state but `"error"`,
/// matching `DevStatus::mark_error`'s own invariant, and no rendered text is
/// re-parsed to recover them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebErrorDevStatusFacts {
    pub state: String,
    pub code: String,
    pub diagnostic: String,
}

/// The exact sentinel `CmdCompile`/`CmdDevTools` write as `DevStatus`'s
/// `code` for a compiler or host failure that is not a registered
/// diagnostic (`Source/CmdCompile.rs`'s `host.mark_error("ICE", ...)`
/// calls). Devstatus facts carrying this code must render through
/// [`WebErrorPage::from_internal`], never as a diagnostic row.
pub const DEV_STATUS_INTERNAL_ERROR_CODE: &str = "ICE";

impl WebErrorDevStatusFacts {
    pub fn new(
        state: impl Into<String>,
        code: impl Into<String>,
        diagnostic: impl Into<String>,
    ) -> Self {
        Self {
            state: state.into(),
            code: code.into(),
            diagnostic: diagnostic.into(),
        }
    }

    /// Whether this snapshot's `code` is the internal-failure sentinel
    /// rather than a registered diagnostic code. An ICE must never be
    /// mistaken for a diagnostic row, so the projection dispatches on this
    /// exact fact instead of guessing from message text.
    pub fn is_internal_failure(&self) -> bool {
        self.code == DEV_STATUS_INTERNAL_ERROR_CODE
    }

    /// Whether the snapshot names a live error. `"ready"`/`"building"`/
    /// `"reconnecting"` are explicit non-error facts, not an absent value to
    /// guess past.
    pub fn is_error(&self) -> bool {
        self.state == "error"
    }
}

/// One typed diagnostic fact.  Runtime reports may legitimately lack `why`
/// and `fix`; their absence is retained rather than reconstructed from text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebErrorDiagnosticFact {
    pub code: Option<String>,
    pub what: Option<String>,
    pub why: Option<String>,
    pub fix: Option<String>,
    pub severity: Option<String>,
    pub moment: Option<String>,
    pub typed_identity: Option<String>,
    pub span: Option<WebErrorSpan>,
}

impl WebErrorDiagnosticFact {
    /// Construct a complete terminal diagnostic from its four owned fields.
    pub fn new(
        code: impl Into<String>,
        what: impl Into<String>,
        why: impl Into<String>,
        fix: impl Into<String>,
    ) -> Result<Self, WebErrorPageError> {
        let fact = Self {
            code: Some(normalize_code(code.into())?),
            what: Some(normalize_required_text(
                what.into(),
                "diagnostic what",
                MAX_WEB_ERROR_TEXT_BYTES,
            )?),
            why: Some(normalize_required_text(
                why.into(),
                "diagnostic why",
                MAX_WEB_ERROR_TEXT_BYTES,
            )?),
            fix: Some(normalize_required_text(
                fix.into(),
                "diagnostic fix",
                MAX_WEB_ERROR_TEXT_BYTES,
            )?),
            severity: None,
            moment: None,
            typed_identity: None,
            span: None,
        };
        fact.validate()?;
        Ok(fact)
    }

    /// Copy the terminal diagnostic's typed fields.  No rendered terminal
    /// string is parsed and an empty fix remains unavailable.
    pub fn from_diagnostic(diagnostic: &Diagnostic) -> Result<Self, WebErrorPageError> {
        let fact = Self {
            code: Some(normalize_code(diagnostic.code.clone())?),
            what: Some(normalize_required_text(
                diagnostic.what.clone(),
                "diagnostic what",
                MAX_WEB_ERROR_TEXT_BYTES,
            )?),
            why: normalize_optional_text(
                Some(diagnostic.why.clone()),
                "diagnostic why",
                MAX_WEB_ERROR_TEXT_BYTES,
            )?,
            fix: normalize_optional_text(
                Some(diagnostic.fix.clone()),
                "diagnostic fix",
                MAX_WEB_ERROR_TEXT_BYTES,
            )?,
            severity: Some(match diagnostic.severity {
                jet_foundation::Diagnostics::Severity::Error => "error".to_string(),
                jet_foundation::Diagnostics::Severity::Lint => "lint".to_string(),
            }),
            moment: Some(diagnostic.moment.as_str().to_string()),
            typed_identity: None,
            span: diagnostic.span.map(web_span_from_span).transpose()?,
        };
        fact.validate()?;
        Ok(fact)
    }

    /// Project a normalized runtime report without using `Display` output.
    /// `JetErrorReport` has no authored why/fix fields, so those fields stay
    /// absent in the web fact.
    pub fn from_report(report: &JetErrorReport) -> Result<Self, WebErrorPageError> {
        let typed_identity = report
            .typed_identity
            .clone()
            .map(|value| normalize_identity(value, "typed error identity"))
            .transpose()?;
        let span = report
            .details
            .as_ref()
            .and_then(|details| details.source_span.clone())
            .map(|span| WebErrorSpan::new(span.start, span.end))
            .transpose()?;
        let fact = Self {
            code: report
                .code
                .clone()
                .map(normalize_code)
                .transpose()?,
            what: Some(normalize_required_text(
                report.message.clone(),
                "diagnostic what",
                MAX_WEB_ERROR_TEXT_BYTES,
            )?),
            why: None,
            fix: None,
            severity: None,
            moment: None,
            typed_identity,
            span,
        };
        fact.validate()?;
        Ok(fact)
    }

    /// Copy the exact code/diagnostic strings `DevStatus` recorded for its
    /// current `"error"` snapshot. There is no why/fix in a dev-status fact,
    /// so those fields stay absent rather than reconstructed.
    pub fn from_dev_status(facts: &WebErrorDevStatusFacts) -> Result<Self, WebErrorPageError> {
        let code = if facts.code.is_empty() {
            None
        } else {
            Some(normalize_code(facts.code.clone())?)
        };
        let fact = Self {
            code,
            what: Some(normalize_required_text(
                facts.diagnostic.clone(),
                "diagnostic what",
                MAX_WEB_ERROR_TEXT_BYTES,
            )?),
            why: None,
            fix: None,
            severity: None,
            moment: None,
            typed_identity: None,
            span: None,
        };
        fact.validate()?;
        Ok(fact)
    }

    pub fn with_span(mut self, span: Option<WebErrorSpan>) -> Result<Self, WebErrorPageError> {
        if let Some(value) = span {
            value.validate()?;
        }
        self.span = span;
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), WebErrorPageError> {
        if let Some(code) = &self.code {
            validate_code(code)?;
        }
        if self.what.is_none() {
            return Err(WebErrorPageError::EmptyField("diagnostic what"));
        }
        validate_optional_text(&self.what, "diagnostic what", MAX_WEB_ERROR_TEXT_BYTES)?;
        validate_optional_text(&self.why, "diagnostic why", MAX_WEB_ERROR_TEXT_BYTES)?;
        validate_optional_text(&self.fix, "diagnostic fix", MAX_WEB_ERROR_TEXT_BYTES)?;
        if let Some(severity) = &self.severity {
            validate_identity(severity, "diagnostic severity")?;
        }
        if let Some(moment) = &self.moment {
            validate_identity(moment, "diagnostic moment")?;
        }
        if let Some(identity) = &self.typed_identity {
            validate_identity(identity, "typed error identity")?;
        }
        if let Some(span) = self.span {
            span.validate()?;
        }
        Ok(())
    }

    fn json(&self) -> String {
        self.json_with_edits(&[])
    }

    fn json_with_edits(&self, edits: &[WebErrorEdit]) -> String {
        let mut fields = Vec::new();
        if let Some(code) = &self.code {
            fields.push(format!("\"code\":{}", json_string(code)));
        }
        if let Some(what) = &self.what {
            fields.push(format!("\"what\":{}", json_string(what)));
        }
        if let Some(why) = &self.why {
            fields.push(format!("\"why\":{}", json_string(why)));
        }
        if let Some(fix) = &self.fix {
            fields.push(format!("\"fix\":{}", json_string(fix)));
        }
        if let Some(severity) = &self.severity {
            fields.push(format!("\"severity\":{}", json_string(severity)));
        }
        if let Some(moment) = &self.moment {
            fields.push(format!("\"moment\":{}", json_string(moment)));
        }
        if let Some(identity) = &self.typed_identity {
            fields.push(format!("\"typed_identity\":{}", json_string(identity)));
        }
        if let Some(span) = self.span {
            fields.push(format!("\"span\":{}", span.json()));
        }
        if !edits.is_empty() {
            let edits = edits
                .iter()
                .map(WebErrorEdit::json)
                .collect::<Vec<_>>()
                .join(",");
            fields.push(format!("\"fix_edits\":[{edits}]"));
        }
        format!("{{{}}}", fields.join(","))
    }

    fn internal_placeholder() -> Self {
        Self {
            code: None,
            what: Some("Internal development-service failure".to_string()),
            why: None,
            fix: None,
            severity: None,
            moment: None,
            typed_identity: None,
            span: None,
        }
    }
}

/// The completed web projection.  `fact_digest` is computed before either
/// renderer runs and is therefore identical for the HTML and JSON bodies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebErrorProjection {
    pub status: u16,
    pub content_type: WebErrorContentType,
    pub body: String,
    pub fact_digest: String,
    pub failure_id: String,
    pub redacted: bool,
}

/// A validated, source-backed failure page ready for content negotiation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebErrorPage {
    identity: WebErrorPageIdentity,
    diagnostic: WebErrorDiagnosticFact,
    internal: Option<WebErrorInternalFact>,
    edits: Vec<WebErrorEdit>,
    source_excerpts: Vec<WebErrorSourceExcerpt>,
    related_cause: Option<WebErrorDiagnosticFact>,
    action_links: Vec<WebErrorActionLink>,
    status: u16,
    failure_id: String,
}

impl WebErrorPage {
    pub fn new(
        identity: WebErrorPageIdentity,
        diagnostic: WebErrorDiagnosticFact,
        source_excerpts: Vec<WebErrorSourceExcerpt>,
        related_cause: Option<WebErrorDiagnosticFact>,
        action_links: Vec<WebErrorActionLink>,
    ) -> Result<Self, WebErrorPageError> {
        Self::new_with_status(
            500,
            identity,
            diagnostic,
            source_excerpts,
            related_cause,
            action_links,
        )
    }

    pub fn new_with_status(
        status: u16,
        identity: WebErrorPageIdentity,
        diagnostic: WebErrorDiagnosticFact,
        mut source_excerpts: Vec<WebErrorSourceExcerpt>,
        related_cause: Option<WebErrorDiagnosticFact>,
        mut action_links: Vec<WebErrorActionLink>,
    ) -> Result<Self, WebErrorPageError> {
        validate_status(status)?;
        identity.validate()?;
        diagnostic.validate()?;
        if let Some(cause) = &related_cause {
            cause.validate()?;
        }
        if source_excerpts.len() > MAX_WEB_ERROR_SOURCE_EXCERPTS {
            return Err(WebErrorPageError::TooManyItems("source excerpts"));
        }
        if action_links.len() > MAX_WEB_ERROR_ACTION_LINKS {
            return Err(WebErrorPageError::TooManyItems("action links"));
        }
        for excerpt in &source_excerpts {
            excerpt.validate()?;
        }
        for link in &action_links {
            link.validate()?;
        }
        source_excerpts.sort_by(source_excerpt_order);
        action_links.sort_by(action_link_order);
        let page = Self {
            identity,
            diagnostic,
            internal: None,
            edits: Vec::new(),
            source_excerpts,
            related_cause,
            action_links,
            status,
            failure_id: String::new(),
        };
        page.validate_size()?;
        let failure_id = page.compute_failure_id();
        Ok(Self { failure_id, ..page })
    }

    /// Build a page directly from the normalized runtime report.  Context and
    /// journey frames remain typed facts; they are never recovered from prose.
    pub fn from_report(
        identity: WebErrorPageIdentity,
        report: &JetErrorReport,
        action_links: Vec<WebErrorActionLink>,
    ) -> Result<Self, WebErrorPageError> {
        let diagnostic = WebErrorDiagnosticFact::from_report(report)?;
        let source_excerpts = report_source_excerpts(report)?;
        let related_cause = report
            .causes
            .first()
            .map(WebErrorDiagnosticFact::from_report)
            .transpose()?;
        Self::new(
            identity,
            diagnostic,
            source_excerpts,
            related_cause,
            action_links,
        )
    }

    /// Build a page from the terminal diagnostic facts and caller-supplied
    /// source excerpts.  This copies fields instead of parsing terminal text.
    pub fn from_diagnostic(
        identity: WebErrorPageIdentity,
        diagnostic: &Diagnostic,
        source_excerpts: Vec<WebErrorSourceExcerpt>,
        action_links: Vec<WebErrorActionLink>,
    ) -> Result<Self, WebErrorPageError> {
        let source_id = identity.source_id.clone();
        let applicability = diagnostic.applicability.map(|value| value.as_str());
        let safety = diagnostic.safety.map(|value| value.as_str());
        let edits = diagnostic
            .all_edits()
            .iter()
            .map(|edit| WebErrorEdit::from_diagnostic(&source_id, edit, applicability, safety))
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(
            identity,
            WebErrorDiagnosticFact::from_diagnostic(diagnostic)?,
            source_excerpts,
            None,
            action_links,
        )?
        .with_edits(edits)
    }

    /// Build a page for a compiler or host failure that is not a user
    /// diagnostic.  The separate internal wire kind prevents an ICE message
    /// from entering the registered diagnostic-code surface.
    pub fn from_internal(
        identity: WebErrorPageIdentity,
        code: impl Into<String>,
        message: impl Into<String>,
        action_links: Vec<WebErrorActionLink>,
    ) -> Result<Self, WebErrorPageError> {
        let internal = WebErrorInternalFact::new(code, message)?;
        let mut page = Self::new(
            identity,
            WebErrorDiagnosticFact::internal_placeholder(),
            Vec::new(),
            None,
            action_links,
        )?;
        page.internal = Some(internal);
        page.failure_id = page.compute_failure_id();
        page.validate_size()?;
        Ok(page)
    }

    /// Build a page directly from `DevStatus`'s live snapshot in one call.
    /// `Ok(None)` is the explicit "no error is live" fact: a `"ready"` or
    /// `"building"` snapshot must never become a placeholder failure page.
    /// A `DEV_STATUS_INTERNAL_ERROR_CODE` snapshot routes through
    /// [`Self::from_internal`] so an ICE keeps its separate wire kind
    /// instead of being copied into a diagnostic's `code` field.
    pub fn from_dev_status(
        identity: WebErrorPageIdentity,
        facts: &WebErrorDevStatusFacts,
        action_links: Vec<WebErrorActionLink>,
    ) -> Result<Option<Self>, WebErrorPageError> {
        if !facts.is_error() {
            return Ok(None);
        }
        if facts.is_internal_failure() {
            return Self::from_internal(
                identity,
                facts.code.clone(),
                facts.diagnostic.clone(),
                action_links,
            )
            .map(Some);
        }
        let diagnostic = WebErrorDiagnosticFact::from_dev_status(facts)?;
        Self::new(identity, diagnostic, Vec::new(), None, action_links).map(Some)
    }

    /// Construct and negotiate a rendered failure response from `DevStatus`
    /// facts in one call — the host-facing entry point this adapter exists
    /// for. `Ok(None)` means there is no live error to report, distinct from
    /// a validation failure.
    pub fn project_dev_status<C: WebErrorCapabilityInput>(
        identity: WebErrorPageIdentity,
        facts: &WebErrorDevStatusFacts,
        action_links: Vec<WebErrorActionLink>,
        accept: &str,
        capabilities: C,
    ) -> Result<Option<WebErrorProjection>, WebErrorPageError> {
        match Self::from_dev_status(identity, facts, action_links)? {
            Some(page) => page.project(accept, capabilities).map(Some),
            None => Ok(None),
        }
    }

    pub fn with_edits(mut self, mut edits: Vec<WebErrorEdit>) -> Result<Self, WebErrorPageError> {
        if edits.len() > MAX_WEB_ERROR_EDITS {
            return Err(WebErrorPageError::TooManyItems("fix edits"));
        }
        for edit in &edits {
            edit.validate()?;
        }
        edits.sort_by(web_error_edit_order);
        self.edits = edits;
        self.failure_id = self.compute_failure_id();
        self.validate_size()?;
        Ok(self)
    }

    pub fn with_status(mut self, status: u16) -> Result<Self, WebErrorPageError> {
        validate_status(status)?;
        self.status = status;
        self.failure_id = self.compute_failure_id();
        self.validate_size()?;
        Ok(self)
    }

    pub fn identity(&self) -> &WebErrorPageIdentity {
        &self.identity
    }

    pub fn diagnostic(&self) -> &WebErrorDiagnosticFact {
        &self.diagnostic
    }
    pub fn internal(&self) -> Option<&WebErrorInternalFact> {
        self.internal.as_ref()
    }

    pub fn is_internal(&self) -> bool {
        self.internal.is_some()
    }

    pub fn edits(&self) -> &[WebErrorEdit] {
        &self.edits
    }

    pub fn with_source_excerpts(
        mut self,
        mut source_excerpts: Vec<WebErrorSourceExcerpt>,
    ) -> Result<Self, WebErrorPageError> {
        if source_excerpts.len() > MAX_WEB_ERROR_SOURCE_EXCERPTS {
            return Err(WebErrorPageError::TooManyItems("source excerpts"));
        }
        for excerpt in &source_excerpts {
            excerpt.validate()?;
        }
        source_excerpts.sort_by(source_excerpt_order);
        self.source_excerpts = source_excerpts;
        self.failure_id = self.compute_failure_id();
        self.validate_size()?;
        Ok(self)
    }


    pub fn source_excerpts(&self) -> &[WebErrorSourceExcerpt] {
        &self.source_excerpts
    }

    pub fn related_cause(&self) -> Option<&WebErrorDiagnosticFact> {
        self.related_cause.as_ref()
    }

    pub fn action_links(&self) -> &[WebErrorActionLink] {
        &self.action_links
    }

    pub const fn status(&self) -> u16 {
        self.status
    }

    pub fn failure_id(&self) -> &str {
        &self.failure_id
    }

    pub fn validate(&self) -> Result<(), WebErrorPageError> {
        validate_status(self.status)?;
        self.identity.validate()?;
        self.diagnostic.validate()?;
        if let Some(internal) = &self.internal {
            internal.validate()?;
        }
        if let Some(cause) = &self.related_cause {
            cause.validate()?;
        }
        if self.source_excerpts.len() > MAX_WEB_ERROR_SOURCE_EXCERPTS {
            return Err(WebErrorPageError::TooManyItems("source excerpts"));
        }
        if self.edits.len() > MAX_WEB_ERROR_EDITS {
            return Err(WebErrorPageError::TooManyItems("fix edits"));
        }
        if self.action_links.len() > MAX_WEB_ERROR_ACTION_LINKS {
            return Err(WebErrorPageError::TooManyItems("action links"));
        }
        for edit in &self.edits {
            edit.validate()?;
        }
        for excerpt in &self.source_excerpts {
            excerpt.validate()?;
        }
        for link in &self.action_links {
            link.validate()?;
        }
        self.validate_size()
    }

    /// Negotiate HTML or JSON and render the same validated facts in that
    /// media type.  `capabilities` defaults to redacted unless local details
    /// are explicitly granted and the build is not release.
    pub fn project<C: WebErrorCapabilityInput>(
        &self,
        accept: &str,
        capabilities: C,
    ) -> Result<WebErrorProjection, WebErrorPageError> {
        if accept.len() > MAX_WEB_ERROR_ACCEPT_BYTES || accept.chars().any(char::is_control) {
            return Err(WebErrorPageError::InvalidAccept);
        }
        self.validate()?;
        let local =
            cfg!(debug_assertions) && capabilities.local_details() && !capabilities.release_build();
        let facts = WebErrorFacts::from_page(self, local);
        let fact_digest = sha256_hex(facts.canonical_json().as_bytes());
        let content_type = negotiate_content_type(accept);
        let body = match content_type {
            WebErrorContentType::Html => facts.render_html(),
            WebErrorContentType::Json => facts.render_json(),
        };
        if body.len() > MAX_WEB_ERROR_OUTPUT_BYTES {
            return Err(WebErrorPageError::OutputTooLarge);
        }
        Ok(WebErrorProjection {
            status: self.status,
            content_type,
            body,
            fact_digest,
            failure_id: self.failure_id.clone(),
            redacted: facts.redacted,
        })
    }

    fn validate_size(&self) -> Result<(), WebErrorPageError> {
        let mut bytes = self.identity.request_id.len()
            + self.identity.session_id.len()
            + self.identity.source_id.len()
            + self.identity.build_id.len()
            + self.identity.revision.len()
            + diagnostic_bytes(&self.diagnostic);
        if let Some(internal) = &self.internal {
            bytes = bytes
                .saturating_add(internal.code.len())
                .saturating_add(internal.message.len());
        }
        for edit in &self.edits {
            bytes = bytes
                .saturating_add(edit.source_id.len())
                .saturating_add(edit.new_text.len())
                .saturating_add(edit.applicability.as_ref().map_or(0, String::len))
                .saturating_add(edit.safety.as_ref().map_or(0, String::len));
        }
        if let Some(cause) = &self.related_cause {
            bytes = bytes.saturating_add(diagnostic_bytes(cause));
        }
        for excerpt in &self.source_excerpts {
            bytes = bytes
                .saturating_add(excerpt.source_id.len())
                .saturating_add(excerpt.label.len())
                .saturating_add(excerpt.excerpt.as_ref().map_or(0, String::len));
        }
        for link in &self.action_links {
            bytes = bytes.saturating_add(link.label.len()).saturating_add(link.href.len());
        }
        if bytes > MAX_WEB_ERROR_INPUT_BYTES {
            return Err(WebErrorPageError::InputTooLarge);
        }
        Ok(())
    }

    fn compute_failure_id(&self) -> String {
        let facts = WebErrorFacts::from_page(self, true);
        format!("fail-{}", sha256_hex(facts.canonical_json().as_bytes()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct WebErrorFacts {
    status: u16,
    failure_id: String,
    identity: WebErrorPageIdentity,
    diagnostic: WebErrorDiagnosticFact,
    internal: Option<WebErrorInternalFact>,
    edits: Vec<WebErrorEdit>,
    source_excerpts: Vec<WebErrorSourceExcerpt>,
    related_cause: Option<WebErrorDiagnosticFact>,
    action_links: Vec<WebErrorActionLink>,
    redacted: bool,
}

impl WebErrorFacts {
    fn from_page(page: &WebErrorPage, local: bool) -> Self {
        Self {
            status: page.status,
            failure_id: page.failure_id.clone(),
            identity: WebErrorPageIdentity {
                request_id: redact_identity(&page.identity.request_id),
                session_id: redact_identity(&page.identity.session_id),
                source_id: redact_identity(&page.identity.source_id),
                build_id: redact_identity(&page.identity.build_id),
                revision: redact_identity(&page.identity.revision),
            },
            diagnostic: if local {
                page.diagnostic.clone()
            } else {
                let mut diagnostic = page.diagnostic.clone();
                diagnostic.why = None;
                diagnostic.fix = None;
                diagnostic.typed_identity = None;
                diagnostic.span = None;
                diagnostic
            },
            internal: page.internal.as_ref().map(|internal| {
                if local {
                    internal.clone()
                } else {
                    WebErrorInternalFact {
                        code: internal.code.clone(),
                        message: "[redacted]".to_string(),
                    }
                }
            }),
            edits: if local { page.edits.clone() } else { Vec::new() },
            source_excerpts: if local {
                page.source_excerpts.clone()
            } else {
                Vec::new()
            },
            related_cause: if local {
                page.related_cause.clone()
            } else {
                None
            },
            action_links: if local {
                page.action_links.clone()
            } else {
                Vec::new()
            },
            redacted: !local,
        }
    }

    fn canonical_json(&self) -> String {
        self.json(false)
    }

    fn render_json(&self) -> String {
        self.json(true)
    }

    fn json(&self, include_failure_id: bool) -> String {
        let source_excerpts = self
            .source_excerpts
            .iter()
            .map(WebErrorSourceExcerpt::json)
            .collect::<Vec<_>>()
            .join(",");
        let action_links = self
            .action_links
            .iter()
            .map(WebErrorActionLink::json)
            .collect::<Vec<_>>()
            .join(",");
        let mut fields = vec![
            format!("\"schema\":{}", json_string(WEB_ERROR_PAGE_SCHEMA)),
            format!("\"status\":{}", self.status),
        ];
        if include_failure_id {
            fields.push(format!("\"failure_id\":{}", json_string(&self.failure_id)));
        }
        fields.push(format!(
            "\"kind\":{}",
            json_string(if self.internal.is_some() {
                "internal"
            } else {
                "diagnostic"
            })
        ));
        fields.push(format!("\"redacted\":{}", if self.redacted { "true" } else { "false" }));
        fields.push(format!("\"identity\":{}", self.identity.json()));
        if let Some(internal) = &self.internal {
            fields.push(format!("\"internal\":{}", internal.json()));
            fields.push("\"diagnostic\":null".to_string());
        } else {
            fields.push(format!(
                "\"diagnostic\":{}",
                self.diagnostic.json_with_edits(&self.edits)
            ));
        }
        fields.push(format!(
            "\"related_cause\":{}",
            self.related_cause
                .as_ref()
                .map_or_else(|| "null".to_string(), WebErrorDiagnosticFact::json)
        ));
        fields.push(format!("\"source_excerpts\":[{source_excerpts}]"));
        fields.push(format!("\"action_links\":[{action_links}]"));
        format!("{{{}}}", fields.join(","))
    }

    fn render_html(&self) -> String {
        let mut html = String::with_capacity(4096);
        html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>Jet development error</title><style>body{font:16px system-ui,sans-serif;max-width:58rem;margin:2rem auto;padding:0 1rem;color:#202124}main{border:1px solid #d0d7de;border-radius:.5rem;padding:1.5rem}h1{font-size:1.5rem;margin:.25rem 0 1rem}h2{font-size:1rem;margin:1.5rem 0 .5rem}dl{display:grid;grid-template-columns:max-content 1fr;gap:.35rem .9rem}dt{font-weight:600}dd{margin:0;overflow-wrap:anywhere}pre{background:#f6f8fa;padding:.75rem;overflow:auto;white-space:pre-wrap}a{display:inline-block;margin:.25rem .5rem .25rem 0}code{font-family:ui-monospace,monospace}.muted{color:#57606a}.redacted{background:#fff8c5;padding:.75rem}</style></head><body><main data-schema=\"jet.dev.error/v1\">");
        html.push_str("<p class=\"muted\">Jet local development error</p><h1>");
        if let Some(what) = &self.diagnostic.what {
            html.push_str(&html_escape(what));
        }
        html.push_str("</h1><p><code>");
        if let Some(code) = &self.diagnostic.code {
            html.push_str(&html_escape(code));
        } else {
            html.push_str("diagnostic code unavailable");
        }
        html.push_str("</code> · HTTP ");
        html.push_str(&self.status.to_string());
        html.push_str(" · <code data-failure-id=\"");
        html.push_str(&html_escape(&self.failure_id));
        html.push_str("\">");
        html.push_str(&html_escape(&self.failure_id));
        html.push_str("</code></p><h2>Request identity</h2><dl>");
        html.push_str(&html_pair("request id", &self.identity.request_id));
        html.push_str(&html_pair("session id", &self.identity.session_id));
        html.push_str(&html_pair("source id", &self.identity.source_id));
        html.push_str(&html_pair("build id", &self.identity.build_id));
        html.push_str(&html_pair("revision", &self.identity.revision));
        html.push_str("</dl><h2>Diagnostic</h2><dl>");
        if let Some(why) = &self.diagnostic.why {
            html.push_str(&html_pair("why", why));
        }
        if let Some(fix) = &self.diagnostic.fix {
            html.push_str(&html_pair("fix", fix));
        }
        if let Some(moment) = &self.diagnostic.moment {
            html.push_str(&html_pair("moment", moment));
        }
        if let Some(severity) = &self.diagnostic.severity {
            html.push_str(&html_pair("severity", severity));
        }
        if let Some(identity) = &self.diagnostic.typed_identity {
            html.push_str(&html_pair("typed identity", identity));
        }
        if let Some(span) = self.diagnostic.span {
            html.push_str(&html_pair("span", &format!("{}..{}", span.start, span.end)));
        }
        html.push_str("</dl>");
        if self.redacted {
            html.push_str("<p class=\"redacted\">Local source, cause, and action details are unavailable outside a local development capability.</p>");
        } else {
            render_source_html(&mut html, &self.source_excerpts);
            if let Some(cause) = &self.related_cause {
                html.push_str("<h2>Related cause</h2><dl>");
                if let Some(code) = &cause.code {
                    html.push_str(&html_pair("code", code));
                }
                if let Some(what) = &cause.what {
                    html.push_str(&html_pair("what", what));
                }
                if let Some(why) = &cause.why {
                    html.push_str(&html_pair("why", why));
                }
                if let Some(fix) = &cause.fix {
                    html.push_str(&html_pair("fix", fix));
                }
                html.push_str("</dl>");
            }
            if !self.action_links.is_empty() {
                html.push_str("<h2>Actions</h2><p>");
                for link in &self.action_links {
                    html.push_str("<a rel=\"noreferrer\" href=\"");
                    html.push_str(&html_escape(&link.href));
                    html.push_str("\">");
                    html.push_str(&html_escape(&link.label));
                    html.push_str("</a>");
                }
                html.push_str("</p>");
            }
        }
        html.push_str("</main></body></html>");
        html
    }
}

fn report_source_excerpts(
    report: &JetErrorReport,
) -> Result<Vec<WebErrorSourceExcerpt>, WebErrorPageError> {
    let mut excerpts = Vec::new();
    for frame in &report.source_journey {
        excerpts.push(source_excerpt_from_journey(frame)?);
    }
    for frame in &report.context_frames {
        excerpts.push(source_excerpt_from_context(frame)?);
    }
    Ok(excerpts)
}

fn source_excerpt_from_journey(
    frame: &JetErrorJourneyFrame,
) -> Result<WebErrorSourceExcerpt, WebErrorPageError> {
    let excerpt = (!frame.note.is_empty()).then(|| frame.note.clone());
    WebErrorSourceExcerpt::without_span(frame.file.clone(), frame.fn_name.clone(), excerpt)?
        .with_location((frame.line > 0).then_some(frame.line), None)
}

fn source_excerpt_from_context(
    frame: &JetErrorContextFrame,
) -> Result<WebErrorSourceExcerpt, WebErrorPageError> {
    WebErrorSourceExcerpt::without_span(
        frame.file.clone(),
        "context".to_string(),
        Some(frame.text.clone()),
    )?
    .with_location((frame.line > 0).then_some(frame.line), None)
}

fn web_span_from_span(span: Span) -> Result<WebErrorSpan, WebErrorPageError> {
    WebErrorSpan::new(span.start, span.end)
}

fn diagnostic_bytes(fact: &WebErrorDiagnosticFact) -> usize {
    fact.code.as_ref().map_or(0, String::len)
        + fact.what.as_ref().map_or(0, String::len)
        + fact.why.as_ref().map_or(0, String::len)
        + fact.fix.as_ref().map_or(0, String::len)
        + fact.severity.as_ref().map_or(0, String::len)
        + fact.moment.as_ref().map_or(0, String::len)
        + fact.typed_identity.as_ref().map_or(0, String::len)
}

fn normalize_identity(value: String, field: &'static str) -> Result<String, WebErrorPageError> {
    validate_identity(&value, field)?;
    Ok(redact_identity(&value))
}

fn normalize_source_identity(value: String) -> Result<String, WebErrorPageError> {
    validate_source_identity(&value)?;
    Ok(redact_identity(&value))
}

fn redact_identity(value: &str) -> String {
    if contains_sensitive_marker(value) {
        "[redacted]".to_string()
    } else {
        value.to_string()
    }
}

fn validate_identity(value: &str, field: &'static str) -> Result<(), WebErrorPageError> {
    if value.is_empty() {
        return Err(WebErrorPageError::EmptyField(field));
    }
    if value.len() > MAX_WEB_ERROR_ID_BYTES {
        return Err(WebErrorPageError::TextTooLarge(field));
    }
    if value.chars().any(char::is_control) {
        return Err(WebErrorPageError::ControlCharacter(field));
    }
    if value.chars().any(|character| {
        character.is_whitespace()
            || matches!(character, '<' | '>' | '"' | '\'' | '`' | '\\')
    }) {
        return Err(WebErrorPageError::InvalidIdentity(field));
    }
    Ok(())
}

fn validate_source_identity(value: &str) -> Result<(), WebErrorPageError> {
    validate_identity(value, "source id")?;
    if value.split('/').any(|part| part == "..") || value.starts_with("//") {
        return Err(WebErrorPageError::InvalidIdentity("source id"));
    }
    Ok(())
}

fn normalize_code(value: String) -> Result<String, WebErrorPageError> {
    validate_code(&value)?;
    Ok(value)
}

fn validate_code(value: &str) -> Result<(), WebErrorPageError> {
    validate_identity(value, "diagnostic code")
}

fn normalize_required_text(
    value: String,
    field: &'static str,
    limit: usize,
) -> Result<String, WebErrorPageError> {
    if value.is_empty() {
        return Err(WebErrorPageError::EmptyField(field));
    }
    let mut normalized = normalize_text(value, field, limit)?;
    if contains_sensitive_marker(&normalized) {
        normalized = "[redacted]".to_string();
    }
    Ok(normalized)
}

fn normalize_optional_text(
    value: Option<String>,
    field: &'static str,
    limit: usize,
) -> Result<Option<String>, WebErrorPageError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    let normalized = normalize_text(value, field, limit)?;
    if contains_sensitive_marker(&normalized) {
        return Ok(None);
    }
    Ok(Some(normalized))
}

fn normalize_text(
    value: String,
    field: &'static str,
    limit: usize,
) -> Result<String, WebErrorPageError> {
    if value.len() > limit {
        return Err(WebErrorPageError::TextTooLarge(field));
    }
    let mut normalized = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\n' | '\r' | '\t' => normalized.push(character),
            character if character.is_control() => normalized.push('�'),
            character => normalized.push(character),
        }
    }
    Ok(normalized)
}

fn validate_optional_text(
    value: &Option<String>,
    field: &'static str,
    limit: usize,
) -> Result<(), WebErrorPageError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.is_empty() {
        return Err(WebErrorPageError::EmptyField(field));
    }
    if value.len() > limit {
        return Err(WebErrorPageError::TextTooLarge(field));
    }
    if value.chars().any(char::is_control) && value.chars().any(|character| {
        !matches!(character, '\n' | '\r' | '\t') && character.is_control()
    }) {
        return Err(WebErrorPageError::ControlCharacter(field));
    }
    if contains_sensitive_marker(value) {
        return Err(WebErrorPageError::UnsafeContent(field));
    }
    Ok(())
}

fn contains_sensitive_marker(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "password=",
        "password:",
        "secret=",
        "secret:",
        "authorization:",
        "authorization=",
        "bearer ",
        "api_key",
        "apikey",
        "access_token",
        "client_secret",
        "private_key",
        "set-cookie",
        "cookie:",
        "sk_live_",
        "ghp_",
        "xoxb-",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn normalize_href(value: String) -> Result<String, WebErrorPageError> {
    validate_href(&value)?;
    Ok(value)
}

fn validate_href(value: &str) -> Result<(), WebErrorPageError> {
    if value.is_empty() || value.len() > MAX_WEB_ERROR_LINK_BYTES {
        return Err(if value.is_empty() {
            WebErrorPageError::EmptyField("action href")
        } else {
            WebErrorPageError::TextTooLarge("action href")
        });
    }
    if value.chars().any(char::is_control) || value.chars().any(char::is_whitespace) {
        return Err(WebErrorPageError::InvalidLink);
    }
    if contains_sensitive_marker(value) || value.contains('@') {
        return Err(WebErrorPageError::InvalidLink);
    }
    let lower = value.to_ascii_lowercase();
    let allowed = (value.starts_with('/') && !value.starts_with("//"))
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("jet://");
    if !allowed
        || lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("file:")
    {
        return Err(WebErrorPageError::InvalidLink);
    }
    Ok(())
}

fn validate_status(status: u16) -> Result<(), WebErrorPageError> {
    if (400..=599).contains(&status) {
        Ok(())
    } else {
        Err(WebErrorPageError::InvalidStatus)
    }
}

fn source_excerpt_order(left: &WebErrorSourceExcerpt, right: &WebErrorSourceExcerpt) -> std::cmp::Ordering {
    left.source_id
        .cmp(&right.source_id)
        .then_with(|| left.label.cmp(&right.label))
        .then_with(|| {
            left.span
                .map(|span| (span.start, span.end))
                .cmp(&right.span.map(|span| (span.start, span.end)))
        })
        .then_with(|| left.line.cmp(&right.line))
        .then_with(|| left.column.cmp(&right.column))
        .then_with(|| left.excerpt.cmp(&right.excerpt))
}

fn action_link_order(left: &WebErrorActionLink, right: &WebErrorActionLink) -> std::cmp::Ordering {
    left.label.cmp(&right.label).then_with(|| left.href.cmp(&right.href))
}
fn web_error_edit_order(left: &WebErrorEdit, right: &WebErrorEdit) -> std::cmp::Ordering {
    left.source_id
        .cmp(&right.source_id)
        .then_with(|| {
            left.span
                .start
                .cmp(&right.span.start)
                .then_with(|| left.span.end.cmp(&right.span.end))
        })
        .then_with(|| left.new_text.cmp(&right.new_text))
        .then_with(|| left.applicability.cmp(&right.applicability))
        .then_with(|| left.safety.cmp(&right.safety))
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", json_escape(value))
}

fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            character => escaped.push(character),
        }
    }
    escaped
}

fn html_pair(label: &str, value: &str) -> String {
    format!(
        "<dt>{}</dt><dd>{}</dd>",
        html_escape(label),
        html_escape(value)
    )
}

fn render_source_html(html: &mut String, excerpts: &[WebErrorSourceExcerpt]) {
    if excerpts.is_empty() {
        return;
    }
    html.push_str("<h2>Source</h2>");
    for excerpt in excerpts {
        html.push_str("<section><h3>");
        html.push_str(&html_escape(&excerpt.label));
        html.push_str("</h3><p><code>");
        html.push_str(&html_escape(&excerpt.source_id));
        if let Some(line) = excerpt.line {
            html.push(':');
            html.push_str(&line.to_string());
            if let Some(column) = excerpt.column {
                html.push(':');
                html.push_str(&column.to_string());
            }
        }
        if let Some(span) = excerpt.span {
            html.push_str(" bytes ");
            html.push_str(&span.start.to_string());
            html.push_str("..");
            html.push_str(&span.end.to_string());
        }
        html.push_str("</code></p>");
        if let Some(text) = &excerpt.excerpt {
            html.push_str("<pre>");
            html.push_str(&html_escape(text));
            html.push_str("</pre>");
        }
        html.push_str("</section>");
    }
}

fn accept_candidate_better(
    candidate: (u16, u8, usize),
    current: Option<(u16, u8, usize)>,
) -> bool {
    current.is_none_or(|current| {
        candidate.1 > current.1
            || (candidate.1 == current.1 && candidate.2 < current.2)
    })
}

fn negotiate_content_type(accept: &str) -> WebErrorContentType {
    // Select the most specific matching range for each representation first.
    // A high-quality wildcard must not override a lower-quality exact range:
    // `application/json;q=0.4, */*;q=1` still makes JSON 0.4, while HTML
    // inherits the wildcard's 1.0.
    let mut html = None;
    let mut json = None;
    for (index, item) in accept.split(',').enumerate() {
        let mut pieces = item.split(';');
        let media = pieces.next().unwrap_or("").trim();
        if media.is_empty() {
            continue;
        }
        let mut quality = 1000;
        let mut valid = true;
        for parameter in pieces {
            let Some((name, value)) = parameter.trim().split_once('=') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("q") {
                match parse_quality(value.trim()) {
                    Some(value) => quality = value,
                    None => valid = false,
                }
            }
        }
        if !valid {
            continue;
        }
        let (content_type, specificity) = if media.eq_ignore_ascii_case("text/html") {
            (Some(WebErrorContentType::Html), 2)
        } else if media.eq_ignore_ascii_case("application/json") {
            (Some(WebErrorContentType::Json), 2)
        } else if media.eq_ignore_ascii_case("text/*") {
            (Some(WebErrorContentType::Html), 1)
        } else if media.eq_ignore_ascii_case("application/*") {
            (Some(WebErrorContentType::Json), 1)
        } else if media == "*/*" || media == "*" {
            (None, 0)
        } else {
            continue;
        };
        let candidate = (quality, specificity, index);
        match content_type {
            Some(WebErrorContentType::Html) => {
                if accept_candidate_better(candidate, html) {
                    html = Some(candidate);
                }
            }
            Some(WebErrorContentType::Json) => {
                if accept_candidate_better(candidate, json) {
                    json = Some(candidate);
                }
            }
            None => {
                if accept_candidate_better(candidate, html) {
                    html = Some(candidate);
                }
                if accept_candidate_better(candidate, json) {
                    json = Some(candidate);
                }
            }
        }
    }

    match (html, json) {
        (Some(html), Some(json))
            if json.0 > html.0
                || (json.0 == html.0
                    && (json.1 > html.1 || (json.1 == html.1 && json.2 < html.2))) =>
        {
            WebErrorContentType::Json
        }
        (Some(_), _) | (None, None) => WebErrorContentType::Html,
        (None, Some(_)) => WebErrorContentType::Json,
    }
}

fn parse_quality(value: &str) -> Option<u16> {
    if value == "0" {
        return Some(0);
    }
    if value == "1" {
        return Some(1000);
    }
    let (whole, fraction) = value.split_once('.')?;
    if whole != "0" && whole != "1" {
        return None;
    }
    if fraction.is_empty() || fraction.len() > 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut padded = fraction.to_string();
    while padded.len() < 3 {
        padded.push('0');
    }
    let fractional = padded.parse::<u16>().ok()?;
    if whole == "1" && fractional != 0 {
        None
    } else {
        Some(if whole == "1" { 1000 } else { fractional })
    }
}
