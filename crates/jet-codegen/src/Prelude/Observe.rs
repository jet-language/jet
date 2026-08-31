// D-OBSERVE-LIVE1=A / D-OBSERVE-TASK1=A: one live, payload-free runtime
// snapshot source. The bounded task registry is always present so the shared
// exit boundary can name parked tasks; JET_OBSERVE=1 only enables the live file
// writer. Channel values, task locals, environment, and credentials never enter
// this registry.

/// Optional scheduler drain installed by the Core scheduler. Keeping the hook
/// in this fixed Prelude part lets the reusable runtime compile without naming
/// the optional scheduler crate, while every tier still reaches one exit seam.
pub trait JetObserveControl: Send + Sync {
    fn cancel(&self);
}

static JET_OBSERVE_EXIT_DRAIN: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

pub fn jet_observe_register_exit_drain(drain: fn()) {
    let _ = JET_OBSERVE_EXIT_DRAIN.set(drain);
}

fn jet_observe_drain_after_exit() {
    if let Some(drain) = JET_OBSERVE_EXIT_DRAIN.get() {
        drain();
    }
}
#[derive(Clone)]
struct JetObserveTask {
    parent: usize,
    label: String,
    spawn_site: usize,
    state: &'static str,
    wait: String,
    deadline_ms: Option<i64>,
    cancelled: bool,
    control: Option<std::sync::Weak<dyn JetObserveControl>>,
}

#[derive(Clone)]
struct JetObserveChannel {
    depth: usize,
    capacity: Option<usize>,
    send_waiters: usize,
    recv_waiters: usize,
    closed: bool,
}

#[derive(Clone)]
struct JetObserveEvent {
    sequence: u64,
    source: &'static str,
    event_id: u64,
    owner_id: u64,
    subscription_id: u64,
    dispatch_id: u64,
    lifecycle: &'static str,
    queued: i64,
    blocked: i64,
    running: i64,
    capacity: i64,
    overflow: &'static str,
    priority: i64,
    failure: &'static str,
    terminal: &'static str,
}

const JET_OBSERVE_EVENT_LIMIT: usize = 256;
const JET_OBSERVE_TASK_LIMIT: usize = 4096;
// Keep the process-exit diagnostic bounded even when every registered task is
// parked. The registry limit bounds cardinality; this limit bounds rendered
// bytes so a pathological task forest cannot exhaust the exit path.
const JET_OBSERVE_EXIT_REPORT_BYTES: usize = 64 * 1024;
const JET_OBSERVE_EXIT_REPORT_MARKER_BYTES: usize = 128;

struct JetObserveRegistry {
    next_task: std::sync::atomic::AtomicUsize,
    next_channel: std::sync::atomic::AtomicUsize,
    next_event_sequence: std::sync::atomic::AtomicU64,
    tasks: std::sync::Mutex<std::collections::HashMap<usize, JetObserveTask>>,
    channels: std::sync::Mutex<std::collections::HashMap<usize, JetObserveChannel>>,
    events: std::sync::Mutex<std::collections::VecDeque<JetObserveEvent>>,
}

static JET_OBSERVE: std::sync::OnceLock<Option<std::sync::Arc<JetObserveRegistry>>> =
    std::sync::OnceLock::new();
static JET_OBSERVE_STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
static JET_OBSERVE_EXIT_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static JET_OBSERVE_ARENAS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static JET_OBSERVE_ARENA_ALLOCS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static JET_OBSERVE_ARENA_BYTES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
// #648 runtime-only allocator memory measurements. These deliberately do not
// add snapshot schema/policy vocabulary; D-MEM-FACTS1 owns any public fact.
static JET_OBSERVE_ARENA_RETAINED_BYTES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static JET_OBSERVE_ARENA_HIGH_WATER_BYTES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static JET_OBSERVE_WORKERS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static JET_OBSERVE_QUEUED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
// AllocationProbe uses a separate resettable window. Live-inspector counters
// remain outstanding-resource facts; probe reset/take cannot corrupt them.
static JET_PROBE_ARENA_ALLOCS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
static JET_PROBE_ARENA_BYTES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

thread_local! {
    pub static JET_OBSERVE_TASK_ID: std::cell::Cell<usize> = const { std::cell::Cell::new(1) };
}

