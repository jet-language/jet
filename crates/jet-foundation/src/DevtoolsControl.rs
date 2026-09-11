//! Host-only control protocol for the resident devtools session.
//!
//! Observation facts stay in [`crate::Devtools`].  These values are never
//! embedded in a generated runtime: they carry only a checked identity and
//! bounded limits across the host command boundary.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_COMMANDS: usize = crate::Devtools::JET_DEVTOOLS_MAX_EVENTS;
pub const JET_DEVTOOLS_MAX_COMMANDS: usize = MAX_COMMANDS;
pub const JET_DEVTOOLS_MAX_COMMAND_ROWS: usize = 256;

fn validate_text(value: &str, field: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("jet.devtools.v1 {field} must not be empty"));
    }
    if value.len() > crate::Devtools::JET_DEVTOOLS_MAX_TEXT_BYTES {
        return Err(format!(
            "jet.devtools.v1 {field} exceeds the Prelude text limit"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "jet.devtools.v1 {field} contains a control character"
        ));
    }
    Ok(())
}

fn render_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0c}' => escaped.push_str("\\f"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                escaped.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => escaped.push(character),
        }
    }
    format!("\"{escaped}\"")
}

/// One internal database EXPLAIN request crossing the resident devtools
/// session. SQL text, bindings, credentials, and authority facts stay in the
/// owning runtime; this DTO carries only the checked identity and limits.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseExplainRequest {
    pub session_id: String,
    pub request_id: String,
    pub statement_identity: String,
    pub timeout_ms: u64,
    pub max_rows: u64,
}

impl JetDevtoolsDatabaseExplainRequest {
    pub fn new(
        session_id: impl Into<String>,
        request_id: impl Into<String>,
        statement_identity: impl Into<String>,
        timeout_ms: u64,
        max_rows: u64,
    ) -> Result<Self, String> {
        let request = Self {
            session_id: session_id.into(),
            request_id: request_id.into(),
            statement_identity: statement_identity.into(),
            timeout_ms,
            max_rows,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_text(&self.session_id, "session_id")?;
        validate_text(&self.request_id, "request_id")?;
        validate_text(&self.statement_identity, "statement_identity")?;
        if self.timeout_ms == 0 || self.timeout_ms > 30_000 {
            return Err("jet.devtools.v1 EXPLAIN timeout is outside its limit".to_string());
        }
        if self.max_rows == 0 || self.max_rows > JET_DEVTOOLS_MAX_COMMAND_ROWS as u64 {
            return Err("jet.devtools.v1 EXPLAIN row limit is outside its limit".to_string());
        }
        Ok(())
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"session_id\":{},\"request_id\":{},\"statement_identity\":{},\"timeout_ms\":{},\"max_rows\":{}}}",
            render_text(&self.session_id),
            render_text(&self.request_id),
            render_text(&self.statement_identity),
            self.timeout_ms,
            self.max_rows,
        )
    }
}

/// One redacted structural EXPLAIN row. The driver may add detail, but never
/// sends SQL literals, parameter values, or credentials through this record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDbPlanRow {
    pub id: String,
    pub parent: Option<String>,
    pub detail: String,
}

impl JetDevtoolsDbPlanRow {
    pub fn new(
        id: impl Into<String>,
        parent: Option<String>,
        detail: impl Into<String>,
    ) -> Result<Self, String> {
        let row = Self {
            id: id.into(),
            parent,
            detail: detail.into(),
        };
        row.validate()?;
        Ok(row)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_text(&self.id, "plan row id")?;
        if let Some(parent) = &self.parent {
            validate_text(parent, "plan row parent")?;
        }
        validate_text(&self.detail, "plan row detail")?;
        Ok(())
    }

    fn render_json(&self) -> String {
        format!(
            "{{\"id\":{},\"parent\":{},\"detail\":{}}}",
            render_text(&self.id),
            self.parent
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            render_text(&self.detail),
        )
    }
}

/// The typed result returned by a database handler. Detailed authority and
/// driver state stay out of this transport projection; errors are bounded and
/// redacted strings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseExplainResult {
    pub session_id: String,
    pub request_id: String,
    pub statement_identity: String,
    pub status: String,
    pub elapsed_ns: u64,
    pub truncated: bool,
    pub row_count: u64,
    pub plan_rows: Vec<JetDevtoolsDbPlanRow>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

