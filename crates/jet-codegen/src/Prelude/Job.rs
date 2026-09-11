/// D-JOB-SUBCMD1=C: one job table and selector serve AOT binaries and the
/// generated argv boundary. Engines only marshal argv into this Prelude API.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JetJobScope {
    Dev,
    Ship,
    Internal,
}
/// Reserved argv marker used by in-process devtools to dispatch an internal
/// job by name without exposing that name as a user-facing subcommand.
pub const JET_JOB_PRIVATE_DISPATCH_FLAG: &str = "--__jet-private-job";


/// D-SCHEDULE1: the checked `EverySchedule` value carried into the runtime.
/// The generated table stores this value instead of re-reading marker text;
/// every consumer uses the same resolved duration or wall-clock minute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetJobSchedule {
    Duration { nanos: i64 },
    WallClockTime { hour: u8, minute: u8 },
}

/// One shared due clock for dev and service lifecycle consumers. The timer
/// wake-up itself belongs to `Prelude/Scheduler.rs`; this value owns only the
/// schedule decision and last-fire facts.
pub struct JetJobClock {
    last_interval_run: std::collections::HashMap<String, std::time::Instant>,
    last_daily_run_day: std::collections::HashMap<String, u64>,
}

impl JetJobClock {
    pub fn new() -> Self {
        Self {
            last_interval_run: std::collections::HashMap::new(),
            last_daily_run_day: std::collections::HashMap::new(),
        }
    }

    /// Return the names due at the current wall time. A duration schedule fires
    /// immediately on its first check, then after its resolved interval.
    pub fn due(&mut self, jobs: &[(&str, JetJobSchedule)]) -> Vec<String> {
        let unix_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.due_at(jobs, unix_secs)
    }

    /// Testable due decision with the wall-clock seconds supplied by the
    /// caller. The monotonic instant still measures duration intervals.
    pub fn due_at(&mut self, jobs: &[(&str, JetJobSchedule)], unix_secs: u64) -> Vec<String> {
        let now = std::time::Instant::now();
        let day = unix_secs / 86_400;
        let secs_of_day = unix_secs % 86_400;
        let mut fired = Vec::new();
        for (name, schedule) in jobs {
            match *schedule {
                JetJobSchedule::Duration { nanos } => {
                    let due = match self.last_interval_run.get(*name) {
                        None => true,
                        Some(last) => now.duration_since(*last).as_nanos() >= nanos.max(0) as u128,
                    };
                    if due {
                        self.last_interval_run.insert((*name).to_string(), now);
                        fired.push((*name).to_string());
                    }
                }
                JetJobSchedule::WallClockTime { hour, minute } => {
                    let target_secs = hour as u64 * 3600 + minute as u64 * 60;
                    let in_window =
                        secs_of_day >= target_secs && secs_of_day < target_secs + 60;
                    let already_ran_today = self.last_daily_run_day.get(*name) == Some(&day);
                    if in_window && !already_ran_today {
                        self.last_daily_run_day.insert((*name).to_string(), day);
                        fired.push((*name).to_string());
                    }
                }
            }
        }
        fired
    }
}

#[derive(Clone, Copy)]
pub struct JetJobEntry {
    pub name: &'static str,
    pub scope: JetJobScope,
    pub schedule: Option<JetJobSchedule>,
    pub invoke: fn(&str, &[String]),
}

/// Adapter emitted beside a checked `#Job` wrapper. The wrapper owns
/// canonical CBOR decoding and the typed call; the queue Prelude owns claim,
/// lease, and acknowledgement state.
pub type JetJobQueueInvoke =
    fn(&JetJobPayload) -> Result<JetJobResult, JetJobError>;
/// The checked dispatch strategy carried by a MIR job row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetJobDispatch {
    Direct,
    Spawn,
    Scheduled,
}

/// The checked cache policy carried by a MIR job row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetJobCachePolicy {
    Uncached,
    Local,
    Shared,
}

/// The checked skip rule carried by a MIR job row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JetJobSkip {
    Always(&'static str),
    UnlessPlatform(&'static str),
}

/// Complete checked job metadata. The legacy `JetJobEntry` remains available
/// for the source emitter; MIR adapters use this typed envelope so runtime
/// dispatch cannot silently discard job policy.
#[derive(Clone, Copy)]
pub struct JetJobSpec {
    pub entry: JetJobEntry,
    /// The checked schema identity of the one queue payload parameter.  This
    /// is matched together with `entry.name`; payload type alone is not a job
    /// identity.
    pub payload_type: Option<&'static str>,
    pub queue_invoke: Option<JetJobQueueInvoke>,
    pub dispatch: JetJobDispatch,
    pub packages: &'static [&'static str],
    pub working_directory: Option<&'static str>,
    pub input_paths: &'static [&'static str],
    pub output_paths: &'static [&'static str],
    pub skip: Option<JetJobSkip>,
    pub cache: JetJobCachePolicy,
    pub limits: &'static [(&'static str, &'static str)],
    /// D-DX-JOBGRAPH1=A: checked predecessor names in the one source
    /// namespace. The scheduler resolves these before invoking any job.
    pub after: &'static [&'static str],
    /// D-DX-JOBGRAPH1=A: maximum number of independent jobs admitted by one
    /// graph run. The checked default is one.
    pub parallel: usize,
    /// Typed argv validation generated from the same MIR input rows as the
    /// invocation wrapper. `None` is retained only for legacy adapters.
    pub validate: Option<fn(&[String]) -> Result<(), String>>,
}

