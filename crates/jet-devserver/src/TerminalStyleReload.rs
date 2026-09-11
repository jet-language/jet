//! Typed terminal-style live reload state for resident `jet dev` sessions.
//!
//! This module owns only style facts.  Widget trees, focus, cursor, and the
//! application model stay with their host, so replacing a style snapshot can
//! never replace application state.  The reload boundary is deliberately
//! independent of `WatchService`: a later host adapter can feed coalesced
//! edits here without teaching the watcher or renderer about style semantics.

use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::{json_escape, parse_json_with_limit};
use jet_foundation::SHA256;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};

/// The protocol boundary used by a future devtools host publication.
pub const TERMINAL_STYLE_PROTOCOL: &str = "jet.devtools.v1";
/// Maximum source bytes retained by one style candidate.
pub const MAX_TERMINAL_STYLE_SOURCE_BYTES: usize = 64 * 1024;
/// Maximum bytes retained by the human-readable part of one error fact.
pub const MAX_TERMINAL_STYLE_ERROR_BYTES: usize = 512;
/// Maximum dependency facts carried by one style snapshot.
pub const MAX_TERMINAL_STYLE_DEPENDENCIES: usize = 256;
/// Maximum semantic role entries in one style document.
pub const MAX_TERMINAL_STYLE_ROLES: usize = 32;
/// Maximum stable identity bytes accepted for a style source or dependency.
pub const MAX_TERMINAL_STYLE_ID_BYTES: usize = 256;
/// Maximum pending style subscribers.
pub const MAX_TERMINAL_STYLE_SUBSCRIBERS: usize = 256;
/// Maximum queued notifications retained for one subscriber.
pub const MAX_TERMINAL_STYLE_NOTIFICATIONS: usize = 256;
/// Maximum bytes accepted for one host `/style` request envelope: the
/// bounded style source plus dependency facts and JSON structure.
pub const MAX_TERMINAL_STYLE_REQUEST_BYTES: usize = 256 * 1024;

/// Stable error categories emitted by the style reload boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalStyleErrorCode {
    SourceTooLarge,
    InvalidSourceIdentity,
    InvalidDependency,
    InvalidFingerprint,
    InvalidStyle,
    TooManyDependencies,
    TooManyRoles,
    RevisionOverflow,
}

impl TerminalStyleErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceTooLarge => "style_source_too_large",
            Self::InvalidSourceIdentity => "style_source_identity",
            Self::InvalidDependency => "style_dependency",
            Self::InvalidFingerprint => "style_fingerprint",
            Self::InvalidStyle => "style_parse",
            Self::TooManyDependencies => "style_dependency_limit",
            Self::TooManyRoles => "style_role_limit",
            Self::RevisionOverflow => "style_revision_overflow",
        }
    }
}

impl fmt::Display for TerminalStyleErrorCode {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(self.as_str())
    }
}

/// A bounded construction failure for typed style facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleFactError {
    pub code: TerminalStyleErrorCode,
    pub message: String,
}

impl TerminalStyleFactError {
    fn new(code: TerminalStyleErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: bounded_text(&message.into(), MAX_TERMINAL_STYLE_ERROR_BYTES),
        }
    }
}

impl fmt::Display for TerminalStyleFactError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(output, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for TerminalStyleFactError {}

/// Terminal color capability selected by the host environment.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum TerminalColorProfile {
    #[default]
    Monochrome,
    Ansi16,
    Ansi256,
    TrueColor,
}

impl TerminalColorProfile {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Monochrome => "monochrome",
            Self::Ansi16 => "ansi16",
            Self::Ansi256 => "ansi256",
            Self::TrueColor => "truecolor",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "monochrome" => Some(Self::Monochrome),
            "ansi16" => Some(Self::Ansi16),
            "ansi256" => Some(Self::Ansi256),
            "truecolor" => Some(Self::TrueColor),
            _ => None,
        }
    }
}

/// Explicit color policy, matching the CLI `--color` control.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub enum TerminalColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

impl TerminalColorMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }

    /// Unknown values use the documented safe default rather than inventing a
    /// second policy ladder.
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "always" => Self::Always,
            "never" => Self::Never,
            _ => Self::Auto,
        }
    }
}

/// Explicit terminal facts used both for rendering and dependency identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalCapabilityFacts {
    pub profile: TerminalColorProfile,
    pub mode: TerminalColorMode,
    pub is_tty: bool,
    pub term_is_dumb: bool,
    pub no_color: bool,
}

impl Default for TerminalCapabilityFacts {
    fn default() -> Self {
        Self::new(
            TerminalColorProfile::Monochrome,
            TerminalColorMode::Auto,
            false,
            true,
            false,
        )
    }
}

impl TerminalCapabilityFacts {
    pub const fn new(
        profile: TerminalColorProfile,
        mode: TerminalColorMode,
        is_tty: bool,
        term_is_dumb: bool,
        no_color: bool,
    ) -> Self {
        Self {
            profile,
            mode,
            is_tty,
            term_is_dumb,
            no_color,
        }
    }

    /// Derive a deterministic capability profile from explicit environment
    /// facts.  No process environment is read here, keeping reload tests and
    /// host adapters deterministic.
    pub fn from_environment(
        term: Option<&str>,
        colorterm: Option<&str>,
        is_tty: bool,
        no_color: bool,
        mode: TerminalColorMode,
    ) -> Self {
        let term = term.unwrap_or("").trim().to_ascii_lowercase();
        let colorterm = colorterm.unwrap_or("").trim().to_ascii_lowercase();
        let term_is_dumb = term == "dumb";
        let enabled = match mode {
            TerminalColorMode::Always => true,
            TerminalColorMode::Never => false,
            TerminalColorMode::Auto => is_tty && !term_is_dumb && !no_color,
        };
        let profile = if !enabled {
            TerminalColorProfile::Monochrome
        } else if matches!(colorterm.as_str(), "truecolor" | "24bit") {
            TerminalColorProfile::TrueColor
        } else if term.contains("256color") {
            TerminalColorProfile::Ansi256
        } else {
            TerminalColorProfile::Ansi16
        };
        Self::new(profile, mode, is_tty, term_is_dumb, no_color)
    }

    pub const fn color_enabled(self) -> bool {
        match self.mode {
            TerminalColorMode::Always => true,
            TerminalColorMode::Never => false,
            TerminalColorMode::Auto => {
                self.is_tty && !self.term_is_dumb && !self.no_color
            }
        }
    }

    /// Stable identity for capability-dependent style selection.
    pub fn fingerprint(self) -> String {
        let canonical = format!(
            "terminal-capabilities-v1\0profile={};mode={};tty={};dumb={};no_color={};",
            self.profile.as_str(),
            self.mode.as_str(),
            self.is_tty as u8,
            self.term_is_dumb as u8,
            self.no_color as u8,
        );
        SHA256::sha256_hex(canonical.as_bytes())
    }
}

/// The typed kinds of inputs that can affect style replacement.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TerminalStyleDependencyKind {
    Source,
    Capability,
    Layout,
    ValueType,
}

impl TerminalStyleDependencyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Capability => "capability",
            Self::Layout => "layout",
            Self::ValueType => "value_type",
        }
    }
}

/// One stable, content-addressed style dependency fact.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TerminalStyleDependency {
    pub kind: TerminalStyleDependencyKind,
    pub id: String,
    pub fingerprint: String,
}

