//! D-DX5-HOOK1=A (Tower #549) — compiler-extension v1 host surface.
//!
//! Reuses the shipped WASM Component Model substrate (`WASMTIME_CRATE_SPEC` +
//! `Prelude/CompilerExtension.rs`) in the isolated sibling `jetpack` process,
//! with a **compiler-specific** WIT world
//! that stays distinct from:
//! - application `target: sandbox` / `core.plugin` (world `jetplugin`, D-PLUGIN1)
//! - PATH-discovered `jet-*` helpers (D-DX5)
//!
//! V1 contract (ratified): post-sema typed read-only snapshot in → validated
//! findings/edit proposals out. The host remains the only semantic authority;
//! plugins cannot mutate compiler state or expose rustc (I2/I3).
//!
//! # Protocol (exact)
//!
//! Wire bytes for `analyze(snapshot) -> response` are UTF-8 DataTree values with
//! **deterministic key order** (lexicographic) and no insignificant whitespace.
//! Schema version is `protocol` (must equal [`PROTOCOL_VERSION`]).
//!
//! Snapshot fields: `protocol`, `stage`, `abilities`, `limits`, `trust`,
//! `types`, `symbols`, `spans`, `host_imports` (symbols carry `effects` +
//! `provenance`; type facts carry structural shape metadata).
//!
//! # Limits / trust / lifecycle
//!
//! - [`ResourceLimits`]: fuel, memory, table, finding/edit/response caps, wall timeout.
//! - [`TrustPolicy`]: components are `untrusted` by default; protocol + ability
//!   negotiation must pass before analyze.
//! - **Deterministic sandbox:** empty host linker — no clock/random/fs/net/process
//!   imports; guests that declare them fail closed at load (D-DX5-HOOK1).
//! - [`ExtensionSession`]: Idle → Loaded → (optional staged response) → Closed.
//!   Staging never mutates compiler facts; [`ExtensionSession::rollback`] discards
//!   staged output only (accept latch stays final); [`ExtensionSession::close`]
//!   invokes a guest closer (host passes `jet_compiler_extension_close`) so
//!   guest Store/memory is dropped.

use crate::FFI::WASMTIME_CRATE_SPEC;
use jet_foundation::Authority::HostImportFact;
use jet_foundation::DataTree::DataTree;
use crate::JSON;
use std::collections::{BTreeMap, BTreeSet};

/// Closed set of Jet plugin mechanisms (I8 — one semantic role each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginMechanism {
    /// D-DX5: `jet-<cmd>` executables discovered on PATH.
    PathHelper,
    /// D-PLUGIN1 / D-DEP-WASM1: application `target: sandbox` + `core.plugin`.
    ApplicationPlugin,
    /// D-DX5-HOOK1=A: compiler-extension WASM component (this module).
    CompilerExtension,
}

/// Wire protocol version for the typed post-sema snapshot contract.
pub const PROTOCOL_VERSION: u32 = 1;

/// First hook stage: after sema, typed facts only.
pub const STAGE: &str = "typed";

/// Component Model world name — fixed for every compiler-extension component.
/// Distinct from application plugins' fixed world `jetplugin` (D-PLUGIN-EXPORT1).
pub const WORLD_NAME: &str = "compiler-extension-v1";

/// WIT package identity for the compiler-extension world.
pub const PACKAGE_NAME: &str = "jet:compiler-extension@0.1.0";

/// Application `target: sandbox` world name (D-PLUGIN1 / D-PLUGIN-EXPORT1).
/// Kept here so callers can assert the two worlds never collide (I8).
pub const APPLICATION_PLUGIN_WORLD: &str = "jetplugin";

/// Required guest export that receives the versioned snapshot and returns a
/// validated response payload (findings / proposed edits as opaque bytes in
/// the host runtime; typed decode is host-owned).
pub const ANALYZE_EXPORT: &str = "analyze";

/// Expert env registration until a user-facing spelling is balloted
/// (D-DX5-HOOK1: "Exact user-facing registration spelling remains a later
/// ballot if new syntax is needed"). Absolute or relative path to one
/// `compiler-extension-v1` `.wasm` component. Unset → skip the post-sema hook.
pub const ENV_COMPILER_EXTENSION: &str = "JET_COMPILER_EXTENSION";

/// Hidden sibling-host verb. Version is part of the process protocol.
pub const HOST_SUBCOMMAND: &str = "__compiler-extension-v1";

/// Maximum encoded snapshot accepted across the compiler/host pipe.
pub const MAX_SNAPSHOT_BYTES: usize = 16 * 1024 * 1024;

/// Outer process deadline. Guest execution has its own 2s Wasmtime deadline;
/// this larger cap also bounds host startup, IPC, and crash cleanup.
pub const HOST_PROCESS_TIMEOUT_MS: u64 = 5_000;

/// Authority a v1 component may negotiate. Later stages extend this set
/// rather than inventing a second plugin system (D-DX5-HOOK1 hybrid law).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ability {
    ReadTypes,
    ReadSymbols,
    ReadEffects,
    ReadSpans,
    ReadProvenance,
    EmitFinding,
    ProposeEdit,
}

impl Ability {
    pub fn as_str(self) -> &'static str {
        match self {
            Ability::ReadTypes => "read_types",
            Ability::ReadSymbols => "read_symbols",
            Ability::ReadEffects => "read_effects",
            Ability::ReadSpans => "read_spans",
            Ability::ReadProvenance => "read_provenance",
            Ability::EmitFinding => "emit_finding",
            Ability::ProposeEdit => "propose_edit",
        }
    }

    pub fn parse(s: &str) -> Option<Ability> {
        Some(match s {
            "read_types" => Ability::ReadTypes,
            "read_symbols" => Ability::ReadSymbols,
            "read_effects" => Ability::ReadEffects,
            "read_spans" => Ability::ReadSpans,
            "read_provenance" => Ability::ReadProvenance,
            "emit_finding" => Ability::EmitFinding,
            "propose_edit" => Ability::ProposeEdit,
            _ => return None,
        })
    }

    /// V1 floor: typed observation + findings/edits.
    pub fn v1_defaults() -> &'static [Ability] {
        &[
            Ability::ReadTypes,
            Ability::ReadSymbols,
            Ability::ReadEffects,
            Ability::ReadSpans,
            Ability::ReadProvenance,
            Ability::EmitFinding,
            Ability::ProposeEdit,
        ]
    }
}

/// Hard resource caps applied by the host (guest cannot raise them).
/// Numeric defaults are mirrored in `Prelude/CompilerExtension.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    /// Wasmtime fuel units granted per `analyze` call.
    pub max_fuel: u64,
    /// Max linear memory bytes per instance.
    pub max_memory_bytes: usize,
    /// Max table elements per instance.
    pub max_table_elements: usize,
    /// Max findings accepted in one response.
    pub max_findings: usize,
    /// Max proposed edits accepted in one response.
    pub max_edits: usize,
    /// Max raw response payload bytes.
    pub max_response_bytes: usize,
    /// Wall-clock budget for one `analyze` (ms). Host enforces via wasmtime
    /// epoch interruption; validation always checks the declared value.
    pub timeout_ms: u64,
}

impl ResourceLimits {
    pub const fn v1_defaults() -> Self {
        Self {
            max_fuel: 10_000_000,
            max_memory_bytes: 16 * 1024 * 1024,
            max_table_elements: 10_000,
            max_findings: 256,
            max_edits: 64,
            max_response_bytes: 256 * 1024,
            timeout_ms: 2_000,
        }
    }
}

/// Trust class for a loaded component. V1 admits only untrusted sandboxed
/// guests — no elevated host imports (no clock/random/fs/net/process), no
/// compiler mutation rights (deterministic sandbox / D-DX5-HOOK1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustPolicy {
    Untrusted,
}

impl TrustPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            TrustPolicy::Untrusted => "untrusted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "untrusted" => Some(TrustPolicy::Untrusted),
            _ => None,
        }
    }
}

/// Protocol / validation failure — Jet-owned; never a rustc message (I2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    pub message: String,
}

impl ProtocolError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Negotiate abilities against the v1 allowlist + protocol version.
pub fn negotiate_abilities(
    protocol: u32,
    requested: &[Ability],
) -> Result<Vec<Ability>, ProtocolError> {
    if protocol != PROTOCOL_VERSION {
        return Err(ProtocolError::new(format!(
            "compiler-extension protocol mismatch: host={PROTOCOL_VERSION}, guest={protocol}"
        )));
    }
    let allow: BTreeSet<_> = Ability::v1_defaults().iter().copied().collect();
    let mut granted = BTreeSet::new();
    for cap in requested {
        if !allow.contains(cap) {
            return Err(ProtocolError::new(format!(
                "ability `{}` is not in the v1 allowlist",
                cap.as_str()
            )));
        }
        granted.insert(*cap);
    }
    Ok(granted.into_iter().collect())
}