impl JetDevtoolsDatabaseExplainResult {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: impl Into<String>,
        request_id: impl Into<String>,
        statement_identity: impl Into<String>,
        status: impl Into<String>,
        elapsed_ns: u64,
        truncated: bool,
        row_count: u64,
        plan_rows: Vec<JetDevtoolsDbPlanRow>,
        error_code: Option<String>,
        error_message: Option<String>,
    ) -> Result<Self, String> {
        if plan_rows.len() > JET_DEVTOOLS_MAX_COMMAND_ROWS {
            return Err("jet.devtools.v1 EXPLAIN result has too many plan rows".to_string());
        }
        let result = Self {
            session_id: session_id.into(),
            request_id: request_id.into(),
            statement_identity: statement_identity.into(),
            status: status.into(),
            elapsed_ns,
            truncated,
            row_count,
            plan_rows,
            error_code,
            error_message,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_text(&self.session_id, "session_id")?;
        validate_text(&self.request_id, "request_id")?;
        validate_text(&self.statement_identity, "statement_identity")?;
        validate_text(&self.status, "status")?;
        if self.plan_rows.len() > JET_DEVTOOLS_MAX_COMMAND_ROWS {
            return Err("jet.devtools.v1 EXPLAIN result has too many plan rows".to_string());
        }
        for row in &self.plan_rows {
            row.validate()?;
        }
        if let Some(code) = &self.error_code {
            validate_text(code, "error code")?;
        }
        if let Some(message) = &self.error_message {
            validate_text(message, "error message")?;
        }
        Ok(())
    }

    pub fn render_json(&self) -> String {
        let rows = self
            .plan_rows
            .iter()
            .map(JetDevtoolsDbPlanRow::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"session_id\":{},\"request_id\":{},\"statement_identity\":{},\"status\":{},\"elapsed_ns\":{},\"truncated\":{},\"row_count\":{},\"plan_rows\":[{}],\"error_code\":{},\"error_message\":{}}}",
            render_text(&self.session_id),
            render_text(&self.request_id),
            render_text(&self.statement_identity),
            render_text(&self.status),
            self.elapsed_ns,
            self.truncated,
            self.row_count,
            rows,
            self.error_code
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.error_message
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
        )
    }
}

/// Game controls are the only interactive commands that cross the resident
/// `jet.devtools.v1` boundary.  The operation is closed so a host cannot
/// smuggle an arbitrary evaluator or an unrecognised state transition into a
/// live game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDevtoolsGameControlKind {
    Play,
    Simulate,
    Pause,
    Step,
    Resume,
    FrameAdvance,
    Edit,
    Eject,
    Keep,
    Discard,
    SelectCategory,
    SelectWorld,
    EditWorld,
    EvaluateWorld,
}

impl JetDevtoolsGameControlKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Simulate => "simulate",
            Self::Pause => "pause",
            Self::Step => "step",
            Self::Resume => "resume",
            Self::FrameAdvance => "frame_advance",
            Self::Edit => "edit",
            Self::Eject => "eject",
            Self::Keep => "keep",
            Self::Discard => "discard",
            Self::SelectCategory => "select_category",
            Self::SelectWorld => "select_world",
            Self::EditWorld => "edit_world",
            Self::EvaluateWorld => "evaluate_world",
        }
    }

    pub fn from_str(value: &str) -> Result<Self, String> {
        match value {
            "play" => Ok(Self::Play),
            "simulate" => Ok(Self::Simulate),
            "pause" => Ok(Self::Pause),
            "step" => Ok(Self::Step),
            "resume" => Ok(Self::Resume),
            "frame_advance" => Ok(Self::FrameAdvance),
            "edit" => Ok(Self::Edit),
            "eject" => Ok(Self::Eject),
            "keep" => Ok(Self::Keep),
            "discard" => Ok(Self::Discard),
            "select_category" => Ok(Self::SelectCategory),
            "select_world" => Ok(Self::SelectWorld),
            "edit_world" => Ok(Self::EditWorld),
            "evaluate_world" => Ok(Self::EvaluateWorld),
            _ => Err(format!(
                "jet.devtools.v1 game control operation `{value}` is unsupported"
            )),
        }
    }
}

fn render_optional_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

