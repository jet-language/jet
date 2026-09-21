//! D-MCP-SURFACE1=A — the shared MCP protocol core.
//!
//! This module owns protocol values, revision negotiation, revision codecs,
//! and the lifecycle state machine. It has no transport or authorization
//! code. Tooling packages can put any transport on top of [`Peer`] without
//! making a second protocol implementation.

use crate::JSON;
use jet_foundation::DataTree::DataTree;
use std::collections::BTreeMap;

fn field<'a>(object: &'a [(String, DataTree)], key: &str) -> Option<&'a DataTree> {
    object
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn object_from_map(map: BTreeMap<String, DataTree>) -> DataTree {
    DataTree::Object(map.into_iter().collect())
}
use std::fmt;

/// Stable MCP revision selected by default.
pub const STABLE_REVISION: &str = "2025-11-25";
/// Release-candidate revision. It needs explicit policy opt-in.
pub const PREVIEW_REVISION: &str = "2026-07-28";

/// Default maximum encoded MCP message size.
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Hard maximum encoded MCP message size accepted by this core.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;
/// Maximum JSON/schema nesting supported by the shared JSON parser.
pub const MAX_SCHEMA_DEPTH: usize = JSON::MAX_JSON_DEPTH;
/// Default maximum number of capability entries in one initialize message.
pub const DEFAULT_MAX_CAPABILITY_ENTRIES: usize = 64;
/// Hard maximum number of capability entries accepted by this core.
pub const MAX_CAPABILITY_ENTRIES: usize = 256;
/// Maximum peer revision offers accepted during negotiation.
const MAX_PEER_VERSION_COUNT: usize = 32;
/// Maximum wire spelling accepted for one peer revision offer.
const MAX_REVISION_BYTES: usize = 64;
/// Maximum implementation identity field length.
const MAX_IMPLEMENTATION_FIELD_BYTES: usize = 256;


/// MCP protocol revisions known to this SDK.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum McpRevision {
    /// Session-oriented stable MCP revision.
    Stable2025_11_25,
    /// Stateless release-candidate MCP revision.
    Preview2026_07_28,
}

impl McpRevision {
    /// Return the wire spelling of this revision.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stable2025_11_25 => STABLE_REVISION,
            Self::Preview2026_07_28 => PREVIEW_REVISION,
        }
    }

    /// Parse an MCP wire revision. Unknown revisions stay unselected.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            STABLE_REVISION => Some(Self::Stable2025_11_25),
            PREVIEW_REVISION => Some(Self::Preview2026_07_28),
            _ => None,
        }
    }

    /// True for a revision that is not stable yet.
    pub const fn is_preview(self) -> bool {
        matches!(self, Self::Preview2026_07_28)
    }

    /// Return the codec profile owned by this revision.
    pub const fn codec(self) -> McpCodec {
        match self {
            Self::Stable2025_11_25 => McpCodec::Stable2025_11_25,
            Self::Preview2026_07_28 => McpCodec::Preview2026_07_28,
        }
    }

    /// Return the lifecycle law owned by this revision.
    pub const fn lifecycle_profile(self) -> LifecycleProfile {
        match self {
            Self::Stable2025_11_25 => LifecycleProfile::Session,
            Self::Preview2026_07_28 => LifecycleProfile::Stateless,
        }
    }
}

impl fmt::Display for McpRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Local revision selection policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VersionPolicy {
    /// Stable MCP only. This is the default.
    Stable,
    /// One exact revision. Selecting the preview revision is explicit.
    Exact(McpRevision),
    /// Stable plus explicitly listed revisions.
    StableAnd(Vec<McpRevision>),
}

impl Default for VersionPolicy {
    fn default() -> Self {
        Self::Stable
    }
}

impl VersionPolicy {
    fn supported(&self) -> Vec<McpRevision> {
        match self {
            Self::Stable => vec![McpRevision::Stable2025_11_25],
            Self::Exact(revision) => vec![*revision],
            Self::StableAnd(extra) => {
                let mut supported = vec![McpRevision::Stable2025_11_25];
                for revision in extra {
                    if !supported.contains(revision) {
                        supported.push(*revision);
                    }
                }
                supported
            }
        }
    }

    fn admits_preview(&self) -> bool {
        self.supported()
            .iter()
            .any(|revision| revision.is_preview())
    }
}

/// Bounds applied by every MCP codec and structured initialize value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct McpLimits {
    /// Maximum encoded UTF-8 JSON message size.
    pub max_message_bytes: usize,
    /// Maximum JSON nesting depth for params, results, and capabilities.
    pub max_schema_depth: usize,
    /// Maximum capability entries in one initialize value.
    pub max_capability_entries: usize,
}

impl Default for McpLimits {
    fn default() -> Self {
        Self {
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
            max_schema_depth: MAX_SCHEMA_DEPTH,
            max_capability_entries: DEFAULT_MAX_CAPABILITY_ENTRIES,
        }
    }
}

impl McpLimits {
    /// Validate that every configured bound is useful and within the hard cap.
    pub fn validate(self) -> Result<(), CodecError> {
        if self.max_message_bytes == 0 || self.max_message_bytes > MAX_MESSAGE_BYTES {
            return Err(CodecError::InvalidLimit {
                name: "max_message_bytes",
                value: self.max_message_bytes,
                maximum: MAX_MESSAGE_BYTES,
            });
        }
        if self.max_schema_depth == 0 || self.max_schema_depth > MAX_SCHEMA_DEPTH {
            return Err(CodecError::InvalidLimit {
                name: "max_schema_depth",
                value: self.max_schema_depth,
                maximum: MAX_SCHEMA_DEPTH,
            });
        }
        if self.max_capability_entries == 0
            || self.max_capability_entries > MAX_CAPABILITY_ENTRIES
        {
            return Err(CodecError::InvalidLimit {
                name: "max_capability_entries",
                value: self.max_capability_entries,
                maximum: MAX_CAPABILITY_ENTRIES,
            });
        }
        Ok(())
    }
}

/// One local MCP protocol policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpPolicy {
    /// Revisions this peer may negotiate.
    pub versions: VersionPolicy,
    /// Wire and structured-value bounds.
    pub limits: McpLimits,
}

