//! D-OPTGC1: bounded, durable automatic-promotion reports.

use std::collections::{BTreeSet, HashMap};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};

use jet_foundation::JSON::{json_escape, parse_json, JSONValue};

use crate::OutputMode;

const MAX_TRACE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_TRACE_FIELD_BYTES: usize = 4 * 1024;
const MAX_TRACE_IDENTITIES: usize = 65_536;
const MAX_TRACE_SITES: usize = 4_096;
const MAX_TRACE_AGE_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
const TRACE_SCHEMA: &str = "jet.gc.trace";
const TRACE_VERSION: u64 = 1;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Identity {
    id: u64,
    retained: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Site {
    source: String,
    span_start: u64,
    span_end: u64,
    scope: String,
    policy_provenance: String,
    reason: String,
    type_name: String,
    identities: Vec<Identity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Trace {
    project: String,
    pid: u64,
    started_unix_ms: u64,
    updated_unix_ms: u64,
    collections: u64,
    sites: Vec<Site>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ErrorKind {
    Missing,
    Unsafe,
    Oversized,
    Malformed,
    Incompatible,
    Stale,
    Incomplete,
}

#[derive(Debug)]
struct TraceError {
    kind: ErrorKind,
    detail: String,
}

impl TraceError {
    fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self { kind, detail: detail.into() }
    }
}

pub(crate) fn configure_trace() {
    let root = project_root();
    std::env::set_var("JET_GC_PROJECT", root.to_string_lossy().as_ref());
    std::env::set_var("JET_GC_TRACE", trace_path(&root));
}

pub(crate) fn run(args: &[String], mode: OutputMode) {
    if let Some(argument) = args.iter().find(|argument| !argument.starts_with('-')) {
        fail(
            TraceError::new(ErrorKind::Malformed, format!("unexpected gc report argument `{argument}`")),
            mode,
        );
    }
    let root = project_root();
    let path = std::env::var_os("JET_GC_TRACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| trace_path(&root));
    let raw = read_trace(&path).unwrap_or_else(|error| fail(error, mode));
    let now = unix_ms();
    let trace = parse_trace(&raw, root.to_string_lossy().as_ref(), now)
        .unwrap_or_else(|error| fail(error, mode));
    if mode.json {
        println!("{}", render_json(&trace));
    } else {
        let color = mode.color_stderr_for(std::io::stdout().is_terminal());
        print!("{}", render_text(&trace, color));
    }
}

fn project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    jet::Loader::find_manifest_root(&cwd).unwrap_or(cwd)
}

fn trace_path(root: &Path) -> PathBuf {
    root.join(".jet").join("gc").join("trace-v1.json")
}

fn read_trace(path: &Path) -> Result<String, TraceError> {
    if let Some(parent) = path.parent() {
        let metadata = std::fs::symlink_metadata(parent).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                TraceError::new(
                    ErrorKind::Missing,
                    format!("no GC trace exists at `{}`", path.display()),
                )
            } else {
                TraceError::new(
                    ErrorKind::Unsafe,
                    format!("cannot inspect GC trace directory: {error}"),
                )
            }
        })?;
        if !metadata.file_type().is_dir() {
            return Err(TraceError::new(
                ErrorKind::Unsafe,
                "GC trace directory path is not a directory",
            ));
        }
    }
    let link = std::fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            TraceError::new(
                ErrorKind::Missing,
                format!("no GC trace exists at `{}`", path.display()),
            )
        } else {
            TraceError::new(ErrorKind::Unsafe, format!("cannot inspect GC trace: {error}"))
        }
    })?;
    if !link.file_type().is_file() {
        return Err(TraceError::new(ErrorKind::Unsafe, "GC trace is not a regular file"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0o400000);
    }
    let file = options.open(path).map_err(|error| {
        TraceError::new(ErrorKind::Unsafe, format!("cannot securely open GC trace: {error}"))
    })?;
    let metadata = file.metadata().map_err(|error| {
        TraceError::new(ErrorKind::Unsafe, format!("cannot inspect GC trace: {error}"))
    })?;
    if !metadata.file_type().is_file() {
        return Err(TraceError::new(ErrorKind::Unsafe, "GC trace is not a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(TraceError::new(
                ErrorKind::Unsafe,
                "GC trace permissions expose runtime state",
            ));
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if let Ok(self_metadata) = std::fs::metadata("/proc/self") {
            if metadata.uid() != self_metadata.uid() {
                return Err(TraceError::new(ErrorKind::Unsafe, "GC trace belongs to another user"));
            }
        }
    }
    if metadata.len() > MAX_TRACE_BYTES {
        return Err(TraceError::new(
            ErrorKind::Oversized,
            "GC trace exceeds the 4 MiB safety limit",
        ));
    }
    let mut raw = String::new();
    file.take(MAX_TRACE_BYTES + 1)
        .read_to_string(&mut raw)
        .map_err(|error| TraceError::new(ErrorKind::Malformed, format!("cannot read GC trace: {error}")))?;
    if raw.len() as u64 > MAX_TRACE_BYTES {
        return Err(TraceError::new(
            ErrorKind::Oversized,
            "GC trace exceeds the 4 MiB safety limit",
        ));
    }
    Ok(raw)
}

