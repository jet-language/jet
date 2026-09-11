// D-DX-DEVTOOLS-TELEMETRY1 / card #2453: metrics, logs, and traces are one
// bounded, typed observation model.  Hosts project these facts into charts,
// tables, filters, and trace links; they do not re-encode signal semantics.
//
// This module is included beside Prelude/Devtools.rs.  Its bounded facts and
// projections cross the same `jet.devtools.v1` envelope; values are absent
// until an observation site explicitly publishes them.

pub const JET_DEVTOOLS_TELEMETRY_MAX_TAGS: usize = 16;
pub const JET_DEVTOOLS_TELEMETRY_MAX_TEXT_BYTES: usize = 16 * 1024;
pub const JET_DEVTOOLS_TELEMETRY_MAX_METRIC_SERIES: usize = 64;
pub const JET_DEVTOOLS_TELEMETRY_MAX_METRIC_POINTS: usize = 256;
pub const JET_DEVTOOLS_TELEMETRY_MAX_LOG_RECORDS: usize = 256;
pub const JET_DEVTOOLS_TELEMETRY_MAX_TRACE_SPANS: usize = 256;
pub const JET_DEVTOOLS_TELEMETRY_MAX_LINKED_LOGS: usize = 32;
pub const JET_DEVTOOLS_TELEMETRY_MAX_SLOW_WORK_ROWS: usize = 256;
pub const JET_DEVTOOLS_TELEMETRY_DEFAULT_SLOW_WORK_NS: u64 = 100_000_000;

/// Retention is a fact carried by the panel, not a host-local cache setting.
/// Every capacity has a published upper bound so a caller cannot turn a
/// development observation surface into an unbounded store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JetDevtoolsTelemetryRetention {
    pub metric_series: usize,
    pub metric_points: usize,
    pub log_records: usize,
    pub trace_spans: usize,
    pub linked_logs: usize,
    pub slow_work_rows: usize,
}

impl JetDevtoolsTelemetryRetention {
    pub const fn development() -> Self {
        Self {
            metric_series: JET_DEVTOOLS_TELEMETRY_MAX_METRIC_SERIES,
            metric_points: JET_DEVTOOLS_TELEMETRY_MAX_METRIC_POINTS,
            log_records: JET_DEVTOOLS_TELEMETRY_MAX_LOG_RECORDS,
            trace_spans: JET_DEVTOOLS_TELEMETRY_MAX_TRACE_SPANS,
            linked_logs: JET_DEVTOOLS_TELEMETRY_MAX_LINKED_LOGS,
            slow_work_rows: JET_DEVTOOLS_TELEMETRY_MAX_SLOW_WORK_ROWS,
        }
    }

    pub fn bounded(
        metric_series: usize,
        metric_points: usize,
        log_records: usize,
        trace_spans: usize,
        linked_logs: usize,
        slow_work_rows: usize,
    ) -> Self {
        Self {
            metric_series: metric_series
                .clamp(1, JET_DEVTOOLS_TELEMETRY_MAX_METRIC_SERIES),
            metric_points: metric_points
                .clamp(1, JET_DEVTOOLS_TELEMETRY_MAX_METRIC_POINTS),
            log_records: log_records.clamp(1, JET_DEVTOOLS_TELEMETRY_MAX_LOG_RECORDS),
            trace_spans: trace_spans.clamp(1, JET_DEVTOOLS_TELEMETRY_MAX_TRACE_SPANS),
            linked_logs: linked_logs.clamp(1, JET_DEVTOOLS_TELEMETRY_MAX_LINKED_LOGS),
            slow_work_rows: slow_work_rows.clamp(1, JET_DEVTOOLS_TELEMETRY_MAX_SLOW_WORK_ROWS),
        }
    }
}