impl TerminalStyleDependency {
    /// Construct a dependency from an already computed lowercase SHA-256.
    pub fn new(
        kind: TerminalStyleDependencyKind,
        id: impl Into<String>,
        fingerprint: impl Into<String>,
    ) -> Result<Self, TerminalStyleFactError> {
        let id = id.into();
        let fingerprint = fingerprint.into();
        validate_identity(&id).map_err(|message| {
            TerminalStyleFactError::new(TerminalStyleErrorCode::InvalidDependency, message)
        })?;
        validate_sha256(&fingerprint).map_err(|message| {
            TerminalStyleFactError::new(TerminalStyleErrorCode::InvalidFingerprint, message)
        })?;
        Ok(Self {
            kind,
            id,
            fingerprint,
        })
    }

    /// Compute the dependency identity from bytes at the host boundary.
    pub fn from_bytes(
        kind: TerminalStyleDependencyKind,
        id: impl Into<String>,
        bytes: &[u8],
    ) -> Result<Self, TerminalStyleFactError> {
        Self::new(kind, id, SHA256::sha256_hex(bytes))
    }
}

/// All typed inputs that contribute to one style snapshot identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleFingerprintFacts {
    pub source_id: String,
    pub source_fingerprint: String,
    pub dependencies: Vec<TerminalStyleDependency>,
    pub capabilities: TerminalCapabilityFacts,
    pub layout_fingerprint: String,
    pub type_fingerprint: String,
    pub fingerprint: String,
}

impl TerminalStyleFingerprintFacts {
    pub fn new(
        source_id: impl Into<String>,
        source: &str,
        mut dependencies: Vec<TerminalStyleDependency>,
        capabilities: TerminalCapabilityFacts,
        layout_fingerprint: impl Into<String>,
        type_fingerprint: impl Into<String>,
    ) -> Result<Self, TerminalStyleFactError> {
        let source_id = source_id.into();
        if let Err(message) = validate_identity(&source_id) {
            return Err(TerminalStyleFactError::new(
                TerminalStyleErrorCode::InvalidSourceIdentity,
                message,
            ));
        }
        if source.len() > MAX_TERMINAL_STYLE_SOURCE_BYTES {
            return Err(TerminalStyleFactError::new(
                TerminalStyleErrorCode::SourceTooLarge,
                format!(
                    "style source exceeds {} bytes",
                    MAX_TERMINAL_STYLE_SOURCE_BYTES
                ),
            ));
        }
        if dependencies.len() > MAX_TERMINAL_STYLE_DEPENDENCIES {
            return Err(TerminalStyleFactError::new(
                TerminalStyleErrorCode::TooManyDependencies,
                format!(
                    "style has {} dependencies; limit is {}",
                    dependencies.len(),
                    MAX_TERMINAL_STYLE_DEPENDENCIES
                ),
            ));
        }
        dependencies.sort();
        for pair in dependencies.windows(2) {
            if pair[0].kind == pair[1].kind && pair[0].id == pair[1].id {
                return Err(TerminalStyleFactError::new(
                    TerminalStyleErrorCode::InvalidDependency,
                    format!("duplicate {} dependency identity", pair[0].id),
                ));
            }
        }
        for dependency in &dependencies {
            validate_identity(&dependency.id).map_err(|message| {
                TerminalStyleFactError::new(TerminalStyleErrorCode::InvalidDependency, message)
            })?;
            validate_sha256(&dependency.fingerprint).map_err(|message| {
                TerminalStyleFactError::new(TerminalStyleErrorCode::InvalidFingerprint, message)
            })?;
        }
        let layout_fingerprint = validate_optional_fingerprint(
            layout_fingerprint.into(),
            "layout fingerprint",
        )?;
        let type_fingerprint = validate_optional_fingerprint(
            type_fingerprint.into(),
            "value-type fingerprint",
        )?;
        let source_fingerprint = SHA256::sha256_hex(source.as_bytes());
        let fingerprint = facts_fingerprint(
            &source_id,
            &source_fingerprint,
            &dependencies,
            capabilities,
            &layout_fingerprint,
            &type_fingerprint,
        );
        Ok(Self {
            source_id,
            source_fingerprint,
            dependencies,
            capabilities,
            layout_fingerprint,
            type_fingerprint,
            fingerprint,
        })
    }

    pub fn dependency(
        &self,
        kind: TerminalStyleDependencyKind,
        id: &str,
    ) -> Option<&TerminalStyleDependency> {
        self.dependencies
            .iter()
            .find(|dependency| dependency.kind == kind && dependency.id == id)
    }
}

fn facts_fingerprint(
    source_id: &str,
    source_fingerprint: &str,
    dependencies: &[TerminalStyleDependency],
    capabilities: TerminalCapabilityFacts,
    layout_fingerprint: &str,
    type_fingerprint: &str,
) -> String {
    let mut canonical = String::from("terminal-style-facts-v1\0");
    frame(&mut canonical, source_id);
    frame(&mut canonical, source_fingerprint);
    frame(&mut canonical, &capabilities.fingerprint());
    frame(&mut canonical, layout_fingerprint);
    frame(&mut canonical, type_fingerprint);
    for dependency in dependencies {
        frame(&mut canonical, dependency.kind.as_str());
        frame(&mut canonical, &dependency.id);
        frame(&mut canonical, &dependency.fingerprint);
    }
    SHA256::sha256_hex(canonical.as_bytes())
}

/// One candidate supplied by a watcher or host adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleEdit {
    pub source_id: String,
    pub source: String,
    pub dependencies: Vec<TerminalStyleDependency>,
    pub capabilities: TerminalCapabilityFacts,
    pub layout_fingerprint: String,
    pub type_fingerprint: String,
}

impl TerminalStyleEdit {
    pub fn new(source_id: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            source_id: source_id.into(),
            source: source.into(),
            dependencies: Vec::new(),
            capabilities: TerminalCapabilityFacts::default(),
            layout_fingerprint: String::new(),
            type_fingerprint: String::new(),
        }
    }

    pub fn with_capabilities(mut self, capabilities: TerminalCapabilityFacts) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_dependencies(mut self, dependencies: Vec<TerminalStyleDependency>) -> Self {
        self.dependencies = dependencies;
        self
    }

    pub fn with_layout_fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.layout_fingerprint = fingerprint.into();
        self
    }

    pub fn with_type_fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.type_fingerprint = fingerprint.into();
        self
    }

    fn bound_source(mut self) -> Self {
        if self.source.len() > MAX_TERMINAL_STYLE_SOURCE_BYTES {
            let mut end = 0;
            for character in self.source.chars() {
                end += character.len_utf8();
                if end > MAX_TERMINAL_STYLE_SOURCE_BYTES {
                    break;
                }
            }
            self.source.truncate(end);
        }
        self
    }
}

/// Semantic style roles shared by terminal renderers.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum TerminalStyleRole {
    Accent,
    Dim,
    Success,
    Warn,
    Error,
    Invert,
    Border,
    Bold,
}

impl TerminalStyleRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accent => "accent",
            Self::Dim => "dim",
            Self::Success => "success",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Invert => "invert",
            Self::Border => "border",
            Self::Bold => "bold",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "accent" => Some(Self::Accent),
            "dim" => Some(Self::Dim),
            "success" => Some(Self::Success),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            "invert" => Some(Self::Invert),
            "border" => Some(Self::Border),
            "bold" => Some(Self::Bold),
            _ => None,
        }
    }
}

/// A validated terminal style value.  Raw source text is not retained.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalStyleValue {
    Plain,
    Sgr(String),
}

impl TerminalStyleValue {
    pub fn sgr(&self) -> Option<&str> {
        match self {
            Self::Plain => None,
            Self::Sgr(value) => Some(value),
        }
    }