fn parse_trace(raw: &str, expected_project: &str, now: u64) -> Result<Trace, TraceError> {
    let value = parse_json(raw)
        .map_err(|_| TraceError::new(ErrorKind::Malformed, "GC trace is not valid JSON"))?;
    let root = object(&value, "GC trace root")?;
    if string_field(root, "schema")? != TRACE_SCHEMA {
        return Err(TraceError::new(ErrorKind::Incompatible, "GC trace schema is not supported"));
    }
    if uint_field(root, "version")? != TRACE_VERSION {
        return Err(TraceError::new(ErrorKind::Incompatible, "GC trace version is not supported"));
    }
    if expected_project.len() > MAX_TRACE_FIELD_BYTES
        || expected_project.chars().any(char::is_control)
    {
        return Err(TraceError::new(
            ErrorKind::Unsafe,
            "project path is not safe to display",
        ));
    }
    let project = safe_string(root, "project")?;
    if project != expected_project {
        return Err(TraceError::new(
            ErrorKind::Stale,
            format!("GC trace belongs to project `{project}`, not `{expected_project}`"),
        ));
    }
    let pid = uint_field(root, "pid")?;
    if pid == 0 {
        return Err(TraceError::new(ErrorKind::Malformed, "GC trace process id is zero"));
    }
    let started_unix_ms = uint_field(root, "started_unix_ms")?;
    let updated_unix_ms = uint_field(root, "updated_unix_ms")?;
    if started_unix_ms > updated_unix_ms || updated_unix_ms > now.saturating_add(5 * 60 * 1_000) {
        return Err(TraceError::new(ErrorKind::Malformed, "GC trace timestamps are inconsistent"));
    }
    if now.saturating_sub(updated_unix_ms) > MAX_TRACE_AGE_MS {
        return Err(TraceError::new(ErrorKind::Stale, "GC trace is older than 30 days"));
    }
    let complete = bool_field(root, "complete")?;
    let dropped_promotions = uint_field(root, "dropped_promotions")?;
    if !complete || dropped_promotions != 0 {
        return Err(TraceError::new(
            ErrorKind::Incomplete,
            format!("GC trace dropped {dropped_promotions} promotions at its safety limits"),
        ));
    }
    let collections = uint_field(root, "collections")?;
    let site_values = array_field(root, "sites")?;
    if site_values.len() > MAX_TRACE_SITES {
        return Err(TraceError::new(ErrorKind::Oversized, "GC trace has too many promotion sites"));
    }
    let mut sites = Vec::with_capacity(site_values.len());
    let mut identities_seen = BTreeSet::new();
    let mut sites_seen = BTreeSet::new();
    for value in site_values {
        let site = object(value, "GC promotion site")?;
        let source = safe_string(site, "source")?;
        let span_start = uint_field(site, "span_start")?;
        let span_end = uint_field(site, "span_end")?;
        if span_start > span_end {
            return Err(TraceError::new(ErrorKind::Malformed, "GC promotion span is reversed"));
        }
        let scope = safe_string(site, "scope")?;
        let policy_provenance = safe_string(site, "policy_provenance")?;
        let reason = safe_string(site, "reason")?;
        let type_name = safe_string(site, "type_name")?;
        if !sites_seen.insert((
            source.clone(), span_start, span_end, scope.clone(), policy_provenance.clone(),
            reason.clone(), type_name.clone(),
        )) {
            return Err(TraceError::new(ErrorKind::Malformed, "GC trace repeats a promotion site"));
        }
        let identity_values = array_field(site, "identities")?;
        if identities_seen.len().saturating_add(identity_values.len()) > MAX_TRACE_IDENTITIES {
            return Err(TraceError::new(ErrorKind::Oversized, "GC trace has too many identities"));
        }
        let mut identities = Vec::with_capacity(identity_values.len());
        for value in identity_values {
            let identity = object(value, "GC identity")?;
            let id = uint_field(identity, "identity")?;
            if id == 0 || !identities_seen.insert(id) {
                return Err(TraceError::new(ErrorKind::Malformed, "GC trace has a zero or duplicate identity"));
            }
            identities.push(Identity { id, retained: bool_field(identity, "retained")? });
        }
        identities.sort_by_key(|identity| identity.id);
        let allocations = usize::try_from(uint_field(site, "allocations")?).map_err(|_| {
            TraceError::new(ErrorKind::Oversized, "GC trace allocation count is too large")
        })?;
        let retained = usize::try_from(uint_field(site, "retained")?).map_err(|_| {
            TraceError::new(ErrorKind::Oversized, "GC trace retained count is too large")
        })?;
        let derived_retained = identities.iter().filter(|identity| identity.retained).count();
        if allocations != identities.len() || retained != derived_retained {
            return Err(TraceError::new(
                ErrorKind::Malformed,
                "GC trace counters do not match identity evidence",
            ));
        }
        sites.push(Site {
            source,
            span_start,
            span_end,
            scope,
            policy_provenance,
            reason,
            type_name,
            identities,
        });
    }
    sites.sort_by(|left, right| {
        (
            &left.source,
            left.span_start,
            left.span_end,
            &left.scope,
            &left.policy_provenance,
            &left.reason,
            &left.type_name,
        )
            .cmp(&(
                &right.source,
                right.span_start,
                right.span_end,
                &right.scope,
                &right.policy_provenance,
                &right.reason,
                &right.type_name,
            ))
    });
    Ok(Trace { project, pid, started_unix_ms, updated_unix_ms, collections, sites })
}

