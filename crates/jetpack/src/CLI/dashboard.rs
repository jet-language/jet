//! Bare `jetpack` dashboard.
//!
//! The dashboard is deliberately a read-only view over the same project,
//! store, and doctor readers used by the CLI. It has no separate state model:
//! `r` starts another snapshot load, and non-TTY callers receive the compact
//! status summary rendered from a synchronous read-only snapshot.

use crate::{Doctor, EnvFile, Lock, Store, Syntax};
use jet_cli::Term::{self, Key, KeyReader, RawGuard};
use jet_env_model::ModuleEval;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_RECENT_ACTIVITY: usize = 5;
const STORE_LOAD_TIMEOUT: Duration = Duration::from_millis(250);
const STORE_REFRESH_CADENCE: Duration = Duration::from_secs(1);

static DASHBOARD_SIGNAL: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
struct DashboardSnapshot {
    project: ProjectSummary,
    store: StoreSummary,
    doctor: Doctor::Report,
    activity: Vec<Activity>,
}

#[derive(Clone, Debug)]
struct ProjectSummary {
    root: PathBuf,
    env_file: PathBuf,
    env_present: bool,
    typed: bool,
    active_environment: Option<String>,
    declared_packages: Option<usize>,
    locked_packages: Option<usize>,
    lock: LockState,
    active: bool,
    active_refs: String,
    update_channels: usize,
}

#[derive(Clone, Copy, Debug)]
enum LockState {
    Missing,
    Ready,
    Invalid,
}

#[derive(Clone, Debug)]
struct StoreSummary {
    path: PathBuf,
    bytes: u64,
    packages: usize,
    health: Doctor::Health,
}

#[derive(Clone, Debug)]
struct Activity {
    name: String,
    version: String,
    when: String,
}

enum DashboardFrame {
    Loading,
    Busy,
    Ready(DashboardSnapshot),
}

enum LoadResult {
    Ready(DashboardSnapshot),
    Busy,
}

struct SnapshotLoader {
    result: Receiver<LoadResult>,
    started: Instant,
    handle: Option<JoinHandle<()>>,
}

impl SnapshotLoader {
    fn spawn(task: impl FnOnce() -> LoadResult + Send + 'static) -> Self {
        let (sender, result) = mpsc::channel();
        let handle = thread::spawn(move || {
            let _ = sender.send(task());
        });
        Self {
            result,
            started: Instant::now(),
            handle: Some(handle),
        }
    }

