//! Streaming compiler trace.
//!
//! `JET_TRACE_FILE=<path>` makes the compiler write Chrome trace-event JSON
//! to `<path>` while it runs: one event per line, flushed per event, so a
//! running or killed compile is still a loadable trace (Perfetto and
//! chrome://tracing accept the JSON array without its closing `]`).
//! `JET_TRACE_DETAIL=functions` also asks for one span per function.
//! A progress kind `counters.<phase>.<counter>` is a #4319 scaling counter
//! (written once, unthrottled); with `JET_PHASE_COUNTERS` also set, each is
//! echoed to stderr as a `JET_PHASE_COUNTERS` line for `Tools/perf/scaling`.
//!
//! This is the one trace writer. The Rust driver opens spans with [`span`];
//! the bootstrap host hands the same sink to the Jet compiler as its
//! `CompilerTraceSink`. Unset, every span site costs one check of the
//! process-wide sink: nothing is formatted or allocated.
//!
//! Spans nest per thread. A span's `kind` is its stable event category
//! (`load`, `sema.check`, ...); `detail` labels one item (a module or a
//! function) and becomes the label the viewer shows. `end(kind)` closes the
//! innermost open span of that kind together with any deeper span an early
//! return left open, and records the items processed and the process's
//! resident and peak memory (`VmRSS`, `VmHWM`).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::File;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Path of the trace file; unset or empty traces nothing.
pub const TRACE_FILE_ENV: &str = "JET_TRACE_FILE";
/// `functions` adds per-function spans.
pub const TRACE_DETAIL_ENV: &str = "JET_TRACE_DETAIL";
/// Set (non-empty): counter events are also `JET_PHASE_COUNTERS` stderr lines.
pub const PHASE_COUNTERS_ENV: &str = "JET_PHASE_COUNTERS";
/// Progress kinds with this prefix are counters (`counters.<phase>.<name>`).
const COUNTER_PREFIX: &str = "counters.";

/// Minimum spacing of progress events of one kind; the last item is
/// always written.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

static SINK: OnceLock<Option<TraceSink>> = OnceLock::new();
static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static THREAD: u64 = NEXT_THREAD.fetch_add(1, Ordering::Relaxed);
}

/// The process's trace sink, opened from the environment on first use.
pub fn sink() -> Option<&'static TraceSink> {
    SINK.get_or_init(TraceSink::from_env).as_ref()
}

/// Open a phase span of `kind` on this thread; it ends when dropped.
pub fn span(kind: &'static str) -> Span {
    let sink = sink();
    if let Some(sink) = sink {
        sink.begin(kind, "", -1, -1);
    }
    Span { sink, kind, items: -1 }
}

/// A phase span that ends when dropped.
#[must_use = "a trace span ends when it is dropped"]
pub struct Span {
    sink: Option<&'static TraceSink>,
    kind: &'static str,
    items: i64,
}

impl Span {
    /// Record how many items the span processed.
    pub fn items(&mut self, items: usize) {
        self.items = i64::try_from(items).unwrap_or(i64::MAX);
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(sink) = self.sink {
            sink.end(self.kind, self.items);
        }
    }
}

pub struct TraceSink {
    start: Instant,
    pid: u32,
    functions: bool,
    phase_counters: bool,
    state: Mutex<TraceState>,
}

struct TraceState {
    /// `None` after a failed write: the trace stops, the compile does not.
    file: Option<File>,
    /// Open spans per thread, innermost last.
    stacks: HashMap<u64, Vec<OpenSpan>>,
    /// When each progress kind was last written.
    progress_at: HashMap<String, Instant>,
    line: String,
}

struct OpenSpan {
    kind: String,
    label: String,
}

