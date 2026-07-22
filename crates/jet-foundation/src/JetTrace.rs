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
pub const CAPTURE_POLICY_SCHEMA: &str = "4";
pub const TRACE_TASK_ROW_LIMIT: u64 = 4096;
pub const TRACE_IO_ROW_LIMIT: u64 = 4096;
pub const TRACE_NATIVE_ROW_LIMIT: u64 = 1;
pub const TRACE_SPAN_ROW_LIMIT: u64 = 4096;

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
    pub io_rows_truncated: bool,
    pub native_rows_truncated: bool,
    pub span_rows_truncated: bool,
    pub task_rows_truncated: bool,
}

impl CapturePolicy {
    pub fn default_exclusions() -> Self {
        Self {
            allowlist: Vec::new(),
            io_rows_truncated: false,
            native_rows_truncated: false,
            span_rows_truncated: false,
            task_rows_truncated: false,
        }
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
            (
                "io_row_limit".into(),
                CanonicalJson::Integer(TRACE_IO_ROW_LIMIT.to_string()),
            ),
            (
                "io_rows_truncated".into(),
                CanonicalJson::Bool(self.io_rows_truncated),
            ),
            (
                "native_row_limit".into(),
                CanonicalJson::Integer(TRACE_NATIVE_ROW_LIMIT.to_string()),
            ),
            (
                "native_rows_truncated".into(),
                CanonicalJson::Bool(self.native_rows_truncated),
            ),
            (
                "span_row_limit".into(),
                CanonicalJson::Integer(TRACE_SPAN_ROW_LIMIT.to_string()),
            ),
            (
                "span_rows_truncated".into(),
                CanonicalJson::Bool(self.span_rows_truncated),
            ),
            ("schema".into(), CanonicalJson::Integer(CAPTURE_POLICY_SCHEMA.into())),
            (
                "task_row_limit".into(),
                CanonicalJson::Integer(TRACE_TASK_ROW_LIMIT.to_string()),
            ),
            (
                "task_rows_truncated".into(),
                CanonicalJson::Bool(self.task_rows_truncated),
            ),
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
    /// Monotonic nanoseconds from session start when capture first observed this wait.
    pub start_ns: u64,
    /// Monotonic nanoseconds from session start when capture last observed this wait.
    pub end_ns: u64,
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
            ("end_ns".into(), CanonicalJson::Integer(self.end_ns.to_string())),
            ("kind".into(), CanonicalJson::String(self.kind.clone())),
            ("start_ns".into(), CanonicalJson::Integer(self.start_ns.to_string())),
            ("symbol".into(), self.symbol.to_json()?),
            ("task_id".into(), CanonicalJson::Integer(self.task_id.to_string())),
            ("wait".into(), CanonicalJson::String(self.wait.clone())),
        ])
    }
}

/// One bounded aggregate of native process CPU time. No native frames,
/// arguments, environment, or process id enter the artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceNative {
    pub clock: String,
    /// Cumulative profiled-process CPU time at observation.
    pub duration_ns: Option<u64>,
    /// Monotonic nanoseconds from this trace session's start.
    pub observed_at_ns: u64,
    pub reason: String,
    pub status: String,
    pub symbol: JetSymbolRef,
    pub target: String,
    pub task_id: Option<u64>,
}

impl TraceNative {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        CanonicalJson::object([
            ("clock".into(), CanonicalJson::String(self.clock.clone())),
            (
                "duration_ns".into(),
                self.duration_ns
                    .map(|value| CanonicalJson::Integer(value.to_string()))
                    .unwrap_or(CanonicalJson::Null),
            ),
            (
                "observed_at_ns".into(),
                CanonicalJson::Integer(self.observed_at_ns.to_string()),
            ),
            ("reason".into(), CanonicalJson::String(self.reason.clone())),
            ("status".into(), CanonicalJson::String(self.status.clone())),
            ("symbol".into(), self.symbol.to_json()?),
            ("target".into(), CanonicalJson::String(self.target.clone())),
            (
                "task_id".into(),
                self.task_id
                    .map(|value| CanonicalJson::Integer(value.to_string()))
                    .unwrap_or(CanonicalJson::Null),
            ),
        ])
    }
}

/// One task-presence interval on the trace-session monotonic clock. Payloads,
/// arguments, locals, and generated frames are never captured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceSpan {
    pub clock: String,
    pub end_ns: Option<u64>,
    pub kind: String,
    pub parent_task_id: Option<u64>,
    pub reason: String,
    pub start_ns: Option<u64>,
    pub status: String,
    pub symbol: JetSymbolRef,
    pub task_id: Option<u64>,
}