    fn try_result(&self) -> Option<LoadResult> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(LoadResult::Busy),
        }
    }

    fn timed_out(&self, now: Instant) -> bool {
        now.checked_duration_since(self.started)
            .is_some_and(|elapsed| elapsed >= STORE_LOAD_TIMEOUT)
    }

    #[cfg(test)]
    fn join(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SnapshotLoader {
    fn drop(&mut self) {
        // A blocking Store::list_checked call cannot be cancelled safely. The
        // UI never joins this worker; dropping its handle detaches it until
        // process exit.
        // ponytail: detached timed-out loader; add cancellation at the Store
        // lock seam when that API can interrupt an in-flight read.
        let _ = self.handle.take();
    }
}

/// Run the bare command. The TTY check is kept here, at the dispatch seam, so
/// the non-interactive path cannot emit an alternate-screen control sequence.
pub(super) fn run_dashboard() -> i32 {
    let interactive = interactive_mode(io::stdin().is_terminal(), io::stdout().is_terminal());
    if !interactive {
        print_status_summary();
        return 0;
    }

    let Some(signals) = SignalGuard::install() else {
        eprintln!("jetpack: cannot install dashboard signal handlers");
        return 1;
    };
    let Some(raw) = RawGuard::enable() else {
        drop(signals);
        print_status_summary();
        return 0;
    };
    run_interactive(signals, raw)
}

fn interactive_mode(stdin_tty: bool, stdout_tty: bool) -> bool {
    stdin_tty && stdout_tty
}

// Keep `_raw` declared after `_signals`: Rust drops parameters in reverse
// declaration order, so raw mode is restored before signal dispositions.
fn run_interactive(_signals: SignalGuard, _raw: RawGuard) -> i32 {
    let _screen = AlternateScreen::enter();
    let stdin = io::stdin();
    let mut reader = KeyReader::new(stdin.lock());
    let mut loader = None;
    let mut frame = DashboardFrame::Loading;
    run_interactive_loop(
        &mut reader,
        &mut loader,
        &mut frame,
        |frame| redraw(&render_frame(frame)),
        start_snapshot_loader,
        Instant::now,
    )
}

fn key_quits(key: &Key) -> bool {
    matches!(
        key,
        Key::Char('q') | Key::Char('Q') | Key::Escape | Key::CtrlC | Key::Eof
    )
}

fn run_interactive_loop<R, Draw, Start, Clock>(
    reader: &mut KeyReader<R>,
    loader: &mut Option<SnapshotLoader>,
    frame: &mut DashboardFrame,
    mut draw: Draw,
    mut start_loader: Start,
    mut now: Clock,
) -> i32
where
    R: Read,
    Draw: FnMut(&DashboardFrame),
    Start: FnMut() -> SnapshotLoader,
    Clock: FnMut() -> Instant,
{
    // Reader exists and the loading frame is visible before the first Store
    // operation starts in the worker.
    draw(frame);
    let mut next_retry = now() + STORE_REFRESH_CADENCE;
    if loader.is_none() {
        *loader = Some(start_loader());
    }

    loop {
        if DASHBOARD_SIGNAL.load(Ordering::SeqCst) {
            break;
        }
        let key = reader.read_key();
        let current = now();
        if key_quits(&key) || DASHBOARD_SIGNAL.load(Ordering::SeqCst) {
            break;
        }

        if matches!(key, Key::Char('r') | Key::Char('R')) {
            let _ = loader.take();
            *frame = DashboardFrame::Loading;
            next_retry = current + STORE_REFRESH_CADENCE;
            draw(frame);
            *loader = Some(start_loader());
            continue;
        }

        if let Some(result) = loader.as_ref().and_then(SnapshotLoader::try_result) {
            let _ = loader.take();
            *frame = match result {
                LoadResult::Ready(snapshot) => DashboardFrame::Ready(snapshot),
                LoadResult::Busy => DashboardFrame::Busy,
            };
            next_retry = current + STORE_REFRESH_CADENCE;
            draw(frame);
            continue;
        }

        if let Some(active) = loader.as_ref() {
            if mark_load_timeout(frame, active, current) {
                next_retry = current + STORE_REFRESH_CADENCE;
                draw(frame);
            }
        }

        if matches!(frame, DashboardFrame::Busy) && current >= next_retry {
            let _ = loader.take();
            *frame = DashboardFrame::Loading;
            next_retry = current + STORE_REFRESH_CADENCE;
            draw(frame);
            *loader = Some(start_loader());
        }
    }
    0
}

fn start_snapshot_loader() -> SnapshotLoader {
    SnapshotLoader::spawn(load_snapshot)
}

fn load_snapshot() -> LoadResult {
    let roots = Store::resolve();
    let lock_path = roots.root.join(".locks").join("hangar.lock");
    // Probe the existing kernel lock without waiting. The checked listing is
    // still the single authoritative Store API, but it can only block here in
    // the detached loader, never in the terminal loop.
    if matches!(
        super::super::RuntimePolicy::lock_state(&lock_path),
        Ok(super::super::RuntimePolicy::LockState::Held)
    ) {
        return LoadResult::Busy;
    }
    let entries = Store::list_checked(&roots).unwrap_or_default();
    LoadResult::Ready(collect_snapshot_from(roots, entries))
}

fn mark_load_timeout(frame: &mut DashboardFrame, loader: &SnapshotLoader, now: Instant) -> bool {
    if matches!(frame, DashboardFrame::Loading) && loader.timed_out(now) {
        *frame = DashboardFrame::Busy;
        true
    } else {
        false
    }
}

fn render_frame(frame: &DashboardFrame) -> String {
    render_frame_width(frame, terminal_width())
}

fn render_frame_width(frame: &DashboardFrame, width: usize) -> String {
    match frame {
        DashboardFrame::Loading => render_loading_dashboard_width(width, "reading store…"),
        DashboardFrame::Busy => {
            render_loading_dashboard_width(width, "store busy (another jetpack is running)")
        }
        DashboardFrame::Ready(snapshot) => render_dashboard_width(snapshot, width),
    }
}

fn render_loading_dashboard_width(width: usize, store_message: &str) -> String {
    let mut out = String::new();
    push_line(&mut out, width, 0, "Jetpack dashboard");
    push_line(&mut out, width, 0, "Project  reading project…");
    out.push('\n');
    push_line(&mut out, width, 0, "Store");
    push_line(&mut out, width, 2, store_message);
    out.push('\n');
    push_line(&mut out, width, 0, "r refresh · q quit");
    out
}

#[cfg(unix)]
const SIGNAL_ERROR: usize = usize::MAX;
#[cfg(unix)]
const DASHBOARD_SIGINT: i32 = 2;
#[cfg(unix)]
const DASHBOARD_SIGTERM: i32 = 15;

#[cfg(unix)]
unsafe extern "C" {
    fn signal(signal: i32, handler: usize) -> usize;
}

#[cfg(unix)]
extern "C" fn mark_dashboard_signal(_: i32) {
    DASHBOARD_SIGNAL.store(true, Ordering::SeqCst);
}

struct SignalGuard {
    #[cfg(unix)]
    previous_int: usize,
    #[cfg(unix)]
    previous_term: usize,
}

impl SignalGuard {
    fn install() -> Option<Self> {
        DASHBOARD_SIGNAL.store(false, Ordering::SeqCst);
        #[cfg(unix)]
        {
            let handler = mark_dashboard_signal as *const () as usize;
            let previous_int = unsafe { signal(DASHBOARD_SIGINT, handler) };
            if previous_int == SIGNAL_ERROR {
                return None;
            }
            let previous_term = unsafe { signal(DASHBOARD_SIGTERM, handler) };
            if previous_term == SIGNAL_ERROR {
                unsafe { signal(DASHBOARD_SIGINT, previous_int) };
                return None;
            }
            Some(Self {
                previous_int,
                previous_term,
            })
        }
        #[cfg(not(unix))]
        {
            Some(Self {})
        }
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            signal(DASHBOARD_SIGTERM, self.previous_term);
            signal(DASHBOARD_SIGINT, self.previous_int);
        }
        DASHBOARD_SIGNAL.store(false, Ordering::SeqCst);
    }
}