/// One checked game control request.  Optional fields are used only by the
/// operation that owns them; validation rejects missing provenance before the
/// request enters either the resident queue or the generated runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsGameControlRequest {
    pub session_id: String,
    pub request_id: String,
    pub kind: JetDevtoolsGameControlKind,
    pub source_id: Option<String>,
    pub revision: Option<String>,
    pub world_id: Option<String>,
    pub actor_id: Option<String>,
    pub component_id: Option<String>,
    pub authored_instance_id: Option<String>,
    pub source_span_start: Option<u64>,
    pub source_span_end: Option<u64>,
    pub field: Option<String>,
    pub value: Option<String>,
    pub category: Option<String>,
    pub expression: Option<String>,
    pub required_authority: Option<String>,
    pub frame_id: Option<u64>,
    pub budget: Option<u64>,
}

fn required_game_control_field<'a>(
    value: Option<&'a String>,
    kind: JetDevtoolsGameControlKind,
    field: &str,
) -> Result<&'a String, String> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("game control `{}` requires {field}", kind.as_str()))
}
impl JetDevtoolsGameControlRequest {
    pub fn new(
        session_id: impl Into<String>,
        request_id: impl Into<String>,
        operation: JetDevtoolsGameControlKind,
    ) -> Result<Self, String> {
        let request = Self {
            session_id: session_id.into(),
            request_id: request_id.into(),
            kind: operation,
            source_id: None,
            revision: None,
            world_id: None,
            actor_id: None,
            component_id: None,
            authored_instance_id: None,
            source_span_start: None,
            source_span_end: None,
            field: None,
            value: None,
            category: None,
            expression: None,
            required_authority: None,
            frame_id: None,
            budget: None,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_text(&self.session_id, "game control session_id")?;
        validate_text(&self.request_id, "game control request_id")?;
        let optional_text = [
            ("source_id", self.source_id.as_deref()),
            ("revision", self.revision.as_deref()),
            ("world_id", self.world_id.as_deref()),
            ("actor_id", self.actor_id.as_deref()),
            ("component_id", self.component_id.as_deref()),
            ("authored_instance_id", self.authored_instance_id.as_deref()),
            ("field", self.field.as_deref()),
            ("value", self.value.as_deref()),
            ("category", self.category.as_deref()),
            ("expression", self.expression.as_deref()),
            ("required_authority", self.required_authority.as_deref()),
        ];
        for (field, value) in optional_text {
            if let Some(value) = value {
                validate_text(value, &format!("game control {field}"))?;
            }
        }
        match self.kind {
            JetDevtoolsGameControlKind::Play | JetDevtoolsGameControlKind::Simulate => {
                required_game_control_field(self.source_id.as_ref(), self.kind, "source_id")?;
                required_game_control_field(self.revision.as_ref(), self.kind, "revision")?;
            }
            JetDevtoolsGameControlKind::Eject => {
                required_game_control_field(self.source_id.as_ref(), self.kind, "source_id")?;
                required_game_control_field(self.revision.as_ref(), self.kind, "revision")?;
                required_game_control_field(self.world_id.as_ref(), self.kind, "world_id")?;
                required_game_control_field(self.actor_id.as_ref(), self.kind, "actor_id")?;
            }
            JetDevtoolsGameControlKind::Keep | JetDevtoolsGameControlKind::EditWorld => {
                required_game_control_field(self.source_id.as_ref(), self.kind, "source_id")?;
                required_game_control_field(self.revision.as_ref(), self.kind, "revision")?;
                required_game_control_field(
                    self.authored_instance_id.as_ref(),
                    self.kind,
                    "authored_instance_id",
                )?;
                required_game_control_field(self.field.as_ref(), self.kind, "field")?;
                required_game_control_field(self.value.as_ref(), self.kind, "value")?;
                if self.source_span_start.is_none() || self.source_span_end.is_none() {
                    return Err(format!(
                        "game control `{}` requires source_span_start and source_span_end",
                        self.kind.as_str()
                    ));
                }
            }
            JetDevtoolsGameControlKind::SelectCategory => {
                required_game_control_field(self.category.as_ref(), self.kind, "category")?;
            }
            JetDevtoolsGameControlKind::SelectWorld => {
                required_game_control_field(self.world_id.as_ref(), self.kind, "world_id")?;
                if self.component_id.is_some() && self.actor_id.is_none() {
                    return Err("select_world component_id requires actor_id".to_string());
                }
                if self.field.is_some() && self.component_id.is_none() {
                    return Err("select_world field requires component_id".to_string());
                }
            }
            JetDevtoolsGameControlKind::EvaluateWorld => {
                required_game_control_field(self.world_id.as_ref(), self.kind, "world_id")?;
                required_game_control_field(self.actor_id.as_ref(), self.kind, "actor_id")?;
                required_game_control_field(
                    self.component_id.as_ref(),
                    self.kind,
                    "component_id",
                )?;
                required_game_control_field(
                    self.expression.as_ref(),
                    self.kind,
                    "expression",
                )?;
                required_game_control_field(
                    self.required_authority.as_ref(),
                    self.kind,
                    "required_authority",
                )?;
                if self.frame_id.is_none() {
                    return Err("evaluate_world requires frame_id".to_string());
                }
                if self.budget.is_none_or(|budget| budget == 0) {
                    return Err("evaluate_world requires a positive budget".to_string());
                }
            }
            JetDevtoolsGameControlKind::Pause
            | JetDevtoolsGameControlKind::Step
            | JetDevtoolsGameControlKind::Resume
            | JetDevtoolsGameControlKind::FrameAdvance
            | JetDevtoolsGameControlKind::Edit
            | JetDevtoolsGameControlKind::Discard => {}
        }
        match (self.source_span_start, self.source_span_end) {
            (Some(start), Some(end)) if start < end => {}
            (Some(_), Some(_)) => {
                return Err("game control source span must end after it starts".to_string())
            }
            (None, None) => {}
            _ => return Err("game control source span must include both bounds".to_string()),
        }
        Ok(())
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"session_id\":{},\"request_id\":{},\"kind\":{},\
\"source_id\":{},\"revision\":{},\"world_id\":{},\"actor_id\":{},\
\"component_id\":{},\"authored_instance_id\":{},\"source_span_start\":{},\
\"source_span_end\":{},\"field\":{},\"value\":{},\"category\":{},\
\"expression\":{},\"required_authority\":{},\"frame_id\":{},\"budget\":{}}}",
            render_text(&self.session_id),
            render_text(&self.request_id),
            render_text(self.kind.as_str()),
            self.source_id
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.revision
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.world_id
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.actor_id
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.component_id
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.authored_instance_id
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            render_optional_u64(self.source_span_start),
            render_optional_u64(self.source_span_end),
            self.field
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.value
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.category
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.expression
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            self.required_authority
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
            render_optional_u64(self.frame_id),
            render_optional_u64(self.budget),
        )
    }
}