fn object<'a>(value: &'a JSONValue, name: &str) -> Result<&'a HashMap<String, JSONValue>, TraceError> {
    match value {
        JSONValue::Object(object) => Ok(object),
        _ => Err(TraceError::new(ErrorKind::Malformed, format!("{name} is not an object"))),
    }
}

fn field<'a>(object: &'a HashMap<String, JSONValue>, name: &str) -> Result<&'a JSONValue, TraceError> {
    object.get(name).ok_or_else(|| {
        TraceError::new(ErrorKind::Malformed, format!("GC trace is missing `{name}`"))
    })
}

fn string_field<'a>(object: &'a HashMap<String, JSONValue>, name: &str) -> Result<&'a str, TraceError> {
    match field(object, name)? {
        JSONValue::String(value) => Ok(value),
        _ => Err(TraceError::new(ErrorKind::Malformed, format!("GC trace `{name}` is not text"))),
    }
}

fn safe_string(object: &HashMap<String, JSONValue>, name: &str) -> Result<String, TraceError> {
    let value = string_field(object, name)?;
    if value.is_empty()
        || value.len() > MAX_TRACE_FIELD_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(TraceError::new(
            ErrorKind::Malformed,
            format!("GC trace `{name}` is empty, unsafe, or too long"),
        ));
    }
    Ok(value.to_string())
}