impl Default for McpPolicy {
    fn default() -> Self {
        Self {
            versions: VersionPolicy::Stable,
            limits: McpLimits::default(),
        }
    }
}

impl McpPolicy {
    /// Stable-by-default policy.
    pub fn stable() -> Self {
        Self::default()
    }

    /// Exact revision policy. Preview selection is explicit at this call.
    pub fn exact(revision: McpRevision) -> Self {
        Self {
            versions: VersionPolicy::Exact(revision),
            ..Self::default()
        }
    }

    /// Stable plus explicit additional revisions.
    pub fn stable_and(revisions: Vec<McpRevision>) -> Self {
        Self {
            versions: VersionPolicy::StableAnd(revisions),
            ..Self::default()
        }
    }

    /// Return the revisions offered by this policy in preference order.
    pub fn supported_revisions(&self) -> Vec<McpRevision> {
        self.versions.supported()
    }

    /// Negotiate against wire version strings from the other peer.
    pub fn negotiate(
        &self,
        peer_versions: &[&str],
    ) -> Result<NegotiatedProtocol, NegotiationError> {
        negotiate(self, peer_versions)
    }
}

/// Revision-specific JSON codec profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpCodec {
    /// Codec for [`McpRevision::Stable2025_11_25`].
    Stable2025_11_25,
    /// Separate codec profile for [`McpRevision::Preview2026_07_28`].
    Preview2026_07_28,
}

impl McpCodec {
    /// Return the revision owned by this codec.
    pub const fn revision(self) -> McpRevision {
        match self {
            Self::Stable2025_11_25 => McpRevision::Stable2025_11_25,
            Self::Preview2026_07_28 => McpRevision::Preview2026_07_28,
        }
    }

    /// Encode one protocol message using this revision profile.
    pub fn encode(&self, message: &McpMessage, limits: McpLimits) -> Result<String, CodecError> {
        limits.validate()?;
        let value = message.to_json_value()?;
        let text = stringify(&value, 0, limits.max_schema_depth, limits.max_message_bytes)?;
        if text.len() > limits.max_message_bytes {
            return Err(CodecError::MessageTooLarge {
                actual: text.len(),
                maximum: limits.max_message_bytes,
            });
        }
        Ok(text)
    }

    /// Decode one bounded protocol message using this revision profile.
    pub fn decode(&self, text: &str, limits: McpLimits) -> Result<McpMessage, CodecError> {
        limits.validate()?;
        if text.len() > limits.max_message_bytes {
            return Err(CodecError::MessageTooLarge {
                actual: text.len(),
                maximum: limits.max_message_bytes,
            });
        }
        let value = JSON::parse_json_with_limit(text, limits.max_message_bytes)
            .map_err(|()| CodecError::InvalidJson)?;
        ensure_depth(&value, 0, limits.max_schema_depth)?;
        McpMessage::from_json_value(value, limits)
    }
}

/// JSON-RPC request identifier. Fractional and null IDs are rejected.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RequestId {
    Number(i64),
    String(String),
}

impl RequestId {
    fn to_json_value(&self) -> DataTree {
        match self {
            Self::Number(value) => DataTree::Int(*value),
            Self::String(value) => DataTree::Text(value.clone()),
        }
    }

    fn from_json_value(value: &DataTree) -> Result<Self, CodecError> {
        match value {
            DataTree::Int(value) => Ok(Self::Number(*value)),
            DataTree::Text(value) => Ok(Self::String(value.clone())),
            _ => Err(CodecError::InvalidMessage(
                "request id must be an integer or string".to_string(),
            )),
        }
    }
}

/// JSON-RPC request.
#[derive(Clone, Debug, PartialEq)]
pub struct McpRequest {
    pub id: RequestId,
    pub method: String,
    pub params: Option<DataTree>,
}

impl McpRequest {
    /// Construct a request with an optional object or array params value.
    pub fn new(
        id: RequestId,
        method: impl Into<String>,
        params: Option<DataTree>,
    ) -> Result<Self, CodecError> {
        let request = Self {
            id,
            method: method.into(),
            params,
        };
        request.validate()?;
        Ok(request)
    }

    fn validate(&self) -> Result<(), CodecError> {
        validate_method(&self.method)?;
        validate_params(self.params.as_ref())
    }
}

/// JSON-RPC notification.
#[derive(Clone, Debug, PartialEq)]
pub struct McpNotification {
    pub method: String,
    pub params: Option<DataTree>,
}

impl McpNotification {
    /// Construct a notification with an optional object or array params value.
    pub fn new(
        method: impl Into<String>,
        params: Option<DataTree>,
    ) -> Result<Self, CodecError> {
        let notification = Self {
            method: method.into(),
            params,
        };
        validate_method(&notification.method)?;
        validate_params(notification.params.as_ref())?;
        Ok(notification)
    }
}

/// JSON-RPC protocol error payload.
#[derive(Clone, Debug, PartialEq)]
pub struct McpRpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<DataTree>,
}

impl McpRpcError {
    /// Construct an error payload.
    pub fn new(code: i64, message: impl Into<String>, data: Option<DataTree>) -> Self {
        Self {
            code,
            message: message.into(),
            data,
        }
    }
}

/// JSON-RPC response, carrying exactly one result or error.
#[derive(Clone, Debug, PartialEq)]
pub struct McpResponse {
    /// `None` represents a JSON-RPC null response ID.
    pub id: Option<RequestId>,
    pub result: Option<DataTree>,
    pub error: Option<McpRpcError>,
}