/// A checked browser-origin request to rebuild the current project through the
/// resident dev loop.  The source revision is admission-bound; the driver
/// must not reinterpret this as an arbitrary evaluator or a file-mtime hint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsProjectRebuildRequest {
    pub session_id: String,
    pub request_id: String,
    pub expected_revision: String,
}

impl JetDevtoolsProjectRebuildRequest {
    pub fn new(
        session_id: impl Into<String>,
        request_id: impl Into<String>,
        expected_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let request = Self {
            session_id: session_id.into(),
            request_id: request_id.into(),
            expected_revision: expected_revision.into(),
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_text(&self.session_id, "project rebuild session_id")?;
        validate_text(&self.request_id, "project rebuild request_id")?;
        validate_text(&self.expected_revision, "project rebuild expected_revision")?;
        Ok(())
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"session_id\":{},\"request_id\":{},\"expected_revision\":{}}}",
            render_text(&self.session_id),
            render_text(&self.request_id),
            render_text(&self.expected_revision),
        )
    }
}

/// Closed internal command vocabulary. New command kinds require a protocol
/// decision; hosts do not register arbitrary handlers through this type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetDevtoolsCommand {
    DatabaseExplain(JetDevtoolsDatabaseExplainRequest),
    GameControl(JetDevtoolsGameControlRequest),
    ProjectRebuild(JetDevtoolsProjectRebuildRequest),
}

impl JetDevtoolsCommand {
    pub fn session_id(&self) -> &str {
        match self {
            Self::DatabaseExplain(request) => &request.session_id,
            Self::GameControl(request) => &request.session_id,
            Self::ProjectRebuild(request) => &request.session_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::DatabaseExplain(request) => request.validate(),
            Self::GameControl(request) => request.validate(),
            Self::ProjectRebuild(request) => request.validate(),
        }
    }