impl TraceSink {
    fn from_env() -> Option<Self> {
        let path = std::env::var_os(TRACE_FILE_ENV).filter(|path| !path.is_empty())?;
        let functions = std::env::var(TRACE_DETAIL_ENV).is_ok_and(|detail| detail == "functions");
        match Self::create(std::path::Path::new(&path), functions) {
            Ok(mut sink) => {
                sink.phase_counters = std::env::var_os(PHASE_COUNTERS_ENV).is_some_and(|value| !value.is_empty());
                Some(sink)
            }
            Err(error) => {
                eprintln!(
                    "jet: {TRACE_FILE_ENV}: cannot write `{}`: {error}",
                    std::path::Path::new(&path).display()
                );
                None
            }
        }
    }

    /// Truncate `path` and write the trace header.
    fn create(path: &std::path::Path, functions: bool) -> std::io::Result<Self> {
        let mut file = File::create(path)?;
        let pid = std::process::id();
        let process = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.file_name().map(|name| name.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "jet".to_string());
        let unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let mut header = String::from("[\n");
        header.push_str("{\"name\":\"process_name\",\"ph\":\"M\",\"pid\":");
        let _ = write!(header, "{pid},\"tid\":0,\"args\":{{\"name\":");
        push_json_string(&mut header, &process);
        let _ = write!(
            header,
            "}}}},\n{{\"name\":\"trace.start\",\"ph\":\"i\",\"s\":\"g\",\"ts\":0,\"pid\":{pid},\"tid\":0,\"args\":{{\"unix_ms\":{unix_ms},\"functions\":{functions}}}}},\n"
        );
        file.write_all(header.as_bytes())?;
        file.flush()?;
        Ok(Self {
            start: Instant::now(),
            pid,
            functions,
            phase_counters: false,
            state: Mutex::new(TraceState {
                file: Some(file),
                stacks: HashMap::new(),
                progress_at: HashMap::new(),
                line: String::new(),
            }),
        })
    }

    /// Whether per-function spans were requested.
    pub fn functions(&self) -> bool {
        self.functions
    }

    /// Open a span of `kind`; `detail` labels one item ("" for a phase),
    /// `index`/`total` place it in its phase (negative when not counted).
    pub fn begin(&self, kind: &str, detail: &str, index: i64, total: i64) {
        let ts = self.micros();
        let thread = current_thread();
        let mut guard = self.lock();
        let state = &mut *guard;
        if state.file.is_none() {
            return;
        }
        let label = if detail.is_empty() { kind } else { detail };
        state.line.clear();
        if !state.stacks.contains_key(&thread) {
            push_thread_name(&mut state.line, self.pid, thread);
        }
        push_event_head(&mut state.line, label, Some(kind), 'B', ts, self.pid, thread);
        if index >= 0 {
            let _ = write!(state.line, ",\"args\":{{\"index\":{index},\"total\":{total}}}");
        }
        state.line.push_str("},\n");
        state.stacks.entry(thread).or_default().push(OpenSpan {
            kind: kind.to_string(),
            label: label.to_string(),
        });
        state.write_line();
    }

    /// Close the innermost open span of `kind` on this thread, and any
    /// deeper span left open; `items` < 0 reports no count.
    pub fn end(&self, kind: &str, items: i64) {
        let memory = memory_kb();
        let ts = self.micros();
        let thread = current_thread();
        let mut guard = self.lock();
        let state = &mut *guard;
        if state.file.is_none() {
            return;
        }
        let Some(stack) = state.stacks.get_mut(&thread) else { return };
        let Some(position) = stack.iter().rposition(|span| span.kind == kind) else { return };
        state.line.clear();
        while stack.len() > position + 1 {
            let abandoned = stack.pop().expect("open span above the closed one");
            push_event_head(&mut state.line, &abandoned.label, Some(&abandoned.kind), 'E', ts, self.pid, thread);
            state.line.push_str("},\n");
        }
        let closed = stack.pop().expect("closed span");
        push_event_head(&mut state.line, &closed.label, Some(&closed.kind), 'E', ts, self.pid, thread);
        state.line.push_str(",\"args\":{");
        let mut separator = "";
        if items >= 0 {
            let _ = write!(state.line, "\"items\":{items}");
            separator = ",";
        }
        if let Some((rss, peak)) = memory {
            let _ = write!(state.line, "{separator}\"rss_kb\":{rss},\"peak_rss_kb\":{peak}");
        }
        state.line.push_str("}},\n");
        // Phases (no item label) also feed one memory counter track.
        if closed.label == closed.kind {
            if let Some((rss, peak)) = memory {
                push_event_head(&mut state.line, "memory", None, 'C', ts, self.pid, thread);
                let _ = write!(
                    state.line,
                    ",\"args\":{{\"rss_mb\":{},\"peak_rss_mb\":{}}}}},\n",
                    rss / 1024,
                    peak / 1024
                );
            }
        }
        state.write_line();
    }