    pub fn paint(&self, text: &str) -> String {
        match self {
            Self::Plain => text.to_string(),
            Self::Sgr(sgr) => format!("\x1b[{sgr}m{text}\x1b[0m"),
        }
    }
}

/// Parsed semantic style roles.  This is an internal line-oriented transport
/// representation, not a new user-facing language syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleDocument {
    roles: BTreeMap<TerminalStyleRole, TerminalStyleValue>,
}

impl TerminalStyleDocument {
    pub fn parse(source: &str) -> Result<Self, TerminalStyleParseError> {
        if source.len() > MAX_TERMINAL_STYLE_SOURCE_BYTES {
            return Err(TerminalStyleParseError::new(
                0,
                TerminalStyleErrorCode::SourceTooLarge,
                format!(
                    "style source exceeds {} bytes",
                    MAX_TERMINAL_STYLE_SOURCE_BYTES
                ),
            ));
        }
        if source.bytes().any(|byte| {
            byte.is_ascii_control() && !matches!(byte, b'\n' | b'\r' | b'\t')
        }) {
            return Err(TerminalStyleParseError::new(
                0,
                TerminalStyleErrorCode::InvalidStyle,
                "style source contains a control byte",
            ));
        }
        let mut roles = BTreeMap::new();
        for (index, line) in source.lines().enumerate() {
            let line_number = index + 1;
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            let Some((raw_role, raw_value)) = line.split_once('=') else {
                return Err(TerminalStyleParseError::new(
                    line_number,
                    TerminalStyleErrorCode::InvalidStyle,
                    "expected role=value",
                ));
            };
            let role_name = raw_role.trim();
            let Some(role) = TerminalStyleRole::parse(role_name) else {
                return Err(TerminalStyleParseError::new(
                    line_number,
                    TerminalStyleErrorCode::InvalidStyle,
                    "unknown semantic style role",
                ));
            };
            if roles.contains_key(&role) {
                return Err(TerminalStyleParseError::new(
                    line_number,
                    TerminalStyleErrorCode::InvalidStyle,
                    "style role is declared more than once",
                ));
            }
            if roles.len() >= MAX_TERMINAL_STYLE_ROLES {
                return Err(TerminalStyleParseError::new(
                    line_number,
                    TerminalStyleErrorCode::TooManyRoles,
                    format!("style exceeds {} roles", MAX_TERMINAL_STYLE_ROLES),
                ));
            }
            let value = parse_style_value(raw_value.trim()).map_err(|message| {
                TerminalStyleParseError::new(line_number, TerminalStyleErrorCode::InvalidStyle, message)
            })?;
            roles.insert(role, value);
        }
        if roles.is_empty() {
            return Err(TerminalStyleParseError::new(
                0,
                TerminalStyleErrorCode::InvalidStyle,
                "style source declares no semantic roles",
            ));
        }
        Ok(Self { roles })
    }

    pub fn roles(&self) -> &BTreeMap<TerminalStyleRole, TerminalStyleValue> {
        &self.roles
    }

    pub fn value(&self, role: TerminalStyleRole) -> Option<&TerminalStyleValue> {
        self.roles.get(&role)
    }
}

/// A parse failure that can be projected into one bounded error fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleParseError {
    pub line: usize,
    pub code: TerminalStyleErrorCode,
    pub message: String,
}

impl TerminalStyleParseError {
    fn new(line: usize, code: TerminalStyleErrorCode, message: impl Into<String>) -> Self {
        Self {
            line,
            code,
            message: bounded_text(&message.into(), MAX_TERMINAL_STYLE_ERROR_BYTES),
        }
    }
}

impl fmt::Display for TerminalStyleParseError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(output, "{}: {}", self.code, self.message)
        } else {
            write!(output, "{} at line {}: {}", self.code, self.line, self.message)
        }
    }
}

impl std::error::Error for TerminalStyleParseError {}

fn parse_style_value(value: &str) -> Result<TerminalStyleValue, String> {
    if value == "plain" {
        return Ok(TerminalStyleValue::Plain);
    }
    let named = match value {
        "black" => Some("30"),
        "red" => Some("31"),
        "green" => Some("32"),
        "yellow" => Some("33"),
        "blue" => Some("34"),
        "magenta" => Some("35"),
        "cyan" => Some("36"),
        "white" => Some("37"),
        "bold" => Some("1"),
        "dim" => Some("2"),
        _ => None,
    };
    let value = named.unwrap_or(value);
    if value.is_empty() || value.len() > 128 {
        return Err("style SGR value is empty or too long".to_string());
    }
    let mut normalized = String::new();
    for (index, part) in value.split(';').enumerate() {
        if part.is_empty() {
            return Err("style SGR value has an empty code".to_string());
        }
        let code = part
            .parse::<u16>()
            .map_err(|_| "style SGR value has a non-numeric code".to_string())?;
        if code > 255 {
            return Err("style SGR code exceeds 255".to_string());
        }
        if index > 0 {
            normalized.push(';');
        }
        normalized.push_str(&code.to_string());
    }
    Ok(TerminalStyleValue::Sgr(normalized))
}

/// One immutable, atomically replaceable style snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleSnapshot {
    pub revision: u64,
    pub source_id: String,
    pub roles: BTreeMap<TerminalStyleRole, TerminalStyleValue>,
    pub facts: TerminalStyleFingerprintFacts,
}

impl TerminalStyleSnapshot {
    pub fn fingerprint(&self) -> &str {
        &self.facts.fingerprint
    }

    pub fn capabilities(&self) -> TerminalCapabilityFacts {
        self.facts.capabilities
    }

    pub fn value(&self, role: TerminalStyleRole) -> Option<&TerminalStyleValue> {
        self.roles.get(&role)
    }

    pub fn paint(&self, role: TerminalStyleRole, text: &str) -> String {
        match self.roles.get(&role) {
            Some(value) if self.facts.capabilities.color_enabled() => value.paint(text),
            _ => text.to_string(),
        }
    }
}

/// Why a valid candidate cannot be applied by a style-only replacement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalStyleRestartReason {
    SourceIdentityChanged,
    LayoutFingerprintChanged,
    TypeFingerprintChanged,
}

impl TerminalStyleRestartReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceIdentityChanged => "source_identity_changed",
            Self::LayoutFingerprintChanged => "layout_fingerprint_changed",
            Self::TypeFingerprintChanged => "type_fingerprint_changed",
        }
    }
}

/// Classification of one valid candidate relative to the last valid snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalStyleChangeKind {
    Initial,
    Unchanged,
    StyleOnly,
    CapabilityOnly,
    RestartRequired(TerminalStyleRestartReason),
    Invalid,
}

impl TerminalStyleChangeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::Unchanged => "unchanged",
            Self::StyleOnly => "style_only",
            Self::CapabilityOnly => "capability_only",
            Self::RestartRequired(_) => "restart_required",
            Self::Invalid => "invalid",
        }
    }
}