thread_local! {
    /// The one invocation-owned checked table.  It is a stack rather than a
    /// process-wide name map so nested test/host invocations restore the
    /// previous owner exactly.
    static JET_JOB_ACTIVE_SPECS: std::cell::RefCell<Vec<&'static [JetJobSpec]>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub struct JetJobRegistryGuard;

impl Drop for JetJobRegistryGuard {
    fn drop(&mut self) {
        JET_JOB_ACTIVE_SPECS.with(|registry| {
            let _ = registry.borrow_mut().pop();
        });
    }
}

/// Register one generated table for the duration of its owning invocation.
/// The table is static in generated programs, so service lifecycle callbacks
/// may safely borrow it after the argv dispatch boundary.
pub fn jet_job_register_specs(jobs: &'static [JetJobSpec]) -> JetJobRegistryGuard {
    JET_JOB_ACTIVE_SPECS.with(|registry| registry.borrow_mut().push(jobs));
    JetJobRegistryGuard
}

pub fn jet_job_active_specs() -> Option<&'static [JetJobSpec]> {
    JET_JOB_ACTIVE_SPECS.with(|registry| registry.borrow().last().copied())
}
pub trait JetJobEventSink: Send + Sync {
    fn publish(&self, event: crate::JetDevtoolsEvent);
}

pub type JetJobEventSinkHandle = std::sync::Arc<dyn JetJobEventSink>;

/// Bounded in-process storage useful as the default sink and for embedders that
/// want to pull lifecycle facts after a run.
pub struct JetJobEventCollector {
    capacity: usize,
    events: std::sync::Mutex<std::collections::VecDeque<crate::JetDevtoolsEvent>>,
}

impl JetJobEventCollector {
    pub fn new() -> Self {
        Self::with_capacity(crate::JET_DEVTOOLS_MAX_HISTORY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, crate::JET_DEVTOOLS_MAX_HISTORY),
            events: std::sync::Mutex::new(std::collections::VecDeque::new()),
        }
    }

    pub fn snapshot(&self) -> Vec<crate::JetDevtoolsEvent> {
        self.events
            .lock()
            .expect("job event collector poisoned")
            .iter()
            .cloned()
            .collect()
    }

    pub fn take(&self) -> Vec<crate::JetDevtoolsEvent> {
        self.events
            .lock()
            .expect("job event collector poisoned")
            .drain(..)
            .collect()
    }
}

impl Default for JetJobEventCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl JetJobEventSink for JetJobEventCollector {
    fn publish(&self, event: crate::JetDevtoolsEvent) {
        let mut events = self.events.lock().expect("job event collector poisoned");
        if events.len() == self.capacity {
            events.pop_front();
        }
        events.push_back(event);
    }
}

static JET_JOB_EVENT_COLLECTOR: std::sync::LazyLock<
    std::sync::Arc<JetJobEventCollector>,
> = std::sync::LazyLock::new(|| std::sync::Arc::new(JetJobEventCollector::new()));
static JET_JOB_EVENT_SINK: std::sync::LazyLock<
    std::sync::Mutex<Option<JetJobEventSinkHandle>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(None));

/// Install the process-wide sink used by the ordinary generated dispatch path.
/// Passing `None` restores the bounded in-process collector.
pub fn jet_job_set_event_sink(sink: Option<JetJobEventSinkHandle>) -> Option<JetJobEventSinkHandle> {
    let mut current = JET_JOB_EVENT_SINK
        .lock()
        .expect("job event sink poisoned");
    std::mem::replace(&mut *current, sink)
}

pub fn jet_job_event_history() -> Vec<crate::JetDevtoolsEvent> {
    JET_JOB_EVENT_COLLECTOR.snapshot()
}

fn jet_job_event_sink() -> JetJobEventSinkHandle {
    JET_JOB_EVENT_SINK
        .lock()
        .expect("job event sink poisoned")
        .clone()
        .unwrap_or_else(|| JET_JOB_EVENT_COLLECTOR.clone())
}


/// Metadata visible to code executing through the shared job Prelude.
#[derive(Clone, Copy)]
pub struct JetJobContext {
    pub name: &'static str,
    pub dispatch: JetJobDispatch,
    pub packages: &'static [&'static str],
    pub working_directory: Option<&'static str>,
    pub input_paths: &'static [&'static str],
    pub output_paths: &'static [&'static str],
    pub skip: Option<JetJobSkip>,
    pub cache: JetJobCachePolicy,
    pub limits: &'static [(&'static str, &'static str)],
    pub after: &'static [&'static str],
    pub parallel: usize,
}

thread_local! {
    static JET_JOB_CONTEXT: std::cell::RefCell<Option<JetJobContext>> =
        const { std::cell::RefCell::new(None) };
}

pub fn jet_job_context() -> Option<JetJobContext> {
    JET_JOB_CONTEXT.with(|context| *context.borrow())
}
fn jet_job_no_scope() -> Result<Option<()>, String> {
    Ok(None)
}

// ponytail: the generated invocation ABI only accepts `(&str, &[String])`, so
// cwd cannot be passed as a per-call `Command` setting yet. Serialize the
// current-directory mutation across every in-process job invocation; the
// upgrade path is a cwd-aware invocation ABI or child process boundary.
static JET_JOB_CWD_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

struct JetJobDirectoryGuard {
    previous: Option<std::path::PathBuf>,
    lock: Option<std::sync::MutexGuard<'static, ()>>,
}

impl JetJobDirectoryGuard {
    fn enter(directory: Option<&str>) -> Self {
        let lock = JET_JOB_CWD_LOCK.lock().expect("job cwd lock poisoned");
        let previous = directory.and_then(|_| std::env::current_dir().ok());
        if let Some(directory) = directory {
            let _ = std::env::set_current_dir(directory);
        }
        Self {
            previous,
            lock: Some(lock),
        }
    }
}

impl Drop for JetJobDirectoryGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            let _ = std::env::set_current_dir(previous);
        }
        let _ = self.lock.take();
    }
}


