// D-DX-DBPANEL1 / card #2460: bounded database observations share the
// `jet.devtools.v1` event boundary.  This module owns only typed facts and
// projections.  It never opens a connection, stores a URL or credential, or
// carries SQL text or binding values.
//
// A host supplies query and pool facts after its driver has done the work.  A
// host must classify a statement before asking for EXPLAIN; this module then
// requires the read-only class and an exact, explicit `DB.Explain` grant for
// the query identity.  The grant is a fact, not ambient authority.

use std::collections::VecDeque as JetDevtoolsDatabaseVecDeque;
use std::fmt as JetDevtoolsDatabaseFmt;

pub const JET_DEVTOOLS_DATABASE_PANEL_ID: &str = "database";
pub const JET_DEVTOOLS_DATABASE_EXPLAIN_CAPABILITY: &str = "DB.Explain";
pub const JET_DEVTOOLS_DATABASE_CREDENTIAL_CAPABILITY: &str = "DB.Credentials";
pub const JET_DEVTOOLS_DATABASE_MAX_HISTORY: usize = 256;
pub const JET_DEVTOOLS_DATABASE_MAX_BINDINGS: usize = 256;
pub const JET_DEVTOOLS_DATABASE_MAX_PLAN_ROWS: usize = 256;
pub const JET_DEVTOOLS_DATABASE_MAX_EXPLAIN_TIMEOUT_MS: u64 = 30_000;

/// Every failure in this module is structural.  Error variants never include
/// statement text, binding values, credentials, or connection details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetDevtoolsDatabaseFactError {
    EmptyField(&'static str),
    OversizedField(&'static str),
    InvalidIdentity(&'static str),
    InvalidLimit(&'static str),
    InvalidPoolCounts,
    DuplicateIdentity(&'static str),
    UnsupportedStatement,
    MissingExplainAuthority,
    MismatchedExplainAuthority,
    InvalidTimestamp,
    InvalidPlanRow,
    Protocol(String),
}

impl JetDevtoolsDatabaseFmt::Display for JetDevtoolsDatabaseFactError {
    fn fmt(&self, formatter: &mut JetDevtoolsDatabaseFmt::Formatter<'_>) -> JetDevtoolsDatabaseFmt::Result {
        match self {
            Self::EmptyField(field) => write!(formatter, "database {field} must not be empty"),
            Self::OversizedField(field) => {
                write!(formatter, "database {field} exceeds the Prelude text limit")
            }
            Self::InvalidIdentity(field) => {
                write!(formatter, "database {field} must be a stable identity")
            }
            Self::InvalidLimit(field) => write!(formatter, "database {field} is outside its limit"),
            Self::InvalidPoolCounts => {
                formatter.write_str("database pool counts exceed the configured maximum")
            }
            Self::DuplicateIdentity(field) => {
                write!(formatter, "database {field} identity is duplicated")
            }
            Self::UnsupportedStatement => {
                formatter.write_str("database EXPLAIN requires a read-only statement")
            }
            Self::MissingExplainAuthority => {
                formatter.write_str("database EXPLAIN requires an explicit DB.Explain grant")
            }
            Self::MismatchedExplainAuthority => formatter.write_str(
                "database EXPLAIN grant does not match the requested capability and query identity",
            ),
            Self::InvalidTimestamp => {
                formatter.write_str("database EXPLAIN timestamps must be monotonic")
            }
            Self::InvalidPlanRow => formatter.write_str("database EXPLAIN plan row is invalid"),
            Self::Protocol(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for JetDevtoolsDatabaseFactError {}

fn jet_devtools_database_validate_text(
    value: &str,
    field: &'static str,
) -> Result<(), JetDevtoolsDatabaseFactError> {
    if value.is_empty() {
        return Err(JetDevtoolsDatabaseFactError::EmptyField(field));
    }
    if value.len() > JET_DEVTOOLS_MAX_TEXT_BYTES {
        return Err(JetDevtoolsDatabaseFactError::OversizedField(field));
    }
    if value.chars().any(char::is_control) {
        return Err(JetDevtoolsDatabaseFactError::InvalidIdentity(field));
    }
    Ok(())
}

fn jet_devtools_database_validate_identity(
    value: &str,
    field: &'static str,
) -> Result<(), JetDevtoolsDatabaseFactError> {
    jet_devtools_database_validate_text(value, field)?;
    if value.chars().any(char::is_whitespace) {
        return Err(JetDevtoolsDatabaseFactError::InvalidIdentity(field));
    }
    Ok(())
}

fn jet_devtools_database_validate_optional_text(
    value: Option<&str>,
    field: &'static str,
) -> Result<(), JetDevtoolsDatabaseFactError> {
    if let Some(value) = value {
        jet_devtools_database_validate_text(value, field)?;
    }
    Ok(())
}

fn jet_devtools_database_json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len().saturating_add(2));
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                let code = character as u32;
                escaped.push_str(&format!("\\u{code:04x}"));
            }
            character => escaped.push(character),
        }
    }
    escaped.push('"');
    escaped
}

fn jet_devtools_database_json_optional(value: Option<&str>) -> String {
    value
        .map(jet_devtools_database_json_string)
        .unwrap_or_else(|| "null".to_string())
}

/// The lifecycle fact for one observed pool.  It is independent of any
/// driver's concrete connection type.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsDatabasePoolLifecycle {
    Open,
    Draining,
    Drained,
}

impl JetDevtoolsDatabasePoolLifecycle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Draining => "draining",
            Self::Drained => "drained",
        }
    }
}

/// Pressure is derived from the pool counters, never supplied by a renderer.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsDatabasePoolPressure {
    Idle,
    Normal,
    Elevated,
    Saturated,
    Exhausted,
    Draining,
    Drained,
}