/// Compare only typed style facts.  Application/widget state is intentionally
/// not an input, so a style-only result cannot accidentally reset it.
pub fn classify_change(
    previous: Option<&TerminalStyleSnapshot>,
    next: &TerminalStyleSnapshot,
) -> TerminalStyleChangeKind {
    let Some(previous) = previous else {
        return TerminalStyleChangeKind::Initial;
    };
    if previous.source_id != next.source_id || previous.facts.source_id != next.facts.source_id {
        return TerminalStyleChangeKind::RestartRequired(
            TerminalStyleRestartReason::SourceIdentityChanged,
        );
    }
    if previous.facts.layout_fingerprint != next.facts.layout_fingerprint
        || !dependencies_equal(
            previous,
            next,
            TerminalStyleDependencyKind::Layout,
        )
    {
        return TerminalStyleChangeKind::RestartRequired(
            TerminalStyleRestartReason::LayoutFingerprintChanged,
        );
    }
    if previous.facts.type_fingerprint != next.facts.type_fingerprint
        || !dependencies_equal(
            previous,
            next,
            TerminalStyleDependencyKind::ValueType,
        )
    {
        return TerminalStyleChangeKind::RestartRequired(
            TerminalStyleRestartReason::TypeFingerprintChanged,
        );
    }
    if previous.facts.fingerprint == next.facts.fingerprint {
        return TerminalStyleChangeKind::Unchanged;
    }
    let style_inputs_changed = previous.facts.source_fingerprint != next.facts.source_fingerprint
        || previous.facts.dependencies != next.facts.dependencies;
    if !style_inputs_changed && previous.facts.capabilities != next.facts.capabilities {
        return TerminalStyleChangeKind::CapabilityOnly;
    }
    TerminalStyleChangeKind::StyleOnly
}

fn dependencies_equal(
    previous: &TerminalStyleSnapshot,
    next: &TerminalStyleSnapshot,
    kind: TerminalStyleDependencyKind,
) -> bool {
    previous
        .facts
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind == kind)
        .map(|dependency| (&dependency.id, &dependency.fingerprint))
        .eq(
            next
                .facts
                .dependencies
                .iter()
                .filter(|dependency| dependency.kind == kind)
                .map(|dependency| (&dependency.id, &dependency.fingerprint)),
        )
}

/// One bounded parse error retained by the reload state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleErrorFact {
    pub code: TerminalStyleErrorCode,
    pub source_id: String,
    pub attempted_fingerprint: Option<String>,
    pub revision: u64,
    pub message: String,
}

impl TerminalStyleErrorFact {
    pub fn is_bounded(&self) -> bool {
        self.message.len() <= MAX_TERMINAL_STYLE_ERROR_BYTES
            && self.source_id.len() <= MAX_TERMINAL_STYLE_ID_BYTES
            && self
                .attempted_fingerprint
                .as_ref()
                .is_none_or(|value| value.len() == 64)
    }

    pub fn render(&self) -> String {
        let fingerprint = self.attempted_fingerprint.as_deref().unwrap_or("none");
        format!(
            "{} source={} revision={} fingerprint={} detail={}",
            self.code, self.source_id, self.revision, fingerprint, self.message
        )
    }
}

/// Result of one immediate or coalesced candidate application.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleReloadReceipt {
    pub revision: u64,
    pub change: TerminalStyleChangeKind,
    /// The last valid snapshot, including when the candidate was rejected or
    /// requires a full application restart.
    pub snapshot: Option<Arc<TerminalStyleSnapshot>>,
    pub error: Option<TerminalStyleErrorFact>,
}

impl TerminalStyleReloadReceipt {
    pub const fn applied(&self) -> bool {
        matches!(
            self.change,
            TerminalStyleChangeKind::Initial
                | TerminalStyleChangeKind::StyleOnly
                | TerminalStyleChangeKind::CapabilityOnly
        )
    }

    pub const fn restart_required(&self) -> bool {
        matches!(self.change, TerminalStyleChangeKind::RestartRequired(_))
    }

    pub const fn rejected(&self) -> bool {
        matches!(self.change, TerminalStyleChangeKind::Invalid)
    }
}

/// One subscriber event.  It carries identities and outcomes, never raw style
/// source or application payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleReloadNotification {
    pub revision: u64,
    pub change: TerminalStyleChangeKind,
    pub prior_fingerprint: Option<String>,
    pub candidate_fingerprint: Option<String>,
    pub error: Option<TerminalStyleErrorFact>,
}

/// Stable handle for one mailbox subscriber.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TerminalStyleSubscriberId(u64);

impl TerminalStyleSubscriberId {
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// Failure to allocate a bounded subscriber mailbox.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalStyleSubscribeError {
    LimitReached,
}

impl fmt::Display for TerminalStyleSubscribeError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitReached => output.write_str("terminal style subscriber limit reached"),
        }
    }
}

impl std::error::Error for TerminalStyleSubscribeError {}

struct SubscriberMailbox {
    notifications: VecDeque<TerminalStyleReloadNotification>,
}

struct ReloadState {
    current: Option<Arc<TerminalStyleSnapshot>>,
    pending: Option<TerminalStyleEdit>,
    last_error: Option<TerminalStyleErrorFact>,
    next_subscriber: u64,
    subscribers: BTreeMap<TerminalStyleSubscriberId, SubscriberMailbox>,
}

/// Atomic style snapshot owner with deterministic last-write-wins coalescing.
///
/// The mutex covers candidate parsing, classification, snapshot replacement,
/// error retention, and notification enqueueing as one transaction.  No
/// widget, focus, cursor, or application model field exists here.
pub struct TerminalStyleReload {
    state: Mutex<ReloadState>,
}

impl Default for TerminalStyleReload {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalStyleReload {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(ReloadState {
                current: None,
                pending: None,
                last_error: None,
                next_subscriber: 1,
                subscribers: BTreeMap::new(),
            }),
        }
    }

    /// Apply one candidate immediately.
    pub fn reload(&self, edit: TerminalStyleEdit) -> TerminalStyleReloadReceipt {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        apply_locked(&mut state, edit.bound_source())
    }

    /// Queue one candidate.  Replacing the pending candidate is the complete
    /// coalescing operation: a burst flushes only its last deterministic edit.
    pub fn enqueue(&self, edit: TerminalStyleEdit) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.pending = Some(edit.bound_source());
    }

    pub fn has_pending(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending
            .is_some()
    }

    /// Apply the last candidate in the current burst, if any.
    pub fn flush(&self) -> Option<TerminalStyleReloadReceipt> {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let edit = state.pending.take()?;
        Some(apply_locked(&mut state, edit))
    }

    pub fn revision(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .current
            .as_ref()
            .map_or(0, |snapshot| snapshot.revision)
    }

    /// Return the current last-valid style.  The returned `Arc` keeps the
    /// snapshot alive while a renderer paints a frame.
    pub fn last_valid(&self) -> Option<Arc<TerminalStyleSnapshot>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .current
            .clone()
    }

    pub fn error_fact(&self) -> Option<TerminalStyleErrorFact> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .last_error
            .clone()
    }

    pub fn subscribe(&self) -> Result<TerminalStyleSubscriberId, TerminalStyleSubscribeError> {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.subscribers.len() >= MAX_TERMINAL_STYLE_SUBSCRIBERS {
            return Err(TerminalStyleSubscribeError::LimitReached);
        }
        let id = TerminalStyleSubscriberId(state.next_subscriber);
        state.next_subscriber = state.next_subscriber.saturating_add(1);
        state.subscribers.insert(
            id,
            SubscriberMailbox {
                notifications: VecDeque::new(),
            },
        );
        Ok(id)
    }

    pub fn unsubscribe(&self, id: TerminalStyleSubscriberId) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .subscribers
            .remove(&id)
            .is_some()
    }

    /// Drain one mailbox in revision/arrival order.
    pub fn drain_notifications(
        &self,
        id: TerminalStyleSubscriberId,
    ) -> Vec<TerminalStyleReloadNotification> {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(mailbox) = state.subscribers.get_mut(&id) else {
            return Vec::new();
        };
        std::mem::take(&mut mailbox.notifications)
            .into_iter()
            .collect()
    }

    pub fn subscriber_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .subscribers
            .len()
    }

    /// Reset the resident kernel to a fresh, unopened state.  Used when
    /// the bound source identity itself changes -- a different style
    /// source entirely, not a live edit of the current one -- so the next
    /// candidate is classified `Initial` instead of being permanently
    /// stuck behind `RestartRequired(SourceIdentityChanged)`.
    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.current = None;
        state.pending = None;
        state.last_error = None;
    }
}