fn print_status_summary() {
    let snapshot = collect_snapshot();
    print!("{}", render_status_summary(&snapshot));
    let _ = io::stdout().flush();
}

fn collect_snapshot() -> DashboardSnapshot {
    let roots = Store::resolve();
    let entries = Store::list_read_only(&roots);
    collect_snapshot_from(roots, entries)
}

fn collect_snapshot_from(
    roots: Store::Roots,
    mut entries: Vec<Store::StoreEntry>,
) -> DashboardSnapshot {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_root = super::workspace_sources::project_root(&current_dir);
    let env_file = EnvFile::path_in(&project_root);
    let env_present = env_file.is_file();
    let env_source = fs::read_to_string(&env_file).ok();
    let typed = env_source
        .as_deref()
        .is_some_and(ModuleEval::is_module_surface);
    let typed_plan = if typed {
        env_source
            .as_deref()
            .and_then(|source| ModuleEval::evaluate_env(source, &project_root).ok())
    } else {
        None
    };
    let env = if typed {
        None
    } else {
        EnvFile::load(&project_root)
    };
    let lock_path = Store::lock_path(&project_root);
    let lock = Lock::load(&project_root);
    let lock_state = if !lock_path.is_file() {
        LockState::Missing
    } else if lock.is_some() {
        LockState::Ready
    } else {
        LockState::Invalid
    };
    let active = std::env::var(Syntax::JETPACK_ENV_MARKER)
        .ok()
        .is_some_and(|value| value == "1");
    let active_refs = std::env::var(Syntax::JETPACK_REF_VAR).unwrap_or_default();
    let update_channels = if let Some(plan) = typed_plan.as_ref() {
        plan.table
            .declared_names()
            .iter()
            .filter(|name| plan.table.channel_policy(name).moves())
            .count()
    } else {
        lock.as_ref()
            .map(|file| file.source_channels.len())
            .unwrap_or_else(|| env.as_ref().map_or(0, |file| file.named.len()))
    };

    let store_packages = entries.len();
    entries.sort_by(|left, right| {
        activity_time(right)
            .cmp(&activity_time(left))
            .then_with(|| left.id.cmp(&right.id))
    });
    let now = unix_now();
    let activity = entries
        .into_iter()
        .take(MAX_RECENT_ACTIVITY)
        .map(|entry| {
            let timestamp = activity_time(&entry);
            let name = if entry.name.is_empty() {
                entry.id.clone()
            } else {
                entry.name
            };
            Activity {
                name,
                version: entry.version,
                when: age_label(timestamp, now),
            }
        })
        .collect();

    let doctor = Doctor::run(&project_root, false);
    let store_health = doctor
        .checks
        .iter()
        .find(|check| check.name == "hangar")
        .map_or(Doctor::Health::Healthy, |check| check.health);
    DashboardSnapshot {
        project: ProjectSummary {
            root: project_root,
            env_file,
            env_present,
            typed,
            active_environment: typed_plan
                .as_ref()
                .and_then(|plan| plan.active_environment.clone()),
            declared_packages: if typed {
                typed_plan
                    .as_ref()
                    .map(|plan| plan.package_refs.len() + plan.adapters.len())
            } else {
                env.as_ref().map(|file| file.packages.len())
            },
            locked_packages: lock.as_ref().map(|file| file.packages.len()),
            lock: lock_state,
            active,
            active_refs,
            update_channels,
        },
        store: StoreSummary {
            path: roots.hangar_dir(),
            bytes: Store::dir_size(&roots.hangar_dir()),
            packages: store_packages,
            health: store_health,
        },
        doctor,
        activity,
    }
}

