//! Canonical, tier-neutral data-loader lifecycle and identity kernel.
//!
//! This module owns loader policy. Prelude adapters only translate their local
//! carrier types into these facts; they do not own a second lifecycle model.

use std::fmt;

pub const MAX_PUBLIC_TEXT: usize = 4096;
pub const MAX_STATUS_TEXT: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LoaderKind {
    File,
    Url,
    Database,
    Value,
}

impl LoaderKind {
    pub const fn debug_name(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Url => "Url",
            Self::Database => "Database",
            Self::Value => "Value",
        }
    }

    pub const fn is_provider(self) -> bool {
        matches!(self, Self::Url | Self::Database)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Freshness {
    Pending,
    Fresh,
    Stale,
    Error,
    Offline,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum InvalidationCause {
    None,
    Loader,
    Input,
    ArchiveMember,
    Parameters,
    Credential,
    Capability,
    Manual,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ErrorKind {
    InvalidArgument,
    Limit,
    State,
    Bridge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KernelError {
    pub kind: ErrorKind,
    pub operation: String,
    pub reason: String,
}

impl KernelError {
    fn new(kind: ErrorKind, operation: &str, reason: impl Into<String>) -> Self {
        Self {
            kind,
            operation: operation.to_string(),
            reason: bounded_text(&reason.into(), MAX_STATUS_TEXT),
        }
    }
}

impl fmt::Display for KernelError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{}: {}", self.operation, self.reason)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Authority {
    pub scope: String,
    pub revision: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SourceIdentity {
    pub kind: LoaderKind,
    pub locator: String,
    pub member: String,
    pub parameters: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    pub buffer_bytes: i64,
    pub max_depth: i64,
    pub max_item_bytes: i64,
    pub max_total_bytes: Option<i64>,
    pub max_expansion_depth: i64,
    pub max_expansion_bytes: i64,
    pub max_groups: i64,
    pub max_sort_rows: i64,
    pub max_join_rows: i64,
    pub max_output_rows: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub identity: String,
    pub freshness: Freshness,
    pub invalidated_by: InvalidationCause,
    pub error: String,
    pub cleanup: String,
    pub buffered_bytes: i64,
    pub backpressure: bool,
    pub last_good: bool,
}

impl Status {
    pub fn pending() -> Self {
        Self {
            identity: String::new(),
            freshness: Freshness::Pending,
            invalidated_by: InvalidationCause::None,
            error: String::new(),
            cleanup: "none".to_string(),
            buffered_bytes: 0,
            backpressure: false,
            last_good: false,
        }
    }
}

/// Owned carrier used by comptime and adapters that cannot lend mutable fields
/// across a core call. Payload and last_good remain distinct by design.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoaderState {
    pub source: SourceIdentity,
    pub format: String,
    pub authority: Authority,
    pub limits: Limits,
    pub payload: Option<Vec<u8>>,
    pub last_good: Option<Vec<u8>>,
    pub cancelled: bool,
    pub offline: bool,
    pub status: Status,
    pub raw_locator: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotFacts {
    pub snapshot_id: String,
    pub source_id: String,
    pub content_id: String,
    pub schema_id: String,
    pub status: Status,
}

pub fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    const SUFFIX: &str = "...<truncated>";
    if max_bytes <= SUFFIX.len() {
        return SUFFIX[..max_bytes].to_string();
    }
    let prefix_limit = max_bytes - SUFFIX.len();
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let next = index + character.len_utf8();
        if next > prefix_limit {
            break;
        }
        end = next;
    }
    let mut output = String::with_capacity(max_bytes);
    output.push_str(&value[..end]);
    output.push_str(SUFFIX);
    output
}

pub fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "password",
        "passwd",
        "pwd",
        "secret",
        "token",
        "credential",
        "authorization",
        "auth",
        "api_key",
        "apikey",
        "access_key",
        "access-token",
        "signature",
        "cookie",
        "session",
        "private_key",
        "private-key",
        "bearer",
    ]
    .iter()
    .any(|marker| key.contains(marker))
}