fn apply_locked(state: &mut ReloadState, edit: TerminalStyleEdit) -> TerminalStyleReloadReceipt {
    let current = state.current.clone();
    let revision = current.as_ref().map_or(0, |snapshot| snapshot.revision);
    let attempted_fingerprint = (edit.source.len() <= MAX_TERMINAL_STYLE_SOURCE_BYTES)
        .then(|| SHA256::sha256_hex(edit.source.as_bytes()));
    let candidate = match build_snapshot(&edit) {
        Ok(candidate) => candidate,
        Err(error) => {
            let fact = TerminalStyleErrorFact {
                code: error.code(),
                source_id: bounded_identity(&edit.source_id),
                attempted_fingerprint: attempted_fingerprint.clone(),
                revision,
                message: bounded_text(&error.message(), MAX_TERMINAL_STYLE_ERROR_BYTES),
            };
            state.last_error = Some(fact.clone());
            let receipt = TerminalStyleReloadReceipt {
                revision,
                change: TerminalStyleChangeKind::Invalid,
                snapshot: current.clone(),
                error: Some(fact.clone()),
            };
            notify(
                state,
                TerminalStyleReloadNotification {
                    revision,
                    change: TerminalStyleChangeKind::Invalid,
                    prior_fingerprint: current
                        .as_ref()
                        .map(|snapshot| snapshot.fingerprint().to_string()),
                    candidate_fingerprint: attempted_fingerprint,
                    error: Some(fact),
                },
            );
            return receipt;
        }
    };
    let change = classify_change(current.as_deref(), &candidate);
    let candidate_fingerprint = Some(candidate.fingerprint().to_string());
    match change {
        TerminalStyleChangeKind::Unchanged => {
            state.last_error = None;
            TerminalStyleReloadReceipt {
                revision,
                change,
                snapshot: current,
                error: None,
            }
        }
        TerminalStyleChangeKind::RestartRequired(reason) => {
            state.last_error = None;
            let receipt = TerminalStyleReloadReceipt {
                revision,
                change: TerminalStyleChangeKind::RestartRequired(reason),
                snapshot: current.clone(),
                error: None,
            };
            notify(
                state,
                TerminalStyleReloadNotification {
                    revision,
                    change: TerminalStyleChangeKind::RestartRequired(reason),
                    prior_fingerprint: current
                        .as_ref()
                        .map(|snapshot| snapshot.fingerprint().to_string()),
                    candidate_fingerprint,
                    error: None,
                },
            );
            receipt
        }
        TerminalStyleChangeKind::Initial
        | TerminalStyleChangeKind::StyleOnly
        | TerminalStyleChangeKind::CapabilityOnly => {
            let Some(next_revision) = revision.checked_add(1) else {
                let fact = TerminalStyleErrorFact {
                    code: TerminalStyleErrorCode::RevisionOverflow,
                    source_id: bounded_identity(&edit.source_id),
                    attempted_fingerprint: attempted_fingerprint.clone(),
                    revision,
                    message: "style revision counter exhausted".to_string(),
                };
                state.last_error = Some(fact.clone());
                let receipt = TerminalStyleReloadReceipt {
                    revision,
                    change: TerminalStyleChangeKind::Invalid,
                    snapshot: current.clone(),
                    error: Some(fact.clone()),
                };
                notify(
                    state,
                    TerminalStyleReloadNotification {
                        revision,
                        change: TerminalStyleChangeKind::Invalid,
                        prior_fingerprint: current
                            .as_ref()
                            .map(|snapshot| snapshot.fingerprint().to_string()),
                        candidate_fingerprint: attempted_fingerprint,
                        error: Some(fact),
                    },
                );
                return receipt;
            };
            let mut candidate = candidate;
            candidate.revision = next_revision;
            let snapshot = Arc::new(candidate);
            state.current = Some(snapshot.clone());
            state.last_error = None;
            notify(
                state,
                TerminalStyleReloadNotification {
                    revision: next_revision,
                    change,
                    prior_fingerprint: current
                        .as_ref()
                        .map(|snapshot| snapshot.fingerprint().to_string()),
                    candidate_fingerprint: Some(snapshot.fingerprint().to_string()),
                    error: None,
                },
            );
            TerminalStyleReloadReceipt {
                revision: next_revision,
                change,
                snapshot: Some(snapshot),
                error: None,
            }
        }
        TerminalStyleChangeKind::Invalid => unreachable!("candidate parsing produced invalid change"),
    }
}

fn build_snapshot(edit: &TerminalStyleEdit) -> Result<TerminalStyleSnapshot, BuildError> {
    let document = TerminalStyleDocument::parse(&edit.source).map_err(BuildError::Parse)?;
    let facts = TerminalStyleFingerprintFacts::new(
        edit.source_id.clone(),
        &edit.source,
        edit.dependencies.clone(),
        edit.capabilities,
        edit.layout_fingerprint.clone(),
        edit.type_fingerprint.clone(),
    )
    .map_err(BuildError::Facts)?;
    Ok(TerminalStyleSnapshot {
        revision: 0,
        source_id: edit.source_id.clone(),
        roles: document.roles,
        facts,
    })
}

enum BuildError {
    Parse(TerminalStyleParseError),
    Facts(TerminalStyleFactError),
}

impl BuildError {
    fn code(&self) -> TerminalStyleErrorCode {
        match self {
            Self::Parse(error) => error.code,
            Self::Facts(error) => error.code,
        }
    }

    fn message(&self) -> String {
        match self {
            Self::Parse(error) => error.to_string(),
            Self::Facts(error) => error.to_string(),
        }
    }
}

fn notify(state: &mut ReloadState, notification: TerminalStyleReloadNotification) {
    for mailbox in state.subscribers.values_mut() {
        if mailbox.notifications.len() >= MAX_TERMINAL_STYLE_NOTIFICATIONS {
            mailbox.notifications.pop_front();
        }
        mailbox.notifications.push_back(notification.clone());
    }
}

fn validate_identity(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("style identity must not be empty".to_string());
    }
    if value.len() > MAX_TERMINAL_STYLE_ID_BYTES {
        return Err(format!(
            "style identity exceeds {} bytes",
            MAX_TERMINAL_STYLE_ID_BYTES
        ));
    }
    if value.chars().any(char::is_control) {
        return Err("style identity contains a control character".to_string());
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("style dependency fingerprint must be lowercase SHA-256".to_string());
    }
    Ok(())
}

fn validate_optional_fingerprint(value: String, label: &str) -> Result<String, TerminalStyleFactError> {
    if value.len() > MAX_TERMINAL_STYLE_ID_BYTES || value.chars().any(char::is_control) {
        return Err(TerminalStyleFactError::new(
            TerminalStyleErrorCode::InvalidFingerprint,
            format!("{label} is not a bounded stable string"),
        ));
    }
    Ok(value)
}

fn frame(output: &mut String, value: &str) {
    output.push_str(&value.len().to_string());
    output.push(':');
    output.push_str(value);
    output.push(';');
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    let mut output = String::new();
    for character in value.chars() {
        if character.is_control() {
            continue;
        }
        if output.len() + character.len_utf8() > max_bytes {
            break;
        }
        output.push(character);
    }
    output
}

fn bounded_identity(value: &str) -> String {
    bounded_text(value, MAX_TERMINAL_STYLE_ID_BYTES)
}