fn uint_field(object: &HashMap<String, JSONValue>, name: &str) -> Result<u64, TraceError> {
    match field(object, name)? {
        JSONValue::Number(value) if *value >= 0 => Ok(*value as u64),
        _ => Err(TraceError::new(
            ErrorKind::Malformed,
            format!("GC trace `{name}` is not a non-negative integer"),
        )),
    }
}

fn bool_field(object: &HashMap<String, JSONValue>, name: &str) -> Result<bool, TraceError> {
    match field(object, name)? {
        JSONValue::Bool(value) => Ok(*value),
        _ => Err(TraceError::new(ErrorKind::Malformed, format!("GC trace `{name}` is not Bool"))),
    }
}

fn array_field<'a>(object: &'a HashMap<String, JSONValue>, name: &str) -> Result<&'a [JSONValue], TraceError> {
    match field(object, name)? {
        JSONValue::Array(values) => Ok(values),
        _ => Err(TraceError::new(ErrorKind::Malformed, format!("GC trace `{name}` is not a list"))),
    }
}

fn render_text(trace: &Trace, color: bool) -> String {
    let allocations = trace.sites.iter().map(|site| site.identities.len()).sum::<usize>();
    let retained = trace
        .sites
        .iter()
        .flat_map(|site| &site.identities)
        .filter(|identity| identity.retained)
        .count();
    let title = paint("jet gc report", "1;36", color);
    let mut out = format!(
        "{title} · {allocations} promotions · {retained} retained · {} collections\n",
        trace.collections
    );
    if trace.sites.is_empty() {
        out.push_str("\nNo automatic GC promotions recorded.\nRemove the effective gc opt-in; ownership already proves every allocation.\n");
        return out;
    }
    for site in &trace.sites {
        let retained = site.identities.iter().filter(|identity| identity.retained).count();
        out.push_str(&format!(
            "\n{}  {}\n  scope: {}\n  policy: {}\n  reason: {}\n  allocations: {}  retained: {}\n  identities: {}\n  rewrite: {}\n",
            paint(&format!("{}:{}..{}", site.source, site.span_start, site.span_end), "1", color),
            site.type_name, site.scope, site.policy_provenance, site.reason,
            site.identities.len(), retained, identity_summary(&site.identities), recommendation(site)
        ));
    }
    out
}

fn identity_summary(identities: &[Identity]) -> String {
    let mut out = identities
        .iter()
        .take(8)
        .map(|identity| format!("{}{}", identity.id, if identity.retained { "*" } else { "" }))
        .collect::<Vec<_>>()
        .join(", ");
    if identities.len() > 8 {
        out.push_str(&format!(" (+{} more)", identities.len() - 8));
    }
    if out.is_empty() { "-".to_string() } else { out }
}

fn recommendation(site: &Site) -> String {
    format!(
        "own {} directly; represent identity-bearing links as Id<{}>, or use Pool<{}> when lifetime is bounded",
        site.type_name, site.type_name, site.type_name
    )
}

