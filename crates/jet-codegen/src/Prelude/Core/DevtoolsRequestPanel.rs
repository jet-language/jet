// D-DX-REQUESTPANEL1 / card #2451: typed request, query, and exception
// observations share one request identity. This fragment is included after
// Prelude/Devtools.rs; it owns only bounded fact state and its projection.
//
// Metadata is safe to retain by default. Bindings, exception messages, source
// lines, query text, and other values stay absent unless an observation site
// uses one of the explicitly named `published_value` constructors.

pub const JET_DEVTOOLS_REQUEST_PANEL_MAX_ROWS: usize = JET_DEVTOOLS_MAX_HISTORY;
pub const JET_DEVTOOLS_REQUEST_PANEL_MAX_ROUTE_INPUTS: usize = 64;
pub const JET_DEVTOOLS_REQUEST_PANEL_MAX_ACCESS_EVENTS: usize = 256;
pub const JET_DEVTOOLS_REQUEST_PANEL_MAX_QUERIES: usize = 256;
pub const JET_DEVTOOLS_REQUEST_PANEL_MAX_EXCEPTIONS: usize = 64;
pub const JET_DEVTOOLS_REQUEST_PANEL_MAX_SOURCE_FRAMES: usize = 64;
pub const JET_DEVTOOLS_REQUEST_PANEL_MAX_LINKS: usize = 128;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum JetDevtoolsRequestPanelFreshness {
    #[default]
    Fresh,
    Stale,
}

impl JetDevtoolsRequestPanelFreshness {
    pub const fn is_fresh(self) -> bool {
        matches!(self, Self::Fresh)
    }

    pub const fn is_stale(self) -> bool {
        matches!(self, Self::Stale)
    }
}

/// A value crossing a request-panel boundary. `Default` is redacted; the
/// only constructor that stores a value names the publication explicitly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JetDevtoolsRequestPanelValue {
    value: Option<JetDevtoolsValue>,
}

impl JetDevtoolsRequestPanelValue {
    /// Publish one selected value. No ordinary fact constructor captures a
    /// binding, query text, message, source line, or payload.
    pub fn published_value(value: JetDevtoolsValue) -> Self {
        Self { value: Some(value) }
    }

    pub fn is_published(&self) -> bool {
        self.value.is_some()
    }

    pub fn value(&self) -> Option<&JetDevtoolsValue> {
        self.value.as_ref()
    }

    fn validate(&self, field: &str) -> Result<(), String> {
        self.value
            .as_ref()
            .map_or(Ok(()), |value| value.validate().map_err(|error| format!("{field}: {error}")))
    }
}

/// One named route parameter or search value. The name is metadata; its value
/// is absent unless the route observation explicitly publishes it.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsRouteInput {
    pub name: String,
    value: JetDevtoolsRequestPanelValue,
}

impl JetDevtoolsRouteInput {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: JetDevtoolsRequestPanelValue::default(),
        }
    }

    pub fn published_value(name: impl Into<String>, value: JetDevtoolsValue) -> Self {
        Self {
            name: name.into(),
            value: JetDevtoolsRequestPanelValue::published_value(value),
        }
    }

    pub fn value(&self) -> Option<&JetDevtoolsValue> {
        self.value.value()
    }

    pub fn is_published(&self) -> bool {
        self.value.is_published()
    }
}

/// Route and request start facts. Response and related observations arrive as
/// separate facts so hosts can ingest an in-flight request in any order.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsRequestFact {
    pub id: String,
    pub method: String,
    pub route: String,
    pub started_at_ms: u64,
    pub freshness: JetDevtoolsRequestPanelFreshness,
    pub route_inputs: Vec<JetDevtoolsRouteInput>,
    payload: JetDevtoolsRequestPanelValue,
}

impl JetDevtoolsRequestFact {
    pub fn new(
        id: impl Into<String>,
        method: impl Into<String>,
        route: impl Into<String>,
        started_at_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            method: method.into(),
            route: route.into(),
            started_at_ms,
            freshness: JetDevtoolsRequestPanelFreshness::Fresh,
            route_inputs: Vec::new(),
            payload: JetDevtoolsRequestPanelValue::default(),
        }
    }

    pub fn with_route_input(mut self, input: JetDevtoolsRouteInput) -> Self {
        self.route_inputs.push(input);
        self
    }

    pub fn with_freshness(mut self, freshness: JetDevtoolsRequestPanelFreshness) -> Self {
        self.freshness = freshness;
        self
    }

    /// Publish a request payload explicitly. The ordinary constructor keeps it
    /// absent, even when a host has access to the request body.
    pub fn with_published_value(mut self, value: JetDevtoolsValue) -> Self {
        self.payload = JetDevtoolsRequestPanelValue::published_value(value);
        self
    }

    pub fn payload(&self) -> Option<&JetDevtoolsValue> {
        self.payload.value()
    }

    pub fn payload_is_published(&self) -> bool {
        self.payload.is_published()
    }
}

/// Response facts correlate status, byte size, and elapsed time to a request.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsResponseFact {
    pub request_id: String,
    pub status: u16,
    pub size_bytes: u64,
    pub duration_ms: u64,
    pub freshness: JetDevtoolsRequestPanelFreshness,
    payload: JetDevtoolsRequestPanelValue,
}

impl JetDevtoolsResponseFact {
    pub fn new(
        request_id: impl Into<String>,
        status: u16,
        size_bytes: u64,
        duration_ms: u64,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            status,
            size_bytes,
            duration_ms,
            freshness: JetDevtoolsRequestPanelFreshness::Fresh,
            payload: JetDevtoolsRequestPanelValue::default(),
        }
    }

    pub fn with_freshness(mut self, freshness: JetDevtoolsRequestPanelFreshness) -> Self {
        self.freshness = freshness;
        self
    }

    pub fn with_published_value(mut self, value: JetDevtoolsValue) -> Self {
        self.payload = JetDevtoolsRequestPanelValue::published_value(value);
        self
    }

    pub fn payload(&self) -> Option<&JetDevtoolsValue> {
        self.payload.value()
    }

    pub fn payload_is_published(&self) -> bool {
        self.payload.is_published()
    }
}

/// One authorization or resource-access observation for a request. The target
/// is an identity, not the accessed value.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsAccessEvent {
    pub id: String,
    pub request_id: String,
    pub operation: String,
    pub target: String,
    pub at_ms: u64,
    pub allowed: bool,
    pub freshness: JetDevtoolsRequestPanelFreshness,
    payload: JetDevtoolsRequestPanelValue,
}