/// ---------------------------------------------------------------------
/// Host-facing adapter
/// ---------------------------------------------------------------------
///
/// The kernel above owns style facts only.  `TerminalStyleHostAdapter` is
/// the boring, typed seam a resident host (for example `WebHost`) stores
/// once and delegates its `/style` surface to.  It injects the two facts
/// only a real host producer can supply -- source identity and terminal
/// capability -- accepts one bounded style-update JSON body per call, and
/// returns a typed outcome plus one event JSON string.  It never derives a
/// capability from a client request, never performs a full application
/// reload, and never reimplements parsing, fingerprinting, or change
/// classification: every decision is delegated to `TerminalStyleReload`.

/// A real host's typed source identity and terminal capability facts.
/// Capability facts are trusted as supplied: this type never derives them
/// from a request or from the process environment, so a caller cannot make
/// the adapter report a terminal capability the host did not observe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalStyleHostSource {
    pub source_id: String,
    pub capabilities: TerminalCapabilityFacts,
}

impl TerminalStyleHostSource {
    pub fn new(
        source_id: impl Into<String>,
        capabilities: TerminalCapabilityFacts,
    ) -> Result<Self, TerminalStyleFactError> {
        let source_id = source_id.into();
        validate_identity(&source_id).map_err(|message| {
            TerminalStyleFactError::new(TerminalStyleErrorCode::InvalidSourceIdentity, message)
        })?;
        Ok(Self {
            source_id,
            capabilities,
        })
    }
}

/// Why one `/style` request could not be evaluated at all.  Distinct from
/// `TerminalStyleHostRejection`, which reports a candidate that was
/// evaluated by the kernel and found invalid or restart-requiring.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalStyleHostUnavailableReason {
    /// No real producer has injected a source identity/capability yet.
    SourceNotInjected,
    /// A style has never been applied, so there is no current snapshot.
    NoStyleApplied,
    /// The request body is not valid JSON, or exceeds the bounded envelope.
    MalformedRequest,
    /// The request JSON does not declare a `source` string field.
    MissingSource,
}

impl TerminalStyleHostUnavailableReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceNotInjected => "source_not_injected",
            Self::NoStyleApplied => "no_style_applied",
            Self::MalformedRequest => "malformed_request",
            Self::MissingSource => "missing_source",
        }
    }
}

/// Why one evaluated style candidate could not be applied by this
/// style-only delegate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalStyleHostRejection {
    /// The candidate failed bounded parsing or fact construction.
    Invalid(TerminalStyleErrorFact),
    /// The candidate is valid but would require a full application
    /// restart; this style-only delegate never performs one.
    RestartRequired(TerminalStyleRestartReason),
}

impl TerminalStyleHostRejection {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(fact) => fact.code.as_str(),
            Self::RestartRequired(reason) => reason.as_str(),
        }
    }
}

/// Typed result of one host `/style` delegate call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalStyleHostOutcome {
    /// The candidate replaced (or matched) the resident style snapshot.
    Applied {
        revision: u64,
        change: TerminalStyleChangeKind,
    },
    /// The request could not be evaluated; see
    /// `TerminalStyleHostUnavailableReason`.
    Unavailable(TerminalStyleHostUnavailableReason),
    /// The request was evaluated and rejected; see
    /// `TerminalStyleHostRejection`.
    Rejected(TerminalStyleHostRejection),
}

/// Persistent host-facing wrapper.  A resident host owns exactly one
/// instance and delegates its `/style` surface to it instead of
/// reimplementing style parsing, fingerprinting, or change classification.
pub struct TerminalStyleHostAdapter {
    reload: TerminalStyleReload,
    source: Mutex<Option<TerminalStyleHostSource>>,
}

impl Default for TerminalStyleHostAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalStyleHostAdapter {
    pub fn new() -> Self {
        Self {
            reload: TerminalStyleReload::new(),
            source: Mutex::new(None),
        }
    }

    /// Inject (or replace) the real host's typed source identity and
    /// terminal capability facts.  Before this is ever called, every
    /// `apply_json` call reports `Unavailable(SourceNotInjected)` rather
    /// than applying against a fabricated default.  Injecting a source
    /// identity that differs from the previously injected one resets the
    /// resident kernel, since that is a bind to a different style source
    /// entirely, not a live edit of the current one; without the reset,
    /// the kernel would classify every future candidate against the old
    /// source's snapshot and stay stuck behind
    /// `RestartRequired(SourceIdentityChanged)` forever.
    pub fn inject(&self, source: TerminalStyleHostSource) {
        let mut guard = self
            .source
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let identity_changed = guard
            .as_ref()
            .is_some_and(|previous| previous.source_id != source.source_id);
        *guard = Some(source);
        drop(guard);
        if identity_changed {
            self.reload.reset();
        }
    }

    pub fn is_injected(&self) -> bool {
        self.source
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }

    /// Apply one bounded `/style` request body.  The JSON must declare a
    /// `source` string; `dependencies`, `layout_fingerprint`, and
    /// `type_fingerprint` are optional.  Source identity and terminal
    /// capability always come from the last injected
    /// `TerminalStyleHostSource`, never from the request body, so a
    /// request cannot fabricate a terminal capability the host did not
    /// report.
    pub fn apply_json(&self, body: &str) -> (TerminalStyleHostOutcome, String) {
        let source = self
            .source
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(source) = source else {
            let outcome = TerminalStyleHostOutcome::Unavailable(
                TerminalStyleHostUnavailableReason::SourceNotInjected,
            );
            let event = outcome_json(&outcome, None);
            return (outcome, event);
        };
        let revision = self.reload.revision();
        let edit = match parse_style_request(body, &source, revision) {
            Ok(edit) => edit,
            Err(outcome) => {
                let event = outcome_json(&outcome, None);
                return (outcome, event);
            }
        };
        let receipt = self.reload.reload(edit);
        let outcome = host_outcome_from_receipt(&receipt);
        let event = outcome_json(&outcome, receipt.snapshot.as_deref());
        (outcome, event)
    }

    /// Return the current last-valid style as one applied outcome and event
    /// JSON, or `Unavailable(NoStyleApplied)` when nothing has ever
    /// applied.
    pub fn current_json(&self) -> (TerminalStyleHostOutcome, String) {
        match self.reload.last_valid() {
            Some(snapshot) => {
                let outcome = TerminalStyleHostOutcome::Applied {
                    revision: snapshot.revision,
                    change: TerminalStyleChangeKind::Unchanged,
                };
                let event = outcome_json(&outcome, Some(&*snapshot));
                (outcome, event)
            }
            None => {
                let outcome = TerminalStyleHostOutcome::Unavailable(
                    TerminalStyleHostUnavailableReason::NoStyleApplied,
                );
                let event = outcome_json(&outcome, None);
                (outcome, event)
            }
        }
    }
}

fn malformed_request() -> TerminalStyleHostOutcome {
    TerminalStyleHostOutcome::Unavailable(TerminalStyleHostUnavailableReason::MalformedRequest)
}