impl McpResponse {
    /// Construct a successful response.
    pub fn success(id: RequestId, result: DataTree) -> Self {
        Self {
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    /// Construct an error response. A null ID is allowed for an unknown ID.
    pub fn failure(id: Option<RequestId>, error: McpRpcError) -> Self {
        Self {
            id,
            result: None,
            error: Some(error),
        }
    }

    fn validate(&self) -> Result<(), CodecError> {
        match (self.result.is_some(), self.error.is_some()) {
            (true, false) | (false, true) => Ok(()),
            _ => Err(CodecError::InvalidMessage(
                "response must contain exactly one result or error".to_string(),
            )),
        }
    }
}

/// One JSON-RPC message in the MCP protocol core.
#[derive(Clone, Debug, PartialEq)]
pub enum McpMessage {
    Request(McpRequest),
    Notification(McpNotification),
    Response(McpResponse),
}

impl McpMessage {
    /// Encode this message through a selected revision codec.
    pub fn encode(&self, codec: McpCodec, limits: McpLimits) -> Result<String, CodecError> {
        codec.encode(self, limits)
    }

    /// Decode this message through a selected revision codec.
    pub fn decode(codec: McpCodec, text: &str, limits: McpLimits) -> Result<Self, CodecError> {
        codec.decode(text, limits)
    }

    fn to_json_value(&self) -> Result<DataTree, CodecError> {
        let mut object = BTreeMap::new();
        object.insert("jsonrpc".to_string(), DataTree::Text("2.0".to_string()));
        match self {
            Self::Request(request) => {
                request.validate()?;
                object.insert("id".to_string(), request.id.to_json_value());
                object.insert(
                    "method".to_string(),
                    DataTree::Text(request.method.clone()),
                );
                if let Some(params) = &request.params {
                    object.insert("params".to_string(), params.clone());
                }
            }
            Self::Notification(notification) => {
                validate_method(&notification.method)?;
                validate_params(notification.params.as_ref())?;
                object.insert(
                    "method".to_string(),
                    DataTree::Text(notification.method.clone()),
                );
                if let Some(params) = &notification.params {
                    object.insert("params".to_string(), params.clone());
                }
            }
            Self::Response(response) => {
                response.validate()?;
                object.insert(
                    "id".to_string(),
                    response
                        .id
                        .as_ref()
                        .map(RequestId::to_json_value)
                        .unwrap_or(DataTree::Null),
                );
                if let Some(result) = &response.result {
                    object.insert("result".to_string(), result.clone());
                }
                if let Some(error) = &response.error {
                    object.insert("error".to_string(), rpc_error_to_json(error));
                }
            }
        }
        Ok(object_from_map(object))
    }

    fn from_json_value(value: DataTree, limits: McpLimits) -> Result<Self, CodecError> {
        limits.validate()?;
        ensure_depth(&value, 0, limits.max_schema_depth)?;
        let object = value
            .as_object()
            .map_err(|_| CodecError::InvalidMessage("message must be a JSON object".to_string()))?;
        if field(object, "jsonrpc").and_then(JSON::json_str) != Some("2.0") {
            return Err(CodecError::InvalidMessage(
                "jsonrpc must be \"2.0\"".to_string(),
            ));
        }
        let method = field(object, "method");
        if method.is_some() && (field(object, "result").is_some() || field(object, "error").is_some())
        {
            return Err(CodecError::InvalidMessage(
                "request or notification cannot contain result or error".to_string(),
            ));
        }
        let params = match field(object, "params") {
            None => None,
            Some(value) => {
                validate_params(Some(value))?;
                Some(value.clone())
            }
        };
        match method {
            Some(DataTree::Text(method)) => {
                validate_method(method)?;
                match field(object, "id") {
                    Some(id) => Ok(Self::Request(McpRequest {
                        id: RequestId::from_json_value(id)?,
                        method: method.clone(),
                        params,
                    })),
                    None => Ok(Self::Notification(McpNotification {
                        method: method.clone(),
                        params,
                    })),
                }
            }
            Some(_) => Err(CodecError::InvalidMessage(
                "method must be a string".to_string(),
            )),
            None => {
                let id = field(object, "id")
                    .ok_or_else(|| CodecError::InvalidMessage("response is missing id".to_string()))?;
                let id = match id {
                    DataTree::Null => None,
                    value => Some(RequestId::from_json_value(value)?),
                };
                let result = field(object, "result").cloned();
                let error = field(object, "error").map(rpc_error_from_json).transpose()?;
                let response = McpResponse { id, result, error };
                response.validate()?;
                if field(object, "params").is_some() {
                    return Err(CodecError::InvalidMessage(
                        "response cannot contain params".to_string(),
                    ));
                }
                if field(object, "result").is_some() && field(object, "error").is_some() {
                    return Err(CodecError::InvalidMessage(
                        "response cannot contain both result and error".to_string(),
                    ));
                }
                Ok(Self::Response(response))
            }
        }
    }
}

/// Capabilities exchanged by initialize. Unknown capability objects are
/// retained so peers can extend the protocol without changing this core.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpCapabilities {
    pub values: BTreeMap<String, DataTree>,
}

impl McpCapabilities {
    /// Empty capabilities.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace one capability entry.
    pub fn insert(&mut self, name: impl Into<String>, value: DataTree) {
        self.values.insert(name.into(), value);
    }

    /// Look up one capability entry.
    pub fn get(&self, name: &str) -> Option<&DataTree> {
        self.values.get(name)
    }

    fn to_json_value(&self, limits: McpLimits) -> Result<DataTree, CodecError> {
        limits.validate()?;
        if self.values.len() > limits.max_capability_entries {
            return Err(CodecError::CapabilityLimitExceeded {
                actual: self.values.len(),
                maximum: limits.max_capability_entries,
            });
        }
        let value = object_from_map(self.values.clone());
        ensure_depth(&value, 0, limits.max_schema_depth)?;
        Ok(value)
    }

    fn from_json_value(value: &DataTree, limits: McpLimits) -> Result<Self, CodecError> {
        limits.validate()?;
        ensure_depth(value, 0, limits.max_schema_depth)?;
        let values = value.as_object().map_err(|_| {
            CodecError::InvalidMessage("capabilities must be a JSON object".to_string())
        })?;
        if values.len() > limits.max_capability_entries {
            return Err(CodecError::CapabilityLimitExceeded {
                actual: values.len(),
                maximum: limits.max_capability_entries,
            });
        }
        Ok(Self {
            values: values.iter().cloned().collect(),
        })
    }
}

/// Identity advertised during initialize.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpImplementation {
    pub name: String,
    pub version: String,
}