fn activity_time(entry: &Store::StoreEntry) -> u64 {
    entry.last_used_at.max(entry.realized_at)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn age_label(timestamp: u64, now: u64) -> String {
    if timestamp == 0 || timestamp > now {
        return "time unknown".to_string();
    }
    let seconds = now.saturating_sub(timestamp);
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        86_400..=2_591_999 => format!("{}d ago", seconds / 86_400),
        _ => format!("{}mo ago", seconds / 2_592_000),
    }
}

fn terminal_width() -> usize {
    if let Some(width) = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width > 0)
    {
        return width;
    }
    if interactive_mode(io::stdin().is_terminal(), io::stdout().is_terminal()) {
        Term::terminal_width()
    } else {
        80
    }
}

fn render_status_summary(snapshot: &DashboardSnapshot) -> String {
    let width = terminal_width();
    let mut out = String::new();
    push_line(&mut out, width, 0, "Jetpack status");
    push_line(
        &mut out,
        width,
        0,
        format!("Project: {}", path_text(&snapshot.project.root)),
    );
    push_line(
        &mut out,
        width,
        0,
        format!("Environment: {}", environment_state(&snapshot.project)),
    );
    push_line(
        &mut out,
        width,
        0,
        format!(
            "Store: {} package(s), {}, health {}",
            snapshot.store.packages,
            format_size(snapshot.store.bytes),
            health_word(snapshot.store.health),
        ),
    );
    push_line(
        &mut out,
        width,
        0,
        format!(
            "Updates: {} moving channel(s); run `jetpack outdated` to check latest",
            snapshot.project.update_channels
        ),
    );
    push_line(
        &mut out,
        width,
        0,
        format!(
            "Doctor: {} ({} checks)",
            health_word(snapshot.doctor.health()),
            snapshot.doctor.checks.len()
        ),
    );
    let recent = snapshot
        .activity
        .first()
        .map(activity_text)
        .unwrap_or_else(|| "none recorded".to_string());
    push_line(&mut out, width, 0, format!("Recent activity: {recent}"));
    out
}