/// Span fact visible to the guest (read-only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanFact {
    pub id: String,
    pub file: String,
    pub start: u32,
    pub end: u32,
}

/// Structural interface shape carried by an existing `TypeFact` identity.
/// This is metadata for copied boundary payloads, not a second wire type
/// system; nested entries refer back to the same `types` identity set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeFactShape {
    Opaque,
    Scalar,
    CodableRecord { fields: Vec<TypeFieldFact> },
    List { element_type_id: String },
    Option { inner_type_id: String },
    Result {
        ok_type_id: String,
        error_type_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeFieldFact {
    pub name: String,
    pub type_id: String,
}

/// Type fact (read-only). `id` remains the interface snapshot identity used
/// by symbols and host imports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeFact {
    pub id: String,
    pub repr: String,
    pub shape: TypeFactShape,
}

/// Symbol fact carrying effects + provenance (read-only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolFact {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub type_id: String,
    pub span_id: String,
    pub effects: Vec<String>,
    pub provenance: String,
}

/// Deterministic typed post-sema snapshot (host → guest).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedSnapshot {
    pub protocol: u32,
    pub stage: String,
    pub abilities: Vec<Ability>,
    pub limits: ResourceLimits,
    pub trust: TrustPolicy,
    pub types: Vec<TypeFact>,
    pub symbols: Vec<SymbolFact>,
    pub spans: Vec<SpanFact>,
    /// Host imports use this same snapshot's type identities for copied
    /// payloads; no parallel interface registry is permitted.
    pub host_imports: Vec<HostImportFact>,
}

impl TypedSnapshot {
    /// Build a v1 snapshot with no host imports.
    pub fn new(
        abilities: Vec<Ability>,
        types: Vec<TypeFact>,
        symbols: Vec<SymbolFact>,
        spans: Vec<SpanFact>,
    ) -> Result<Self, ProtocolError> {
        Self::new_with_imports(abilities, types, symbols, spans, Vec::new())
    }

    /// Build a snapshot with structured host-import facts. All facts are
    /// sorted by identity before encode for byte-stable replay.
    pub fn new_with_imports(
        abilities: Vec<Ability>,
        types: Vec<TypeFact>,
        symbols: Vec<SymbolFact>,
        spans: Vec<SpanFact>,
        host_imports: Vec<HostImportFact>,
    ) -> Result<Self, ProtocolError> {
        let abilities = negotiate_abilities(PROTOCOL_VERSION, &abilities)?;
        let mut types = types;
        let mut symbols = symbols;
        let mut spans = spans;
        let mut host_imports = host_imports;
        types.sort_by(|a, b| a.id.cmp(&b.id));
        symbols.sort_by(|a, b| a.id.cmp(&b.id));
        spans.sort_by(|a, b| a.id.cmp(&b.id));
        host_imports.sort_by(|a, b| a.id.cmp(&b.id));
        validate_interface_facts(&types, &symbols, &spans, &host_imports)?;
        Ok(Self {
            protocol: PROTOCOL_VERSION,
            stage: STAGE.to_string(),
            abilities,
            limits: ResourceLimits::v1_defaults(),
            trust: TrustPolicy::Untrusted,
            types,
            symbols,
            spans,
            host_imports,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        if self.protocol != PROTOCOL_VERSION {
            return Err(ProtocolError::new(format!(
                "snapshot protocol must be {PROTOCOL_VERSION}"
            )));
        }
        if self.stage != STAGE {
            return Err(ProtocolError::new(format!(
                "snapshot stage must be `{STAGE}` in v1"
            )));
        }
        if self.trust != TrustPolicy::Untrusted {
            return Err(ProtocolError::new(
                "v1 trust policy admits only `untrusted` components",
            ));
        }
        let caps: Vec<String> = self
            .abilities
            .iter()
            .map(|c| c.as_str().to_string())
            .collect();
        let mut obj = BTreeMap::new();
        obj.insert(
            "abilities".into(),
            DataTree::Array(caps.into_iter().map(DataTree::Text).collect()),
        );
        obj.insert("limits".into(), limits_to_json(&self.limits));
        obj.insert(
            "host_imports".into(),
            DataTree::Array(
                self.host_imports
                    .iter()
                    .map(host_import_to_json)
                    .collect(),
            ),
        );
        obj.insert("protocol".into(), DataTree::Int(self.protocol as i64));
        obj.insert(
            "spans".into(),
            DataTree::Array(self.spans.iter().map(span_to_json).collect()),
        );
        obj.insert("stage".into(), DataTree::Text(self.stage.clone()));
        obj.insert(
            "symbols".into(),
            DataTree::Array(self.symbols.iter().map(symbol_to_json).collect()),
        );
        obj.insert(
            "trust".into(),
            DataTree::Text(self.trust.as_str().to_string()),
        );
        obj.insert(
            "types".into(),
            DataTree::Array(self.types.iter().map(type_to_json).collect()),
        );
        Ok(stringify(&object(obj)).into_bytes())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| ProtocolError::new("snapshot is not valid UTF-8"))?;
        let root = JSON::parse(text).map_err(ProtocolError::new)?;
        let obj = object_map(&root)
            .map_err(|e| ProtocolError::new(format!("snapshot root: {e}")))?;
        require_keys_with_optional(
            &obj,
            &[
                "protocol",
                "stage",
                "abilities",
                "limits",
                "trust",
                "types",
                "symbols",
                "spans",
            ],
            &["host_imports"],
        )?;
        let protocol = json_u32(obj.get("protocol").unwrap(), "protocol")?;
        if protocol != PROTOCOL_VERSION {
            return Err(ProtocolError::new(format!(
                "compiler-extension protocol mismatch: host={PROTOCOL_VERSION}, snapshot={protocol}"
            )));
        }
        let stage = obj
            .get("stage")
            .unwrap()
            .as_str()
            .map_err(ProtocolError::new)?;
        if stage != STAGE {
            return Err(ProtocolError::new(format!(
                "snapshot stage must be `{STAGE}`, got `{stage}`"
            )));
        }
        let trust_s = obj
            .get("trust")
            .unwrap()
            .as_str()
            .map_err(ProtocolError::new)?;
        let trust = TrustPolicy::parse(trust_s)
            .ok_or_else(|| ProtocolError::new(format!("unknown trust policy `{trust_s}`")))?;
        let caps = parse_abilities(obj.get("abilities").unwrap())?;
        let limits = limits_from_json(obj.get("limits").unwrap())?;
        let types = parse_types(obj.get("types").unwrap())?;
        let symbols = parse_symbols(obj.get("symbols").unwrap())?;
        let spans = parse_spans(obj.get("spans").unwrap())?;
        let host_imports = obj
            .get("host_imports")
            .map(parse_host_imports)
            .transpose()?
            .unwrap_or_default();
        validate_interface_facts(&types, &symbols, &spans, &host_imports)?;
        Ok(Self {
            protocol,
            stage: stage.to_string(),
            abilities: caps,
            limits,
            trust,
            types,
            symbols,
            spans,
            host_imports,
        })
    }

    fn span_ids(&self) -> BTreeSet<&str> {
        self.spans.iter().map(|s| s.id.as_str()).collect()
    }

    fn type_ids(&self) -> BTreeSet<&str> {
        self.types.iter().map(|t| t.id.as_str()).collect()
    }
}

/// One validated finding proposed by the guest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: String,
    pub span_id: String,
    pub message: String,
    pub severity: String,
}

/// One validated edit proposal (host applies only after accept).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedEdit {
    pub span_id: String,
    pub replacement: String,
    pub rationale: String,
}

/// Guest → host analyze response (pre-validation shape after decode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzeResponse {
    pub protocol: u32,
    pub findings: Vec<Finding>,
    pub proposed_edits: Vec<ProposedEdit>,
    /// V1 requires this empty; non-empty is a protocol error.
    pub artifacts: Vec<String>,
}