fn jet_job_platform_matches(platform: &str) -> bool {
    match platform {
        "Linux" | "linux" => cfg!(target_os = "linux"),
        "MacOS" | "macOS" | "macos" => cfg!(target_os = "macos"),
        "Windows" | "windows" => cfg!(target_os = "windows"),
        "FreeBSD" | "freebsd" => cfg!(target_os = "freebsd"),
        _ => false,
    }
}

fn jet_job_is_skipped(job: &JetJobSpec) -> bool {
    match job.skip {
        None => false,
        Some(JetJobSkip::Always(_)) => true,
        Some(JetJobSkip::UnlessPlatform(platform)) => !jet_job_platform_matches(platform),
    }
}

/// A checked, explainable freshness observation for one typed job. The
/// fingerprint is deliberately independent of process identity so a cache
/// decision can be recorded by an embedding host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetJobFreshness {
    pub fresh: bool,
    pub reason: String,
    pub fingerprint: u64,
}

fn jet_job_base_directory(job: &JetJobSpec) -> std::path::PathBuf {
    let current = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    match job.working_directory {
        Some(directory) => {
            let path = std::path::Path::new(directory);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                current.join(path)
            }
        }
        None => current,
    }
}

fn jet_job_glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut row = vec![false; value.len() + 1];
    row[0] = true;
    for &token in pattern {
        let mut next = vec![false; value.len() + 1];
        if token == b'*' {
            next[0] = row[0];
            for index in 1..=value.len() {
                next[index] = row[index] || next[index - 1];
            }
        } else {
            for index in 1..=value.len() {
                if token == b'?' || token == value[index - 1] {
                    next[index] = row[index - 1];
                }
            }
        }
        row = next;
    }
    row[value.len()]
}

fn jet_job_declared_paths(job: &JetJobSpec, pattern: &str) -> Vec<std::path::PathBuf> {
    let base = jet_job_base_directory(job);
    let normalized = pattern.replace('\\', "/");
    if !normalized.contains('*') && !normalized.contains('?') {
        return vec![base.join(pattern)];
    }
    let mut pending = vec![base.clone()];
    let mut paths = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else { continue };
        let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path.clone());
            }
            let Ok(relative) = path.strip_prefix(&base) else { continue };
            let relative = relative.to_string_lossy().replace('\\', "/");
            if jet_job_glob_match(&normalized, &relative) {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn jet_job_latest_modified(path: &std::path::Path) -> Option<std::time::SystemTime> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    let mut latest = metadata.modified().ok();
    if metadata.is_dir() {
        let mut entries = std::fs::read_dir(path)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        entries.sort();
        for child in entries {
            if let Some(modified) = jet_job_latest_modified(&child) {
                latest = Some(latest.map_or(modified, |current| current.max(modified)));
            }
        }
    }
    latest
}

fn jet_job_hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(1_099_511_628_211);
    }
}

fn jet_job_hash_path(path: &std::path::Path, hash: &mut u64) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        jet_job_hash_bytes(hash, b"<missing>");
        return;
    };
    if metadata.file_type().is_symlink() {
        jet_job_hash_bytes(hash, b"<symlink>");
        if let Ok(target) = std::fs::read_link(path) {
            jet_job_hash_bytes(hash, target.to_string_lossy().as_bytes());
        }
    } else if metadata.is_file() {
        if let Ok(bytes) = std::fs::read(path) {
            jet_job_hash_bytes(hash, &bytes);
        }
    } else if metadata.is_dir() {
        let mut entries = std::fs::read_dir(path)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(Result::ok).map(|entry| entry.path()))
            .collect::<Vec<_>>();
        entries.sort();
        for child in entries {
            jet_job_hash_path(&child, hash);
        }
    }
}

/// Explain the freshness decision without running the job.
pub fn jet_job_freshness(job: &JetJobSpec) -> JetJobFreshness {
    let mut fingerprint = 14_695_981_039_346_656_037u64;
    let mut input_paths = Vec::new();
    let mut output_paths = Vec::new();
    let mut missing_input = None;
    let mut missing_output = None;
    let mut newest_input: Option<std::time::SystemTime> = None;
    let mut newest_output: Option<std::time::SystemTime> = None;

    for pattern in job.input_paths {
        jet_job_hash_bytes(&mut fingerprint, pattern.as_bytes());
        let paths = jet_job_declared_paths(job, pattern);
        if paths.is_empty() {
            missing_input = Some((*pattern).to_string());
        }
        for path in paths {
            jet_job_hash_path(&path, &mut fingerprint);
            if !path.exists() {
                missing_input = Some((*pattern).to_string());
            }
            if let Some(modified) = jet_job_latest_modified(&path) {
                newest_input = Some(newest_input.map_or(modified, |current| current.max(modified)));
            }
            input_paths.push(path);
        }
    }
    for pattern in job.output_paths {
        jet_job_hash_bytes(&mut fingerprint, pattern.as_bytes());
        let paths = jet_job_declared_paths(job, pattern);
        if paths.is_empty() {
            missing_output = Some((*pattern).to_string());
        }
        for path in paths {
            jet_job_hash_path(&path, &mut fingerprint);
            if !path.exists() {
                missing_output = Some((*pattern).to_string());
            }
            if let Some(modified) = jet_job_latest_modified(&path) {
                newest_output = Some(newest_output.map_or(modified, |current| current.max(modified)));
            }
            output_paths.push(path);
        }
    }
    let (fresh, reason) = if job.cache == JetJobCachePolicy::Uncached {
        (false, "cache policy is uncached".to_string())
    } else if job.output_paths.is_empty() {
        (false, "no declared outputs".to_string())
    } else if let Some(path) = missing_output {
        (false, format!("output `{path}` is missing"))
    } else if let Some(path) = missing_input {
        (false, format!("input `{path}` is missing"))
    } else if newest_input > newest_output {
        (false, "an input is newer than the declared outputs".to_string())
    } else {
        (true, "all declared outputs are newer than declared inputs".to_string())
    };
    let _ = (input_paths, output_paths);
    JetJobFreshness {
        fresh,
        reason,
        fingerprint,
    }
}

