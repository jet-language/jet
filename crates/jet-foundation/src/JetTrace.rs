//! D-PERFSESSION1=D / D-ARTIFACT-EXT1=A: versioned `.jettrace` artifact identity.
//!
//! Reuses the PerformanceBudget canonical-JSON substrate so budget reports and
//! performance traces share one encoding/hash law. Capture payloads grow later;
//! schema identity and verify are the durable seam.

use crate::PerformanceBudget::{stable_id, verify_stable_id, CanonicalJson};
use crate::Syntax::ARTIFACT_EXT_TRACE;
use std::collections::BTreeMap;

pub const TRACE_SCHEMA: &str = "jet.trace";
pub const TRACE_VERSION: &str = "1";
pub const CAPTURE_POLICY_SCHEMA: &str = "1";

/// Default privacy exclusions from D-PERFSESSION1 (sorted for A-canonical bytes).
pub const DEFAULT_EXCLUSIONS: &[&str] = &[
    "arguments",
    "credentials",
    "environment",
    "request_bodies",
    "response_bodies",
    "secret_types",
    "sql",
    "urls",
    "values",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceToolchain {
    pub jet_version: String,
    pub compiler_build_id: String,
    pub stdlib_id: String,
    pub runner_id: String,
}

impl TraceToolchain {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        let digest_content = CanonicalJson::object([
            ("compiler_build_id".into(), CanonicalJson::String(self.compiler_build_id.clone())),
            ("jet_version".into(), CanonicalJson::String(self.jet_version.clone())),
            ("runner_id".into(), CanonicalJson::String(self.runner_id.clone())),
            ("stdlib_id".into(), CanonicalJson::String(self.stdlib_id.clone())),
        ])?;
        let digest = stable_id(&digest_content);
        CanonicalJson::object([
            ("compiler_build_id".into(), CanonicalJson::String(self.compiler_build_id.clone())),
            ("digest".into(), CanonicalJson::String(digest)),
            ("jet_version".into(), CanonicalJson::String(self.jet_version.clone())),
            ("runner_id".into(), CanonicalJson::String(self.runner_id.clone())),
            ("stdlib_id".into(), CanonicalJson::String(self.stdlib_id.clone())),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturePolicy {
    pub allowlist: Vec<String>,
}

impl CapturePolicy {
    pub fn default_exclusions() -> Self {
        Self { allowlist: Vec::new() }
    }

    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        let mut allowlist = self.allowlist.clone();
        allowlist.sort();
        allowlist.dedup();
        let exclusions = DEFAULT_EXCLUSIONS
            .iter()
            .map(|item| CanonicalJson::String((*item).into()))
            .collect::<Vec<_>>();
        CanonicalJson::object([
            (
                "allowlist".into(),
                CanonicalJson::Array(allowlist.into_iter().map(CanonicalJson::String).collect()),
            ),
            ("default_exclusions".into(), CanonicalJson::Array(exclusions)),
            ("schema".into(), CanonicalJson::Integer(CAPTURE_POLICY_SCHEMA.into())),
        ])
    }
}

/// Jet source symbol attribution for a captured fact (path + entry name).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetSymbolRef {
    pub path: String,
    pub name: String,
}

impl JetSymbolRef {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        CanonicalJson::object([
            ("name".into(), CanonicalJson::String(self.name.clone())),
            ("path".into(), CanonicalJson::String(self.path.clone())),
        ])
    }
}

/// Best-effort top-level `fn name` spellings in source order.
/// Used so capture never invents a symbol that is not present in `--source`.
pub fn fn_names_from_source(src: &str) -> Vec<String> {
    let mut names = Vec::new();
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i + 2 < bytes.len() {
        let at_word_start = i == 0 || !is_ident_byte(bytes[i - 1]);
        if at_word_start && bytes[i] == b'f' && bytes[i + 1] == b'n' {
            let after = i + 2;
            if after < bytes.len() && bytes[after].is_ascii_whitespace() {
                let mut j = after;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let start = j;
                while j < bytes.len() && is_ident_byte(bytes[j]) {
                    j += 1;
                }
                if j > start {
                    names.push(String::from_utf8_lossy(&bytes[start..j]).into_owned());
                }
                i = j;
                continue;
            }
        }
        i += 1;
    }
    names
}

/// Jet program entry is only `fn run` (codegen D-CLIFLAG1). Returns `None`
/// when that spelling is absent — callers must not invent `"run"`.
pub fn entrypoint_name_from_source(src: &str) -> Option<String> {
    fn_names_from_source(src)
        .into_iter()
        .find(|name| name == "run")
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceSample {
    pub domain: String,
    pub duration_ns: u64,
    pub symbol: JetSymbolRef,
}

impl TraceSample {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        CanonicalJson::object([
            ("domain".into(), CanonicalJson::String(self.domain.clone())),
            ("duration_ns".into(), CanonicalJson::Integer(self.duration_ns.to_string())),
            ("symbol".into(), self.symbol.to_json()?),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceAllocation {
    pub count: u64,
    pub bytes: u64,
    pub symbol: JetSymbolRef,
}

impl TraceAllocation {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        CanonicalJson::object([
            ("bytes".into(), CanonicalJson::Integer(self.bytes.to_string())),
            ("count".into(), CanonicalJson::Integer(self.count.to_string())),
            ("symbol".into(), self.symbol.to_json()?),
        ])
    }
}

/// One observe-published task fact attributed to a Jet symbol.
/// Fields mirror D-OBSERVE-LIVE1 task rows (no payloads/locals).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceTask {
    pub id: u64,
    pub parent: u64,
    pub state: String,
    pub wait: String,
    pub cancelled: bool,
    pub symbol: JetSymbolRef,
}

impl TraceTask {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        CanonicalJson::object([
            ("cancelled".into(), CanonicalJson::Bool(self.cancelled)),
            ("id".into(), CanonicalJson::Integer(self.id.to_string())),
            ("parent".into(), CanonicalJson::Integer(self.parent.to_string())),
            ("state".into(), CanonicalJson::String(self.state.clone())),
            ("symbol".into(), self.symbol.to_json()?),
            ("wait".into(), CanonicalJson::String(self.wait.clone())),
        ])
    }
}

/// One observe-published channel contention fact (Jet sync waits; no Mutex).
/// Only contended channels belong here — idle scrape rows are omitted by capture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceLock {
    pub kind: String,
    pub id: u64,
    pub depth: u64,
    pub capacity: Option<u64>,
    pub send_waiters: u64,
    pub recv_waiters: u64,
    pub closed: bool,
    pub symbol: JetSymbolRef,
}

impl TraceLock {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        CanonicalJson::object([
            (
                "capacity".into(),
                match self.capacity {
                    Some(capacity) => CanonicalJson::Integer(capacity.to_string()),
                    None => CanonicalJson::Null,
                },
            ),
            ("closed".into(), CanonicalJson::Bool(self.closed)),
            ("depth".into(), CanonicalJson::Integer(self.depth.to_string())),
            ("id".into(), CanonicalJson::Integer(self.id.to_string())),
            ("kind".into(), CanonicalJson::String(self.kind.clone())),
            (
                "recv_waiters".into(),
                CanonicalJson::Integer(self.recv_waiters.to_string()),
            ),
            (
                "send_waiters".into(),
                CanonicalJson::Integer(self.send_waiters.to_string()),
            ),
            ("symbol".into(), self.symbol.to_json()?),
        ])
    }
}