impl McpImplementation {
    /// Construct an implementation identity.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Result<Self, CodecError> {
        let implementation = Self {
            name: name.into(),
            version: version.into(),
        };
        implementation.validate()?;
        Ok(implementation)
    }

    fn validate(&self) -> Result<(), CodecError> {
        if self.name.is_empty()
            || self.version.is_empty()
            || self.name.len() > MAX_IMPLEMENTATION_FIELD_BYTES
            || self.version.len() > MAX_IMPLEMENTATION_FIELD_BYTES
            || self.name.chars().any(char::is_control)
            || self.version.chars().any(char::is_control)
        {
            return Err(CodecError::InvalidMessage(
                "implementation name and version must be non-empty bounded text".to_string(),
            ));
        }
        Ok(())
    }

    fn to_json_value(&self) -> Result<DataTree, CodecError> {
        self.validate()?;
        let mut object = BTreeMap::new();
        object.insert("name".to_string(), DataTree::Text(self.name.clone()));
        object.insert(
            "version".to_string(),
            DataTree::Text(self.version.clone()),
        );
        Ok(object_from_map(object))
    }

    fn from_json_value(value: &DataTree) -> Result<Self, CodecError> {
        let object = value.as_object().map_err(|_| {
            CodecError::InvalidMessage("implementation must be a JSON object".to_string())
        })?;
        let name = required_string(object, "name")?;
        let version = required_string(object, "version")?;
        Self::new(name, version)
    }
}

/// Parameters carried by the stable `initialize` request.
#[derive(Clone, Debug, PartialEq)]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: McpCapabilities,
    pub client_info: Option<McpImplementation>,
}

impl InitializeParams {
    /// Construct initialize parameters for a wire revision.
    pub fn new(
        protocol_version: McpRevision,
        capabilities: McpCapabilities,
        client_info: Option<McpImplementation>,
    ) -> Self {
        Self {
            protocol_version: protocol_version.as_str().to_string(),
            capabilities,
            client_info,
        }
    }

    /// Construct a request carrying these parameters.
    pub fn into_request(self, id: RequestId) -> Result<McpRequest, CodecError> {
        McpRequest::new(
            id,
            "initialize",
            Some(self.to_json_value(McpLimits::default())?),
        )
    }

    /// Encode this structured initialize value as JSON.
    pub fn to_json_value(&self, limits: McpLimits) -> Result<DataTree, CodecError> {
        limits.validate()?;
        let protocol_version = McpRevision::parse(&self.protocol_version).ok_or_else(|| {
            CodecError::InvalidMessage("unknown initialize protocol version".to_string())
        })?;
        let mut object = BTreeMap::new();
        object.insert(
            "capabilities".to_string(),
            self.capabilities.to_json_value(limits)?,
        );
        if let Some(client_info) = &self.client_info {
            object.insert("clientInfo".to_string(), client_info.to_json_value()?);
        }
        object.insert(
            "protocolVersion".to_string(),
            DataTree::Text(protocol_version.as_str().to_string()),
        );
        let value = object_from_map(object);
        ensure_depth(&value, 0, limits.max_schema_depth)?;
        Ok(value)
    }

    /// Decode a structured initialize value from JSON.
    pub fn from_json_value(value: &DataTree, limits: McpLimits) -> Result<Self, CodecError> {
        limits.validate()?;
        ensure_depth(value, 0, limits.max_schema_depth)?;
        let object = value.as_object().map_err(|_| {
            CodecError::InvalidMessage("initialize params must be a JSON object".to_string())
        })?;
        let protocol_version = required_string(object, "protocolVersion")?;
        McpRevision::parse(&protocol_version).ok_or_else(|| {
            CodecError::InvalidMessage("unknown initialize protocol version".to_string())
        })?;
        let capabilities = McpCapabilities::from_json_value(
            field(object, "capabilities").ok_or_else(|| {
                CodecError::InvalidMessage("initialize params missing capabilities".to_string())
            })?,
            limits,
        )?;
        let client_info = field(object, "clientInfo")
            .map(McpImplementation::from_json_value)
            .transpose()?;
        Ok(Self {
            protocol_version,
            capabilities,
            client_info,
        })
    }
}

/// Result carried by the stable `initialize` response.
#[derive(Clone, Debug, PartialEq)]
pub struct InitializeResult {
    pub protocol_version: McpRevision,
    pub capabilities: McpCapabilities,
    pub server_info: McpImplementation,
}

impl InitializeResult {
    /// Construct an initialize result for the negotiated revision.
    pub fn new(
        protocol_version: McpRevision,
        capabilities: McpCapabilities,
        server_info: McpImplementation,
    ) -> Self {
        Self {
            protocol_version,
            capabilities,
            server_info,
        }
    }

    /// Construct a response carrying this result.
    pub fn into_response(self, id: RequestId) -> Result<McpResponse, CodecError> {
        Ok(McpResponse::success(
            id,
            self.to_json_value(McpLimits::default())?,
        ))
    }

    /// Encode this structured initialize value as JSON.
    pub fn to_json_value(&self, limits: McpLimits) -> Result<DataTree, CodecError> {
        limits.validate()?;
        let mut object = BTreeMap::new();
        object.insert(
            "capabilities".to_string(),
            self.capabilities.to_json_value(limits)?,
        );
        object.insert(
            "protocolVersion".to_string(),
            DataTree::Text(self.protocol_version.as_str().to_string()),
        );
        object.insert(
            "serverInfo".to_string(),
            self.server_info.to_json_value()?,
        );
        let value = object_from_map(object);
        ensure_depth(&value, 0, limits.max_schema_depth)?;
        Ok(value)
    }

    /// Decode a structured initialize result from JSON.
    pub fn from_json_value(value: &DataTree, limits: McpLimits) -> Result<Self, CodecError> {
        limits.validate()?;
        ensure_depth(value, 0, limits.max_schema_depth)?;
        let object = value.as_object().map_err(|_| {
            CodecError::InvalidMessage("initialize result must be a JSON object".to_string())
        })?;
        let protocol_version = required_string(object, "protocolVersion")?;
        let protocol_version = McpRevision::parse(&protocol_version).ok_or_else(|| {
            CodecError::InvalidMessage("unknown initialize protocol version".to_string())
        })?;
        let capabilities = McpCapabilities::from_json_value(
            field(object, "capabilities").ok_or_else(|| {
                CodecError::InvalidMessage("initialize result missing capabilities".to_string())
            })?,
            limits,
        )?;
        let server_info = McpImplementation::from_json_value(
            field(object, "serverInfo").ok_or_else(|| {
                CodecError::InvalidMessage("initialize result missing serverInfo".to_string())
            })?,
        )?;
        Ok(Self {
            protocol_version,
            capabilities,
            server_info,
        })
    }
}