fn jet_job_cache_claim(job: &JetJobSpec) -> bool {
    !jet_job_freshness(job).fresh
}

fn jet_job_timeout(job: &JetJobSpec) -> Option<std::time::Duration> {
    job.limits
        .iter()
        .find(|(name, _)| matches!(*name, "timeout" | "timeout_ms"))
        .and_then(|(_, value)| value.parse::<u64>().ok())
        .map(std::time::Duration::from_millis)
}

fn jet_job_limit<'a>(job: &'a JetJobSpec, wanted: &str) -> Option<&'a str> {
    job.limits
        .iter()
        .find(|(name, _)| *name == wanted)
        .map(|(_, value)| *value)
}

fn jet_job_retries(job: &JetJobSpec) -> u32 {
    jet_job_limit(job, "retry")
        .or_else(|| jet_job_limit(job, "retries"))
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
        .min(16)
}

fn jet_job_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn jet_job_json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len().saturating_add(2));
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn jet_job_emit(
    sink: &JetJobEventSinkHandle,
    timestamp_ms: u64,
    kind: &str,
    state: &str,
    job_id: &str,
    name: &str,
    queue: &str,
    attempts: u32,
    duration_ms: Option<u64>,
    worker: Option<&str>,
    request_id: Option<&str>,
    failure: Option<&str>,
) {
    let duration = duration_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    let worker = worker
        .map(jet_job_json_string)
        .unwrap_or_else(|| "null".to_string());
    let request_id = request_id
        .map(jet_job_json_string)
        .unwrap_or_else(|| "null".to_string());
    let failure = failure
        .map(jet_job_json_string)
        .unwrap_or_else(|| "null".to_string());
    let fields = format!(
        "{{\"event\":{},\"state\":{},\"job_id\":{},\"name\":{},\"queue\":{},\"attempts\":{},\"duration_ms\":{},\"worker\":{},\"request_id\":{},\"failure\":{}}}",
        jet_job_json_string(kind),
        jet_job_json_string(state),
        jet_job_json_string(job_id),
        jet_job_json_string(name),
        jet_job_json_string(queue),
        attempts,
        duration,
        worker,
        request_id,
        failure,
    );
    if let Ok(event) =
        crate::JetDevtoolsEvent::from_parts(timestamp_ms, "job-runtime", "Job", job_id, fields)
    {
        // The ordinary job sink remains available to embedders, while the
        // canonical devtools sink gives the live Jobs panel the same typed
        // observation without capturing a payload.
        sink.publish(event.clone());
        jet_foundation::Devtools::jet_devtools_publish_event(event);
    }
}

enum JetJobRunOutcome {
    Complete,
    Fresh(String),
    Skipped(String),
    TimedOut,
    Panic(Box<dyn std::any::Any + Send>),
}

static JET_JOB_NEXT_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

fn jet_job_run_spec<F, G>(
    job: JetJobSpec,
    program: String,
    argv: Vec<String>,
    sink: JetJobEventSinkHandle,
    scope_enter: F,
) -> JetJobRunOutcome
where
    F: Fn() -> Result<Option<G>, String> + Clone + Send + Sync + 'static,
    G: Send + 'static,
{
    let id = JET_JOB_NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let job_id = format!("{}#{}", job.entry.name, id);
    let queue = jet_job_limit(&job, "queue")
        .unwrap_or(job.entry.name)
        .to_string();
    if jet_job_is_skipped(&job) {
        let reason = match job.skip {
            Some(JetJobSkip::Always(reason)) => reason.to_string(),
            Some(JetJobSkip::UnlessPlatform(platform)) => {
                format!("host platform does not match {platform}")
            }
            None => "declared skip rule".to_string(),
        };
        jet_job_emit(
            &sink,
            jet_job_now_ms(),
            "skip",
            "skipped",
            &job_id,
            job.entry.name,
            &queue,
            0,
            None,
            None,
            None,
            Some(&reason),
        );
        return JetJobRunOutcome::Skipped(reason);
    }
    // Input/output patterns are relative to the declared job directory, so
    // enter it before cache freshness and invocation.
    let _directory = JetJobDirectoryGuard::enter(job.working_directory);
    let freshness = jet_job_freshness(&job);
    if freshness.fresh && !jet_job_cache_claim(&job) {
        jet_job_emit(
            &sink,
            jet_job_now_ms(),
            "fresh",
            "fresh",
            &job_id,
            job.entry.name,
            &queue,
            0,
            None,
            None,
            None,
            Some(&freshness.reason),
        );
        return JetJobRunOutcome::Fresh(freshness.reason);
    }
    jet_job_emit(
        &sink,
        jet_job_now_ms(),
        "enqueue",
        "enqueued",
        &job_id,
        job.entry.name,
        &queue,
        0,
        None,
        None,
        None,
        None,
    );
    let context = JetJobContext {
        name: job.entry.name,
        dispatch: job.dispatch,
        packages: job.packages,
        working_directory: job.working_directory,
        input_paths: job.input_paths,
        output_paths: job.output_paths,
        skip: job.skip,
        cache: job.cache,
        limits: job.limits,
        after: job.after,
        parallel: job.parallel,
    };
    let previous = JET_JOB_CONTEXT.with(|slot| slot.replace(Some(context)));
    let run_started = std::time::Instant::now();
    let outcome = (|| {
        let retries = jet_job_retries(&job);
        let mut attempts = 0u32;
        loop {
            attempts = attempts.saturating_add(1);
            jet_job_emit(
                &sink,
                jet_job_now_ms(),
                "start",
                "started",
                &job_id,
                job.entry.name,
                &queue,
                attempts,
                None,
                Some(&program),
                None,
                None,
            );
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _scope = scope_enter()
                    .unwrap_or_else(|error| panic!("job service scope rejected: {error}"));
                (job.entry.invoke)(&program, &argv);
            }));
            let duration_ms = run_started
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX);
            let timed_out = jet_job_timeout(&job)
                .is_some_and(|timeout| run_started.elapsed() > timeout);
            if timed_out {
                jet_job_emit(
                    &sink,
                    jet_job_now_ms(),
                    "fail",
                    "failed",
                    &job_id,
                    job.entry.name,
                    &queue,
                    attempts,
                    Some(duration_ms),
                    Some(&program),
                    None,
                    Some("timed_out"),
                );
                eprintln!("job `{}` exceeded its configured timeout", job.entry.name);
                return match result {
                    Ok(()) => JetJobRunOutcome::TimedOut,
                    Err(payload) => JetJobRunOutcome::Panic(payload),
                };
            }
            match result {
                Ok(()) => {
                    jet_job_emit(
                        &sink,
                        jet_job_now_ms(),
                        "complete",
                        "completed",
                        &job_id,
                        job.entry.name,
                        &queue,
                        attempts,
                        Some(duration_ms),
                        Some(&program),
                        None,
                        None,
                    );
                    return JetJobRunOutcome::Complete;
                }
                Err(_payload) if attempts <= retries => {
                    jet_job_emit(
                        &sink,
                        jet_job_now_ms(),
                        "retry",
                        "retrying",
                        &job_id,
                        job.entry.name,
                        &queue,
                        attempts,
                        Some(duration_ms),
                        Some(&program),
                        None,
                        Some("error"),
                    );
                }
                Err(payload) => {
                    jet_job_emit(
                        &sink,
                        jet_job_now_ms(),
                        "fail",
                        "failed",
                        &job_id,
                        job.entry.name,
                        &queue,
                        attempts,
                        Some(duration_ms),
                        Some(&program),
                        None,
                        Some("error"),
                    );
                    return JetJobRunOutcome::Panic(payload);
                }
            }
        }
    })();
    JET_JOB_CONTEXT.with(|slot| {
        slot.replace(previous);
    });
    outcome
}