impl JetDevtoolsDatabasePoolPressure {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Normal => "normal",
            Self::Elevated => "elevated",
            Self::Saturated => "saturated",
            Self::Exhausted => "exhausted",
            Self::Draining => "draining",
            Self::Drained => "drained",
        }
    }
}

/// Bounded pool counters suitable for a panel row.  `pool_id` is an opaque
/// stable identity; no endpoint, URL, username, password, or credential
/// material is part of this fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabasePoolStats {
    pub pool_id: String,
    pub lifecycle: JetDevtoolsDatabasePoolLifecycle,
    pub max_connections: u64,
    pub available_connections: u64,
    pub leased_connections: u64,
    pub opening_connections: u64,
    pub waiting_requests: u64,
    pub timed_out_requests: u64,
    pub acquire_count: u64,
    pub release_count: u64,
    pub open_failures: u64,
    pub unhealthy_connections: u64,
    pub replacement_count: u64,
    pub replacement_failures: u64,
    pub observed_at_ms: u64,
}

impl JetDevtoolsDatabasePoolStats {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool_id: impl Into<String>,
        lifecycle: JetDevtoolsDatabasePoolLifecycle,
        max_connections: u64,
        available_connections: u64,
        leased_connections: u64,
        opening_connections: u64,
        waiting_requests: u64,
        timed_out_requests: u64,
        acquire_count: u64,
        release_count: u64,
        open_failures: u64,
        unhealthy_connections: u64,
        replacement_count: u64,
        replacement_failures: u64,
        observed_at_ms: u64,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        let pool_id = pool_id.into();
        jet_devtools_database_validate_identity(&pool_id, "pool_id")?;
        if max_connections == 0 {
            return Err(JetDevtoolsDatabaseFactError::InvalidLimit("max_connections"));
        }
        if available_connections
            .saturating_add(leased_connections)
            .saturating_add(opening_connections)
            > max_connections
        {
            return Err(JetDevtoolsDatabaseFactError::InvalidPoolCounts);
        }
        Ok(Self {
            pool_id,
            lifecycle,
            max_connections,
            available_connections,
            leased_connections,
            opening_connections,
            waiting_requests,
            timed_out_requests,
            acquire_count,
            release_count,
            open_failures,
            unhealthy_connections,
            replacement_count,
            replacement_failures,
            observed_at_ms,
        })
    }

    pub fn reserved_connections(&self) -> u64 {
        self.available_connections
            .saturating_add(self.leased_connections)
            .saturating_add(self.opening_connections)
    }

    pub fn available_capacity(&self) -> u64 {
        self.max_connections
            .saturating_sub(self.reserved_connections())
    }

    /// Utilization in thousandths avoids floating-point drift between tiers.
    pub fn utilization_milli(&self) -> u64 {
        self.reserved_connections()
            .saturating_mul(1_000)
            .checked_div(self.max_connections)
            .unwrap_or(1_000)
            .min(1_000)
    }

    pub fn pressure(&self) -> JetDevtoolsDatabasePoolPressure {
        match self.lifecycle {
            JetDevtoolsDatabasePoolLifecycle::Draining => {
                JetDevtoolsDatabasePoolPressure::Draining
            }
            JetDevtoolsDatabasePoolLifecycle::Drained => JetDevtoolsDatabasePoolPressure::Drained,
            JetDevtoolsDatabasePoolLifecycle::Open => {
                if self.available_capacity() == 0 {
                    if self.waiting_requests > 0 {
                        JetDevtoolsDatabasePoolPressure::Exhausted
                    } else {
                        JetDevtoolsDatabasePoolPressure::Saturated
                    }
                } else if self.waiting_requests > 0 || self.utilization_milli() >= 750 {
                    JetDevtoolsDatabasePoolPressure::Elevated
                } else if self.reserved_connections() == 0 {
                    JetDevtoolsDatabasePoolPressure::Idle
                } else {
                    JetDevtoolsDatabasePoolPressure::Normal
                }
            }
        }
    }

    pub fn freshness_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.observed_at_ms)
    }

    pub fn render_json(&self) -> String {
        self.render_json_at(self.observed_at_ms)
    }

    pub fn render_json_at(&self, now_ms: u64) -> String {
        format!(
            "{{\"pool_id\":{},\"lifecycle\":{},\"max_connections\":{},\"available_connections\":{},\"leased_connections\":{},\"opening_connections\":{},\"waiting_requests\":{},\"timed_out_requests\":{},\"acquire_count\":{},\"release_count\":{},\"open_failures\":{},\"unhealthy_connections\":{},\"replacement_count\":{},\"replacement_failures\":{},\"reserved_connections\":{},\"available_capacity\":{},\"utilization_milli\":{},\"pressure\":{},\"observed_at_ms\":{},\"freshness_ms\":{}}}",
            jet_devtools_database_json_string(&self.pool_id),
            jet_devtools_database_json_string(self.lifecycle.as_str()),
            self.max_connections,
            self.available_connections,
            self.leased_connections,
            self.opening_connections,
            self.waiting_requests,
            self.timed_out_requests,
            self.acquire_count,
            self.release_count,
            self.open_failures,
            self.unhealthy_connections,
            self.replacement_count,
            self.replacement_failures,
            self.reserved_connections(),
            self.available_capacity(),
            self.utilization_milli(),
            jet_devtools_database_json_string(self.pressure().as_str()),
            self.observed_at_ms,
            self.freshness_ms(now_ms),
        )
    }

    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, JetDevtoolsDatabaseFactError> {
        JetDevtoolsEvent::from_parts(
            self.observed_at_ms,
            source,
            "DatabasePool",
            self.pool_id.clone(),
            self.render_json(),
        )
        .map_err(JetDevtoolsDatabaseFactError::Protocol)
    }
}