/// Lifecycle law selected by a negotiated revision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleProfile {
    /// `initialize` → `initialized` → `shutdown` → `exit`.
    Session,
    /// No session handshake; each request is independently usable.
    Stateless,
}

/// Current peer lifecycle stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleState {
    New,
    Negotiated,
    Initializing,
    Ready,
    ShuttingDown,
    Closed,
}

/// Client or server role for audit and lifecycle ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerRole {
    Client,
    Server,
}

/// Result of successful revision negotiation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NegotiatedProtocol {
    pub revision: McpRevision,
    pub codec: McpCodec,
    pub lifecycle: LifecycleProfile,
}

fn validate_peer_versions(peer_versions: &[&str]) -> Result<(), NegotiationError> {
    if peer_versions.len() > MAX_PEER_VERSION_COUNT {
        return Err(NegotiationError::PeerVersionListTooLarge {
            actual: peer_versions.len(),
            maximum: MAX_PEER_VERSION_COUNT,
        });
    }
    if peer_versions
        .iter()
        .any(|version| version.len() > MAX_REVISION_BYTES)
    {
        return Err(NegotiationError::PeerVersionTooLong {
            maximum: MAX_REVISION_BYTES,
        });
    }
    Ok(())
}

fn redacted_peer_versions(peer_versions: &[&str]) -> Vec<String> {
    peer_versions
        .iter()
        .map(|version| {
            McpRevision::parse(version)
                .map(|revision| revision.as_str().to_string())
                .unwrap_or_else(|| "<unknown>".to_string())
        })
        .collect()
}
fn redacted_revision(value: &str) -> String {
    if let Some(revision) = McpRevision::parse(value) {
        revision.as_str().to_string()
    } else if value.is_empty() {
        "<empty>".to_string()
    } else if value.len() > MAX_REVISION_BYTES {
        "<oversized>".to_string()
    } else {
        "<unknown>".to_string()
    }

}

/// Negotiate one revision, preferring stable when both peers offer it.
pub fn negotiate(
    policy: &McpPolicy,
    peer_versions: &[&str],
) -> Result<NegotiatedProtocol, NegotiationError> {
    policy
        .limits
        .validate()
        .map_err(|error| NegotiationError::InvalidPolicy(error.to_string()))?;
    validate_peer_versions(peer_versions)?;
    let peer_revisions: Vec<_> = peer_versions
        .iter()
        .filter_map(|version| McpRevision::parse(version))
        .collect();
    let supported = policy.supported_revisions();
    if let Some(revision) = supported
        .iter()
        .find(|revision| peer_revisions.contains(revision))
        .copied()
    {
        return Ok(NegotiatedProtocol {
            revision,
            codec: revision.codec(),
            lifecycle: revision.lifecycle_profile(),
        });
    }

    let peer_has_preview = peer_revisions.iter().any(|revision| revision.is_preview());
    if peer_has_preview && !policy.versions.admits_preview() {
        return Err(NegotiationError::PreviewRequiresOptIn);
    }
    if let VersionPolicy::Exact(requested) = &policy.versions {
        if requested.is_preview()
            && peer_revisions.contains(&McpRevision::Stable2025_11_25)
        {
            return Err(NegotiationError::DowngradeRefused {
                requested: *requested,
                offered: peer_revisions,
            });
        }
    }
    Err(NegotiationError::NoCommonRevision {
        local: supported,
        peer: redacted_peer_versions(peer_versions),
    })
}

/// Revision negotiation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NegotiationError {
    InvalidPolicy(String),
    PreviewRequiresOptIn,
    PeerVersionListTooLarge {
        actual: usize,
        maximum: usize,
    },
    PeerVersionTooLong {
        maximum: usize,
    },
    DowngradeRefused {
        requested: McpRevision,
        offered: Vec<McpRevision>,
    },
    NoCommonRevision {
        local: Vec<McpRevision>,
        peer: Vec<String>,
    },
    AlreadyNegotiated,
}


impl fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy(message) => write!(f, "invalid MCP policy: {message}"),
            Self::PreviewRequiresOptIn => {
                f.write_str("MCP preview revision requires explicit policy opt-in")
            }
            Self::PeerVersionListTooLarge { actual, maximum } => write!(
                f,
                "MCP peer revision offer count {actual} exceeds maximum {maximum}"
            ),
            Self::PeerVersionTooLong { maximum } => {
                write!(f, "MCP peer revision offer exceeds {maximum} bytes")
            }
            Self::DowngradeRefused { requested, offered } => write!(
                f,
                "MCP downgrade refused for {requested}; peer offered {count} compatible revision(s)",
                count = offered.len()
            ),
            Self::NoCommonRevision { local, peer } => write!(
                f,
                "no common MCP revision ({local_count} local, {peer_count} peer offer(s))",
                local_count = local.len(),
                peer_count = peer.len()
            ),
            Self::AlreadyNegotiated => f.write_str("MCP peer already negotiated"),
        }
    }
}

impl std::error::Error for NegotiationError {}

/// Lifecycle transition failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleError {
    InvalidState {
        operation: &'static str,
        state: LifecycleState,
    },
    NotNegotiated,
    StatelessRevision,
    InvalidInitialize,
    ProtocolVersionMismatch {
        expected: McpRevision,
        received: String,
    },
}


impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidState { operation, state } => {
                write!(f, "cannot {operation} MCP peer in state {state:?}")
            }
            Self::NotNegotiated => f.write_str("MCP peer has no negotiated revision"),
            Self::StatelessRevision => {
                f.write_str("the negotiated MCP revision does not use session lifecycle")
            }
            Self::InvalidInitialize => f.write_str("invalid MCP initialize value"),
            Self::ProtocolVersionMismatch {
                expected,
                received: _,
            } => write!(
                f,
                "initialize protocol version mismatch: expected {expected}"
            ),
        }
    }
}

impl std::error::Error for LifecycleError {}