/// One observe-published blocked I/O wait (D-OBSERVE-LIVE1 io classifier).
/// Idle scrapes and non-I/O waits are omitted by capture; verify rejects them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceIo {
    pub kind: String,
    pub task_id: u64,
    pub wait: String,
    pub symbol: JetSymbolRef,
}

impl TraceIo {
    /// Same classifier observe uses for `effects.io` (tcp / network / `io `).
    pub fn is_io_wait(wait: &str) -> bool {
        wait.contains("tcp") || wait.contains("network") || wait.starts_with("io ")
    }

    pub fn kind_for_wait(wait: &str) -> Option<&'static str> {
        if !Self::is_io_wait(wait) {
            return None;
        }
        if wait.contains("tcp") {
            Some("tcp")
        } else if wait.contains("network") {
            Some("network")
        } else {
            Some("io")
        }
    }

    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        CanonicalJson::object([
            ("kind".into(), CanonicalJson::String(self.kind.clone())),
            ("symbol".into(), self.symbol.to_json()?),
            ("task_id".into(), CanonicalJson::Integer(self.task_id.to_string())),
            ("wait".into(), CanonicalJson::String(self.wait.clone())),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceIdentity {
    pub path: String,
    pub sha256: String,
    pub symbols: Vec<(String, String)>,
}

impl SourceIdentity {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        let mut symbols = self.symbols.clone();
        symbols.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let symbols = symbols
            .into_iter()
            .map(|(name, kind)| {
                CanonicalJson::object([
                    ("kind".into(), CanonicalJson::String(kind)),
                    ("name".into(), CanonicalJson::String(name)),
                ])
            })
            .collect::<Result<Vec<_>, _>>()?;
        CanonicalJson::object([
            ("path".into(), CanonicalJson::String(self.path.clone())),
            ("sha256".into(), CanonicalJson::String(self.sha256.clone())),
            ("symbols".into(), CanonicalJson::Array(symbols)),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceSkeleton {
    pub command: String,
    pub argv: Vec<String>,
    pub toolchain: TraceToolchain,
    pub capture_policy: CapturePolicy,
    pub samples: Vec<TraceSample>,
    pub allocations: Vec<TraceAllocation>,
    pub tasks: Vec<TraceTask>,
    pub locks: Vec<TraceLock>,
    pub io: Vec<TraceIo>,
    pub source_identity: Vec<SourceIdentity>,
}

impl TraceSkeleton {
    pub fn content_json(&self) -> Result<CanonicalJson, String> {
        let argv = CanonicalJson::Array(self.argv.iter().cloned().map(CanonicalJson::String).collect());
        let samples = self
            .samples
            .iter()
            .map(TraceSample::to_json)
            .collect::<Result<Vec<_>, _>>()?;
        let allocations = self
            .allocations
            .iter()
            .map(TraceAllocation::to_json)
            .collect::<Result<Vec<_>, _>>()?;
        let tasks = self
            .tasks
            .iter()
            .map(TraceTask::to_json)
            .collect::<Result<Vec<_>, _>>()?;
        let locks = self
            .locks
            .iter()
            .map(TraceLock::to_json)
            .collect::<Result<Vec<_>, _>>()?;
        let io = self
            .io
            .iter()
            .map(TraceIo::to_json)
            .collect::<Result<Vec<_>, _>>()?;
        let source_identity = self
            .source_identity
            .iter()
            .map(SourceIdentity::to_json)
            .collect::<Result<Vec<_>, _>>()?;
        CanonicalJson::object([
            ("allocations".into(), CanonicalJson::Array(allocations)),
            ("argv".into(), argv),
            ("browser".into(), CanonicalJson::Array(Vec::new())),
            ("capture_policy".into(), self.capture_policy.to_json()?),
            ("command".into(), CanonicalJson::String(self.command.clone())),
            ("io".into(), CanonicalJson::Array(io)),
            ("locks".into(), CanonicalJson::Array(locks)),
            ("native".into(), CanonicalJson::Array(Vec::new())),
            ("samples".into(), CanonicalJson::Array(samples)),
            ("source_identity".into(), CanonicalJson::Array(source_identity)),
            ("spans".into(), CanonicalJson::Array(Vec::new())),
            ("tasks".into(), CanonicalJson::Array(tasks)),
            ("toolchain".into(), self.toolchain.to_json()?),
        ])
    }
}

pub fn jettrace_artifact(content: CanonicalJson) -> CanonicalJson {
    let trace_id = stable_id(&content);
    CanonicalJson::object([
        ("content".into(), content),
        ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
        ("trace_id".into(), CanonicalJson::String(trace_id)),
        ("version".into(), CanonicalJson::Integer(TRACE_VERSION.into())),
    ])
    .expect("fixed jettrace wrapper keys are unique")
}

pub fn build_skeleton_bytes(skeleton: &TraceSkeleton) -> Result<Vec<u8>, String> {
    let content = skeleton.content_json()?;
    Ok(jettrace_artifact(content).bytes())
}

pub fn verify_jettrace(bytes: &[u8]) -> Result<CanonicalJson, String> {
    let report = CanonicalJson::parse_canonical(bytes)?;
    let fields = match &report {
        CanonicalJson::Object(fields) => fields,
        _ => return Err("jettrace wrapper is not an object".into()),
    };
    let expected = ["content", "schema", "trace_id", "version"];
    if fields.len() != expected.len() || !expected.iter().all(|key| fields.contains_key(*key)) {
        return Err("jettrace wrapper has missing or unknown keys".into());
    }
    if fields.get("schema") != Some(&CanonicalJson::String(TRACE_SCHEMA.into())) {
        return Err(format!("unsupported jettrace schema (need {TRACE_SCHEMA})"));
    }
    match fields.get("version") {
        Some(CanonicalJson::Integer(version)) if version == TRACE_VERSION => {}
        Some(CanonicalJson::Integer(version)) => {
            return Err(format!(
                "jettrace version {version} needs a newer jet toolchain (this reader supports {TRACE_VERSION})"
            ));
        }
        _ => return Err("jettrace version is not an integer".into()),
    }
    let content = fields.get("content").expect("checked content key");
    validate_content(content)?;
    let claimed = match fields.get("trace_id") {
        Some(CanonicalJson::String(id)) => id,
        _ => return Err("jettrace trace_id is not text".into()),
    };
    if claimed.len() != 64 || !claimed.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        return Err("jettrace trace_id is not lowercase Hex64".into());
    }
    verify_stable_id(content, claimed)?;
    Ok(report)
}

fn validate_content(value: &CanonicalJson) -> Result<(), String> {
    let fields = object_keys(
        value,
        "content",
        &[
            "allocations",
            "argv",
            "browser",
            "capture_policy",
            "command",
            "io",
            "locks",
            "native",
            "samples",
            "source_identity",
            "spans",
            "tasks",
            "toolchain",
        ],
    )?;
    text(&fields["command"], "content.command")?;
    match &fields["argv"] {
        CanonicalJson::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                text(item, &format!("content.argv[{i}]"))?;
            }
        }
        _ => return Err("content.argv is not an array".into()),
    }
    for key in ["browser", "native", "spans"] {
        match &fields[key] {
            CanonicalJson::Array(_) => {}
            _ => return Err(format!("content.{key} is not an array")),
        }
    }
    validate_samples(&fields["samples"])?;
    validate_allocations(&fields["allocations"])?;
    validate_tasks(&fields["tasks"])?;
    validate_locks(&fields["locks"])?;
    validate_io(&fields["io"])?;
    validate_source_identity(&fields["source_identity"])?;
    validate_capture_policy(&fields["capture_policy"])?;
    validate_toolchain(&fields["toolchain"])?;
    Ok(())
}

fn validate_symbol(value: &CanonicalJson, label: &str) -> Result<(), String> {
    let fields = object_keys(value, label, &["name", "path"])?;
    text(&fields["name"], &format!("{label}.name"))?;
    text(&fields["path"], &format!("{label}.path"))?;
    Ok(())
}

fn validate_samples(value: &CanonicalJson) -> Result<(), String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.samples is not an array".into()),
    };
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.samples[{i}]");
        let fields = object_keys(item, &label, &["domain", "duration_ns", "symbol"])?;
        let domain = text(&fields["domain"], &format!("{label}.domain"))?;
        if !matches!(domain, "wall" | "cpu") {
            return Err(format!("{label}.domain must be wall or cpu"));
        }
        unsigned(&fields["duration_ns"], &format!("{label}.duration_ns"))?;
        validate_symbol(&fields["symbol"], &format!("{label}.symbol"))?;
    }
    Ok(())
}