/// Typed table access facts.  Names identify schema objects; values are only
/// aggregate counters and never row contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseTableUsage {
    pub table_id: String,
    pub rows_read: u64,
    pub rows_written: u64,
}

impl JetDevtoolsDatabaseTableUsage {
    pub fn new(
        table_id: impl Into<String>,
        rows_read: u64,
        rows_written: u64,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        let table_id = table_id.into();
        jet_devtools_database_validate_identity(&table_id, "table_id")?;
        Ok(Self {
            table_id,
            rows_read,
            rows_written,
        })
    }
}

/// Typed index usage facts.  The index name and counters are safe metadata;
/// indexed values and rows are never retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseIndexUsage {
    pub index_id: String,
    pub used: bool,
    pub rows_examined: u64,
}

impl JetDevtoolsDatabaseIndexUsage {
    pub fn new(
        index_id: impl Into<String>,
        used: bool,
        rows_examined: u64,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        let index_id = index_id.into();
        jet_devtools_database_validate_identity(&index_id, "index_id")?;
        Ok(Self {
            index_id,
            used,
            rows_examined,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsDatabaseQueryStatus {
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

impl JetDevtoolsDatabaseQueryStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }
}

/// One payload-free query observation.  `statement_identity` is an opaque
/// stable token, not SQL text.  Bindings are intentionally absent from this
/// fact; EXPLAIN has a separate redacted binding type below.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseQueryStats {
    pub query_id: String,
    pub statement_identity: String,
    pub status: JetDevtoolsDatabaseQueryStatus,
    pub duration_ms: u64,
    pub source: String,
    pub source_span: Option<(u64, u64)>,
    pub request_id: Option<String>,
    pub rows_returned: Option<u64>,
    pub rows_affected: Option<u64>,
    pub table_usage: Vec<JetDevtoolsDatabaseTableUsage>,
    pub index_usage: Vec<JetDevtoolsDatabaseIndexUsage>,
    pub slow_threshold_ms: u64,
    pub observed_at_ms: u64,

}

impl JetDevtoolsDatabaseQueryStats {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        query_id: impl Into<String>,
        statement_identity: impl Into<String>,
        status: JetDevtoolsDatabaseQueryStatus,
        duration_ms: u64,
        source: impl Into<String>,
        request_id: Option<String>,
        mut table_usage: Vec<JetDevtoolsDatabaseTableUsage>,
        mut index_usage: Vec<JetDevtoolsDatabaseIndexUsage>,
        slow_threshold_ms: u64,
        observed_at_ms: u64,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        let query_id = query_id.into();
        let statement_identity = statement_identity.into();
        let source = source.into();
        jet_devtools_database_validate_identity(&query_id, "query_id")?;
        jet_devtools_database_validate_identity(&statement_identity, "statement_identity")?;
        jet_devtools_database_validate_text(&source, "source")?;
        jet_devtools_database_validate_optional_text(request_id.as_deref(), "request_id")?;
        if request_id.as_deref().is_some_and(|value| value.is_empty()) {
            return Err(JetDevtoolsDatabaseFactError::EmptyField("request_id"));
        }
        if slow_threshold_ms == 0 {
            return Err(JetDevtoolsDatabaseFactError::InvalidLimit("slow_threshold_ms"));
        }
        if table_usage.len() > JET_DEVTOOLS_DATABASE_MAX_HISTORY {
            return Err(JetDevtoolsDatabaseFactError::InvalidLimit("table_usage"));
        }
        if index_usage.len() > JET_DEVTOOLS_DATABASE_MAX_HISTORY {
            return Err(JetDevtoolsDatabaseFactError::InvalidLimit("index_usage"));
        }
        table_usage.sort_by(|left, right| left.table_id.cmp(&right.table_id));
        index_usage.sort_by(|left, right| left.index_id.cmp(&right.index_id));
        if table_usage
            .windows(2)
            .any(|rows| rows[0].table_id == rows[1].table_id)
        {
            return Err(JetDevtoolsDatabaseFactError::DuplicateIdentity("table_id"));
        }
        if index_usage
            .windows(2)
            .any(|rows| rows[0].index_id == rows[1].index_id)
        {
            return Err(JetDevtoolsDatabaseFactError::DuplicateIdentity("index_id"));
        }
        Ok(Self {
            query_id,
            statement_identity,
            status,
            duration_ms,
            source,
            source_span: None,
            request_id,
            rows_returned: None,
            rows_affected: None,
            table_usage,
            index_usage,
            slow_threshold_ms,
            observed_at_ms,
        })
    }

    pub fn with_runtime_facts(
        mut self,
        source_span: Option<(u64, u64)>,
        rows_returned: Option<u64>,
        rows_affected: Option<u64>,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        if source_span.is_some_and(|(start, end)| end < start) {
            return Err(JetDevtoolsDatabaseFactError::InvalidTimestamp);
        }
        self.source_span = source_span;
        self.rows_returned = rows_returned;
        self.rows_affected = rows_affected;
        Ok(self)
    }

    pub fn is_slow(&self) -> bool {
        self.duration_ms >= self.slow_threshold_ms
    }

    pub fn freshness_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.observed_at_ms)
    }

    pub fn request_link(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub fn render_json(&self) -> String {
        self.render_json_at(self.observed_at_ms)
    }

    pub fn render_json_at(&self, now_ms: u64) -> String {
        let tables = self
            .table_usage
            .iter()
            .map(|usage| {
                format!(
                    "{{\"table_id\":{},\"rows_read\":{},\"rows_written\":{}}}",
                    jet_devtools_database_json_string(&usage.table_id),
                    usage.rows_read,
                    usage.rows_written,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let indexes = self
            .index_usage
            .iter()
            .map(|usage| {
                format!(
                    "{{\"index_id\":{},\"used\":{},\"rows_examined\":{}}}",
                    jet_devtools_database_json_string(&usage.index_id),
                    usage.used,
                    usage.rows_examined,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let source_span = self
            .source_span
            .map(|(start, end)| format!("{{\"start\":{start},\"end\":{end}}}"))
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"query_id\":{},\"statement_identity\":{},\"status\":{},\"duration_ms\":{},\"source\":{},\"source_span\":{},\"request_id\":{},\"rows_returned\":{},\"rows_affected\":{},\"table_usage\":[{}],\"index_usage\":[{}],\"slow_threshold_ms\":{},\"slow\":{},\"observed_at_ms\":{},\"freshness_ms\":{}}}",
            jet_devtools_database_json_string(&self.query_id),
            jet_devtools_database_json_string(&self.statement_identity),
            jet_devtools_database_json_string(self.status.as_str()),
            self.duration_ms,
            jet_devtools_database_json_string(&self.source),
            source_span,
            jet_devtools_database_json_optional(self.request_id.as_deref()),
            self.rows_returned
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            self.rows_affected
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            tables,
            indexes,
            self.slow_threshold_ms,
            self.is_slow(),
            self.observed_at_ms,
            self.freshness_ms(now_ms),
        )
    }

    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, JetDevtoolsDatabaseFactError> {
        JetDevtoolsEvent::from_parts(
            self.observed_at_ms,
            source,
            "DatabaseQuery",
            self.query_id.clone(),
            self.render_json(),
        )
        .map_err(JetDevtoolsDatabaseFactError::Protocol)
    }
}

/// A bounded query history.  Entries are retained in insertion order for the
/// FIFO window, while `snapshot` provides a deterministic timestamp/id order
/// for a renderer or protocol projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseQueryHistory {
    capacity: usize,
    entries: JetDevtoolsDatabaseVecDeque<JetDevtoolsDatabaseQueryStats>,
}

impl JetDevtoolsDatabaseQueryHistory {
    pub fn new(capacity: u64) -> Self {
        let capacity = usize::try_from(capacity)
            .unwrap_or(JET_DEVTOOLS_DATABASE_MAX_HISTORY)
            .clamp(1, JET_DEVTOOLS_DATABASE_MAX_HISTORY);
        Self {
            capacity,
            entries: JetDevtoolsDatabaseVecDeque::with_capacity(capacity),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push(
        &mut self,
        entry: JetDevtoolsDatabaseQueryStats,
    ) -> Result<(), JetDevtoolsDatabaseFactError> {
        if self.entries.iter().any(|known| known.query_id == entry.query_id) {
            return Err(JetDevtoolsDatabaseFactError::DuplicateIdentity("query_id"));
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
        Ok(())
    }

    pub fn latest(&self) -> Option<&JetDevtoolsDatabaseQueryStats> {
        self.entries.back()
    }

    pub fn entries(&self) -> impl Iterator<Item = &JetDevtoolsDatabaseQueryStats> {
        self.entries.iter()
    }

    pub fn snapshot(&self) -> Vec<JetDevtoolsDatabaseQueryStats> {
        let mut snapshot = self.entries.iter().cloned().collect::<Vec<_>>();
        snapshot.sort_by(|left, right| {
            left.observed_at_ms
                .cmp(&right.observed_at_ms)
                .then_with(|| left.query_id.cmp(&right.query_id))
        });
        snapshot
    }
}

/// The statement class is supplied by the checked database adapter.  Only the
/// read-only class may cross the EXPLAIN request constructor.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsDatabaseStatementClass {
    ReadOnly,
    Mutation,
    Unknown,
}

impl JetDevtoolsDatabaseStatementClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Mutation => "mutation",
            Self::Unknown => "unknown",
        }
    }

    pub const fn is_read_only(self) -> bool {
        matches!(self, Self::ReadOnly)
    }
}

/// Binding metadata contains a type and ordinal only.  There is deliberately
/// no value field, and the type itself is rendered with `redacted: true`.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsDatabaseBindingKind {
    Null,
    Boolean,
    Integer,
    Float,
    Text,
    Bytes,
    Other,
}

impl JetDevtoolsDatabaseBindingKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Text => "text",
            Self::Bytes => "bytes",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JetDevtoolsDatabaseRedactedBinding {
    pub ordinal: u64,
    pub kind: JetDevtoolsDatabaseBindingKind,
}

impl JetDevtoolsDatabaseRedactedBinding {
    pub const fn new(ordinal: u64, kind: JetDevtoolsDatabaseBindingKind) -> Self {
        Self { ordinal, kind }
    }

    pub const fn is_redacted(self) -> bool {
        true
    }
}

/// A safe EXPLAIN request fact.  It has no SQL string and no binding payload;
/// `statement_identity` is restricted to an opaque identity token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseExplainFactRequest {
    pub query_id: String,
    pub statement_identity: String,
    pub statement_class: JetDevtoolsDatabaseStatementClass,
    pub source: String,
    pub request_id: Option<String>,
    pub bindings: Vec<JetDevtoolsDatabaseRedactedBinding>,
    pub timeout_ms: u64,
    pub max_rows: u64,
    required_capability: JetDevtoolsDatabaseCapabilityFact,
}

impl JetDevtoolsDatabaseExplainFactRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        query_id: impl Into<String>,
        statement_identity: impl Into<String>,
        statement_class: JetDevtoolsDatabaseStatementClass,
        source: impl Into<String>,
        request_id: Option<String>,
        mut bindings: Vec<JetDevtoolsDatabaseRedactedBinding>,
        timeout_ms: u64,
        max_rows: u64,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        if !statement_class.is_read_only() {
            return Err(JetDevtoolsDatabaseFactError::UnsupportedStatement);
        }
        let query_id = query_id.into();
        let statement_identity = statement_identity.into();
        let source = source.into();
        jet_devtools_database_validate_identity(&query_id, "query_id")?;
        jet_devtools_database_validate_identity(&statement_identity, "statement_identity")?;
        jet_devtools_database_validate_text(&source, "source")?;
        jet_devtools_database_validate_optional_text(request_id.as_deref(), "request_id")?;
        if request_id.as_deref().is_some_and(|value| value.is_empty()) {
            return Err(JetDevtoolsDatabaseFactError::EmptyField("request_id"));
        }
        if timeout_ms == 0 || timeout_ms > JET_DEVTOOLS_DATABASE_MAX_EXPLAIN_TIMEOUT_MS {
            return Err(JetDevtoolsDatabaseFactError::InvalidLimit("timeout_ms"));
        }
        if max_rows == 0
            || max_rows
                > u64::try_from(JET_DEVTOOLS_DATABASE_MAX_PLAN_ROWS).unwrap_or(u64::MAX)
        {
            return Err(JetDevtoolsDatabaseFactError::InvalidLimit("max_rows"));
        }
        if bindings.len() > JET_DEVTOOLS_DATABASE_MAX_BINDINGS {
            return Err(JetDevtoolsDatabaseFactError::InvalidLimit("bindings"));
        }
        bindings.sort_by_key(|binding| binding.ordinal);
        if bindings.windows(2).any(|rows| rows[0].ordinal == rows[1].ordinal) {
            return Err(JetDevtoolsDatabaseFactError::DuplicateIdentity("binding ordinal"));
        }
        let required_capability = JetDevtoolsDatabaseCapabilityFact::for_explain(&query_id, false)?;
        Ok(Self {
            query_id,
            statement_identity,
            statement_class,
            source,
            request_id,
            bindings,
            timeout_ms,
            max_rows,
            required_capability,
        })
    }

    pub fn required_capability(&self) -> &JetDevtoolsDatabaseCapabilityFact {
        &self.required_capability
    }

    pub fn binding_count(&self) -> usize {
        self.bindings.len()
    }

    pub fn render_json(&self, started_at_ms: u64) -> String {
        let bindings = self
            .bindings
            .iter()
            .map(|binding| {
                format!(
                    "{{\"ordinal\":{},\"kind\":{},\"redacted\":true}}",
                    binding.ordinal,
                    jet_devtools_database_json_string(binding.kind.as_str()),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"query_id\":{},\"statement_identity\":{},\"statement_class\":{},\"source\":{},\"request_id\":{},\"binding_count\":{},\"bindings\":[{}],\"timeout_ms\":{},\"max_rows\":{},\"capability\":{},\"started_at_ms\":{}}}",
            jet_devtools_database_json_string(&self.query_id),
            jet_devtools_database_json_string(&self.statement_identity),
            jet_devtools_database_json_string(self.statement_class.as_str()),
            jet_devtools_database_json_string(&self.source),
            jet_devtools_database_json_optional(self.request_id.as_deref()),
            self.bindings.len(),
            bindings,
            self.timeout_ms,
            self.max_rows,
            jet_devtools_database_json_string(JET_DEVTOOLS_DATABASE_EXPLAIN_CAPABILITY),
            started_at_ms,
        )
    }

    pub fn authorize(
        &self,
        grant: &JetDevtoolsDatabaseCapabilityFact,
        started_at_ms: u64,
    ) -> Result<JetDevtoolsDatabaseAuthorizedExplainRequest, JetDevtoolsDatabaseFactError> {
        if !grant.granted {
            return Err(JetDevtoolsDatabaseFactError::MissingExplainAuthority);
        }
        if !grant.matches_explain(&self.query_id) {
            return Err(JetDevtoolsDatabaseFactError::MismatchedExplainAuthority);
        }
        Ok(JetDevtoolsDatabaseAuthorizedExplainRequest {
            request: self.clone(),
            grant: grant.clone(),
            started_at_ms,
        })
    }
}

/// One explicit authority fact.  The resource must be the exact query
/// identity; a broad database grant cannot silently authorize EXPLAIN.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseCapabilityFact {
    pub capability: String,
    pub resource: String,
    pub granted: bool,
}