fn render_json(trace: &Trace) -> String {
    let allocations = trace.sites.iter().map(|site| site.identities.len()).sum::<usize>();
    let retained = trace
        .sites
        .iter()
        .flat_map(|site| &site.identities)
        .filter(|identity| identity.retained)
        .count();
    let sites = trace.sites.iter().map(|site| {
        let retained = site.identities.iter().filter(|identity| identity.retained).count();
        let identities = site.identities.iter().map(|identity| {
            format!("{{\"identity\":{},\"retained\":{}}}", identity.id, identity.retained)
        }).collect::<Vec<_>>().join(",");
        format!(
            "{{\"source\":\"{}\",\"span_start\":{},\"span_end\":{},\"scope\":\"{}\",\"policy_provenance\":\"{}\",\"reason\":\"{}\",\"type_name\":\"{}\",\"allocations\":{},\"retained\":{},\"identities\":[{}],\"recommendation\":\"{}\"}}",
            json_escape(&site.source), site.span_start, site.span_end, json_escape(&site.scope),
            json_escape(&site.policy_provenance), json_escape(&site.reason), json_escape(&site.type_name),
            site.identities.len(), retained, identities, json_escape(&recommendation(site))
        )
    }).collect::<Vec<_>>().join(",");
    format!(
        "{{\"schema\":\"jet.gc.report\",\"version\":1,\"project\":\"{}\",\"pid\":{},\"started_unix_ms\":{},\"updated_unix_ms\":{},\"collections\":{},\"summary\":{{\"sites\":{},\"allocations\":{},\"retained\":{}}},\"sites\":[{}]}}",
        json_escape(&trace.project), trace.pid, trace.started_unix_ms, trace.updated_unix_ms,
        trace.collections, trace.sites.len(), allocations, retained, sites
    )
}

fn paint(value: &str, code: &str, enabled: bool) -> String {
    if enabled { format!("\x1b[{code}m{value}\x1b[0m") } else { value.to_string() }
}

fn fail(error: TraceError, mode: OutputMode) -> ! {
    let fix = match error.kind {
        ErrorKind::Missing => "run `jet run --gc-trace <file.jet>`, then rerun `jet gc report`",
        ErrorKind::Incomplete => "trace a smaller complete workload; reports never estimate dropped promotions",
        _ => "rerun the program with `--gc-trace` to replace the rejected trace",
    };
    if mode.json {
        println!(
            "{{\"schema_version\":1,\"diagnostics\":[{{\"schema_version\":1,\"code\":\"E2110\",\"severity\":\"error\",\"message\":\"GC trace cannot be reported\",\"why\":\"{}\",\"fix\":\"{}\",\"detail\":null,\"file\":null,\"line\":null,\"col\":null,\"span\":null,\"edit\":null}}]}}",
            json_escape(&error.detail), json_escape(fix)
        );
    } else {
        eprintln!("Error [E2110]: GC trace cannot be reported");
        eprintln!(" Why: {}.", error.detail.trim_end_matches('.'));
        eprintln!(" Fix: {fix}.");
    }
    exit(jet::ExitCodes::USER_ERROR)
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "{\"schema\":\"jet.gc.trace\",\"version\":1,\"project\":\"/project\",\"pid\":7,\"started_unix_ms\":10,\"updated_unix_ms\":20,\"complete\":true,\"dropped_promotions\":0,\"collections\":1,\"sites\":[{\"source\":\"src/main.jet\",\"span_start\":4,\"span_end\":8,\"scope\":\"fn run\",\"policy_provenance\":\"pkg.jet:4\",\"reason\":\"cycle\",\"type_name\":\"Node\",\"allocations\":2,\"retained\":1,\"identities\":[{\"identity\":1,\"retained\":true},{\"identity\":2,\"retained\":false}]}]}";

    #[test]
    fn projections_derive_counts_from_identity_evidence() {
        let trace = parse_trace(SAMPLE, "/project", 20).unwrap();
        let text = render_text(&trace, false);
        assert!(text.contains("2 promotions · 1 retained"));
        assert!(text.contains("Id<Node>"));
        let json = render_json(&trace);
        assert!(json.contains("\"allocations\":2,\"retained\":1"));
        assert!(!json.contains("\x1b"));
    }

    #[test]
    fn malformed_or_incomplete_evidence_never_becomes_a_report() {
        let bad_count = SAMPLE.replace("\"allocations\":2", "\"allocations\":3");
        assert_eq!(parse_trace(&bad_count, "/project", 20).unwrap_err().kind, ErrorKind::Malformed);
        let incomplete = SAMPLE.replace("\"complete\":true", "\"complete\":false");
        assert_eq!(parse_trace(&incomplete, "/project", 20).unwrap_err().kind, ErrorKind::Incomplete);
    }
}