    pub fn render_json(&self) -> String {
        match self {
            Self::DatabaseExplain(request) => request.render_json(),
            Self::GameControl(request) => request.render_json(),
            Self::ProjectRebuild(request) => request.render_json(),
        }
    }

    pub const fn kind(&self) -> &'static str {
        match self {
            Self::DatabaseExplain(_) => "DatabaseExplain",
            Self::GameControl(_) => "GameControl",
            Self::ProjectRebuild(_) => "ProjectRebuild",
        }
    }

    pub fn request_id(&self) -> &str {
        match self {
            Self::DatabaseExplain(request) => &request.request_id,
            Self::GameControl(request) => &request.request_id,
            Self::ProjectRebuild(request) => &request.request_id,
        }
    }
}

/// A bounded admission/dispatch receipt for one host command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsCommandReceipt {
    pub sequence: u64,
    pub session_id: String,
    pub request_id: String,
    pub kind: String,
    pub status: String,
    pub error: Option<String>,
}

impl JetDevtoolsCommandReceipt {
    fn queued(command: &JetDevtoolsCommand) -> Self {
        Self {
            sequence: NEXT_COMMAND_RECEIPT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            session_id: command.session_id().to_string(),
            request_id: command.request_id().to_string(),
            kind: command.kind().to_string(),
            status: "queued".to_string(),
            error: None,
        }
    }
    pub fn for_command(
        command: &JetDevtoolsCommand,
        status: impl Into<String>,
        error: Option<String>,
    ) -> Self {
        Self {
            sequence: NEXT_COMMAND_RECEIPT_SEQUENCE.fetch_add(1, Ordering::Relaxed),
            session_id: command.session_id().to_string(),
            request_id: command.request_id().to_string(),
            kind: command.kind().to_string(),
            status: status.into(),
            error,
        }
    }


    pub fn validate(&self) -> Result<(), String> {
        validate_text(&self.session_id, "command receipt session_id")?;
        validate_text(&self.request_id, "command receipt request_id")?;
        validate_text(&self.kind, "command receipt kind")?;
        validate_text(&self.status, "command receipt status")?;
        if let Some(error) = self.error.as_deref() {
            validate_text(error, "command receipt error")?;
        }
        Ok(())
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"sequence\":{},\"session_id\":{},\"request_id\":{},\"kind\":{},\
\"status\":{},\"error\":{}}}",
            self.sequence,
            render_text(&self.session_id),
            render_text(&self.request_id),
            render_text(&self.kind),
            render_text(&self.status),
            self.error
                .as_deref()
                .map(render_text)
                .unwrap_or_else(|| "null".to_string()),
        )
    }
}

static NEXT_COMMAND_RECEIPT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static JET_DEVTOOLS_COMMAND_RECEIPTS: std::sync::LazyLock<
    std::sync::Mutex<BTreeMap<String, VecDeque<JetDevtoolsCommandReceipt>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(BTreeMap::new()));

fn command_receipt_cleanup(
    receipts: &mut BTreeMap<String, VecDeque<JetDevtoolsCommandReceipt>>,
    session_id: &str,
) {
    if receipts
        .get(session_id)
        .is_some_and(std::collections::VecDeque::is_empty)
    {
        receipts.remove(session_id);
    }
}

/// Update the queued receipt for one request without adding a second stream row.
pub fn jet_devtools_update_command_receipt(
    session_id: &str,
    request_id: &str,
    status: impl Into<String>,
    error: Option<String>,
) -> Result<(), String> {
    validate_text(session_id, "command receipt session_id")?;
    validate_text(request_id, "command receipt request_id")?;
    let status = status.into();
    validate_text(&status, "command receipt status")?;
    if let Some(error) = error.as_deref() {
        validate_text(error, "command receipt error")?;
    }
    let mut receipts = JET_DEVTOOLS_COMMAND_RECEIPTS
        .lock()
        .map_err(|_| "jet.devtools.v1 command receipt queue is poisoned".to_string())?;
    let receipt = receipts
        .get_mut(session_id)
        .and_then(|queue| queue.iter_mut().rev().find(|receipt| {
            receipt.request_id == request_id
        }))
        .ok_or_else(|| "jet.devtools.v1 command receipt was not retained".to_string())?;
    receipt.status = status;
    receipt.error = error;
    Ok(())
}