pub fn sensitive_value(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    [
        "bearer ",
        "basic ",
        "-----begin",
        "password=",
        "secret=",
        "token=",
        "credential=",
        "authorization=",
    ]
    .iter()
    .any(|marker| value.starts_with(marker))
}

pub fn status_text(value: &str) -> String {
    let lowered = value.to_ascii_lowercase();
    let credential_markers = [
        "password=",
        "password:",
        "passwd=",
        "passwd:",
        "secret=",
        "secret:",
        "token=",
        "token:",
        "credential=",
        "credential:",
        "authorization=",
        "authorization:",
        "api_key=",
        "api_key:",
        "access_key=",
        "access_key:",
        "cookie=",
        "cookie:",
        "session=",
        "session:",
        "bearer ",
        "basic ",
        "-----begin",
    ];
    if credential_markers.iter().any(|marker| lowered.contains(marker)) {
        return "<redacted>".to_string();
    }
    bounded_text(value, MAX_STATUS_TEXT)
}

pub fn public_parameter(parameter: &str) -> String {
    let parameter = parameter.trim();
    let Some((key, value)) = parameter.split_once('=') else {
        return format!("value-{}", digest(parameter.as_bytes()));
    };
    let key = key.trim();
    if key.is_empty() || sensitive_key(key) || sensitive_value(value) {
        if key.is_empty() {
            return format!("value-{}", digest(parameter.as_bytes()));
        }
        return format!("{}=<redacted>", bounded_text(key, MAX_PUBLIC_TEXT));
    }
    bounded_text(parameter, MAX_PUBLIC_TEXT)
}

pub fn public_member(member: &str) -> String {
    bounded_text(member, MAX_PUBLIC_TEXT)
}

pub fn public_locator(locator: &str) -> String {
    let without_fragment = locator.split('#').next().unwrap_or(locator);
    let (base, query) = without_fragment
        .split_once('?')
        .map_or((without_fragment, None), |(base, query)| (base, Some(query)));
    let base = if let Some(scheme) = base.find("://") {
        let authority_start = scheme + 3;
        if let Some(at) = base[authority_start..].find('@') {
            format!("{}{}", &base[..authority_start], &base[authority_start + at + 1..])
        } else {
            base.to_string()
        }
    } else {
        base.to_string()
    };
    let Some(query) = query else {
        return base;
    };
    let public_query = query
        .split('&')
        .map(public_parameter)
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{public_query}")
}

pub fn validate_public_text(
    label: &str,
    value: &str,
    allow_empty: bool,
) -> Result<(), KernelError> {
    let blank = value.trim().is_empty();
    if (blank && !(allow_empty && value.is_empty()))
        || value.chars().any(char::is_control)
        || value.len() > MAX_PUBLIC_TEXT
    {
        return Err(KernelError::new(
            ErrorKind::InvalidArgument,
            "data.loader",
            format!(
                "{label} must be non-empty, contain no control characters, and be at most {MAX_PUBLIC_TEXT} bytes"
            ),
        ));
    }
    Ok(())
}

pub fn authority(scope: &str, revision: &str) -> Result<Authority, KernelError> {
    validate_public_text("authority scope", scope, false)?;
    validate_public_text("authority revision", revision, true)?;
    if scope.contains('=')
        || scope.contains('&')
        || revision.contains('=')
        || revision.contains('&')
        || sensitive_value(scope)
        || sensitive_value(revision)
    {
        return Err(KernelError::new(
            ErrorKind::InvalidArgument,
            "data.loader",
            "authority identity cannot carry credential material",
        ));
    }
    Ok(Authority {
        scope: scope.to_string(),
        revision: revision.to_string(),
    })
}

pub fn source(
    kind: LoaderKind,
    locator: &str,
    member: &str,
    parameters: &[String],
) -> SourceIdentity {
    let parameters = parameters.iter().map(|value| public_parameter(value)).collect();
    let locator = if kind == LoaderKind::Database {
        format!("query-{}", digest(locator.as_bytes()))
    } else {
        public_locator(locator)
    };
    SourceIdentity {
        kind,
        locator,
        member: public_member(member),
        parameters,
    }
}