impl Default for JetDevtoolsTelemetryRetention {
    fn default() -> Self {
        Self::development()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetDevtoolsTelemetryError {
    TextTooLong(&'static str),
    EmptyIdentity(&'static str),
    DuplicateTag(String),
    InvalidMetricValue,
    InvalidLogValue,
    TooManyTags,
    TooManyFields,
    CapacityExceeded(&'static str),
    UnknownInstrument(String),
    DuplicateInstrument(String),
    InstrumentMismatch { expected: String, actual: String },
    NonMonotonicTimestamp { previous: u64, actual: u64 },
    TraceContextIncomplete,
    TraceIdentityMismatch,
    SpanIdentityMismatch,
    InvalidSpanBounds { start_ns: u64, end_ns: u64 },
    TooManyLinkedLogs,
    SequenceExhausted(&'static str),
}

impl JetDevtoolsTelemetryError {
    pub fn message(&self) -> String {
        match self {
            Self::TextTooLong(field) => format!(
                "devtools telemetry {field} exceeds {} bytes",
                JET_DEVTOOLS_TELEMETRY_MAX_TEXT_BYTES
            ),
            Self::EmptyIdentity(field) => format!("devtools telemetry {field} must not be empty"),
            Self::DuplicateTag(key) => format!("devtools telemetry tag `{key}` is duplicated"),
            Self::InvalidMetricValue => {
                "devtools telemetry metric value must be finite".to_string()
            }
            Self::InvalidLogValue => "devtools telemetry log value must be finite".to_string(),
            Self::TooManyTags => format!(
                "devtools telemetry metric has more than {} tags",
                JET_DEVTOOLS_TELEMETRY_MAX_TAGS
            ),
            Self::TooManyFields => format!(
                "devtools telemetry log has more than {} fields",
                JET_DEVTOOLS_TELEMETRY_MAX_TAGS
            ),
            Self::CapacityExceeded(signal) => {
                format!("devtools telemetry {signal} retention capacity is exhausted")
            }
            Self::UnknownInstrument(name) => {
                format!("devtools telemetry instrument `{name}` is not registered")
            }
            Self::DuplicateInstrument(name) => {
                format!("devtools telemetry instrument `{name}` is registered twice")
            }
            Self::InstrumentMismatch { expected, actual } => format!(
                "devtools telemetry point names instrument `{actual}`, expected `{expected}`"
            ),
            Self::NonMonotonicTimestamp { previous, actual } => format!(
                "devtools telemetry timestamp moved backward from {previous} to {actual}"
            ),
            Self::TraceContextIncomplete => {
                "devtools telemetry trace and span ids must be present together".to_string()
            }
            Self::TraceIdentityMismatch => {
                "devtools telemetry log and span trace ids do not match".to_string()
            }
            Self::SpanIdentityMismatch => {
                "devtools telemetry log and span ids do not match".to_string()
            }
            Self::InvalidSpanBounds { start_ns, end_ns } => {
                format!("devtools telemetry span ends at {end_ns} before start {start_ns}")
            }
            Self::TooManyLinkedLogs => format!(
                "devtools telemetry span has more than {} linked logs",
                JET_DEVTOOLS_TELEMETRY_MAX_LINKED_LOGS
            ),
            Self::SequenceExhausted(signal) => {
                format!("devtools telemetry {signal} sequence is exhausted")
            }
        }
    }
}
impl std::fmt::Display for JetDevtoolsTelemetryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for JetDevtoolsTelemetryError {}

/// A source location is a stable compiler identity, not a human-readable log
/// line.  Hosts use `source_id` as the join key and may display the location.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct JetDevtoolsSourceIdentity {
    pub source_id: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub function: String,
}

impl JetDevtoolsSourceIdentity {
    pub fn new(
        source_id: impl Into<String>,
        file: impl Into<String>,
        line: u32,
        column: u32,
        function: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            file: file.into(),
            line,
            column,
            function: function.into(),
        }
    }

    pub fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        jet_devtools_telemetry_require_text(&self.source_id, "source id")?;
        jet_devtools_telemetry_require_text(&self.file, "source file")?;
        jet_devtools_telemetry_require_text(&self.function, "source function")
    }
}

/// One identity shared by all three signals.  A metric or log may be outside
/// a trace; in that case both trace ids are absent rather than fabricated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsTelemetryContext {
    pub source: JetDevtoolsSourceIdentity,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub request_id: Option<String>,
}

impl JetDevtoolsTelemetryContext {
    pub fn new(
        source: JetDevtoolsSourceIdentity,
        trace_id: Option<String>,
        span_id: Option<String>,
        request_id: Option<String>,
    ) -> Self {
        Self {
            source,
            trace_id,
            span_id,
            request_id,
        }
    }

    pub fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        self.source.validate()?;
        match (&self.trace_id, &self.span_id) {
            (Some(trace), Some(span)) => {
                jet_devtools_telemetry_require_text(trace, "trace id")?;
                jet_devtools_telemetry_require_text(span, "span id")?;
            }
            (None, None) => {}
            _ => return Err(JetDevtoolsTelemetryError::TraceContextIncomplete),
        }
        if let Some(request) = self.request_id.as_deref() {
            jet_devtools_telemetry_require_text(request, "request id")?;
        }
        Ok(())
    }

    pub fn source_id(&self) -> &str {
        &self.source.source_id
    }

    pub fn is_traced(&self) -> bool {
        self.trace_id.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsMetricKind {
    Counter,
    Gauge,
    Histogram,
    UpDownCounter,
}

impl JetDevtoolsMetricKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Counter => "counter",
            Self::Gauge => "gauge",
            Self::Histogram => "histogram",
            Self::UpDownCounter => "up_down_counter",
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum JetDevtoolsMetricValue {
    Integer(i64),
    Float(f64),
}

impl JetDevtoolsMetricValue {
    pub const fn integer(value: i64) -> Self {
        Self::Integer(value)
    }

    pub const fn float(value: f64) -> Self {
        Self::Float(value)
    }

    pub fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        match self {
            Self::Integer(_) => Ok(()),
            Self::Float(value) if value.is_finite() => Ok(()),
            Self::Float(_) => Err(JetDevtoolsTelemetryError::InvalidMetricValue),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetDevtoolsMetricTagValue {
    Boolean(bool),
    Integer(i64),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDevtoolsMetricTag {
    pub key: String,
    pub value: JetDevtoolsMetricTagValue,
}

impl JetDevtoolsMetricTag {
    pub fn new(key: impl Into<String>, value: JetDevtoolsMetricTagValue) -> Self {
        Self {
            key: key.into(),
            value,
        }
    }

    pub fn text(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self::new(key, JetDevtoolsMetricTagValue::Text(value.into()))
    }

    pub fn integer(key: impl Into<String>, value: i64) -> Self {
        Self::new(key, JetDevtoolsMetricTagValue::Integer(value))
    }

    pub fn boolean(key: impl Into<String>, value: bool) -> Self {
        Self::new(key, JetDevtoolsMetricTagValue::Boolean(value))
    }
    fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        jet_devtools_telemetry_require_text(&self.key, "metric tag key")?;
        if let JetDevtoolsMetricTagValue::Text(value) = &self.value {
            jet_devtools_telemetry_require_text(value, "metric tag value")?;
        }
        Ok(())
    }
}

/// Metadata is always safe to inspect; it never contains a measurement value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsMetricInstrument {
    pub name: String,
    pub kind: JetDevtoolsMetricKind,
    pub unit: String,
    pub description: String,
    pub source: JetDevtoolsSourceIdentity,
}

impl JetDevtoolsMetricInstrument {
    pub fn new(
        name: impl Into<String>,
        kind: JetDevtoolsMetricKind,
        unit: impl Into<String>,
        description: impl Into<String>,
        source: JetDevtoolsSourceIdentity,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            unit: unit.into(),
            description: description.into(),
            source,
        }
    }

    pub fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        jet_devtools_telemetry_require_text(&self.name, "metric instrument name")?;
        if !self.unit.is_empty() {
            jet_devtools_telemetry_require_text(&self.unit, "metric instrument unit")?;
        }
        if !self.description.is_empty() {
            jet_devtools_telemetry_require_text(&self.description, "metric instrument description")?;
        }
        self.source.validate()
    }
}

impl JetDevtoolsMetricInstrument {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        self.validate().map_err(|error| error.to_string())?;
        JetDevtoolsEvent::from_parts(
            0,
            source,
            "Metric",
            self.name.clone(),
            jet_devtools_telemetry_metric_instrument_fields(self),
        )
    }
}

fn jet_devtools_telemetry_metric_instrument_fields(
    instrument: &JetDevtoolsMetricInstrument,
) -> String {
    format!("{{{}}}", vec![
        format!(
            "\"name\":{}",
            jet_devtools_telemetry_json_string(&instrument.name)
        ),
        format!(
            "\"kind\":{}",
            jet_devtools_telemetry_json_string(instrument.kind.as_str())
        ),
        format!(
            "\"unit\":{}",
            jet_devtools_telemetry_json_string(&instrument.unit)
        ),
        format!(
            "\"description\":{}",
            jet_devtools_telemetry_json_string(&instrument.description)
        ),
        format!(
            "\"source_id\":{}",
            jet_devtools_telemetry_json_string(&instrument.source.source_id)
        ),
        format!(
            "\"source_file\":{}",
            jet_devtools_telemetry_json_string(&instrument.source.file)
        ),
        format!("\"source_line\":{}", instrument.source.line),
        format!("\"source_column\":{}", instrument.source.column),
        format!(
            "\"source_function\":{}",
            jet_devtools_telemetry_json_string(&instrument.source.function)
        ),
    ].join(","))
}

/// A point starts unpublished.  `publish` is the only operation that makes a
/// metric value visible to a projection; timestamp, tags, and identity remain
/// useful facts even when the value is intentionally withheld.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsMetricPoint {
    pub index: u64,
    pub timestamp_ns: u64,
    pub instrument: String,
    pub tags: Vec<JetDevtoolsMetricTag>,
    pub identity: JetDevtoolsTelemetryContext,
    pub value: Option<JetDevtoolsMetricValue>,
}

impl JetDevtoolsMetricPoint {
    pub fn new(
        timestamp_ns: u64,
        instrument: impl Into<String>,
        tags: Vec<JetDevtoolsMetricTag>,
        identity: JetDevtoolsTelemetryContext,
    ) -> Self {
        let mut tags = tags;
        tags.sort_by(|left, right| left.key.cmp(&right.key));
        Self {
            index: 0,
            timestamp_ns,
            instrument: instrument.into(),
            tags,
            identity,
            value: None,
        }
    }

    pub fn publish(mut self, value: JetDevtoolsMetricValue) -> Self {
        if value.validate().is_ok() {
            self.value = Some(value);
        }
        self
    }

    pub fn is_published(&self) -> bool {
        self.value.is_some()
    }

    pub fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        jet_devtools_telemetry_require_text(&self.instrument, "metric instrument")?;
        self.identity.validate()?;
        if self.tags.len() > JET_DEVTOOLS_TELEMETRY_MAX_TAGS {
            return Err(JetDevtoolsTelemetryError::TooManyTags);
        }
        for (index, tag) in self.tags.iter().enumerate() {
            tag.validate()?;
            if self.tags[index + 1..]
                .iter()
                .any(|other| other.key == tag.key)
            {
                return Err(JetDevtoolsTelemetryError::DuplicateTag(tag.key.clone()));
            }
        }
        if let Some(value) = &self.value {
            value.validate()?;
        }
        Ok(())
    }

    fn chart_point(&self) -> JetDevtoolsMetricChartPoint {
        JetDevtoolsMetricChartPoint {
            index: self.index,
            timestamp_ns: self.timestamp_ns,
            instrument: self.instrument.clone(),
            value: self.value.clone(),
            identity: self.identity.clone(),
        }
    }

    fn table_row(&self) -> JetDevtoolsMetricTableRow {
        JetDevtoolsMetricTableRow {
            index: self.index,
            timestamp_ns: self.timestamp_ns,
            instrument: self.instrument.clone(),
            tags: self.tags.clone(),
            value: self.value.clone(),
            identity: self.identity.clone(),
        }
    }
}

impl JetDevtoolsMetricPoint {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        self.validate().map_err(|error| error.to_string())?;
        JetDevtoolsEvent::from_parts_with_payload(
            self.timestamp_ns / 1_000_000,
            source,
            "Metric",
            self.instrument.clone(),
            jet_devtools_telemetry_metric_fields(self),
            self.value.as_ref().map(|_| self.table_row().payload_json()),
        )
    }
}