fn validate_allocations(value: &CanonicalJson) -> Result<(), String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.allocations is not an array".into()),
    };
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.allocations[{i}]");
        let fields = object_keys(item, &label, &["bytes", "count", "symbol"])?;
        unsigned(&fields["bytes"], &format!("{label}.bytes"))?;
        unsigned(&fields["count"], &format!("{label}.count"))?;
        validate_symbol(&fields["symbol"], &format!("{label}.symbol"))?;
    }
    Ok(())
}

fn validate_tasks(value: &CanonicalJson) -> Result<(), String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.tasks is not an array".into()),
    };
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.tasks[{i}]");
        let fields = object_keys(
            item,
            &label,
            &["cancelled", "id", "parent", "state", "symbol", "wait"],
        )?;
        unsigned(&fields["id"], &format!("{label}.id"))?;
        unsigned(&fields["parent"], &format!("{label}.parent"))?;
        let state = text(&fields["state"], &format!("{label}.state"))?;
        if !matches!(state, "running" | "queued" | "blocked" | "done") {
            return Err(format!("{label}.state must be a live observe task state"));
        }
        text(&fields["wait"], &format!("{label}.wait"))?;
        match &fields["cancelled"] {
            CanonicalJson::Bool(_) => {}
            _ => return Err(format!("{label}.cancelled is not a boolean")),
        }
        validate_symbol(&fields["symbol"], &format!("{label}.symbol"))?;
    }
    Ok(())
}