pub fn validate_limits(limits: &Limits) -> Result<(), KernelError> {
    let invalid = if !(4096..=16_777_216).contains(&limits.buffer_bytes) {
        Some(format!(
            "buffer_bytes {} is outside 4096..16777216",
            limits.buffer_bytes
        ))
    } else if !(1..=4096).contains(&limits.max_depth) {
        Some(format!(
            "max_depth {} is outside 1..4096",
            limits.max_depth
        ))
    } else if !(1..=1_073_741_824).contains(&limits.max_item_bytes) {
        Some(format!(
            "max_item_bytes {} is outside 1..1073741824",
            limits.max_item_bytes
        ))
    } else if limits.max_total_bytes.is_some_and(|value| value < 0) {
        Some(format!(
            "max_total_bytes {} is outside 0..Int.max",
            limits.max_total_bytes.unwrap_or_default()
        ))
    } else if !(0..=256).contains(&limits.max_expansion_depth) {
        Some(format!(
            "max_expansion_depth {} is outside 0..256",
            limits.max_expansion_depth
        ))
    } else if !(0..=1_073_741_824).contains(&limits.max_expansion_bytes) {
        Some(format!(
            "max_expansion_bytes {} is outside 0..1073741824",
            limits.max_expansion_bytes
        ))
    } else {
        None
    };
    if let Some(reason) = invalid {
        return Err(KernelError::new(ErrorKind::Limit, "DataLimits", reason));
    }
    for (name, value) in [
        ("max_groups", limits.max_groups),
        ("max_sort_rows", limits.max_sort_rows),
        ("max_join_rows", limits.max_join_rows),
        ("max_output_rows", limits.max_output_rows),
    ] {
        if value < 1 {
            return Err(KernelError::new(
                ErrorKind::InvalidArgument,
                "DataLimits",
                format!("{name} must be positive"),
            ));
        }
    }
    Ok(())
}

pub fn new_loader(
    kind: LoaderKind,
    locator: String,
    member: String,
    parameters: Vec<String>,
    format: String,
    limits: Limits,
    authority: Authority,
) -> Result<LoaderState, KernelError> {
    validate_limits(&limits)?;
    validate_public_text("source locator", &locator, false)?;
    if !member.is_empty() {
        validate_public_text("archive member", &member, false)?;
    }
    for parameter in &parameters {
        validate_public_text("loader parameter", parameter, true)?;
    }
    let authority = self::authority(&authority.scope, &authority.revision)?;
    Ok(LoaderState {
        source: source(kind, &locator, &member, &parameters),
        format,
        authority,
        limits,
        payload: None,
        last_good: None,
        cancelled: false,
        offline: false,
        status: Status::pending(),
        raw_locator: locator,
    })
}

pub fn validate_loader(state: &LoaderState) -> Result<(), KernelError> {
    validate_public_text("source identity locator", &state.source.locator, false)?;
    if !state.source.member.is_empty() {
        validate_public_text("source identity member", &state.source.member, false)?;
    }
    for parameter in &state.source.parameters {
        validate_public_text("source identity parameter", parameter, true)?;
    }
    if state.source.kind != LoaderKind::File && !state.source.member.is_empty() {
        return Err(KernelError::new(
            ErrorKind::InvalidArgument,
            "data.loader",
            "only file loaders may carry an archive member identity",
        ));
    }
    authority(&state.authority.scope, &state.authority.revision).map(|_| ())
}

pub fn check_payload(limits: &Limits, bytes: usize, operation: &str) -> Result<(), KernelError> {
    let bytes = i64::try_from(bytes).unwrap_or(i64::MAX);
    if bytes > limits.max_item_bytes {
        return Err(KernelError::new(
            ErrorKind::Limit,
            operation,
            format!(
                "payload is {bytes} bytes; max_item_bytes is {}",
                limits.max_item_bytes
            ),
        ));
    }
    if limits.max_total_bytes.is_some_and(|limit| bytes > limit) {
        let limit = limits.max_total_bytes.unwrap_or_default();
        return Err(KernelError::new(
            ErrorKind::Limit,
            operation,
            format!("payload is {bytes} bytes; max_total_bytes is {limit}"),
        ));
    }
    Ok(())
}