fn jet_devtools_telemetry_metric_fields(point: &JetDevtoolsMetricPoint) -> String {
    let mut fields = vec![
        format!("\"index\":{}", point.index),
        format!("\"timestamp_ns\":{}", point.timestamp_ns),
        format!(
            "\"instrument\":{}",
            jet_devtools_telemetry_json_string(&point.instrument)
        ),
        format!(
            "\"source_id\":{}",
            jet_devtools_telemetry_json_string(point.identity.source_id())
        ),
        format!(
            "\"source_file\":{}",
            jet_devtools_telemetry_json_string(&point.identity.source.file)
        ),
        format!("\"source_line\":{}", point.identity.source.line),
        format!("\"source_column\":{}", point.identity.source.column),
        format!(
            "\"source_function\":{}",
            jet_devtools_telemetry_json_string(&point.identity.source.function)
        ),
        format!(
            "\"tags\":{}",
            jet_devtools_telemetry_tags_json(&point.tags)
        ),
    ];
    jet_devtools_telemetry_push_context_json(&point.identity, &mut fields);
    format!("{{{}}}", fields.join(","))
}


#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsMetricChartPoint {
    pub index: u64,
    pub timestamp_ns: u64,
    pub instrument: String,
    pub value: Option<JetDevtoolsMetricValue>,
    pub identity: JetDevtoolsTelemetryContext,
}

impl JetDevtoolsMetricChartPoint {
    pub fn source_id(&self) -> &str {
        self.identity.source_id()
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.identity.trace_id.as_deref()
    }

    pub fn span_id(&self) -> Option<&str> {
        self.identity.span_id.as_deref()
    }