fn jet_job_dependency_indices(jobs: &[JetJobSpec], index: usize) -> Vec<usize> {
    jobs[index]
        .after
        .iter()
        .filter_map(|dependency| {
            jobs.iter()
                .position(|job| job.entry.name == *dependency)
        })
        .collect()
}

fn jet_job_validate_graph(jobs: &[JetJobSpec]) -> Result<(), String> {
    let mut names = std::collections::BTreeMap::<String, Vec<usize>>::new();
    for (index, job) in jobs.iter().enumerate() {
        if job.parallel == 0 {
            return Err(format!("job `{}` declares a zero parallel bound", job.entry.name));
        }
        names
            .entry(job.entry.name.to_string())
            .or_default()
            .push(index);
    }
    for (index, job) in jobs.iter().enumerate() {
        for dependency in job.after {
            if dependency == &job.entry.name {
                return Err(format!("job `{}` depends on itself", job.entry.name));
            }
            let Some(candidates) = names.get(*dependency) else {
                return Err(format!(
                    "job `{}` depends on unknown job `{dependency}`",
                    job.entry.name
                ));
            };
            if candidates.len() != 1 {
                return Err(format!(
                    "job `{}` has an ambiguous prerequisite `{dependency}`",
                    job.entry.name
                ));
            }
        }
        let _ = index;
    }

    fn visit(
        index: usize,
        jobs: &[JetJobSpec],
        names: &std::collections::BTreeMap<String, Vec<usize>>,
        state: &mut [u8],
        stack: &mut Vec<usize>,
    ) -> Result<(), String> {
        if state[index] == 2 {
            return Ok(());
        }
        if state[index] == 1 {
            let start = stack.iter().position(|candidate| *candidate == index).unwrap_or(0);
            let cycle = stack[start..]
                .iter()
                .map(|candidate| jobs[*candidate].entry.name)
                .chain(std::iter::once(jobs[index].entry.name))
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(format!("job graph contains a cycle: {cycle}"));
        }
        state[index] = 1;
        stack.push(index);
        for dependency in jobs[index].after {
            if let Some(candidate) = names.get(*dependency).and_then(|indices| indices.first()) {
                visit(*candidate, jobs, names, state, stack)?;
            }
        }
        stack.pop();
        state[index] = 2;
        Ok(())
    }

    let mut state = vec![0u8; jobs.len()];
    let mut stack = Vec::new();
    for index in 0..jobs.len() {
        visit(index, jobs, &names, &mut state, &mut stack)?;
    }
    Ok(())
}

fn jet_job_graph_failure(reason: &str) -> ! {
    eprintln!("Error [E1331]: {reason}");
    eprintln!(
        " Why: job dependencies, authority, and arguments are checked before any job effect"
    );
    eprintln!(" Fix: correct the #Job metadata, then run the command again");
    std::process::exit(2)
}

fn jet_job_emit_cancelled(
    job: &JetJobSpec,
    sink: &JetJobEventSinkHandle,
    reason: &str,
) {
    let id = JET_JOB_NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let job_id = format!("{}#{}", job.entry.name, id);
    let queue = jet_job_limit(job, "queue")
        .unwrap_or(job.entry.name)
        .to_string();
    jet_job_emit(
        sink,
        jet_job_now_ms(),
        "cancel",
        "cancelled",
        &job_id,
        job.entry.name,
        &queue,
        0,
        None,
        None,
        None,
        Some(reason),
    );
}