fn jet_observe_registry() -> Option<&'static std::sync::Arc<JetObserveRegistry>> {
    JET_OBSERVE
        .get_or_init(|| {
            Some({
                std::sync::Arc::new(JetObserveRegistry {
                    next_task: std::sync::atomic::AtomicUsize::new(2),
                    next_channel: std::sync::atomic::AtomicUsize::new(1),
                    next_event_sequence: std::sync::atomic::AtomicU64::new(1),
                    tasks: std::sync::Mutex::new(std::collections::HashMap::new()),
                    channels: std::sync::Mutex::new(std::collections::HashMap::new()),
                    events: std::sync::Mutex::new(std::collections::VecDeque::new()),
                })
            })
        })
        .as_ref()
}

fn jet_observe_live_enabled() -> bool {
    std::env::var("JET_OBSERVE").ok().as_deref() == Some("1")
}

fn jet_observe_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars().take(160) {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' | '\t' => out.push(' '),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn jet_observe_process_start_id() -> String {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Ok(stat) = std::fs::read_to_string("/proc/self/stat") {
        if let Some(value) = stat
            .rsplit_once(") ")
            .and_then(|(_, tail)| tail.split_whitespace().nth(19))
        {
            return value.to_string();
        }
    }
    String::new()
}

fn jet_observe_snapshot(registry: &JetObserveRegistry, start_id: &str) -> String {
    use std::sync::atomic::Ordering;
    let mut tasks: Vec<_> = registry.tasks.lock().unwrap().iter()
        .map(|(id, task)| (*id, task.clone())).collect();
    tasks.sort_by_key(|(id, _)| *id);
    let mut channels: Vec<_> = registry.channels.lock().unwrap().iter()
        .map(|(id, channel)| (*id, channel.clone())).collect();
    channels.sort_by_key(|(id, _)| *id);
    let events = registry.events.lock().unwrap().iter().cloned().collect::<Vec<_>>();
    tasks.truncate(4096);
    channels.truncate(4096);

    let task_json = tasks.iter().map(|(id, task)| format!(
        "{{\"id\":{id},\"parent\":{},\"label\":\"{}\",\"spawn_site\":{},\"state\":\"{}\",\"wait\":\"{}\",\"deadline_ms\":{},\"cancelled\":{}}}",
        task.parent, jet_observe_escape(&task.label), task.spawn_site, task.state, jet_observe_escape(&task.wait),
        task.deadline_ms.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string()),
        task.cancelled
    )).collect::<Vec<_>>().join(",");
    let channel_json = channels.iter().map(|(id, channel)| format!(
        "{{\"id\":{id},\"depth\":{},\"capacity\":{},\"send_waiters\":{},\"recv_waiters\":{},\"closed\":{}}}",
        channel.depth,
        channel.capacity.map(|v| v.to_string()).unwrap_or_else(|| "null".to_string()),
        channel.send_waiters, channel.recv_waiters, channel.closed
    )).collect::<Vec<_>>().join(",");
    let event_json = events.iter().map(|event| format!(
        "{{\"sequence\":{},\"source\":\"{}\",\"event_id\":{},\"owner_id\":{},\"subscription_id\":{},\"dispatch_id\":{},\"lifecycle\":\"{}\",\"queued\":{},\"blocked\":{},\"running\":{},\"capacity\":{},\"overflow\":\"{}\",\"priority\":{},\"failure\":\"{}\",\"terminal\":\"{}\"}}",
        event.sequence, event.source, event.event_id, event.owner_id,
        event.subscription_id, event.dispatch_id, event.lifecycle, event.queued,
        event.blocked, event.running, event.capacity, event.overflow,
        event.priority, event.failure, event.terminal
    )).collect::<Vec<_>>().join(",");
    let blocked = tasks.iter().filter(|(_, task)| task.state == "blocked").count();
    let channel_waits = tasks.iter().filter(|(_, task)|
        task.state == "blocked" && task.wait.starts_with("channel ")).count();
    let time_waits = tasks.iter().filter(|(_, task)|
        task.state == "blocked" && task.wait.starts_with("time ")).count();
    let io_waits = tasks.iter().filter(|(_, task)| task.state == "blocked" &&
        (task.wait.contains("tcp") || task.wait.contains("network") || task.wait.starts_with("io "))
    ).count();
    let cancelled = tasks.iter().filter(|(_, task)| task.cancelled).count();
    let running = tasks.iter().filter(|(_, task)| task.state == "running").count();

    format!(
        "{{\"schema_version\":1,\"pid\":{},\"start_id\":\"{}\",\"captured_ms\":{},\"tasks\":[{}],\"channels\":[{}],\"event_observations\":[{}],\"effects\":{{\"compute\":{},\"waiting\":{},\"channel\":{},\"time\":{},\"io\":{}}},\"resources\":{{\"workers\":{},\"running\":{},\"queued\":{},\"cancelled\":{},\"arenas\":{},\"arena_allocations\":{},\"arena_bytes\":{}}}}}",
        std::process::id(),
        start_id,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_millis(),
        task_json, channel_json, event_json,
        tasks.len().saturating_sub(blocked), blocked, channel_waits, time_waits, io_waits,
        JET_OBSERVE_WORKERS.load(Ordering::Relaxed),
        running, JET_OBSERVE_QUEUED.load(Ordering::Relaxed), cancelled,
        JET_OBSERVE_ARENAS.load(Ordering::Relaxed),
        JET_OBSERVE_ARENA_ALLOCS.load(Ordering::Relaxed),
        JET_OBSERVE_ARENA_BYTES.load(Ordering::Relaxed)
    )
}