impl JetDevtoolsDatabaseCapabilityFact {
    pub fn new(
        capability: impl Into<String>,
        resource: impl Into<String>,
        granted: bool,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        let capability = capability.into();
        let resource = resource.into();
        jet_devtools_database_validate_identity(&capability, "capability")?;
        jet_devtools_database_validate_identity(&resource, "resource")?;
        Ok(Self {
            capability,
            resource,
            granted,
        })
    }

    pub fn for_explain(
        resource: impl Into<String>,
        granted: bool,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        Self::new(
            JET_DEVTOOLS_DATABASE_EXPLAIN_CAPABILITY,
            resource,
            granted,
        )
    }
    /// Construct the separate credential grant used only for an explicitly
    /// opted-in connection-facts request. The grant carries no credential
    /// material; it is only an exact resource authority fact.
    pub fn for_credentials(
        resource: impl Into<String>,
        granted: bool,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        Self::new(
            JET_DEVTOOLS_DATABASE_CREDENTIAL_CAPABILITY,
            resource,
            granted,
        )
    }

    pub fn matches_credentials(&self, resource: &str) -> bool {
        self.granted
            && self.capability == JET_DEVTOOLS_DATABASE_CREDENTIAL_CAPABILITY
            && self.resource == resource
    }

    pub fn matches_explain(&self, query_id: &str) -> bool {
        self.granted
            && self.capability == JET_DEVTOOLS_DATABASE_EXPLAIN_CAPABILITY
            && self.resource == query_id
    }
}