fn jet_job_run_graph<F, G>(
    root: usize,
    jobs: &[JetJobSpec],
    program: String,
    root_args: Vec<String>,
    sink: JetJobEventSinkHandle,
    scope_enter: F,
)
where
    F: Fn() -> Result<Option<G>, String> + Clone + Send + Sync + 'static,
    G: Send + 'static,
{
    jet_job_run_graph_roots(
        std::slice::from_ref(&root),
        jobs,
        program,
        root_args,
        sink,
        scope_enter,
    );
}

/// Run several roots against one closure. A service tick uses this instead of
/// invoking the graph once per due root, so a shared predecessor is admitted
/// exactly once.
fn jet_job_run_graph_roots<F, G>(
    roots: &[usize],
    jobs: &[JetJobSpec],
    program: String,
    root_args: Vec<String>,
    sink: JetJobEventSinkHandle,
    scope_enter: F,
)
where
    F: Fn() -> Result<Option<G>, String> + Clone + Send + Sync + 'static,
    G: Send + 'static,
{
    let Some(primary_root) = roots.first().copied() else {
        return;
    };
    let mut closure = std::collections::BTreeSet::new();
    fn collect(
        index: usize,
        jobs: &[JetJobSpec],
        closure: &mut std::collections::BTreeSet<usize>,
    ) {
        if !closure.insert(index) {
            return;
        }
        for dependency in jet_job_dependency_indices(jobs, index) {
            collect(dependency, jobs, closure);
        }
    }
    for root in roots {
        collect(*root, jobs, &mut closure);
    }
    let mut pending = closure;
    let mut status = std::collections::BTreeMap::<usize, bool>::new();
    let limit = pending
        .iter()
        .map(|index| jobs[*index].parallel)
        .max()
        .unwrap_or(1)
        .max(1);

    while !pending.is_empty() {
        let pending_now = pending.iter().copied().collect::<Vec<_>>();
        for index in pending_now {
            let blocked = jet_job_dependency_indices(jobs, index)
                .into_iter()
                .any(|dependency| status.get(&dependency) == Some(&false));
            if blocked {
                pending.remove(&index);
                status.insert(index, false);
                jet_job_emit_cancelled(
                    &jobs[index],
                    &sink,
                    "a prerequisite failed or was cancelled",
                );
            }
        }
        let mut ready = pending
            .iter()
            .copied()
            .filter(|index| {
                jet_job_dependency_indices(jobs, *index)
                    .into_iter()
                    .all(|dependency| status.get(&dependency) == Some(&true))
            })
            .collect::<Vec<_>>();
        ready.sort_by(|left, right| {
            jobs[*left]
                .entry
                .name
                .cmp(jobs[*right].entry.name)
                .then_with(|| left.cmp(right))
        });
        if ready.is_empty() {
            for index in pending.iter().copied().collect::<Vec<_>>() {
                pending.remove(&index);
                status.insert(index, false);
                jet_job_emit_cancelled(&jobs[index], &sink, "job graph made no schedulable progress");
            }
            break;
        }
        ready.truncate(limit);
        for index in &ready {
            pending.remove(index);
        }
        let results = std::thread::scope(|scope| {
            let handles = ready
                .iter()
                .map(|index| {
                    let job = jobs[*index];
                    let program = program.clone();
                    let args = if *index == primary_root {
                        root_args.clone()
                    } else {
                        Vec::new()
                    };
                    let sink = sink.clone();
                    let scope_enter = scope_enter.clone();
                    scope.spawn(move || {
                        jet_job_run_spec(job, program, args, sink, scope_enter)
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        JetJobRunOutcome::Panic(Box::new("job worker panicked"))
                    })
                })
                .collect::<Vec<_>>()
        });
        for (index, outcome) in ready.into_iter().zip(results) {
            let succeeded = matches!(
                outcome,
                JetJobRunOutcome::Complete
                    | JetJobRunOutcome::Fresh(_)
                    | JetJobRunOutcome::Skipped(_)
            );
            status.insert(index, succeeded);
        }
    }
}

/// Dispatch a typed MIR job table through the installed canonical sink.
pub fn jet_job_dispatch_specs<F, G>(
    argv: &[String],
    jobs: &[JetJobSpec],
    scope_enter: F,
) -> bool
where
    F: Fn() -> Result<Option<G>, String> + Clone + Send + Sync + 'static,
    G: Send + 'static,
{
    jet_job_dispatch_specs_with_sink(argv, jobs, jet_job_event_sink(), scope_enter)
}