impl JetDevtoolsAccessEvent {
    pub fn new(
        id: impl Into<String>,
        request_id: impl Into<String>,
        operation: impl Into<String>,
        target: impl Into<String>,
        at_ms: u64,
        allowed: bool,
    ) -> Self {
        Self {
            id: id.into(),
            request_id: request_id.into(),
            operation: operation.into(),
            target: target.into(),
            at_ms,
            allowed,
            freshness: JetDevtoolsRequestPanelFreshness::Fresh,
            payload: JetDevtoolsRequestPanelValue::default(),
        }
    }

    pub fn with_freshness(mut self, freshness: JetDevtoolsRequestPanelFreshness) -> Self {
        self.freshness = freshness;
        self
    }

    pub fn with_published_value(mut self, value: JetDevtoolsValue) -> Self {
        self.payload = JetDevtoolsRequestPanelValue::published_value(value);
        self
    }

    pub fn payload(&self) -> Option<&JetDevtoolsValue> {
        self.payload.value()
    }

    pub fn payload_is_published(&self) -> bool {
        self.payload.is_published()
    }
}

/// One SQL/query observation. `source_identity` identifies the Jet source site;
/// query text and bindings stay payload-free unless explicitly published.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsQueryFact {
    pub id: String,
    pub request_id: String,
    pub name: String,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub source_identity: String,
    pub freshness: JetDevtoolsRequestPanelFreshness,
    payload: JetDevtoolsRequestPanelValue,
}

impl JetDevtoolsQueryFact {
    pub fn new(
        id: impl Into<String>,
        request_id: impl Into<String>,
        name: impl Into<String>,
        started_at_ms: u64,
        duration_ms: u64,
        source_identity: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            request_id: request_id.into(),
            name: name.into(),
            started_at_ms,
            duration_ms,
            source_identity: source_identity.into(),
            freshness: JetDevtoolsRequestPanelFreshness::Fresh,
            payload: JetDevtoolsRequestPanelValue::default(),
        }
    }

    pub fn with_freshness(mut self, freshness: JetDevtoolsRequestPanelFreshness) -> Self {
        self.freshness = freshness;
        self
    }

    pub fn with_published_value(mut self, value: JetDevtoolsValue) -> Self {
        self.payload = JetDevtoolsRequestPanelValue::published_value(value);
        self
    }

    pub fn payload(&self) -> Option<&JetDevtoolsValue> {
        self.payload.value()
    }

    pub fn payload_is_published(&self) -> bool {
        self.payload.is_published()
    }
}

/// A source-linked exception for one request. Source frames are copied into
/// the request projection and also remain attached to this exception row.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsExceptionFact {
    pub id: String,
    pub request_id: String,
    pub kind: String,
    pub code: String,
    pub at_ms: u64,
    pub freshness: JetDevtoolsRequestPanelFreshness,
    source_frames: Vec<JetDevtoolsSourceFrame>,
    message: JetDevtoolsRequestPanelValue,
    payload: JetDevtoolsRequestPanelValue,
}

impl JetDevtoolsExceptionFact {
    pub fn new(
        id: impl Into<String>,
        request_id: impl Into<String>,
        kind: impl Into<String>,
        code: impl Into<String>,
        at_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            request_id: request_id.into(),
            kind: kind.into(),
            code: code.into(),
            at_ms,
            freshness: JetDevtoolsRequestPanelFreshness::Fresh,
            source_frames: Vec::new(),
            message: JetDevtoolsRequestPanelValue::default(),
            payload: JetDevtoolsRequestPanelValue::default(),
        }
    }

    pub fn with_source_frame(mut self, frame: JetDevtoolsSourceFrame) -> Self {
        self.source_frames.push(frame);
        self
    }

    pub fn with_freshness(mut self, freshness: JetDevtoolsRequestPanelFreshness) -> Self {
        self.freshness = freshness;
        self
    }

    /// Publish an exception message explicitly. Default exception rows retain
    /// only kind/code/source identity.
    pub fn with_published_message(mut self, value: JetDevtoolsValue) -> Self {
        self.message = JetDevtoolsRequestPanelValue::published_value(value);
        self
    }

    pub fn with_published_value(mut self, value: JetDevtoolsValue) -> Self {
        self.payload = JetDevtoolsRequestPanelValue::published_value(value);
        self
    }

    pub fn source_frames(&self) -> &[JetDevtoolsSourceFrame] {
        &self.source_frames
    }

    pub fn message(&self) -> Option<&JetDevtoolsValue> {
        self.message.value()
    }

    pub fn payload(&self) -> Option<&JetDevtoolsValue> {
        self.payload.value()
    }

    pub fn message_is_published(&self) -> bool {
        self.message.is_published()
    }

    pub fn payload_is_published(&self) -> bool {
        self.payload.is_published()
    }
}

/// A Jet source location. `source_line` is treated as a value and remains
/// absent by default; file/function/line identity stays available for links.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsSourceFrame {
    pub id: String,
    pub request_id: String,
    pub function: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub frame_index: u32,
    pub freshness: JetDevtoolsRequestPanelFreshness,
    source_line: JetDevtoolsRequestPanelValue,
}

impl JetDevtoolsSourceFrame {
    pub fn new(
        id: impl Into<String>,
        request_id: impl Into<String>,
        function: impl Into<String>,
        file: impl Into<String>,
        line: u32,
        column: u32,
        frame_index: u32,
    ) -> Self {
        Self {
            id: id.into(),
            request_id: request_id.into(),
            function: function.into(),
            file: file.into(),
            line,
            column,
            frame_index,
            freshness: JetDevtoolsRequestPanelFreshness::Fresh,
            source_line: JetDevtoolsRequestPanelValue::default(),
        }
    }

    pub fn with_freshness(mut self, freshness: JetDevtoolsRequestPanelFreshness) -> Self {
        self.freshness = freshness;
        self
    }

    pub fn with_published_source_line(mut self, value: JetDevtoolsValue) -> Self {
        self.source_line = JetDevtoolsRequestPanelValue::published_value(value);
        self
    }

    pub fn source_line(&self) -> Option<&JetDevtoolsValue> {
        self.source_line.value()
    }