impl TraceSpan {
    pub fn to_json(&self) -> Result<CanonicalJson, String> {
        let optional = |value: Option<u64>| {
            value
                .map(|value| CanonicalJson::Integer(value.to_string()))
                .unwrap_or(CanonicalJson::Null)
        };
        CanonicalJson::object([
            ("clock".into(), CanonicalJson::String(self.clock.clone())),
            ("end_ns".into(), optional(self.end_ns)),
            ("kind".into(), CanonicalJson::String(self.kind.clone())),
            ("parent_task_id".into(), optional(self.parent_task_id)),
            ("reason".into(), CanonicalJson::String(self.reason.clone())),
            ("start_ns".into(), optional(self.start_ns)),
            ("status".into(), CanonicalJson::String(self.status.clone())),
            ("symbol".into(), self.symbol.to_json()?),
            ("task_id".into(), optional(self.task_id)),
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
    pub native: Vec<TraceNative>,
    pub spans: Vec<TraceSpan>,
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
        let native = self
            .native
            .iter()
            .map(TraceNative::to_json)
            .collect::<Result<Vec<_>, _>>()?;
        let spans = self
            .spans
            .iter()
            .map(TraceSpan::to_json)
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
            ("native".into(), CanonicalJson::Array(native)),
            ("samples".into(), CanonicalJson::Array(samples)),
            ("source_identity".into(), CanonicalJson::Array(source_identity)),
            ("spans".into(), CanonicalJson::Array(spans)),
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
    let limits = validate_capture_policy(&fields["capture_policy"])?;
    validate_samples(&fields["samples"])?;
    validate_allocations(&fields["allocations"])?;
    let tasks = validate_tasks(&fields["tasks"], limits.task_rows)?;
    validate_locks(&fields["locks"])?;
    validate_io(&fields["io"], &tasks, limits.io_rows)?;
    validate_native(&fields["native"], &tasks, limits.native_rows)?;
    validate_spans(&fields["spans"], &tasks, limits.span_rows)?;
    validate_source_identity(&fields["source_identity"])?;
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

fn validate_tasks(
    value: &CanonicalJson,
    row_limit: Option<usize>,
) -> Result<BTreeMap<u64, (u64, String, String, CanonicalJson)>, String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.tasks is not an array".into()),
    };
    if row_limit.is_some_and(|limit| items.len() > limit) {
        return Err("content.tasks exceeds capture_policy.task_row_limit".into());
    }
    let mut tasks = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.tasks[{i}]");
        let fields = object_keys(
            item,
            &label,
            &["cancelled", "id", "parent", "state", "symbol", "wait"],
        )?;
        let id = unsigned(&fields["id"], &format!("{label}.id"))?;
        if id == 0 {
            return Err(format!("{label}.id must be greater than zero"));
        }
        let parent = unsigned(&fields["parent"], &format!("{label}.parent"))?;
        if id == parent {
            return Err(format!("{label}.parent cannot be self"));
        }
        let state = text(&fields["state"], &format!("{label}.state"))?;
        if !matches!(state, "running" | "queued" | "blocked" | "done") {
            return Err(format!("{label}.state must be a live observe task state"));
        }
        let wait = text(&fields["wait"], &format!("{label}.wait"))?;
        match &fields["cancelled"] {
            CanonicalJson::Bool(_) => {}
            _ => return Err(format!("{label}.cancelled is not a boolean")),
        }
        validate_symbol(&fields["symbol"], &format!("{label}.symbol"))?;
        if tasks
            .insert(
                id,
                (
                    parent,
                    state.into(),
                    wait.into(),
                    fields["symbol"].clone(),
                ),
            )
            .is_some()
        {
            return Err(format!("{label}.id duplicates task {id}"));
        }
    }
    for (&id, (parent, _, _, _)) in &tasks {
        if *parent != 0 && !tasks.contains_key(parent) {
            return Err(format!("content.tasks task {id} has missing parent {parent}"));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut current = id;
        while let Some((next, _, _, _)) = tasks.get(&current) {
            if *next == 0 {
                break;
            }
            if !seen.insert(current) {
                return Err(format!("content.tasks task {id} has cyclic parent causality"));
            }
            current = *next;
        }
    }
    Ok(tasks)
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

fn validate_io(
    value: &CanonicalJson,
    tasks: &BTreeMap<u64, (u64, String, String, CanonicalJson)>,
    row_limit: Option<usize>,
) -> Result<(), String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.io is not an array".into()),
    };
    if row_limit.is_some_and(|limit| items.len() > limit) {
        return Err("content.io exceeds capture_policy.io_row_limit".into());
    }
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.io[{i}]");
        let fields = object_keys(
            item,
            &label,
            &["end_ns", "kind", "start_ns", "symbol", "task_id", "wait"],
        )?;
        let kind = text(&fields["kind"], &format!("{label}.kind"))?;
        if !matches!(kind, "tcp" | "network" | "io") {
            return Err(format!("{label}.kind must be tcp, network, or io"));
        }
        let task_id = unsigned(&fields["task_id"], &format!("{label}.task_id"))?;
        let Some((_, task_state, task_wait, task_symbol)) = tasks.get(&task_id) else {
            return Err(format!("{label}.task_id refers to missing task {task_id}"));
        };
        let start_ns = unsigned(&fields["start_ns"], &format!("{label}.start_ns"))?;
        let end_ns = unsigned(&fields["end_ns"], &format!("{label}.end_ns"))?;
        if end_ns < start_ns {
            return Err(format!("{label}.end_ns precedes start_ns"));
        }
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
        match task_state.as_str() {
            "done" if task_wait.is_empty() => {}
            "blocked" if task_wait == wait => {}
            "done" => return Err(format!("{label} completed task must have an empty wait")),
            "blocked" => return Err(format!("{label}.wait does not match its blocked task")),
            _ => return Err(format!("{label} task must be blocked or done")),
        }
        validate_symbol(&fields["symbol"], &format!("{label}.symbol"))?;
        if &fields["symbol"] != task_symbol {
            return Err(format!("{label}.symbol does not match its task"));
        }
    }
    Ok(())
}