/// Dispatch a typed MIR job table with an explicit lifecycle sink.
pub fn jet_job_dispatch_specs_with_sink<F, G>(
    argv: &[String],
    jobs: &[JetJobSpec],
    sink: JetJobEventSinkHandle,
    scope_enter: F,
) -> bool
where
    F: Fn() -> Result<Option<G>, String> + Clone + Send + Sync + 'static,
    G: Send + 'static,
{
    let specs = jobs
        .iter()
        .map(|job| (job.entry.name, job.entry.scope))
        .collect::<Vec<_>>();
    if specs.is_empty() {
        return false;
    }
    if argv.get(1).map(String::as_str) == Some(JET_JOB_PRIVATE_DISPATCH_FLAG) {
        let Some(name) = argv.get(2).map(String::as_str) else {
            jet_job_graph_failure("private job dispatch is missing a job name");
        };
        let Some(index) = jobs.iter().position(|job| job.entry.name == name) else {
            jet_job_graph_failure(&format!("private job dispatch names unknown job `{name}`"));
        };
        if let Err(reason) = jet_job_validate_graph(jobs) {
            jet_job_graph_failure(&reason);
        }
        let args = argv[3..].to_vec();
        if let Some(validate) = jobs[index].validate {
            if let Err(reason) = validate(&args) {
                jet_job_graph_failure(&format!(
                    "job `{}` received invalid arguments: {reason}",
                    jobs[index].entry.name
                ));
            }
        }
        jet_job_run_graph(
            index,
            jobs,
            argv.first().cloned().unwrap_or_else(|| "program".to_string()),
            args,
            sink,
            scope_enter.clone(),
        );
        return true;
    }
    if !jet_job_has_visible(&specs) {
        let Some(name) = argv.get(1).map(String::as_str) else {
            return false;
        };
        if name.starts_with('-') || !specs.iter().any(|(job_name, _)| *job_name == name) {
            return false;
        }
        jet_job_unknown(argv, &specs, name);
    }
    match jet_job_select(argv, &specs) {
        JetJobSelection::Ordinary => false,
        JetJobSelection::Help => {
            jet_job_help(argv, &specs);
            true
        }
        JetJobSelection::Job(index) => {
            if let Err(reason) = jet_job_validate_graph(jobs) {
                jet_job_graph_failure(&reason);
            }
            let args = argv[1..].to_vec();
            if let Some(validate) = jobs[index].validate {
                if let Err(reason) = validate(&args) {
                    jet_job_graph_failure(&format!(
                        "job `{}` received invalid arguments: {reason}",
                        jobs[index].entry.name
                    ));
                }
            }
            jet_job_run_graph(
                index,
                jobs,
                argv.first().cloned().unwrap_or_else(|| "program".to_string()),
                args,
                sink,
                scope_enter.clone(),
            );
            true
        }
        JetJobSelection::Unknown => {
            let name = argv.get(1).map(String::as_str).unwrap_or("");
            jet_job_unknown(argv, &specs, name);
        }
    }
}

static JET_JOB_SERVICE_CLOCKS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::BTreeMap<String, JetJobClock>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::BTreeMap::new()));

/// Keep one due clock per generated program. A fresh clock on every AOT or
/// resident-JIT tick makes every duration schedule due on every poll.
pub fn jet_job_service_tick_specs_retained<F, G>(
    jobs: &[JetJobSpec],
    program: &str,
    scope_enter: F,
)
where
    F: Fn() -> Result<Option<G>, String> + Clone + Send + Sync + 'static,
    G: Send + 'static,
{
    let mut clocks = JET_JOB_SERVICE_CLOCKS
        .lock()
        .expect("job service clocks poisoned");
    let clock = clocks
        .entry(program.to_string())
        .or_insert_with(JetJobClock::new);
    jet_job_service_tick_specs_with_sink(clock, jobs, program, jet_job_event_sink(), scope_enter);
}

pub fn jet_job_service_tick_specs_with_sink<F, G>(
    clock: &mut JetJobClock,
    jobs: &[JetJobSpec],
    program: &str,
    sink: JetJobEventSinkHandle,
    scope_enter: F,
)
where
    F: Fn() -> Result<Option<G>, String> + Clone + Send + Sync + 'static,
    G: Send + 'static,
{
    if let Err(reason) = jet_job_validate_graph(jobs) {
        eprintln!("Error [E1331]: {reason}");
        return;
    }
    let schedules = jobs
        .iter()
        .filter(|job| jet_job_schedule_enabled(job.entry.scope))
        .filter_map(|job| {
            job.entry
                .schedule
                .map(|schedule| (job.entry.name, schedule))
        })
        .collect::<Vec<_>>();
    let mut due_roots = Vec::new();
    for name in jet_job_schedule_due(clock, &schedules) {
        if let Some(index) = jobs.iter().position(|job| job.entry.name == name.as_str()) {
            let args = Vec::new();
            if let Some(validate) = jobs[index].validate {
                if let Err(reason) = validate(&args) {
                    eprintln!(
                        "Error [E1331]: job `{}` received invalid scheduled arguments: {reason}",
                        jobs[index].entry.name
                    );
                    continue;
                }
            }
            due_roots.push(index);
        }
    }
    jet_job_run_graph_roots(
        &due_roots,
        jobs,
        program.to_string(),
        Vec::new(),
        sink,
        scope_enter,
    );
}
fn jet_job_legacy_specs(jobs: &[JetJobEntry]) -> Vec<JetJobSpec> {
    jobs.iter()
        .copied()
        .map(|entry| JetJobSpec {
            entry,
            payload_type: None,
            queue_invoke: None,
            dispatch: JetJobDispatch::Direct,
            packages: &[],
            working_directory: None,
            input_paths: &[],
            output_paths: &[],
            skip: None,
            cache: JetJobCachePolicy::Uncached,
            limits: &[],
            after: &[],
            parallel: 1,
            validate: None,
        })
        .collect()
}

/// Legacy callers are adapted into the typed spec path instead of invoking a
/// second runtime implementation.
pub fn jet_job_service_tick(clock: &mut JetJobClock, jobs: &[JetJobEntry], program: &str) {
    let specs = jet_job_legacy_specs(jobs);
    jet_job_service_tick_specs_with_sink(clock, &specs, program, jet_job_event_sink(), jet_job_no_scope);
}

/// Shared schedule decision used by AOT, the resident JIT adapter, the TIR
/// evaluator, and `jet dev`.
pub fn jet_job_schedule_due(
    clock: &mut JetJobClock,
    jobs: &[(&str, JetJobSchedule)],
) -> Vec<String> {
    clock.due(jobs)
}

fn jet_job_schedule_enabled(scope: JetJobScope) -> bool {
    match scope {
        JetJobScope::Dev => !cfg!(jet_release),
        JetJobScope::Ship | JetJobScope::Internal => true,
    }
}