fn validate_locks(value: &CanonicalJson) -> Result<(), String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.locks is not an array".into()),
    };
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.locks[{i}]");
        let fields = object_keys(
            item,
            &label,
            &[
                "capacity",
                "closed",
                "depth",
                "id",
                "kind",
                "recv_waiters",
                "send_waiters",
                "symbol",
            ],
        )?;
        let kind = text(&fields["kind"], &format!("{label}.kind"))?;
        if kind != "channel" {
            return Err(format!("{label}.kind must be channel"));
        }
        unsigned(&fields["id"], &format!("{label}.id"))?;
        unsigned(&fields["depth"], &format!("{label}.depth"))?;
        match &fields["capacity"] {
            CanonicalJson::Null => {}
            other => {
                unsigned(other, &format!("{label}.capacity"))?;
            }
        }
        let send_waiters = unsigned(&fields["send_waiters"], &format!("{label}.send_waiters"))?;
        let recv_waiters = unsigned(&fields["recv_waiters"], &format!("{label}.recv_waiters"))?;
        if send_waiters == 0 && recv_waiters == 0 {
            return Err(format!("{label} has no waiters (idle channel is not a lock fact)"));
        }
        match &fields["closed"] {
            CanonicalJson::Bool(_) => {}
            _ => return Err(format!("{label}.closed is not a boolean")),
        }
        validate_symbol(&fields["symbol"], &format!("{label}.symbol"))?;
    }
    Ok(())
}

