// Shared machine-report value model.
//
// This file is dependency-free because the AOT test harness includes the
// same source. CLI, LSP, Foundation JSON, and generated test reports all
// serialize this envelope instead of maintaining separate protocol shapes.

pub const REPORT_NAME: &str = "jet.report";
pub const REPORT_VERSION: u32 = 2;
pub const REPORT_SCHEMA: &str = "jet.report/v2";
pub const STATUS_NAME: &str = "jet.status";
pub const STATUS_VERSION: u32 = 1;
pub const STATUS_SCHEMA: &str = "jet.status/v1";

/// Why a diagnostic has no machine-applicable edit.
///
/// The next action is deliberately part of the value instead of being
/// inferred from the kind. A reason is useful only when it tells a person
/// what to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoFixReasonKind {
    Behavior,
    Design,
    Ambiguous,
}

impl NoFixReasonKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Behavior => "behavior",
            Self::Design => "design",
            Self::Ambiguous => "ambiguous",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "behavior" => Ok(Self::Behavior),
            "design" => Ok(Self::Design),
            "ambiguous" => Ok(Self::Ambiguous),
            _ => Err(format!("unknown no-fix reason kind `{value}`")),
        }
    }
}

/// A reviewed action for a diagnostic that cannot carry an automatic edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoFixReason {
    pub kind: NoFixReasonKind,
    pub next: String,
}

impl NoFixReason {
    pub fn new(kind: NoFixReasonKind, next: impl Into<String>) -> Self {
        let reason = Self { kind, next: next.into() };
        reason
            .validate()
            .expect("NoFixReason::new requires a reviewed next action");
        reason
    }

    pub fn try_new(kind: NoFixReasonKind, next: impl Into<String>) -> Result<Self, String> {
        let reason = Self { kind, next: next.into() };
        reason.validate()?;
        Ok(reason)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.next.trim().is_empty() {
            return Err("no-fix reason next action must not be empty".to_string());
        }
        if self.next.chars().any(char::is_control) {
            return Err("no-fix reason next action must not contain control characters".to_string());
        }
        Ok(())
    }
}

/// A typed JSON value accepted as an action-specific status field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusValue {
    Null,
    Bool(bool),
    Integer(i128),
    Float(String),
    String(String),
    Array(Vec<Self>),
    Object(StatusFields),
}

impl StatusValue {
    pub fn object(fields: StatusFields) -> Self {
        Self::Object(fields)
    }

    pub fn array(values: impl IntoIterator<Item = Self>) -> Self {
        Self::Array(values.into_iter().collect())
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let value = crate::JSON::parse(text)?;
        Self::from_data_tree(&value)
    }

    pub fn from_data_tree(value: &crate::DataTree::DataTree) -> Result<Self, String> {
        match value {
            crate::DataTree::DataTree::Null => Ok(Self::Null),
            crate::DataTree::DataTree::Bool(value) => Ok(Self::Bool(*value)),
            crate::DataTree::DataTree::Int(value) => Ok(Self::Integer(i128::from(*value))),
            crate::DataTree::DataTree::Float(value) if value.is_finite() => {
                Ok(Self::Float(value.to_string()))
            }
            crate::DataTree::DataTree::Float(_) => {
                Err("status fields cannot contain non-finite numbers".to_string())
            }
            crate::DataTree::DataTree::Number(value) => {
                if let Ok(parsed) = value.parse::<i128>() {
                    Ok(Self::Integer(parsed))
                } else {
                    Ok(Self::Float(value.clone()))
                }
            }
            crate::DataTree::DataTree::TypedText(value)
            | crate::DataTree::DataTree::Text(value) => Ok(Self::String(value.clone())),
            crate::DataTree::DataTree::Bytes(values) => Ok(Self::Array(
                values
                    .iter()
                    .map(|value| Self::Integer(i128::from(*value)))
                    .collect(),
            )),
            crate::DataTree::DataTree::Array(values) => Ok(Self::Array(
                values
                    .iter()
                    .map(Self::from_data_tree)
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            crate::DataTree::DataTree::Object(values) => {
                Ok(Self::Object(StatusFields::from_data_tree_object(values)?))
            }
        }
    }

    fn json(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
            Self::Float(value) => value.clone(),
            Self::String(value) => report_json_string(value),
            Self::Array(values) => format!(
                "[{}]",
                values.iter().map(Self::json).collect::<Vec<_>>().join(",")
            ),
            Self::Object(fields) => format!("{{{}}}", fields.json()),
        }
    }
}

impl From<bool> for StatusValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i128> for StatusValue {
    fn from(value: i128) -> Self {
        Self::Integer(value)
    }
}

impl From<i64> for StatusValue {
    fn from(value: i64) -> Self {
        Self::Integer(i128::from(value))
    }
}

impl From<i32> for StatusValue {
    fn from(value: i32) -> Self {
        Self::Integer(i128::from(value))
    }
}

impl From<u64> for StatusValue {
    fn from(value: u64) -> Self {
        Self::Integer(i128::from(value))
    }
}

impl From<u32> for StatusValue {
    fn from(value: u32) -> Self {
        Self::Integer(i128::from(value))
    }
}

impl From<usize> for StatusValue {
    fn from(value: usize) -> Self {
        Self::Integer(value as i128)
    }
}

impl From<String> for StatusValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for StatusValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

/// Ordered, typed action-specific fields for a status object.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusFields(Vec<(String, StatusValue)>);

impl StatusFields {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn with(mut self, name: impl Into<String>, value: impl Into<StatusValue>) -> Self {
        self.insert(name, value)
            .expect("status fields must have unique, non-empty names");
        self
    }