/// Append one bounded receipt to the session stream.
pub fn jet_devtools_record_command_receipt(
    receipt: JetDevtoolsCommandReceipt,
) -> Result<(), String> {
    receipt.validate()?;
    let session_id = receipt.session_id.clone();
    let mut receipts = JET_DEVTOOLS_COMMAND_RECEIPTS
        .lock()
        .map_err(|_| "jet.devtools.v1 command receipt queue is poisoned".to_string())?;
    let queue = receipts.entry(session_id).or_insert_with(VecDeque::new);
    if queue.len() >= JET_DEVTOOLS_MAX_COMMANDS {
        return Err("jet.devtools.v1 command receipt queue is full".to_string());
    }
    queue.push_back(receipt);
    Ok(())
}

/// Remove the oldest receipt for one exact session.
pub fn jet_devtools_poll_command_receipt(
    session_id: &str,
) -> Option<JetDevtoolsCommandReceipt> {
    let mut receipts = JET_DEVTOOLS_COMMAND_RECEIPTS.lock().ok()?;
    let receipt = receipts.get_mut(session_id)?.pop_front();
    command_receipt_cleanup(&mut receipts, session_id);
    receipt
}

pub fn jet_devtools_command_receipt_count(session_id: &str) -> usize {
    JET_DEVTOOLS_COMMAND_RECEIPTS
        .lock()
        .ok()
        .and_then(|receipts| receipts.get(session_id).map(VecDeque::len))
        .unwrap_or(0)
}
/// Return a non-destructive snapshot of the retained command receipts for one
/// exact session.  Browser projections use this to report terminal outcomes
/// without stealing the canonical receipt stream.
pub fn jet_devtools_command_receipts(
    session_id: &str,
) -> Vec<JetDevtoolsCommandReceipt> {
    JET_DEVTOOLS_COMMAND_RECEIPTS
        .lock()
        .ok()
        .and_then(|receipts| {
            receipts
                .get(session_id)
                .map(|queue| queue.iter().cloned().collect::<Vec<_>>())
        })
        .unwrap_or_default()
}

pub fn jet_devtools_clear_command_receipts(session_id: &str) {
    if let Ok(mut receipts) = JET_DEVTOOLS_COMMAND_RECEIPTS.lock() {
        receipts.remove(session_id);
    }
}
/// One bounded command FIFO per session. Database and game controls share this
/// storage so polling one command family cannot silently consume another.
static JET_DEVTOOLS_COMMAND_QUEUES: std::sync::LazyLock<
    std::sync::Mutex<BTreeMap<String, VecDeque<JetDevtoolsCommand>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(BTreeMap::new()));

fn command_queue_cleanup(
    queues: &mut BTreeMap<String, VecDeque<JetDevtoolsCommand>>,
    session_id: &str,
) {
    if queues
        .get(session_id)
        .is_some_and(std::collections::VecDeque::is_empty)
    {
        queues.remove(session_id);
    }
}

/// Admit one checked host command into the canonical per-session FIFO.
pub fn jet_devtools_enqueue_command(command: JetDevtoolsCommand) -> Result<(), String> {
    command.validate()?;
    let session_id = command.session_id().to_string();
    let receipt = JetDevtoolsCommandReceipt::queued(&command);
    let mut receipts = JET_DEVTOOLS_COMMAND_RECEIPTS
        .lock()
        .map_err(|_| "jet.devtools.v1 command receipt queue is poisoned".to_string())?;
    if receipts
        .get(&session_id)
        .is_some_and(|queue| queue.len() >= JET_DEVTOOLS_MAX_COMMANDS)
    {
        return Err("jet.devtools.v1 command receipt queue is full".to_string());
    }
    let mut queues = JET_DEVTOOLS_COMMAND_QUEUES
        .lock()
        .map_err(|_| "jet.devtools.v1 command queue is poisoned".to_string())?;
    let queue = queues.entry(session_id.clone()).or_insert_with(VecDeque::new);
    if queue.len() >= JET_DEVTOOLS_MAX_COMMANDS {
        return Err("jet.devtools.v1 command queue is full".to_string());
    }
    queue.push_back(command);
    receipts
        .entry(session_id)
        .or_insert_with(VecDeque::new)
        .push_back(receipt);
    Ok(())
}

