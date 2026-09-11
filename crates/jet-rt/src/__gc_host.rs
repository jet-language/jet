// Hosted GC adapter.  The collector algorithms stay in __gc_core.rs; this
// file owns std synchronization, identity allocation, trace persistence, the
// memory ledger bridge, panic transport, and the hosted E2110 boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as IOWrite;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use std::sync::{
    Mutex as GcMutex,
    OnceLock as GcOnce,
    TryLockError as GcTryLockError,
};

use super::{Fault, GcPanic, ObjectId, PromotionSite};

const TRACE_VERSION: u32 = 1;
const MAX_TRACE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TRACE_FIELD_BYTES: usize = 4 * 1024;
const MAX_TRACE_IDENTITIES: usize = 65_536;
const MAX_TRACE_SITES: usize = 4_096;
const TRACE_SCHEMA: &str = "jet.gc.trace";

static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TRACE_TEMP: AtomicU64 = AtomicU64::new(1);
static TRACE: GcOnce<Option<GcMutex<TraceState>>> = GcOnce::new();

impl std::error::Error for super::Fault {}

pub(crate) fn gc_next_object_id() -> Result<u64, Fault> {
    NEXT_OBJECT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| Fault::IdExhausted)
}

pub(crate) fn gc_catch_unwind<F, R>(f: F) -> Result<R, GcPanic>
where
    F: FnOnce() -> R,
{
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
}

pub(crate) fn gc_resume_unwind(payload: GcPanic) -> ! {
    std::panic::resume_unwind(payload)
}

pub(crate) fn gc_initialize_trace() -> Result<(), Fault> {
    let Some(trace) = TRACE
        .get_or_init(|| TraceState::from_env().map(GcMutex::new))
        .as_ref()
    else {
        return Ok(());
    };
    trace.lock().map_err(|_| Fault::TracePoisoned)?.persist()
}

struct StderrWriter<'a> {
    stderr: std::io::StderrLock<'a>,
}

impl std::fmt::Write for StderrWriter<'_> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        self.stderr
            .write_all(value.as_bytes())
            .map_err(|_| std::fmt::Error)
    }
}

pub(crate) fn gc_runtime_or_exit(fault: Fault) -> ! {
    let stderr = std::io::stderr();
    let mut writer = StderrWriter {
        stderr: stderr.lock(),
    };
    let _ = super::gc_write_failure(&mut writer, &fault);
    std::process::exit(1);
}

pub(crate) fn gc_record_memory_ledger(site: PromotionSite) {
    let repairs = [
        "own the value directly",
        "represent identity-bearing links as Id<T>",
        "use Pool<T> when the lifetime is bounded",
    ];
    let _ = crate::jet_mem::jet_memory_ledger_record(crate::jet_mem::MemoryLedgerWitness {
        kind: "gc",
        code: "gc",
        source: site.source,
        span_start: site.span_start,
        span_end: site.span_end,
        byte_spans: !site.source.starts_with('<'),
        scope: site.scope,
        provenance: site.policy_provenance,
        detail: site.reason,
        expected: None,
        repairs: &repairs,
    });
}

pub(crate) fn gc_trace_promotion(id: ObjectId, site: PromotionSite) -> Result<(), Fault> {
    let Some(trace) = TRACE
        .get_or_init(|| TraceState::from_env().map(GcMutex::new))
        .as_ref()
    else {
        return Ok(());
    };
    trace
        .lock()
        .map_err(|_| Fault::TracePoisoned)?
        .record(id, site)
}