    pub fn source_line_is_published(&self) -> bool {
        self.source_line.is_published()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsLogLink {
    pub id: String,
    pub request_id: String,
    pub target: String,
    pub at_ms: u64,
    pub freshness: JetDevtoolsRequestPanelFreshness,
}

impl JetDevtoolsLogLink {
    pub fn new(
        id: impl Into<String>,
        request_id: impl Into<String>,
        target: impl Into<String>,
        at_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            request_id: request_id.into(),
            target: target.into(),
            at_ms,
            freshness: JetDevtoolsRequestPanelFreshness::Fresh,
        }
    }

    pub fn with_freshness(mut self, freshness: JetDevtoolsRequestPanelFreshness) -> Self {
        self.freshness = freshness;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsTraceLink {
    pub id: String,
    pub request_id: String,
    pub span_id: String,
    pub target: String,
    pub at_ms: u64,
    pub duration_ms: u64,
    pub freshness: JetDevtoolsRequestPanelFreshness,
}

impl JetDevtoolsTraceLink {
    pub fn new(
        id: impl Into<String>,
        request_id: impl Into<String>,
        span_id: impl Into<String>,
        target: impl Into<String>,
        at_ms: u64,
        duration_ms: u64,
    ) -> Self {
        Self {
            id: id.into(),
            request_id: request_id.into(),
            span_id: span_id.into(),
            target: target.into(),
            at_ms,
            duration_ms,
            freshness: JetDevtoolsRequestPanelFreshness::Fresh,
        }
    }

    pub fn with_freshness(mut self, freshness: JetDevtoolsRequestPanelFreshness) -> Self {
        self.freshness = freshness;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetDevtoolsRequestPanelFact {
    Request(JetDevtoolsRequestFact),
    Response(JetDevtoolsResponseFact),
    Access(JetDevtoolsAccessEvent),
    Query(JetDevtoolsQueryFact),
    Exception(JetDevtoolsExceptionFact),
    SourceFrame(JetDevtoolsSourceFrame),
    LogLink(JetDevtoolsLogLink),
    TraceLink(JetDevtoolsTraceLink),
}

impl JetDevtoolsRequestFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_request_panel_validate_request(self)?;
        JetDevtoolsEvent::from_parts_with_payload(
            self.started_at_ms,
            source,
            "Request",
            self.id.clone(),
            jet_devtools_request_panel_request_fields(self)?,
            jet_devtools_request_panel_request_payload(self)?,
        )
    }
}

impl JetDevtoolsResponseFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_request_panel_validate_response(self)?;
        JetDevtoolsEvent::from_parts_with_payload(
            0,
            source,
            "Response",
            self.request_id.clone(),
            jet_devtools_request_panel_response_fields(self)?,
            jet_devtools_request_panel_payload(&[("value", self.payload())])?,
        )
    }
}

impl JetDevtoolsAccessEvent {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_request_panel_validate_access(self)?;
        JetDevtoolsEvent::from_parts_with_payload(
            self.at_ms,
            source,
            "Access",
            self.id.clone(),
            jet_devtools_request_panel_access_fields(self)?,
            jet_devtools_request_panel_payload(&[("value", self.payload())])?,
        )
    }
}

impl JetDevtoolsQueryFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_request_panel_validate_query(self)?;
        JetDevtoolsEvent::from_parts_with_payload(
            self.started_at_ms,
            source,
            "Query",
            self.id.clone(),
            jet_devtools_request_panel_query_fields(self)?,
            jet_devtools_request_panel_payload(&[("value", self.payload())])?,
        )
    }
}

impl JetDevtoolsExceptionFact {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_request_panel_validate_exception(self)?;
        JetDevtoolsEvent::from_parts_with_payload(
            self.at_ms,
            source,
            "Exception",
            self.id.clone(),
            jet_devtools_request_panel_exception_fields(self)?,
            jet_devtools_request_panel_payload(&[
                ("message", self.message()),
                ("value", self.payload()),
            ])?,
        )
    }
}

impl JetDevtoolsSourceFrame {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_request_panel_validate_source_frame(self)?;
        JetDevtoolsEvent::from_parts_with_payload(
            0,
            source,
            "Frame",
            self.id.clone(),
            jet_devtools_request_panel_source_frame_fields(self)?,
            jet_devtools_request_panel_payload(&[("source_line", self.source_line())])?,
        )
    }
}

impl JetDevtoolsLogLink {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_request_panel_validate_log_link(self)?;
        JetDevtoolsEvent::from_parts(
            self.at_ms,
            source,
            "LogLink",
            self.id.clone(),
            jet_devtools_request_panel_log_link_fields(self)?,
        )
    }
}

impl JetDevtoolsTraceLink {
    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        jet_devtools_request_panel_validate_trace_link(self)?;
        JetDevtoolsEvent::from_parts(
            self.at_ms,
            source,
            "TraceLink",
            self.id.clone(),
            jet_devtools_request_panel_trace_link_fields(self)?,
        )
    }
}

impl JetDevtoolsRequestPanelFact {
    pub fn request_id(&self) -> &str {
        match self {
            Self::Request(fact) => &fact.id,
            Self::Response(fact) => &fact.request_id,
            Self::Access(fact) => &fact.request_id,
            Self::Query(fact) => &fact.request_id,
            Self::Exception(fact) => &fact.request_id,
            Self::SourceFrame(fact) => &fact.request_id,
            Self::LogLink(fact) => &fact.request_id,
            Self::TraceLink(fact) => &fact.request_id,
        }
    }

    pub fn to_protocol_event(
        &self,
        source: impl Into<String>,
    ) -> Result<JetDevtoolsEvent, String> {
        match self {
            Self::Request(fact) => fact.to_protocol_event(source),
            Self::Response(fact) => fact.to_protocol_event(source),
            Self::Access(fact) => fact.to_protocol_event(source),
            Self::Query(fact) => fact.to_protocol_event(source),
            Self::Exception(fact) => fact.to_protocol_event(source),
            Self::SourceFrame(fact) => fact.to_protocol_event(source),
            Self::LogLink(fact) => fact.to_protocol_event(source),
            Self::TraceLink(fact) => fact.to_protocol_event(source),
        }
    }
}

fn jet_devtools_request_panel_request_fields(
    fact: &JetDevtoolsRequestFact,
) -> Result<String, String> {
    let mut fields = vec![
        jet_devtools_request_panel_text_field("id", &fact.id)?,
        jet_devtools_request_panel_text_field("method", &fact.method)?,
        jet_devtools_request_panel_text_field("route", &fact.route)?,
        jet_devtools_request_panel_u64_field("started_at_ms", fact.started_at_ms),
        jet_devtools_request_panel_text_field(
            "freshness",
            jet_devtools_request_panel_freshness(fact.freshness),
        )?,
    ];
    fields.push(format!(
        "\"route_inputs\":{}",
        jet_devtools_request_panel_route_inputs(&fact.route_inputs)?
    ));
    Ok(jet_devtools_request_panel_object(fields))
}

fn jet_devtools_request_panel_response_fields(
    fact: &JetDevtoolsResponseFact,
) -> Result<String, String> {
    Ok(jet_devtools_request_panel_object(vec![
        jet_devtools_request_panel_text_field("request_id", &fact.request_id)?,
        jet_devtools_request_panel_u64_field("status", u64::from(fact.status)),
        jet_devtools_request_panel_u64_field("size_bytes", fact.size_bytes),
        jet_devtools_request_panel_u64_field("duration_ms", fact.duration_ms),
        jet_devtools_request_panel_text_field(
            "freshness",
            jet_devtools_request_panel_freshness(fact.freshness),
        )?,
    ]))
}