impl JetDevtoolsDatabaseExplainFactRequest {
    pub fn to_protocol_event(
        &self,
        started_at_ms: u64,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, JetDevtoolsDatabaseFactError> {
        JetDevtoolsEvent::from_parts(
            started_at_ms,
            source,
            "DatabaseExplainRequest",
            self.query_id.clone(),
            self.render_json(started_at_ms),
        )
        .map_err(JetDevtoolsDatabaseFactError::Protocol)
    }
}

/// An EXPLAIN request after exact authority matching.  The grant remains a
/// value on this object so a result can be traced to the authorization fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseAuthorizedExplainRequest {
    pub request: JetDevtoolsDatabaseExplainFactRequest,
    pub grant: JetDevtoolsDatabaseCapabilityFact,
    pub started_at_ms: u64,
}

impl JetDevtoolsDatabaseAuthorizedExplainRequest {
    pub fn deadline_ms(&self) -> u64 {
        self.started_at_ms.saturating_add(self.request.timeout_ms)
    }

    pub fn finish(
        self,
        finished_at_ms: u64,
        plan_rows: Vec<JetDevtoolsDatabasePlanRow>,
    ) -> Result<JetDevtoolsDatabaseExplainFactResult, JetDevtoolsDatabaseFactError> {
        self.finish_with_status(finished_at_ms, plan_rows, false, false)
    }