/// Redacted, structured facts suitable for a dossier or trace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAuditFacts {
    pub role: PeerRole,
    pub negotiated_revision: Option<McpRevision>,
    pub codec: Option<McpCodec>,
    pub lifecycle: LifecycleState,
}

impl McpAuditFacts {
    /// Convert facts to stable, secret-free key/value fields.
    pub fn as_map(&self) -> BTreeMap<String, String> {
        let mut facts = BTreeMap::new();
        facts.insert(
            "mcp.role".to_string(),
            match self.role {
                PeerRole::Client => "client",
                PeerRole::Server => "server",
            }
            .to_string(),
        );
        facts.insert(
            "mcp.lifecycle".to_string(),
            lifecycle_name(self.lifecycle).to_string(),
        );
        if let Some(revision) = self.negotiated_revision {
            facts.insert("mcp.protocol_version".to_string(), revision.to_string());
        }
        if let Some(codec) = self.codec {
            facts.insert("mcp.codec".to_string(), codec.revision().to_string());
        }
        facts
    }
}

/// One negotiated MCP peer. Transports own I/O; this type owns protocol and
/// lifecycle state only.
#[derive(Clone, Debug)]
pub struct Peer {
    role: PeerRole,
    policy: McpPolicy,
    negotiated: Option<NegotiatedProtocol>,
    state: LifecycleState,
}

impl Peer {
    /// Construct a client peer with stable-by-default policy.
    pub fn client(policy: McpPolicy) -> Self {
        Self::new(PeerRole::Client, policy)
    }

    /// Construct a server peer with stable-by-default policy.
    pub fn server(policy: McpPolicy) -> Self {
        Self::new(PeerRole::Server, policy)
    }

    /// Construct a peer for an explicit role and policy.
    pub fn new(role: PeerRole, policy: McpPolicy) -> Self {
        Self {
            role,
            policy,
            negotiated: None,
            state: LifecycleState::New,
        }
    }

    /// Return this peer's role.
    pub const fn role(&self) -> PeerRole {
        self.role
    }

    /// Return the current lifecycle stage.
    pub const fn state(&self) -> LifecycleState {
        self.state
    }

    /// Return this peer's configured protocol bounds.
    pub const fn limits(&self) -> McpLimits {
        self.policy.limits
    }

    /// Return the selected revision, if negotiation completed.
    pub const fn revision(&self) -> Option<McpRevision> {
        match self.negotiated {
            Some(protocol) => Some(protocol.revision),
            None => None,
        }
    }

    /// Return the selected codec, if negotiation completed.
    pub const fn codec(&self) -> Option<McpCodec> {
        match self.negotiated {
            Some(protocol) => Some(protocol.codec),
            None => None,
        }
    }

    /// Return the selected lifecycle profile, if negotiation completed.
    pub const fn lifecycle_profile(&self) -> Option<LifecycleProfile> {
        match self.negotiated {
            Some(protocol) => Some(protocol.lifecycle),
            None => None,
        }
    }

    /// Negotiate once against the other peer's wire version list.
    pub fn negotiate(
        &mut self,
        peer_versions: &[&str],
    ) -> Result<NegotiatedProtocol, NegotiationError> {
        if self.state != LifecycleState::New {
            return Err(NegotiationError::AlreadyNegotiated);
        }
        let negotiated = self.policy.negotiate(peer_versions)?;
        self.state = if negotiated.lifecycle == LifecycleProfile::Stateless {
            LifecycleState::Ready
        } else {
            LifecycleState::Negotiated
        };
        self.negotiated = Some(negotiated);
        Ok(negotiated)
    }

    /// Process stable-session initialize and produce the typed response.
    pub fn initialize(
        &mut self,
        params: &InitializeParams,
        server_info: McpImplementation,
        server_capabilities: McpCapabilities,
    ) -> Result<InitializeResult, LifecycleError> {
        let protocol = self.negotiated.ok_or(LifecycleError::NotNegotiated)?;
        if protocol.lifecycle != LifecycleProfile::Session {
            return Err(LifecycleError::StatelessRevision);
        }
        if self.state != LifecycleState::Negotiated {
            return Err(LifecycleError::InvalidState {
                operation: "initialize",
                state: self.state,
            });
        }
        if params.protocol_version != protocol.revision.as_str() {
            return Err(LifecycleError::ProtocolVersionMismatch {
                expected: protocol.revision,
                received: redacted_revision(&params.protocol_version),
            });
        }
        if params.to_json_value(self.policy.limits).is_err() {
            return Err(LifecycleError::InvalidInitialize);
        }
        let result = InitializeResult::new(
            protocol.revision,
            server_capabilities,
            server_info,
        );
        if result.to_json_value(self.policy.limits).is_err() {
            return Err(LifecycleError::InvalidInitialize);
        }
        self.state = LifecycleState::Initializing;
        Ok(result)
    }

    /// Process the stable `initialized` notification.
    pub fn initialized(&mut self) -> Result<(), LifecycleError> {
        if self.negotiated.is_none() {
            return Err(LifecycleError::NotNegotiated);
        }
        if self.state != LifecycleState::Initializing {
            return Err(LifecycleError::InvalidState {
                operation: "initialized",
                state: self.state,
            });
        }
        self.state = LifecycleState::Ready;
        Ok(())
    }

    /// Process the stable `shutdown` request.
    pub fn shutdown(&mut self) -> Result<(), LifecycleError> {
        let profile = self
            .lifecycle_profile()
            .ok_or(LifecycleError::NotNegotiated)?;
        if profile == LifecycleProfile::Stateless {
            return Err(LifecycleError::StatelessRevision);
        }
        if self.state != LifecycleState::Ready {
            return Err(LifecycleError::InvalidState {
                operation: "shutdown",
                state: self.state,
            });
        }
        self.state = LifecycleState::ShuttingDown;
        Ok(())
    }

    /// Process the stable `exit` notification, or close a stateless peer.
    pub fn exit(&mut self) -> Result<(), LifecycleError> {
        match self.lifecycle_profile() {
            Some(LifecycleProfile::Session) if self.state == LifecycleState::ShuttingDown => {
                self.state = LifecycleState::Closed;
                Ok(())
            }
            Some(LifecycleProfile::Stateless) if self.state == LifecycleState::Ready => {
                self.state = LifecycleState::Closed;
                Ok(())
            }
            Some(_) => Err(LifecycleError::InvalidState {
                operation: "exit",
                state: self.state,
            }),
            None => Err(LifecycleError::NotNegotiated),
        }
    }