pub fn set_authority(
    state: &mut LoaderState,
    scope: &str,
    revision: &str,
) -> Result<(), KernelError> {
    let authority = self::authority(scope, revision)?;
    if state.authority == authority {
        return Ok(());
    }
    state.authority = authority;
    state.status.invalidated_by = InvalidationCause::Capability;
    state.status.freshness = Freshness::Stale;
    state.status.error.clear();
    state.status.buffered_bytes = 0;
    state.status.backpressure = false;
    state.status.cleanup = if state.last_good.is_some() {
        "retained".to_string()
    } else {
        "none".to_string()
    };
    state.status.last_good = state.last_good.is_some();
    if state.source.kind != LoaderKind::Value {
        state.payload = None;
    }
    Ok(())
}

pub fn bind(state: &mut LoaderState, payload: Vec<u8>) -> Result<(), KernelError> {
    if state.cancelled {
        return Err(KernelError::new(
            ErrorKind::State,
            "data.loader.bind",
            "loader was cancelled",
        ));
    }
    validate_loader(state)?;
    if !state.source.kind.is_provider()
        && (state.source.kind != LoaderKind::File || state.source.member.is_empty())
    {
        return Err(KernelError::new(
            ErrorKind::InvalidArgument,
            "data.loader.bind",
            "only URL, database, or archive-member loaders accept provider payloads",
        ));
    }
    check_payload(&state.limits, payload.len(), "data.loader.bind")?;
    let bytes = i64::try_from(payload.len()).unwrap_or(i64::MAX);
    state.payload = Some(payload);
    state.status.freshness = Freshness::Pending;
    state.status.error.clear();
    state.status.cleanup = if state.last_good.is_some() {
        "retained".to_string()
    } else {
        "none".to_string()
    };
    state.status.last_good = state.last_good.is_some();
    state.status.buffered_bytes = bytes;
    state.status.backpressure = bytes > state.limits.buffer_bytes;
    Ok(())
}

pub fn cancel(state: &mut LoaderState) {
    state.cancelled = true;
    if state.source.kind != LoaderKind::Value {
        state.payload = None;
    }
    state.status.freshness = Freshness::Cancelled;
    state.status.error = bounded_text("loader was cancelled", MAX_STATUS_TEXT);
    state.status.cleanup = if state.source.kind == LoaderKind::Value {
        "retained".to_string()
    } else {
        "payload-released".to_string()
    };
    state.status.buffered_bytes = 0;
    state.status.backpressure = false;
    state.status.last_good = state.last_good.is_some();
}

pub fn set_offline(state: &mut LoaderState, enabled: bool) {
    state.offline = enabled;
    if enabled {
        if state.payload.is_none() && state.last_good.is_some() {
            state.status.freshness = Freshness::Offline;
        }
    } else if state.status.freshness == Freshness::Offline {
        state.status.freshness = if state.payload.is_some() {
            Freshness::Pending
        } else if state.last_good.is_some()
            && state.status.invalidated_by == InvalidationCause::None
        {
            Freshness::Fresh
        } else if state.last_good.is_some() {
            Freshness::Stale
        } else {
            Freshness::Pending
        };
    }
}

pub fn invalidate(state: &mut LoaderState, cause: InvalidationCause) {
    state.status.invalidated_by = cause;
    state.status.freshness = Freshness::Stale;
    state.status.error.clear();
    if state.source.kind != LoaderKind::Value {
        state.payload = None;
    }
    state.status.cleanup = if state.last_good.is_some() {
        "retained".to_string()
    } else {
        "none".to_string()
    };
    state.status.buffered_bytes = 0;
    state.status.backpressure = false;
    state.status.last_good = state.last_good.is_some();
}

pub fn needs_refresh(state: &LoaderState) -> bool {
    !matches!(
        state.status.freshness,
        Freshness::Fresh | Freshness::Offline | Freshness::Cancelled
    )
}

pub fn ready(state: &LoaderState) -> bool {
    !state.cancelled && !state.status.backpressure
}