impl AnalyzeResponse {
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut obj = BTreeMap::new();
        obj.insert(
            "artifacts".into(),
            DataTree::Array(
                self.artifacts
                    .iter()
                    .cloned()
                    .map(DataTree::Text)
                    .collect(),
            ),
        );
        obj.insert(
            "findings".into(),
            DataTree::Array(self.findings.iter().map(finding_to_json).collect()),
        );
        obj.insert("protocol".into(), DataTree::Int(self.protocol as i64));
        obj.insert(
            "proposed_edits".into(),
            DataTree::Array(self.proposed_edits.iter().map(edit_to_json).collect()),
        );
        Ok(stringify(&object(obj)).into_bytes())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| ProtocolError::new("response is not valid UTF-8"))?;
        let root = JSON::parse(text).map_err(ProtocolError::new)?;
        let obj = object_map(&root)
            .map_err(|e| ProtocolError::new(format!("response root: {e}")))?;
        require_keys(
            &obj,
            &["protocol", "findings", "proposed_edits", "artifacts"],
        )?;
        let protocol = json_u32(obj.get("protocol").unwrap(), "protocol")?;
        let findings = parse_findings(obj.get("findings").unwrap())?;
        let proposed_edits = parse_edits(obj.get("proposed_edits").unwrap())?;
        let artifacts = parse_string_array(obj.get("artifacts").unwrap(), "artifacts")?;
        Ok(Self {
            protocol,
            findings,
            proposed_edits,
            artifacts,
        })
    }
}

/// Validate a decoded response against the snapshot, granted abilities,
/// and resource limits. Successful validation does **not** mutate compiler
/// state — the host must explicitly accept staged findings/edits.
pub fn validate_response(
    snapshot: &TypedSnapshot,
    response: &AnalyzeResponse,
    raw_len: usize,
) -> Result<(), ProtocolError> {
    if response.protocol != PROTOCOL_VERSION {
        return Err(ProtocolError::new(format!(
            "response protocol must be {PROTOCOL_VERSION}, got {}",
            response.protocol
        )));
    }
    if raw_len > snapshot.limits.max_response_bytes {
        return Err(ProtocolError::new(format!(
            "response exceeds max_response_bytes ({} > {})",
            raw_len, snapshot.limits.max_response_bytes
        )));
    }
    if !response.artifacts.is_empty() {
        return Err(ProtocolError::new(
            "v1 responses must leave `artifacts` empty (no artifact mutation)",
        ));
    }
    if response.findings.len() > snapshot.limits.max_findings {
        return Err(ProtocolError::new(format!(
            "too many findings ({} > {})",
            response.findings.len(),
            snapshot.limits.max_findings
        )));
    }
    if response.proposed_edits.len() > snapshot.limits.max_edits {
        return Err(ProtocolError::new(format!(
            "too many proposed_edits ({} > {})",
            response.proposed_edits.len(),
            snapshot.limits.max_edits
        )));
    }
    let caps: BTreeSet<_> = snapshot.abilities.iter().copied().collect();
    if !response.findings.is_empty() && !caps.contains(&Ability::EmitFinding) {
        return Err(ProtocolError::new(
            "findings require ability `emit_finding`",
        ));
    }
    if !response.proposed_edits.is_empty() && !caps.contains(&Ability::ProposeEdit) {
        return Err(ProtocolError::new(
            "proposed_edits require ability `propose_edit`",
        ));
    }
    let span_ids = snapshot.span_ids();
    let type_ids = snapshot.type_ids();
    // Snapshot internal consistency: symbol refs must resolve when those
    // read abilities were granted.
    if caps.contains(&Ability::ReadSymbols) {
        for sym in &snapshot.symbols {
            if caps.contains(&Ability::ReadTypes) && !type_ids.contains(sym.type_id.as_str()) {
                return Err(ProtocolError::new(format!(
                    "symbol `{}` references unknown type_id `{}`",
                    sym.id, sym.type_id
                )));
            }
            if caps.contains(&Ability::ReadSpans) && !span_ids.contains(sym.span_id.as_str()) {
                return Err(ProtocolError::new(format!(
                    "symbol `{}` references unknown span_id `{}`",
                    sym.id, sym.span_id
                )));
            }
        }
    }
    for f in &response.findings {
        if f.rule.is_empty() || f.message.is_empty() {
            return Err(ProtocolError::new(
                "finding `rule` and `message` must be non-empty",
            ));
        }
        if !matches!(f.severity.as_str(), "error" | "warning" | "note") {
            return Err(ProtocolError::new(format!(
                "finding severity must be error|warning|note, got `{}`",
                f.severity
            )));
        }
        if !span_ids.contains(f.span_id.as_str()) {
            return Err(ProtocolError::new(format!(
                "finding references unknown span_id `{}`",
                f.span_id
            )));
        }
    }
    for e in &response.proposed_edits {
        if e.rationale.is_empty() {
            return Err(ProtocolError::new(
                "proposed_edit `rationale` must be non-empty",
            ));
        }
        if !span_ids.contains(e.span_id.as_str()) {
            return Err(ProtocolError::new(format!(
                "proposed_edit references unknown span_id `{}`",
                e.span_id
            )));
        }
    }
    Ok(())
}

/// Decode + validate in one step (host analyze path).
pub fn decode_and_validate_response(
    snapshot: &TypedSnapshot,
    bytes: &[u8],
) -> Result<AnalyzeResponse, ProtocolError> {
    let response = AnalyzeResponse::decode(bytes)?;
    validate_response(snapshot, &response, bytes.len())?;
    Ok(response)
}

/// Host-side lifecycle for one extension instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPhase {
    Idle,
    Loaded,
    Closed,
}

/// Session state machine: staging is ephemeral; rollback discards staged only
/// (accept latch stays final); close invokes the guest closer so WASM memory
/// is dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionSession {
    phase: SessionPhase,
    handle: Option<u64>,
    staged: Option<AnalyzeResponse>,
    /// True only after host explicitly accepts staged output (never auto).
    committed: bool,
}

impl ExtensionSession {
    pub fn new() -> Self {
        Self {
            phase: SessionPhase::Idle,
            handle: None,
            staged: None,
            committed: false,
        }
    }

    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    pub fn is_committed(&self) -> bool {
        self.committed
    }

    pub fn staged(&self) -> Option<&AnalyzeResponse> {
        self.staged.as_ref()
    }

    pub fn on_loaded(&mut self, handle: u64) -> Result<(), ProtocolError> {
        if self.phase != SessionPhase::Idle {
            return Err(ProtocolError::new(
                "extension session can only load from Idle",
            ));
        }
        if handle == 0 {
            return Err(ProtocolError::new("handle 0 is the error sentinel"));
        }
        self.handle = Some(handle);
        self.phase = SessionPhase::Loaded;
        Ok(())
    }

    /// Validate guest bytes and stage them. Does not commit into compiler state.
    pub fn stage_response(
        &mut self,
        snapshot: &TypedSnapshot,
        bytes: &[u8],
    ) -> Result<&AnalyzeResponse, ProtocolError> {
        if self.phase != SessionPhase::Loaded {
            return Err(ProtocolError::new(
                "stage_response requires a Loaded session",
            ));
        }
        if self.committed {
            return Err(ProtocolError::new(
                "session already committed; open a new session",
            ));
        }
        let response = decode_and_validate_response(snapshot, bytes)?;
        self.staged = Some(response);
        Ok(self.staged.as_ref().unwrap())
    }

    /// Discard staged findings/edits only. Compiler facts unchanged.
    /// Does **not** clear the accept latch — after [`Self::accept_staged`],
    /// rollback cannot reopen staging; open a new session instead.
    pub fn rollback(&mut self) {
        self.staged = None;
    }

    /// Host-only accept of staged output. Still does not call rustc (I2/I3).
    pub fn accept_staged(&mut self) -> Result<AnalyzeResponse, ProtocolError> {
        if self.phase != SessionPhase::Loaded {
            return Err(ProtocolError::new("accept requires Loaded session"));
        }
        let staged = self
            .staged
            .take()
            .ok_or_else(|| ProtocolError::new("nothing staged to accept"))?;
        self.committed = true;
        Ok(staged)
    }

    /// Close the session and drop the guest via `close_guest(handle)`.
    ///
    /// Host path must pass the WASM runtime closer
    /// (`jet_compiler_extension_close` in `Prelude/CompilerExtension.rs`) so
    /// the guest `Store`/linear memory is freed. Returns whatever the closer
    /// reports (or `false` when no handle was live). Clears staged output;
    /// session ends in [`SessionPhase::Closed`].
    pub fn close(&mut self, close_guest: impl FnOnce(u64) -> bool) -> bool {
        let handle = self.handle.take();
        self.staged = None;
        self.committed = false;
        self.phase = SessionPhase::Closed;
        match handle {
            Some(h) => close_guest(h),
            None => false,
        }
    }
}

impl Default for ExtensionSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse `jet_compiler_extension_load` wire (`O:<handle>` / `E:<message>`).
/// Failures are Jet-owned strings — never rustc diagnostics (I2).
pub fn parse_load_result(wire: &str) -> Result<u64, ProtocolError> {
    if let Some(rest) = wire.strip_prefix("O:") {
        rest.parse::<u64>().map_err(|_| {
            ProtocolError::new(format!("malformed compiler-extension load handle: {wire}"))
        })
    } else if let Some(msg) = wire.strip_prefix("E:") {
        Err(ProtocolError::new(msg.to_string()))
    } else {
        Err(ProtocolError::new(format!(
            "malformed compiler-extension load wire: {wire}"
        )))
    }
}