fn render_dashboard_width(snapshot: &DashboardSnapshot, width: usize) -> String {
    let mut out = String::new();
    push_line(&mut out, width, 0, "Jetpack dashboard");
    push_line(
        &mut out,
        width,
        0,
        format!("Project  {}", path_text(&snapshot.project.root)),
    );
    out.push('\n');

    push_line(&mut out, width, 0, "Project environment");
    push_line(
        &mut out,
        width,
        2,
        format!("state      {}", environment_state(&snapshot.project)),
    );
    push_line(
        &mut out,
        width,
        2,
        format!("env file   {}", path_text(&snapshot.project.env_file)),
    );
    if let Some(packages) = snapshot.project.declared_packages {
        push_line(
            &mut out,
            width,
            2,
            format!("declared   {packages} package(s)"),
        );
    }
    if let Some(packages) = snapshot.project.locked_packages {
        push_line(
            &mut out,
            width,
            2,
            format!("locked     {packages} package(s)"),
        );
    }
    push_line(
        &mut out,
        width,
        2,
        format!(
            "lock       {}",
            lock_text(snapshot.project.lock, snapshot.project.locked_packages)
        ),
    );
    push_line(
        &mut out,
        width,
        2,
        format!("active     {}", active_text(&snapshot.project)),
    );
    out.push('\n');

    push_line(&mut out, width, 0, "Store");
    push_line(
        &mut out,
        width,
        2,
        format!("health     {}", health_word(snapshot.store.health)),
    );
    push_line(
        &mut out,
        width,
        2,
        format!(
            "size       {} in {}",
            format_size(snapshot.store.bytes),
            path_text(&snapshot.store.path)
        ),
    );
    push_line(
        &mut out,
        width,
        2,
        format!("packages   {}", snapshot.store.packages),
    );
    out.push('\n');

    push_line(&mut out, width, 0, "Available updates");
    push_line(
        &mut out,
        width,
        2,
        format!(
            "channels   {} moving channel(s) recorded",
            snapshot.project.update_channels
        ),
    );
    push_line(
        &mut out,
        width,
        2,
        "check      run `jetpack outdated` for latest availability",
    );
    out.push('\n');

    push_line(&mut out, width, 0, "Doctor findings");
    for check in &snapshot.doctor.checks {
        push_line(
            &mut out,
            width,
            2,
            format!(
                "[{}] {:<12} {}",
                health_mark(check.health),
                check.name,
                sanitize(&check.detail)
            ),
        );
        if !check.fix.is_empty() {
            push_line(&mut out, width, 4, format!("fix: {}", sanitize(&check.fix)));
        }
    }
    out.push('\n');

    push_line(&mut out, width, 0, "Recent activity");
    if snapshot.activity.is_empty() {
        push_line(&mut out, width, 2, "none recorded");
    } else {
        for activity in &snapshot.activity {
            push_line(&mut out, width, 2, activity_text(activity));
        }
    }
    out.push('\n');
    push_line(&mut out, width, 0, "r refresh · q quit");
    out
}