pub fn status(state: &LoaderState) -> Status {
    let mut status = state.status.clone();
    status.identity = bounded_text(&status.identity, MAX_STATUS_TEXT);
    status.error = status_text(&status.error);
    status.cleanup = bounded_text(&status.cleanup, MAX_STATUS_TEXT);
    status.buffered_bytes = status.buffered_bytes.max(0);
    status
}

pub fn payload(state: &LoaderState) -> Result<Option<(Vec<u8>, bool)>, KernelError> {
    if let Some(payload) = &state.payload {
        check_payload(&state.limits, payload.len(), "data.loader")?;
        return Ok(Some((payload.clone(), state.offline)));
    }
    if (state.offline || state.source.kind == LoaderKind::Value) && state.last_good.is_some() {
        let payload = state.last_good.as_ref().expect("checked above");
        check_payload(&state.limits, payload.len(), "data.loader.offline")?;
        return Ok(Some((payload.clone(), state.offline)));
    }
    if state.offline
        && (state.source.kind != LoaderKind::File || !state.source.member.is_empty())
    {
        return Err(KernelError::new(
            ErrorKind::Bridge,
            "data.loader.offline",
            "offline mode has no last-good snapshot",
        ));
    }
    Ok(None)
}

pub fn fail(state: &mut LoaderState, error: &KernelError) {
    let freshness = if state.cancelled {
        Freshness::Cancelled
    } else {
        Freshness::Error
    };
    let last_good = state.last_good.is_some();
    let cleanup = if state.source.kind == LoaderKind::Value {
        "retained"
    } else {
        state.payload = None;
        "payload-released"
    };
    state.status = Status {
        identity: state.status.identity.clone(),
        freshness,
        invalidated_by: state.status.invalidated_by,
        error: status_text(&error.to_string()),
        cleanup: cleanup.to_string(),
        buffered_bytes: 0,
        backpressure: false,
        last_good,
    };
}

pub fn commit_snapshot(
    state: &mut LoaderState,
    payload: Vec<u8>,
    canonical: &[u8],
    schema_id: &str,
    offline: bool,
) -> Result<SnapshotFacts, KernelError> {
    check_payload(&state.limits, canonical.len(), "data.snapshot")?;
    let content_id = digest(canonical);
    let mut source_material = String::new();
    identity_part(&mut source_material, state.source.kind.debug_name());
    identity_part(&mut source_material, &state.source.locator);
    identity_part(&mut source_material, &state.source.member);
    for parameter in &state.source.parameters {
        identity_part(&mut source_material, parameter);
    }
    identity_part(&mut source_material, &state.authority.scope);
    identity_part(&mut source_material, &state.authority.revision);
    let source_id = digest(source_material.as_bytes());
    let mut snapshot_material = String::new();
    identity_part(&mut snapshot_material, &source_id);
    identity_part(&mut snapshot_material, &content_id);
    identity_part(&mut snapshot_material, schema_id);
    identity_part(&mut snapshot_material, &state.format);
    let snapshot_id = digest(snapshot_material.as_bytes());
    let status = Status {
        identity: format!("snapshot-{snapshot_id}"),
        freshness: if offline {
            Freshness::Offline
        } else {
            Freshness::Fresh
        },
        invalidated_by: if offline {
            state.status.invalidated_by
        } else {
            InvalidationCause::None
        },
        error: String::new(),
        cleanup: "retained".to_string(),
        buffered_bytes: 0,
        backpressure: false,
        last_good: true,
    };
    state.last_good = Some(payload);
    state.payload = None;
    state.status = status.clone();
    Ok(SnapshotFacts {
        snapshot_id,
        source_id,
        content_id,
        schema_id: schema_id.to_string(),
        status,
    })
}

pub fn digest(bytes: &[u8]) -> String {
    format!("sha256-{}", crate::SHA256::sha256_hex(bytes))
}

/// Derive the stable identity shared by typed table columns and plot fields.
///
/// The byte key is the existing length-delimited `name:type` spelling and the
/// result keeps the established `column-<16 lowercase hex>` representation.
pub fn column_identity(name: &str, type_name: &str) -> String {
    let key = format!("{}:{}", name.len(), type_name);
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3_u64);
    }
    format!("column-{hash:016x}")
}

pub fn identity_part(output: &mut String, value: &str) {
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push(';');
}