pub(crate) fn gc_trace_collection(reclaimed: &[ObjectId]) -> Result<(), Fault> {
    let Some(trace) = TRACE.get().and_then(Option::as_ref) else {
        return Ok(());
    };
    trace
        .lock()
        .map_err(|_| Fault::TracePoisoned)?
        .record_collection(reclaimed)
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TraceKey {
    source: String,
    span_start: u64,
    span_end: u64,
    scope: String,
    policy_provenance: String,
    reason: String,
    type_name: String,
    bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TraceIdentity {
    id: ObjectId,
    retained: bool,
}

struct TraceState {
    path: PathBuf,
    project: String,
    pid: u32,
    started_unix_ms: u128,
    updated_unix_ms: u128,
    complete: bool,
    dropped_promotions: u64,
    collections: u64,
    identity_count: usize,
    sites: BTreeMap<TraceKey, Vec<TraceIdentity>>,
}

impl TraceState {
    fn from_env() -> Option<Self> {
        let path = std::env::var_os("JET_GC_TRACE").filter(|value| !value.is_empty())?;
        let project = std::env::var("JET_GC_PROJECT").unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let now = unix_ms();
        Some(Self {
            path: PathBuf::from(path),
            project,
            pid: std::process::id(),
            started_unix_ms: now,
            updated_unix_ms: now,
            complete: true,
            dropped_promotions: 0,
            collections: 0,
            identity_count: 0,
            sites: BTreeMap::new(),
        })
    }

    fn record(&mut self, id: ObjectId, site: PromotionSite) -> Result<(), Fault> {
        if site.span_start > site.span_end
            || [
                site.source,
                site.scope,
                site.policy_provenance,
                site.reason,
                site.type_name,
            ]
            .iter()
            .any(|value| {
                value.is_empty()
                    || value.len() > MAX_TRACE_FIELD_BYTES
                    || value.chars().any(char::is_control)
            })
        {
            self.drop_promotion();
            return self.persist();
        }
        let key = TraceKey {
            source: site.source.to_string(),
            span_start: site.span_start,
            span_end: site.span_end,
            scope: site.scope.to_string(),
            policy_provenance: site.policy_provenance.to_string(),
            reason: site.reason.to_string(),
            type_name: site.type_name.to_string(),
            bytes: site.bytes,
        };
        if self.identity_count >= MAX_TRACE_IDENTITIES
            || (!self.sites.contains_key(&key) && self.sites.len() >= MAX_TRACE_SITES)
        {
            self.drop_promotion();
            return self.persist();
        }
        self.sites
            .entry(key.clone())
            .or_default()
            .push(TraceIdentity { id, retained: true });
        self.identity_count += 1;
        if self.render().len() > MAX_TRACE_BYTES {
            self.remove_last(&key);
            self.drop_promotion();
            return self.persist();
        }
        if let Err(fault) = self.persist() {
            self.remove_last(&key);
            return Err(fault);
        }
        Ok(())
    }

    fn remove_last(&mut self, key: &TraceKey) {
        let remove_site = if let Some(identities) = self.sites.get_mut(key) {
            identities.pop();
            identities.is_empty()
        } else {
            false
        };
        if remove_site {
            self.sites.remove(key);
        }
        self.identity_count = self.identity_count.saturating_sub(1);
    }

    fn record_collection(&mut self, reclaimed: &[ObjectId]) -> Result<(), Fault> {
        self.collections = self.collections.saturating_add(1);
        let reclaimed: BTreeSet<ObjectId> = reclaimed.iter().copied().collect();
        for identities in self.sites.values_mut() {
            for identity in identities {
                if identity.retained && reclaimed.contains(&identity.id) {
                    identity.retained = false;
                }
            }
        }
        self.persist()
    }

    fn drop_promotion(&mut self) {
        self.complete = false;
        self.dropped_promotions = self.dropped_promotions.saturating_add(1);
    }

    fn persist(&mut self) -> Result<(), Fault> {
        self.updated_unix_ms = unix_ms();
        let json = self.render();
        if json.len() > MAX_TRACE_BYTES {
            return Err(Fault::TraceIo(
                "GC trace exceeds its 4 MiB safety limit".to_string(),
            ));
        }
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        {
            let _parent_existed = match std::fs::symlink_metadata(parent) {
                Ok(metadata) if !metadata.file_type().is_dir() => {
                    return Err(Fault::TraceIo(
                        "GC trace directory path is not a directory".to_string(),
                    ));
                }
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => {
                    return Err(Fault::TraceIo(format!(
                        "cannot inspect GC trace directory: {error}"
                    )));
                }
            };
            std::fs::create_dir_all(parent).map_err(|error| {
                Fault::TraceIo(format!("cannot create GC trace directory: {error}"))
            })?;
            #[cfg(unix)]
            if !_parent_existed {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).map_err(
                    |error| Fault::TraceIo(format!("cannot secure GC trace directory: {error}")),
                )?;
            }
        }
        let _existing_metadata = match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(Fault::TraceIo(
                    "GC trace path is not a regular file".to_string(),
                ));
            }
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(Fault::TraceIo(format!("cannot inspect GC trace: {error}")));
            }
        };
        let file_name = self
            .path
            .file_name()
            .ok_or_else(|| Fault::TraceIo("GC trace path has no file name".to_string()))?;
        let mut temp = None;
        for _ in 0..64 {
            let sequence = NEXT_TRACE_TEMP.fetch_add(1, Ordering::Relaxed);
            let temp_name = format!(
                ".{}.tmp.{}.{}",
                file_name.to_string_lossy(),
                std::process::id(),
                sequence
            );
            let temp_path = parent.join(temp_name);
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(0o400000);
            }
            match options.open(&temp_path) {
                Ok(file) => {
                    temp = Some((temp_path, file));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(Fault::TraceIo(format!(
                        "cannot create GC trace temporary file: {error}"
                    )));
                }
            }
        }
        let (temp_path, mut file) = temp.ok_or_else(|| {
            Fault::TraceIo("cannot reserve a GC trace temporary file".to_string())
        })?;
        #[cfg(unix)]
        if let Some(existing) = &_existing_metadata {
            use std::os::unix::fs::MetadataExt;
            let created = match file.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    drop(file);
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(Fault::TraceIo(format!(
                        "cannot inspect GC trace temporary file: {error}"
                    )));
                }
            };
            if existing.uid() != created.uid() {
                drop(file);
                let _ = std::fs::remove_file(&temp_path);
                return Err(Fault::TraceIo(
                    "GC trace path is owned by a different user".to_string(),
                ));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
                drop(file);
                let _ = std::fs::remove_file(&temp_path);
                return Err(Fault::TraceIo(format!("cannot secure GC trace: {error}")));
            }
        }
        let write = file
            .write_all(json.as_bytes())
            .and_then(|_| file.sync_all());
        drop(file);
        let persist = write
            .and_then(|_| std::fs::rename(&temp_path, &self.path))
            .and_then(|_| sync_trace_parent(parent));
        if let Err(error) = persist {
            let _ = std::fs::remove_file(&temp_path);
            return Err(Fault::TraceIo(format!("cannot persist GC trace: {error}")));
        }
        Ok(())
    }

    fn render(&self) -> String {
        let mut sites = String::new();
        for (index, (key, identities)) in self.sites.iter().enumerate() {
            if index != 0 {
                sites.push(',');
            }
            let retained = identities
                .iter()
                .filter(|identity| identity.retained)
                .count();
            let identity_json = identities
                .iter()
                .map(|identity| {
                    format!(
                        "{{\"identity\":{},\"retained\":{}}}",
                        identity.id.get(),
                        identity.retained
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            sites.push_str(&format!(
                "{{\"source\":\"{}\",\"span_start\":{},\"span_end\":{},\"scope\":\"{}\",\"policy_provenance\":\"{}\",\"reason\":\"{}\",\"type_name\":\"{}\",\"bytes\":{},\"allocations\":{},\"retained\":{},\"identities\":[{}]}}",
                trace_escape(&key.source), key.span_start, key.span_end,
                trace_escape(&key.scope), trace_escape(&key.policy_provenance),
                trace_escape(&key.reason), trace_escape(&key.type_name),
                key.bytes, identities.len(), retained, identity_json
            ));
        }
        format!(
            "{{\"schema\":\"{TRACE_SCHEMA}\",\"version\":{TRACE_VERSION},\"project\":\"{}\",\"pid\":{},\"started_unix_ms\":{},\"updated_unix_ms\":{},\"complete\":{},\"dropped_promotions\":{},\"collections\":{},\"sites\":[{}]}}\n",
            trace_escape(&self.project), self.pid, self.started_unix_ms,
            self.updated_unix_ms, self.complete, self.dropped_promotions,
            self.collections, sites
        )
    }
}

#[cfg(unix)]
fn sync_trace_parent(parent: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_trace_parent(_parent: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn trace_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}