fn jet_observe_event(mut event: JetObserveEvent) {
    use std::sync::atomic::Ordering;
    let Some(registry) = jet_observe_registry() else { return };
    let mut events = registry.events.lock().unwrap();
    event.sequence = registry.next_event_sequence.fetch_add(1, Ordering::Relaxed);
    if events.len() == JET_OBSERVE_EVENT_LIMIT {
        events.pop_front();
    }
    events.push_back(event);
}

pub fn jet_observe_task_register(observe_id: &std::sync::atomic::AtomicUsize) -> usize {
    jet_observe_task_register_at(observe_id, 0)
}

/// Register one bounded task identity. The compiler-assigned spawn-site index
/// supplies a stable fallback label; a caller with a source name may replace
/// it with `jet_observe_task_set_label` before the child starts.
pub fn jet_observe_task_register_at(
    observe_id: &std::sync::atomic::AtomicUsize,
    spawn_site: usize,
) -> usize {
    jet_observe_task_register_at_with_control_weak(observe_id, spawn_site, None)
}

/// Register one task and retain only a weak link to its cancellation control.
/// The exit edge uses this link to quiesce detached work before an engine tears
/// down its runtime; the registry never owns a task control or its payload.
pub fn jet_observe_task_register_at_with_control<C>(
    observe_id: &std::sync::atomic::AtomicUsize,
    spawn_site: usize,
    control: Option<&std::sync::Arc<C>>,
) -> usize
where
    C: JetObserveControl + 'static,
{
    let control = control.map(|control| {
        let control: std::sync::Arc<dyn JetObserveControl> = control.clone();
        std::sync::Arc::downgrade(&control)
    });
    jet_observe_task_register_at_with_control_weak(observe_id, spawn_site, control)
}

fn jet_observe_task_register_at_with_control_weak(
    observe_id: &std::sync::atomic::AtomicUsize,
    spawn_site: usize,
    control: Option<std::sync::Weak<dyn JetObserveControl>>,
) -> usize {
    use std::sync::atomic::Ordering;
    let Some(registry) = jet_observe_registry() else {
        observe_id.store(0, Ordering::Relaxed);
        return 0;
    };
    let parent = JET_OBSERVE_TASK_ID.with(|current| current.get());
    let id = registry.next_task.fetch_add(1, Ordering::Relaxed);
    let mut tasks = registry.tasks.lock().unwrap();
    if tasks.len() >= JET_OBSERVE_TASK_LIMIT {
        observe_id.store(0, Ordering::Relaxed);
        return 0;
    }
    tasks.insert(
        id,
        JetObserveTask {
            parent,
            label: format!("task@{spawn_site}"),
            spawn_site,
            state: "queued",
            wait: String::new(),
            deadline_ms: None,
            cancelled: false,
            control,
        },
    );
    observe_id.store(id, Ordering::Relaxed);
    id
}

/// Attach the optional source label to a registered task. The registry stores
/// only a bounded string, and rendering escapes it before it crosses the
/// diagnostic or JSON boundary.
pub fn jet_observe_task_set_label(id: usize, label: &str) {
    if id == 0 || label.trim().is_empty() {
        return;
    }
    let Some(registry) = jet_observe_registry() else {
        return;
    };
    if let Some(task) = registry.tasks.lock().unwrap().get_mut(&id) {
        task.label = label.chars().take(160).collect();
    }
}