fn validate_native(
    value: &CanonicalJson,
    tasks: &BTreeMap<u64, (u64, String, String, CanonicalJson)>,
    row_limit: Option<usize>,
) -> Result<(), String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.native is not an array".into()),
    };
    if let Some(row_limit) = row_limit {
        if items.len() > row_limit {
            return Err("content.native exceeds capture_policy.native_row_limit".into());
        }
    }
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.native[{i}]");
        let fields = object_keys(
            item,
            &label,
            &[
                "clock",
                "duration_ns",
                "observed_at_ns",
                "reason",
                "status",
                "symbol",
                "target",
                "task_id",
            ],
        )?;
        if text(&fields["clock"], &format!("{label}.clock"))? != "process_cpu" {
            return Err(format!("{label}.clock must be process_cpu"));
        }
        unsigned(&fields["observed_at_ns"], &format!("{label}.observed_at_ns"))?;
        let reason = text(&fields["reason"], &format!("{label}.reason"))?;
        let status = text(&fields["status"], &format!("{label}.status"))?;
        if text(&fields["target"], &format!("{label}.target"))?.is_empty() {
            return Err(format!("{label}.target is empty"));
        }
        validate_symbol(&fields["symbol"], &format!("{label}.symbol"))?;
        match status {
            "captured" => {
                unsigned(&fields["duration_ns"], &format!("{label}.duration_ns"))?;
                let task_id = unsigned(&fields["task_id"], &format!("{label}.task_id"))?;
                let Some((parent, _, _, task_symbol)) = tasks.get(&task_id) else {
                    return Err(format!("{label}.task_id refers to missing task {task_id}"));
                };
                if *parent != 0 {
                    return Err(format!("{label}.task_id must identify the root task"));
                }
                if &fields["symbol"] != task_symbol {
                    return Err(format!("{label}.symbol does not match its task"));
                }
                if !reason.is_empty() {
                    return Err(format!("{label}.reason must be empty when captured"));
                }
            }
            "unavailable" => {
                if fields["duration_ns"] != CanonicalJson::Null
                    || fields["task_id"] != CanonicalJson::Null
                {
                    return Err(format!("{label} unavailable timing must not claim data"));
                }
                if reason.is_empty() {
                    return Err(format!("{label}.reason is empty"));
                }
            }
            _ => return Err(format!("{label}.status must be captured or unavailable")),
        }
    }
    Ok(())
}