    /// Return redacted facts for the current peer state.
    pub fn audit_facts(&self) -> McpAuditFacts {
        McpAuditFacts {
            role: self.role,
            negotiated_revision: self.revision(),
            codec: self.codec(),
            lifecycle: self.state,
        }
    }
}

impl Default for Peer {
    fn default() -> Self {
        Self::server(McpPolicy::default())
    }
}

/// Codec and structured-value failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodecError {
    InvalidJson,
    InvalidMessage(String),
    MessageTooLarge {
        actual: usize,
        maximum: usize,
    },
    SchemaTooDeep {
        maximum: usize,
    },
    CapabilityLimitExceeded {
        actual: usize,
        maximum: usize,
    },
    InvalidLimit {
        name: &'static str,
        value: usize,
        maximum: usize,
    },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson => f.write_str("invalid MCP JSON message"),
            Self::InvalidMessage(message) => f.write_str(message),
            Self::MessageTooLarge { actual, maximum } => {
                write!(f, "MCP message exceeds {maximum} bytes ({actual})")
            }
            Self::SchemaTooDeep { maximum } => {
                write!(f, "MCP value exceeds schema depth limit {maximum}")
            }
            Self::CapabilityLimitExceeded { actual, maximum } => write!(
                f,
                "MCP capability count exceeds {maximum} entries ({actual})"
            ),
            Self::InvalidLimit {
                name,
                value,
                maximum,
            } => write!(f, "MCP limit {name}={value} exceeds maximum {maximum}"),
        }
    }
}

impl std::error::Error for CodecError {}

fn validate_method(method: &str) -> Result<(), CodecError> {
    if method.is_empty() || method.len() > 256 || method.chars().any(|c| c.is_control()) {
        return Err(CodecError::InvalidMessage(
            "method must be 1..=256 non-control bytes".to_string(),
        ));
    }
    Ok(())
}

fn validate_params(params: Option<&DataTree>) -> Result<(), CodecError> {
    if matches!(params, Some(DataTree::Array(_) | DataTree::Object(_)) | None) {
        Ok(())
    } else {
        Err(CodecError::InvalidMessage(
            "params must be an object or array".to_string(),
        ))
    }
}

fn required_string(object: &[(String, DataTree)], name: &str) -> Result<String, CodecError> {
    field(object, name)
        .and_then(JSON::json_str)
        .map(str::to_string)
        .ok_or_else(|| CodecError::InvalidMessage(format!("{name} must be a string")))
}

fn rpc_error_to_json(error: &McpRpcError) -> DataTree {
    let mut object = BTreeMap::new();
    object.insert("code".to_string(), DataTree::Int(error.code));
    object.insert(
        "message".to_string(),
        DataTree::Text(error.message.clone()),
    );
    if let Some(data) = &error.data {
        object.insert("data".to_string(), data.clone());
    }
    object_from_map(object)
}

fn rpc_error_from_json(value: &DataTree) -> Result<McpRpcError, CodecError> {
    let object = value.as_object().map_err(|_| {
        CodecError::InvalidMessage("response error must be a JSON object".to_string())
    })?;
    let code = match field(object, "code") {
        Some(DataTree::Int(code)) => *code,
        _ => {
            return Err(CodecError::InvalidMessage(
                "response error code must be an integer".to_string(),
            ))
        }
    };
    Ok(McpRpcError {
        code,
        message: required_string(object, "message")?,
        data: field(object, "data").cloned(),
    })
}

fn ensure_depth(value: &DataTree, depth: usize, maximum: usize) -> Result<(), CodecError> {
    if depth > maximum {
        return Err(CodecError::SchemaTooDeep { maximum });
    }
    match value {
        DataTree::Array(values) => {
            for value in values {
                ensure_depth(value, depth + 1, maximum)?;
            }
        }
        DataTree::Object(values) => {
            for (_, value) in values {
                ensure_depth(value, depth + 1, maximum)?;
            }
        }
        DataTree::Null
        | DataTree::Bool(_)
        | DataTree::Int(_)
        | DataTree::Float(_)
        | DataTree::Number(_)
        | DataTree::TypedText(_)
        | DataTree::Text(_)
        | DataTree::Bytes(_) => {}
    }
    Ok(())
}

fn append_fragment(
    output: &mut String,
    fragment: &str,
    maximum: usize,
) -> Result<(), CodecError> {
    let actual = output.len().saturating_add(fragment.len());
    if actual > maximum {
        return Err(CodecError::MessageTooLarge { actual, maximum });
    }
    output.push_str(fragment);
    Ok(())
}

fn append_json_string(
    output: &mut String,
    value: &str,
    maximum: usize,
) -> Result<(), CodecError> {
    append_fragment(output, "\"", maximum)?;
    for character in value.chars() {
        let escaped = match character {
            '"' => Some("\\\""),
            '\\' => Some("\\\\"),
            '\n' => Some("\\n"),
            '\r' => Some("\\r"),
            '\t' => Some("\\t"),
            '\u{0008}' => Some("\\b"),
            '\u{000c}' => Some("\\f"),
            character if character.is_control() => {
                let escape = format!("\\u{:04x}", character as u32);
                append_fragment(output, &escape, maximum)?;
                None
            }
            _ => None,
        };
        if let Some(escaped) = escaped {
            append_fragment(output, escaped, maximum)?;
        } else if !character.is_control() {
            let mut encoded = [0; 4];
            append_fragment(
                output,
                character.encode_utf8(&mut encoded),
                maximum,
            )?;
        }
    }
    append_fragment(output, "\"", maximum)
}