fn environment_state(project: &ProjectSummary) -> String {
    if !project.env_present {
        return "not found".to_string();
    }
    if project.typed {
        let module = project.active_environment.as_deref().map_or_else(
            || "typed module".to_string(),
            |name| format!("typed module env.{name}"),
        );
        return project.declared_packages.map_or_else(
            || format!("ready · {module}"),
            |count| format!("ready · {module} · {count} package(s)"),
        );
    }
    if project.declared_packages.is_none() {
        return "present · unreadable".to_string();
    }
    let packages = project
        .declared_packages
        .map_or_else(|| "unknown".to_string(), |count| format!("{count}"));
    format!("ready · {packages} declared package(s)")
}

fn lock_text(lock: LockState, packages: Option<usize>) -> String {
    match lock {
        LockState::Missing => "missing".to_string(),
        LockState::Ready => format!("ready · {} package(s)", packages.unwrap_or(0)),
        LockState::Invalid => "invalid; run `jetpack update`".to_string(),
    }
}

fn active_text(project: &ProjectSummary) -> String {
    if !project.active {
        return "not active in this shell".to_string();
    }
    if project.active_refs.is_empty() {
        "active in this shell".to_string()
    } else {
        format!(
            "active in this shell · refs {}",
            sanitize(&project.active_refs)
        )
    }
}

fn activity_text(activity: &Activity) -> String {
    if activity.version.is_empty() {
        format!("{} · {}", sanitize(&activity.name), activity.when)
    } else {
        format!(
            "{} {} · {}",
            sanitize(&activity.name),
            sanitize(&activity.version),
            activity.when
        )
    }
}

fn health_word(health: Doctor::Health) -> &'static str {
    match health {
        Doctor::Health::Healthy => "healthy",
        Doctor::Health::Degraded => "degraded",
        Doctor::Health::Broken => "broken",
    }
}

fn health_mark(health: Doctor::Health) -> &'static str {
    match health {
        Doctor::Health::Healthy => "OK",
        Doctor::Health::Degraded => "WARN",
        Doctor::Health::Broken => "FAIL",
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1_000.0 && unit + 1 < UNITS.len() {
        value /= 1_000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn path_text(path: &Path) -> String {
    sanitize(&path.display().to_string())
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                '?'
            } else {
                character
            }
        })
        .collect()
}

fn push_line(out: &mut String, width: usize, indent: usize, value: impl std::fmt::Display) {
    let prefix = " ".repeat(indent);
    let text = fit(&value.to_string(), width.saturating_sub(indent));
    let _ = writeln!(out, "{prefix}{text}");
}

fn fit(value: &str, width: usize) -> String {
    let value = sanitize(value);
    let length = value.chars().count();
    if length <= width {
        return value;
    }
    if width == 0 {
        return String::new();
    }
    let mut result: String = value.chars().take(width.saturating_sub(1)).collect();
    result.push('…');
    result
}

fn redraw(frame: &str) {
    let mut stdout = io::stdout().lock();
    let _ = write!(stdout, "\x1b[2J\x1b[H{frame}");
    let _ = stdout.flush();
}

struct AlternateScreen<W: Write> {
    writer: W,
}

impl AlternateScreen<io::Stdout> {
    fn enter() -> Self {
        Self::enter_with(io::stdout())
    }
}

impl<W: Write> AlternateScreen<W> {
    fn enter_with(mut writer: W) -> Self {
        let _ = write!(writer, "\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H");
        let _ = writer.flush();
        Self { writer }
    }
}