    pub fn finish_with_status(
        self,
        finished_at_ms: u64,
        mut plan_rows: Vec<JetDevtoolsDatabasePlanRow>,
        timed_out: bool,
        driver_truncated: bool,
    ) -> Result<JetDevtoolsDatabaseExplainFactResult, JetDevtoolsDatabaseFactError> {
        if finished_at_ms < self.started_at_ms {
            return Err(JetDevtoolsDatabaseFactError::InvalidTimestamp);
        }
        if plan_rows.len() > JET_DEVTOOLS_DATABASE_MAX_PLAN_ROWS {
            return Err(JetDevtoolsDatabaseFactError::InvalidLimit("plan_rows"));
        }
        plan_rows.sort_by_key(|row| row.ordinal);
        if plan_rows.windows(2).any(|rows| rows[0].ordinal == rows[1].ordinal) {
            return Err(JetDevtoolsDatabaseFactError::DuplicateIdentity("plan ordinal"));
        }
        let max_rows = usize::try_from(self.request.max_rows).unwrap_or(usize::MAX);
        let truncated = driver_truncated || plan_rows.len() > max_rows;
        plan_rows.truncate(max_rows);
        let duration_ms = finished_at_ms.saturating_sub(self.started_at_ms);
        let timed_out = timed_out || duration_ms > self.request.timeout_ms;
        let deadline_ms = self.deadline_ms();
        Ok(JetDevtoolsDatabaseExplainFactResult {
            query_id: self.request.query_id,
            statement_identity: self.request.statement_identity,
            source: self.request.source,
            request_id: self.request.request_id,
            capability: self.grant,
            duration_ms,
            deadline_ms,
            finished_at_ms,
            timed_out,
            truncated,
            rows: plan_rows,
        })
    }
}

/// A structural EXPLAIN operation row.  It carries plan metadata only: no
/// literal predicates, parameter values, or result rows are retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabasePlanRow {
    pub ordinal: u64,
    pub node_id: String,
    pub parent_id: Option<String>,
    pub depth: u64,
    pub operation: String,
    pub relation: Option<String>,
    pub index_name: Option<String>,
    pub estimated_rows: Option<u64>,
    pub actual_rows: Option<u64>,
}

impl JetDevtoolsDatabasePlanRow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ordinal: u64,
        node_id: impl Into<String>,
        parent_id: Option<String>,
        depth: u64,
        operation: impl Into<String>,
        relation: Option<String>,
        index_name: Option<String>,
        estimated_rows: Option<u64>,
        actual_rows: Option<u64>,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        let node_id = node_id.into();
        let operation = operation.into();
        jet_devtools_database_validate_identity(&node_id, "node_id")?;
        jet_devtools_database_validate_text(&operation, "operation")?;
        jet_devtools_database_validate_optional_text(parent_id.as_deref(), "parent_id")?;
        jet_devtools_database_validate_optional_text(relation.as_deref(), "relation")?;
        jet_devtools_database_validate_optional_text(index_name.as_deref(), "index_name")?;
        if parent_id.as_deref().is_some_and(|value| value.is_empty())
            || relation.as_deref().is_some_and(|value| value.is_empty())
            || index_name.as_deref().is_some_and(|value| value.is_empty())
        {
            return Err(JetDevtoolsDatabaseFactError::InvalidPlanRow);
        }
        Ok(Self {
            ordinal,
            node_id,
            parent_id,
            depth,
            operation,
            relation,
            index_name,
            estimated_rows,
            actual_rows,
        })
    }

    fn render_json(&self) -> String {
        format!(
            "{{\"ordinal\":{},\"node_id\":{},\"parent_id\":{},\"depth\":{},\"operation\":{},\"relation\":{},\"index_name\":{},\"estimated_rows\":{},\"actual_rows\":{}}}",
            self.ordinal,
            jet_devtools_database_json_string(&self.node_id),
            jet_devtools_database_json_optional(self.parent_id.as_deref()),
            self.depth,
            jet_devtools_database_json_string(&self.operation),
            jet_devtools_database_json_optional(self.relation.as_deref()),
            jet_devtools_database_json_optional(self.index_name.as_deref()),
            self.estimated_rows
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
            self.actual_rows
                .map(|value| value.to_string())
                .unwrap_or_else(|| "null".to_string()),
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsDatabaseExplainStatus {
    Complete,
    TimedOut,
    RowLimitReached,
}

impl JetDevtoolsDatabaseExplainStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::TimedOut => "timed_out",
            Self::RowLimitReached => "row_limit_reached",
        }
    }
}