/// Parse `jet_compiler_extension_analyze` wire (`O:<len>:<hex>` / `E:<message>`).
pub fn parse_analyze_result(wire: &str) -> Result<Vec<u8>, ProtocolError> {
    if let Some(rest) = wire.strip_prefix("O:") {
        let (len_s, hex) = rest.split_once(':').ok_or_else(|| {
            ProtocolError::new(format!("malformed compiler-extension analyze wire: {wire}"))
        })?;
        let len: usize = len_s.parse().map_err(|_| {
            ProtocolError::new(format!(
                "malformed compiler-extension analyze length: {len_s}"
            ))
        })?;
        if hex.len() != len * 2 {
            return Err(ProtocolError::new(format!(
                "compiler-extension analyze hex length mismatch: expected {}, got {}",
                len * 2,
                hex.len()
            )));
        }
        let mut bytes = Vec::with_capacity(len);
        let chars: Vec<char> = hex.chars().collect();
        for chunk in chars.chunks(2) {
            let s: String = chunk.iter().collect();
            let b = u8::from_str_radix(&s, 16).map_err(|_| {
                ProtocolError::new(format!("malformed compiler-extension analyze hex: {s}"))
            })?;
            bytes.push(b);
        }
        Ok(bytes)
    } else if let Some(msg) = wire.strip_prefix("E:") {
        Err(ProtocolError::new(msg.to_string()))
    } else {
        Err(ProtocolError::new(format!(
            "malformed compiler-extension analyze wire: {wire}"
        )))
    }
}

/// True when a host/guest failure string looks like a leaked rustc diagnostic (I2).
pub fn message_exposes_rustc(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("rustc") || lower.contains("error[e") || lower.contains("--explain")
}

/// Load → analyze → validate/stage → accept → close through one
/// supplied Component Model host. `jetpack` supplies the real Wasmtime
/// functions; keeping orchestration here preserves one protocol/lifecycle.
pub fn analyze_with_host(
    wasm_path: &str,
    snapshot: &TypedSnapshot,
    load_guest: fn(&str) -> String,
    analyze_guest: fn(u64, &[u8]) -> String,
    close_guest: fn(u64) -> bool,
) -> Result<AnalyzeResponse, ProtocolError> {
    let wire = load_guest(wasm_path);
    let handle = match parse_load_result(&wire) {
        Ok(h) => h,
        Err(e) => {
            if message_exposes_rustc(&e.message) {
                return Err(ProtocolError::new(
                    "compiler-extension failed with an internal message",
                ));
            }
            return Err(e);
        }
    };
    let mut session = ExtensionSession::new();
    if let Err(e) = session.on_loaded(handle) {
        let _ = close_guest(handle);
        return Err(e);
    }

    let snap_bytes = match snapshot.encode() {
        Ok(b) => b,
        Err(e) => {
            let _ = session.close(close_guest);
            return Err(e);
        }
    };
    let analyze_wire = analyze_guest(handle, &snap_bytes);
    let raw = match parse_analyze_result(&analyze_wire) {
        Ok(b) => b,
        Err(e) => {
            let _ = session.close(close_guest);
            if message_exposes_rustc(&e.message) {
                return Err(ProtocolError::new(
                    "compiler-extension failed with an internal message",
                ));
            }
            return Err(e);
        }
    };
    if let Err(e) = session.stage_response(snapshot, &raw) {
        let _ = session.close(close_guest);
        return Err(e);
    }
    let accepted = match session.accept_staged() {
        Ok(r) => r,
        Err(e) => {
            let _ = session.close(close_guest);
            return Err(e);
        }
    };
    let _ = session.close(close_guest);
    Ok(accepted)
}

/// This host's mechanism identity.
pub fn mechanism() -> PluginMechanism {
    PluginMechanism::CompilerExtension
}

/// Same wasmtime crate pin as application `core.plugin` (D-DEP-WASM1=A).
/// Compiler-extension host must not invent a second loader dependency.
pub fn wasm_substrate_crate_spec() -> (&'static str, &'static str) {
    WASMTIME_CRATE_SPEC
}

/// Hand-written wasmtime Component Model host runtime (include_str substrate,
/// same ownership pattern as `Prelude/Plugin.rs` for application plugins).
pub fn runtime_source() -> &'static str {
    include_str!("Prelude/CompilerExtension.rs")
}

/// Canonical `.wit` world text for a compiler-extension component.
/// Snapshot and response travel as `list<u8>` so the wire schema can version
/// independently of the WIT shape; the host validates decoded payloads.
pub fn wit_world() -> String {
    format!(
        "package {PACKAGE_NAME};\n\n\
         world {WORLD_NAME} {{\n\
         \t/// Versioned typed post-sema snapshot bytes (host-owned schema).\n\
         \texport {ANALYZE_EXPORT}: func(snapshot: list<u8>) -> list<u8>;\n\
         }}\n"
    )
}

/// True when `world` names the application-plugin world, not this host.
pub fn is_application_plugin_world(world: &str) -> bool {
    world == APPLICATION_PLUGIN_WORLD
}

/// True when `world` names this compiler-extension world.
pub fn is_compiler_extension_world(world: &str) -> bool {
    world == WORLD_NAME
}

// ── DataTree helpers (deterministic stringify; BTreeMap key order) ────────────

fn object(map: BTreeMap<String, DataTree>) -> DataTree {
    DataTree::Object(map.into_iter().collect())
}

fn object_map(value: &DataTree) -> Result<BTreeMap<String, DataTree>, String> {
    let DataTree::Object(fields) = value else {
        return Err("expected a DataTree object".to_string());
    };
    Ok(fields.iter().cloned().collect())
}

fn stringify(v: &DataTree) -> String {
    match v {
        DataTree::Null => "null".into(),
        DataTree::Bool(true) => "true".into(),
        DataTree::Bool(false) => "false".into(),
        DataTree::Int(n) => n.to_string(),
        DataTree::Float(n) => format!("{n}"),
        DataTree::Number(text) => text.clone(),
        DataTree::TypedText(text) | DataTree::Text(text) => JSON::quote(text),
        DataTree::Bytes(values) => format!(
            "[{}]",
            values
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        DataTree::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&stringify(item));
            }
            out.push(']');
            out
        }
        DataTree::Object(map) => {
            let mut out = String::from("{");
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&JSON::quote(k));
                out.push(':');
                out.push_str(&stringify(v));
            }
            out.push('}');
            out
        }
    }
}

fn limits_to_json(l: &ResourceLimits) -> DataTree {
    let mut m = BTreeMap::new();
    m.insert("max_edits".into(), DataTree::Int(l.max_edits as i64));
    m.insert(
        "max_findings".into(),
        DataTree::Int(l.max_findings as i64),
    );
    m.insert("max_fuel".into(), DataTree::Int(l.max_fuel as i64));
    m.insert(
        "max_memory_bytes".into(),
        DataTree::Int(l.max_memory_bytes as i64),
    );
    m.insert(
        "max_response_bytes".into(),
        DataTree::Int(l.max_response_bytes as i64),
    );
    m.insert(
        "max_table_elements".into(),
        DataTree::Int(l.max_table_elements as i64),
    );
    m.insert("timeout_ms".into(), DataTree::Int(l.timeout_ms as i64));
    object(m)
}

fn limits_from_json(v: &DataTree) -> Result<ResourceLimits, ProtocolError> {
    let obj =
        object_map(v).map_err(|e| ProtocolError::new(format!("limits: {e}")))?;
    require_keys(
        &obj,
        &[
            "max_fuel",
            "max_memory_bytes",
            "max_table_elements",
            "max_findings",
            "max_edits",
            "max_response_bytes",
            "timeout_ms",
        ],
    )?;
    Ok(ResourceLimits {
        max_fuel: json_u64(obj.get("max_fuel").unwrap(), "max_fuel")?,
        max_memory_bytes: json_usize(obj.get("max_memory_bytes").unwrap(), "max_memory_bytes")?,
        max_table_elements: json_usize(
            obj.get("max_table_elements").unwrap(),
            "max_table_elements",
        )?,
        max_findings: json_usize(obj.get("max_findings").unwrap(), "max_findings")?,
        max_edits: json_usize(obj.get("max_edits").unwrap(), "max_edits")?,
        max_response_bytes: json_usize(
            obj.get("max_response_bytes").unwrap(),
            "max_response_bytes",
        )?,
        timeout_ms: json_u64(obj.get("timeout_ms").unwrap(), "timeout_ms")?,
    })
}