    /// `done` of `total` items of the open `kind` phase are finished.
    /// Written at most every 200 ms per kind, and always for the last item.
    pub fn progress(&self, kind: &str, done: i64, total: i64) {
        if let Some(counter) = kind.strip_prefix(COUNTER_PREFIX) {
            self.counter(counter, done);
            return;
        }
        let now = Instant::now();
        let thread = current_thread();
        let mut guard = self.lock();
        let state = &mut *guard;
        if state.file.is_none() {
            return;
        }
        let last_item = total > 0 && done + 1 >= total;
        if let Some(previous) = state.progress_at.get_mut(kind) {
            if !last_item && now.duration_since(*previous) < PROGRESS_INTERVAL {
                return;
            }
            *previous = now;
        } else {
            state.progress_at.insert(kind.to_string(), now);
        }
        let ts = u64::try_from(now.saturating_duration_since(self.start).as_micros()).unwrap_or(u64::MAX);
        state.line.clear();
        push_event_head(&mut state.line, kind, None, 'C', ts, self.pid, thread);
        let _ = write!(state.line, ",\"args\":{{\"done\":{done},\"total\":{total}}}}},\n");
        state.write_line();
    }

    fn micros(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    /// One `<phase>.<name>` counter value: a counter event in the trace and,
    /// with `JET_PHASE_COUNTERS` set, a `JET_PHASE_COUNTERS` stderr line.
    fn counter(&self, counter: &str, value: i64) {
        let Some((phase, name)) = counter.rsplit_once('.') else { return };
        let ts = self.micros();
        let thread = current_thread();
        {
            let mut guard = self.lock();
            let state = &mut *guard;
            if state.file.is_some() {
                state.line.clear();
                push_event_head(&mut state.line, counter, None, 'C', ts, self.pid, thread);
                let _ = write!(state.line, ",\"args\":{{\"value\":{value}}}}},\n");
                state.write_line();
            }
        }
        if self.phase_counters {
            let mut line = String::from("JET_PHASE_COUNTERS {\"phase\":");
            push_json_string(&mut line, phase);
            line.push(',');
            push_json_string(&mut line, name);
            let _ = write!(line, ":{value}}}");
            eprintln!("{line}");
        }
    }

    fn lock(&self) -> MutexGuard<'_, TraceState> {
        // A panic while tracing leaves whole lines behind; keep writing.
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl TraceState {
    fn write_line(&mut self) {
        let Some(file) = self.file.as_mut() else { return };
        if let Err(error) = file.write_all(self.line.as_bytes()).and_then(|()| file.flush()) {
            eprintln!("jet: {TRACE_FILE_ENV}: write failed, trace stopped: {error}");
            self.file = None;
        }
    }
}

fn current_thread() -> u64 {
    THREAD.with(|thread| *thread)
}

fn push_thread_name(line: &mut String, pid: u32, thread: u64) {
    let current = std::thread::current();
    let name = current.name().map_or_else(|| format!("thread {thread}"), str::to_string);
    let _ = write!(line, "{{\"name\":\"thread_name\",\"ph\":\"M\",\"pid\":{pid},\"tid\":{thread},\"args\":{{\"name\":");
    push_json_string(line, &name);
    line.push_str("}},\n");
}

/// `{"name":..,"cat":..,"ph":..,"ts":..,"pid":..,"tid":..` without the
/// closing brace, so the caller can append `args`.
fn push_event_head(line: &mut String, name: &str, kind: Option<&str>, phase: char, ts: u64, pid: u32, thread: u64) {
    line.push_str("{\"name\":");
    push_json_string(line, name);
    if let Some(kind) = kind {
        line.push_str(",\"cat\":");
        push_json_string(line, kind);
    }
    let _ = write!(line, ",\"ph\":\"{phase}\",\"ts\":{ts},\"pid\":{pid},\"tid\":{thread}");
}

fn push_json_string(line: &mut String, text: &str) {
    line.push('"');
    for character in text.chars() {
        match character {
            '"' => line.push_str("\\\""),
            '\\' => line.push_str("\\\\"),
            '\n' => line.push_str("\\n"),
            '\r' => line.push_str("\\r"),
            '\t' => line.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                let _ = write!(line, "\\u{:04x}", u32::from(control));
            }
            other => line.push(other),
        }
    }
    line.push('"');
}

/// Resident and peak resident set size in kB (`VmRSS`, `VmHWM`), where
/// the platform reports them.
fn memory_kb() -> Option<(u64, u64)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.trim().strip_suffix("kB"))
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    Some((field("VmRSS:")?, field("VmHWM:")?))
}