/// Safe EXPLAIN result facts.  The result cannot be built without first
/// authorizing a matching request, and its plan rows are capped by that
/// request's limit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabaseExplainFactResult {
    pub query_id: String,
    pub statement_identity: String,
    pub source: String,
    pub request_id: Option<String>,
    pub capability: JetDevtoolsDatabaseCapabilityFact,
    pub duration_ms: u64,
    pub deadline_ms: u64,
    pub finished_at_ms: u64,
    pub timed_out: bool,
    pub truncated: bool,
    pub rows: Vec<JetDevtoolsDatabasePlanRow>,
}

impl JetDevtoolsDatabaseExplainFactResult {
    pub fn status(&self) -> JetDevtoolsDatabaseExplainStatus {
        if self.timed_out {
            JetDevtoolsDatabaseExplainStatus::TimedOut
        } else if self.truncated {
            JetDevtoolsDatabaseExplainStatus::RowLimitReached
        } else {
            JetDevtoolsDatabaseExplainStatus::Complete
        }
    }

    pub fn render_json(&self) -> String {
        self.render_json_at(self.finished_at_ms)
    }

    pub fn render_json_at(&self, now_ms: u64) -> String {
        let rows = self
            .rows
            .iter()
            .map(JetDevtoolsDatabasePlanRow::render_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"query_id\":{},\"statement_identity\":{},\"source\":{},\"request_id\":{},\"capability\":{},\"authority_granted\":{},\"duration_ms\":{},\"deadline_ms\":{},\"finished_at_ms\":{},\"freshness_ms\":{},\"status\":{},\"timed_out\":{},\"truncated\":{},\"row_count\":{},\"plan_rows\":[{}]}}",
            jet_devtools_database_json_string(&self.query_id),
            jet_devtools_database_json_string(&self.statement_identity),
            jet_devtools_database_json_string(&self.source),
            jet_devtools_database_json_optional(self.request_id.as_deref()),
            jet_devtools_database_json_string(&self.capability.capability),
            self.capability.granted,
            self.duration_ms,
            self.deadline_ms,
            self.finished_at_ms,
            now_ms.saturating_sub(self.finished_at_ms),
            jet_devtools_database_json_string(self.status().as_str()),
            self.timed_out,
            self.truncated,
            self.rows.len(),
            rows,
        )
    }

    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, JetDevtoolsDatabaseFactError> {
        JetDevtoolsEvent::from_parts(
            self.finished_at_ms,
            source,
            "DatabaseExplainResult",
            self.query_id.clone(),
            self.render_json(),
        )
        .map_err(JetDevtoolsDatabaseFactError::Protocol)
    }
}

/// One panel snapshot.  The snapshot copies only the bounded query window and
/// an optional latest EXPLAIN result; it has no credential or driver handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsDatabasePanelFacts {
    pub panel_id: String,
    pub captured_at_ms: u64,
    pub pool: JetDevtoolsDatabasePoolStats,
    pub queries: Vec<JetDevtoolsDatabaseQueryStats>,
    pub latest_explain: Option<JetDevtoolsDatabaseExplainFactResult>,
}

impl JetDevtoolsDatabasePanelFacts {
    pub fn new(
        panel_id: impl Into<String>,
        captured_at_ms: u64,
        pool: JetDevtoolsDatabasePoolStats,
        history: &JetDevtoolsDatabaseQueryHistory,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        let panel_id = panel_id.into();
        jet_devtools_database_validate_identity(&panel_id, "panel_id")?;
        Ok(Self {
            panel_id,
            captured_at_ms,
            pool,
            queries: history.snapshot(),
            latest_explain: None,
        })
    }

    pub fn with_latest_explain(
        mut self,
        result: JetDevtoolsDatabaseExplainFactResult,
    ) -> Result<Self, JetDevtoolsDatabaseFactError> {
        if !self.queries.iter().any(|query| query.query_id == result.query_id) {
            return Err(JetDevtoolsDatabaseFactError::InvalidPlanRow);
        }
        self.latest_explain = Some(result);
        Ok(self)
    }

    pub fn query(&self, query_id: &str) -> Option<&JetDevtoolsDatabaseQueryStats> {
        self.queries.iter().find(|query| query.query_id == query_id)
    }

    pub fn pressure(&self) -> JetDevtoolsDatabasePoolPressure {
        self.pool.pressure()
    }

    pub fn render_json(&self) -> String {
        let queries = self
            .queries
            .iter()
            .map(|query| query.render_json_at(self.captured_at_ms))
            .collect::<Vec<_>>()
            .join(",");
        let explain = self
            .latest_explain
            .as_ref()
            .map(|result| result.render_json_at(self.captured_at_ms))
            .unwrap_or_else(|| "null".to_string());
        format!(
            "{{\"panel_id\":{},\"captured_at_ms\":{},\"pool\":{},\"queries\":[{}],\"latest_explain\":{}}}",
            jet_devtools_database_json_string(&self.panel_id),
            self.captured_at_ms,
            self.pool.render_json_at(self.captured_at_ms),
            queries,
            explain,
        )
    }