fn validate_io(value: &CanonicalJson) -> Result<(), String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.io is not an array".into()),
    };
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.io[{i}]");
        let fields = object_keys(item, &label, &["kind", "symbol", "task_id", "wait"])?;
        let kind = text(&fields["kind"], &format!("{label}.kind"))?;
        if !matches!(kind, "tcp" | "network" | "io") {
            return Err(format!("{label}.kind must be tcp, network, or io"));
        }
        unsigned(&fields["task_id"], &format!("{label}.task_id"))?;
        let wait = text(&fields["wait"], &format!("{label}.wait"))?;
        if wait.is_empty() {
            return Err(format!("{label}.wait is empty (vacuous I/O row)"));
        }
        let Some(expected) = TraceIo::kind_for_wait(wait) else {
            return Err(format!(
                "{label}.wait is not an observe I/O wait (idle scrape is not an I/O fact)"
            ));
        };
        if kind != expected {
            return Err(format!("{label}.kind does not match wait classifier"));
        }
        validate_symbol(&fields["symbol"], &format!("{label}.symbol"))?;
    }
    Ok(())
}

fn validate_source_identity(value: &CanonicalJson) -> Result<(), String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.source_identity is not an array".into()),
    };
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.source_identity[{i}]");
        let fields = object_keys(item, &label, &["path", "sha256", "symbols"])?;
        text(&fields["path"], &format!("{label}.path"))?;
        let hash = text(&fields["sha256"], &format!("{label}.sha256"))?;
        if hash.len() != 64 || !hash.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(format!("{label}.sha256 is not lowercase Hex64"));
        }
        let symbols = match &fields["symbols"] {
            CanonicalJson::Array(symbols) => symbols,
            _ => return Err(format!("{label}.symbols is not an array")),
        };
        let mut prior: Option<(&str, &str)> = None;
        for (j, symbol) in symbols.iter().enumerate() {
            let slabel = format!("{label}.symbols[{j}]");
            let sfields = object_keys(symbol, &slabel, &["kind", "name"])?;
            let kind = text(&sfields["kind"], &format!("{slabel}.kind"))?;
            let name = text(&sfields["name"], &format!("{slabel}.name"))?;
            if prior.is_some_and(|p| p > (name, kind)) {
                return Err(format!("{label}.symbols is not sorted"));
            }
            prior = Some((name, kind));
        }
    }
    Ok(())
}

