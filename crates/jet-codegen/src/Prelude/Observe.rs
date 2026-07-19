// D-OBSERVE-LIVE1=A: one live, payload-free runtime snapshot source. Generated
// programs publish bounded runtime facts only when JET_OBSERVE=1. Channel
// values, task locals, environment, and credentials never enter this registry.

#[derive(Clone)]
struct JetObserveTask {
    parent: usize,
    state: &'static str,
    wait: String,
    deadline_ms: Option<i64>,
    cancelled: bool,
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
    static JET_OBSERVE_TASK_ID: std::cell::Cell<usize> = const { std::cell::Cell::new(1) };
}

fn jet_observe_registry() -> Option<&'static std::sync::Arc<JetObserveRegistry>> {
    JET_OBSERVE
        .get_or_init(|| {
            (std::env::var("JET_OBSERVE").ok().as_deref() == Some("1")).then(|| {
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
        "{{\"id\":{id},\"parent\":{},\"state\":\"{}\",\"wait\":\"{}\",\"deadline_ms\":{},\"cancelled\":{}}}",
        task.parent, task.state, jet_observe_escape(&task.wait),
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

fn jet_observe_runtime_start() {
    let Some(registry) = jet_observe_registry().cloned() else { return };
    registry.tasks.lock().unwrap().insert(1, JetObserveTask {
        parent: 0, state: "running", wait: String::new(), deadline_ms: None, cancelled: false,
    });
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