/// Stable text for a task failure. A source label, when present, is paired
/// with the compiler spawn site so repeated tasks remain distinguishable.
pub fn jet_observe_task_identity(id: usize) -> String {
    let Some(registry) = jet_observe_registry() else {
        return format!("task #{id}");
    };
    let Some(task) = registry.tasks.lock().unwrap().get(&id).cloned() else {
        return format!("task #{id}");
    };
    format!(
        "{} (spawn site {})",
        jet_observe_escape(&task.label),
        task.spawn_site
    )
}

pub fn jet_observe_task_failure_message(id: usize, reason: String) -> String {
    jet_observe_task_failure_message_for_identity(&jet_observe_task_identity(id), reason)
}

pub fn jet_observe_task_failure_message_for_identity(identity: &str, reason: String) -> String {
    format!("{identity}: {reason}")
}

pub fn jet_observe_task_enter(id: usize) {
    if id == 0 {
        return;
    }
    JET_OBSERVE_TASK_ID.with(|current| current.set(id));
    if let Some(registry) = jet_observe_registry() {
        if let Some(task) = registry.tasks.lock().unwrap().get_mut(&id) {
            task.state = "running";
        }
    }
}

pub fn jet_observe_task_finish(id: usize) {
    if id == 0 {
        return;
    }
    if let Some(registry) = jet_observe_registry() {
        registry.tasks.lock().unwrap().remove(&id);
    }
}