fn parse_style_request(
    body: &str,
    source: &TerminalStyleHostSource,
    revision: u64,
) -> Result<TerminalStyleEdit, TerminalStyleHostOutcome> {
    let value =
        parse_json_with_limit(body, MAX_TERMINAL_STYLE_REQUEST_BYTES).map_err(|_| malformed_request())?;
    let object = object_map(&value).map_err(|_| malformed_request())?;
    let style_source = match object.get("source") {
        Some(DataTree::Text(text)) => text.clone(),
        _ => {
            return Err(TerminalStyleHostOutcome::Unavailable(
                TerminalStyleHostUnavailableReason::MissingSource,
            ))
        }
    };
    let mut edit = TerminalStyleEdit::new(source.source_id.clone(), style_source)
        .with_capabilities(source.capabilities);
    if let Some(DataTree::Text(layout)) = object.get("layout_fingerprint") {
        edit = edit.with_layout_fingerprint(layout.clone());
    }
    if let Some(DataTree::Text(type_fingerprint)) = object.get("type_fingerprint") {
        edit = edit.with_type_fingerprint(type_fingerprint.clone());
    }
    if let Some(entries) = object.get("dependencies") {
        let DataTree::Array(entries) = entries else {
            return Err(malformed_request());
        };
        let mut dependencies = Vec::with_capacity(entries.len());
        for entry in entries {
            let fields = object_map(entry).map_err(|_| malformed_request())?;
            let kind = match fields.get("kind") {
                Some(DataTree::Text(kind)) => {
                    parse_dependency_kind(kind).ok_or_else(malformed_request)?
                }
                _ => return Err(malformed_request()),
            };
            let id = match fields.get("id") {
                Some(DataTree::Text(id)) => id.clone(),
                _ => return Err(malformed_request()),
            };
            let fingerprint = match fields.get("fingerprint") {
                Some(DataTree::Text(fingerprint)) => fingerprint.clone(),
                _ => return Err(malformed_request()),
            };
            let dependency = TerminalStyleDependency::new(kind, id, fingerprint).map_err(|error| {
                TerminalStyleHostOutcome::Rejected(TerminalStyleHostRejection::Invalid(
                    TerminalStyleErrorFact {
                        code: error.code,
                        source_id: bounded_identity(&source.source_id),
                        attempted_fingerprint: None,
                        revision,
                        message: error.message,
                    },
                ))
            })?;
            dependencies.push(dependency);
        }
        edit = edit.with_dependencies(dependencies);
    }
    Ok(edit)
}
fn object_map(value: &DataTree) -> Result<BTreeMap<String, DataTree>, String> {
    let DataTree::Object(fields) = value else {
        return Err("terminal style request must be an object".to_string());
    };
    Ok(fields.iter().cloned().collect())
}

fn parse_dependency_kind(value: &str) -> Option<TerminalStyleDependencyKind> {
    match value {
        "source" => Some(TerminalStyleDependencyKind::Source),
        "capability" => Some(TerminalStyleDependencyKind::Capability),
        "layout" => Some(TerminalStyleDependencyKind::Layout),
        "value_type" => Some(TerminalStyleDependencyKind::ValueType),
        _ => None,
    }
}

fn host_outcome_from_receipt(receipt: &TerminalStyleReloadReceipt) -> TerminalStyleHostOutcome {
    match receipt.change {
        TerminalStyleChangeKind::Initial
        | TerminalStyleChangeKind::StyleOnly
        | TerminalStyleChangeKind::CapabilityOnly
        | TerminalStyleChangeKind::Unchanged => TerminalStyleHostOutcome::Applied {
            revision: receipt.revision,
            change: receipt.change,
        },
        TerminalStyleChangeKind::RestartRequired(reason) => TerminalStyleHostOutcome::Rejected(
            TerminalStyleHostRejection::RestartRequired(reason),
        ),
        TerminalStyleChangeKind::Invalid => {
            let fact = receipt.error.clone().unwrap_or_else(|| TerminalStyleErrorFact {
                code: TerminalStyleErrorCode::InvalidStyle,
                source_id: String::new(),
                attempted_fingerprint: None,
                revision: receipt.revision,
                message: "style candidate was rejected".to_string(),
            });
            TerminalStyleHostOutcome::Rejected(TerminalStyleHostRejection::Invalid(fact))
        }
    }
}