/// Requeue one command at the head after a runtime destination is unavailable.
pub fn jet_devtools_requeue_command_front(command: JetDevtoolsCommand) -> Result<(), String> {
    command.validate()?;
    let session_id = command.session_id().to_string();
    let mut queues = JET_DEVTOOLS_COMMAND_QUEUES
        .lock()
        .map_err(|_| "jet.devtools.v1 command queue is poisoned".to_string())?;
    let queue = queues.entry(session_id).or_insert_with(VecDeque::new);
    if queue.len() >= JET_DEVTOOLS_MAX_COMMANDS {
        return Err("jet.devtools.v1 command queue is full".to_string());
    }
    queue.push_front(command);
    Ok(())
}

/// Remove the next command without changing its family or payload.
pub fn jet_devtools_poll_command(session_id: &str) -> Option<JetDevtoolsCommand> {
    let mut queues = JET_DEVTOOLS_COMMAND_QUEUES.lock().ok()?;
    let command = queues.get_mut(session_id)?.pop_front();
    command_queue_cleanup(&mut queues, session_id);
    command
}

/// Return the number of commands waiting for one exact session.
pub fn jet_devtools_command_count(session_id: &str) -> usize {
    JET_DEVTOOLS_COMMAND_QUEUES
        .lock()
        .ok()
        .and_then(|queues| queues.get(session_id).map(VecDeque::len))
        .unwrap_or(0)
}

/// Poll only a database command while preserving every intervening game
/// command in the same FIFO.
pub fn jet_devtools_poll_database_explain(
    session_id: &str,
) -> Option<JetDevtoolsDatabaseExplainRequest> {
    let mut queues = JET_DEVTOOLS_COMMAND_QUEUES.lock().ok()?;
    let queue = queues.get_mut(session_id)?;
    let index = queue
        .iter()
        .position(|command| matches!(command, JetDevtoolsCommand::DatabaseExplain(_)))?;
    let command = queue.remove(index)?;
    command_queue_cleanup(&mut queues, session_id);
    match command {
        JetDevtoolsCommand::DatabaseExplain(request) => Some(request),
        JetDevtoolsCommand::GameControl(_) | JetDevtoolsCommand::ProjectRebuild(_) => None,
    }
}

/// Poll only a game command while preserving database commands in the same
/// FIFO. The general dispatcher should normally use `poll_command`.
pub fn jet_devtools_poll_game_control(
    session_id: &str,
) -> Option<JetDevtoolsGameControlRequest> {
    let mut queues = JET_DEVTOOLS_COMMAND_QUEUES.lock().ok()?;
    let queue = queues.get_mut(session_id)?;
    let index = queue
        .iter()
        .position(|command| matches!(command, JetDevtoolsCommand::GameControl(_)))?;
    let command = queue.remove(index)?;
    command_queue_cleanup(&mut queues, session_id);
    match command {
        JetDevtoolsCommand::GameControl(request) => Some(request),
        JetDevtoolsCommand::DatabaseExplain(_) | JetDevtoolsCommand::ProjectRebuild(_) => None,
    }
}

/// Drop all queued commands for one session at teardown.
pub fn jet_devtools_clear_commands(session_id: &str) {
    if let Ok(mut queues) = JET_DEVTOOLS_COMMAND_QUEUES.lock() {
        queues.remove(session_id);
    }
    jet_devtools_clear_command_receipts(session_id);
}

/// Compatibility name retained for generated game adapters. It now feeds the
/// shared command FIFO instead of a second game-only queue.
pub fn jet_devtools_enqueue_game_control(
    request: JetDevtoolsGameControlRequest,
) -> Result<(), String> {
    jet_devtools_enqueue_command(JetDevtoolsCommand::GameControl(request))
}

/// Direction is part of the canonical command envelope. Runtime-to-host
/// traffic is event-only and therefore cannot be placed in this envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetDevtoolsCommandDirection {
    HostToRuntime,
    RuntimeToHost,
}

impl JetDevtoolsCommandDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HostToRuntime => "host_to_runtime",
            Self::RuntimeToHost => "runtime_to_host",
        }
    }
}

/// A bounded host-to-runtime command envelope. It is deliberately separate
/// from the event-only `JetDevtoolsEnvelope` mirrored into generated Prelude.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsCommandEnvelope {
    pub protocol: String,
    pub session_id: String,
    pub started_at_ms: u64,
    pub direction: JetDevtoolsCommandDirection,
    commands: VecDeque<JetDevtoolsCommand>,
}