pub fn jet_observe_runtime_start() {
    let Some(registry) = jet_observe_registry().cloned() else { return };
    use std::sync::atomic::Ordering;
    JET_OBSERVE_EXIT_REPORTED.store(false, Ordering::Release);
    let first_start = JET_OBSERVE_STARTED.set(()).is_ok();
    if first_start {
        registry.tasks.lock().unwrap().insert(1, JetObserveTask {
            parent: 0,
            label: String::from("root"),
            spawn_site: 0,
            state: "running",
            wait: String::new(),
            deadline_ms: None,
            cancelled: false,
            // The root task is the program itself, not a spawned body, so no
            // cancellation control exists to link. `None` says that; a dangling
            // `Weak` would claim a control that was never there.
            control: None,
        });
    }
    if !first_start || !jet_observe_live_enabled() {
        return;
    }
    std::thread::spawn(move || {
        use std::io::Write;
        let pid = std::process::id();
        let start_id = jet_observe_process_start_id();
        let path = std::env::temp_dir().join(format!("jet-observe-{pid}.json"));
        let mut sequence = 0_u64;
        loop {
            sequence = sequence.wrapping_add(1);
            let snapshot = jet_observe_snapshot(&registry, &start_id);
            let staging = std::env::temp_dir().join(format!(
                ".jet-observe-{pid}-{start_id}-{sequence}.tmp"
            ));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            if snapshot.len() <= 1024 * 1024 {
                if let Ok(mut file) = options.open(&staging) {
                    if file.write_all(snapshot.as_bytes()).is_ok() && file.sync_all().is_ok() {
                        let _ = std::fs::rename(&staging, &path);
                    }
                }
            }
            let _ = std::fs::remove_file(&staging);
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

fn jet_observe_parked_tasks(
    registry: &JetObserveRegistry,
) -> Vec<(usize, JetObserveTask)> {
    let mut tasks: Vec<_> = registry
        .tasks
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, task)| task.state == "blocked")
        .map(|(id, task)| (*id, task.clone()))
        .collect();
    tasks.sort_by_key(|(id, _)| *id);
    tasks
}

fn jet_observe_cancel_live_tasks(registry: &JetObserveRegistry) {
    let controls: Vec<_> = registry
        .tasks
        .lock()
        .unwrap()
        .iter()
        .filter(|(id, _)| **id != 1)
        .filter_map(|(_, task)| task.control.as_ref().and_then(|control| control.upgrade()))
        .collect();
    for control in controls {
        control.cancel();
    }
}

pub fn jet_observe_has_parked_tasks() -> bool {
    jet_observe_registry().is_some_and(|registry| {
        registry
            .tasks
            .lock()
            .unwrap()
            .values()
            .any(|task| task.state == "blocked")
    })
}

/// Render the bounded parked-task snapshot at the common process exit edge.
/// The short observation window lets a just-submitted child publish its first
/// wait state without making normal programs pay a scheduler drain timeout.
pub fn jet_observe_parked_tasks_report() -> Option<JetRuntimeDiagnostic> {
    use std::sync::atomic::Ordering;
    let registry = jet_observe_registry()?.clone();
    if JET_OBSERVE_EXIT_REPORTED.swap(true, Ordering::AcqRel) {
        return None;
    }
    let mut parked = jet_observe_parked_tasks(&registry);
    if parked.is_empty() {
        for _ in 0..25 {
            let has_live_child = registry
                .tasks
                .lock()
                .unwrap()
                .keys()
                .any(|id| *id != 1);
            if !has_live_child {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            parked = jet_observe_parked_tasks(&registry);
            if !parked.is_empty() {
                break;
            }
        }
    }
    if parked.is_empty() {
        jet_observe_cancel_live_tasks(&registry);
        return None;
    }

    let mut details = String::new();
    let mut omitted = 0usize;
    for (_, task) in parked {
        let entry = format!(
            "\n  {} (spawn site {})\n    state: {}\n    wait target: {}",
            jet_observe_escape(&task.label),
            task.spawn_site,
            task.state,
            if task.wait.is_empty() {
                "unknown".to_string()
            } else {
                jet_observe_escape(&task.wait)
            },
        );
        if details.len().saturating_add(entry.len())
            > JET_OBSERVE_EXIT_REPORT_BYTES
                .saturating_sub(JET_OBSERVE_EXIT_REPORT_MARKER_BYTES)
        {
            omitted = omitted.saturating_add(1);
        } else {
            details.push_str(&entry);
        }
    }
    if omitted > 0 {
        let marker = format!("\n  ... {omitted} more parked task(s) omitted by the report limit ...");
        if details.len().saturating_add(marker.len()) <= JET_OBSERVE_EXIT_REPORT_BYTES {
            details.push_str(&marker);
        }
    }
    jet_observe_cancel_live_tasks(&registry);
    Some(jet_render_runtime_stop(
        "E3013", "", 0, "", "", 1, 1, &details, "",
    ))
}

fn jet_observe_task_update(state: &'static str, wait: &str, deadline_ms: Option<i64>) {
    let Some(registry) = jet_observe_registry() else { return };
    let id = JET_OBSERVE_TASK_ID.with(|current| current.get());
    if let Some(task) = registry.tasks.lock().unwrap().get_mut(&id) {
        task.state = state;
        task.wait = wait.chars().take(160).collect();
        task.deadline_ms = deadline_ms;
    }
}

fn jet_observe_arena_open() {
    JET_OBSERVE_ARENAS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}
fn jet_observe_arena_alloc(bytes: usize) {
    JET_OBSERVE_ARENA_ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let live = JET_OBSERVE_ARENA_BYTES
        .fetch_add(bytes, std::sync::atomic::Ordering::Relaxed)
        .saturating_add(bytes);
    JET_OBSERVE_ARENA_HIGH_WATER_BYTES.fetch_max(live, std::sync::atomic::Ordering::Relaxed);
    JET_PROBE_ARENA_ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    JET_PROBE_ARENA_BYTES.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
}
fn jet_observe_arena_retain(bytes: usize) {
    JET_OBSERVE_ARENA_RETAINED_BYTES.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
}
fn jet_observe_arena_release(bytes: usize) {
    JET_OBSERVE_ARENA_RETAINED_BYTES.fetch_sub(bytes, std::sync::atomic::Ordering::Relaxed);
}
fn jet_observe_allocator_memory() -> (usize, usize) {
    (
        JET_OBSERVE_ARENA_RETAINED_BYTES.load(std::sync::atomic::Ordering::Relaxed),
        JET_OBSERVE_ARENA_HIGH_WATER_BYTES.load(std::sync::atomic::Ordering::Relaxed),
    )
}
fn jet_observe_arena_reset(allocations: usize, bytes: usize) {
    JET_OBSERVE_ARENA_ALLOCS.fetch_sub(allocations, std::sync::atomic::Ordering::Relaxed);
    JET_OBSERVE_ARENA_BYTES.fetch_sub(bytes, std::sync::atomic::Ordering::Relaxed);
}
fn jet_observe_arena_close() {
    JET_OBSERVE_ARENAS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
}

fn jet_allocation_probe_reset() {
    JET_PROBE_ARENA_ALLOCS.store(0, std::sync::atomic::Ordering::SeqCst);
    JET_PROBE_ARENA_BYTES.store(0, std::sync::atomic::Ordering::SeqCst);
}

fn jet_allocation_probe_take() -> (usize, usize) {
    (
        JET_PROBE_ARENA_ALLOCS.swap(0, std::sync::atomic::Ordering::SeqCst),
        JET_PROBE_ARENA_BYTES.swap(0, std::sync::atomic::Ordering::SeqCst),
    )
}