fn jet_devtools_request_panel_access_fields(
    fact: &JetDevtoolsAccessEvent,
) -> Result<String, String> {
    Ok(jet_devtools_request_panel_object(vec![
        jet_devtools_request_panel_text_field("id", &fact.id)?,
        jet_devtools_request_panel_text_field("request_id", &fact.request_id)?,
        jet_devtools_request_panel_text_field("operation", &fact.operation)?,
        jet_devtools_request_panel_text_field("target", &fact.target)?,
        jet_devtools_request_panel_u64_field("at_ms", fact.at_ms),
        jet_devtools_request_panel_bool_field("allowed", fact.allowed),
        jet_devtools_request_panel_text_field(
            "freshness",
            jet_devtools_request_panel_freshness(fact.freshness),
        )?,
    ]))
}

fn jet_devtools_request_panel_query_fields(
    fact: &JetDevtoolsQueryFact,
) -> Result<String, String> {
    Ok(jet_devtools_request_panel_object(vec![
        jet_devtools_request_panel_text_field("id", &fact.id)?,
        jet_devtools_request_panel_text_field("request_id", &fact.request_id)?,
        jet_devtools_request_panel_text_field("name", &fact.name)?,
        jet_devtools_request_panel_u64_field("started_at_ms", fact.started_at_ms),
        jet_devtools_request_panel_u64_field("duration_ms", fact.duration_ms),
        jet_devtools_request_panel_text_field("source_identity", &fact.source_identity)?,
        jet_devtools_request_panel_text_field(
            "freshness",
            jet_devtools_request_panel_freshness(fact.freshness),
        )?,
    ]))
}

fn jet_devtools_request_panel_exception_fields(
    fact: &JetDevtoolsExceptionFact,
) -> Result<String, String> {
    let mut fields = vec![
        jet_devtools_request_panel_text_field("id", &fact.id)?,
        jet_devtools_request_panel_text_field("request_id", &fact.request_id)?,
        jet_devtools_request_panel_text_field("kind", &fact.kind)?,
        jet_devtools_request_panel_text_field("code", &fact.code)?,
        jet_devtools_request_panel_u64_field("at_ms", fact.at_ms),
        jet_devtools_request_panel_text_field(
            "freshness",
            jet_devtools_request_panel_freshness(fact.freshness),
        )?,
    ];
    fields.push(format!(
        "\"source_frames\":{}",
        jet_devtools_request_panel_source_frames(&fact.source_frames)?
    ));
    Ok(jet_devtools_request_panel_object(fields))
}

fn jet_devtools_request_panel_source_frame_fields(
    fact: &JetDevtoolsSourceFrame,
) -> Result<String, String> {
    Ok(jet_devtools_request_panel_object(vec![
        jet_devtools_request_panel_text_field("id", &fact.id)?,
        jet_devtools_request_panel_text_field("request_id", &fact.request_id)?,
        jet_devtools_request_panel_text_field("function", &fact.function)?,
        jet_devtools_request_panel_text_field("file", &fact.file)?,
        jet_devtools_request_panel_u64_field("line", u64::from(fact.line)),
        jet_devtools_request_panel_u64_field("column", u64::from(fact.column)),
        jet_devtools_request_panel_u64_field("frame_index", u64::from(fact.frame_index)),
        jet_devtools_request_panel_text_field(
            "freshness",
            jet_devtools_request_panel_freshness(fact.freshness),
        )?,
    ]))
}

fn jet_devtools_request_panel_log_link_fields(
    fact: &JetDevtoolsLogLink,
) -> Result<String, String> {
    Ok(jet_devtools_request_panel_object(vec![
        jet_devtools_request_panel_text_field("id", &fact.id)?,
        jet_devtools_request_panel_text_field("request_id", &fact.request_id)?,
        jet_devtools_request_panel_text_field("target", &fact.target)?,
        jet_devtools_request_panel_u64_field("at_ms", fact.at_ms),
        jet_devtools_request_panel_text_field(
            "freshness",
            jet_devtools_request_panel_freshness(fact.freshness),
        )?,
    ]))
}

fn jet_devtools_request_panel_trace_link_fields(
    fact: &JetDevtoolsTraceLink,
) -> Result<String, String> {
    Ok(jet_devtools_request_panel_object(vec![
        jet_devtools_request_panel_text_field("id", &fact.id)?,
        jet_devtools_request_panel_text_field("request_id", &fact.request_id)?,
        jet_devtools_request_panel_text_field("span_id", &fact.span_id)?,
        jet_devtools_request_panel_text_field("target", &fact.target)?,
        jet_devtools_request_panel_u64_field("at_ms", fact.at_ms),
        jet_devtools_request_panel_u64_field("duration_ms", fact.duration_ms),
        jet_devtools_request_panel_text_field(
            "freshness",
            jet_devtools_request_panel_freshness(fact.freshness),
        )?,
    ]))
}