    /// Project every fact as a normal `jet.devtools.v1` event.  No event gets
    /// a payload: SQL values remain absent, and redacted binding metadata is
    /// part of the request fields only.
    pub fn to_protocol_events(
        &self,
        source: impl Into<String>,
    ) -> Result<Vec<JetDevtoolsEvent>, JetDevtoolsDatabaseFactError> {
        let source = source.into();
        jet_devtools_database_validate_text(&source, "source")?;
        let mut events = Vec::with_capacity(
            1 + self.queries.len() + usize::from(self.latest_explain.is_some()),
        );
        events.push(self.pool.to_protocol_event(source.clone())?);
        for query in &self.queries {
            events.push(query.to_protocol_event(source.clone())?);
        }
        if let Some(result) = &self.latest_explain {
            events.push(result.to_protocol_event(source)?);
        }
        Ok(events)
    }
}

#[cfg(test)]
mod jet_devtools_database_panel_tests {
    use super::*;

    fn pool() -> JetDevtoolsDatabasePoolStats {
        JetDevtoolsDatabasePoolStats::new(
            "primary",
            JetDevtoolsDatabasePoolLifecycle::Open,
            4,
            0,
            4,
            0,
            2,
            1,
            8,
            4,
            0,
            0,
            0,
            0,
            100,
        )
        .unwrap()
    }

    fn query(id: &str, observed_at_ms: u64) -> JetDevtoolsDatabaseQueryStats {
        JetDevtoolsDatabaseQueryStats::new(
            id,
            "sha256:query",
            JetDevtoolsDatabaseQueryStatus::Succeeded,
            25,
            "src/routes/orders.jet:8",
            Some("req-7".to_string()),
            vec![JetDevtoolsDatabaseTableUsage::new("orders", 3, 0).unwrap()],
            vec![JetDevtoolsDatabaseIndexUsage::new("orders_by_id", true, 3).unwrap()],
            20,
            observed_at_ms,
        )
        .unwrap()
    }

    #[test]
    fn pressure_and_query_projection_are_deterministic() {
        assert_eq!(pool().pressure(), JetDevtoolsDatabasePoolPressure::Exhausted);
        assert_eq!(pool().utilization_milli(), 1_000);
        let query = query("q-1", 20);
        assert!(query.is_slow());
        assert_eq!(query.request_link(), Some("req-7"));
        let json = query.render_json();
        assert!(json.contains("\"duration_ms\":25"));
        assert!(json.contains("\"request_id\":\"req-7\""));
    }

    #[test]
    fn history_is_bounded_and_snapshot_order_is_stable() {
        let mut history = JetDevtoolsDatabaseQueryHistory::new(2);
        history.push(query("q-2", 20)).unwrap();
        history.push(query("q-1", 10)).unwrap();
        history.push(query("q-3", 30)).unwrap();
        assert_eq!(history.len(), 2);
        assert!(history.entries().all(|entry| entry.query_id != "q-2"));
        let ids = history
            .snapshot()
            .into_iter()
            .map(|entry| entry.query_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["q-1", "q-3"]);
    }

    #[test]
    fn explain_requires_exact_grant_and_redacts_bindings() {
        let request = JetDevtoolsDatabaseExplainFactRequest::new(
            "q-1",
            "sha256:query",
            JetDevtoolsDatabaseStatementClass::ReadOnly,
            "src/routes/orders.jet:8",
            Some("req-7".to_string()),
            vec![JetDevtoolsDatabaseRedactedBinding::new(
                0,
                JetDevtoolsDatabaseBindingKind::Text,
            )],
            1_000,
            2,
        )
        .unwrap();
        let denied = JetDevtoolsDatabaseCapabilityFact::for_explain("q-2", true).unwrap();
        assert_eq!(
            request.authorize(&denied, 100).unwrap_err(),
            JetDevtoolsDatabaseFactError::MismatchedExplainAuthority
        );
        let granted = JetDevtoolsDatabaseCapabilityFact::for_explain("q-1", true).unwrap();
        let authorized = request.authorize(&granted, 100).unwrap();
        let json = request.render_json(100);
        assert!(json.contains("\"redacted\":true"));
        assert!(!json.contains("secret"));
        let row = JetDevtoolsDatabasePlanRow::new(
            0,
            "node-0",
            None,
            0,
            "Index Scan",
            Some("orders".to_string()),
            Some("orders_by_id".to_string()),
            Some(3),
            Some(3),
        )
        .unwrap();
        let result = authorized.finish(150, vec![row]).unwrap();
        assert_eq!(result.status(), JetDevtoolsDatabaseExplainStatus::Complete);
        assert_eq!(result.duration_ms, 50);
    }

    #[test]
    fn mutation_and_missing_authority_are_rejected_without_sql_payloads() {
        assert_eq!(
            JetDevtoolsDatabaseExplainFactRequest::new(
                "q-1",
                "sha256:query",
                JetDevtoolsDatabaseStatementClass::Mutation,
                "source",
                None,
                Vec::new(),
                1_000,
                1,
            )
            .unwrap_err(),
            JetDevtoolsDatabaseFactError::UnsupportedStatement
        );
        let request = JetDevtoolsDatabaseExplainFactRequest::new(
            "q-1",
            "sha256:query",
            JetDevtoolsDatabaseStatementClass::ReadOnly,
            "source",
            None,
            Vec::new(),
            1_000,
            1,
        )
        .unwrap();
        let denied = JetDevtoolsDatabaseCapabilityFact::for_explain("q-1", false).unwrap();
        assert_eq!(
            request.authorize(&denied, 0).unwrap_err(),
            JetDevtoolsDatabaseFactError::MissingExplainAuthority
        );
        assert!(!request.render_json(0).contains("DROP"));
    }
}