    pub fn try_with(
        mut self,
        name: impl Into<String>,
        value: impl Into<StatusValue>,
    ) -> Result<Self, String> {
        self.insert(name, value)?;
        Ok(self)
    }

    pub fn insert(
        &mut self,
        name: impl Into<String>,
        value: impl Into<StatusValue>,
    ) -> Result<(), String> {
        let name = name.into();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err("status field names must be non-empty and printable".to_string());
        }
        if self.0.iter().any(|(existing, _)| existing == &name) {
            return Err(format!("duplicate status field `{name}`"));
        }
        self.0.push((name, value.into()));
        Ok(())
    }

    pub fn extend(&mut self, fields: Self) -> Result<(), String> {
        for (name, value) in fields.0 {
            self.insert(name, value)?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &StatusValue)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }

    fn from_data_tree_object(
        value: &[(String, crate::DataTree::DataTree)],
    ) -> Result<Self, String> {
        let mut fields = Self::new();
        for (name, value) in value {
            fields.insert(name.clone(), StatusValue::from_data_tree(value)?)?;
        }
        Ok(fields)
    }


    fn json(&self) -> String {
        self.0
            .iter()
            .map(|(name, value)| format!("{}:{}", report_json_string(name), value.json()))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// The one machine status object for every command `--json` result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusEnvelope {
    pub action: String,
    pub ok: bool,
    pub reports: Vec<ReportEnvelope>,
    pub fields: StatusFields,
}

impl StatusEnvelope {
    pub fn new(action: impl Into<String>, ok: bool) -> Self {
        Self {
            action: action.into(),
            ok,
            reports: Vec::new(),
            fields: StatusFields::new(),
        }
    }

    pub fn with_report(mut self, report: ReportEnvelope) -> Self {
        self.reports.push(report);
        self
    }

    pub fn with_reports(mut self, reports: impl IntoIterator<Item = ReportEnvelope>) -> Self {
        self.reports.extend(reports);
        self
    }

    pub fn with_field(
        mut self,
        name: impl Into<String>,
        value: impl Into<StatusValue>,
    ) -> Self {
        self.fields = self.fields.with(name, value);
        self
    }

    pub fn with_fields(mut self, fields: StatusFields) -> Self {
        self.fields
            .extend(fields)
            .expect("status fields must not duplicate envelope fields");
        self
    }

    pub fn try_with_fields(mut self, fields: StatusFields) -> Result<Self, String> {
        self.fields.extend(fields)?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.action.is_empty() {
            return Err("status action must not be empty".to_string());
        }
        if self.action.chars().any(char::is_control) {
            return Err("status action must not contain control characters".to_string());
        }
        for (name, _) in self.fields.iter() {
            if matches!(name, "schema" | "action" | "ok" | "reports") {
                return Err(format!("status field `{name}` is reserved"));
            }
        }
        for report in &self.reports {
            report.validate()?;
        }
        Ok(())
    }

    pub fn json(&self) -> String {
        self.validate().expect("invalid jet.status/v1 envelope");
        let mut out = format!(
            "{{\"schema\":{},\"action\":{},\"ok\":{},\"reports\":[",
            report_json_string(STATUS_SCHEMA),
            report_json_string(&self.action),
            self.ok
        );
        for (index, report) in self.reports.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&report.json());
        }
        out.push(']');
        let fields = self.fields.json();
        if !fields.is_empty() {
            out.push(',');
            out.push_str(&fields);
        }
        out.push('}');
        out
    }

    pub fn json_line(&self) -> String {
        let mut out = self.json();
        out.push('\n');
        out
    }
}
/// Render a typed status envelope without exposing the serialization details
/// at a command boundary.
///
/// `StatusFields` is deliberately the public input. This keeps status
/// producers from assembling untyped JSON fragments and makes the reserved
/// envelope keys fail through the same validator as every other producer.
pub fn render_status(
    action: impl Into<String>,
    ok: bool,
    fields: StatusFields,
) -> String {
    StatusEnvelope::new(action, ok)
        .try_with_fields(fields)
        .expect("status fields must not duplicate envelope fields")
        .json()
}