impl<W: Write> Drop for AlternateScreen<W> {
    fn drop(&mut self) {
        let _ = write!(self.writer, "\x1b[?25h\x1b[?1049l");
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::mpsc;
    use std::time::Instant;

    fn sample_snapshot() -> DashboardSnapshot {
        DashboardSnapshot {
            project: ProjectSummary {
                root: PathBuf::from("/work/demo"),
                env_file: PathBuf::from("/work/demo/env.jet"),
                env_present: true,
                typed: false,
                active_environment: None,
                declared_packages: Some(2),
                locked_packages: Some(2),
                lock: LockState::Ready,
                active: false,
                active_refs: String::new(),
                update_channels: 1,
            },
            store: StoreSummary {
                path: PathBuf::from("/home/demo/.local/share/jet/hangar"),
                bytes: 12_345,
                packages: 3,
                health: Doctor::Health::Healthy,
            },
            doctor: Doctor::Report {
                checks: vec![Doctor::Check {
                    name: "hangar",
                    health: Doctor::Health::Healthy,
                    detail: "ready".to_string(),
                    fix: String::new(),
                }],
            },
            activity: vec![Activity {
                name: "ripgrep".to_string(),
                version: "14.1".to_string(),
                when: "just now".to_string(),
            }],
        }
    }

    #[test]
    fn non_tty_selects_ansi_free_status_summary() {
        assert!(!interactive_mode(false, false));
        assert!(!interactive_mode(false, true));
        let summary = render_status_summary(&sample_snapshot());
        assert!(!summary.contains('\x1b'));
        assert!(summary.contains("Jetpack status"));
        assert!(summary.contains("Environment: ready"));
        assert!(summary.contains("Store: 3 package(s)"));
        assert!(summary.contains("Recent activity: ripgrep 14.1"));
    }

    #[test]
    fn tty_selects_dashboard_and_q_quits() {
        assert!(interactive_mode(true, true));
        assert!(key_quits(&Key::Char('q')));
        assert!(key_quits(&Key::Char('Q')));
        assert!(key_quits(&Key::Escape));
        assert!(key_quits(&Key::CtrlC));
        assert!(key_quits(&Key::Eof));
        assert!(!key_quits(&Key::Char('r')));

        let dashboard = render_dashboard_width(&sample_snapshot(), 100);
        assert!(dashboard.contains("Jetpack dashboard"));
        assert!(dashboard.contains("Project environment"));
        assert!(dashboard.contains("Available updates"));
        assert!(dashboard.contains("Doctor findings"));
        assert!(dashboard.contains("Recent activity"));
        assert!(dashboard.contains("q quit"));
    }

    #[test]
    fn input_before_data_quit_works_with_stalled_loader() {
        let (release, wait) = mpsc::channel();
        let mut loader = Some(SnapshotLoader::spawn(move || {
            let _ = wait.recv();
            LoadResult::Ready(sample_snapshot())
        }));
        let mut frame = DashboardFrame::Loading;
        let mut reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let mut rendered = Vec::new();
        let mut start_loader = || SnapshotLoader::spawn(|| LoadResult::Ready(sample_snapshot()));

        let code = run_interactive_loop(
            &mut reader,
            &mut loader,
            &mut frame,
            |frame| rendered.push(render_frame_width(frame, 80)),
            &mut start_loader,
            Instant::now,
        );

        assert_eq!(code, 0);
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].contains("reading store…"));
        release.send(()).unwrap();
        loader.as_mut().unwrap().join();
    }

    #[test]
    fn terminal_restore_on_panic() {
        let mut output = Vec::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _screen = AlternateScreen::enter_with(&mut output);
            panic!("dashboard test panic");
        }));

        assert!(result.is_err());
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("\x1b[?1049h"));
        assert!(output.ends_with("\x1b[?25h\x1b[?1049l"));
    }

    #[test]
    fn store_busy_rendering_on_load_timeout() {
        let (release, wait) = mpsc::channel();
        let mut loader = SnapshotLoader::spawn(move || {
            let _ = wait.recv();
            LoadResult::Ready(sample_snapshot())
        });
        let mut frame = DashboardFrame::Loading;
        let started = loader.started;

        assert!(mark_load_timeout(
            &mut frame,
            &loader,
            started + STORE_LOAD_TIMEOUT
        ));
        assert!(render_frame_width(&frame, 80).contains("store busy (another jetpack is running)"));

        release.send(()).unwrap();
        loader.join();
    }
}