fn outcome_json(outcome: &TerminalStyleHostOutcome, snapshot: Option<&TerminalStyleSnapshot>) -> String {
    let mut output = String::with_capacity(256);
    output.push_str("{\"protocol\":\"");
    output.push_str(TERMINAL_STYLE_PROTOCOL);
    output.push('"');
    match outcome {
        TerminalStyleHostOutcome::Applied { revision, change } => {
            output.push_str(",\"outcome\":\"applied\",\"revision\":");
            output.push_str(&revision.to_string());
            output.push_str(",\"change\":\"");
            output.push_str(change.as_str());
            output.push('"');
            if let Some(snapshot) = snapshot {
                output.push_str(",\"fingerprint\":\"");
                output.push_str(&json_escape(snapshot.fingerprint()));
                output.push('"');
            }
        }
        TerminalStyleHostOutcome::Rejected(rejection) => {
            output.push_str(",\"outcome\":\"rejected\",\"code\":\"");
            output.push_str(rejection.code());
            output.push('"');
            if let TerminalStyleHostRejection::Invalid(fact) = rejection {
                output.push_str(",\"message\":\"");
                output.push_str(&json_escape(&fact.message));
                output.push('"');
            }
        }
        TerminalStyleHostOutcome::Unavailable(reason) => {
            output.push_str(",\"outcome\":\"unavailable\",\"reason\":\"");
            output.push_str(reason.as_str());
            output.push('"');
        }
    }
    output.push('}');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(source: &str) -> TerminalStyleEdit {
        TerminalStyleEdit::new("app/style", source)
    }

    #[test]
    fn capability_facts_are_deterministic_and_follow_explicit_policy() {
        let ansi256 = TerminalCapabilityFacts::from_environment(
            Some("xterm-256color"),
            None,
            true,
            false,
            TerminalColorMode::Auto,
        );
        assert_eq!(ansi256.profile, TerminalColorProfile::Ansi256);
        assert!(ansi256.color_enabled());
        let dumb = TerminalCapabilityFacts::from_environment(
            Some("dumb"),
            Some("truecolor"),
            true,
            false,
            TerminalColorMode::Auto,
        );
        assert_eq!(dumb.profile, TerminalColorProfile::Monochrome);
        assert!(!dumb.color_enabled());
        let forced = TerminalCapabilityFacts::from_environment(
            Some("dumb"),
            None,
            false,
            true,
            TerminalColorMode::Always,
        );
        assert_eq!(forced.profile, TerminalColorProfile::Ansi16);
        assert!(forced.color_enabled());
        assert_eq!(ansi256.fingerprint(), ansi256.fingerprint());
    }

    #[test]
    fn parser_produces_typed_roles_without_retaining_source() {
        let document = TerminalStyleDocument::parse(
            "# semantic roles\naccent=1;96\nsuccess=green\nerror=plain\n",
        )
        .expect("style parses");
        assert_eq!(document.value(TerminalStyleRole::Accent).unwrap().sgr(), Some("1;96"));
        assert_eq!(document.value(TerminalStyleRole::Success).unwrap().sgr(), Some("32"));
        assert_eq!(document.value(TerminalStyleRole::Error), Some(&TerminalStyleValue::Plain));
        assert!(TerminalStyleDocument::parse("accent=1;;96").is_err());
    }

    #[test]
    fn style_reload_is_atomic_and_keeps_last_valid_snapshot_on_failure() {
        let reload = TerminalStyleReload::new();
        let subscriber = reload.subscribe().expect("subscriber");
        let first = reload.reload(edit("accent=red\nsuccess=green"));
        assert!(first.applied());
        assert_eq!(first.revision, 1);
        let prior = reload.last_valid().expect("initial style");
        let state_outside_snapshot = String::from("focused:search cursor:3");
        let rejected = reload.reload(edit("accent=1;;31"));
        assert!(rejected.rejected());
        assert_eq!(rejected.revision, 1);
        assert_eq!(reload.last_valid().unwrap().fingerprint(), prior.fingerprint());
        assert_eq!(reload.revision(), 1);
        assert_eq!(state_outside_snapshot, "focused:search cursor:3");
        let error = reload.error_fact().expect("one error fact");
        assert_eq!(error.code, TerminalStyleErrorCode::InvalidStyle);
        assert!(error.is_bounded());
        let notifications = reload.drain_notifications(subscriber);
        assert_eq!(notifications.len(), 2);
        assert_eq!(notifications[0].revision, 1);
        assert_eq!(notifications[1].change, TerminalStyleChangeKind::Invalid);
        assert_eq!(notifications[1].revision, 1);
    }

    #[test]
    fn burst_coalescing_notifies_once_for_last_edit() {
        let reload = TerminalStyleReload::new();
        let subscriber = reload.subscribe().expect("subscriber");
        reload.reload(edit("accent=red"));
        let _ = reload.drain_notifications(subscriber);
        reload.enqueue(edit("accent=green"));
        reload.enqueue(edit("accent=blue"));
        reload.enqueue(edit("accent=yellow"));
        assert!(reload.has_pending());
        let receipt = reload.flush().expect("coalesced edit");
        assert_eq!(receipt.change, TerminalStyleChangeKind::StyleOnly);
        assert_eq!(receipt.revision, 2);
        assert_eq!(reload.drain_notifications(subscriber).len(), 1);
        assert_eq!(reload.last_valid().unwrap().value(TerminalStyleRole::Accent).unwrap().sgr(), Some("33"));
    }

    #[test]
    fn capability_change_applies_but_layout_or_type_change_requires_restart() {
        let reload = TerminalStyleReload::new();
        let base = TerminalCapabilityFacts::from_environment(
            Some("xterm-256color"),
            None,
            true,
            false,
            TerminalColorMode::Auto,
        );
        let plain = TerminalCapabilityFacts::from_environment(
            Some("dumb"),
            None,
            true,
            false,
            TerminalColorMode::Auto,
        );
        let first = reload.reload(edit("accent=red").with_capabilities(base));
        assert_eq!(first.revision, 1);
        let capability = reload.reload(edit("accent=red").with_capabilities(plain));
        assert_eq!(capability.change, TerminalStyleChangeKind::CapabilityOnly);
        assert_eq!(capability.revision, 2);
        let layout = reload.reload(
            edit("accent=green")
                .with_capabilities(plain)
                .with_layout_fingerprint("layout-v2"),
        );
        assert_eq!(
            layout.change,
            TerminalStyleChangeKind::RestartRequired(
                TerminalStyleRestartReason::LayoutFingerprintChanged
            )
        );
        assert_eq!(layout.revision, 2);
        let type_change = reload.reload(
            edit("accent=green")
                .with_capabilities(plain)
                .with_type_fingerprint("type-v2"),
        );
        assert_eq!(
            type_change.change,
            TerminalStyleChangeKind::RestartRequired(
                TerminalStyleRestartReason::TypeFingerprintChanged
            )
        );
        assert_eq!(type_change.revision, 2);
    }

    #[test]
    fn dependency_order_is_canonical() {
        let first = TerminalStyleDependency::from_bytes(
            TerminalStyleDependencyKind::Source,
            "z",
            b"z",
        )
        .unwrap();
        let second = TerminalStyleDependency::from_bytes(
            TerminalStyleDependencyKind::Source,
            "a",
            b"a",
        )
        .unwrap();
        let a = TerminalStyleFingerprintFacts::new(
            "style",
            "accent=red",
            vec![first.clone(), second.clone()],
            TerminalCapabilityFacts::default(),
            "",
            "",
        )
        .unwrap();
        let b = TerminalStyleFingerprintFacts::new(
            "style",
            "accent=red",
            vec![second, first],
            TerminalCapabilityFacts::default(),
            "",
            "",
        )
        .unwrap();
        assert_eq!(a.dependencies, b.dependencies);
        assert_eq!(a.fingerprint, b.fingerprint);
    }

    #[test]
    fn host_adapter_reports_unavailable_before_source_is_injected() {
        let adapter = TerminalStyleHostAdapter::new();
        assert!(!adapter.is_injected());
        let (outcome, event) = adapter.apply_json("{\"source\":\"accent=red\"}");
        assert_eq!(
            outcome,
            TerminalStyleHostOutcome::Unavailable(
                TerminalStyleHostUnavailableReason::SourceNotInjected
            )
        );
        assert!(event.contains("\"outcome\":\"unavailable\""));
        assert!(event.contains("\"reason\":\"source_not_injected\""));
    }

    #[test]
    fn host_adapter_applies_bounded_json_after_injection() {
        let adapter = TerminalStyleHostAdapter::new();
        adapter.inject(
            TerminalStyleHostSource::new("app/style", TerminalCapabilityFacts::default()).unwrap(),
        );
        assert!(adapter.is_injected());
        let (outcome, event) =
            adapter.apply_json("{\"source\":\"accent=red\\nsuccess=green\"}");
        assert_eq!(
            outcome,
            TerminalStyleHostOutcome::Applied {
                revision: 1,
                change: TerminalStyleChangeKind::Initial,
            }
        );
        assert!(event.contains("\"outcome\":\"applied\""));
        assert!(event.contains("\"fingerprint\""));
    }

    #[test]
    fn host_adapter_rejects_invalid_style_json() {
        let adapter = TerminalStyleHostAdapter::new();
        adapter.inject(
            TerminalStyleHostSource::new("app/style", TerminalCapabilityFacts::default()).unwrap(),
        );
        let (outcome, event) = adapter.apply_json("{\"source\":\"accent=1;;96\"}");
        match outcome {
            TerminalStyleHostOutcome::Rejected(TerminalStyleHostRejection::Invalid(fact)) => {
                assert_eq!(fact.code, TerminalStyleErrorCode::InvalidStyle);
            }
            other => panic!("expected a rejected invalid style, got {other:?}"),
        }
        assert!(event.contains("\"outcome\":\"rejected\""));
    }

    #[test]
    fn host_adapter_rejects_restart_required_layout_change_without_reloading() {
        let adapter = TerminalStyleHostAdapter::new();
        adapter.inject(
            TerminalStyleHostSource::new("app/style", TerminalCapabilityFacts::default()).unwrap(),
        );
        let (first, _) = adapter.apply_json("{\"source\":\"accent=red\"}");
        assert!(matches!(first, TerminalStyleHostOutcome::Applied { .. }));
        let (outcome, event) = adapter.apply_json(
            "{\"source\":\"accent=green\",\"layout_fingerprint\":\"layout-v2\"}",
        );
        assert_eq!(
            outcome,
            TerminalStyleHostOutcome::Rejected(TerminalStyleHostRejection::RestartRequired(
                TerminalStyleRestartReason::LayoutFingerprintChanged
            ))
        );
        assert!(event.contains("\"code\":\"layout_fingerprint_changed\""));
    }

    #[test]
    fn host_adapter_current_json_reports_unavailable_then_applied() {
        let adapter = TerminalStyleHostAdapter::new();
        let (before, _) = adapter.current_json();
        assert_eq!(
            before,
            TerminalStyleHostOutcome::Unavailable(TerminalStyleHostUnavailableReason::NoStyleApplied)
        );
        adapter.inject(
            TerminalStyleHostSource::new("app/style", TerminalCapabilityFacts::default()).unwrap(),
        );
        adapter.apply_json("{\"source\":\"accent=red\"}");
        let (after, event) = adapter.current_json();
        assert_eq!(
            after,
            TerminalStyleHostOutcome::Applied {
                revision: 1,
                change: TerminalStyleChangeKind::Unchanged,
            }
        );
        assert!(event.contains("\"fingerprint\""));
    }
}