/// Render one status object with typed report children.
pub fn render_status_with_reports(
    action: impl Into<String>,
    ok: bool,
    reports: impl IntoIterator<Item = ReportEnvelope>,
    fields: StatusFields,
) -> String {
    StatusEnvelope::new(action, ok)
        .with_reports(reports)
        .try_with_fields(fields)
        .expect("status fields must not duplicate envelope fields")
        .json()
}



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
    BuildError {
        report: String,
    },
}

/// A `jet.report/v2` diagnostic or machine finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportEnvelope {
    pub schema_name: &'static str,
    pub schema_version: u32,
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
    fix_edits: Vec<ReportEdit>,
    pub cause: Vec<String>,
    pub clears: usize,
    pub extension: Option<ReportExtension>,
    no_fix_reason: Option<NoFixReason>,
    pub denial_kind: Option<String>,
    pub call_chain: Vec<String>,
    pub scope_chain: Vec<String>,
    pub nearest_granting_scope: Option<String>,
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
            schema_name: REPORT_NAME,
            schema_version: REPORT_VERSION,
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
            no_fix_reason: None,
            denial_kind: None,
            call_chain: Vec::new(),
            scope_chain: Vec::new(),
            nearest_granting_scope: None,
        }
    }

    pub fn fix_edits(&self) -> &[ReportEdit] {
        &self.fix_edits
    }

    pub fn no_fix_reason(&self) -> Option<&NoFixReason> {
        self.no_fix_reason.as_ref()
    }

    pub fn push_fix_edit(&mut self, edit: ReportEdit) -> Result<(), String> {
        if self.no_fix_reason.is_some() {
            return Err("a report cannot carry fix_edits and no_fix_reason".to_string());
        }
        self.fix_edits.push(edit);
        Ok(())
    }

    pub fn with_fix_edits(
        mut self,
        edits: impl IntoIterator<Item = ReportEdit>,
    ) -> Result<Self, String> {
        for edit in edits {
            self.push_fix_edit(edit)?;
        }
        Ok(self)
    }

    pub fn set_no_fix_reason(&mut self, reason: NoFixReason) -> Result<(), String> {
        reason.validate()?;
        if !self.fix_edits.is_empty() {
            return Err("a report cannot carry fix_edits and no_fix_reason".to_string());
        }
        self.no_fix_reason = Some(reason);
        Ok(())
    }

    pub fn with_no_fix_reason(mut self, reason: NoFixReason) -> Result<Self, String> {
        self.set_no_fix_reason(reason)?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_name != REPORT_NAME || self.schema_version != REPORT_VERSION {
            return Err("report envelope has an unsupported schema identity".to_string());
        }
        if let Some(reason) = &self.no_fix_reason {
            reason.validate()?;
            if !self.fix_edits.is_empty() {
                return Err("a report cannot carry fix_edits and no_fix_reason".to_string());
            }
        }
        Ok(())
    }
    pub fn json(&self) -> String {
        self.validate().expect("invalid jet.report/v2 envelope");
        let mut out = String::from("{\"schema\":");
        let schema = format!("{}/v{}", self.schema_name, self.schema_version);
        out.push_str(&report_json_string(&schema));
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
        out.push_str(",\"denial_kind\":");
        match &self.denial_kind {
            Some(kind) => out.push_str(&report_json_string(kind)),
            None => out.push_str("null"),
        }
        out.push_str(",\"call_chain\":");
        out.push_str(&report_json_strings(&self.call_chain));
        out.push_str(",\"scope_chain\":");
        out.push_str(&report_json_strings(&self.scope_chain));
        out.push_str(",\"nearest_granting_scope\":");
        match &self.nearest_granting_scope {
            Some(scope) => out.push_str(&report_json_string(scope)),
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
        if !self.fix_edits.is_empty() {
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
            out.push(']');
        } else if let Some(reason) = &self.no_fix_reason {
            out.push_str(",\"no_fix_reason\":{");
            out.push_str("\"kind\":");
            out.push_str(&report_json_string(reason.kind.as_str()));
            out.push_str(",\"next\":");
            out.push_str(&report_json_string(&reason.next));
            out.push('}');
        } else {
            out.push_str(",\"fix_edits\":[]");
        }
        out.push_str(",\"cause\":[");
        for (index, cause) in self.cause.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&report_json_string(cause));
        }
        out.push(']');
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
        if let Some(ReportExtension::BuildError { report }) = &self.extension {
            out.push_str(",\"build_error\":");
            out.push_str(report);
        }
        out.push('}');
        out
    }

    pub fn json_line(&self) -> String {
        let mut out = self.json();
        out.push('\n');
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

fn report_json_strings(values: &[String]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&report_json_string(value));
    }
    out.push(']');
    out
}