fn valid_json_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    if bytes.get(index) == Some(&b'-') {
        index += 1;
    }
    match bytes.get(index) {
        Some(b'0') => {
            index += 1;
            if matches!(bytes.get(index), Some(b'0'..=b'9')) {
                return false;
            }
        }
        Some(b'1'..=b'9') => {
            while matches!(bytes.get(index), Some(b'0'..=b'9')) {
                index += 1;
            }
        }
        _ => return false,
    }
    let mut floating = false;
    if bytes.get(index) == Some(&b'.') {
        floating = true;
        index += 1;
        let start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        floating = true;
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let start = index;
        while matches!(bytes.get(index), Some(b'0'..=b'9')) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    if index != bytes.len() {
        return false;
    }
    if floating {
        value
            .parse::<f64>()
            .map(|number| number.is_finite())
            .unwrap_or(false)
    } else {
        value.parse::<i64>().is_ok()
    }
}

fn stringify(
    value: &DataTree,
    depth: usize,
    maximum_depth: usize,
    maximum_bytes: usize,
) -> Result<String, CodecError> {
    ensure_depth(value, depth, maximum_depth)?;
    let mut output = String::with_capacity(maximum_bytes.min(4096));
    stringify_into(value, &mut output, maximum_bytes)?;
    Ok(output)
}

fn stringify_into(
    value: &DataTree,
    output: &mut String,
    maximum: usize,
) -> Result<(), CodecError> {
    match value {
        DataTree::Null => append_fragment(output, "null", maximum),
        DataTree::Bool(true) => append_fragment(output, "true", maximum),
        DataTree::Bool(false) => append_fragment(output, "false", maximum),
        DataTree::Int(value) => append_fragment(output, &value.to_string(), maximum),
        DataTree::Float(value) if value.is_finite() => {
            append_fragment(output, &value.to_string(), maximum)
        }
        DataTree::Float(_) => Err(CodecError::InvalidMessage(
            "JSON numbers must be finite".to_string(),
        )),
        DataTree::Number(value) => {
            if value.len() > maximum {
                return Err(CodecError::MessageTooLarge {
                    actual: value.len(),
                    maximum,
                });
            }
            if !valid_json_number(value) {
                return Err(CodecError::InvalidMessage(
                    "JSON number is invalid".to_string(),
                ));
            }
            append_fragment(output, value, maximum)
        }
        DataTree::TypedText(value) | DataTree::Text(value) => {
            append_json_string(output, value, maximum)
        }
        DataTree::Bytes(values) => {
            append_fragment(output, "[", maximum)?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    append_fragment(output, ",", maximum)?;
                }
                append_fragment(output, &value.to_string(), maximum)?;
            }
            append_fragment(output, "]", maximum)
        }
        DataTree::Array(values) => {
            append_fragment(output, "[", maximum)?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    append_fragment(output, ",", maximum)?;
                }
                stringify_into(value, output, maximum)?;
            }
            append_fragment(output, "]", maximum)
        }
        DataTree::Object(values) => {
            append_fragment(output, "{", maximum)?;
            for (index, (key, value)) in values.iter().enumerate() {
                if index > 0 {
                    append_fragment(output, ",", maximum)?;
                }
                append_json_string(output, key, maximum)?;
                append_fragment(output, ":", maximum)?;
                stringify_into(value, output, maximum)?;
            }
            append_fragment(output, "}", maximum)
        }
    }
}

fn lifecycle_name(state: LifecycleState) -> &'static str {
    match state {
        LifecycleState::New => "new",
        LifecycleState::Negotiated => "negotiated",
        LifecycleState::Initializing => "initializing",
        LifecycleState::Ready => "ready",
        LifecycleState::ShuttingDown => "shutting_down",
        LifecycleState::Closed => "closed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_peer_negotiates_and_completes_session_lifecycle() {
        let mut peer = Peer::server(McpPolicy::default());
        let selected = peer.negotiate(&[PREVIEW_REVISION, STABLE_REVISION]).unwrap();
        assert_eq!(selected.revision, McpRevision::Stable2025_11_25);
        assert_eq!(peer.state(), LifecycleState::Negotiated);

        let client_info = McpImplementation::new("client", "1").unwrap();
        let params = InitializeParams::new(
            McpRevision::Stable2025_11_25,
            McpCapabilities::new(),
            Some(client_info),
        );
        let server_info = McpImplementation::new("server", "1").unwrap();
        peer.initialize(&params, server_info, McpCapabilities::new())
            .unwrap();
        peer.initialized().unwrap();
        peer.shutdown().unwrap();
        peer.exit().unwrap();
        assert_eq!(peer.state(), LifecycleState::Closed);
        assert_eq!(peer.audit_facts().as_map()["mcp.protocol_version"], STABLE_REVISION);
    }

    #[test]
    fn preview_requires_explicit_opt_in_and_uses_stateless_profile() {
        let mut stable = Peer::default();
        assert_eq!(
            stable.negotiate(&[PREVIEW_REVISION]).unwrap_err(),
            NegotiationError::PreviewRequiresOptIn
        );

        let mut preview = Peer::server(McpPolicy::stable_and(vec![
            McpRevision::Preview2026_07_28,
        ]));
        let selected = preview.negotiate(&[PREVIEW_REVISION]).unwrap();
        assert_eq!(selected.lifecycle, LifecycleProfile::Stateless);
        assert_eq!(preview.state(), LifecycleState::Ready);
        assert_eq!(
            preview
                .initialize(
                    &InitializeParams::new(
                        McpRevision::Preview2026_07_28,
                        McpCapabilities::new(),
                        None,
                    ),
                    McpImplementation::new("server", "1").unwrap(),
                    McpCapabilities::new(),
                )
                .unwrap_err(),
            LifecycleError::StatelessRevision
        );
        preview.exit().unwrap();
    }

    #[test]
    fn codecs_round_trip_messages_and_enforce_bounds() {
        let request = McpMessage::Request(
            McpRequest::new(
                RequestId::Number(7),
                "tools/list",
                Some(DataTree::Object(Vec::new())),
            )
            .unwrap(),
        );
        let codec = McpCodec::Stable2025_11_25;
        let encoded = codec.encode(&request, McpLimits::default()).unwrap();
        assert_eq!(codec.decode(&encoded, McpLimits::default()).unwrap(), request);
        assert!(matches!(
            codec.encode(
                &request,
                McpLimits {
                    max_message_bytes: 1,
                    ..McpLimits::default()
                }
            ),
            Err(CodecError::MessageTooLarge { .. })
        ));
    }
}