impl JetDevtoolsCommandEnvelope {
    pub fn new(session_id: impl Into<String>, started_at_ms: u64) -> Self {
        Self {
            protocol: crate::Devtools::JET_DEVTOOLS_PROTOCOL.to_string(),
            session_id: session_id.into(),
            started_at_ms,
            direction: JetDevtoolsCommandDirection::HostToRuntime,
            commands: VecDeque::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != crate::Devtools::JET_DEVTOOLS_PROTOCOL {
            return Err("jet.devtools.v1 command envelope protocol mismatch".to_string());
        }
        validate_text(&self.session_id, "session_id")?;
        if self.direction != JetDevtoolsCommandDirection::HostToRuntime {
            return Err("jet.devtools.v1 command envelope direction must be host_to_runtime".to_string());
        }
        if self.commands.len() > JET_DEVTOOLS_MAX_COMMANDS {
            return Err("jet.devtools.v1 command queue is full".to_string());
        }
        for command in &self.commands {
            command.validate()?;
            if command.session_id() != self.session_id {
                return Err("jet.devtools.v1 command session identity mismatch".to_string());
            }
        }
        Ok(())
    }

    pub fn enqueue_command(&mut self, command: JetDevtoolsCommand) -> Result<(), String> {
        command.validate()?;
        if command.session_id() != self.session_id {
            return Err("jet.devtools.v1 command session identity mismatch".to_string());
        }
        if self.commands.len() >= JET_DEVTOOLS_MAX_COMMANDS {
            return Err("jet.devtools.v1 command queue is full".to_string());
        }
        self.direction = JetDevtoolsCommandDirection::HostToRuntime;
        self.commands.push_back(command);
        Ok(())
    }

    /// Put a temporarily unavailable command back at the head of the queue.
    /// Dispatchers use this to preserve FIFO order when the runtime callback
    /// is not installed yet.
    pub fn requeue_command_front(&mut self, command: JetDevtoolsCommand) -> Result<(), String> {
        command.validate()?;
        if command.session_id() != self.session_id {
            return Err("jet.devtools.v1 command session identity mismatch".to_string());
        }
        if self.commands.len() >= JET_DEVTOOLS_MAX_COMMANDS {
            return Err("jet.devtools.v1 command queue is full".to_string());
        }
        self.direction = JetDevtoolsCommandDirection::HostToRuntime;
        self.commands.push_front(command);
        Ok(())
    }


    pub fn poll_command(&mut self) -> Option<JetDevtoolsCommand> {
        self.commands.pop_front()
    }

    pub fn commands(&self) -> impl Iterator<Item = &JetDevtoolsCommand> {
        self.commands.iter()
    }

    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    pub fn serialize(&self) -> Result<String, String> {
        self.validate()?;
        let commands = self
            .commands
            .iter()
            .map(|command| {
                format!(
                    "{{\"kind\":{},\"payload\":{}}}",
                    render_text(command.kind()),
                    command.render_json()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let out = format!(
            "{{\"protocol\":{},\"session_id\":{},\"started_at_ms\":{},\"direction\":{},\"commands\":[{}]}}",
            render_text(&self.protocol),
            render_text(&self.session_id),
            self.started_at_ms,
            render_text(self.direction.as_str()),
            commands,
        );
        if out.len() > crate::Devtools::JET_DEVTOOLS_MAX_ENVELOPE_BYTES {
            return Err("jet.devtools.v1 command envelope exceeds the Prelude byte limit".to_string());
        }
        Ok(out)
    }
}
/// A decoded transport has one direction and one payload kind. Mixed event
/// and command envelopes are rejected by the host decoder before Session sees
/// them.
#[derive(Clone, Debug)]
pub enum JetDevtoolsDecodedEnvelope {
    Events(crate::Devtools::JetDevtoolsEnvelope),
    Commands(JetDevtoolsCommandEnvelope),
}

impl JetDevtoolsDecodedEnvelope {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Events(envelope) => envelope.serialize().map(|_| ()),
            Self::Commands(envelope) => envelope.validate(),
        }
    }

    pub fn session_id(&self) -> &str {
        match self {
            Self::Events(envelope) => &envelope.session_id,
            Self::Commands(envelope) => &envelope.session_id,
        }
    }
}