fn jet_args_program_name(prog: &str) -> String {
    if prog.is_empty() {
        return "program".to_string();
    }
    std::path::Path::new(prog)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(prog)
        .to_string()
}

/// Normalize a source-file argv[0] to the name used by its built program.
pub(crate) fn jet_args_source_program_name(prog: &str) -> String {
    let (program, suffix) = prog.split_once(' ').unwrap_or((prog, ""));
    let name = jet_args_program_name(program);
    let source = name.strip_suffix(".jet").unwrap_or(&name);
    if suffix.is_empty() {
        source.to_string()
    } else {
        format!("{source} {suffix}")
    }
}

/// The one terminator for a banner a generated CLI writes to a stream: help,
/// usage and command errors. `JetArgsSpec::help()` renders the block, this
/// closes it with the single trailing newline `println!` would add. AOT emits
/// `print!`/`eprint!` around it, the Cranelift host and the interpreter push
/// the result into their own stdout/stderr buffers — no engine re-decides how
/// a banner ends.
#[allow(dead_code)]
pub(crate) fn jet_cli_banner(text: &str) -> String {
    format!("{text}\n")
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JetJobSelection {
    Ordinary,
    Help,
    Job(usize),
    Unknown,
}

fn jet_job_scope_visible(scope: JetJobScope) -> bool {
    match scope {
        JetJobScope::Dev => !cfg!(jet_release),
        JetJobScope::Ship => true,
        // Internal jobs remain callable by code and schedulers, never argv.
        JetJobScope::Internal => false,
    }
}

fn jet_job_scope_name(scope: JetJobScope) -> &'static str {
    match scope {
        JetJobScope::Dev => "dev",
        JetJobScope::Ship => "ship",
        JetJobScope::Internal => "internal",
    }
}

pub fn jet_job_has_visible(jobs: &[(&str, JetJobScope)]) -> bool {
    jobs.iter().any(|(_, scope)| jet_job_scope_visible(*scope))
}

pub fn jet_job_help_text(argv: &[String], jobs: &[(&str, JetJobScope)]) -> String {
    let program = jet_args_source_program_name(argv.first().map(String::as_str).unwrap_or(""));
    let mut text = format!("Usage: {program} <job> [options]\n\nJobs:");
    for (name, scope) in jobs {
        if jet_job_scope_visible(*scope) {
            text.push_str(&format!("\n  {:<20} {}", name, jet_job_scope_name(*scope)));
        }
    }
    text.push('\n');
    text
}

pub fn jet_job_help(argv: &[String], jobs: &[(&str, JetJobScope)]) {
    print!("{}", jet_job_help_text(argv, jobs));
}

/// I4: the one registered E1294 wording (the `E1294` row in
/// `Prelude/Diagnostics.jet`). The compiler tier renders it as a `Diagnostic`
/// and a built binary renders it to stderr through `jet_job_unknown` below —
/// one registered fact, two renderings, never two wordings.
#[allow(dead_code)]
pub const JET_JOB_UNKNOWN_WHY: &str =
    "The first program subcommand must name a function marked `#Job`.";
#[allow(dead_code)]
pub const JET_JOB_UNKNOWN_FIX: &str =
    "Mark a function `#Job`, or check the subcommand spelling.";

#[allow(dead_code)]
pub fn jet_job_unknown_what(name: &str) -> String {
    format!("No job named `{name}`")
}

/// The one label and separator a job refusal uses to advertise names. The
/// caller picks the list, because the honest list is tier-dependent: the
/// compiler tier names every declared job, while a built binary names only the
/// scopes it can still dispatch.
#[allow(dead_code)]
pub fn jet_job_declared_detail(names: &[&str]) -> String {
    format!("declared jobs: {}", names.join(", "))
}

fn jet_job_unknown(argv: &[String], jobs: &[(&str, JetJobScope)], name: &str) -> ! {
    let program = jet_args_source_program_name(argv.first().map(String::as_str).unwrap_or(""));
    let dispatchable = jobs
        .iter()
        .filter(|(_, scope)| jet_job_scope_visible(*scope))
        .map(|(name, _)| *name)
        .collect::<Vec<_>>();
    eprintln!("Error [E1294]: {}", jet_job_unknown_what(name));
    eprintln!(" Why: {JET_JOB_UNKNOWN_WHY}");
    eprintln!(" Fix: {JET_JOB_UNKNOWN_FIX}");
    eprintln!("More: jet-lang.dev/e/E1294");
    eprintln!("{}", jet_job_declared_detail(&dispatchable));
    eprintln!("\nUsage: {program} <job> [options]");
    std::process::exit(2)
}

/// Select the first argv word before any ordinary CLI parser sees it. This is
/// the shared decision kernel used by AOT and the JIT/interpreter adapters.
pub fn jet_job_select(argv: &[String], jobs: &[(&str, JetJobScope)]) -> JetJobSelection {
    let Some(name) = argv.get(1).map(String::as_str) else {
        return JetJobSelection::Ordinary;
    };
    if name == "--help" {
        return JetJobSelection::Help;
    }
    if name.starts_with('-') {
        return JetJobSelection::Ordinary;
    }
    for (index, (job_name, scope)) in jobs.iter().enumerate() {
        if *job_name == name && jet_job_scope_visible(*scope) {
            return JetJobSelection::Job(index);
        }
    }
    JetJobSelection::Unknown
}

/// Dispatch a legacy entry table through the canonical typed runtime.
pub fn jet_job_dispatch(argv: &[String], jobs: &[JetJobEntry]) -> bool {
    let specs = jet_job_legacy_specs(jobs);
    jet_job_dispatch_specs(argv, &specs, jet_job_no_scope)
}