fn unsigned(value: &CanonicalJson, label: &str) -> Result<u64, String> {
    match value {
        CanonicalJson::Integer(text) => text
            .parse::<u64>()
            .map_err(|_| format!("{label} is not an unsigned integer")),
        _ => Err(format!("{label} is not an integer")),
    }
}

fn validate_capture_policy(value: &CanonicalJson) -> Result<(), String> {
    let fields = object_keys(value, "capture_policy", &["allowlist", "default_exclusions", "schema"])?;
    if fields["schema"] != CanonicalJson::Integer(CAPTURE_POLICY_SCHEMA.into()) {
        return Err("unsupported capture_policy schema".into());
    }
    let exclusions = match &fields["default_exclusions"] {
        CanonicalJson::Array(items) => items,
        _ => return Err("capture_policy.default_exclusions is not an array".into()),
    };
    let expected = DEFAULT_EXCLUSIONS
        .iter()
        .map(|item| CanonicalJson::String((*item).into()))
        .collect::<Vec<_>>();
    if exclusions != &expected {
        return Err("capture_policy.default_exclusions does not match D-PERFSESSION1 defaults".into());
    }
    match &fields["allowlist"] {
        CanonicalJson::Array(items) => {
            let mut prior: Option<&str> = None;
            for item in items {
                let text = text(item, "capture_policy.allowlist item")?;
                if prior.is_some_and(|p| p > text) {
                    return Err("capture_policy.allowlist is not sorted".into());
                }
                prior = Some(text);
            }
        }
        _ => return Err("capture_policy.allowlist is not an array".into()),
    }
    Ok(())
}

fn validate_toolchain(value: &CanonicalJson) -> Result<(), String> {
    let fields = object_keys(
        value,
        "toolchain",
        &["compiler_build_id", "digest", "jet_version", "runner_id", "stdlib_id"],
    )?;
    for key in ["compiler_build_id", "jet_version", "runner_id", "stdlib_id"] {
        text(&fields[key], &format!("toolchain.{key}"))?;
    }
    let digest = text(&fields["digest"], "toolchain.digest")?;
    if digest.len() != 64 || !digest.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        return Err("toolchain.digest is not lowercase Hex64".into());
    }
    let digest_content = CanonicalJson::object([
        ("compiler_build_id".into(), fields["compiler_build_id"].clone()),
        ("jet_version".into(), fields["jet_version"].clone()),
        ("runner_id".into(), fields["runner_id"].clone()),
        ("stdlib_id".into(), fields["stdlib_id"].clone()),
    ])?;
    verify_stable_id(&digest_content, digest).map_err(|_| "toolchain digest mismatch".to_string())?;
    Ok(())
}

fn object_keys<'a>(
    value: &'a CanonicalJson,
    label: &str,
    keys: &[&str],
) -> Result<&'a BTreeMap<String, CanonicalJson>, String> {
    let fields = match value {
        CanonicalJson::Object(fields) => fields,
        _ => return Err(format!("{label} is not an object")),
    };
    if fields.len() != keys.len() || !keys.iter().all(|key| fields.contains_key(*key)) {
        return Err(format!("{label} has missing or unknown keys"));
    }
    Ok(fields)
}