fn validate_spans(
    value: &CanonicalJson,
    tasks: &BTreeMap<u64, (u64, String, String, CanonicalJson)>,
    row_limit: Option<usize>,
) -> Result<(), String> {
    let items = match value {
        CanonicalJson::Array(items) => items,
        _ => return Err("content.spans is not an array".into()),
    };
    if let Some(row_limit) = row_limit {
        if items.len() > row_limit {
            return Err("content.spans exceeds capture_policy.span_row_limit".into());
        }
    }
    let mut captured = 0usize;
    let mut unavailable = 0usize;
    for (i, item) in items.iter().enumerate() {
        let label = format!("content.spans[{i}]");
        let fields = object_keys(
            item,
            &label,
            &[
                "clock",
                "end_ns",
                "kind",
                "parent_task_id",
                "reason",
                "start_ns",
                "status",
                "symbol",
                "task_id",
            ],
        )?;
        if text(&fields["clock"], &format!("{label}.clock"))? != "monotonic" {
            return Err(format!("{label}.clock must be monotonic"));
        }
        if text(&fields["kind"], &format!("{label}.kind"))? != "task_observed" {
            return Err(format!("{label}.kind must be task_observed"));
        }
        let reason = text(&fields["reason"], &format!("{label}.reason"))?;
        let status = text(&fields["status"], &format!("{label}.status"))?;
        validate_symbol(&fields["symbol"], &format!("{label}.symbol"))?;
        match status {
            "captured" => {
                captured += 1;
                let start = unsigned(&fields["start_ns"], &format!("{label}.start_ns"))?;
                let end = unsigned(&fields["end_ns"], &format!("{label}.end_ns"))?;
                if end < start {
                    return Err(format!("{label} ends before it starts"));
                }
                let task_id = unsigned(&fields["task_id"], &format!("{label}.task_id"))?;
                let Some((parent, _, _, task_symbol)) = tasks.get(&task_id) else {
                    return Err(format!("{label}.task_id refers to missing task {task_id}"));
                };
                if &fields["symbol"] != task_symbol {
                    return Err(format!("{label}.symbol does not match its task"));
                }
                if *parent == 0 {
                    if fields["parent_task_id"] != CanonicalJson::Null {
                        return Err(format!("{label} root span must not claim a parent"));
                    }
                } else {
                    let parent_task_id = unsigned(
                        &fields["parent_task_id"],
                        &format!("{label}.parent_task_id"),
                    )?;
                    if parent_task_id != *parent || !tasks.contains_key(&parent_task_id) {
                        return Err(format!("{label}.parent_task_id does not match its task"));
                    }
                }
                if !reason.is_empty() {
                    return Err(format!("{label}.reason must be empty when captured"));
                }
            }
            "unavailable" => {
                unavailable += 1;
                if fields["start_ns"] != CanonicalJson::Null
                    || fields["end_ns"] != CanonicalJson::Null
                    || fields["task_id"] != CanonicalJson::Null
                    || fields["parent_task_id"] != CanonicalJson::Null
                {
                    return Err(format!("{label} unavailable span must not claim data"));
                }
                if reason.is_empty() {
                    return Err(format!("{label}.reason is empty"));
                }
            }
            _ => return Err(format!("{label}.status must be captured or unavailable")),
        }
    }
    if unavailable > 1 || (unavailable == 1 && captured != 0) {
        return Err("content.spans unavailable state must be the only row".into());
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

#[derive(Clone, Copy)]
struct CaptureLimits {
    io_rows: Option<usize>,
    native_rows: Option<usize>,
    span_rows: Option<usize>,
    task_rows: Option<usize>,
}

fn validate_capture_policy(value: &CanonicalJson) -> Result<CaptureLimits, String> {
    let schema = match value {
        CanonicalJson::Object(fields) => fields.get("schema"),
        _ => return Err("capture_policy is not an object".into()),
    };
    let (fields, limits) = match schema {
        Some(CanonicalJson::Integer(schema)) if schema == "1" => (
            object_keys(
                value,
                "capture_policy",
                &["allowlist", "default_exclusions", "schema"],
            )?,
            // Schema 1 declared neither limits nor truncation. Absence remains
            // unknown/unbounded legacy semantics; never normalize it to false.
            CaptureLimits {
                io_rows: None,
                native_rows: None,
                span_rows: None,
                task_rows: None,
            },
        ),
        Some(CanonicalJson::Integer(schema)) if schema == "2" => (
            object_keys(
                value,
                "capture_policy",
                &[
                    "allowlist",
                    "default_exclusions",
                    "io_row_limit",
                    "io_rows_truncated",
                    "schema",
                    "task_row_limit",
                    "task_rows_truncated",
                ],
            )?,
            CaptureLimits {
                io_rows: Some(TRACE_IO_ROW_LIMIT as usize),
                native_rows: None,
                span_rows: None,
                task_rows: Some(TRACE_TASK_ROW_LIMIT as usize),
            },
        ),
        Some(CanonicalJson::Integer(schema)) if schema == "3" => (
            object_keys(
                value,
                "capture_policy",
                &[
                    "allowlist",
                    "default_exclusions",
                    "io_row_limit",
                    "io_rows_truncated",
                    "native_row_limit",
                    "native_rows_truncated",
                    "schema",
                    "task_row_limit",
                    "task_rows_truncated",
                ],
            )?,
            CaptureLimits {
                io_rows: Some(TRACE_IO_ROW_LIMIT as usize),
                native_rows: Some(TRACE_NATIVE_ROW_LIMIT as usize),
                span_rows: None,
                task_rows: Some(TRACE_TASK_ROW_LIMIT as usize),
            },
        ),
        Some(CanonicalJson::Integer(schema)) if schema == CAPTURE_POLICY_SCHEMA => (
            object_keys(
                value,
                "capture_policy",
                &[
                    "allowlist",
                    "default_exclusions",
                    "io_row_limit",
                    "io_rows_truncated",
                    "native_row_limit",
                    "native_rows_truncated",
                    "schema",
                    "span_row_limit",
                    "span_rows_truncated",
                    "task_row_limit",
                    "task_rows_truncated",
                ],
            )?,
            CaptureLimits {
                io_rows: Some(TRACE_IO_ROW_LIMIT as usize),
                native_rows: Some(TRACE_NATIVE_ROW_LIMIT as usize),
                span_rows: Some(TRACE_SPAN_ROW_LIMIT as usize),
                task_rows: Some(TRACE_TASK_ROW_LIMIT as usize),
            },
        ),
        Some(CanonicalJson::Integer(_)) => return Err("unsupported capture_policy schema".into()),
        _ => return Err("capture_policy schema is not an integer".into()),
    };
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
    if limits.task_rows.is_some() {
        if unsigned(&fields["io_row_limit"], "capture_policy.io_row_limit")?
            != TRACE_IO_ROW_LIMIT
            || unsigned(&fields["task_row_limit"], "capture_policy.task_row_limit")?
                != TRACE_TASK_ROW_LIMIT
        {
            return Err("capture_policy row limits do not match this schema".into());
        }
        for key in ["io_rows_truncated", "task_rows_truncated"] {
            if !matches!(fields[key], CanonicalJson::Bool(_)) {
                return Err(format!("capture_policy.{key} is not a boolean"));
            }
        }
    }
    if limits.native_rows.is_some() {
        if unsigned(&fields["native_row_limit"], "capture_policy.native_row_limit")?
            != TRACE_NATIVE_ROW_LIMIT
        {
            return Err("capture_policy native row limit does not match this schema".into());
        }
        if !matches!(fields["native_rows_truncated"], CanonicalJson::Bool(_)) {
            return Err("capture_policy.native_rows_truncated is not a boolean".into());
        }
    }
    if limits.span_rows.is_some() {
        if unsigned(&fields["span_row_limit"], "capture_policy.span_row_limit")?
            != TRACE_SPAN_ROW_LIMIT
        {
            return Err("capture_policy span row limit does not match this schema".into());
        }
        if !matches!(fields["span_rows_truncated"], CanonicalJson::Bool(_)) {
            return Err("capture_policy.span_rows_truncated is not a boolean".into());
        }
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
    Ok(limits)
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
            native: Vec::new(),
            spans: Vec::new(),
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
        skeleton.tasks.push(TraceTask {
            id: 3,
            parent: 0,
            state: "done".into(),
            wait: String::new(),
            cancelled: false,
            symbol: symbol.clone(),
        });
        skeleton.io.push(TraceIo {
            end_ns: 29,
            kind: "tcp".into(),
            start_ns: 11,
            task_id: 3,
            wait: "tcp accept".into(),
            symbol: symbol.clone(),
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"io\":[{"), "{text}");
        assert!(text.contains("\"kind\":\"tcp\""), "{text}");
        assert!(text.contains("\"start_ns\":11"), "{text}");
        assert!(text.contains("\"end_ns\":29"), "{text}");
        assert!(text.contains("\"wait\":\"tcp accept\""), "{text}");
        assert!(text.contains("\"task_id\":3"), "{text}");
        verify_jettrace(&bytes).unwrap();

        skeleton.tasks[0].wait = "tcp accept".into();
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        assert!(
            verify_jettrace(&bytes)
                .unwrap_err()
                .contains("completed task must have an empty wait")
        );
    }

    #[test]
    fn vacuous_io_row_is_rejected() {
        let mut skeleton = sample_skeleton();
        let symbol = JetSymbolRef {
            path: "app.jet".into(),
            name: "run".into(),
        };
        skeleton.tasks.push(TraceTask {
            id: 1,
            parent: 0,
            state: "blocked".into(),
            wait: "time sleep".into(),
            cancelled: false,
            symbol: symbol.clone(),
        });
        skeleton.io.push(TraceIo {
            end_ns: 2,
            kind: "tcp".into(),
            start_ns: 1,
            task_id: 1,
            wait: "time sleep".into(),
            symbol,
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let err = verify_jettrace(&bytes).unwrap_err();
        assert!(err.contains("not an observe I/O wait"), "{err}");
    }

    #[test]
    fn orphan_task_and_io_causality_are_rejected() {
        let mut skeleton = sample_skeleton();
        let symbol = JetSymbolRef {
            path: "app.jet".into(),
            name: "run".into(),
        };
        skeleton.tasks.push(TraceTask {
            id: 2,
            parent: 1,
            state: "done".into(),
            wait: String::new(),
            cancelled: false,
            symbol: symbol.clone(),
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        assert!(verify_jettrace(&bytes).unwrap_err().contains("missing parent 1"));

        skeleton.tasks[0].parent = 0;
        skeleton.io.push(TraceIo {
            end_ns: 2,
            kind: "tcp".into(),
            start_ns: 1,
            task_id: 3,
            wait: "tcp accept".into(),
            symbol,
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        assert!(verify_jettrace(&bytes).unwrap_err().contains("missing task 3"));
    }

    #[test]
    fn native_timing_round_trips_with_root_causality_and_unavailable_state() {
        let mut skeleton = sample_skeleton();
        let symbol = JetSymbolRef {
            path: "app.jet".into(),
            name: "run".into(),
        };
        skeleton.tasks.push(TraceTask {
            id: 1,
            parent: 0,
            state: "done".into(),
            wait: String::new(),
            cancelled: false,
            symbol: symbol.clone(),
        });
        skeleton.native.push(TraceNative {
            clock: "process_cpu".into(),
            duration_ns: Some(42),
            observed_at_ns: 99,
            reason: String::new(),
            status: "captured".into(),
            symbol: symbol.clone(),
            target: "x86_64-unknown-linux-gnu".into(),
            task_id: Some(1),
        });
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"native\":[{"), "{text}");
        assert!(text.contains("\"duration_ns\":42"), "{text}");
        assert!(text.contains("\"observed_at_ns\":99"), "{text}");
        assert!(text.contains("\"task_id\":1"), "{text}");
        verify_jettrace(&bytes).unwrap();

        skeleton.tasks.clear();
        skeleton.native[0] = TraceNative {
            clock: "process_cpu".into(),
            duration_ns: None,
            observed_at_ns: 0,
            reason: "process CPU timing is unavailable on target wasm32-unknown-unknown".into(),
            status: "unavailable".into(),
            symbol,
            target: "wasm32-unknown-unknown".into(),
            task_id: None,
        };
        verify_jettrace(&build_skeleton_bytes(&skeleton).unwrap()).unwrap();
    }

    #[test]
    fn task_observation_spans_round_trip_with_parent_causality_and_unavailable_state() {
        let mut skeleton = sample_skeleton();
        let symbol = JetSymbolRef {
            path: "app.jet".into(),
            name: "run".into(),
        };
        for (id, parent) in [(1, 0), (2, 1)] {
            skeleton.tasks.push(TraceTask {
                id,
                parent,
                state: "done".into(),
                wait: String::new(),
                cancelled: false,
                symbol: symbol.clone(),
            });
            skeleton.spans.push(TraceSpan {
                clock: "monotonic".into(),
                end_ns: Some(90 + id),
                kind: "task_observed".into(),
                parent_task_id: (parent != 0).then_some(parent),
                reason: String::new(),
                start_ns: Some(10 + id),
                status: "captured".into(),
                symbol: symbol.clone(),
                task_id: Some(id),
            });
        }
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"spans\":[{"), "{text}");
        assert!(text.contains("\"parent_task_id\":1"), "{text}");
        verify_jettrace(&bytes).unwrap();

        skeleton.spans[1].parent_task_id = Some(99);
        let error = verify_jettrace(&build_skeleton_bytes(&skeleton).unwrap()).unwrap_err();
        assert!(error.contains("parent_task_id does not match"), "{error}");

        skeleton.tasks.clear();
        skeleton.spans = vec![TraceSpan {
            clock: "monotonic".into(),
            end_ns: None,
            kind: "task_observed".into(),
            parent_task_id: None,
            reason: "task span requires multiple live observations".into(),
            start_ns: None,
            status: "unavailable".into(),
            symbol,
            task_id: None,
        }];
        verify_jettrace(&build_skeleton_bytes(&skeleton).unwrap()).unwrap();
    }

    #[test]
    fn capture_policy_audits_fixed_row_caps_and_truncation() {
        let mut skeleton = sample_skeleton();
        skeleton.capture_policy.io_rows_truncated = true;
        skeleton.capture_policy.native_rows_truncated = true;
        skeleton.capture_policy.span_rows_truncated = true;
        skeleton.capture_policy.task_rows_truncated = true;
        let bytes = build_skeleton_bytes(&skeleton).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"capture_policy\":{\"allowlist\":[]"), "{text}");
        assert!(text.contains("\"schema\":4,\"span_row_limit\""), "{text}");
        assert!(text.contains("\"io_row_limit\":4096"), "{text}");
        assert!(text.contains("\"io_rows_truncated\":true"), "{text}");
        assert!(text.contains("\"native_row_limit\":1"), "{text}");
        assert!(text.contains("\"native_rows_truncated\":true"), "{text}");
        assert!(text.contains("\"span_row_limit\":4096"), "{text}");
        assert!(text.contains("\"span_rows_truncated\":true"), "{text}");
        assert!(text.contains("\"task_row_limit\":4096"), "{text}");
        assert!(text.contains("\"task_rows_truncated\":true"), "{text}");
        verify_jettrace(&bytes).unwrap();

        let mut content = skeleton.content_json().unwrap();
        let CanonicalJson::Object(content) = &mut content else {
            unreachable!()
        };
        let CanonicalJson::Object(policy) = content.get_mut("capture_policy").unwrap() else {
            unreachable!()
        };
        policy.insert("task_row_limit".into(), CanonicalJson::Integer("0".into()));
        let bytes = jettrace_artifact(CanonicalJson::Object(content.clone())).bytes();
        assert!(
            verify_jettrace(&bytes)
                .unwrap_err()
                .contains("row limits do not match")
        );

        let mut content = skeleton.content_json().unwrap();
        let CanonicalJson::Object(fields) = &mut content else {
            unreachable!()
        };
        fields.insert(
            "tasks".into(),
            CanonicalJson::Array(vec![CanonicalJson::Null; TRACE_TASK_ROW_LIMIT as usize + 1]),
        );
        let bytes = jettrace_artifact(content).bytes();
        assert!(
            verify_jettrace(&bytes)
                .unwrap_err()
                .contains("exceeds capture_policy.task_row_limit")
        );

        let mut content = skeleton.content_json().unwrap();
        let CanonicalJson::Object(fields) = &mut content else {
            unreachable!()
        };
        fields.insert(
            "spans".into(),
            CanonicalJson::Array(vec![CanonicalJson::Null; TRACE_SPAN_ROW_LIMIT as usize + 1]),
        );
        let error = verify_jettrace(&jettrace_artifact(content).bytes()).unwrap_err();
        assert!(error.contains("exceeds capture_policy.span_row_limit"), "{error}");
    }

    #[test]
    fn legacy_capture_policy_v1_keeps_unknown_limit_semantics() {
        let mut content = sample_skeleton().content_json().unwrap();
        let CanonicalJson::Object(fields) = &mut content else {
            unreachable!()
        };
        let CanonicalJson::Object(policy) = fields.get_mut("capture_policy").unwrap() else {
            unreachable!()
        };
        for key in [
            "io_row_limit",
            "io_rows_truncated",
            "native_row_limit",
            "native_rows_truncated",
            "span_row_limit",
            "span_rows_truncated",
            "task_row_limit",
            "task_rows_truncated",
        ] {
            policy.remove(key);
        }
        policy.insert("schema".into(), CanonicalJson::Integer("1".into()));
        let bytes = jettrace_artifact(content).bytes();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"capture_policy\":{\"allowlist\":[]"), "{text}");
        assert!(text.contains("\"schema\":1},\"command\""), "{text}");
        assert!(!text.contains("row_limit"), "legacy limits were fabricated: {text}");
        assert!(!text.contains("rows_truncated"), "legacy truncation was fabricated: {text}");
        assert!(text.contains("\"version\":1"), "outer version changed: {text}");
        verify_jettrace(&bytes).unwrap();
    }

    #[test]
    fn capture_policy_v2_remains_readable_with_unknown_native_limit() {
        let mut content = sample_skeleton().content_json().unwrap();
        let CanonicalJson::Object(fields) = &mut content else {
            unreachable!()
        };
        let CanonicalJson::Object(policy) = fields.get_mut("capture_policy").unwrap() else {
            unreachable!()
        };
        policy.remove("native_row_limit");
        policy.remove("native_rows_truncated");
        policy.remove("span_row_limit");
        policy.remove("span_rows_truncated");
        policy.insert("schema".into(), CanonicalJson::Integer("2".into()));
        verify_jettrace(&jettrace_artifact(content).bytes()).unwrap();
    }

    #[test]
    fn capture_policy_v3_remains_readable_with_unknown_span_limit() {
        let mut content = sample_skeleton().content_json().unwrap();
        let CanonicalJson::Object(fields) = &mut content else {
            unreachable!()
        };
        let CanonicalJson::Object(policy) = fields.get_mut("capture_policy").unwrap() else {
            unreachable!()
        };
        policy.remove("span_row_limit");
        policy.remove("span_rows_truncated");
        policy.insert("schema".into(), CanonicalJson::Integer("3".into()));
        verify_jettrace(&jettrace_artifact(content).bytes()).unwrap();
    }

    #[test]
    fn legacy_capture_policy_v3_still_rejects_malformed_spans() {
        let mut skeleton = sample_skeleton();
        let symbol = JetSymbolRef {
            path: "app.jet".into(),
            name: "run".into(),
        };
        skeleton.tasks.push(TraceTask {
            id: 1,
            parent: 0,
            state: "done".into(),
            wait: String::new(),
            cancelled: false,
            symbol: symbol.clone(),
        });
        skeleton.spans.push(TraceSpan {
            clock: "monotonic".into(),
            end_ns: Some(10),
            kind: "task_observed".into(),
            parent_task_id: None,
            reason: String::new(),
            start_ns: Some(20),
            status: "captured".into(),
            symbol,
            task_id: Some(1),
        });
        let mut content = skeleton.content_json().unwrap();
        let CanonicalJson::Object(fields) = &mut content else {
            unreachable!()
        };
        let CanonicalJson::Object(policy) = fields.get_mut("capture_policy").unwrap() else {
            unreachable!()
        };
        policy.remove("span_row_limit");
        policy.remove("span_rows_truncated");
        policy.insert("schema".into(), CanonicalJson::Integer("3".into()));
        let error = verify_jettrace(&jettrace_artifact(content).bytes()).unwrap_err();
        assert!(error.contains("ends before it starts"), "{error}");
    }

    #[test]
    fn legacy_capture_policies_still_reject_malformed_native_rows() {
        for schema in ["1", "2"] {
            let mut skeleton = sample_skeleton();
            let symbol = JetSymbolRef {
                path: "app.jet".into(),
                name: "run".into(),
            };
            skeleton.tasks.push(TraceTask {
                id: 1,
                parent: 0,
                state: "done".into(),
                wait: String::new(),
                cancelled: false,
                symbol: symbol.clone(),
            });
            skeleton.native.push(TraceNative {
                clock: "process_cpu".into(),
                duration_ns: Some(42),
                observed_at_ns: 99,
                reason: String::new(),
                status: "captured".into(),
                symbol,
                target: "x86_64-unknown-linux-gnu".into(),
                task_id: Some(1),
            });
            let mut content = skeleton.content_json().unwrap();
            let CanonicalJson::Object(fields) = &mut content else {
                unreachable!()
            };
            let CanonicalJson::Object(policy) = fields.get_mut("capture_policy").unwrap() else {
                unreachable!()
            };
            policy.remove("native_row_limit");
            policy.remove("native_rows_truncated");
            policy.remove("span_row_limit");
            policy.remove("span_rows_truncated");
            if schema == "1" {
                for key in [
                    "io_row_limit",
                    "io_rows_truncated",
                    "task_row_limit",
                    "task_rows_truncated",
                ] {
                    policy.remove(key);
                }
            }
            policy.insert("schema".into(), CanonicalJson::Integer(schema.into()));
            let CanonicalJson::Array(native) = fields.get_mut("native").unwrap() else {
                unreachable!()
            };
            let CanonicalJson::Object(row) = &mut native[0] else {
                unreachable!()
            };
            row.insert("status".into(), CanonicalJson::String("forged".into()));

            let error = verify_jettrace(&jettrace_artifact(content).bytes()).unwrap_err();
            assert!(error.contains("status must be captured or unavailable"), "{schema}: {error}");
        }
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