fn span_to_json(s: &SpanFact) -> DataTree {
    let mut m = BTreeMap::new();
    m.insert("end".into(), DataTree::Int(s.end as i64));
    m.insert("file".into(), DataTree::Text(s.file.clone()));
    m.insert("id".into(), DataTree::Text(s.id.clone()));
    m.insert("start".into(), DataTree::Int(s.start as i64));
    object(m)
}

fn type_to_json(t: &TypeFact) -> DataTree {
    let mut m = BTreeMap::new();
    m.insert("id".into(), DataTree::Text(t.id.clone()));
    m.insert("repr".into(), DataTree::Text(t.repr.clone()));
    m.insert("shape".into(), type_shape_to_json(&t.shape));
    object(m)
}

fn type_shape_to_json(shape: &TypeFactShape) -> DataTree {
    let mut m = BTreeMap::new();
    match shape {
        TypeFactShape::Opaque => {
            m.insert("kind".into(), DataTree::Text("opaque".into()));
        }
        TypeFactShape::Scalar => {
            m.insert("kind".into(), DataTree::Text("scalar".into()));
        }
        TypeFactShape::CodableRecord { fields } => {
            m.insert("kind".into(), DataTree::Text("codable_record".into()));
            m.insert(
                "fields".into(),
                DataTree::Array(
                    fields
                        .iter()
                        .map(|field| {
                            let mut value = BTreeMap::new();
                            value.insert("name".into(), DataTree::Text(field.name.clone()));
                            value.insert(
                                "type_id".into(),
                                DataTree::Text(field.type_id.clone()),
                            );
                            object(value)
                        })
                        .collect(),
                ),
            );
        }
        TypeFactShape::List { element_type_id } => {
            m.insert("element_type_id".into(), DataTree::Text(element_type_id.clone()));
            m.insert("kind".into(), DataTree::Text("list".into()));
        }
        TypeFactShape::Option { inner_type_id } => {
            m.insert("inner_type_id".into(), DataTree::Text(inner_type_id.clone()));
            m.insert("kind".into(), DataTree::Text("option".into()));
        }
        TypeFactShape::Result {
            ok_type_id,
            error_type_id,
        } => {
            m.insert("error_type_id".into(), DataTree::Text(error_type_id.clone()));
            m.insert("kind".into(), DataTree::Text("result".into()));
            m.insert("ok_type_id".into(), DataTree::Text(ok_type_id.clone()));
        }
    }
    object(m)
}

fn host_import_to_json(import: &HostImportFact) -> DataTree {
    let mut m = BTreeMap::new();
    m.insert("id".into(), DataTree::Text(import.id.clone()));
    m.insert(
        "operation".into(),
        DataTree::Text(import.operation.clone()),
    );
    m.insert(
        "parameter_type_ids".into(),
        DataTree::Array(
            import
                .parameter_type_ids
                .iter()
                .cloned()
                .map(DataTree::Text)
                .collect(),
        ),
    );
    m.insert(
        "required_right".into(),
        DataTree::Text(import.required_right.clone()),
    );
    m.insert(
        "result_type_id".into(),
        import
            .result_type_id
            .as_ref()
            .map_or(DataTree::Null, |id| DataTree::Text(id.clone())),
    );
    object(m)
}

fn symbol_to_json(s: &SymbolFact) -> DataTree {
    let mut m = BTreeMap::new();
    m.insert(
        "effects".into(),
        DataTree::Array(s.effects.iter().cloned().map(DataTree::Text).collect()),
    );
    m.insert("id".into(), DataTree::Text(s.id.clone()));
    m.insert("kind".into(), DataTree::Text(s.kind.clone()));
    m.insert("name".into(), DataTree::Text(s.name.clone()));
    m.insert("provenance".into(), DataTree::Text(s.provenance.clone()));
    m.insert("span_id".into(), DataTree::Text(s.span_id.clone()));
    m.insert("type_id".into(), DataTree::Text(s.type_id.clone()));
    object(m)
}

fn finding_to_json(f: &Finding) -> DataTree {
    let mut m = BTreeMap::new();
    m.insert("message".into(), DataTree::Text(f.message.clone()));
    m.insert("rule".into(), DataTree::Text(f.rule.clone()));
    m.insert("severity".into(), DataTree::Text(f.severity.clone()));
    m.insert("span_id".into(), DataTree::Text(f.span_id.clone()));
    object(m)
}

fn edit_to_json(e: &ProposedEdit) -> DataTree {
    let mut m = BTreeMap::new();
    m.insert("rationale".into(), DataTree::Text(e.rationale.clone()));
    m.insert(
        "replacement".into(),
        DataTree::Text(e.replacement.clone()),
    );
    m.insert("span_id".into(), DataTree::Text(e.span_id.clone()));
    object(m)
}

fn require_keys(obj: &BTreeMap<String, DataTree>, keys: &[&str]) -> Result<(), ProtocolError> {
    for k in keys {
        if !obj.contains_key(*k) {
            return Err(ProtocolError::new(format!("missing key `{k}`")));
        }
    }
    // Exact schema: reject unknown keys so wire shape stays frozen.
    for k in obj.keys() {
        if !keys.contains(&k.as_str()) {
            return Err(ProtocolError::new(format!("unknown key `{k}`")));
        }
    }
    Ok(())
}

fn require_keys_with_optional(
    obj: &BTreeMap<String, DataTree>,
    required: &[&str],
    optional: &[&str],
) -> Result<(), ProtocolError> {
    for key in required {
        if !obj.contains_key(*key) {
            return Err(ProtocolError::new(format!("missing key `{key}`")));
        }
    }
    for key in obj.keys() {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            return Err(ProtocolError::new(format!("unknown key `{key}`")));
        }
    }
    Ok(())
}

fn json_u32(v: &DataTree, name: &str) -> Result<u32, ProtocolError> {
    match v {
        DataTree::Int(n) if *n >= 0 && *n <= u32::MAX as i64 => Ok(*n as u32),
        DataTree::Float(n) if n.fract() == 0.0 && *n >= 0.0 && *n <= u32::MAX as f64 => {
            Ok(*n as u32)
        }
        _ => Err(ProtocolError::new(format!("`{name}` must be a u32"))),
    }
}

fn json_u64(v: &DataTree, name: &str) -> Result<u64, ProtocolError> {
    match v {
        DataTree::Int(n) if *n >= 0 => Ok(*n as u64),
        DataTree::Float(n) if n.fract() == 0.0 && *n >= 0.0 && *n <= (u64::MAX as f64) => {
            Ok(*n as u64)
        }
        _ => Err(ProtocolError::new(format!("`{name}` must be a u64"))),
    }
}

fn json_usize(v: &DataTree, name: &str) -> Result<usize, ProtocolError> {
    let n = json_u64(v, name)?;
    usize::try_from(n).map_err(|_| ProtocolError::new(format!("`{name}` out of range")))
}

fn parse_abilities(v: &DataTree) -> Result<Vec<Ability>, ProtocolError> {
    let arr = v
        .as_array()
        .map_err(|e| ProtocolError::new(format!("abilities: {e}")))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let s = item.as_str().map_err(ProtocolError::new)?;
        let cap = Ability::parse(s)
            .ok_or_else(|| ProtocolError::new(format!("unknown ability `{s}`")))?;
        out.push(cap);
    }
    out.sort();
    out.dedup();
    negotiate_abilities(PROTOCOL_VERSION, &out)
}

fn parse_string_array(v: &DataTree, name: &str) -> Result<Vec<String>, ProtocolError> {
    let arr = v
        .as_array()
        .map_err(|e| ProtocolError::new(format!("{name}: {e}")))?;
    arr.iter()
        .map(|item| {
            item.as_str()
                .map(|s| s.to_string())
                .map_err(ProtocolError::new)
        })
        .collect()
}

fn parse_types(v: &DataTree) -> Result<Vec<TypeFact>, ProtocolError> {
    let arr = v
        .as_array()
        .map_err(|e| ProtocolError::new(format!("types: {e}")))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let obj = object_map(item).map_err(|e| ProtocolError::new(format!("type: {e}")))?;
        require_keys_with_optional(&obj, &["id", "repr"], &["shape"])?;
        let shape = obj
            .get("shape")
            .map(parse_type_shape)
            .transpose()?
            .unwrap_or(TypeFactShape::Opaque);
        out.push(TypeFact {
            id: obj
                .get("id")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            repr: obj
                .get("repr")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            shape,
        });
    }
    Ok(out)
}