#[cfg(test)]
mod tests {
    use super::TraceSink;

    #[test]
    fn end_closes_abandoned_spans_and_every_line_is_one_event() {
        let path = std::env::temp_dir().join(format!("jet-trace-test-{}.json", std::process::id()));
        let sink = TraceSink::create(&path, false).expect("trace file");
        sink.begin("compile", "", -1, -1);
        sink.begin("sema.module", "a \"quoted\" module", 0, 2);
        sink.progress("sema.bodies", 0, 10);
        sink.progress("sema.bodies", 1, 10);
        sink.progress("sema.bodies", 9, 10);
        sink.end("missing", 1);
        sink.end("compile", 3);
        let text = std::fs::read_to_string(&path).expect("trace text");
        let _ = std::fs::remove_file(&path);
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], "[");
        assert!(lines[1..].iter().all(|line| line.starts_with('{') && line.ends_with("},")));
        assert!(text.contains("\"name\":\"a \\\"quoted\\\" module\",\"cat\":\"sema.module\",\"ph\":\"B\""));
        // The first and the last progress events are written; the middle one is throttled.
        assert_eq!(text.matches("\"name\":\"sema.bodies\",\"ph\":\"C\"").count(), 2);
        let ends = lines.iter().filter(|line| line.contains("\"ph\":\"E\"")).collect::<Vec<_>>();
        assert_eq!(ends.len(), 2);
        assert!(ends[0].contains("\"cat\":\"sema.module\""));
        assert!(ends[1].contains("\"cat\":\"compile\"") && ends[1].contains("\"items\":3"));
    }

    #[test]
    fn counter_kinds_are_unthrottled_counter_events() {
        let path = std::env::temp_dir().join(format!("jet-trace-counter-test-{}.json", std::process::id()));
        let sink = TraceSink::create(&path, false).expect("trace file");
        sink.progress("counters.sema.check.module_lookups", 7, -1);
        sink.progress("counters.sema.check.name_lookups", 3, -1);
        let text = std::fs::read_to_string(&path).expect("trace text");
        let _ = std::fs::remove_file(&path);
        assert!(text.contains("\"name\":\"sema.check.module_lookups\",\"ph\":\"C\"") && text.contains("\"args\":{\"value\":7}"));
        assert!(text.contains("\"name\":\"sema.check.name_lookups\",\"ph\":\"C\"") && text.contains("\"args\":{\"value\":3}"));
    }
}