fn jet_devtools_request_panel_route_inputs(
    inputs: &[JetDevtoolsRouteInput],
) -> Result<String, String> {
    inputs
        .iter()
        .map(|input| {
            jet_devtools_request_panel_text_field("name", &input.name)
                .map(|name| jet_devtools_request_panel_object(vec![name]))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|values| format!("[{}]", values.join(",")))
}

fn jet_devtools_request_panel_request_payload(
    fact: &JetDevtoolsRequestFact,
) -> Result<Option<String>, String> {
    let mut fields = Vec::new();
    if let Some(value) = fact.payload() {
        fields.push(format!(
            "{}:{}",
            jet_devtools_request_panel_json_string("value")?,
            jet_devtools_request_panel_value_json(value)?
        ));
    }
    if let Some(route_inputs) =
        jet_devtools_request_panel_published_route_inputs(&fact.route_inputs)?
    {
        fields.push(format!(
            "{}:{}",
            jet_devtools_request_panel_json_string("route_inputs")?,
            route_inputs
        ));
    }
    Ok((!fields.is_empty()).then(|| jet_devtools_request_panel_object(fields)))
}

fn jet_devtools_request_panel_published_route_inputs(
    inputs: &[JetDevtoolsRouteInput],
) -> Result<Option<String>, String> {
    let mut values = Vec::new();
    for input in inputs {
        let Some(value) = input.value() else {
            continue;
        };
        values.push(jet_devtools_request_panel_object(vec![
            jet_devtools_request_panel_text_field("name", &input.name)?,
            format!(
                "{}:{}",
                jet_devtools_request_panel_json_string("value")?,
                jet_devtools_request_panel_value_json(value)?
            ),
        ]));
    }
    Ok((!values.is_empty()).then(|| format!("[{}]", values.join(","))))
}

fn jet_devtools_request_panel_source_frames(
    frames: &[JetDevtoolsSourceFrame],
) -> Result<String, String> {
    frames
        .iter()
        .map(|frame| {
            jet_devtools_request_panel_source_frame_fields(frame)
                .map(|fields| fields)
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|values| format!("[{}]", values.join(",")))
}

fn jet_devtools_request_panel_payload(
    values: &[(&str, Option<&JetDevtoolsValue>)],
) -> Result<Option<String>, String> {
    let mut fields = Vec::new();
    for (key, value) in values {
        if let Some(value) = value {
            fields.push(format!(
                "{}:{}",
                jet_devtools_request_panel_json_string(key)?,
                jet_devtools_request_panel_value_json(value)?
            ));
        }
    }
    Ok((!fields.is_empty()).then(|| jet_devtools_request_panel_object(fields)))
}

fn jet_devtools_request_panel_value_json(
    value: &JetDevtoolsValue,
) -> Result<String, String> {
    let mut out = String::new();
    render_value(value, &mut out)?;
    Ok(out)
}

fn jet_devtools_request_panel_text_field(
    key: &str,
    value: &str,
) -> Result<String, String> {
    Ok(format!(
        "{}:{}",
        jet_devtools_request_panel_json_string(key)?,
        jet_devtools_request_panel_json_string(value)?
    ))
}

fn jet_devtools_request_panel_u64_field(key: &str, value: u64) -> String {
    format!("\"{key}\":{value}")
}

fn jet_devtools_request_panel_bool_field(key: &str, value: bool) -> String {
    format!("\"{key}\":{}", if value { "true" } else { "false" })
}

fn jet_devtools_request_panel_freshness(
    freshness: JetDevtoolsRequestPanelFreshness,
) -> &'static str {
    if freshness.is_stale() {
        "stale"
    } else {
        "fresh"
    }
}

fn jet_devtools_request_panel_object(fields: Vec<String>) -> String {
    format!("{{{}}}", fields.join(","))
}

fn jet_devtools_request_panel_json_string(value: &str) -> Result<String, String> {
    let mut out = String::new();
    render_text(value, &mut out)?;
    Ok(out)
}

/// One correlated request row. The optional request fact allows response or
/// query observations to arrive before route dispatch has published its row.
#[derive(Clone, Debug, PartialEq)]
pub struct JetDevtoolsRequestPanelProjection {
    pub request_id: String,
    pub request: Option<JetDevtoolsRequestFact>,
    pub response: Option<JetDevtoolsResponseFact>,
    pub access_events: Vec<JetDevtoolsAccessEvent>,
    pub queries: Vec<JetDevtoolsQueryFact>,
    pub exceptions: Vec<JetDevtoolsExceptionFact>,
    pub source_frames: Vec<JetDevtoolsSourceFrame>,
    pub log_links: Vec<JetDevtoolsLogLink>,
    pub trace_links: Vec<JetDevtoolsTraceLink>,
    pub freshness: JetDevtoolsRequestPanelFreshness,
}

#[derive(Clone, Debug, Default)]
struct JetDevtoolsRequestPanelRow {
    request: Option<JetDevtoolsRequestFact>,
    response: Option<JetDevtoolsResponseFact>,
    access_events: Vec<JetDevtoolsAccessEvent>,
    queries: Vec<JetDevtoolsQueryFact>,
    exceptions: Vec<JetDevtoolsExceptionFact>,
    source_frames: Vec<JetDevtoolsSourceFrame>,
    log_links: Vec<JetDevtoolsLogLink>,
    trace_links: Vec<JetDevtoolsTraceLink>,
}

impl JetDevtoolsRequestPanelRow {
    fn mark_stale(&mut self) {
        if let Some(request) = &mut self.request {
            request.freshness = JetDevtoolsRequestPanelFreshness::Stale;
        }
        if let Some(response) = &mut self.response {
            response.freshness = JetDevtoolsRequestPanelFreshness::Stale;
        }
        for event in &mut self.access_events {
            event.freshness = JetDevtoolsRequestPanelFreshness::Stale;
        }
        for query in &mut self.queries {
            query.freshness = JetDevtoolsRequestPanelFreshness::Stale;
        }
        for exception in &mut self.exceptions {
            exception.freshness = JetDevtoolsRequestPanelFreshness::Stale;
            for frame in &mut exception.source_frames {
                frame.freshness = JetDevtoolsRequestPanelFreshness::Stale;
            }
        }
        for frame in &mut self.source_frames {
            frame.freshness = JetDevtoolsRequestPanelFreshness::Stale;
        }
        for link in &mut self.log_links {
            link.freshness = JetDevtoolsRequestPanelFreshness::Stale;
        }
        for link in &mut self.trace_links {
            link.freshness = JetDevtoolsRequestPanelFreshness::Stale;
        }
    }

    fn mark_fresh(&mut self) {
        if let Some(request) = &mut self.request {
            request.freshness = JetDevtoolsRequestPanelFreshness::Fresh;
        }
        if let Some(response) = &mut self.response {
            response.freshness = JetDevtoolsRequestPanelFreshness::Fresh;
        }
        for event in &mut self.access_events {
            event.freshness = JetDevtoolsRequestPanelFreshness::Fresh;
        }
        for query in &mut self.queries {
            query.freshness = JetDevtoolsRequestPanelFreshness::Fresh;
        }
        for exception in &mut self.exceptions {
            exception.freshness = JetDevtoolsRequestPanelFreshness::Fresh;
            for frame in &mut exception.source_frames {
                frame.freshness = JetDevtoolsRequestPanelFreshness::Fresh;
            }
        }
        for frame in &mut self.source_frames {
            frame.freshness = JetDevtoolsRequestPanelFreshness::Fresh;
        }
        for link in &mut self.log_links {
            link.freshness = JetDevtoolsRequestPanelFreshness::Fresh;
        }
        for link in &mut self.trace_links {
            link.freshness = JetDevtoolsRequestPanelFreshness::Fresh;
        }
    }

    fn freshness(&self) -> JetDevtoolsRequestPanelFreshness {
        if self
            .request
            .as_ref()
            .is_some_and(|fact| fact.freshness.is_stale())
            || self
                .response
                .as_ref()
                .is_some_and(|fact| fact.freshness.is_stale())
            || self.access_events.iter().any(|fact| fact.freshness.is_stale())
            || self.queries.iter().any(|fact| fact.freshness.is_stale())
            || self.exceptions.iter().any(|fact| {
                fact.freshness.is_stale()
                    || fact
                        .source_frames
                        .iter()
                        .any(|frame| frame.freshness.is_stale())
            })
            || self.source_frames.iter().any(|fact| fact.freshness.is_stale())
            || self.log_links.iter().any(|fact| fact.freshness.is_stale())
            || self.trace_links.iter().any(|fact| fact.freshness.is_stale())
        {
            JetDevtoolsRequestPanelFreshness::Stale
        } else {
            JetDevtoolsRequestPanelFreshness::Fresh
        }
    }
}

/// Bounded request-keyed state. All related facts share one row identity and
/// every retained collection has a fixed upper bound.
#[derive(Clone, Debug)]
pub struct JetDevtoolsRequestPanelState {
    capacity: usize,
    rows: std::collections::BTreeMap<String, JetDevtoolsRequestPanelRow>,
    order: std::collections::VecDeque<String>,
    selected_request_id: Option<String>,
}

impl Default for JetDevtoolsRequestPanelState {
    fn default() -> Self {
        Self::new()
    }
}

impl JetDevtoolsRequestPanelState {
    pub fn new() -> Self {
        Self::with_capacity(JET_DEVTOOLS_REQUEST_PANEL_MAX_ROWS)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, JET_DEVTOOLS_REQUEST_PANEL_MAX_ROWS),
            rows: std::collections::BTreeMap::new(),
            order: std::collections::VecDeque::new(),
            selected_request_id: None,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn request_ids(&self) -> Vec<String> {
        self.rows.keys().cloned().collect()
    }

    pub fn ingest(&mut self, fact: JetDevtoolsRequestPanelFact) -> Result<(), String> {
        match fact {
            JetDevtoolsRequestPanelFact::Request(fact) => self.ingest_request(fact),
            JetDevtoolsRequestPanelFact::Response(fact) => self.ingest_response(fact),
            JetDevtoolsRequestPanelFact::Access(fact) => self.ingest_access(fact),
            JetDevtoolsRequestPanelFact::Query(fact) => self.ingest_query(fact),
            JetDevtoolsRequestPanelFact::Exception(fact) => self.ingest_exception(fact),
            JetDevtoolsRequestPanelFact::SourceFrame(fact) => self.ingest_source_frame(fact),
            JetDevtoolsRequestPanelFact::LogLink(fact) => self.ingest_log_link(fact),
            JetDevtoolsRequestPanelFact::TraceLink(fact) => self.ingest_trace_link(fact),
        }
    }

    pub fn ingest_request(&mut self, mut fact: JetDevtoolsRequestFact) -> Result<(), String> {
        jet_devtools_request_panel_validate_request(&fact)?;
        fact.route_inputs.sort_by(|left, right| left.name.cmp(&right.name));
        let id = fact.id.clone();
        self.ensure_row(&id).request = Some(fact);
        Ok(())
    }

    pub fn ingest_response(&mut self, fact: JetDevtoolsResponseFact) -> Result<(), String> {
        jet_devtools_request_panel_validate_response(&fact)?;
        let request_id = fact.request_id.clone();
        self.ensure_row(&request_id).response = Some(fact);
        Ok(())
    }

    pub fn ingest_access(&mut self, fact: JetDevtoolsAccessEvent) -> Result<(), String> {
        jet_devtools_request_panel_validate_access(&fact)?;
        let request_id = fact.request_id.clone();
        let row = self.ensure_row(&request_id);
        jet_devtools_request_panel_upsert_access(&mut row.access_events, fact);
        Ok(())
    }

    pub fn ingest_query(&mut self, fact: JetDevtoolsQueryFact) -> Result<(), String> {
        jet_devtools_request_panel_validate_query(&fact)?;
        let request_id = fact.request_id.clone();
        let row = self.ensure_row(&request_id);
        jet_devtools_request_panel_upsert_query(&mut row.queries, fact);
        Ok(())
    }

    pub fn ingest_exception(&mut self, mut fact: JetDevtoolsExceptionFact) -> Result<(), String> {
        jet_devtools_request_panel_validate_exception(&fact)?;
        jet_devtools_request_panel_normalize_frames(&mut fact.source_frames);
        let request_id = fact.request_id.clone();
        let row = self.ensure_row(&request_id);
        jet_devtools_request_panel_upsert_exception(&mut row.exceptions, fact);
        Ok(())
    }

    pub fn ingest_source_frame(&mut self, fact: JetDevtoolsSourceFrame) -> Result<(), String> {
        jet_devtools_request_panel_validate_source_frame(&fact)?;
        let request_id = fact.request_id.clone();
        let row = self.ensure_row(&request_id);
        jet_devtools_request_panel_upsert_source_frame(&mut row.source_frames, fact);
        Ok(())
    }

    pub fn ingest_log_link(&mut self, fact: JetDevtoolsLogLink) -> Result<(), String> {
        jet_devtools_request_panel_validate_log_link(&fact)?;
        let request_id = fact.request_id.clone();
        let row = self.ensure_row(&request_id);
        jet_devtools_request_panel_upsert_log_link(&mut row.log_links, fact);
        Ok(())
    }

    pub fn ingest_trace_link(&mut self, fact: JetDevtoolsTraceLink) -> Result<(), String> {
        jet_devtools_request_panel_validate_trace_link(&fact)?;
        let request_id = fact.request_id.clone();
        let row = self.ensure_row(&request_id);
        jet_devtools_request_panel_upsert_trace_link(&mut row.trace_links, fact);
        Ok(())
    }

    /// Read one deterministic projection by stable request ID.
    pub fn project(&self, request_id: &str) -> Option<JetDevtoolsRequestPanelProjection> {
        let row = self.rows.get(request_id)?;
        let mut source_frames = row.source_frames.clone();
        for exception in &row.exceptions {
            jet_devtools_request_panel_merge_frames(&mut source_frames, &exception.source_frames);
        }
        jet_devtools_request_panel_normalize_frames(&mut source_frames);
        Some(JetDevtoolsRequestPanelProjection {
            request_id: request_id.to_string(),
            request: row.request.clone(),
            response: row.response.clone(),
            access_events: row.access_events.clone(),
            queries: row.queries.clone(),
            exceptions: row.exceptions.clone(),
            source_frames,
            log_links: row.log_links.clone(),
            trace_links: row.trace_links.clone(),
            freshness: row.freshness(),
        })
    }

    /// Select is a read-side projection operation. Selection state itself is
    /// changed only by `set_selection`, matching the shared envelope contract.
    pub fn select(&self, request_id: &str) -> Option<JetDevtoolsRequestPanelProjection> {
        self.project(request_id)
    }

    pub fn project_all(&self) -> Vec<JetDevtoolsRequestPanelProjection> {
        self.rows
            .keys()
            .filter_map(|request_id| self.project(request_id))
            .collect()
    }

    pub fn rows(&self) -> Vec<JetDevtoolsRequestPanelProjection> {
        self.project_all()
    }

    pub fn set_selection(&mut self, request_id: Option<String>) -> bool {
        match request_id {
            None => {
                self.selected_request_id = None;
                true
            }
            Some(request_id) if self.rows.contains_key(&request_id) => {
                self.selected_request_id = Some(request_id);
                true
            }
            Some(_) => false,
        }
    }

    pub fn selected_request_id(&self) -> Option<&str> {
        self.selected_request_id.as_deref()
    }

    pub fn selected(&self) -> Option<JetDevtoolsRequestPanelProjection> {
        self.selected_request_id
            .as_deref()
            .and_then(|request_id| self.project(request_id))
    }

    pub fn freshness(&self, request_id: &str) -> Option<JetDevtoolsRequestPanelFreshness> {
        self.rows.get(request_id).map(JetDevtoolsRequestPanelRow::freshness)
    }

    pub fn mark_stale(&mut self) {
        for row in self.rows.values_mut() {
            row.mark_stale();
        }
    }

    pub fn mark_fresh(&mut self, request_id: &str) -> bool {
        let Some(row) = self.rows.get_mut(request_id) else {
            return false;
        };
        row.mark_fresh();
        true
    }

    /// Marshal every retained request fact through the shared Prelude event
    /// boundary.  The insertion order is preserved for reconnect stability;
    /// each fact still validates and renders itself before publication.
    pub fn to_protocol_events(
        &self,
        source: impl Into<String>,
    ) -> Result<Vec<JetDevtoolsEvent>, String> {
        let source = source.into();
        let mut events = Vec::new();
        for request_id in &self.order {
            let Some(row) = self.rows.get(request_id) else {
                continue;
            };
            if let Some(fact) = &row.request {
                events.push(fact.to_protocol_event(source.clone())?);
            }
            if let Some(fact) = &row.response {
                events.push(fact.to_protocol_event(source.clone())?);
            }
            for fact in &row.access_events {
                events.push(fact.to_protocol_event(source.clone())?);
            }
            for fact in &row.queries {
                events.push(fact.to_protocol_event(source.clone())?);
            }
            for fact in &row.exceptions {
                events.push(fact.to_protocol_event(source.clone())?);
            }
            for fact in &row.source_frames {
                events.push(fact.to_protocol_event(source.clone())?);
            }
            for fact in &row.log_links {
                events.push(fact.to_protocol_event(source.clone())?);
            }
            for fact in &row.trace_links {
                events.push(fact.to_protocol_event(source.clone())?);
            }
        }
        Ok(events)
    }

    pub fn protocol_events(
        &self,
        source: impl Into<String>,
    ) -> Result<Vec<JetDevtoolsEvent>, String> {
        self.to_protocol_events(source)
    }

    fn ensure_row(&mut self, request_id: &str) -> &mut JetDevtoolsRequestPanelRow {
        if !self.rows.contains_key(request_id) {
            if self.rows.len() == self.capacity {
                if let Some(evicted) = self.order.pop_front() {
                    self.rows.remove(&evicted);
                    if self.selected_request_id.as_deref() == Some(evicted.as_str()) {
                        self.selected_request_id = None;
                    }
                }
            }
            self.order.push_back(request_id.to_string());
            self.rows.insert(
                request_id.to_string(),
                JetDevtoolsRequestPanelRow::default(),
            );
        }
        self.rows
            .get_mut(request_id)
            .expect("request panel row inserted before lookup")
    }
}

pub fn jet_devtools_request_panel_ingest(
    state: &mut JetDevtoolsRequestPanelState,
    fact: JetDevtoolsRequestPanelFact,
) -> Result<(), String> {
    state.ingest(fact)
}

pub fn jet_devtools_request_panel_select(
    state: &JetDevtoolsRequestPanelState,
    request_id: &str,
) -> Option<JetDevtoolsRequestPanelProjection> {
    state.select(request_id)
}

pub fn jet_devtools_request_panel_project(
    state: &JetDevtoolsRequestPanelState,
    request_id: &str,
) -> Option<JetDevtoolsRequestPanelProjection> {
    state.project(request_id)
}

fn jet_devtools_request_panel_validate_request(
    fact: &JetDevtoolsRequestFact,
) -> Result<(), String> {
    jet_devtools_request_panel_validate_text(&fact.id, "request id", true)?;
    jet_devtools_request_panel_validate_text(&fact.method, "request method", true)?;
    jet_devtools_request_panel_validate_text(&fact.route, "request route", true)?;
    if fact.route_inputs.len() > JET_DEVTOOLS_REQUEST_PANEL_MAX_ROUTE_INPUTS {
        return Err("jet.devtools.v1 request panel route inputs exceed the Prelude limit".to_string());
    }
    for (index, input) in fact.route_inputs.iter().enumerate() {
        jet_devtools_request_panel_validate_text(&input.name, "route input name", true)?;
        input
            .value
            .validate(&format!("route input {index}"))?;
        if fact.route_inputs[index + 1..]
            .iter()
            .any(|other| other.name == input.name)
        {
            return Err(format!(
                "jet.devtools.v1 request panel contains duplicate route input `{}`",
                input.name
            ));
        }
    }
    fact.payload.validate("request payload")
}

fn jet_devtools_request_panel_validate_response(
    fact: &JetDevtoolsResponseFact,
) -> Result<(), String> {
    jet_devtools_request_panel_validate_text(&fact.request_id, "response request id", true)?;
    fact.payload.validate("response payload")
}

fn jet_devtools_request_panel_validate_access(
    fact: &JetDevtoolsAccessEvent,
) -> Result<(), String> {
    jet_devtools_request_panel_validate_text(&fact.id, "access id", true)?;
    jet_devtools_request_panel_validate_text(&fact.request_id, "access request id", true)?;
    jet_devtools_request_panel_validate_text(&fact.operation, "access operation", true)?;
    jet_devtools_request_panel_validate_text(&fact.target, "access target", true)?;
    fact.payload.validate("access payload")
}

fn jet_devtools_request_panel_validate_query(
    fact: &JetDevtoolsQueryFact,
) -> Result<(), String> {
    jet_devtools_request_panel_validate_text(&fact.id, "query id", true)?;
    jet_devtools_request_panel_validate_text(&fact.request_id, "query request id", true)?;
    jet_devtools_request_panel_validate_text(&fact.name, "query name", true)?;
    jet_devtools_request_panel_validate_text(&fact.source_identity, "query source identity", true)?;
    fact.payload.validate("query payload")
}

fn jet_devtools_request_panel_validate_exception(
    fact: &JetDevtoolsExceptionFact,
) -> Result<(), String> {
    jet_devtools_request_panel_validate_text(&fact.id, "exception id", true)?;
    jet_devtools_request_panel_validate_text(&fact.request_id, "exception request id", true)?;
    jet_devtools_request_panel_validate_text(&fact.kind, "exception kind", true)?;
    jet_devtools_request_panel_validate_text(&fact.code, "exception code", false)?;
    if fact.source_frames.len() > JET_DEVTOOLS_REQUEST_PANEL_MAX_SOURCE_FRAMES {
        return Err("jet.devtools.v1 request panel exception frames exceed the Prelude limit".to_string());
    }
    for frame in &fact.source_frames {
        jet_devtools_request_panel_validate_source_frame(frame)?;
        if frame.request_id != fact.request_id {
            return Err(
                "jet.devtools.v1 request panel exception frame request id does not match"
                    .to_string(),
            );
        }
    }
    fact.message.validate("exception message")?;
    fact.payload.validate("exception payload")
}

fn jet_devtools_request_panel_validate_source_frame(
    fact: &JetDevtoolsSourceFrame,
) -> Result<(), String> {
    jet_devtools_request_panel_validate_text(&fact.id, "source frame id", true)?;
    jet_devtools_request_panel_validate_text(&fact.request_id, "source frame request id", true)?;
    jet_devtools_request_panel_validate_text(&fact.function, "source frame function", false)?;
    jet_devtools_request_panel_validate_text(&fact.file, "source frame file", true)?;
    fact.source_line.validate("source frame line")
}

fn jet_devtools_request_panel_validate_log_link(
    fact: &JetDevtoolsLogLink,
) -> Result<(), String> {
    jet_devtools_request_panel_validate_text(&fact.id, "log link id", true)?;
    jet_devtools_request_panel_validate_text(&fact.request_id, "log link request id", true)?;
    jet_devtools_request_panel_validate_text(&fact.target, "log link target", true)
}

fn jet_devtools_request_panel_validate_trace_link(
    fact: &JetDevtoolsTraceLink,
) -> Result<(), String> {
    jet_devtools_request_panel_validate_text(&fact.id, "trace link id", true)?;
    jet_devtools_request_panel_validate_text(&fact.request_id, "trace link request id", true)?;
    jet_devtools_request_panel_validate_text(&fact.span_id, "trace span id", true)?;
    jet_devtools_request_panel_validate_text(&fact.target, "trace link target", true)
}

fn jet_devtools_request_panel_validate_text(
    value: &str,
    field: &str,
    required: bool,
) -> Result<(), String> {
    if required && value.is_empty() {
        return Err(format!("jet.devtools.v1 request panel {field} is empty"));
    }
    if value.len() > JET_DEVTOOLS_MAX_TEXT_BYTES {
        return Err(format!(
            "jet.devtools.v1 request panel {field} exceeds the Prelude limit"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "jet.devtools.v1 request panel {field} contains a control character"
        ));
    }
    Ok(())
}

fn jet_devtools_request_panel_normalize_frames(frames: &mut Vec<JetDevtoolsSourceFrame>) {
    frames.sort_by(|left, right| {
        left.frame_index
            .cmp(&right.frame_index)
            .then_with(|| left.id.cmp(&right.id))
    });
    frames.dedup_by(|left, right| left.id == right.id);
    frames.truncate(JET_DEVTOOLS_REQUEST_PANEL_MAX_SOURCE_FRAMES);
}

fn jet_devtools_request_panel_merge_frames(
    destination: &mut Vec<JetDevtoolsSourceFrame>,
    incoming: &[JetDevtoolsSourceFrame],
) {
    for frame in incoming {
        if let Some(existing) = destination.iter_mut().find(|item| item.id == frame.id) {
            *existing = frame.clone();
        } else {
            destination.push(frame.clone());
        }
    }
    jet_devtools_request_panel_normalize_frames(destination);
}

fn jet_devtools_request_panel_upsert_access(
    values: &mut Vec<JetDevtoolsAccessEvent>,
    value: JetDevtoolsAccessEvent,
) {
    if let Some(existing) = values.iter_mut().find(|item| item.id == value.id) {
        *existing = value;
    } else {
        values.push(value);
    }
    values.sort_by(|left, right| {
        left.at_ms
            .cmp(&right.at_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    jet_devtools_request_panel_trim_tail(values, JET_DEVTOOLS_REQUEST_PANEL_MAX_ACCESS_EVENTS);
}

fn jet_devtools_request_panel_upsert_query(
    values: &mut Vec<JetDevtoolsQueryFact>,
    value: JetDevtoolsQueryFact,
) {
    if let Some(existing) = values.iter_mut().find(|item| item.id == value.id) {
        *existing = value;
    } else {
        values.push(value);
    }
    values.sort_by(|left, right| {
        left.started_at_ms
            .cmp(&right.started_at_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    jet_devtools_request_panel_trim_tail(values, JET_DEVTOOLS_REQUEST_PANEL_MAX_QUERIES);
}

fn jet_devtools_request_panel_upsert_exception(
    values: &mut Vec<JetDevtoolsExceptionFact>,
    value: JetDevtoolsExceptionFact,
) {
    if let Some(existing) = values.iter_mut().find(|item| item.id == value.id) {
        *existing = value;
    } else {
        values.push(value);
    }
    values.sort_by(|left, right| {
        left.at_ms
            .cmp(&right.at_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    jet_devtools_request_panel_trim_tail(values, JET_DEVTOOLS_REQUEST_PANEL_MAX_EXCEPTIONS);
}

fn jet_devtools_request_panel_upsert_source_frame(
    values: &mut Vec<JetDevtoolsSourceFrame>,
    value: JetDevtoolsSourceFrame,
) {
    if let Some(existing) = values.iter_mut().find(|item| item.id == value.id) {
        *existing = value;
    } else {
        values.push(value);
    }
    jet_devtools_request_panel_normalize_frames(values);
}

fn jet_devtools_request_panel_upsert_log_link(
    values: &mut Vec<JetDevtoolsLogLink>,
    value: JetDevtoolsLogLink,
) {
    if let Some(existing) = values.iter_mut().find(|item| item.id == value.id) {
        *existing = value;
    } else {
        values.push(value);
    }
    values.sort_by(|left, right| {
        left.at_ms
            .cmp(&right.at_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    jet_devtools_request_panel_trim_tail(values, JET_DEVTOOLS_REQUEST_PANEL_MAX_LINKS);
}

fn jet_devtools_request_panel_upsert_trace_link(
    values: &mut Vec<JetDevtoolsTraceLink>,
    value: JetDevtoolsTraceLink,
) {
    if let Some(existing) = values.iter_mut().find(|item| item.id == value.id) {
        *existing = value;
    } else {
        values.push(value);
    }
    values.sort_by(|left, right| {
        left.at_ms
            .cmp(&right.at_ms)
            .then_with(|| left.id.cmp(&right.id))
    });
    jet_devtools_request_panel_trim_tail(values, JET_DEVTOOLS_REQUEST_PANEL_MAX_LINKS);
}

fn jet_devtools_request_panel_trim_tail<T>(values: &mut Vec<T>, limit: usize) {
    if values.len() > limit {
        let remove = values.len() - limit;
        values.drain(..remove);
    }
}