fn parse_type_shape(v: &DataTree) -> Result<TypeFactShape, ProtocolError> {
    let obj = object_map(v).map_err(|e| ProtocolError::new(format!("type shape: {e}")))?;
    let kind = obj
        .get("kind")
        .ok_or_else(|| ProtocolError::new("type shape is missing `kind`"))?
        .as_str()
        .map_err(ProtocolError::new)?;
    match kind {
        "opaque" => {
            require_keys(&obj, &["kind"])?;
            Ok(TypeFactShape::Opaque)
        }
        "scalar" => {
            require_keys(&obj, &["kind"])?;
            Ok(TypeFactShape::Scalar)
        }
        "codable_record" => {
            require_keys(&obj, &["kind", "fields"])?;
            let fields = obj
                .get("fields")
                .unwrap()
                .as_array()
                .map_err(|e| ProtocolError::new(format!("record fields: {e}")))?;
            let mut out = Vec::with_capacity(fields.len());
            for field in fields {
                let field =
                    object_map(field).map_err(|e| ProtocolError::new(format!("record field: {e}")))?;
                require_keys(&field, &["name", "type_id"])?;
                out.push(TypeFieldFact {
                    name: field
                        .get("name")
                        .unwrap()
                        .as_str()
                        .map_err(ProtocolError::new)?
                        .to_string(),
                    type_id: field
                        .get("type_id")
                        .unwrap()
                        .as_str()
                        .map_err(ProtocolError::new)?
                        .to_string(),
                });
            }
            Ok(TypeFactShape::CodableRecord { fields: out })
        }
        "list" => {
            require_keys(&obj, &["kind", "element_type_id"])?;
            Ok(TypeFactShape::List {
                element_type_id: obj
                    .get("element_type_id")
                    .unwrap()
                    .as_str()
                    .map_err(ProtocolError::new)?
                    .to_string(),
            })
        }
        "option" => {
            require_keys(&obj, &["kind", "inner_type_id"])?;
            Ok(TypeFactShape::Option {
                inner_type_id: obj
                    .get("inner_type_id")
                    .unwrap()
                    .as_str()
                    .map_err(ProtocolError::new)?
                    .to_string(),
            })
        }
        "result" => {
            require_keys(&obj, &["kind", "ok_type_id", "error_type_id"])?;
            Ok(TypeFactShape::Result {
                ok_type_id: obj
                    .get("ok_type_id")
                    .unwrap()
                    .as_str()
                    .map_err(ProtocolError::new)?
                    .to_string(),
                error_type_id: obj
                    .get("error_type_id")
                    .unwrap()
                    .as_str()
                    .map_err(ProtocolError::new)?
                    .to_string(),
            })
        }
        other => Err(ProtocolError::new(format!(
            "unknown type shape `{other}`"
        ))),
    }
}

fn parse_host_imports(v: &DataTree) -> Result<Vec<HostImportFact>, ProtocolError> {
    let arr = v
        .as_array()
        .map_err(|e| ProtocolError::new(format!("host_imports: {e}")))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let obj = object_map(item).map_err(|e| ProtocolError::new(format!("host import: {e}")))?;
        require_keys(
            &obj,
            &[
                "id",
                "operation",
                "parameter_type_ids",
                "required_right",
                "result_type_id",
            ],
        )?;
        let result_type_id = match obj.get("result_type_id").unwrap() {
            DataTree::Null => None,
            value => Some(value.as_str().map_err(ProtocolError::new)?.to_string()),
        };
        out.push(HostImportFact {
            id: obj
                .get("id")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            operation: obj
                .get("operation")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            parameter_type_ids: parse_string_array(
                obj.get("parameter_type_ids").unwrap(),
                "parameter_type_ids",
            )?,
            required_right: obj
                .get("required_right")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            result_type_id,
        });
    }
    Ok(out)
}

fn validate_interface_facts(
    types: &[TypeFact],
    symbols: &[SymbolFact],
    spans: &[SpanFact],
    host_imports: &[HostImportFact],
) -> Result<(), ProtocolError> {
    let mut type_ids = BTreeSet::new();
    for fact in types {
        if fact.id.is_empty() || !type_ids.insert(fact.id.as_str()) {
            return Err(ProtocolError::new(format!(
                "duplicate or empty type identity `{}`",
                fact.id
            )));
        }
    }
    for fact in types {
        let referenced = |id: &str, label: &str| {
            if id.is_empty() || !type_ids.contains(id) {
                Err(ProtocolError::new(format!(
                    "type `{}` references unknown {label} `{id}`",
                    fact.id
                )))
            } else {
                Ok(())
            }
        };
        match &fact.shape {
            TypeFactShape::Opaque | TypeFactShape::Scalar => {}
            TypeFactShape::CodableRecord { fields } => {
                let mut field_names = BTreeSet::new();
                for field in fields {
                    if field.name.is_empty() || !field_names.insert(field.name.as_str()) {
                        return Err(ProtocolError::new(format!(
                            "type `{}` repeats or omits a record field name",
                            fact.id
                        )));
                    }
                    referenced(&field.type_id, "record field type")?;
                }
            }
            TypeFactShape::List { element_type_id } => {
                referenced(element_type_id, "list element type")?;
            }
            TypeFactShape::Option { inner_type_id } => {
                referenced(inner_type_id, "option inner type")?;
            }
            TypeFactShape::Result {
                ok_type_id,
                error_type_id,
            } => {
                referenced(ok_type_id, "result success type")?;
                referenced(error_type_id, "result error type")?;
            }
        }
    }

    let mut span_ids = BTreeSet::new();
    for span in spans {
        if span.id.is_empty() || !span_ids.insert(span.id.as_str()) {
            return Err(ProtocolError::new(format!(
                "duplicate or empty span identity `{}`",
                span.id
            )));
        }
    }

    let mut symbol_ids = BTreeSet::new();
    for symbol in symbols {
        if symbol.id.is_empty() || !symbol_ids.insert(symbol.id.as_str()) {
            return Err(ProtocolError::new(format!(
                "duplicate or empty symbol identity `{}`",
                symbol.id
            )));
        }
        if !type_ids.contains(symbol.type_id.as_str()) {
            return Err(ProtocolError::new(format!(
                "symbol `{}` references unknown type `{}`",
                symbol.id, symbol.type_id
            )));
        }
        if !span_ids.contains(symbol.span_id.as_str()) {
            return Err(ProtocolError::new(format!(
                "symbol `{}` references unknown span `{}`",
                symbol.id, symbol.span_id
            )));
        }
    }

    let mut import_ids = BTreeSet::new();
    for import in host_imports {
        if import.id.is_empty() || !import_ids.insert(import.id.as_str()) {
            return Err(ProtocolError::new(format!(
                "duplicate or empty host-import identity `{}`",
                import.id
            )));
        }
        if jet_foundation::Authority::parse_right(&import.required_right).is_none() {
            return Err(ProtocolError::new(format!(
                "host import `{}` has unknown required right `{}`",
                import.id, import.required_right
            )));
        }
        for type_id in import.payload_type_ids() {
            if !type_ids.contains(type_id) {
                return Err(ProtocolError::new(format!(
                    "host import `{}` references unknown payload type `{type_id}`",
                    import.id
                )));
            }
        }
    }
    Ok(())
}

fn parse_spans(v: &DataTree) -> Result<Vec<SpanFact>, ProtocolError> {
    let arr = v
        .as_array()
        .map_err(|e| ProtocolError::new(format!("spans: {e}")))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let obj = object_map(item).map_err(|e| ProtocolError::new(format!("span: {e}")))?;
        require_keys(&obj, &["id", "file", "start", "end"])?;
        out.push(SpanFact {
            id: obj
                .get("id")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            file: obj
                .get("file")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            start: json_u32(obj.get("start").unwrap(), "start")?,
            end: json_u32(obj.get("end").unwrap(), "end")?,
        });
    }
    Ok(out)
}

fn parse_symbols(v: &DataTree) -> Result<Vec<SymbolFact>, ProtocolError> {
    let arr = v
        .as_array()
        .map_err(|e| ProtocolError::new(format!("symbols: {e}")))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let obj = object_map(item).map_err(|e| ProtocolError::new(format!("symbol: {e}")))?;
        require_keys(
            &obj,
            &[
                "id",
                "name",
                "kind",
                "type_id",
                "span_id",
                "effects",
                "provenance",
            ],
        )?;
        out.push(SymbolFact {
            id: obj
                .get("id")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            name: obj
                .get("name")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            kind: obj
                .get("kind")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            type_id: obj
                .get("type_id")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            span_id: obj
                .get("span_id")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            effects: parse_string_array(obj.get("effects").unwrap(), "effects")?,
            provenance: obj
                .get("provenance")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
        });
    }
    Ok(out)
}