fn text<'a>(value: &'a CanonicalJson, label: &str) -> Result<&'a str, String> {
    match value {
        CanonicalJson::String(text) => Ok(text),
        _ => Err(format!("{label} is not text")),
    }
}

pub fn trace_id(value: &CanonicalJson) -> Result<&str, String> {
    let fields = match value {
        CanonicalJson::Object(fields) => fields,
        _ => return Err("jettrace wrapper is not an object".into()),
    };
    text(fields.get("trace_id").ok_or("jettrace missing trace_id")?, "trace_id")
}

pub fn artifact_extension() -> &'static str {
    ARTIFACT_EXT_TRACE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_skeleton() -> TraceSkeleton {
        TraceSkeleton {
            command: "run".into(),
            argv: vec!["run".into(), "app.jet".into()],
            toolchain: TraceToolchain {
                jet_version: "0.0.0-test".into(),
                compiler_build_id: "build-test".into(),
                stdlib_id: "stdlib-test".into(),
                runner_id: "runner-test".into(),
            },
            capture_policy: CapturePolicy::default_exclusions(),
            samples: Vec::new(),
            allocations: Vec::new(),
            tasks: Vec::new(),
            locks: Vec::new(),
            io: Vec::new(),
            source_identity: Vec::new(),
        }
    }

    #[test]
    fn entrypoint_is_parsed_never_invented() {
        assert_eq!(entrypoint_name_from_source("fn probe() {\n}\n"), None);
        assert_eq!(
            entrypoint_name_from_source("fn probe() {}\nfn run() {}\n").as_deref(),
            Some("run")
        );
        assert_eq!(
            fn_names_from_source("fn probe() {}\nfn run() {}\n"),
            vec!["probe".to_string(), "run".to_string()]
        );
    }

    #[test]
    fn captured_wall_and_alloc_round_trip_with_symbol() {
        let mut skeleton = sample_skeleton();
        let symbol = JetSymbolRef {
            path: "app.jet".into(),
            name: "run".into(),
        };
        skeleton.samples.push(TraceSample {
            domain: "wall".into(),
            duration_ns: 42,
            symbol: symbol.clone(),
        });
        skeleton.allocations.push(TraceAllocation {
            count: 3,
            bytes: 96,
            symbol: symbol.clone(),
        });
        skeleton.source_identity.push(SourceIdentity {
            path: "app.jet".into(),
            sha256: "a".repeat(64),
            symbols: vec![("run".into(), "fn".into())],
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let verified = verify_jettrace(&bytes).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains("\"domain\":\"wall\""), "{text}");
        assert!(text.contains("\"duration_ns\":42"), "{text}");
        assert!(text.contains("\"count\":3"), "{text}");
        assert!(text.contains("\"name\":\"run\""), "{text}");
        assert_eq!(trace_id(&verified).unwrap().len(), 64);
    }

    #[test]
    fn captured_tasks_round_trip_with_parent_causality() {
        let mut skeleton = sample_skeleton();
        let symbol = JetSymbolRef {
            path: "app.jet".into(),
            name: "run".into(),
        };
        skeleton.tasks.push(TraceTask {
            id: 1,
            parent: 0,
            state: "running".into(),
            wait: String::new(),
            cancelled: false,
            symbol: symbol.clone(),
        });
        skeleton.tasks.push(TraceTask {
            id: 2,
            parent: 1,
            state: "blocked".into(),
            wait: "time sleep".into(),
            cancelled: false,
            symbol: symbol.clone(),
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"tasks\":[{"), "{text}");
        assert!(text.contains("\"parent\":1"), "{text}");
        assert!(text.contains("\"state\":\"blocked\""), "{text}");
        assert!(text.contains("\"wait\":\"time sleep\""), "{text}");
        verify_jettrace(&bytes).unwrap();
    }

    #[test]
    fn captured_locks_round_trip_with_channel_waiters() {
        let mut skeleton = sample_skeleton();
        let symbol = JetSymbolRef {
            path: "app.jet".into(),
            name: "run".into(),
        };
        skeleton.locks.push(TraceLock {
            kind: "channel".into(),
            id: 3,
            depth: 0,
            capacity: Some(1),
            send_waiters: 0,
            recv_waiters: 1,
            closed: false,
            symbol: symbol.clone(),
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"locks\":[{"), "{text}");
        assert!(text.contains("\"kind\":\"channel\""), "{text}");
        assert!(text.contains("\"recv_waiters\":1"), "{text}");
        assert!(text.contains("\"capacity\":1"), "{text}");
        verify_jettrace(&bytes).unwrap();
    }

    #[test]
    fn idle_channel_lock_row_is_rejected() {
        let mut skeleton = sample_skeleton();
        skeleton.locks.push(TraceLock {
            kind: "channel".into(),
            id: 1,
            depth: 0,
            capacity: Some(1),
            send_waiters: 0,
            recv_waiters: 0,
            closed: false,
            symbol: JetSymbolRef {
                path: "app.jet".into(),
                name: "run".into(),
            },
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let err = verify_jettrace(&bytes).unwrap_err();
        assert!(err.contains("no waiters"), "{err}");
    }

    #[test]
    fn captured_io_round_trip_with_tcp_accept_wait() {
        let mut skeleton = sample_skeleton();
        let symbol = JetSymbolRef {
            path: "app.jet".into(),
            name: "run".into(),
        };
        skeleton.io.push(TraceIo {
            kind: "tcp".into(),
            task_id: 3,
            wait: "tcp accept".into(),
            symbol: symbol.clone(),
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"io\":[{"), "{text}");
        assert!(text.contains("\"kind\":\"tcp\""), "{text}");
        assert!(text.contains("\"wait\":\"tcp accept\""), "{text}");
        assert!(text.contains("\"task_id\":3"), "{text}");
        verify_jettrace(&bytes).unwrap();
    }

    #[test]
    fn vacuous_io_row_is_rejected() {
        let mut skeleton = sample_skeleton();
        skeleton.io.push(TraceIo {
            kind: "tcp".into(),
            task_id: 1,
            wait: "time sleep".into(),
            symbol: JetSymbolRef {
                path: "app.jet".into(),
                name: "run".into(),
            },
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let err = verify_jettrace(&bytes).unwrap_err();
        assert!(err.contains("not an observe I/O wait"), "{err}");
    }

    #[test]
    fn skeleton_round_trips_with_schema_identity() {
        let bytes = build_skeleton_bytes(&sample_skeleton()).unwrap();
        let verified = verify_jettrace(&bytes).unwrap();
        assert_eq!(
            match &verified {
                CanonicalJson::Object(fields) => fields.get("schema"),
                _ => None,
            },
            Some(&CanonicalJson::String(TRACE_SCHEMA.into()))
        );
        let id = trace_id(&verified).unwrap();
        assert_eq!(id.len(), 64);
        assert!(artifact_extension().ends_with("jettrace"));
    }

    #[test]
    fn forged_trace_id_is_rejected() {
        let mut bytes = build_skeleton_bytes(&sample_skeleton()).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let forged = text.replacen(
            &stable_id(&sample_skeleton().content_json().unwrap()),
            &"0".repeat(64),
            1,
        );
        bytes = forged.into_bytes();
        assert!(verify_jettrace(&bytes).unwrap_err().contains("content hash mismatch"));
    }

    #[test]
    fn newer_major_version_names_required_toolchain() {
        let content = sample_skeleton().content_json().unwrap();
        let wrapper = CanonicalJson::object([
            ("content".into(), content.clone()),
            ("schema".into(), CanonicalJson::String(TRACE_SCHEMA.into())),
            ("trace_id".into(), CanonicalJson::String(stable_id(&content))),
            ("version".into(), CanonicalJson::Integer("2".into())),
        ])
        .unwrap();
        let err = verify_jettrace(&wrapper.bytes()).unwrap_err();
        assert!(err.contains("newer jet toolchain"), "{err}");
    }
}