    pub fn payload_json(&self) -> String {
        let mut fields = vec![
            format!("\"index\":{}", self.index),
            format!("\"timestamp_ns\":{}", self.timestamp_ns),
            format!("\"instrument\":{}", jet_devtools_telemetry_json_string(&self.instrument)),
            format!("\"source_id\":{}", jet_devtools_telemetry_json_string(self.source_id())),
        ];
        jet_devtools_telemetry_push_context_json(&self.identity, &mut fields);
        if let Some(value) = &self.value {
            fields.push(format!("\"value\":{}", jet_devtools_telemetry_metric_value_json(value)));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsMetricTableRow {
    pub index: u64,
    pub timestamp_ns: u64,
    pub instrument: String,
    pub tags: Vec<JetDevtoolsMetricTag>,
    pub value: Option<JetDevtoolsMetricValue>,
    pub identity: JetDevtoolsTelemetryContext,
}

impl JetDevtoolsMetricTableRow {
    pub fn source_id(&self) -> &str {
        self.identity.source_id()
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.identity.trace_id.as_deref()
    }

    pub fn span_id(&self) -> Option<&str> {
        self.identity.span_id.as_deref()
    }

    pub fn payload_json(&self) -> String {
        let mut fields = vec![
            format!("\"index\":{}", self.index),
            format!("\"timestamp_ns\":{}", self.timestamp_ns),
            format!("\"instrument\":{}", jet_devtools_telemetry_json_string(&self.instrument)),
            format!("\"source_id\":{}", jet_devtools_telemetry_json_string(self.source_id())),
            format!("\"tags\":{}", jet_devtools_telemetry_tags_json(&self.tags)),
        ];
        jet_devtools_telemetry_push_context_json(&self.identity, &mut fields);
        if let Some(value) = &self.value {
            fields.push(format!("\"value\":{}", jet_devtools_telemetry_metric_value_json(value)));
        }
        format!("{{{}}}", fields.join(","))
    }
}

/// One named metric's deterministic, bounded series.  Point indexes increase
/// from one and remain monotonic after the oldest point is evicted.
#[derive(Clone, Debug)]
pub struct JetDevtoolsMetricSeries {
    pub instrument: JetDevtoolsMetricInstrument,
    capacity: usize,
    points: std::collections::VecDeque<JetDevtoolsMetricPoint>,
    next_index: u64,
}

impl JetDevtoolsMetricSeries {
    pub fn new(instrument: JetDevtoolsMetricInstrument, capacity: usize) -> Self {
        Self {
            instrument,
            capacity: capacity.clamp(1, JET_DEVTOOLS_TELEMETRY_MAX_METRIC_POINTS),
            points: std::collections::VecDeque::new(),
            next_index: 1,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn points(&self) -> impl Iterator<Item = &JetDevtoolsMetricPoint> {
        self.points.iter()
    }

    pub fn record(&mut self, mut point: JetDevtoolsMetricPoint) -> Result<u64, JetDevtoolsTelemetryError> {
        point.tags.sort_by(|left, right| left.key.cmp(&right.key));
        self.instrument.validate()?;
        point.validate()?;
        if point.instrument != self.instrument.name {
            return Err(JetDevtoolsTelemetryError::InstrumentMismatch {
                expected: self.instrument.name.clone(),
                actual: point.instrument,
            });
        }
        if let Some(previous) = self.points.back() {
            if point.timestamp_ns < previous.timestamp_ns {
                return Err(JetDevtoolsTelemetryError::NonMonotonicTimestamp {
                    previous: previous.timestamp_ns,
                    actual: point.timestamp_ns,
                });
            }
        }
        if self.next_index == u64::MAX {
            return Err(JetDevtoolsTelemetryError::SequenceExhausted("metric point"));
        }
        point.index = self.next_index;
        self.next_index += 1;
        if self.points.len() == self.capacity {
            self.points.pop_front();
        }
        self.points.push_back(point);
        Ok(self.next_index - 1)
    }

    pub fn chart_points(&self) -> Vec<JetDevtoolsMetricChartPoint> {
        self.points
            .iter()
            .map(JetDevtoolsMetricPoint::chart_point)
            .collect()
    }

    pub fn table_rows(&self) -> Vec<JetDevtoolsMetricTableRow> {
        self.points
            .iter()
            .map(JetDevtoolsMetricPoint::table_row)
            .collect()
    }
}

/// The metric registry owns deterministic series order and a common per-series
/// point bound.  Series are sorted by instrument name after every registration.
#[derive(Clone, Debug)]
pub struct JetDevtoolsMetricStore {
    capacity: usize,
    max_series: usize,
    series: Vec<JetDevtoolsMetricSeries>,
}

impl JetDevtoolsMetricStore {
    pub fn new(retention: JetDevtoolsTelemetryRetention) -> Self {
        Self {
            capacity: retention.metric_points,
            max_series: retention.metric_series,
            series: Vec::new(),
        }
    }

    pub fn register(
        &mut self,
        instrument: JetDevtoolsMetricInstrument,
    ) -> Result<(), JetDevtoolsTelemetryError> {
        instrument.validate()?;
        if self.series.iter().any(|series| series.instrument.name == instrument.name) {
            return Err(JetDevtoolsTelemetryError::DuplicateInstrument(instrument.name));
        }
        if self.series.len() == self.max_series {
            return Err(JetDevtoolsTelemetryError::CapacityExceeded("metric series"));
        }
        self.series
            .push(JetDevtoolsMetricSeries::new(instrument, self.capacity));
        self.series
            .sort_by(|left, right| left.instrument.name.cmp(&right.instrument.name));
        Ok(())
    }

    pub fn series(&self) -> impl Iterator<Item = &JetDevtoolsMetricSeries> {
        self.series.iter()
    }

    pub fn record(&mut self, point: JetDevtoolsMetricPoint) -> Result<u64, JetDevtoolsTelemetryError> {
        let instrument = point.instrument.clone();
        self.series
            .iter_mut()
            .find(|series| series.instrument.name == instrument)
            .ok_or(JetDevtoolsTelemetryError::UnknownInstrument(instrument))?
            .record(point)
    }

    pub fn chart_points(&self, instrument: &str) -> Vec<JetDevtoolsMetricChartPoint> {
        self.series
            .iter()
            .find(|series| series.instrument.name == instrument)
            .map_or_else(Vec::new, JetDevtoolsMetricSeries::chart_points)
    }

    pub fn table_rows(&self, instrument: Option<&str>) -> Vec<JetDevtoolsMetricTableRow> {
        self.series
            .iter()
            .filter(|series| instrument.is_none_or(|name| name == series.instrument.name))
            .flat_map(JetDevtoolsMetricSeries::table_rows)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl JetDevtoolsLogLevel {
    pub const fn rank(self) -> u8 {
        match self {
            Self::Trace => 0,
            Self::Debug => 1,
            Self::Info => 2,
            Self::Warn => 3,
            Self::Error => 4,
            Self::Fatal => 5,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Fatal => "fatal",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetDevtoolsLogValue {
    Boolean(bool),
    Integer(i64),
    Float(f64),
    Text(String),
}

impl JetDevtoolsLogValue {
    pub const fn boolean(value: bool) -> Self {
        Self::Boolean(value)
    }

    pub const fn integer(value: i64) -> Self {
        Self::Integer(value)
    }

    pub const fn float(value: f64) -> Self {
        Self::Float(value)
    }

    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        match self {
            Self::Float(value) if !value.is_finite() => {
                Err(JetDevtoolsTelemetryError::InvalidLogValue)
            }
            Self::Text(value) => jet_devtools_telemetry_require_text(value, "log value"),
            _ => Ok(()),
        }
    }
}

/// A field has a type even when its value is unpublished.  Redaction clears
/// the value and leaves a visible redaction fact for hosts.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsLogField {
    pub key: String,
    pub value: Option<JetDevtoolsLogValue>,
    pub redacted: bool,
}

impl JetDevtoolsLogField {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: None,
            redacted: false,
        }
    }

    pub fn publish(mut self, value: JetDevtoolsLogValue) -> Self {
        if value.validate().is_ok() && !self.redacted {
            self.value = Some(value);
        }
        self
    }

    pub fn redact(mut self) -> Self {
        self.value = None;
        self.redacted = true;
        self
    }

    fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        jet_devtools_telemetry_require_text(&self.key, "log field key")?;
        if let Some(value) = &self.value {
            value.validate()?;
        }
        Ok(())
    }
}

/// Structured log metadata is retained with trace/source identity.  Body and
/// field values are optional because a log event is payload-free by default.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsLogRecord {
    pub index: u64,
    pub timestamp_ns: u64,
    pub level: JetDevtoolsLogLevel,
    pub body: Option<String>,
    pub fields: Vec<JetDevtoolsLogField>,
    pub identity: JetDevtoolsTelemetryContext,
}

impl JetDevtoolsLogRecord {
    pub fn new(
        timestamp_ns: u64,
        level: JetDevtoolsLogLevel,
        identity: JetDevtoolsTelemetryContext,
    ) -> Self {
        Self {
            index: 0,
            timestamp_ns,
            level,
            body: None,
            fields: Vec::new(),
            identity,
        }
    }

    pub fn publish_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn with_field(mut self, field: JetDevtoolsLogField) -> Self {
        if field.validate().is_ok()
            && self.fields.len() < JET_DEVTOOLS_TELEMETRY_MAX_TAGS
            && !self.fields.iter().any(|existing| existing.key == field.key)
        {
            self.fields.push(field);
            self.fields.sort_by(|left, right| left.key.cmp(&right.key));
        }
        self
    }

    pub fn source_id(&self) -> &str {
        self.identity.source_id()
    }

    pub fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        self.identity.validate()?;
        if let Some(body) = &self.body {
            jet_devtools_telemetry_require_text(body, "log body")?;
        }
        if self.fields.len() > JET_DEVTOOLS_TELEMETRY_MAX_TAGS {
            return Err(JetDevtoolsTelemetryError::TooManyFields);
        }
        for (index, field) in self.fields.iter().enumerate() {
            field.validate()?;
            if self.fields[index + 1..]
                .iter()
                .any(|other| other.key == field.key)
            {
                return Err(JetDevtoolsTelemetryError::DuplicateTag(field.key.clone()));
            }
        }
        Ok(())
    }

    fn query_row(&self) -> JetDevtoolsLogQueryRow {
        JetDevtoolsLogQueryRow {
            index: self.index,
            timestamp_ns: self.timestamp_ns,
            level: self.level,
            body: self.body.clone(),
            fields: self.fields.clone(),
            identity: self.identity.clone(),
        }
    }

    /// Render only the values explicitly published on this record.  The
    /// metadata event remains payload-free; callers attach this object through
    /// `from_parts_with_payload` only when a body or field value was published.
    pub fn payload_json(&self) -> String {
        let mut fields = vec![
            format!("\"index\":{}", self.index),
            format!("\"timestamp_ns\":{}", self.timestamp_ns),
            format!(
                "\"level\":{}",
                jet_devtools_telemetry_json_string(self.level.as_str())
            ),
            format!(
                "\"source_id\":{}",
                jet_devtools_telemetry_json_string(self.source_id())
            ),
        ];
        jet_devtools_telemetry_push_context_json(&self.identity, &mut fields);
        if let Some(body) = &self.body {
            fields.push(format!(
                "\"body\":{}",
                jet_devtools_telemetry_json_string(body)
            ));
        }
        fields.push(format!(
            "\"fields\":{}",
            jet_devtools_telemetry_log_fields_json(&self.fields)
        ));
        format!("{{{}}}", fields.join(","))
    }

    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        self.validate().map_err(|error| error.to_string())?;
        let payload = (self.body.is_some()
            || self.fields.iter().any(|field| field.value.is_some()))
            .then(|| self.payload_json());
        JetDevtoolsEvent::from_parts_with_payload(
            self.timestamp_ns / 1_000_000,
            source,
            "Log",
            self.index.to_string(),
            jet_devtools_telemetry_log_fields(self),
            payload,
        )
    }

}
fn jet_devtools_telemetry_log_fields(record: &JetDevtoolsLogRecord) -> String {
    let mut fields = vec![
        format!("\"index\":{}", record.index),
        format!("\"timestamp_ns\":{}", record.timestamp_ns),
        format!(
            "\"level\":{}",
            jet_devtools_telemetry_json_string(record.level.as_str())
        ),
        format!(
            "\"source_id\":{}",
            jet_devtools_telemetry_json_string(record.source_id())
        ),
        format!(
            "\"source_file\":{}",
            jet_devtools_telemetry_json_string(&record.identity.source.file)
        ),
        format!("\"source_line\":{}", record.identity.source.line),
        format!("\"source_column\":{}", record.identity.source.column),
        format!(
            "\"source_function\":{}",
            jet_devtools_telemetry_json_string(&record.identity.source.function)
        ),
        format!(
            "\"fields\":{}",
            jet_devtools_telemetry_log_metadata_fields_json(&record.fields)
        ),
    ];
    jet_devtools_telemetry_push_context_json(&record.identity, &mut fields);
    format!("{{{}}}", fields.join(","))
}

fn jet_devtools_telemetry_log_metadata_fields_json(
    fields: &[JetDevtoolsLogField],
) -> String {
    let values = fields
        .iter()
        .map(|field| {
            let mut parts = vec![format!(
                "\"key\":{}",
                jet_devtools_telemetry_json_string(&field.key)
            )];
            if field.redacted {
                parts.push("\"redacted\":true".to_string());
            }
            format!("{{{}}}", parts.join(","))
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}


#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsLogQueryRow {
    pub index: u64,
    pub timestamp_ns: u64,
    pub level: JetDevtoolsLogLevel,
    pub body: Option<String>,
    pub fields: Vec<JetDevtoolsLogField>,
    pub identity: JetDevtoolsTelemetryContext,
}

impl JetDevtoolsLogQueryRow {
    pub fn source_id(&self) -> &str {
        self.identity.source_id()
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.identity.trace_id.as_deref()
    }

    pub fn span_id(&self) -> Option<&str> {
        self.identity.span_id.as_deref()
    }

    pub fn payload_json(&self) -> String {
        let mut fields = vec![
            format!("\"index\":{}", self.index),
            format!("\"timestamp_ns\":{}", self.timestamp_ns),
            format!("\"level\":{}", jet_devtools_telemetry_json_string(self.level.as_str())),
            format!("\"source_id\":{}", jet_devtools_telemetry_json_string(self.source_id())),
        ];
        jet_devtools_telemetry_push_context_json(&self.identity, &mut fields);
        if let Some(body) = &self.body {
            fields.push(format!("\"body\":{}", jet_devtools_telemetry_json_string(body)));
        }
        fields.push(format!("\"fields\":{}", jet_devtools_telemetry_log_fields_json(&self.fields)));
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsLogQuery {
    pub minimum_level: Option<JetDevtoolsLogLevel>,
    pub source_id: Option<String>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub request_id: Option<String>,
    pub field_key: Option<String>,
    pub field_value: Option<JetDevtoolsLogValue>,
    pub contains: Option<String>,
    pub from_ns: Option<u64>,
    pub to_ns: Option<u64>,
    pub limit: usize,
}
impl Default for JetDevtoolsLogQuery {
    fn default() -> Self {
        Self {
            minimum_level: None,
            source_id: None,
            trace_id: None,
            span_id: None,
            request_id: None,
            field_key: None,
            field_value: None,
            contains: None,
            from_ns: None,
            to_ns: None,
            limit: JET_DEVTOOLS_TELEMETRY_MAX_LOG_RECORDS,
        }
    }
}

impl JetDevtoolsLogQuery {
    pub fn matches(&self, record: &JetDevtoolsLogRecord) -> bool {
        if self
            .minimum_level
            .is_some_and(|level| record.level.rank() < level.rank())
        {
            return false;
        }
        if self
            .source_id
            .as_deref()
            .is_some_and(|source| source != record.source_id())
        {
            return false;
        }
        if self
            .trace_id
            .as_deref()
            .is_some_and(|trace| record.identity.trace_id.as_deref() != Some(trace))
        {
            return false;
        }
        if self
            .span_id
            .as_deref()
            .is_some_and(|span| record.identity.span_id.as_deref() != Some(span))
        {
            return false;
        }
        if self
            .request_id
            .as_deref()
            .is_some_and(|request| record.identity.request_id.as_deref() != Some(request))
        {
            return false;
        }
        if let Some(key) = &self.field_key {
            let matching_field = record.fields.iter().find(|field| &field.key == key);
            if self.field_value.as_ref().is_some_and(|value| {
                matching_field.and_then(|field| field.value.as_ref()) != Some(value)
            }) {
                return false;
            }
            if self.field_value.is_none() && matching_field.is_none() {
                return false;
            }
        } else if self.field_value.is_some() {
            return false;
        }
        if self
            .from_ns
            .is_some_and(|from| record.timestamp_ns < from)
        {
            return false;
        }
        if self.to_ns.is_some_and(|to| record.timestamp_ns > to) {
            return false;
        }
        if let Some(needle) = &self.contains {
            if record.body.as_deref().is_none_or(|body| !body.contains(needle)) {
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Debug)]
pub struct JetDevtoolsLogRing {
    capacity: usize,
    records: std::collections::VecDeque<JetDevtoolsLogRecord>,
    next_index: u64,
}

impl JetDevtoolsLogRing {
    pub fn new(retention: JetDevtoolsTelemetryRetention) -> Self {
        Self {
            capacity: retention.log_records,
            records: std::collections::VecDeque::new(),
            next_index: 1,
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn records(&self) -> impl Iterator<Item = &JetDevtoolsLogRecord> {
        self.records.iter()
    }

    pub fn append(&mut self, mut record: JetDevtoolsLogRecord) -> Result<u64, JetDevtoolsTelemetryError> {
        record.fields.sort_by(|left, right| left.key.cmp(&right.key));
        record.validate()?;
        if let Some(previous) = self.records.back() {
            if record.timestamp_ns < previous.timestamp_ns {
                return Err(JetDevtoolsTelemetryError::NonMonotonicTimestamp {
                    previous: previous.timestamp_ns,
                    actual: record.timestamp_ns,
                });
            }
        }
        if self.next_index == u64::MAX {
            return Err(JetDevtoolsTelemetryError::SequenceExhausted("log record"));
        }
        record.index = self.next_index;
        self.next_index += 1;
        if self.records.len() == self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
        Ok(self.next_index - 1)
    }

    pub fn query(&self, query: &JetDevtoolsLogQuery) -> Vec<JetDevtoolsLogQueryRow> {
        self.records
            .iter()
            .filter(|record| query.matches(record))
            .take(query.limit.min(JET_DEVTOOLS_TELEMETRY_MAX_LOG_RECORDS))
            .map(JetDevtoolsLogRecord::query_row)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsSpanKind {
    Internal,
    Server,
    Client,
    Producer,
    Consumer,
}

impl JetDevtoolsSpanKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Server => "server",
            Self::Client => "client",
            Self::Producer => "producer",
            Self::Consumer => "consumer",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsSpanStatus {
    Unset,
    Ok,
    Error,
}
impl JetDevtoolsSpanStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unset => "unset",
            Self::Ok => "ok",
            Self::Error => "error",
        }
    }
}

/// One trace span carries the same context object used by metric and log
/// records.  It may be open while it is retained; duration is then absent.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsTraceSpan {
    pub index: u64,
    pub name: String,
    pub kind: JetDevtoolsSpanKind,
    pub parent_span_id: Option<String>,
    pub start_ns: u64,
    pub end_ns: Option<u64>,
    pub status: JetDevtoolsSpanStatus,
    pub identity: JetDevtoolsTelemetryContext,
    pub linked_log_indexes: Vec<u64>,
}


impl JetDevtoolsTraceSpan {
    pub fn new(
        name: impl Into<String>,
        kind: JetDevtoolsSpanKind,
        start_ns: u64,
        identity: JetDevtoolsTelemetryContext,
    ) -> Self {
        Self {
            index: 0,
            name: name.into(),
            kind,
            parent_span_id: None,
            start_ns,
            end_ns: None,
            status: JetDevtoolsSpanStatus::Unset,
            identity,
            linked_log_indexes: Vec::new(),
        }
    }

    pub fn with_parent_span(mut self, parent_span_id: impl Into<String>) -> Self {
        self.parent_span_id = Some(parent_span_id.into());
        self

    }
    pub fn finish(
        &mut self,
        end_ns: u64,
        status: JetDevtoolsSpanStatus,
    ) -> Result<(), JetDevtoolsTelemetryError> {
        if end_ns < self.start_ns {
            return Err(JetDevtoolsTelemetryError::InvalidSpanBounds {
                start_ns: self.start_ns,
                end_ns,
            });
        }
        self.end_ns = Some(end_ns);
        self.status = status;
        Ok(())
    }

    pub fn duration_ns(&self) -> Option<u64> {
        self.end_ns.map(|end| end - self.start_ns)
    }

    pub fn source_id(&self) -> &str {
        self.identity.source_id()
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.identity.trace_id.as_deref()
    }

    pub fn span_id(&self) -> Option<&str> {
        self.identity.span_id.as_deref()
    }

    pub fn validate(&self) -> Result<(), JetDevtoolsTelemetryError> {
        jet_devtools_telemetry_require_text(&self.name, "span name")?;
        self.identity.validate()?;
        if let Some(parent_span_id) = &self.parent_span_id {
            jet_devtools_telemetry_require_text(parent_span_id, "parent span id")?;
        }
        if self.identity.trace_id.is_none() || self.identity.span_id.is_none() {
            return Err(JetDevtoolsTelemetryError::TraceContextIncomplete);
        }
        if let Some(end_ns) = self.end_ns {
            if end_ns < self.start_ns {
                return Err(JetDevtoolsTelemetryError::InvalidSpanBounds {
                    start_ns: self.start_ns,
                    end_ns,
                });
            }
        }
        if self.linked_log_indexes.len() > JET_DEVTOOLS_TELEMETRY_MAX_LINKED_LOGS {
            return Err(JetDevtoolsTelemetryError::TooManyLinkedLogs);
        }
        Ok(())
    }

    pub fn link_log(
        &mut self,
        record: &JetDevtoolsLogRecord,
    ) -> Result<(), JetDevtoolsTelemetryError> {
        self.link_log_with_limit(record, JET_DEVTOOLS_TELEMETRY_MAX_LINKED_LOGS)
    }

    fn link_log_with_limit(
        &mut self,
        record: &JetDevtoolsLogRecord,
        limit: usize,
    ) -> Result<(), JetDevtoolsTelemetryError> {
        if self.identity.trace_id != record.identity.trace_id {
            return Err(JetDevtoolsTelemetryError::TraceIdentityMismatch);
        }
        if self.identity.span_id != record.identity.span_id {
            return Err(JetDevtoolsTelemetryError::SpanIdentityMismatch);
        }
        if self.linked_log_indexes.contains(&record.index) {
            return Ok(());
        }
        if self.linked_log_indexes.len() == limit {
            return Err(JetDevtoolsTelemetryError::TooManyLinkedLogs);
        }
        self.linked_log_indexes.push(record.index);
        self.linked_log_indexes.sort_unstable();
        Ok(())
    }

    fn row(&self) -> JetDevtoolsTraceRow {
        JetDevtoolsTraceRow {
            index: self.index,
            name: self.name.clone(),
            kind: self.kind,
            parent_span_id: self.parent_span_id.clone(),
            start_ns: self.start_ns,
            end_ns: self.end_ns,
            duration_ns: self.duration_ns(),
            status: self.status,
            trace_id: self.identity.trace_id.clone(),
            span_id: self.identity.span_id.clone(),
            source: self.identity.source.clone(),
            request_id: self.identity.request_id.clone(),
            linked_log_indexes: self.linked_log_indexes.clone(),
        }
    }
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        self.validate().map_err(|error| error.to_string())?;
        let entity = self
            .span_id()
            .map_or_else(|| self.index.to_string(), str::to_string);
        JetDevtoolsEvent::from_parts(
            self.start_ns / 1_000_000,
            source,
            "Trace",
            entity,
            self.row().payload_json(),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsTraceRow {
    pub index: u64,
    pub name: String,
    pub kind: JetDevtoolsSpanKind,
    pub parent_span_id: Option<String>,
    pub start_ns: u64,
    pub end_ns: Option<u64>,
    pub duration_ns: Option<u64>,
    pub status: JetDevtoolsSpanStatus,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub source: JetDevtoolsSourceIdentity,
    pub request_id: Option<String>,
    pub linked_log_indexes: Vec<u64>,
}
impl JetDevtoolsTraceRow {
    pub fn source_id(&self) -> &str {
        &self.source.source_id
    }

    pub fn payload_json(&self) -> String {
        let mut fields = vec![
            format!("\"index\":{}", self.index),
            format!("\"name\":{}", jet_devtools_telemetry_json_string(&self.name)),
            format!("\"kind\":{}", jet_devtools_telemetry_json_string(self.kind.as_str())),
            format!("\"parent_span_id\":{}", jet_devtools_telemetry_optional_string_json(self.parent_span_id.as_deref())),
            format!("\"start_ns\":{}", self.start_ns),
            format!("\"status\":{}", jet_devtools_telemetry_json_string(self.status.as_str())),
            format!("\"trace_id\":{}", jet_devtools_telemetry_optional_string_json(self.trace_id.as_deref())),
            format!("\"span_id\":{}", jet_devtools_telemetry_optional_string_json(self.span_id.as_deref())),
            format!("\"source_id\":{}", jet_devtools_telemetry_json_string(self.source_id())),
            format!("\"linked_log_indexes\":{}", jet_devtools_telemetry_u64_array_json(&self.linked_log_indexes)),
        ];
        if let Some(end_ns) = self.end_ns {
            fields.push(format!("\"end_ns\":{}", end_ns));
        }
        if let Some(duration_ns) = self.duration_ns {
            fields.push(format!("\"duration_ns\":{}", duration_ns));
        }
        if let Some(request_id) = &self.request_id {
            fields.push(format!("\"request_id\":{}", jet_devtools_telemetry_json_string(request_id)));
        }
        format!("{{{}}}", fields.join(","))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsTraceLogLink {
    pub span_index: u64,
    pub log_index: u64,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub span_source_id: String,
    pub log_source_id: String,
    pub timestamp_ns: u64,
    pub level: JetDevtoolsLogLevel,
    pub body: Option<String>,
}

impl JetDevtoolsTraceLogLink {
    pub fn payload_json(&self) -> String {
        let mut fields = vec![
            format!("\"span_index\":{}", self.span_index),
            format!("\"log_index\":{}", self.log_index),
            format!("\"trace_id\":{}", jet_devtools_telemetry_optional_string_json(self.trace_id.as_deref())),
            format!("\"span_id\":{}", jet_devtools_telemetry_optional_string_json(self.span_id.as_deref())),
            format!("\"span_source_id\":{}", jet_devtools_telemetry_json_string(&self.span_source_id)),
            format!("\"log_source_id\":{}", jet_devtools_telemetry_json_string(&self.log_source_id)),
            format!("\"timestamp_ns\":{}", self.timestamp_ns),
            format!("\"level\":{}", jet_devtools_telemetry_json_string(self.level.as_str())),
        ];
        if let Some(body) = &self.body {
            fields.push(format!("\"body\":{}", jet_devtools_telemetry_json_string(body)));
        }
        format!("{{{}}}", fields.join(","))
    }
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        let payload = self.body.as_ref().map(|_| self.payload_json());
        JetDevtoolsEvent::from_parts_with_payload(
            self.timestamp_ns / 1_000_000,
            source,
            "TraceLink",
            format!("{}:{}", self.span_index, self.log_index),
            jet_devtools_telemetry_trace_link_fields(self),
            payload,
        )
    }
}

fn jet_devtools_telemetry_trace_link_fields(link: &JetDevtoolsTraceLogLink) -> String {
    format!(
        "{{\"span_index\":{},\"log_index\":{},\"trace_id\":{},\"span_id\":{},\"span_source_id\":{},\"log_source_id\":{},\"timestamp_ns\":{},\"level\":{}}}",
        link.span_index,
        link.log_index,
        jet_devtools_telemetry_optional_string_json(link.trace_id.as_deref()),
        jet_devtools_telemetry_optional_string_json(link.span_id.as_deref()),
        jet_devtools_telemetry_json_string(&link.span_source_id),
        jet_devtools_telemetry_json_string(&link.log_source_id),
        link.timestamp_ns,
        jet_devtools_telemetry_json_string(link.level.as_str()),
    )
}

#[derive(Clone, Debug)]
pub struct JetDevtoolsTraceRing {
    capacity: usize,
    linked_logs: usize,
    spans: std::collections::VecDeque<JetDevtoolsTraceSpan>,
    next_index: u64,
}

impl JetDevtoolsTraceRing {
    pub fn new(retention: JetDevtoolsTelemetryRetention) -> Self {
        Self {
            capacity: retention.trace_spans,
            linked_logs: retention.linked_logs,
            spans: std::collections::VecDeque::new(),
            next_index: 1,
        }
    }

    pub fn spans(&self) -> impl Iterator<Item = &JetDevtoolsTraceSpan> {
        self.spans.iter()
    }

    pub fn rows(&self) -> Vec<JetDevtoolsTraceRow> {
        self.spans.iter().map(JetDevtoolsTraceSpan::row).collect()
    }

    pub fn append(&mut self, mut span: JetDevtoolsTraceSpan) -> Result<u64, JetDevtoolsTelemetryError> {
        span.linked_log_indexes.sort_unstable();
        span.validate()?;
        if self.next_index == u64::MAX {
            return Err(JetDevtoolsTelemetryError::SequenceExhausted("trace span"));
        }
        span.index = self.next_index;
        self.next_index += 1;
        if self.spans.len() == self.capacity {
            self.spans.pop_front();
        }
        self.spans.push_back(span);
        Ok(self.next_index - 1)
    }

    pub fn link_log(
        &mut self,
        span_index: u64,
        record: &JetDevtoolsLogRecord,
    ) -> Result<(), JetDevtoolsTelemetryError> {
        let span = self
            .spans
            .iter_mut()
            .find(|span| span.index == span_index)
            .ok_or(JetDevtoolsTelemetryError::CapacityExceeded("trace span link"))?;
        span.link_log_with_limit(record, self.linked_logs)
    }

    pub fn linked_logs(
        &self,
        span_index: u64,
        logs: &JetDevtoolsLogRing,
    ) -> Vec<JetDevtoolsTraceLogLink> {
        let Some(span) = self.spans.iter().find(|span| span.index == span_index) else {
            return Vec::new();
        };
        span.linked_log_indexes
            .iter()
            .filter_map(|log_index| {
                let record = logs.records.iter().find(|record| record.index == *log_index)?;
                Some(JetDevtoolsTraceLogLink {
                    span_index: span.index,
                    log_index: record.index,
                    trace_id: span.identity.trace_id.clone(),
                    span_id: span.identity.span_id.clone(),
                    span_source_id: span.source_id().to_string(),
                    log_source_id: record.source_id().to_string(),
                    timestamp_ns: record.timestamp_ns,
                    level: record.level,
                    body: record.body.clone(),
                })
            })
            .collect()
    }
}

/// A threshold is itself inspectable metadata.  No slow row is emitted until a
/// closed span's duration meets or exceeds the threshold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsSlowWorkThreshold {
    pub operation: String,
    pub threshold_ns: u64,
}

impl JetDevtoolsSlowWorkThreshold {
    pub fn new(operation: impl Into<String>, threshold_ns: u64) -> Self {
        Self {
            operation: operation.into(),
            threshold_ns,
        }
    }

    pub fn default_for(operation: impl Into<String>) -> Self {
        Self::new(operation, JET_DEVTOOLS_TELEMETRY_DEFAULT_SLOW_WORK_NS)
    }

    pub fn matches(&self, duration_ns: u64) -> bool {
        duration_ns >= self.threshold_ns
    }

    pub fn fact(&self) -> JetDevtoolsSlowWorkThresholdFact {
        JetDevtoolsSlowWorkThresholdFact {
            operation: self.operation.clone(),
            threshold_ns: self.threshold_ns,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetDevtoolsSlowWorkThresholdFact {
    pub operation: String,
    pub threshold_ns: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsSlowWorkRow {
    pub index: u64,
    pub operation: String,
    pub threshold_ns: u64,
    pub duration_ns: u64,
    pub start_ns: u64,
    pub end_ns: u64,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub source: JetDevtoolsSourceIdentity,
    pub request_id: Option<String>,
}

impl JetDevtoolsSlowWorkRow {
    pub fn source_id(&self) -> &str {
        &self.source.source_id
    }

    pub fn payload_json(&self) -> String {
        let mut fields = vec![
            format!("\"index\":{}", self.index),
            format!("\"operation\":{}", jet_devtools_telemetry_json_string(&self.operation)),
            format!("\"threshold_ns\":{}", self.threshold_ns),
            format!("\"duration_ns\":{}", self.duration_ns),
            format!("\"start_ns\":{}", self.start_ns),
            format!("\"end_ns\":{}", self.end_ns),
            format!("\"trace_id\":{}", jet_devtools_telemetry_optional_string_json(self.trace_id.as_deref())),
            format!("\"span_id\":{}", jet_devtools_telemetry_optional_string_json(self.span_id.as_deref())),
            format!("\"source_id\":{}", jet_devtools_telemetry_json_string(self.source_id())),
        ];
        if let Some(request_id) = &self.request_id {
            fields.push(format!("\"request_id\":{}", jet_devtools_telemetry_json_string(request_id)));
        }
        format!("{{{}}}", fields.join(","))
    }
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        JetDevtoolsEvent::from_parts(
            self.start_ns / 1_000_000,
            source,
            "SlowWork",
            self.index.to_string(),
            self.payload_json(),
        )
    }
}

#[derive(Clone, Debug)]
pub struct JetDevtoolsSlowWorkRing {
    capacity: usize,
    rows: std::collections::VecDeque<JetDevtoolsSlowWorkRow>,
    next_index: u64,
}

impl JetDevtoolsSlowWorkRing {
    pub fn new(retention: JetDevtoolsTelemetryRetention) -> Self {
        Self {
            capacity: retention.slow_work_rows,
            rows: std::collections::VecDeque::new(),
            next_index: 1,
        }
    }

    pub fn rows(&self) -> impl Iterator<Item = &JetDevtoolsSlowWorkRow> {
        self.rows.iter()
    }

    pub fn record(
        &mut self,
        span: &JetDevtoolsTraceSpan,
        threshold: &JetDevtoolsSlowWorkThreshold,
    ) -> Result<Option<u64>, JetDevtoolsTelemetryError> {
        span.validate()?;
        jet_devtools_telemetry_require_text(&threshold.operation, "slow-work operation")?;
        if threshold.operation != span.name {
            return Ok(None);
        }
        let Some(end_ns) = span.end_ns else {
            return Ok(None);
        };
        let duration_ns = end_ns - span.start_ns;
        if !threshold.matches(duration_ns) {
            return Ok(None);
        }
        if self.next_index == u64::MAX {
            return Err(JetDevtoolsTelemetryError::SequenceExhausted("slow-work row"));
        }
        let index = self.next_index;
        self.next_index += 1;
        let row = JetDevtoolsSlowWorkRow {
            index,
            operation: threshold.operation.clone(),
            threshold_ns: threshold.threshold_ns,
            duration_ns,
            start_ns: span.start_ns,
            end_ns,
            trace_id: span.identity.trace_id.clone(),
            span_id: span.identity.span_id.clone(),
            source: span.identity.source.clone(),
            request_id: span.identity.request_id.clone(),
        };
        if self.rows.len() == self.capacity {
            self.rows.pop_front();
        }
        self.rows.push_back(row);
        Ok(Some(index))
    }
}

/// The panel state is the one portable joining point for host adapters.  It
/// exposes no second envelope and keeps all signal retention in one fact.
#[derive(Clone, Debug)]
pub struct JetDevtoolsTelemetryPanelState {
    pub retention: JetDevtoolsTelemetryRetention,
    pub metrics: JetDevtoolsMetricStore,
    pub logs: JetDevtoolsLogRing,
    pub traces: JetDevtoolsTraceRing,
    pub slow_work: JetDevtoolsSlowWorkRing,
}

impl JetDevtoolsTelemetryPanelState {
    pub fn new(retention: JetDevtoolsTelemetryRetention) -> Self {
        Self {
            retention,
            metrics: JetDevtoolsMetricStore::new(retention),
            logs: JetDevtoolsLogRing::new(retention),
            traces: JetDevtoolsTraceRing::new(retention),
            slow_work: JetDevtoolsSlowWorkRing::new(retention),
        }
    }

    pub fn record_metric(
        &mut self,
        point: JetDevtoolsMetricPoint,
    ) -> Result<u64, JetDevtoolsTelemetryError> {
        self.metrics.record(point)
    }

    pub fn record_log(
        &mut self,
        record: JetDevtoolsLogRecord,
    ) -> Result<u64, JetDevtoolsTelemetryError> {
        self.logs.append(record)
    }

    pub fn record_span(
        &mut self,
        span: JetDevtoolsTraceSpan,
    ) -> Result<u64, JetDevtoolsTelemetryError> {
        self.traces.append(span)
    }

    pub fn link_log(
        &mut self,
        span_index: u64,
        log_index: u64,
    ) -> Result<(), JetDevtoolsTelemetryError> {
        let record = self
            .logs
            .records
            .iter()
            .find(|record| record.index == log_index)
            .ok_or(JetDevtoolsTelemetryError::CapacityExceeded("log link"))?
            .clone();
        self.traces.link_log(span_index, &record)
    }

    pub fn record_slow_work(
        &mut self,
        span_index: u64,
        threshold: &JetDevtoolsSlowWorkThreshold,
    ) -> Result<Option<u64>, JetDevtoolsTelemetryError> {
        let span = self
            .traces
            .spans
            .iter()
            .find(|span| span.index == span_index)
            .ok_or(JetDevtoolsTelemetryError::CapacityExceeded("slow-work span"))?
            .clone();
        self.slow_work.record(&span, threshold)
    }
    pub fn metric_chart_points(&self, instrument: &str) -> Vec<JetDevtoolsMetricChartPoint> {
        self.metrics.chart_points(instrument)
    }

    pub fn metric_table_rows(&self, instrument: Option<&str>) -> Vec<JetDevtoolsMetricTableRow> {
        self.metrics.table_rows(instrument)
    }

    pub fn query_logs(&self, query: &JetDevtoolsLogQuery) -> Vec<JetDevtoolsLogQueryRow> {
        self.logs.query(query)
    }

    pub fn trace_rows(&self) -> Vec<JetDevtoolsTraceRow> {
        self.traces.rows()
    }

    pub fn trace_log_links(&self, span_index: u64) -> Vec<JetDevtoolsTraceLogLink> {
        self.traces.linked_logs(span_index, &self.logs)
    }

    pub fn slow_work_rows(&self) -> Vec<JetDevtoolsSlowWorkRow> {
        self.slow_work.rows().cloned().collect()
    }
    /// Marshal the retained telemetry facts into the shared Prelude stream.
    /// The ordering is deterministic and each signal keeps its own typed
    /// conversion before entering the canonical envelope.
    pub fn to_protocol_events(
        &self,
        source: impl Into<String>,
    ) -> Result<Vec<JetDevtoolsEvent>, String> {
        let source = source.into();
        let mut events = Vec::new();
        for series in self.metrics.series() {
            events.push(series.instrument.to_protocol_event(source.clone())?);
            for point in series.points() {
                events.push(point.to_protocol_event(source.clone())?);
            }
        }
        for record in self.logs.records() {
            events.push(record.to_protocol_event(source.clone())?);
        }
        for span in self.traces.spans() {
            events.push(span.to_protocol_event(source.clone())?);
            for link in self.traces.linked_logs(span.index, &self.logs) {
                events.push(link.to_protocol_event(source.clone())?);
            }
        }
        for row in self.slow_work.rows() {
            events.push(row.to_protocol_event(source.clone())?);
        }
        Ok(events)
    }

    pub fn protocol_events(
        &self,
        source: impl Into<String>,
    ) -> Result<Vec<JetDevtoolsEvent>, String> {
        self.to_protocol_events(source)
    }
}

fn jet_devtools_telemetry_require_text(
    value: &str,
    field: &'static str,
) -> Result<(), JetDevtoolsTelemetryError> {
    if value.is_empty() {
        Err(JetDevtoolsTelemetryError::EmptyIdentity(field))
    } else if value.len() > JET_DEVTOOLS_TELEMETRY_MAX_TEXT_BYTES {
        Err(JetDevtoolsTelemetryError::TextTooLong(field))
    } else {
        Ok(())
    }
}

fn jet_devtools_telemetry_push_context_json(
    identity: &JetDevtoolsTelemetryContext,
    fields: &mut Vec<String>,
) {
    if let Some(trace_id) = &identity.trace_id {
        fields.push(format!("\"trace_id\":{}", jet_devtools_telemetry_json_string(trace_id)));
    }
    if let Some(span_id) = &identity.span_id {
        fields.push(format!("\"span_id\":{}", jet_devtools_telemetry_json_string(span_id)));
    }
    if let Some(request_id) = &identity.request_id {
        fields.push(format!("\"request_id\":{}", jet_devtools_telemetry_json_string(request_id)));
    }
}

fn jet_devtools_telemetry_metric_value_json(value: &JetDevtoolsMetricValue) -> String {
    match value {
        JetDevtoolsMetricValue::Integer(value) => value.to_string(),
        JetDevtoolsMetricValue::Float(value) => value.to_string(),
    }
}

fn jet_devtools_telemetry_metric_tag_value_json(value: &JetDevtoolsMetricTagValue) -> String {
    match value {
        JetDevtoolsMetricTagValue::Boolean(value) => value.to_string(),
        JetDevtoolsMetricTagValue::Integer(value) => value.to_string(),
        JetDevtoolsMetricTagValue::Text(value) => jet_devtools_telemetry_json_string(value),
    }
}

fn jet_devtools_telemetry_tags_json(tags: &[JetDevtoolsMetricTag]) -> String {
    let values = tags
        .iter()
        .map(|tag| {
            format!(
                "{{\"key\":{},\"value\":{}}}",
                jet_devtools_telemetry_json_string(&tag.key),
                jet_devtools_telemetry_metric_tag_value_json(&tag.value)
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn jet_devtools_telemetry_log_value_json(value: &JetDevtoolsLogValue) -> String {
    match value {
        JetDevtoolsLogValue::Boolean(value) => value.to_string(),
        JetDevtoolsLogValue::Integer(value) => value.to_string(),
        JetDevtoolsLogValue::Float(value) => value.to_string(),
        JetDevtoolsLogValue::Text(value) => jet_devtools_telemetry_json_string(value),
    }
}

fn jet_devtools_telemetry_log_fields_json(fields: &[JetDevtoolsLogField]) -> String {
    let values = fields
        .iter()
        .map(|field| {
            let mut parts = vec![format!(
                "\"key\":{}",
                jet_devtools_telemetry_json_string(&field.key)
            )];
            if let Some(value) = &field.value {
                parts.push(format!("\"value\":{}", jet_devtools_telemetry_log_value_json(value)));
            }
            if field.redacted {
                parts.push("\"redacted\":true".to_string());
            }
            format!("{{{}}}", parts.join(","))
        })
        .collect::<Vec<_>>();
    format!("[{}]", values.join(","))
}

fn jet_devtools_telemetry_optional_string_json(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_string(), jet_devtools_telemetry_json_string)
}

fn jet_devtools_telemetry_u64_array_json(values: &[u64]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn jet_devtools_telemetry_json_string(value: &str) -> String {
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
            '\u{0c}' => out.push_str("\\f"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod jet_devtools_telemetry_panel_tests {
    use super::*;

    fn context() -> JetDevtoolsTelemetryContext {
        JetDevtoolsTelemetryContext::new(
            JetDevtoolsSourceIdentity::new("src/orders", "orders.jet", 12, 4, "orders.list"),
            Some("trace-1".to_string()),
            Some("span-1".to_string()),
            Some("request-1".to_string()),
        )
    }

    #[test]
    fn projections_share_identity_and_omit_unpublished_values() {
        let retention = JetDevtoolsTelemetryRetention::bounded(2, 2, 2, 2, 2, 2);
        let source = context().source.clone();
        let instrument = JetDevtoolsMetricInstrument::new(
            "orders.count",
            JetDevtoolsMetricKind::Counter,
            "items",
            "orders",
            source,
        );
        let mut metrics = JetDevtoolsMetricStore::new(retention);
        metrics.register(instrument).unwrap();
        metrics
            .record(JetDevtoolsMetricPoint::new(
                10,
                "orders.count",
                vec![JetDevtoolsMetricTag::new(
                    "route",
                    JetDevtoolsMetricTagValue::Text("list".to_string()),
                )],
                context(),
            ))
            .unwrap();
        let chart = metrics.chart_points("orders.count");
        let table = metrics.table_rows(Some("orders.count"));
        assert_eq!(chart[0].source_id(), table[0].source_id());
        assert_eq!(chart[0].trace_id(), Some("trace-1"));
        assert!(!chart[0].payload_json().contains("\"value\""));
    }

    #[test]
    fn query_and_trace_links_preserve_shared_context() {
        let retention = JetDevtoolsTelemetryRetention::bounded(1, 1, 4, 4, 2, 2);
        let mut logs = JetDevtoolsLogRing::new(retention);
        let log = JetDevtoolsLogRecord::new(20, JetDevtoolsLogLevel::Warn, context())
            .publish_body("slow orders")
            .with_field(
                JetDevtoolsLogField::new("kind").publish(JetDevtoolsLogValue::text("slow")),
            )
            .with_field(JetDevtoolsLogField::new("secret").redact());
        let log_index = logs.append(log).unwrap();
        let mut span = JetDevtoolsTraceSpan::new(
            "orders.list",
            JetDevtoolsSpanKind::Server,
            10,
            context(),
        )
        .with_parent_span("root-span");
        span.finish(120, JetDevtoolsSpanStatus::Ok).unwrap();
        span.link_log(logs.records().next().unwrap()).unwrap();
        let mut traces = JetDevtoolsTraceRing::new(retention);
        let span_index = traces.append(span).unwrap();
        let trace_row = traces.rows().pop().unwrap();
        assert_eq!(trace_row.parent_span_id.as_deref(), Some("root-span"));
        assert!(trace_row.payload_json().contains("\"parent_span_id\":\"root-span\""));
        let links = traces.linked_logs(span_index, &logs);
        assert_eq!(links[0].trace_id.as_deref(), Some("trace-1"));
        assert_eq!(links[0].span_id.as_deref(), Some("span-1"));
        assert_eq!(links[0].log_index, log_index);
        let rows = logs.query(&JetDevtoolsLogQuery {
            minimum_level: Some(JetDevtoolsLogLevel::Error),
            ..JetDevtoolsLogQuery::default()
        });
        assert!(rows.is_empty());
        let rows = logs.query(&JetDevtoolsLogQuery {
            contains: Some("slow".to_string()),
            ..JetDevtoolsLogQuery::default()
        });
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source_id(), "src/orders");
        let field_rows = logs.query(&JetDevtoolsLogQuery {
            field_key: Some("kind".to_string()),
            field_value: Some(JetDevtoolsLogValue::text("slow")),
            ..JetDevtoolsLogQuery::default()
        });
        assert_eq!(field_rows.len(), 1);
        let payload = field_rows[0].payload_json();
        assert!(payload.contains("\"key\":\"secret\",\"redacted\":true"));
        assert!(!payload.contains("\"key\":\"secret\",\"value\""));
    }

    #[test]
    fn slow_work_is_bounded_and_keeps_ids() {
        let retention = JetDevtoolsTelemetryRetention::bounded(1, 1, 1, 1, 1, 1);
        let mut span = JetDevtoolsTraceSpan::new(
            "orders.list",
            JetDevtoolsSpanKind::Internal,
            10,
            context(),
        );
        span.finish(110, JetDevtoolsSpanStatus::Ok).unwrap();
        let mut slow = JetDevtoolsSlowWorkRing::new(retention);
        let row = slow
            .record(&span, &JetDevtoolsSlowWorkThreshold::new("orders.list", 100))
            .unwrap();
        assert_eq!(row, Some(1));
        let row = &slow.rows().next().unwrap();
        assert_eq!(row.source_id(), "src/orders");
        assert_eq!(row.trace_id.as_deref(), Some("trace-1"));
        assert_eq!(row.span_id.as_deref(), Some("span-1"));
    }
}