fn parse_findings(v: &DataTree) -> Result<Vec<Finding>, ProtocolError> {
    let arr = v
        .as_array()
        .map_err(|e| ProtocolError::new(format!("findings: {e}")))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let obj = object_map(item).map_err(|e| ProtocolError::new(format!("finding: {e}")))?;
        require_keys(&obj, &["rule", "span_id", "message", "severity"])?;
        out.push(Finding {
            rule: obj
                .get("rule")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            span_id: obj
                .get("span_id")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            message: obj
                .get("message")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            severity: obj
                .get("severity")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
        });
    }
    Ok(out)
}

fn parse_edits(v: &DataTree) -> Result<Vec<ProposedEdit>, ProtocolError> {
    let arr = v
        .as_array()
        .map_err(|e| ProtocolError::new(format!("proposed_edits: {e}")))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let obj = object_map(item).map_err(|e| ProtocolError::new(format!("proposed_edit: {e}")))?;
        require_keys(&obj, &["span_id", "replacement", "rationale"])?;
        out.push(ProposedEdit {
            span_id: obj
                .get("span_id")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            replacement: obj
                .get("replacement")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
            rationale: obj
                .get("rationale")
                .unwrap()
                .as_str()
                .map_err(ProtocolError::new)?
                .to_string(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_snapshot() -> TypedSnapshot {
        TypedSnapshot::new(
            Ability::v1_defaults().to_vec(),
            vec![TypeFact {
                id: "t1".into(),
                repr: "Int".into(),
                shape: TypeFactShape::Scalar,
            }],
            vec![SymbolFact {
                id: "s1".into(),
                name: "x".into(),
                kind: "let".into(),
                type_id: "t1".into(),
                span_id: "sp1".into(),
                effects: vec!["pure".into()],
                provenance: "sema".into(),
            }],
            vec![SpanFact {
                id: "sp1".into(),
                file: "main.jet".into(),
                start: 10,
                end: 11,
            }],
        )
        .unwrap()
    }
    #[test]
    fn structured_types_and_host_imports_roundtrip_as_one_snapshot() {
        let snapshot = TypedSnapshot::new_with_imports(
            Ability::v1_defaults().to_vec(),
            vec![
                TypeFact {
                    id: "t-list".into(),
                    repr: "[Int]".into(),
                    shape: TypeFactShape::List {
                        element_type_id: "t-int".into(),
                    },
                },
                TypeFact {
                    id: "t-int".into(),
                    repr: "Int".into(),
                    shape: TypeFactShape::Scalar,
                },
            ],
            Vec::new(),
            Vec::new(),
            vec![HostImportFact::new(
                "host-read",
                "read_file",
                "FS.Read",
                vec!["t-list".into()],
                Some("t-int".into()),
            )],
        )
        .expect("valid structured snapshot");
        let decoded = TypedSnapshot::decode(&snapshot.encode().unwrap()).unwrap();
        assert_eq!(decoded.types, snapshot.types);
        assert_eq!(decoded.host_imports, snapshot.host_imports);
    }


    #[test]
    fn dx5_hook1_world_is_distinct_from_application_plugin_and_path_helpers() {
        assert_eq!(mechanism(), PluginMechanism::CompilerExtension);
        assert_ne!(WORLD_NAME, APPLICATION_PLUGIN_WORLD);
        assert!(is_compiler_extension_world(WORLD_NAME));
        assert!(is_application_plugin_world(APPLICATION_PLUGIN_WORLD));
        assert!(!is_compiler_extension_world(APPLICATION_PLUGIN_WORLD));
        assert!(!is_application_plugin_world(WORLD_NAME));
        assert_ne!(mechanism(), PluginMechanism::PathHelper);
        assert_ne!(mechanism(), PluginMechanism::ApplicationPlugin);
    }

    #[test]
    fn host_reuses_jet_pkg_model_wasmtime_substrate() {
        assert_eq!(wasm_substrate_crate_spec(), WASMTIME_CRATE_SPEC);
        assert_eq!(wasm_substrate_crate_spec(), ("wasmtime", "25"));
        let runtime = runtime_source();
        assert!(
            runtime.contains("wasmtime::component"),
            "compiler-extension host must use the Component Model substrate"
        );
        assert!(
            runtime.contains("jet_compiler_extension_load"),
            "host entry must be compiler-extension-specific"
        );
        assert!(
            !runtime.contains("jet_plugin_load"),
            "must not reuse application core.plugin entry points"
        );
        assert!(
            runtime.contains(WORLD_NAME),
            "runtime must name the compiler-extension world"
        );
        assert!(
            !runtime.contains("\"jetplugin\""),
            "runtime must not bind the application plugin world name"
        );
        assert!(
            runtime.contains("consume_fuel"),
            "runtime must apply fuel limits"
        );
        assert!(
            runtime.contains("epoch_interruption"),
            "runtime must enable wall-clock epoch interruption"
        );
        assert!(
            runtime.contains("set_epoch_deadline") && runtime.contains("increment_epoch"),
            "runtime must arm and tick the epoch deadline on analyze"
        );
        assert!(
            runtime.contains("StoreLimitsBuilder"),
            "runtime must apply memory/table StoreLimits"
        );
        assert!(
            runtime.contains("Linker::new")
                && runtime.contains("no clock, random, filesystem, network, or process"),
            "runtime must deny host imports under the deterministic sandbox law"
        );
        assert!(
            !runtime.contains("wasi:")
                && !runtime.contains("add_to_linker")
                && !runtime.contains("WasiCtx"),
            "runtime must not wire WASI or other ambient host imports"
        );
        assert!(
            runtime.contains("10_000_000")
                && runtime.contains("16777216")
                && runtime.contains("2_000"),
            "runtime defaults must mirror ResourceLimits::v1_defaults"
        );
    }

    #[test]
    fn v1_wit_world_and_protocol_match_dx5_hook1() {
        assert_eq!(PROTOCOL_VERSION, 1);
        assert_eq!(STAGE, "typed");
        let wit = wit_world();
        assert!(wit.contains(&format!("package {PACKAGE_NAME};")));
        assert!(wit.contains(&format!("world {WORLD_NAME}")));
        assert!(wit.contains(&format!("export {ANALYZE_EXPORT}:")));
        assert!(wit.contains("list<u8>"));
        assert!(!wit.contains("world jetplugin"));
        assert!(Ability::v1_defaults().contains(&Ability::ReadTypes));
        assert!(Ability::v1_defaults().contains(&Ability::EmitFinding));
        assert_eq!(Ability::ReadSymbols.as_str(), "read_symbols");
    }

    #[test]
    fn ability_negotiation_rejects_unknown_and_version_mismatch() {
        assert!(negotiate_abilities(2, &[Ability::ReadTypes]).is_err());
        let granted = negotiate_abilities(1, &[Ability::ReadTypes, Ability::EmitFinding]).unwrap();
        assert_eq!(granted, vec![Ability::ReadTypes, Ability::EmitFinding]);
    }

    #[test]
    fn snapshot_roundtrip_is_byte_deterministic() {
        let snap = sample_snapshot();
        let a = snap.encode().unwrap();
        let b = snap.encode().unwrap();
        assert_eq!(a, b);
        let text = std::str::from_utf8(&a).unwrap();
        assert!(!text.contains(' '));
        assert!(text.contains("\"effects\":[\"pure\"]"));
        assert!(text.contains("\"provenance\":\"sema\""));
        assert!(text.starts_with("{\"abilities\":["));
        let decoded = TypedSnapshot::decode(&a).unwrap();
        assert_eq!(decoded, snap);
        // Re-encode of decoded must match original bytes (stable order).
        assert_eq!(decoded.encode().unwrap(), a);
    }

    #[test]
    fn typed_snapshot_decode_rejects_nested_and_oversized_json_before_conversion() {
        let nested = format!(
            "{{\"abilities\":[],\"limits\":{{\"max_edits\":64,\"max_findings\":256,\"max_fuel\":10000000,\"max_memory_bytes\":16777216,\"max_response_bytes\":262144,\"max_table_elements\":10000,\"timeout_ms\":2000}},\"protocol\":1,\"spans\":[],\"stage\":\"typed\",\"symbols\":[],\"trust\":\"untrusted\",\"types\":{nested}}}",
            nested = format!(
                "{}0{}",
                "[".repeat(JSON::MAX_JSON_DEPTH + 1),
                "]".repeat(JSON::MAX_JSON_DEPTH + 1)
            )
        );
        let error = TypedSnapshot::decode(nested.as_bytes())
            .expect_err("nested snapshot JSON must hit the shared parser depth cap");
        assert_eq!(error.message, "JSON exceeds maximum nesting depth");

        let snapshot = sample_snapshot();
        let valid = snapshot
            .encode()
            .expect("sample snapshot must produce valid protocol JSON");
        assert_eq!(
            TypedSnapshot::decode(&valid).expect("valid snapshot protocol must decode"),
            snapshot
        );

        let mut oversized = valid;
        oversized.resize(JSON::MAX_PROTOCOL_MESSAGE_BYTES + 1, b' ');
        let error = TypedSnapshot::decode(&oversized)
            .expect_err("oversized snapshot JSON must hit the shared parser size cap");
        assert_eq!(error.message, "JSON input exceeds the 1 MiB limit");
    }

    #[test]
    fn analyze_response_decode_rejects_nested_and_oversized_json_before_conversion() {
        let nested = format!(
            "{{\"artifacts\":[],\"findings\":{nested},\"protocol\":1,\"proposed_edits\":[]}}",
            nested = format!(
                "{}0{}",
                "[".repeat(JSON::MAX_JSON_DEPTH + 1),
                "]".repeat(JSON::MAX_JSON_DEPTH + 1)
            )
        );
        let error = AnalyzeResponse::decode(nested.as_bytes())
            .expect_err("nested response JSON must hit the shared parser depth cap");
        assert_eq!(error.message, "JSON exceeds maximum nesting depth");

        let response = AnalyzeResponse {
            protocol: PROTOCOL_VERSION,
            findings: vec![Finding {
                rule: "no-x".into(),
                span_id: "sp1".into(),
                message: "prefer y".into(),
                severity: "warning".into(),
            }],
            proposed_edits: vec![ProposedEdit {
                span_id: "sp1".into(),
                replacement: "y".into(),
                rationale: "rename".into(),
            }],
            artifacts: vec![],
        };
        let valid = response
            .encode()
            .expect("sample response must produce valid protocol JSON");
        assert_eq!(
            AnalyzeResponse::decode(&valid).expect("valid response protocol must decode"),
            response
        );

        let mut oversized = valid;
        oversized.resize(JSON::MAX_PROTOCOL_MESSAGE_BYTES + 1, b' ');
        let error = AnalyzeResponse::decode(&oversized)
            .expect_err("oversized response JSON must hit the shared parser size cap");
        assert_eq!(error.message, "JSON input exceeds the 1 MiB limit");
    }

    #[test]
    fn validate_accepts_in_span_findings_and_edits() {
        let snap = sample_snapshot();
        let response = AnalyzeResponse {
            protocol: 1,
            findings: vec![Finding {
                rule: "no-x".into(),
                span_id: "sp1".into(),
                message: "prefer y".into(),
                severity: "warning".into(),
            }],
            proposed_edits: vec![ProposedEdit {
                span_id: "sp1".into(),
                replacement: "y".into(),
                rationale: "rename".into(),
            }],
            artifacts: vec![],
        };
        let raw = response.encode().unwrap();
        validate_response(&snap, &response, raw.len()).unwrap();
        let again = decode_and_validate_response(&snap, &raw).unwrap();
        assert_eq!(again.findings.len(), 1);
    }

    #[test]
    fn validate_rejects_bad_span_artifacts_caps_and_limits() {
        let snap = sample_snapshot();
        let bad_span = AnalyzeResponse {
            protocol: 1,
            findings: vec![Finding {
                rule: "r".into(),
                span_id: "missing".into(),
                message: "m".into(),
                severity: "error".into(),
            }],
            proposed_edits: vec![],
            artifacts: vec![],
        };
        assert!(validate_response(&snap, &bad_span, 8).is_err());

        let with_artifact = AnalyzeResponse {
            protocol: 1,
            findings: vec![],
            proposed_edits: vec![],
            artifacts: vec!["obj.o".into()],
        };
        assert!(validate_response(&snap, &with_artifact, 8).is_err());

        let mut no_emit = sample_snapshot();
        no_emit.abilities.retain(|c| *c != Ability::EmitFinding);
        let finding = AnalyzeResponse {
            protocol: 1,
            findings: vec![Finding {
                rule: "r".into(),
                span_id: "sp1".into(),
                message: "m".into(),
                severity: "note".into(),
            }],
            proposed_edits: vec![],
            artifacts: vec![],
        };
        assert!(validate_response(&no_emit, &finding, 8).is_err());

        let mut tiny = sample_snapshot();
        tiny.limits.max_response_bytes = 4;
        assert!(validate_response(&tiny, &finding, 99).is_err());
    }

    #[test]
    fn session_lifecycle_stages_rollbacks_and_closes() {
        let snap = sample_snapshot();
        let response = AnalyzeResponse {
            protocol: 1,
            findings: vec![Finding {
                rule: "r".into(),
                span_id: "sp1".into(),
                message: "m".into(),
                severity: "warning".into(),
            }],
            proposed_edits: vec![],
            artifacts: vec![],
        };
        let raw = response.encode().unwrap();

        let mut session = ExtensionSession::new();
        assert_eq!(session.phase(), SessionPhase::Idle);
        session.on_loaded(7).unwrap();
        assert_eq!(session.phase(), SessionPhase::Loaded);
        session.stage_response(&snap, &raw).unwrap();
        assert!(session.staged().is_some());
        assert!(!session.is_committed());

        session.rollback();
        assert!(session.staged().is_none());
        assert!(!session.is_committed());

        session.stage_response(&snap, &raw).unwrap();
        let accepted = session.accept_staged().unwrap();
        assert_eq!(accepted.findings[0].rule, "r");
        assert!(session.is_committed());
        assert!(session.staged().is_none());

        let mut closed_handle = None;
        let dropped = session.close(|h| {
            closed_handle = Some(h);
            true // stand-in for jet_compiler_extension_close
        });
        assert!(dropped);
        assert_eq!(closed_handle, Some(7));
        assert_eq!(session.phase(), SessionPhase::Closed);
        assert!(session.stage_response(&snap, &raw).is_err());
    }

    #[test]
    fn accept_then_rollback_refuses_restage() {
        let snap = sample_snapshot();
        let response = AnalyzeResponse {
            protocol: 1,
            findings: vec![Finding {
                rule: "r".into(),
                span_id: "sp1".into(),
                message: "m".into(),
                severity: "warning".into(),
            }],
            proposed_edits: vec![],
            artifacts: vec![],
        };
        let raw = response.encode().unwrap();

        let mut session = ExtensionSession::new();
        session.on_loaded(3).unwrap();
        session.stage_response(&snap, &raw).unwrap();
        session.accept_staged().unwrap();
        assert!(session.is_committed());

        session.rollback();
        assert!(session.staged().is_none());
        assert!(
            session.is_committed(),
            "rollback must not clear the accept latch"
        );
        let err = session.stage_response(&snap, &raw).unwrap_err();
        assert!(
            err.message.contains("already committed"),
            "restage after accept+rollback must refuse: {}",
            err.message
        );
    }

    #[test]
    fn close_invokes_guest_closer_with_handle() {
        let mut session = ExtensionSession::new();
        session.on_loaded(42).unwrap();
        let mut calls = Vec::new();
        assert!(session.close(|h| {
            calls.push(h);
            true
        }));
        assert_eq!(calls, vec![42]);
        assert_eq!(session.phase(), SessionPhase::Closed);
        // Second close: no live handle → closer not called, returns false.
        assert!(!session.close(|_| panic!("closer must not run without handle")));
    }

    #[test]
    fn trust_and_limits_defaults_are_exact() {
        assert_eq!(TrustPolicy::Untrusted.as_str(), "untrusted");
        let d = ResourceLimits::v1_defaults();
        assert_eq!(d.max_fuel, 10_000_000);
        assert_eq!(d.max_memory_bytes, 16 * 1024 * 1024);
        assert_eq!(d.max_table_elements, 10_000);
        assert_eq!(d.max_findings, 256);
        assert_eq!(d.max_edits, 64);
        assert_eq!(d.max_response_bytes, 256 * 1024);
        assert_eq!(d.timeout_ms, 2_000);
    }

    #[test]
    fn load_and_analyze_wire_parsers_roundtrip_and_reject_errors() {
        assert_eq!(parse_load_result("O:42").unwrap(), 42);
        let err = parse_load_result("E:couldn't load").unwrap_err();
        assert!(err.message.contains("couldn't load"));
        assert!(!message_exposes_rustc(&err.message));

        let bytes = b"{\"protocol\":1}";
        let mut hex = String::new();
        for b in bytes {
            hex.push_str(&format!("{b:02x}"));
        }
        let wire = format!("O:{}:{hex}", bytes.len());
        assert_eq!(parse_analyze_result(&wire).unwrap(), bytes);
        let trap = parse_analyze_result("E:calling `analyze` trapped: all fuel").unwrap_err();
        assert!(trap.message.contains("trapped"));
        assert!(!message_exposes_rustc(&trap.message));
        assert!(message_exposes_rustc("rustc error[E0308]"));
    }
}
