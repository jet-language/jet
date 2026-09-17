//! dev / repl / doctor / explain / completions / bind / eval / emit
//! developer-tooling subcommand handlers.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{exit, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use jet::Diagnostics::{json_str as json_string, ColorChoice};
use jet::ExitCodes;
use jet::RecordIndex::{RecordBudget, RecordCapture, RecordIndex, RecordKind};
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};

use crate::CmdCompile::{build, stem};
use crate::{report_problems, BuildProfile, OutputMode};
pub(crate) use jet_devserver::{watch_policy_from, WatchPolicy};

struct DevCaptureSession {
    name: String,
    capture: crate::ProveReplay::NamedCapture,
    budget: RecordBudget,
}

fn dev_capture_budget(file: &str) -> (jet::Package::DevCaptureSetting, RecordBudget) {
    let records = jet::Loader::package_facts_for_entry(Path::new(file))
        .ok()
        .flatten()
        .map(|facts| facts.dev.records)
        .unwrap_or_default();
    let budget =
        RecordBudget::new(records.budget.max_bytes, records.budget.max_records).unwrap_or_default();
    (records.capture, budget)
}

fn configure_dev_capture_budget(budget: RecordBudget) -> Result<(), String> {
    let mut index = RecordIndex::load_for_project(".")?;
    index.set_budget(budget)?;
    index.store()
}

fn generated_dev_capture_name(source: &str) -> String {
    let digest = jet::SHA256::sha256_hex(source.as_bytes());
    let short = digest.get(..16).unwrap_or(&digest);
    format!("dev-{short}-{}", std::process::id())
}

fn start_dev_capture(
    file: &str,
    source: &str,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    record_name: Option<&str>,
    no_capture: bool,
    mode: OutputMode,
) -> Option<DevCaptureSession> {
    let (setting, budget) = dev_capture_budget(file);
    if no_capture {
        if !mode.quiet && !mode.json {
            eprintln!("capture: skipped (no-capture override)");
        }
        return None;
    }
    if matches!(setting, jet::Package::DevCaptureSetting::Off) {
        if !mode.quiet && !mode.json {
            eprintln!("capture: skipped (package policy)");
        }
        return None;
    }
    let explicit = record_name.is_some();
    if profile == "release" && !explicit {
        if !mode.quiet && !mode.json {
            eprintln!("capture: skipped (release mode)");
        }
        return None;
    }
    if let Err(error) = configure_dev_capture_budget(budget) {
        if explicit {
            eprintln!("capture: record index unavailable: {error}");
        } else if !mode.quiet && !mode.json {
            eprintln!("capture: skipped (record index unavailable: {error})");
        }
        return None;
    }
    let name = record_name
        .map(str::to_owned)
        .unwrap_or_else(|| generated_dev_capture_name(source));
    let capture =
        crate::ProveReplay::begin_named_capture(file, &name, profile, setting_overrides, mode.json)
            .unwrap_or_else(|status| exit(status));
    if !mode.quiet && !mode.json {
        eprintln!(
            "capture: safe Time only; budget={} bytes/{} records",
            budget.max_bytes, budget.max_records
        );
    }
    Some(DevCaptureSession {
        name,
        capture,
        budget,
    })
}

fn finish_dev_capture(
    session: &DevCaptureSession,
    exit_code: i32,
    mode: OutputMode,
) -> Option<String> {
    crate::ProveReplay::finish_named_capture(&session.capture, exit_code, mode.json)
        .unwrap_or_else(|status| exit(status));
    let path = PathBuf::from(format!(".jet/replays/{}.jetproof-replay", session.name));
    let link = match crate::ProveReplay::index_named_replay_artifact(
        &session.capture,
        &path,
        RecordCapture::Safe,
    ) {
        Ok(link) => link,
        Err(error) => {
            if !mode.json {
                eprintln!("capture: index skipped ({error})");
            }
            return None;
        }
    };
    let mut index = match RecordIndex::load_for_project(".") {
        Ok(index) => index,
        Err(error) => {
            if !mode.json {
                eprintln!("capture: retention skipped ({error})");
            }
            return None;
        }
    };
    if let Err(error) = index
        .set_budget(session.budget)
        .and_then(|()| index.store())
    {
        if !mode.json {
            eprintln!("capture: retention skipped ({error})");
        }
        return None;
    }
    if index
        .find(RecordKind::Replay, &link.artifact_id, true)
        .is_some()
    {
        Some(link.artifact_id)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
enum DevSessionAction {
    Rerun,
    RestartFresh,
    Tests,
    FailedClaimsOnly,
    GamePlay,
    GameSimulate,
    GamePause,
    GameResume,
    GameStep,
    GameFrameAdvance,
    GameEdit,
    GameSelectCategory,
    GameInspect,
    GameEvaluate,
    GameEject,
    GameKeep,
    GameDiscard,
    Quit,
}

fn dev_session_action(byte: u8, game_controls_enabled: bool) -> Option<DevSessionAction> {
    match byte as char {
        'P' if game_controls_enabled => Some(DevSessionAction::GamePlay),
        'S' if game_controls_enabled => Some(DevSessionAction::GameSimulate),
        'p' if game_controls_enabled => Some(DevSessionAction::GamePause),
        'R' if game_controls_enabled => Some(DevSessionAction::GameResume),
        't' if game_controls_enabled => Some(DevSessionAction::GameStep),
        'f' if game_controls_enabled => Some(DevSessionAction::GameFrameAdvance),
        'i' if game_controls_enabled => Some(DevSessionAction::GameEdit),
        'c' if game_controls_enabled => Some(DevSessionAction::GameSelectCategory),
        'o' if game_controls_enabled => Some(DevSessionAction::GameInspect),
        'v' if game_controls_enabled => Some(DevSessionAction::GameEvaluate),
        'e' if game_controls_enabled => Some(DevSessionAction::GameEject),
        'k' if game_controls_enabled => Some(DevSessionAction::GameKeep),
        'd' if game_controls_enabled => Some(DevSessionAction::GameDiscard),
        c if c == jet::Syntax::SESSION_KEY_RERUN.chars().next().unwrap() => {
            Some(DevSessionAction::Rerun)
        }
        c if c == jet::Syntax::SESSION_KEY_RESTART.chars().next().unwrap() => {
            Some(DevSessionAction::RestartFresh)
        }
        c if c == jet::Syntax::SESSION_KEY_TESTS.chars().next().unwrap() => {
            Some(DevSessionAction::Tests)
        }
        c if c
            == jet::Syntax::SESSION_KEY_FAILED_CLAIMS
                .chars()
                .next()
                .unwrap() =>
        {
            Some(DevSessionAction::FailedClaimsOnly)
        }
        c if c == jet::Syntax::SESSION_KEY_QUIT.chars().next().unwrap() => {
            Some(DevSessionAction::Quit)
        }
        _ => None,
    }
}

fn dev_session_label(action: DevSessionAction) -> &'static str {
    match action {
        DevSessionAction::Rerun => "Re-run",
        DevSessionAction::RestartFresh => "Restart Fresh",
        DevSessionAction::Tests => "Tests",
        DevSessionAction::FailedClaimsOnly => "Failed Claims Only",
        DevSessionAction::GamePlay => "Game Play",
        DevSessionAction::GameSimulate => "Game Simulate",
        DevSessionAction::GamePause => "Game Pause",
        DevSessionAction::GameResume => "Game Resume",
        DevSessionAction::GameStep => "Game Step",
        DevSessionAction::GameFrameAdvance => "Game Frame Advance",
        DevSessionAction::GameEdit => "Game Edit",
        DevSessionAction::GameSelectCategory => "Game Select Category",
        DevSessionAction::GameInspect => "Game Inspect",
        DevSessionAction::GameEvaluate => "Game Evaluate",
        DevSessionAction::GameEject => "Game Eject",
        DevSessionAction::GameKeep => "Game Keep",
        DevSessionAction::GameDiscard => "Game Discard",
        DevSessionAction::Quit => "Quit",
    }
}

fn spawn_dev_session_input(wake: jet_devserver::WatchWake) -> Receiver<u8> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut byte = [0u8; 1];
        loop {
            match stdin.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if sender.send(byte[0]).is_err() {
                        break;
                    }
                    wake.wake();
                }
            }
        }
    });
    receiver
}

fn terminal_key_from_byte(byte: u8) -> Option<jet_devserver::TerminalHost::TerminalKey> {
    use jet_devserver::TerminalHost::TerminalKey;

    Some(match byte {
        b'\t' => TerminalKey::Tab,
        b'\n' | b'\r' => TerminalKey::Enter,
        0x1b => TerminalKey::Escape,
        0x7f => TerminalKey::Character('\u{7f}'),
        byte if byte.is_ascii() => TerminalKey::Character(byte as char),
        _ => return None,
    })
}

fn terminal_host_for(mode: OutputMode, profile: &str) -> jet_devserver::TerminalHost::TerminalHost {
    use jet_devserver::TerminalHost::{
        TerminalHost, TerminalHostAccess, TerminalHostCapabilities, TerminalHostConfig,
        TerminalViewport, DEFAULT_TERMINAL_COLUMNS, DEFAULT_TERMINAL_ROWS,
    };

    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let ansi = tty
        && !no_color
        && mode.color_stderr()
        && std::env::var("TERM").map_or(true, |term| term != "dumb");
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width > 0)
        .unwrap_or(DEFAULT_TERMINAL_COLUMNS);
    let access = TerminalHostAccess::new(true, profile == "release", mode.json);
    let capabilities = TerminalHostCapabilities::new(
        tty,
        ansi,
        no_color,
        width,
        false,
        mode.quiet || mode.json,
        access,
    );
    TerminalHost::new(TerminalHostConfig::new(
        TerminalViewport::new(width, DEFAULT_TERMINAL_ROWS),
        capabilities,
    ))
}

fn apply_terminal_action(
    session: &jet_devserver::ResidentDevSession,
    action: &jet_devserver::TerminalHost::TerminalAction,
) {
    use jet_devserver::TerminalHost::TerminalAction;

    let request = match action {
        TerminalAction::FocusPanel { panel_id } => {
            let Some(panel_id) = jet_devserver::Devtools::catalog::descriptors()
                .iter()
                .find(|descriptor| descriptor.id.as_str() == panel_id)
                .map(|descriptor| descriptor.id.as_str())
            else {
                return;
            };
            format!(
                "{{\"panel_id\":{},\"origin\":\"terminal\"}}",
                json_string(panel_id)
            )
        }
        TerminalAction::Inspect { panel_id, item_key } => {
            let Some(panel_id) = jet_devserver::Devtools::catalog::descriptors()
                .iter()
                .find(|descriptor| descriptor.id.as_str() == panel_id)
                .map(|descriptor| descriptor.id.as_str())
            else {
                return;
            };
            format!(
                "{{\"panel_id\":{},\"item_key\":{},\"origin\":\"terminal\"}}",
                json_string(panel_id),
                json_string(item_key)
            )
        }
        TerminalAction::FocusJob => "{\"panel_id\":\"jobs\",\"origin\":\"terminal\"}".to_string(),
        TerminalAction::MoveCursor { cursor } => {
            let request = format!("{{\"cursor\":{},\"origin\":\"terminal\"}}", cursor);
            let _ = session.set_devtools_cursor(&request);
            return;
        }
        _ => return,
    };
    let _ = session.set_devtools_selection(&request);
}

fn dispatch_terminal_game_control(
    session: &jet_devserver::ResidentDevSession,
    file: &str,
    action: DevSessionAction,
) -> Result<usize, String> {
    use jet_foundation::DevtoolsControl::{
        JetDevtoolsGameControlKind, JetDevtoolsGameControlRequest,
    };
    if matches!(action, DevSessionAction::GameKeep) {
        return Err(
            "game keep requires an authored source selection with revision and source span"
                .to_string(),
        );
    }
    if matches!(action, DevSessionAction::GameEvaluate) {
        return Err(
            "game evaluate requires a paused world selection and an evaluation expression"
                .to_string(),
        );
    }
    let kind = match action {
        DevSessionAction::GamePlay => JetDevtoolsGameControlKind::Play,
        DevSessionAction::GameSimulate => JetDevtoolsGameControlKind::Simulate,
        DevSessionAction::GamePause => JetDevtoolsGameControlKind::Pause,
        DevSessionAction::GameResume => JetDevtoolsGameControlKind::Resume,
        DevSessionAction::GameStep => JetDevtoolsGameControlKind::Step,
        DevSessionAction::GameFrameAdvance => JetDevtoolsGameControlKind::FrameAdvance,
        DevSessionAction::GameEdit => JetDevtoolsGameControlKind::Edit,
        DevSessionAction::GameSelectCategory => JetDevtoolsGameControlKind::SelectCategory,
        DevSessionAction::GameInspect => JetDevtoolsGameControlKind::SelectWorld,
        DevSessionAction::GameEject => JetDevtoolsGameControlKind::Eject,
        DevSessionAction::GameDiscard => JetDevtoolsGameControlKind::Discard,
        _ => return Err("not a game control action".to_string()),
    };
    let (source_id, _build_id, revision, world_id) = live_lineage_for_file(file);
    let selected_target = session.selected_target();
    let selected_output = session.selected_output();
    let mut request = JetDevtoolsGameControlRequest {
        session_id: session.id().to_string(),
        request_id: format!("terminal-game-{}", dev_job_now_ms()),
        kind,
        source_id: Some(source_id),
        revision: Some(revision),
        world_id: None,
        actor_id: None,
        component_id: None,
        authored_instance_id: None,
        source_span_start: None,
        source_span_end: None,
        field: None,
        value: None,
        category: None,
        expression: None,
        required_authority: None,
        frame_id: None,
        budget: None,
    };
    match kind {
        JetDevtoolsGameControlKind::SelectCategory => {
            request.category = Some(
                selected_output
                    .or(selected_target)
                    .ok_or_else(|| "select_category requires a selected category".to_string())?,
            );
        }
        JetDevtoolsGameControlKind::SelectWorld => {
            request.world_id = Some(world_id);
            request.actor_id = selected_target;
            request.component_id = selected_output;
        }
        JetDevtoolsGameControlKind::Eject => {
            request.world_id = Some(world_id);
            request.actor_id = Some(
                selected_target
                    .ok_or_else(|| "eject requires a selected world actor".to_string())?,
            );
        }
        _ => {}
    }
    request.validate()?;
    session.enqueue_game_control(request)?;
    session.dispatch_pending_devtools_commands()
}
fn terminal_session_action(
    byte: u8,
    host: &mut jet_devserver::TerminalHost::TerminalHost,
    session: &jet_devserver::ResidentDevSession,
    game_controls_enabled: bool,
) -> Option<DevSessionAction> {
    let Some(key) = terminal_key_from_byte(byte) else {
        return dev_session_action(byte, game_controls_enabled);
    };
    let action = host.handle_key(key);
    apply_terminal_action(session, &action);
    if matches!(
        action,
        jet_devserver::TerminalHost::TerminalAction::ReturnToPanel
    ) {
        if let Some(panel_id) = host.focus().panel_id.clone() {
            let focus_action = jet_devserver::TerminalHost::TerminalAction::FocusPanel { panel_id };
            apply_terminal_action(session, &focus_action);
        }
    }
    match action {
        jet_devserver::TerminalHost::TerminalAction::FocusPanel { .. }
        | jet_devserver::TerminalHost::TerminalAction::Inspect { .. }
        | jet_devserver::TerminalHost::TerminalAction::ReturnToPanel
        | jet_devserver::TerminalHost::TerminalAction::MoveCursor { .. }
        | jet_devserver::TerminalHost::TerminalAction::FocusJob => None,
        jet_devserver::TerminalHost::TerminalAction::Rerun => Some(DevSessionAction::Rerun),
        jet_devserver::TerminalHost::TerminalAction::Restart => {
            Some(DevSessionAction::RestartFresh)
        }
        jet_devserver::TerminalHost::TerminalAction::Quit => Some(DevSessionAction::Quit),
        jet_devserver::TerminalHost::TerminalAction::Ignored => {
            dev_session_action(byte, game_controls_enabled)
        }
    }
}
fn refresh_terminal_host(
    host: &mut jet_devserver::TerminalHost::TerminalHost,
    session: &jet_devserver::ResidentDevSession,
    previous: &mut Option<jet_devserver::TerminalHost::TerminalFrame>,
) {
    let Ok(frame) = host.sync_session(session) else {
        return;
    };
    let changed = previous.as_ref().map_or(true, |old| {
        old.tree != frame.tree
            || old.status != frame.status
            || old.focus != frame.focus
            || old.keyboard != frame.keyboard
            || old.cursor != frame.cursor
            || old.revision != frame.revision
    });
    if changed && !frame.capabilities.quiet {
        if frame.capabilities.tty && frame.capabilities.ansi {
            print!("\x1b[2J\x1b[H{}", frame.text());
        } else {
            println!("{}", frame.plain_text());
        }
        let _ = std::io::stdout().flush();
    }
    *previous = Some(frame);
}

fn run_dev_tests(
    file: &str,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    filters: &[String],
) -> Vec<String> {
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("[Tests] could not locate jet: {error}");
            return Vec::new();
        }
    };
    // The CLI filter is a single substring. Run one child per remembered
    // failure so `f` remains failed-claims-only when several claims failed;
    // passing the whole list to one child would silently drop every filter.
    let selected_filters: Vec<Option<&str>> = if filters.len() > 1 {
        filters.iter().map(|filter| Some(filter.as_str())).collect()
    } else {
        vec![filters.first().map(String::as_str)]
    };
    let mut failed = Vec::new();
    for filter in selected_filters {
        let mut command = Command::new(&executable);
        command.arg("test").arg(file);
        if profile != "dev" {
            command.arg(format!("--profile={profile}"));
        }
        for (key, value) in setting_overrides {
            command.arg(format!("--set={key}={value}"));
        }
        if let Some(filter) = filter {
            command.arg(format!("--filter={filter}"));
        }
        let output = match command.output() {
            Ok(output) => output,
            Err(error) => {
                eprintln!("[Tests] could not start test runner: {error}");
                continue;
            }
        };
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        for name in output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter_map(|line| {
                let line = String::from_utf8_lossy(line);
                let line = line.trim();
                line.strip_suffix(": FAIL")
                    .or_else(|| line.strip_prefix("FAIL "))
                    .map(str::to_string)
            })
        {
            if !failed.contains(&name) {
                failed.push(name);
            }
        }
    }
    failed
}

/// Preserve the Prelude's stream order when a run outcome crosses the CLI
/// adapter as separate stdout/stderr buffers.
fn emit_run_output(stdout: &str, stderr: &str) {
    print!("{stdout}");
    let _ = std::io::stdout().flush();
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }
}
fn emit_internal_fault(stdout: &str, what: &str) -> ! {
    emit_run_output(stdout, "");
    let _ = std::io::stdout().flush();
    eprintln!("{}", jet::Diagnostics::render_ice_report(what, "", false));
    exit(ExitCodes::ICE);
}
fn exit_if_internal_fault(diagnostics: &[jet::Diagnostics::Diagnostic]) {
    if let Some((stdout, what)) = diagnostics
        .iter()
        .find_map(jet::Diagnostics::Diagnostic::runtime_host_fault_parts)
    {
        emit_internal_fault(stdout, what);
    }
}

pub(crate) fn open_canvas_browser(url: &str) {
    let explicit = std::env::var_os("JET_CANVAS_BROWSER")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("BROWSER").filter(|value| !value.is_empty()));
    let mut command = if let Some(browser) = explicit {
        let mut command = Command::new(browser);
        command.arg(url);
        command
    } else {
        #[cfg(target_os = "macos")]
        {
            let mut command = Command::new("open");
            command.arg(url);
            command
        }
        #[cfg(target_os = "windows")]
        {
            // Pass the URL directly to Explorer. `cmd /C start` would make
            // URL metacharacters part of a command string.
            let mut command = Command::new("explorer.exe");
            command.arg(url);
            command
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let mut command = Command::new("xdg-open");
            command.arg(url);
            command
        }
    };
    if let Err(error) = command.spawn() {
        crate::emit_cli_diagnostic_with_fix(
            "E2105",
            format!("Canvas browser launch failed: {error}"),
            format!("open `{url}` in a browser, or set JET_CANVAS_BROWSER to a browser command"),
        );
        eprintln!("Canvas: {url}");
    }
}

fn print_canvas_hint(file: &str, mode: OutputMode, printed: &mut bool) {
    if *printed
        || mode.json
        || mode.quiet
        || !std::io::stdin().is_terminal()
        || !std::io::stdout().is_terminal()
    {
        return;
    }
    println!("Canvas: jet dev {file} {}", jet::CLI::CANVAS_FLAG);
    *printed = true;
}

/// `jet dev <file>` — the E2-M4 watch/interpret loop (D-DEV4), extended by c77
/// with three-mode routing (D-DEVMODE1=A) and hot-swap/restart (D-HOTSWAP1=B).
/// Re-checks and re-runs on dependency-aware invalidation (#439 / E3-UL6),
/// streaming output. The per-iteration work lives in
/// `jet::Interpreter::dev_iteration` (so it can be golden-tested); this is the
/// thin std-only watcher around the shared `WatchSession` engine (I6: no
/// `notify` crate).
pub(crate) fn run_dev(
    file: &str,
    entry_fn: Option<&str>,
    try_anyway: bool,
    policy: WatchPolicy,
    gates: jet::Policy::GateSet,
    mode: OutputMode,
    use_interpreter: bool,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    program_args: &[&String],
    record_name: Option<&str>,
    no_capture: bool,
    canvas: bool,
    canvas_options: Option<jet_devserver::WebHost::CanvasHostOptions>,
) {
    crate::CmdCompile::require_project_environment("dev", Path::new(file), mode);
    let mut runtime_args = Vec::with_capacity(program_args.len() + 1);
    runtime_args.push(file.to_string());
    runtime_args.extend(program_args.iter().map(|arg| (*arg).clone()));
    jet_jit::with_program_args(&runtime_args, || {
        run_dev_inner(
            file,
            entry_fn,
            try_anyway,
            policy,
            gates,
            mode,
            use_interpreter,
            profile,
            setting_overrides,
            program_args,
            record_name,
            no_capture,
            canvas,
            canvas_options,
        );
    });
}

fn run_dev_iteration_with_entry(
    file: &str,
    entry_fn: Option<&str>,
    program_args: &[&String],
    try_anyway: bool,
    use_interpreter: bool,
    gates: jet::Policy::GateSet,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> jet::Interpreter::RunWithLints {
    let args = program_args
        .iter()
        .map(|arg| arg.as_str())
        .collect::<Vec<_>>();
    jet::Interpreter::dev_iteration_with_args_and_gates_profile_and_settings_with_lints_and_entry(
        file,
        &args,
        try_anyway,
        use_interpreter,
        gates,
        profile,
        setting_overrides,
        entry_fn,
    )
}

fn detect_static_output_root(file: &str) -> Option<PathBuf> {
    let source_dir = Path::new(file)
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut candidates = ["dist", "public", "out", "build"]
        .into_iter()
        .map(|name| source_dir.join(name))
        .collect::<Vec<_>>();
    if let Ok(entries) = fs::read_dir(source_dir) {
        let mut extras = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        extras.sort();
        candidates.extend(extras);
    }
    candidates.dedup();
    candidates.into_iter().find_map(|path| {
        let metadata = fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || !path.join("index.html").is_file()
        {
            return None;
        }
        fs::canonicalize(path).ok()
    })
}

fn start_static_output_host(
    file: &str,
    release_policy: &jet::Package::ReleaseDevtoolsPolicy,
    pending_application_listener: &mut Option<TcpListener>,
    session: Arc<jet_devserver::ResidentDevSession>,
) -> Option<jet_devserver::WebHost::WebHost> {
    let root = detect_static_output_root(file)?;
    let application_listener = pending_application_listener.take()?;
    let host = match jet_devserver::WebHost::WebHost::bind_static_with_policy(
        file,
        &root,
        false,
        application_listener,
        session,
        release_policy.clone(),
    ) {
        Ok(host) => host,
        Err(message) => {
            eprintln!("{message}");
            exit(ExitCodes::USER_ERROR);
        }
    };
    host.start_canvas();
    Some(host)
}

fn execute_native_project_rebuild(
    file: &str,
    entry_fn: Option<&str>,
    program_args: &[&String],
    try_anyway: bool,
    gates: jet::Policy::GateSet,
    mode: OutputMode,
    use_interpreter: bool,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    release_policy: &jet::Package::ReleaseDevtoolsPolicy,
    resident_session: &Arc<jet_devserver::ResidentDevSession>,
    canvas_host: Option<&jet_devserver::WebHost::WebHost>,
    static_host: &mut Option<jet_devserver::WebHost::WebHost>,
    pending_application_listener: &mut Option<TcpListener>,
    prev_snapshot: &mut Option<jet::CheckedMirSnapshot>,
    game_controls_enabled: &mut bool,
    canvas_hint_printed: &mut bool,
) -> Result<(), String> {
    resident_session.mark_building();
    let _source_transaction = canvas_host.map(|host| host.lock_source_transaction());
    if let Some(host) = canvas_host.or(static_host.as_ref()) {
        host.mark_building();
    }
    let previous_snapshot = prev_snapshot.take();
    let persist_before_rebuild = jet_foundation::Persist::shared_clone();
    let rebuilt_snapshot = render_dev_iteration(
        file,
        entry_fn,
        program_args,
        try_anyway,
        gates,
        mode,
        use_interpreter,
        profile,
        setting_overrides,
    );
    if rebuilt_snapshot.is_none() {
        jet_foundation::Persist::shared_replace(persist_before_rebuild);
        jet_jit::discard_hot_swap_plan();
    }
    *prev_snapshot = rebuilt_snapshot.or(previous_snapshot);
    *game_controls_enabled = prev_snapshot.as_ref().is_some_and(|snapshot| {
        matches!(
            jet::Interpreter::detect_dev_mode(&snapshot.bundle),
            jet::Interpreter::DevMode::Resident
        )
    });
    if prev_snapshot.is_some() {
        resident_session.mark_ready();
    } else {
        resident_session.mark_error("E2105", "terminal rerun is unavailable");
    }
    if prev_snapshot.is_some() && canvas_host.is_none() && static_host.is_none() {
        *static_host = start_static_output_host(
            file,
            release_policy,
            pending_application_listener,
            Arc::clone(resident_session),
        );
        if let Some(host) = static_host.as_ref() {
            let (source_id, build_id, revision, world_id) = live_lineage_for_file(file);
            let _ = host
                .resident_session()
                .set_live_lineage(&source_id, &build_id, &revision, &world_id);
        }
    }
    if let Some(host) = canvas_host.or(static_host.as_ref()) {
        if prev_snapshot.is_some() {
            host.mark_ready(0, true);
        } else {
            host.mark_error(
                "E2105".to_string(),
                format!("Canvas kept the last-good program; `{file}` is not ready"),
                true,
            );
        }
    }
    if prev_snapshot.is_some() {
        if let Some(host) = static_host.as_ref() {
            let (source_id, build_id, revision, world_id) = live_lineage_for_file(file);
            let _ = host
                .resident_session()
                .set_live_lineage(&source_id, &build_id, &revision, &world_id);
        }
    }
    if static_host.is_none() && prev_snapshot.is_some() {
        print_canvas_hint(file, mode, canvas_hint_printed);
    }
    if prev_snapshot.is_some() {
        Ok(())
    } else {
        Err("terminal rerun is unavailable".to_string())
    }
}

fn register_dev_watch_paths(watch: &mut jet_devserver::WatchSession, path: &Path) {
    watch.register_game_path(
        path.to_path_buf(),
        jet_foundation::Game::JetGameChangeKind::Script,
    );
    if let Some(parent) = path.parent() {
        watch.register_asset_root(parent.join("assets"));
    }
}

fn run_dev_inner(
    file: &str,
    entry_fn: Option<&str>,
    try_anyway: bool,
    policy: WatchPolicy,
    gates: jet::Policy::GateSet,
    mode: OutputMode,
    use_interpreter: bool,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    program_args: &[&String],
    record_name: Option<&str>,
    no_capture: bool,
    canvas: bool,
    canvas_options: Option<jet_devserver::WebHost::CanvasHostOptions>,
) {
    let path = Path::new(file);
    if !path.exists() {
        crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
        exit(ExitCodes::USER_ERROR);
    }
    // Fold manifest profile and deployment environment facts before any host
    // binds.  Every production-capable host receives this same typed policy.
    let release_policy = crate::CmdCompile::release_devtools_policy_for_name(file, profile, mode);

    let canvas_host = if canvas && policy != WatchPolicy::Once {
        let options = canvas_options.unwrap_or_default();
        match jet_devserver::WebHost::WebHost::bind_canvas_with_options_and_policy(
            file,
            false,
            &options,
            release_policy.clone(),
        ) {
            Ok(host) => Some(host),
            Err(message) => {
                crate::emit_cli_diagnostic_with_fix(
                    "E2105",
                    message,
                    "close the existing Canvas session or choose another `--canvas-port`"
                        .to_string(),
                );
                exit(ExitCodes::USER_ERROR);
            }
        }
    } else {
        None
    };

    // D-DEVR-PROD1=A / I9: native `jet dev` and `jet run --watch` use the
    // same receipt context as one-shot `jet run`; the execution tier only
    // marshals the shared Prelude writer.
    let source = fs::read_to_string(file).unwrap_or_default();
    crate::ProductionReceipt::prepare(file, &source, program_args).install();

    // The dev loop re-prints the run's output and nothing else reads it, so the
    // program owns the process's streams. Without this a piped `jet dev` showed
    // nothing at all for a program that prints and keeps running, because both
    // in-process tiers held the bytes until the run ended.
    jet_jit::set_program_owns_streams();

    let record = start_dev_capture(
        file,
        &source,
        profile,
        setting_overrides,
        record_name,
        no_capture,
        mode,
    );

    // `--watch=off`: run once and exit (no loop).
    if policy == WatchPolicy::Once {
        let run = run_dev_iteration_with_entry(
            file,
            entry_fn,
            program_args,
            try_anyway,
            use_interpreter,
            gates,
            profile,
            setting_overrides,
        );
        render_lints(file, mode, &run.lints);
        let outcome = run.outcome;
        render_dev_outcome(&outcome, file, mode);
        let status = match &outcome {
            jet::Interpreter::RunOutcome::Ran { exit_code, .. } => *exit_code,
            jet::Interpreter::RunOutcome::Problems(_) => ExitCodes::USER_ERROR,
        };
        if let Some(capture) = record.as_ref() {
            if let Some(replay_id) = finish_dev_capture(capture, status, mode) {
                if (status != ExitCodes::OK
                    || matches!(&outcome, jet::Interpreter::RunOutcome::Problems(_)))
                    && !mode.quiet
                    && !mode.json
                {
                    println!("replay: {replay_id}");
                }
            }
        }
        exit_dev_outcome(outcome);
    }

    let mut pending_application_listener = if canvas_host.is_none() {
        jet_devserver::WebHost::WebHost::bind_application_preview_listener(None).ok()
    } else {
        None
    };
    let application_port = pending_application_listener
        .as_ref()
        .and_then(|listener| listener.local_addr().ok())
        .map(|address| address.port())
        .unwrap_or(0);
    let resident_session = canvas_host
        .as_ref()
        .map(|host| host.resident_session())
        .unwrap_or_else(|| {
            Arc::new(jet_devserver::ResidentDevSession::new(
                file,
                0,
                application_port,
            ))
        });
    let (source_id, build_id, revision, world_id) = live_lineage_for_file(file);
    let _ = resident_session.set_live_lineage(&source_id, &build_id, &revision, &world_id);
    let _project_rebuild_executor = match resident_session.register_project_rebuild_executor() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("{error}");
            exit(ExitCodes::USER_ERROR);
        }
    };
    provision_live_lineage_env(
        release_policy.local_rail,
        resident_session.id(),
        &source_id,
        &build_id,
        &revision,
        &world_id,
    );

    // Open the watcher before the first run. The running callable owns stdout,
    // so a caller can edit the file as soon as its first line appears; opening
    // the session afterward would sample that edit as the baseline and lose
    // the invalidation.
    let mut watch = match jet_devserver::WatchSession::open(path) {
        Ok(watch) => watch,
        Err(diagnostic) => {
            eprint!(
                "{}",
                jet::render_all_colored(file, "", &[diagnostic], mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    };
    register_dev_watch_paths(&mut watch, path);
    let mut incremental_cache = jet::Sema::IncrementalSemaCache::new();


    // The checked snapshot from the last successful load, kept so a resident
    // edit can be diffed against it for type stability (D-HOTSWAP1).
    let mut prev_snapshot = render_dev_iteration(
        file,
        entry_fn,
        program_args,
        try_anyway,
        gates,
        mode,
        use_interpreter,
        profile,
        setting_overrides,
    );
    if dev_incremental_reload_enabled(entry_fn, profile, gates, setting_overrides) {
        if prev_snapshot.is_some() {
            prime_dev_incremental_cache(file, &mut incremental_cache);
        }
    }

    let mut static_host = if canvas_host.is_none() && prev_snapshot.is_some() {
        start_static_output_host(
            file,
            &release_policy,
            &mut pending_application_listener,
            Arc::clone(&resident_session),
        )
    } else {
        None
    };
    let mut canvas_hint_printed = false;
    if let Some(host) = canvas_host.as_ref() {
        host.start_canvas();
        if prev_snapshot.is_some() {
            host.mark_ready(0, false);
        } else {
            host.mark_error(
                "E2105".to_string(),
                format!("Canvas kept the last-good program; `{file}` is not ready"),
                false,
            );
        }
        open_canvas_browser(&host.canvas_url());
    } else if let Some(host) = static_host.as_ref() {
        let _ = host
            .resident_session()
            .set_live_lineage(&source_id, &build_id, &revision, &world_id);
        if prev_snapshot.is_some() {
            host.mark_ready(0, false);
        }
    } else if prev_snapshot.is_some() {
        print_canvas_hint(file, mode, &mut canvas_hint_printed);
    }
    // D-SCHEDULE1 (card #505): due `#Job #Every(…)` fns fire on their own
    // schedule, independent of file-change ticks.
    let mut clock = JobClock::new();
    let mut persist = jet_devserver::PersistStore::new();
    let mut session = jet_devserver::SessionSnapshot::new(0, "gen-0", persist.clone())
        .with_lineage(
            source_id.clone(),
            build_id.clone(),
            revision.clone(),
            world_id.clone(),
        );
    let _devtools_sink_guard = resident_session.install_devtools_event_sink();
    let _native_overlay_guard = release_policy.panel_code.then(|| {
        let native_overlay_host = Arc::new(
            jet_devserver::NativeOverlayHost::NativeOverlayHostBridge::new(
                resident_session.clone(),
                false,
            ),
        );
        jet_devserver::Devtools::jet_devtools_install_native_host(
            resident_session.id().to_string(),
            native_overlay_host,
        )
        .expect("failed to install native devtools overlay host")
    });
    let mut game_dev_session = jet_foundation::Game::JetGameDevSession::new();
    if prev_snapshot.is_some() {
        resident_session.mark_ready();
    } else {
        resident_session.mark_error("E2105", "initial devtools build is unavailable");
    }
    let mut terminal_host = terminal_host_for(mode, profile);
    let mut terminal_frame = None;
    refresh_terminal_host(&mut terminal_host, &resident_session, &mut terminal_frame);

    let input = spawn_dev_session_input(watch.wake_handle());
    let mut failed_claims = Vec::new();
    let mut game_controls_enabled = prev_snapshot.as_ref().is_some_and(|snapshot| {
        matches!(
            jet::Interpreter::detect_dev_mode(&snapshot.bundle),
            jet::Interpreter::DevMode::Resident
        )
    });
    let mut first_tick = true;

    loop {
        let watch_woke = if first_tick {
            first_tick = false;
            true
        } else {
            watch.wait_for_change_for(next_dev_job_wake(prev_snapshot.as_ref()))
        };
        while let Ok(byte) = input.try_recv() {
            let action = if terminal_host.capabilities().tty {
                terminal_session_action(
                    byte,
                    &mut terminal_host,
                    &resident_session,
                    game_controls_enabled,
                )
            } else {
                dev_session_action(byte, game_controls_enabled)
            };
            let Some(action) = action else {
                continue;
            };
            if !mode.quiet {
                println!("Action: {}", dev_session_label(action));
            }
            match action {
                DevSessionAction::Rerun => {
                    let _ = execute_native_project_rebuild(
                        file,
                        entry_fn,
                        program_args,
                        try_anyway,
                        gates,
                        mode,
                        use_interpreter,
                        profile,
                        setting_overrides,
                        &release_policy,
                        &resident_session,
                        canvas_host.as_ref(),
                        &mut static_host,
                        &mut pending_application_listener,
                        &mut prev_snapshot,
                        &mut game_controls_enabled,
                        &mut canvas_hint_printed,
                    );
                }
                DevSessionAction::RestartFresh => {
                    let mut launch_profile =
                        jet::Codegen::MIREval::game_dev_protocol::GameDevLaunchProfile::new(
                            file,
                            jet::Codegen::MIREval::game_dev_protocol::GameDevRunMode::Headless,
                        );
                    launch_profile.record_phase(
                        jet::Codegen::MIREval::game_dev_protocol::GameDevPhase::Editing,
                        jet::Codegen::MIREval::game_dev_protocol::GameDevTransitionStatus::Applied,
                        "stop: previous resident game session replaced",
                    );
                    resident_session.mark_building();
                    game_dev_session = jet_foundation::Game::JetGameDevSession::new();

                    let _source_transaction = canvas_host
                        .as_ref()
                        .map(|host| host.lock_source_transaction());
                    if let Some(host) = canvas_host.as_ref().or(static_host.as_ref()) {
                        host.mark_building();
                    }
                    let (source_id, build_id, revision, world_id) = live_lineage_for_file(file);
                    let _ = resident_session
                        .set_live_lineage(&source_id, &build_id, &revision, &world_id);
                    provision_live_lineage_env(
                        release_policy.local_rail,
                        resident_session.id(),
                        &source_id,
                        &build_id,
                        &revision,
                        &world_id,
                    );

                    session = jet_devserver::SessionSnapshot::new(0, "gen-0", persist.clone())
                        .with_lineage(source_id, build_id, revision, world_id);
                    if let Err(error) = resident_session.reopen_game_asset_watcher(file) {
                        eprintln!("game assets: {error}");
                    }
                    if let Err(diagnostic) = watch.reopen(path) {
                        eprint!(
                            "{}",
                            jet::render_all_colored(
                                file,
                                "",
                                &[diagnostic],
                                mode.color_stderr(),
                            )
                        );
                        exit(ExitCodes::USER_ERROR);
                    }
                    register_dev_watch_paths(&mut watch, path);
                    prev_snapshot = render_dev_iteration(
                        file,
                        entry_fn,
                        program_args,
                        try_anyway,
                        gates,
                        mode,
                        use_interpreter,
                        profile,
                        setting_overrides,
                    );
                    incremental_cache.clear();
                    if dev_incremental_reload_enabled(entry_fn, profile, gates, setting_overrides) {
                        if prev_snapshot.is_some() {
                            prime_dev_incremental_cache(file, &mut incremental_cache);
                        }
                    }

                    game_controls_enabled = prev_snapshot.as_ref().is_some_and(|snapshot| {
                        matches!(
                            jet::Interpreter::detect_dev_mode(&snapshot.bundle),
                            jet::Interpreter::DevMode::Resident
                        )
                    });
                    if prev_snapshot.is_some() {
                        resident_session.mark_ready();
                    } else {
                        resident_session.mark_error("E2105", "terminal restart is unavailable");
                    }

                    let build_succeeded = prev_snapshot.is_some();
                    launch_profile.record_phase(
                        jet::Codegen::MIREval::game_dev_protocol::GameDevPhase::Editing,
                        if build_succeeded {
                            jet::Codegen::MIREval::game_dev_protocol::GameDevTransitionStatus::Applied
                        } else {
                            jet::Codegen::MIREval::game_dev_protocol::GameDevTransitionStatus::Rejected
                        },
                        if build_succeeded {
                            "build: checked program is ready"
                        } else {
                            "build: checked program is unavailable"
                        },
                    );
                    launch_profile.record_phase(
                        jet::Codegen::MIREval::game_dev_protocol::GameDevPhase::Playing,
                        if build_succeeded {
                            jet::Codegen::MIREval::game_dev_protocol::GameDevTransitionStatus::Applied
                        } else {
                            jet::Codegen::MIREval::game_dev_protocol::GameDevTransitionStatus::Rejected
                        },
                        if build_succeeded {
                            "run: resident game launch is ready"
                        } else {
                            "run: not started because build was rejected"
                        },
                    );
                    launch_profile.record_phase(
                        jet::Codegen::MIREval::game_dev_protocol::GameDevPhase::Editing,
                        jet::Codegen::MIREval::game_dev_protocol::GameDevTransitionStatus::Applied,
                        "stop: restart-fresh boundary complete",
                    );
                    // Keep the canonical profile value available in the
                    // headless representation as well; this is the same
                    // phase receipt, not a second policy or status path.
                    let headless_profile = launch_profile.headless_variant();
                    if let Err(error) = publish_game_launch_profile(
                        &resident_session,
                        file,
                        &headless_profile,
                        dev_job_now_ms(),
                    ) {
                        eprintln!("game launch profile: event rejected: {error}");
                    }
                    if !mode.quiet && !mode.json {
                        println!("game launch profile: {}", headless_profile.render_json());
                    }

                    if prev_snapshot.is_some() && canvas_host.is_none() && static_host.is_none() {
                        static_host = start_static_output_host(
                            file,
                            &release_policy,
                            &mut pending_application_listener,
                            Arc::clone(&resident_session),
                        );
                    }
                    if let Some(host) = canvas_host.as_ref().or(static_host.as_ref()) {
                        if prev_snapshot.is_some() {
                            host.mark_ready(0, true);
                        } else {
                            host.mark_error(
                                "E2105".to_string(),
                                format!("Canvas kept the last-good program; `{file}` is not ready"),
                                true,
                            );
                        }
                    }
                    if static_host.is_none() && prev_snapshot.is_some() {
                        print_canvas_hint(file, mode, &mut canvas_hint_printed);
                    }
                }
                DevSessionAction::GamePlay
                | DevSessionAction::GameSimulate
                | DevSessionAction::GamePause
                | DevSessionAction::GameResume
                | DevSessionAction::GameStep
                | DevSessionAction::GameFrameAdvance
                | DevSessionAction::GameEdit
                | DevSessionAction::GameSelectCategory
                | DevSessionAction::GameInspect
                | DevSessionAction::GameEvaluate
                | DevSessionAction::GameEject
                | DevSessionAction::GameKeep
                | DevSessionAction::GameDiscard => {
                    match dispatch_terminal_game_control(&resident_session, file, action) {
                        Ok(dispatched) => {
                            if !mode.quiet {
                                println!("GAME control dispatched={dispatched}");
                            }
                        }
                        Err(error) => {
                            if !mode.quiet {
                                eprintln!("GAME control rejected: {error}");
                            }
                        }
                    }
                    refresh_terminal_host(
                        &mut terminal_host,
                        &resident_session,
                        &mut terminal_frame,
                    );
                }
                DevSessionAction::Tests => {
                    failed_claims = run_dev_tests(file, profile, setting_overrides, &[]);
                }
                DevSessionAction::FailedClaimsOnly => {
                    if failed_claims.is_empty() {
                        if !mode.quiet {
                            println!("no failed claims recorded");
                        }
                    } else {
                        let previous = failed_claims.clone();
                        failed_claims = run_dev_tests(file, profile, setting_overrides, &previous);
                    }
                }
                DevSessionAction::Quit => {
                    if let Some(capture) = record.as_ref() {
                        let _ = finish_dev_capture(capture, ExitCodes::OK, mode);
                    }
                    exit(ExitCodes::OK);
                }
            }
        }
        if let Some(snapshot) = &prev_snapshot {
            run_due_jobs(
                snapshot,
                file,
                try_anyway,
                use_interpreter,
                mode,
                &release_policy,
                &mut clock,
                &resident_session,
            );
            sync_persist_bindings(&snapshot.bundle, &mut persist);
        }
        if watch_woke {
            if let Some(receipt) = watch.poll() {
            if receipt.change_kinds.iter().all(|k| *k == "stale") {
                continue;
            }
            let _source_transaction = canvas_host
                .as_ref()
                .map(|host| host.lock_source_transaction());
            resident_session.mark_building();
            if let Some(host) = canvas_host.as_ref().or(static_host.as_ref()) {
                host.mark_building();
            }
            let mut game_facts: Vec<jet_foundation::Game::JetGameChangeFact> = Vec::new();
            let persist_before_change = jet_foundation::Persist::shared_clone();
            let next = render_dev_change(
                file,
                entry_fn,
                program_args,
                try_anyway,
                policy,
                prev_snapshot.as_ref(),
                gates,
                mode,
                use_interpreter,
                profile,
                &release_policy,
                setting_overrides,
                &mut incremental_cache,

                receipt.game_dev_entries(),
                &mut game_facts,
                canvas_host.as_ref().or(static_host.as_ref()),
            );
            if next.is_none() {
                jet_foundation::Persist::shared_replace(persist_before_change.clone());
                jet_jit::discard_hot_swap_plan();
            }
            if let Err(reason) =
                publish_game_asset_watch_events(file, receipt.game_dev_entries(), dev_job_now_ms())
            {
                if !mode.json {
                    eprintln!("[game-assets] asset watch report rejected: {reason}");
                }
            }
            // Canonical transaction: pause → preflight → stage → commit.
            // Shared persistence is restored on every rejected path.
            let mut txn = jet_devserver::HotReplaceTxn::begin(session.clone());
            let transaction_result = match &next {
                Some(snapshot) => txn.pause().and_then(|_| {
                    sync_persist_bindings(&snapshot.bundle, &mut persist);
                    let (source_id, build_id, revision, world_id) = live_lineage_for_file(file);
                    let candidate = session
                        .clone()
                        .with_persist(persist.clone())
                        .with_lineage(source_id, build_id, revision, world_id);
                    txn.preflight(&candidate).and_then(|_| txn.stage())
                }),
                None => Err("reload failed; prior session kept".to_string()),
            };
            match transaction_result {
                Ok(()) => {
                    // Keep the typed retention plan until commit succeeds. A
                    // rejected transaction must not publish a new live
                    // application/session state.
                    let decisions = txn.decisions().to_vec();
                    match txn.commit() {
                        Ok(snap) => {
                            session = snap;
                            if let Err(reason) = apply_game_watch_facts(
                                &mut game_dev_session,
                                &game_facts,
                                true,
                                "hot reload committed",
                                dev_job_now_ms(),
                                file,
                            ) {
                                resident_session.mark_error("E2105", &reason);
                                eprintln!("[hot-replace] game swap publication rejected: {reason}");
                            }
                            if let Err(reason) =
                                resident_session.record_live_transaction(&session, &decisions)
                            {
                                resident_session.mark_error("E2105", &reason);
                                eprintln!("[hot-replace] live state record rejected: {reason}");
                            } else {
                                resident_session.mark_ready();
                            }
                            // The running app owns the typed UI tree. A
                            // successful swap asks the existing terminal host
                            // for its ordinary projection; it never rebuilds
                            // host/session state or parses a style asset.
                            refresh_terminal_host(
                                &mut terminal_host,
                                &resident_session,
                                &mut terminal_frame,
                            );

                            if canvas_host.is_none() && static_host.is_none() {
                                static_host = start_static_output_host(
                                    file,
                                    &release_policy,
                                    &mut pending_application_listener,
                                    Arc::clone(&resident_session),
                                );
                            }
                            if let Some(host) = canvas_host.as_ref().or(static_host.as_ref()) {
                                host.mark_ready(0, true);
                            }
                            if let Some(host) = static_host.as_ref() {
                                let (source_id, build_id, revision, world_id) =
                                    live_lineage_for_file(file);
                                let _ = host
                                    .resident_session()
                                    .set_live_lineage(&source_id, &build_id, &revision, &world_id);
                            }
                            if static_host.is_none() {
                                print_canvas_hint(file, mode, &mut canvas_hint_printed);
                            }
                            prev_snapshot = next;
                        }
                        Err((prior, reason)) => {
                            jet_foundation::Persist::shared_replace(persist_before_change.clone());
                            jet_jit::discard_hot_swap_plan();
                            if let Err(publication) = apply_game_watch_facts(
                                &mut game_dev_session,
                                &game_facts,
                                false,
                                &reason,
                                dev_job_now_ms(),
                                file,
                            ) {
                                eprintln!(
                                    "[hot-replace] rejected game swap publication failed: {publication}"
                                );
                            }
                            eprintln!("[hot-replace] {reason}");
                            if let Some(host) = canvas_host.as_ref().or(static_host.as_ref()) {
                                host.mark_error(
                                    "E2105".to_string(),
                                    format!(
                                        "Canvas kept the last-good program after `{file}` failed to commit"
                                    ),
                                    true,
                                );
                            }
                            session = prior;
                        }
                    }
                }
                Err(reason) => {
                    jet_foundation::Persist::shared_replace(persist_before_change);
                    jet_jit::discard_hot_swap_plan();
                    if let Err(publication) = apply_game_watch_facts(
                        &mut game_dev_session,
                        &game_facts,
                        false,
                        &reason,
                        dev_job_now_ms(),
                        file,
                    ) {
                        eprintln!(
                            "[hot-replace] rejected game swap publication failed: {publication}"
                        );
                    }
                    eprintln!("[hot-replace] {reason}");
                    if let Some(host) = canvas_host.as_ref().or(static_host.as_ref()) {
                        host.mark_error(
                            "E2105".to_string(),
                            format!(
                                "Canvas kept the last-good program after `{file}` failed to reload"
                            ),
                            true,
                        );
                    }
                    session = txn.rollback(reason);
                }
            }
            if let Err(diagnostic) = watch.acknowledge(&receipt) {
                eprint!(
                    "{}",
                    jet::render_all_colored(file, "", &[diagnostic], mode.color_stderr())
                );
                exit(ExitCodes::USER_ERROR);
            }
            if let Some(ms) = receipt.edit_to_visible_ms {
                if !jet_devserver::within_budget(&receipt) {
                    eprintln!(
                        "[watch] edit-to-visible {ms}ms exceeded budget {}ms",
                        jet_devserver::EDIT_TO_VISIBLE_BUDGET_MS
                    );
                }
            }
            }
        }
        let project_session = static_host
            .as_ref()
            .map(|host| host.resident_session())
            .unwrap_or_else(|| resident_session.clone());
        match project_session.take_project_rebuild() {
            Ok(Some(request)) => {
                let result = execute_native_project_rebuild(
                    file,
                    entry_fn,
                    program_args,
                    try_anyway,
                    gates,
                    mode,
                    use_interpreter,
                    profile,
                    setting_overrides,
                    &release_policy,
                    &resident_session,
                    canvas_host.as_ref(),
                    &mut static_host,
                    &mut pending_application_listener,
                    &mut prev_snapshot,
                    &mut game_controls_enabled,
                    &mut canvas_hint_printed,
                );
                incremental_cache.clear();
                if dev_incremental_reload_enabled(entry_fn, profile, gates, setting_overrides) {
                    if prev_snapshot.is_some() {
                        prime_dev_incremental_cache(file, &mut incremental_cache);
                    }
                }
                if let Err(error) = project_session.finish_project_rebuild(&request, result) {
                    if !mode.quiet {
                        eprintln!("Project rebuild receipt: {error}");
                    }
                }
            }
            Ok(None) => {}
            Err(error) => {
                if !mode.quiet {
                    eprintln!("Project rebuild: {error}");
                }
            }
        }
        refresh_terminal_host(&mut terminal_host, &resident_session, &mut terminal_frame);
    }
}

/// D-PERSIST1: refresh `#Persist` bindings from the loaded bundle into the
/// shared runtime-heap persist store (typed migration on shape change).
fn sync_persist_bindings(
    bundle: &jet::AST::ProgramBundle,
    store: &mut jet_devserver::PersistStore,
) {
    let prep = jet_foundation::Persist::prepare_bundle(bundle);
    for msg in &prep.messages {
        eprintln!("{msg}");
    }
    if let Some(error) = prep.error {
        eprintln!("[persist] transaction rejected: {error}");
    }
    *store = jet_foundation::Persist::shared_clone();
}
fn live_lineage_for_file(file: &str) -> (String, String, String, String) {
    let source_id = file.to_string();
    let revision = fs::read(file)
        .map(|bytes| format!("sha256-{}", jet::SHA256::sha256_hex(&bytes)))
        .unwrap_or_default();
    let build_id = revision.clone();
    let world_id = format!("world-{}", jet::SHA256::sha256_hex(source_id.as_bytes()));
    (source_id, build_id, revision, world_id)
}
fn provision_live_lineage_env(
    local_rail: bool,
    session_id: &str,
    source_id: &str,
    build_id: &str,
    revision: &str,
    world_id: &str,
) {
    const SESSION: &str = "JET_DEVTOOLS_RELAY_SESSION_ID";
    const SOURCE: &str = "JET_DEVTOOLS_RELAY_SOURCE_ID";
    const BUILD: &str = "JET_DEVTOOLS_RELAY_BUILD_ID";
    const REVISION: &str = "JET_DEVTOOLS_RELAY_REVISION";
    const WORLD: &str = "JET_DEVTOOLS_RELAY_WORLD_ID";
    const COMMANDS: &str = "JET_DEVTOOLS_COMMAND_RELAY_PATH";
    let command_directory =
        std::env::temp_dir().join(format!("jet-devtools-commands-{}", std::process::id()));
    let command_directory = command_directory.to_string_lossy().into_owned();
    if !local_rail {
        for key in [SESSION, SOURCE, BUILD, REVISION, WORLD, COMMANDS] {
            std::env::remove_var(key);
        }
        return;
    }
    for (key, value) in [
        (SESSION, session_id),
        (SOURCE, source_id),
        (BUILD, build_id),
        (REVISION, revision),
        (WORLD, world_id),
        (COMMANDS, command_directory.as_str()),
    ] {
        std::env::set_var(key, value);
    }
}

/// Return the next timer wake needed by scheduled dev jobs. File changes
/// continue to wake through the native watcher; a timer is only needed when a
/// checked `#Every(…)` schedule is present.
fn next_dev_job_wake(snapshot: Option<&jet::CheckedMirSnapshot>) -> Option<Duration> {
    let snapshot = snapshot?;
    let jobs = jet::Interpreter::scheduled_jobs(&snapshot.bundle);
    if jobs.is_empty() {
        return None;
    }
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let second_of_day = now_secs % 86_400;
    jobs.into_iter()
        .map(|(_, schedule)| match schedule {
            jet::AST::EverySchedule::Duration { nanos } => {
                Duration::from_nanos(nanos.max(1) as u64)
            }
            jet::AST::EverySchedule::WallClockTime { hour, minute } => {
                let target = hour as u64 * 3_600 + minute as u64 * 60;
                let seconds = if second_of_day >= target && second_of_day < target + 60 {
                    1
                } else if second_of_day < target {
                    target - second_of_day
                } else {
                    86_400 - second_of_day + target
                };
                Duration::from_secs(seconds.max(1))
            }
        })
        .min()
}

/// D-SCHEDULE1 (ratified 2026-07-11, card #505): the `jet dev` consumer of
/// schedule-as-code — check every `#Job #Every(…)` fn in `snapshot.bundle`
/// against `clock`, and run whichever are due through the same interpreter tier
/// the rest of the dev loop uses (`jet::Interpreter::run_named_job`). This is
/// the dev-loop tier only (D-DEV3); the service runtime (D-SERVICE1) and a
/// jetos timer projection are the production/OS consumers of the identical
/// `#Every(…)` declaration — see the D-SCHEDULE1 row in
/// docs/spec/syntax-decisions.md for the full three-consumer law.
fn run_due_jobs(
    snapshot: &jet::CheckedMirSnapshot,
    file: &str,
    try_anyway: bool,
    use_interpreter: bool,
    mode: OutputMode,
    release_policy: &jet::Package::ReleaseDevtoolsPolicy,
    clock: &mut JobClock,
    resident_session: &jet_devserver::ResidentDevSession,
) {
    let jobs = jet::Interpreter::scheduled_jobs(&snapshot.bundle);
    if jobs.is_empty() {
        return;
    }
    for name in clock.due(&jobs) {
        if !mode.quiet {
            println!("\n— due job `{}` —", name);
        }
        let invocation = if use_interpreter {
            jet::Interpreter::InterpreterInvocation::DevInterpret
        } else {
            jet::Interpreter::InterpreterInvocation::DevDefault
        };
        let job_id = format!("{name}#{}", NEXT_DEV_JOB_ID.fetch_add(1, Ordering::Relaxed));
        let queue = name.clone();
        let worker = if use_interpreter {
            "dev-interpreter"
        } else {
            "dev-cranelift"
        }
        .to_string();
        let enqueued_at_ms = dev_job_now_ms();
        publish_dev_job_fact(
            resident_session,
            dev_job_fact(
                jet_devserver::Devtools::JetDevtoolsJobPanelEventKind::Enqueue,
                jet_devserver::Devtools::JetDevtoolsJobPanelLifecycleState::Enqueued,
                &job_id,
                &name,
                &queue,
                0,
                None,
                None,
                None,
                enqueued_at_ms,
            ),
        );
        let started_at = Instant::now();
        let started_at_ms = dev_job_now_ms().max(enqueued_at_ms);
        publish_dev_job_fact(
            resident_session,
            dev_job_fact(
                jet_devserver::Devtools::JetDevtoolsJobPanelEventKind::Start,
                jet_devserver::Devtools::JetDevtoolsJobPanelLifecycleState::Started,
                &job_id,
                &name,
                &queue,
                1,
                None,
                Some(worker.clone()),
                None,
                started_at_ms,
            ),
        );
        match jet::Interpreter::run_checked_job_snapshot(
            snapshot,
            file,
            &name,
            &[],
            try_anyway,
            invocation,
            release_policy,
        ) {
            jet::Interpreter::RunOutcome::Ran {
                stdout,
                stderr,
                exit_code,
            } => {
                let finished_at_ms = dev_job_now_ms().max(started_at_ms);
                let duration_ms = started_at
                    .elapsed()
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX);
                if exit_code == 0 {
                    publish_dev_job_fact(
                        resident_session,
                        dev_job_fact(
                            jet_devserver::Devtools::JetDevtoolsJobPanelEventKind::Complete,
                            jet_devserver::Devtools::JetDevtoolsJobPanelLifecycleState::Completed,
                            &job_id,
                            &name,
                            &queue,
                            1,
                            Some(duration_ms),
                            Some(worker),
                            None,
                            finished_at_ms,
                        ),
                    );
                } else {
                    publish_dev_job_fact(
                        resident_session,
                        dev_job_fact(
                            jet_devserver::Devtools::JetDevtoolsJobPanelEventKind::Fail,
                            jet_devserver::Devtools::JetDevtoolsJobPanelLifecycleState::Failed,
                            &job_id,
                            &name,
                            &queue,
                            1,
                            Some(duration_ms),
                            Some(worker),
                            Some(jet_devserver::Devtools::JetDevtoolsJobPanelFailureKind::Error),
                            finished_at_ms,
                        ),
                    );
                }
                emit_run_output(&stdout, &stderr);
                if exit_code != 0 {
                    eprintln!("job `{name}` exited with code {exit_code}");
                }
            }
            jet::Interpreter::RunOutcome::Problems(diags) => {
                let finished_at_ms = dev_job_now_ms().max(started_at_ms);
                let duration_ms = started_at
                    .elapsed()
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX);
                publish_dev_job_fact(
                    resident_session,
                    dev_job_fact(
                        jet_devserver::Devtools::JetDevtoolsJobPanelEventKind::Fail,
                        jet_devserver::Devtools::JetDevtoolsJobPanelLifecycleState::Failed,
                        &job_id,
                        &name,
                        &queue,
                        1,
                        Some(duration_ms),
                        Some(worker),
                        Some(jet_devserver::Devtools::JetDevtoolsJobPanelFailureKind::Error),
                        finished_at_ms,
                    ),
                );
                exit_if_internal_fault(&diags);
                let src = fs::read_to_string(file).unwrap_or_default();
                report_problems(mode, file, &src, &diags);
            }
        }
    }
}

static NEXT_DEV_JOB_ID: AtomicU64 = AtomicU64::new(1);
const DEV_JOB_FACT_SOURCE: &str = "jet-dev.jobs";

fn dev_job_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn dev_job_fact(
    kind: jet_devserver::Devtools::JetDevtoolsJobPanelEventKind,
    state: jet_devserver::Devtools::JetDevtoolsJobPanelLifecycleState,
    job_id: &str,
    name: &str,
    queue: &str,
    attempts: u32,
    duration_ms: Option<u64>,
    worker: Option<String>,
    failure: Option<jet_devserver::Devtools::JetDevtoolsJobPanelFailureKind>,
    timestamp_ms: u64,
) -> jet_devserver::Devtools::JetDevtoolsJobPanelFact {
    jet_devserver::Devtools::JetDevtoolsJobPanelFact {
        sequence: 0,
        timestamp_ms,
        kind,
        state,
        job_id: job_id.to_string(),
        name: name.to_string(),
        labels: Vec::new(),
        queue: queue.to_string(),
        attempts,
        duration_ms,
        worker,
        request_id: None,
        failure,
    }
}

fn publish_dev_job_fact(
    resident_session: &jet_devserver::ResidentDevSession,
    fact: jet_devserver::Devtools::JetDevtoolsJobPanelFact,
) {
    let _ = resident_session.publish_job_fact(&fact, DEV_JOB_FACT_SOURCE);
}

/// D-SCHEDULE1: per-job last-run bookkeeping for the due-job tick. A
/// `Duration` schedule tracks the `Instant` it last ran; a `WallClockTime`
/// schedule tracks the UTC day index (days since the Unix epoch) it last
/// ran, so it fires once inside its matching minute, not on every 120ms
/// tick within that minute. UTC only — D-SCHEDULE1's own law text carves
/// timezone-aware calendars out to "the runtime API or jetos timers"; this
/// is the lightweight dev-loop convenience tier, not that.
pub(crate) struct JobClock {
    inner: jet_jit::Job::JetJobClock,
}

impl JobClock {
    pub(crate) fn new() -> Self {
        JobClock {
            inner: jet_jit::Job::JetJobClock::new(),
        }
    }

    /// Which job names are due right now — records the firing so the same
    /// job doesn't fire again on the very next tick.
    pub(crate) fn due(&mut self, jobs: &[(String, jet::AST::EverySchedule)]) -> Vec<String> {
        let schedules = jobs
            .iter()
            .map(|(name, schedule)| (name.as_str(), prelude_schedule(*schedule)))
            .collect::<Vec<_>>();
        jet_jit::Job::jet_job_schedule_due(&mut self.inner, &schedules)
    }

    /// The testable core of `due`: `unix_secs` is injected so the day/window
    /// arithmetic can be checked without racing real wall-clock time.
    #[allow(dead_code)]
    fn due_at(
        &mut self,
        jobs: &[(String, jet::AST::EverySchedule)],
        unix_secs: u64,
    ) -> Vec<String> {
        let schedules = jobs
            .iter()
            .map(|(name, schedule)| (name.as_str(), prelude_schedule(*schedule)))
            .collect::<Vec<_>>();
        self.inner.due_at(&schedules, unix_secs)
    }
}

fn prelude_schedule(schedule: jet::AST::EverySchedule) -> jet_jit::Job::JetJobSchedule {
    match schedule {
        jet::AST::EverySchedule::Duration { nanos } => {
            jet_jit::Job::JetJobSchedule::Duration { nanos }
        }
        jet::AST::EverySchedule::WallClockTime { hour, minute } => {
            jet_jit::Job::JetJobSchedule::WallClockTime { hour, minute }
        }
    }
}
fn dev_artifact_request(
    bundle: &jet::AST::ProgramBundle,
    target: jet_foundation::MIR::MirArtifactTarget,
    profile: &str,
) -> jet_foundation::MIR::MirArtifactRequest {
    let build_mode =
        jet::Driver::mir_artifact_build_mode_for(bundle, jet::Sema::CompileMode::Run, profile);
    let kind = match build_mode {
        jet_foundation::MIR::MirArtifactBuildMode::Test
        | jet_foundation::MIR::MirArtifactBuildMode::Coverage => {
            jet_foundation::MIR::MirArtifactKind::TestExecutable
        }
        jet_foundation::MIR::MirArtifactBuildMode::Fuzz => {
            jet_foundation::MIR::MirArtifactKind::FuzzExecutable
        }
        jet_foundation::MIR::MirArtifactBuildMode::Dev
        | jet_foundation::MIR::MirArtifactBuildMode::Release => {
            jet_foundation::MIR::MirArtifactKind::NativeExecutable
        }
    };
    jet_foundation::MIR::MirArtifactRequest::new(target, kind, build_mode)
}

fn dev_incremental_reload_enabled(
    entry_fn: Option<&str>,
    profile: &str,
    gates: jet::Policy::GateSet,
    setting_overrides: &BTreeMap<String, String>,
) -> bool {
    entry_fn.is_none() && profile == "dev" && gates.is_empty() && setting_overrides.is_empty()
}

fn prime_dev_incremental_cache(
    file: &str,
    cache: &mut jet::Sema::IncrementalSemaCache,
) {
    let (diagnostics, _, _) =
        jet::Driver::check_file_with_effect_facts_incremental(file, None, false, cache);
    if diagnostics
        .iter()
        .any(|diagnostic| matches!(diagnostic.severity, jet::Diagnostics::Severity::Error))
    {
        cache.clear();
    }
}

fn load_and_check_dev_change(
    file: &str,
    entry_fn: Option<&str>,
    gates: jet::Policy::GateSet,
    mode: OutputMode,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Option<(jet::AST::ProgramBundle, jet::Sema::SemIndexEffectFacts)> {
    let mut new_bundle = match jet::Loader::load_entry(file) {
        Ok(mut b) => {
            if let Err(diags) =
                jet::Driver::seed_build_facts(&mut b, profile, false, setting_overrides)
            {
                let src = fs::read_to_string(file).unwrap_or_default();
                println!("\n— {} changed —", file);
                report_problems(mode, file, &src, &diags);
                return None;
            }
            b
        }
        Err(diags) => {
            let src = fs::read_to_string(file).unwrap_or_default();
            println!("\n— {} changed —", file);
            report_problems(mode, file, &src, &diags);
            return None;
        }
    };

    if let Some(entry_fn) = entry_fn {
        jet::Driver::swap_entry_point(&mut new_bundle, entry_fn);
    }

    let (diags, effect_facts) = jet::Sema::check_bundle_gates_with_effect_facts(
        &mut new_bundle,
        jet::Sema::CompileMode::Run,
        gates,
    );
    let errs: Vec<_> = diags
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .cloned()
        .collect();
    if !errs.is_empty() {
        let src = fs::read_to_string(file).unwrap_or_default();
        println!("\n— {} changed —", file);
        report_problems(mode, file, &src, &errs);
        return None;
    }
    render_dev_lints(file, mode, &diags);
    Some((new_bundle, effect_facts))
}

/// Handle one detected file change: pick swap vs rerun vs restart and render.
/// Returns the freshly checked snapshot (or `None` if it failed to load) for
/// the next watch iteration.
fn render_dev_change(
    file: &str,
    entry_fn: Option<&str>,
    program_args: &[&String],
    try_anyway: bool,
    policy: WatchPolicy,
    prev: Option<&jet::CheckedMirSnapshot>,
    gates: jet::Policy::GateSet,
    mode: OutputMode,
    use_interpreter: bool,
    profile: &str,
    release_policy: &jet::Package::ReleaseDevtoolsPolicy,
    setting_overrides: &BTreeMap<String, String>,
    incremental_cache: &mut jet::Sema::IncrementalSemaCache,
    game_entries: &[jet_devserver::DevWatchEntry],
    game_facts_out: &mut Vec<jet_foundation::Game::JetGameChangeFact>,
    devtools_host: Option<&jet_devserver::WebHost::WebHost>,
) -> Option<jet::CheckedMirSnapshot> {
    if !game_entries.is_empty() {
        match game_change_facts(game_entries, None) {
            Ok(facts) => *game_facts_out = facts,
            Err(reason) => {
                eprintln!("[hot-swap] checked game facts rejected: {reason}");
                return None;
            }
        }
    }
    let mut incremental_decision = None;
    let incrementally_checked = if prev.is_some()
        && dev_incremental_reload_enabled(entry_fn, profile, gates, setting_overrides)
    {
        let old = prev.expect("incremental reload requires a baseline");
        let module_name = old
            .bundle
            .modules
            .get(old.bundle.entry)
            .map(|module| module.display.clone())
            .unwrap_or_else(|| file.to_string());
        let source = fs::read_to_string(file).unwrap_or_default();
        let (diags, bundle, facts, decision) = jet::Driver::check_file_with_hot_swap_incremental(
            &old.bundle,
            file,
            Some((Path::new(file), source.as_str())),
            false,
            &module_name,
            incremental_cache,
        );
        let errs: Vec<_> = diags
            .iter()
            .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
            .cloned()
            .collect();
        if !errs.is_empty() {
            println!("\n— {} changed —", file);
            report_problems(mode, file, &source, &errs);
            return None;
        }
        let safe_for_run = bundle.as_ref().is_some_and(|bundle| {
            let has_run = bundle
                .modules
                .get(bundle.entry)
                .is_some_and(|module| {
                    module.items.iter().any(|item| {
                        matches!(item, jet::AST::Item::Func(function) if function.name == "run")
                    })
                });
            let has_build = bundle.modules.iter().any(|module| {
                module.items.iter().any(|item| {
                    matches!(
                        item,
                        jet::AST::Item::Func(function) if jet::Sema::is_build_entry(function)
                    )
                })
            });
            has_run && !has_build
        });
        if safe_for_run {
            render_dev_lints(file, mode, &diags);
            incremental_decision = decision;
            bundle.map(|bundle| (bundle, facts))
        } else {
            incremental_cache.clear();
            None
        }
    } else {
        None
    };
    let (new_bundle, effect_facts) = match incrementally_checked {
        Some(checked) => checked,
        None => load_and_check_dev_change(
            file,
            entry_fn,
            gates,
            mode,
            profile,
            setting_overrides,
        )?,
    };

    // Decide whether this save uses the swap path for the selected callable.
    let resident = match policy {
        WatchPolicy::Swap => true,
        WatchPolicy::Restart => false,
        WatchPolicy::Once => false, // unreachable here (handled in run_dev)
        WatchPolicy::Auto => true,
    };

    let artifact_target = if use_interpreter {
        jet_foundation::MIR::MirArtifactTarget::Interpreter
    } else {
        jet_foundation::MIR::MirArtifactTarget::Cranelift
    };
    let (mir, artifact) = jet::lower_checked_semantic_mir_program_for(
        &new_bundle,
        dev_artifact_request(&new_bundle, artifact_target, profile),
    );

    if resident {
        // The hot-reload unit is the entry module (D-HOTSWAP1).
        let module_name = new_bundle
            .modules
            .get(new_bundle.entry)
            .map(|m| m.display.clone())
            .unwrap_or_else(|| file.to_string());
        match prev {
            Some(old) => {
                let decision_result = match incremental_decision.take() {
                    Some(decision) => Ok(decision),
                    None => jet::Sema::HotSwap::type_stable_decision(
                        &old.bundle,
                        &new_bundle,
                        &module_name,
                    ),
                };
                match decision_result {
                    Ok(decision) => {
                        let facts = match game_change_facts(game_entries, Some(&decision)) {
                            Ok(facts) => facts,
                            Err(reason) => {
                                eprintln!("[hot-swap] checked game facts rejected: {reason}");
                                return None;
                            }
                        };
                        *game_facts_out = facts.clone();
                        let decision = decision.with_change_facts(facts);
                        if let Some(host) = devtools_host {
                            let _ = host.publish_hot_swap_decision(&decision);
                        }
                        let persist_before_migration = (!use_interpreter
                            && !decision.schema_migrations().is_empty())
                        .then(jet_foundation::Persist::shared_clone);
                        let migrated_in_place = if decision.is_compatible() {
                            if !use_interpreter {
                                if let Err(reason) = jet_jit::apply_hot_swap(&decision) {
                                    if !mode.json {
                                        eprintln!(
                                            "[hot-swap] {} — adapter preparation failed: {}",
                                            module_name, reason
                                        );
                                    }
                                    return None;
                                }
                            }
                            false
                        } else if !use_interpreter && !decision.schema_migrations().is_empty() {
                            match jet_jit::apply_hot_swap_with_program(
                                &mir,
                                artifact,
                                &decision,
                                release_policy,
                            ) {
                                Ok(receipts) => {
                                    if !receipts.is_empty() && !mode.quiet && !mode.json {
                                        let migrated = receipts
                                            .iter()
                                            .filter(|receipt| !receipt.migrated.is_empty())
                                            .count();
                                        println!(
                                            "[hot-swap] migrated {} published-schema value(s)",
                                            migrated
                                        );
                                    }
                                    true
                                }
                                Err(reason) => {
                                    if !mode.json {
                                        eprintln!(
                                            "[hot-swap] {} — schema migration rejected: {}",
                                            module_name, reason
                                        );
                                    }
                                    return None;
                                }
                            }
                        } else {
                            false
                        };
                        let swap_ok = if decision.is_compatible() || migrated_in_place {
                            run_resident_swap(
                                &mir,
                                artifact,
                                try_anyway,
                                &module_name,
                                file,
                                mode,
                                use_interpreter,
                                release_policy,
                            )
                        } else {
                            run_resident_restart(
                                &mir,
                                artifact,
                                try_anyway,
                                file,
                                mode,
                                use_interpreter,
                                release_policy,
                            )
                        };
                        if !swap_ok {
                            if let Some(store) = persist_before_migration {
                                jet_foundation::Persist::shared_replace(store);
                                jet_jit::discard_hot_swap_plan();
                            }
                            return None;
                        }
                        render_hot_swap_decision(&decision, mode, migrated_in_place);
                    }
                    Err(diags) => {
                        let src = fs::read_to_string(file).unwrap_or_default();
                        report_problems(mode, file, &src, &diags);
                        return None;
                    }
                }
            }
            None => {
                // No baseline yet (first run after an error): a clean restart.
                if !mode.quiet && !mode.json {
                    println!("\n[restart] {} — first run", module_name);
                }
                if !run_resident_restart(
                    &mir,
                    artifact,
                    try_anyway,
                    file,
                    mode,
                    use_interpreter,
                    release_policy,
                ) {
                    return None;
                }
            }
        }
    } else {
        // Run-to-completion (default / `--restart`): run the already-checked
        // optimized MIR instead of checking and lowering the edit a second
        // time.
        if !mode.quiet {
            println!("\n— {} changed, re-running —", file);
        }
        let jobs = jet::Interpreter::scheduled_jobs(&new_bundle);
        let requested = program_args
            .first()
            .map(|arg| arg.as_str())
            .filter(|arg| !arg.starts_with('-'));
        let selected = requested.filter(|name| jobs.iter().any(|(job, _)| job.as_str() == *name));
        let runtime_args = if selected.is_some() {
            &program_args[1..]
        } else {
            program_args
        };
        let mut args = Vec::with_capacity(runtime_args.len() + 1);
        args.push(selected.map_or_else(|| file.to_string(), |name| format!("{file} {name}")));
        args.extend(runtime_args.iter().map(|arg| (*arg).to_string()));
        let outcome = jet_jit::with_program_args(&args, || {
            jet::Interpreter::dev_run_snapshot(
                &mir,
                artifact,
                try_anyway,
                if use_interpreter {
                    jet::Interpreter::InterpreterInvocation::DevInterpret
                } else {
                    jet::Interpreter::InterpreterInvocation::DevDefault
                },
                release_policy,
            )
        });
        render_dev_outcome(&outcome, file, mode);
        if !matches!(outcome, jet::Interpreter::RunOutcome::Ran { .. }) {
            return None;
        }
    }

    Some(jet::CheckedMirSnapshot {
        bundle: new_bundle,
        facts: effect_facts,
        mir,
        artifact,
    })
}

fn game_change_facts(
    entries: &[jet_devserver::DevWatchEntry],
    decision: Option<&jet_foundation::HotSwap::HotSwapDecision>,
) -> Result<Vec<jet_foundation::Game::JetGameChangeFact>, String> {
    entries
        .iter()
        .map(|entry| {
            let (migration, reason) = match entry.game_kind {
                jet_foundation::Game::JetGameChangeKind::Asset => (
                    jet_foundation::Game::JetGameMigrationDecision::Preserve,
                    format!("checked asset watch change `{}`", entry.change_kind),
                ),
                jet_foundation::Game::JetGameChangeKind::Script
                | jet_foundation::Game::JetGameChangeKind::World => {
                    if decision.map_or(false, |decision| decision.is_compatible()) {
                        (
                            jet_foundation::Game::JetGameMigrationDecision::Preserve,
                            "checked type-stable reload".to_string(),
                        )
                    } else {
                        (
                            jet_foundation::Game::JetGameMigrationDecision::Reject,
                            decision
                                .and_then(|decision| decision.compatibility.reason())
                                .unwrap_or("checked restart or incompatible reload")
                                .to_string(),
                        )
                    }
                }
            };
            jet_foundation::Game::JetGameChangeFact::new(
                entry.path.display().to_string(),
                entry.game_kind,
                entry.old_schema_id.clone(),
                entry.new_schema_id.clone(),
                migration,
                reason,
            )
        })
        .collect()
}

fn publish_game_asset_watch_events(
    file: &str,
    entries: &[jet_devserver::DevWatchEntry],
    timestamp_ms: u64,
) -> Result<(), String> {
    let asset_entries = entries
        .iter()
        .filter(|entry| entry.game_kind == jet_foundation::Game::JetGameChangeKind::Asset)
        .collect::<Vec<_>>();
    if asset_entries.is_empty() {
        return Ok(());
    }
    let asset_root = fs::canonicalize(
        Path::new(file)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("assets"),
    )
    .unwrap_or_else(|_| {
        let parent = Path::new(file).parent().unwrap_or_else(|| Path::new("."));
        if parent.is_absolute() {
            parent.join("assets")
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(parent)
                .join("assets")
        }
    });
    let root = jet::Codegen::MIREval::game_dev_protocol::JetGameAssetRootIdentity::new("game", ".")
        .map_err(|error| error.to_string())?;
    let mut events = Vec::with_capacity(asset_entries.len());
    let mut skipped_count = 0usize;
    for entry in asset_entries {
        if entry.change_kind == "stale" {
            skipped_count = skipped_count.saturating_add(1);
            continue;
        }
        let relative = entry
            .path
            .strip_prefix(&asset_root)
            .map_err(|_| {
                format!(
                    "asset watcher path `{}` is outside root `{}`",
                    entry.path.display(),
                    asset_root.display()
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        if relative.is_empty() {
            return Err(format!(
                "asset watcher path `{}` is the asset root",
                entry.path.display()
            ));
        }
        let logical_path = format!("assets/{relative}");
        let kind = match entry.change_kind {
            "created" => {
                jet::Codegen::MIREval::game_dev_protocol::JetGameAssetWatchEventKind::Created
            }
            "deleted" => {
                jet::Codegen::MIREval::game_dev_protocol::JetGameAssetWatchEventKind::Deleted
            }
            _ => jet::Codegen::MIREval::game_dev_protocol::JetGameAssetWatchEventKind::Changed,
        };
        events.push(
            jet::Codegen::MIREval::game_dev_protocol::JetGameAssetWatchEvent::new(
                root.clone(),
                logical_path,
                kind,
            )
            .map_err(|error| error.to_string())?,
        );
    }
    let roots = jet::Codegen::MIREval::game_dev_protocol::JetGameAssetRootSet::new(vec![root])
        .map_err(|error| error.to_string())?;
    let projected = jet::Codegen::MIREval::game_dev_protocol::project_asset_watch_import_events(
        timestamp_ms,
        file,
        roots,
        &events,
        skipped_count,
    )?;
    for event in projected {
        jet_foundation::Devtools::jet_devtools_publish_event(event);
    }
    Ok(())
}

fn apply_game_watch_facts(
    session: &mut jet_foundation::Game::JetGameDevSession,
    facts: &[jet_foundation::Game::JetGameChangeFact],
    accepted: bool,
    reason: &str,
    timestamp_ms: u64,
    source: &str,
) -> Result<(), String> {
    for fact in facts {
        let fact = if accepted {
            fact.clone()
        } else {
            fact.clone().with_migration(
                jet_foundation::Game::JetGameMigrationDecision::Reject,
                reason,
            )?
        };
        let outcome = match fact.kind {
            jet_foundation::Game::JetGameChangeKind::Asset => session.apply_asset_swap(fact)?,
            jet_foundation::Game::JetGameChangeKind::Script => session.apply_script_reload(fact)?,
            jet_foundation::Game::JetGameChangeKind::World => session.apply_world_reload(fact)?,
        };
        let body = jet_foundation::Devtools::JetDevtoolsEventBody::GameSwap {
            sequence: outcome.sequence,
            path: outcome.fact.path,
            kind: outcome.fact.kind.as_str().to_string(),
            status: outcome.status.as_str().to_string(),
            old_schema_id: outcome.fact.old_schema_id,
            new_schema_id: outcome.fact.new_schema_id,
            migration: outcome.fact.migration.as_str().to_string(),
            reason: outcome.fact.reason,
        };
        let event = jet_foundation::Devtools::JetDevtoolsEvent::try_new(
            timestamp_ms,
            source.to_string(),
            body,
        )?;
        jet_foundation::Devtools::jet_devtools_publish_event(event);
    }
    Ok(())
}
fn publish_game_launch_profile(
    resident_session: &jet_devserver::ResidentDevSession,
    file: &str,
    profile: &jet::Codegen::MIREval::game_dev_protocol::GameDevLaunchProfile,
    timestamp_ms: u64,
) -> Result<(), String> {
    let body = jet_foundation::Devtools::JetDevtoolsEventBody::GameLaunchProfile {
        target: profile.target.clone(),
        run_mode: profile.run_mode.as_str().to_string(),
        headless_compatible: profile.headless_compatible,
        phases: profile
            .phases
            .iter()
            .map(|phase| {
                jet_foundation::Devtools::JetDevtoolsGameLaunchPhase::new(
                    phase.phase.as_str(),
                    phase.status.as_str(),
                    phase.detail.clone(),
                )
            })
            .collect(),
    };
    let event = jet_foundation::Devtools::JetDevtoolsEvent::try_new(timestamp_ms, "game", body)?;
    resident_session
        .publish_devtools_event_typed(event)
        .map(|_| ())
}
fn render_hot_swap_decision(
    decision: &jet_foundation::HotSwap::HotSwapDecision,
    mode: OutputMode,
    migrated_in_place: bool,
) {
    if mode.quiet || mode.json {
        return;
    }
    let list = |preserved: bool| {
        let values = decision
            .state_facts()
            .iter()
            .filter(|fact| fact.is_preserved() == preserved)
            .map(|fact| fact.key.as_str())
            .collect::<Vec<_>>();
        if values.is_empty() {
            "—".to_string()
        } else {
            values.join(", ")
        }
    };
    let changed = if decision.changed_functions.is_empty() {
        "—".to_string()
    } else {
        decision.changed_functions.join(", ")
    };
    if decision.is_compatible() {
        println!(
            "saved {}  swapped {}  kept {}",
            decision.module,
            changed,
            list(true)
        );
    } else if migrated_in_place {
        println!(
            "saved {}  migrated in place  swapped {}  kept {}",
            decision.module,
            changed,
            list(true)
        );
    } else {
        let reason = decision
            .compatibility
            .reason()
            .unwrap_or("type surface changed");
        println!(
            "saved {}  restarting  kept {}  reset {}  reason {}",
            decision.module,
            list(true),
            list(false),
            reason
        );
    }
}
/// Hot-swap via the strict Cranelift backend (`--interpret` uses tier-0).
fn run_resident_swap(
    program: &jet_foundation::MIR::MirProgram,
    artifact: jet_foundation::MIR::MirArtifactId,
    try_anyway: bool,
    module_name: &str,
    file: &str,
    mode: OutputMode,
    use_interpreter: bool,
    release_policy: &jet::Package::ReleaseDevtoolsPolicy,
) -> bool {
    use jet::JitBackend::{InterpreterBackend, JitBackend};
    use jet_jit::CraneliftBackend;

    let outcome = if use_interpreter {
        let mut b = InterpreterBackend::new(jet::Interpreter::InterpreterInvocation::DevInterpret);
        b.hot_swap(module_name, program, artifact, try_anyway, release_policy)
    } else {
        let mut b = CraneliftBackend::new();
        b.hot_swap(module_name, program, artifact, try_anyway, release_policy)
    };
    match outcome {
        Ok(o) => {
            render_outcome(o, file, mode);
            true
        }
        Err(diags) => {
            exit_if_internal_fault(&diags);
            let src = fs::read_to_string(file).unwrap_or_default();
            report_problems(mode, file, &src, &diags);
            false
        }
    }
}

/// Clean restart via the strict Cranelift backend.
fn run_resident_restart(
    program: &jet_foundation::MIR::MirProgram,
    artifact: jet_foundation::MIR::MirArtifactId,
    try_anyway: bool,
    file: &str,
    mode: OutputMode,
    use_interpreter: bool,
    release_policy: &jet::Package::ReleaseDevtoolsPolicy,
) -> bool {
    use jet::JitBackend::{InterpreterBackend, JitBackend};
    use jet_jit::CraneliftBackend;
    let outcome = if use_interpreter {
        let mut b = InterpreterBackend::new(jet::Interpreter::InterpreterInvocation::DevInterpret);
        b.restart(program, artifact, try_anyway, release_policy)
    } else {
        let mut b = CraneliftBackend::new();
        b.restart(program, artifact, try_anyway, release_policy)
    };
    let ok = matches!(&outcome, jet::Interpreter::RunOutcome::Ran { .. });
    render_outcome(outcome, file, mode);
    ok
}

/// `jet repl [<file>.jet]` — interactive REPL session (E2-M18, D-REPL3=A).
/// `project_dir` sets the base for `:load` paths and (eventually) import
/// context (D-REPL10=A sandbox; `--project <dir>` enables project mode).
///
/// #2038: a positional `<file>.jet` is a session preload — its definitions
/// become available in turn 1 through the one ratified file-into-session
/// mechanism, `:load` (D-REPL15=B); `fn run` is NOT invoked, because running a
/// program is `jet run`. A path that isn't there is refused here, before the
/// banner, exactly the way `run_dev` above refuses one (E2105). Silently
/// starting an empty session was the bug.
pub(crate) fn run_repl(
    project_dir: Option<&str>,
    preload: Option<&str>,
    allow: &[String],
    deny: &[String],
    color: ColorChoice,
) {
    let mut flags = jet::REPL::ReplFlags::new(allow, deny).with_color(color);
    if let Some(file) = preload {
        let resolved = crate::resolve_source_path(file);
        if !Path::new(&resolved).exists() {
            crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
            exit(ExitCodes::USER_ERROR);
        }
        flags = flags.with_preload(resolved);
    }
    let code = jet::REPL::run(project_dir, flags);
    exit(code);
}

/// Run one dev iteration and render its outcome to the terminal in the active
/// output mode. Diagnostics use the SAME renderer as batch compilation
/// (D-DEV), so a problem looks identical whether seen via `jet check` or
/// `jet dev`.
fn render_dev_iteration(
    file: &str,
    entry_fn: Option<&str>,
    program_args: &[&String],
    try_anyway: bool,
    gates: jet::Policy::GateSet,
    mode: OutputMode,
    use_interpreter: bool,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Option<jet::CheckedMirSnapshot> {
    let started = std::time::Instant::now();
    let mut run = run_dev_iteration_with_entry(
        file,
        entry_fn,
        program_args,
        try_anyway,
        use_interpreter,
        gates,
        profile,
        setting_overrides,
    );
    render_lints(file, mode, &run.lints);
    let snapshot = run.snapshot.take();
    let outcome = run.outcome;
    let elapsed = started.elapsed();
    let ran_ok = matches!(outcome, jet::Interpreter::RunOutcome::Ran { .. });
    render_outcome_timed(outcome, file, Some(elapsed), mode);
    if ran_ok {
        if let Some(snapshot) = snapshot {
            run_dev_budget_refresh(file, &snapshot.bundle, mode);
            return Some(snapshot);
        }
    }
    None
}

fn render_dev_outcome(outcome: &jet::Interpreter::RunOutcome, file: &str, mode: OutputMode) {
    match outcome {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, .. } => {
            emit_run_output(stdout, stderr);
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            exit_if_internal_fault(diags);
            let src = fs::read_to_string(file).unwrap_or_default();
            report_problems(mode, file, &src, diags);
        }
    }
}

fn exit_dev_outcome(outcome: jet::Interpreter::RunOutcome) {
    match outcome {
        jet::Interpreter::RunOutcome::Ran { exit_code, .. } => exit(exit_code),
        jet::Interpreter::RunOutcome::Problems(diags) => {
            exit_if_internal_fault(&diags);
            exit(ExitCodes::USER_ERROR);
        }
    }
}

pub(crate) fn render_lints(
    file: &str,
    mode: OutputMode,
    diagnostics: &[jet::Diagnostics::Diagnostic],
) {
    let lints = visible_lints(diagnostics);
    if !lints.is_empty() {
        let source = fs::read_to_string(file).unwrap_or_default();
        report_problems(mode, file, &source, &lints);
    }
}

fn render_dev_lints(file: &str, mode: OutputMode, diagnostics: &[jet::Diagnostics::Diagnostic]) {
    render_lints(file, mode, diagnostics);
}

/// After a successful dev iteration, collect ServiceProbe/SceneProbe evidence
/// for any active dev-owned budgets and trigger a report refresh.
fn run_dev_budget_refresh(file: &str, bundle: &jet::AST::ProgramBundle, mode: OutputMode) {
    let specs = match jet::Sema::collect_located_budget_specs_bundle(bundle) {
        Ok(specs) => specs,
        Err(_) => return,
    };
    let has_service = specs.iter().any(|s| {
        s.spec
            .provider
            .split_once('(')
            .map(|(k, _)| k)
            .unwrap_or(&s.spec.provider)
            == "ServiceProbe"
    });
    let has_scene = specs.iter().any(|s| {
        s.spec
            .provider
            .split_once('(')
            .map(|(k, _)| k)
            .unwrap_or(&s.spec.provider)
            == "SceneProbe"
    });
    if !has_service && !has_scene {
        return;
    }
    let root = Path::new(file)
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .and_then(|d| jet::Loader::find_manifest_root(&d).or_else(|| Some(d)))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => return,
    };
    let service_evidence = if has_service {
        collect_service_evidence(&root, &specs)
    } else {
        Vec::new()
    };
    let scene_evidence = if has_scene {
        collect_scene_evidence(file, &src, mode, &specs)
    } else {
        Vec::new()
    };
    let status = crate::CmdBudget::run_dev_refresh(file, &service_evidence, &scene_evidence);
    if status != 0 {
        eprintln!("budget: dev refresh failed with status {status}");
    }
}

/// Render a run outcome with no timing line (used for re-runs/swaps).
fn render_outcome(outcome: jet::Interpreter::RunOutcome, file: &str, mode: OutputMode) {
    render_outcome_timed(outcome, file, None, mode);
}

fn render_outcome_timed(
    outcome: jet::Interpreter::RunOutcome,
    file: &str,
    elapsed: Option<std::time::Duration>,
    mode: OutputMode,
) {
    match outcome {
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, .. } => {
            emit_run_output(&stdout, &stderr);
            if let Some(e) = elapsed {
                if !mode.quiet {
                    println!("✓ ran in {} ms", e.as_millis());
                }
            }
        }
        jet::Interpreter::RunOutcome::Problems(diags) => {
            exit_if_internal_fault(&diags);
            let src = fs::read_to_string(file).unwrap_or_default();
            report_problems(mode, file, &src, &diags);
        }
    }
}

/// `jet self completions SHELL [--for PROGRAM]` (D-DX4,
/// D-SHAPE-CLI-COMPLETE1=A). External schemas are read statically from the
/// executable; no application code runs.
pub(crate) fn run_completions(args: &[String]) {
    const USAGE: &str = "jet self completions <bash|zsh|fish|powershell> [--for PROGRAM]";
    let shell = args.first().map(String::as_str);
    if !matches!(shell, Some("bash" | "zsh" | "fish" | "powershell"))
        || !matches!(args.len(), 1 | 3)
        || (args.len() == 3 && args[1] != jet::Syntax::CLI_COMPLETIONS_FOR)
    {
        eprintln!("Error [E2102]: Completions arguments don't match `{USAGE}`.");
        eprintln!(" Why: A completion script needs one supported shell and, optionally, one compiled Jet program.");
        eprintln!(" Fix: Run `{USAGE}`.");
        eprintln!("More: jet-lang.dev/e/E2102");
        exit(ExitCodes::USAGE);
    }
    let shell = shell.unwrap();
    let out = if args.len() == 1 {
        match shell {
            "bash" => jet::CLI::completions_bash(),
            "zsh" => jet::CLI::completions_zsh(),
            "fish" => jet::CLI::completions_fish(),
            "powershell" => jet::CLI::completions_powershell(),
            _ => unreachable!(),
        }
    } else {
        let program = &args[2];
        let command_name = jet::CLI::completion_command_name(program)
            .unwrap_or_else(|error| completion_metadata_error(program, error));
        let mut file = open_completion_program(program).unwrap_or_else(|error| {
            completion_metadata_error(
                program,
                &format!("the program could not be opened ({error})"),
            )
        });
        let metadata = file.metadata().unwrap_or_else(|error| {
            completion_metadata_error(
                program,
                &format!("the opened program could not be inspected ({error})"),
            )
        });
        if !metadata.file_type().is_file() {
            completion_metadata_error(program, "the program is not a regular file");
        }
        const MAX_PROGRAM_BYTES: u64 = 512 * 1024 * 1024;
        if metadata.len() > MAX_PROGRAM_BYTES {
            completion_metadata_error(
                program,
                "the program is larger than the 512 MiB metadata-reader limit",
            );
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        Read::by_ref(&mut file)
            .take(MAX_PROGRAM_BYTES + 1)
            .read_to_end(&mut bytes)
            .unwrap_or_else(|error| {
                completion_metadata_error(
                    program,
                    &format!("the opened program could not be read ({error})"),
                )
            });
        if bytes.len() as u64 > MAX_PROGRAM_BYTES {
            completion_metadata_error(
                program,
                "the program grew beyond the 512 MiB metadata-reader limit while being read",
            );
        }
        let schema = jet_foundation::CLISchema::read_executable(&bytes)
            .unwrap_or_else(|error| completion_metadata_error(program, &error.to_string()));
        jet::CLI::completions_for_program(shell, &command_name, &schema).unwrap()
    };
    print!("{}", out);
}

#[cfg(unix)]
fn open_completion_program(path: &str) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const NONBLOCK: i32 = 0o4000;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    const NONBLOCK: i32 = 0x0004;
    OpenOptions::new()
        .read(true)
        .custom_flags(NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_completion_program(path: &str) -> std::io::Result<File> {
    OpenOptions::new().read(true).open(path)
}

fn completion_metadata_error(program: &str, why: &str) -> ! {
    let program = program
        .chars()
        .flat_map(char::escape_default)
        .collect::<String>();
    eprintln!("Error [E2103]: Couldn't read command metadata from `{program}`.");
    eprintln!(" Why: {why}.");
    eprintln!(" Fix: Rebuild the program with this Jet toolchain, then try again.");
    eprintln!("More: jet-lang.dev/e/E2103");
    exit(ExitCodes::USER_ERROR);
}

/// Every `jet self devtools` subcommand name, for the usage line and typo errors.
const DEVTOOLS_SUBCOMMANDS: &str =
    "grammars | reduce | ice-report | new-example | new-ui | check-fixture-paths | bless";

/// `jet self devtools grammars` — D-HL1 generated lexical base for editor grammars.
/// c450 (D-DEVTOOLS1=A): extended with maintainer-facing minimizer/scaffolding
/// tools, all under this same hidden namespace (never top-level commands).
pub(crate) fn run_devtools(args: &[&String], mode: OutputMode) {
    match args.first().map(|s| s.as_str()) {
        Some("grammars") => {
            write_generated_section(
                "editors/vscode/syntaxes/jet.tmLanguage.json",
                &jet::Syntax::render_vscode_generated_highlights(),
                mode.quiet,
            );
            write_generated_section(
                "editors/jet.tmGrammar",
                &jet::Syntax::render_vscode_generated_highlights(),
                mode.quiet,
            );
            write_generated_section(
                "editors/tree-sitter/grammar.js",
                &jet::Syntax::render_tree_sitter_generated_highlights(),
                mode.quiet,
            );
            write_generated_section(
                "editors/zed/languages/jet/highlights.scm",
                &jet::Syntax::render_zed_generated_highlights(),
                mode.quiet,
            );
            // #1659 criterion 3: the files were still written; `--quiet`
            // only mutes this confirmation line.
            if !mode.quiet {
                println!("regenerated editor grammar sections");
            }
        }
        Some("reduce") => run_devtools_reduce(&args[1..]),
        Some("ice-report") => run_devtools_ice_report(&args[1..]),
        Some("new-example") => run_devtools_new_example(&args[1..]),
        Some("new-ui") => run_devtools_new_ui(&args[1..]),
        Some("check-fixture-paths") => run_devtools_check_fixture_paths(),
        Some("bless") => run_devtools_bless(&args[1..]),
        Some("probe") => run_devtools_probe(&args[1..]),
        Some(other) => {
            crate::cli_error!("E2101", "unknown `devtools` subcommand `{}`", other);
            eprintln!(
                "usage: {} devtools <{}>",
                jet::Syntax::BINARY_NAME,
                DEVTOOLS_SUBCOMMANDS
            );
            exit(ExitCodes::USAGE);
        }
        None => {
            eprintln!(
                "usage: {} devtools <{}>",
                jet::Syntax::BINARY_NAME,
                DEVTOOLS_SUBCOMMANDS
            );
            exit(ExitCodes::USAGE);
        }
    }
}

// ──────────────────────────────────────────────
// c450: `jet self devtools reduce` — delta-debugging minimizer.
// ──────────────────────────────────────────────

/// `jet self devtools reduce <file.jet> [--code EXXXX]`.
///
/// Oracle (what makes a candidate "still interesting"):
///   - default: the front end accepts the file AND rustc rejects the
///     generated Rust (an I2 repro — invariant I2 says this must never
///     happen in a shipped compiler, so a minimal repro is worth having);
///   - `--code EXXXX`: the front end emits diagnostic `EXXXX` (from either
///     an error result or a lint on a successful compile).
///
/// Shrinks by removing line-chunks (a simplified Zeller ddmin: try removing
/// progressively smaller contiguous chunks, re-trying the whole file at a
/// finer granularity whenever a whole pass makes no progress) and writes the
/// smallest failing case to `<file>.reduced.<ext>`.
pub(crate) fn run_devtools_reduce(args: &[&String]) {
    if args.is_empty() {
        eprintln!(
            "usage: {} devtools reduce <file.{}> [--code EXXXX]",
            jet::Syntax::BINARY_NAME,
            jet::Syntax::FILE_EXT
        );
        exit(ExitCodes::USAGE);
    }
    let file = args[0].as_str();
    let mut code_filter: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--code" => {
                code_filter = args.get(i + 1).map(|s| s.to_string());
                i += 2;
            }
            other => {
                crate::cli_error!("E2102", "unknown `reduce` flag `{}`", other);
                exit(ExitCodes::USAGE);
            }
        }
    }

    let src = fs::read_to_string(file).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read `{}`: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });

    // `compile_with_path` reads its *file* argument straight off disk (its
    // `src` parameter is display-only), so every candidate has to actually
    // exist on disk under the same extension before we can ask the oracle
    // about it. One scratch path, rewritten each try.
    let ext = Path::new(file)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or(jet::Syntax::FILE_EXT);
    let scratch = std::env::temp_dir().join(format!(
        "jet_devtools_reduce_{}.{}",
        std::process::id(),
        ext
    ));
    let interesting = |text: &str| reduce_oracle(&scratch, text, code_filter.as_deref());

    if !interesting(&src) {
        let why = code_filter.as_ref().map_or_else(
            || {
                "either the front end already rejects it, or rustc accepts the generated Rust"
                    .to_string()
            },
            |code| format!("the front end never emits `{code}` for this file"),
        );
        crate::cli_error!(@full "E2104", format!("`{}` doesn't reproduce the target oracle as given", file), why, "confirm the case fails the way you expect, then reduce it");
        exit(ExitCodes::USER_ERROR);
    }

    let lines: Vec<String> = src.lines().map(|s| s.to_string()).collect();
    println!("reduce: starting at {} line(s)", lines.len());
    let reduced = ddmin(lines, &interesting);
    println!("reduce: finished at {} line(s)", reduced.len());

    let mut out_text = reduced.join("\n");
    if !out_text.is_empty() {
        out_text.push('\n');
    }
    let out_path = reduced_path(file);
    fs::write(&out_path, &out_text).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", out_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });
    let _ = fs::remove_file(&scratch);
    println!("wrote {}", out_path.display());
}

/// Whether `src` still triggers the target oracle. `code`: `None` = default
/// I2 oracle (front end accepts, rustc rejects); `Some(code)` = the front end
/// emits that diagnostic code (error or lint). Writes `src` to `scratch`
/// first since `compile_with_path` reads its file argument off disk.
fn reduce_oracle(scratch: &Path, src: &str, code: Option<&str>) -> bool {
    if fs::write(scratch, src).is_err() {
        return false;
    }
    let shown = scratch.to_string_lossy().into_owned();
    match jet::compile_with_path(src, &shown) {
        Ok(out) => match code {
            Some(c) => out.lints.iter().any(|d| d.code == c),
            None => rustc_rejects(&out.rust),
        },
        Err(diags) => match code {
            Some(c) => diags.iter().any(|d| d.code == c),
            None => false, // default oracle needs the front end to accept
        },
    }
}

/// Simplified Zeller ddmin over line-chunks: repeatedly try removing a
/// contiguous chunk of the current granularity; on any removal that keeps the
/// oracle interesting, keep the shrunk version and tighten the granularity;
/// on a full pass with no progress, double the chunk count (finer chunks)
/// until we're at single-line granularity, then stop.
fn ddmin(lines: Vec<String>, interesting: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut current = lines;
    let mut n: usize = 2;
    loop {
        if current.len() < 2 {
            break;
        }
        let chunk_size = (current.len() + n - 1) / n;
        if chunk_size == 0 {
            break;
        }
        let mut made_progress = false;
        let mut start = 0;
        while start < current.len() {
            let end = (start + chunk_size).min(current.len());
            let mut candidate = current.clone();
            candidate.drain(start..end);
            if !candidate.is_empty() && interesting(&candidate.join("\n")) {
                println!(
                    "reduce: dropped lines {}..{} -> {} line(s) left",
                    start + 1,
                    end,
                    candidate.len()
                );
                current = candidate;
                n = if n > 2 { n - 1 } else { 2 };
                made_progress = true;
                break;
            }
            start = end;
        }
        if !made_progress {
            if n >= current.len() {
                break;
            }
            n = (n * 2).min(current.len());
        }
    }
    current
}

/// `<file>.reduced.<ext>` sibling path for the reducer's output.
fn reduced_path(file: &str) -> PathBuf {
    let p = Path::new(file);
    let stem_name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("reduced");
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or(jet::Syntax::FILE_EXT);
    let name = format!("{}.reduced.{}", stem_name, ext);
    match p.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

/// Run rustc over generated Rust source with no linking (`--emit=metadata`,
/// I2's oracle only cares whether rustc accepts the code, not whether it
/// links). Returns `(accepted, stderr)`; a rustc that can't even be invoked
/// counts as "accepted" so a missing toolchain never manufactures a false
/// I2 repro.
fn rustc_probe(rust_code: &str) -> (bool, String) {
    let tmp_dir = std::env::temp_dir().join(format!("jet_devtools_rustc_{}", std::process::id()));
    if fs::create_dir_all(&tmp_dir).is_err() {
        return (true, String::new());
    }
    let rs_path = tmp_dir.join("check.rs");
    if fs::write(&rs_path, rust_code).is_err() {
        let _ = fs::remove_dir_all(&tmp_dir);
        return (true, String::new());
    }
    let meta_path = tmp_dir.join("check.rmeta");
    let result = Command::new("rustc")
        .arg("--edition")
        .arg("2021")
        .arg("--crate-name")
        .arg("jet_devtools_check")
        .arg("--emit=metadata")
        .arg(&rs_path)
        .arg("-o")
        .arg(&meta_path)
        .output();
    let outcome = match result {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(_) => (true, String::new()),
    };
    let _ = fs::remove_dir_all(&tmp_dir);
    outcome
}

fn rustc_rejects(rust_code: &str) -> bool {
    !rustc_probe(rust_code).0
}

// ──────────────────────────────────────────────
// c450: `jet self devtools ice-report` — bundle an I2 repro for a bug report.
// ──────────────────────────────────────────────

/// `jet self devtools ice-report <file.jet>` — bundles the source, generated Rust,
/// rustc's stderr, and both tool versions into one directory under
/// `.jet/ice-report/<stem>-<unix-time>/` so a bug report has everything
/// attached in one place. Prints the bundle path.
pub(crate) fn run_devtools_ice_report(args: &[&String]) {
    if args.is_empty() {
        eprintln!(
            "usage: {} devtools ice-report <file.{}>",
            jet::Syntax::BINARY_NAME,
            jet::Syntax::FILE_EXT
        );
        exit(ExitCodes::USAGE);
    }
    let file = args[0].as_str();
    let src = fs::read_to_string(file).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read `{}`: {}", file, e);
        exit(ExitCodes::USER_ERROR);
    });

    let out = match jet::compile_with_path(&src, file) {
        Ok(o) => o,
        Err(diags) => {
            crate::cli_error!(@full "E2105", format!("`{}` doesn't reach codegen — the front end already rejects it", file), "ice-report bundles a case that compiles to Rust (an I2 repro)", format!("fix the front-end errors first, or use `{} devtools reduce --code <CODE>` to shrink a front-end diagnostic instead", jet::Syntax::BINARY_NAME));
            eprint!("{}", jet::render_diagnostics(file, &src, &diags));
            exit(ExitCodes::USER_ERROR);
        }
    };

    let (accepted, rustc_stderr) = rustc_probe(&out.rust);
    if accepted {
        println!(
            "note: rustc accepted the generated Rust for `{}` — bundling anyway, but this isn't an I2 repro",
            file
        );
    }

    let rustc_version = Command::new("rustc")
        .arg("--version")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "rustc: not found".to_string());
    let jet_version = env!("CARGO_PKG_VERSION");

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let bundle_dir =
        PathBuf::from(".jet")
            .join("ice-report")
            .join(format!("{}-{}", stem(file), ts));
    fs::create_dir_all(&bundle_dir).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't create `{}`: {}", bundle_dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    let write = |name: &str, content: &str| {
        fs::write(bundle_dir.join(name), content).unwrap_or_else(|e| {
            crate::cli_error!("E2105", "couldn't write `{}`: {}", name, e);
            exit(ExitCodes::USER_ERROR);
        });
    };
    write(&format!("source.{}", jet::Syntax::FILE_EXT), &src);
    write("generated.rs", &out.rust);
    write("rustc.stderr", &rustc_stderr);
    write(
        "versions.txt",
        &format!("jet {}\n{}\n", jet_version, rustc_version),
    );

    println!("wrote ICE report bundle to {}", bundle_dir.display());
}

// ──────────────────────────────────────────────
// c450: `jet self devtools new-example` / `new-ui` — scaffold I5/I4 fixtures.
// ──────────────────────────────────────────────

/// `jet self devtools new-example <topic>/<name>` — scaffolds
/// `examples/features/<topic>/<name>.jet` and
/// `examples/features/expected/<topic>/<name>.out`, matching the layout
/// `tests/golden.rs` walks exactly. The stub is a real, passing example (I5:
/// no example ships broken) that the author edits to demonstrate the feature.
pub(crate) fn run_devtools_new_example(args: &[&String]) {
    if args.is_empty() {
        eprintln!(
            "usage: {} devtools new-example <topic>/<name>",
            jet::Syntax::BINARY_NAME
        );
        exit(ExitCodes::USAGE);
    }
    let spec = args[0].as_str();
    let (topic, name) = match spec.split_once('/') {
        Some((t, n)) if !t.is_empty() && !n.is_empty() => (t, n),
        _ => {
            crate::cli_error!("E2104", "expected `<topic>/<name>`, got `{}`", spec);
            exit(ExitCodes::USER_ERROR);
        }
    };

    let ext = jet::Syntax::FILE_EXT;
    let example_dir = PathBuf::from("examples/features").join(topic);
    let expected_dir = PathBuf::from("examples/features/expected").join(topic);
    let example_path = example_dir.join(format!("{}.{}", name, ext));
    let expected_path = expected_dir.join(format!("{}.out", name));

    if example_path.exists() {
        crate::cli_error!("E2104", "`{}` already exists", example_path.display());
        exit(ExitCodes::USER_ERROR);
    }

    fs::create_dir_all(&example_dir).unwrap_or_else(|e| {
        crate::cli_error!(
            "E2105",
            "couldn't create `{}`: {}",
            example_dir.display(),
            e
        );
        exit(ExitCodes::USER_ERROR);
    });
    fs::create_dir_all(&expected_dir).unwrap_or_else(|e| {
        crate::cli_error!(
            "E2105",
            "couldn't create `{}`: {}",
            expected_dir.display(),
            e
        );
        exit(ExitCodes::USER_ERROR);
    });

    let greeting = format!("scaffold: {}/{}", topic, name);
    let src = format!(
        "// TODO: describe examples/features/{}/{}.{}\nfn run() {{\n    print(\"{}\")\n}}\n",
        topic, name, ext, greeting
    );
    fs::write(&example_path, &src).unwrap_or_else(|e| {
        crate::cli_error!(
            "E2105",
            "couldn't write `{}`: {}",
            example_path.display(),
            e
        );
        exit(ExitCodes::USER_ERROR);
    });
    fs::write(&expected_path, format!("{}\n", greeting)).unwrap_or_else(|e| {
        crate::cli_error!(
            "E2105",
            "couldn't write `{}`: {}",
            expected_path.display(),
            e
        );
        exit(ExitCodes::USER_ERROR);
    });

    println!("wrote {}", example_path.display());
    println!("wrote {}", expected_path.display());
}

/// `jet self devtools new-ui <name>` — scaffolds `tests/ui/<name>.jet` and its
/// `<name>.stderr` snapshot, matching the layout `tests/diagnostic_snapshots.rs`
/// walks exactly. The stub triggers a real (if generic) diagnostic and its
/// `.stderr` is computed with the SAME calls the harness uses, so the pair is
/// valid the moment it's written — edit the `.jet` to demonstrate the real
/// diagnostic, then re-bless with `jet self devtools bless diagnostic_snapshots`.
pub(crate) fn run_devtools_new_ui(args: &[&String]) {
    if args.is_empty() {
        eprintln!("usage: {} devtools new-ui <name>", jet::Syntax::BINARY_NAME);
        exit(ExitCodes::USAGE);
    }
    let name = args[0].as_str();
    if name.is_empty() || name.contains('/') || name.contains('.') {
        crate::cli_error!(
            "E2104",
            "`new-ui` takes a bare fixture name, no path or extension: got `{}`",
            name
        );
        exit(ExitCodes::USER_ERROR);
    }

    let ext = jet::Syntax::FILE_EXT;
    let dir = PathBuf::from("tests/ui");
    let jet_path = dir.join(format!("{}.{}", name, ext));
    let stderr_path = dir.join(format!("{}.stderr", name));

    if jet_path.exists() {
        crate::cli_error!("E2104", "`{}` already exists", jet_path.display());
        exit(ExitCodes::USER_ERROR);
    }
    fs::create_dir_all(&dir).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't create `{}`: {}", dir.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    let src = "// TODO: describe what this diagnostic demonstrates.\n\
fn run() {\n    print(definitely_undefined_scaffold_symbol)\n}\n"
        .to_string();
    fs::write(&jet_path, &src).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", jet_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    // Mirror `tests/diagnostic_snapshots.rs`'s `ui_snapshots` exactly, so the
    // pair this writes is already a valid harness fixture.
    let shown_path = format!("tests/ui/{}.{}", name, ext);
    let actual = match jet::compile_with_path(&src, &shown_path) {
        Err(diags) => jet::render_diagnostics(&shown_path, &src, &diags),
        Ok(_) => "(no errors)\n".to_string(),
    };
    fs::write(&stderr_path, &actual).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", stderr_path.display(), e);
        exit(ExitCodes::USER_ERROR);
    });

    println!("wrote {}", jet_path.display());
    println!("wrote {}", stderr_path.display());
    if actual == "(no errors)\n" {
        println!(
            "note: scaffold compiled cleanly — edit `{}` to trigger the diagnostic you want, \
             then `{} devtools bless diagnostic_snapshots`",
            jet_path.display(),
            jet::Syntax::BINARY_NAME
        );
    }
}

// ──────────────────────────────────────────────
// c450: `jet self devtools check-fixture-paths` — validate hardcoded path fixtures.
// ──────────────────────────────────────────────

/// `jet self devtools check-fixture-paths` — greps every `tests/**/*.rs` file for
/// hardcoded fixture path literals (`examples/features/...`, `docs/spec/...`,
/// `tests/ui/...`, etc.) and confirms each one exists on disk relative to the
/// current directory (run from the repo root). Path-embedding fixtures rot
/// silently when an example moves; this is the check that catches it.
pub(crate) fn run_devtools_check_fixture_paths() {
    let tests_dir = PathBuf::from("tests");
    if !tests_dir.is_dir() {
        crate::cli_error!(@fix "E2104", "no `tests/` directory here", "run from the repo root");
        exit(ExitCodes::USER_ERROR);
    }

    let mut rs_files = Vec::new();
    collect_rs_files(&tests_dir, &mut rs_files);

    let mut checked = 0usize;
    let mut missing: Vec<(PathBuf, String)> = Vec::new();
    for rs in &rs_files {
        let text = fs::read_to_string(rs).unwrap_or_default();
        for candidate in extract_hardcoded_paths(&text) {
            checked += 1;
            if !Path::new(&candidate).exists() {
                missing.push((rs.clone(), candidate));
            }
        }
    }

    if missing.is_empty() {
        println!(
            "check-fixture-paths: {} embedded path(s) across {} file(s), all present",
            checked,
            rs_files.len()
        );
    } else {
        eprintln!("check-fixture-paths: {} missing path(s):", missing.len());
        for (file, path) in &missing {
            eprintln!("  {} -> `{}` does not exist", file.display(), path);
        }
        exit(ExitCodes::USER_ERROR);
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// Pull out string-literal path fixtures from Rust source text. Deliberately
/// conservative: only whole quoted literals (never `format!` templates with a
/// `{`) whose prefix marks them as a fixture path and whose suffix is a known
/// fixture extension, so dynamically-joined paths (already covered by their
/// own `read_to_string`/`unwrap_or_else` panics at test time) are left alone.
fn extract_hardcoded_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'"' {
                if bytes[j] == b'\\' {
                    j += 1;
                }
                j += 1;
            }
            if j <= text.len() {
                let lit = &text[start..j.min(text.len())];
                // `render_diagnostics`/`render_all_*` take a "shown" display
                // label as their file-name argument, not a path that must
                // exist (tests/net_tls.rs feeds one a made-up name purely so
                // the rendered banner looks like a real fixture path).
                let mut before_start = start.saturating_sub(40);
                while before_start < i && !text.is_char_boundary(before_start) {
                    before_start += 1;
                }
                let before = &text[before_start..i];
                let is_display_label = before.contains("render_diagnostics(")
                    || before.contains("render_all_colored(")
                    || before.contains("render_all_json(");
                if !is_display_label && is_hardcoded_fixture_path(lit) {
                    out.push(lit.to_string());
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn is_hardcoded_fixture_path(lit: &str) -> bool {
    let known_prefix = lit.starts_with("examples/features/")
        || lit.starts_with("docs/spec/")
        || lit.starts_with("tests/ui/")
        || lit.starts_with("tests/ui_lint/")
        || lit.starts_with("tests/cli/")
        || lit.starts_with("tests/release/");
    if !known_prefix || lit.contains('{') || lit.contains('}') {
        return false;
    }
    let ext = format!(".{}", jet::Syntax::FILE_EXT);
    lit.ends_with(&ext)
        || lit.ends_with(".md")
        || lit.ends_with(".out")
        || lit.ends_with(".stderr")
        || lit.ends_with(".warn")
        || lit.ends_with(".txt")
        || lit.ends_with(".snapshot")
}

// ──────────────────────────────────────────────
// c450: `jet self devtools bless` — wrapper over the UPDATE_EXPECT re-bless convention.
// ──────────────────────────────────────────────

/// Every test binary that owns `UPDATE_EXPECT`-blessable snapshots (I4). Kept
/// as one list so `bless` never drifts from what actually re-blesses on
/// `UPDATE_EXPECT=1`.
pub(crate) const BLESS_TARGETS: &[&str] = &[
    "cli",
    "cross",
    "diagnostic_snapshots",
    "diagnostics_coverage",
    "ice_report_codegen_rejection",
    "release_gates",
];

/// `jet self devtools bless [target...] [--dry-run]` — runs
/// `UPDATE_EXPECT=1 cargo test --test <target>` for each named target (all of
/// `BLESS_TARGETS` when none is given). This is the one re-bless mechanism in
/// the repo (see each test file's own "bless with UPDATE_EXPECT=1" doc
/// comment) — `bless` just names and runs it so nobody has to remember the
/// env var or the target list. `--dry-run` prints the commands without
/// running them (never mutates a snapshot file).
///
/// A real sweep also needs `UPDATE_EXPECT_REASON` in the environment (card
/// #2026): the snapshot harness refuses a blanket bless that does not name the
/// compiler change it records, because 6fd88282b rewrote 122 diagnostic
/// contracts with zero crate source changed. `Command` inherits the
/// environment, so the reason reaches every target through the same variable.
pub(crate) fn run_devtools_bless(args: &[&String]) {
    let mut dry_run = false;
    let mut requested: Vec<String> = Vec::new();
    for a in args {
        if a.as_str() == jet::CLI::DRY_RUN_FLAG {
            dry_run = true;
        } else {
            requested.push(a.to_string());
        }
    }

    let targets = match resolve_bless_targets(&requested) {
        Ok(t) => t,
        Err(unknown) => {
            crate::cli_error!("E2104", "unknown bless target(s): {}", unknown);
            eprintln!(
                "usage: {} devtools bless [target...] [--dry-run]",
                jet::Syntax::BINARY_NAME
            );
            eprintln!("known targets: {}", BLESS_TARGETS.join(", "));
            exit(ExitCodes::USAGE);
        }
    };

    if dry_run {
        for t in &targets {
            println!("would run: UPDATE_EXPECT=1 cargo test --test {}", t);
        }
        return;
    }

    let reason = std::env::var("UPDATE_EXPECT_REASON")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    if reason.len() < 8 {
        crate::cli_error!(
            "E2104",
            "a blanket bless must name the compiler change it records"
        );
        eprintln!(
            "set the reason, then re-run:\n    UPDATE_EXPECT_REASON=\"<commit or ratified decision>\" {} self devtools bless",
            jet::Syntax::BINARY_NAME
        );
        eprintln!(
            "to bless one reviewed fixture instead: JET_UI_FILTER=<fixture> UPDATE_EXPECT=<same fixture> cargo test --test diagnostic_snapshots"
        );
        exit(ExitCodes::USAGE);
    }
    println!("bless: reason recorded — {}", reason);

    let mut any_failed = false;
    for t in &targets {
        println!("bless: UPDATE_EXPECT=1 cargo test --test {}", t);
        match bless_command(t).status() {
            Ok(s) if s.success() => {}
            Ok(s) => {
                any_failed = true;
                eprintln!("bless: `{}` exited with {}", t, s);
            }
            Err(e) => {
                any_failed = true;
                eprintln!("bless: couldn't run `cargo test --test {}`: {}", t, e);
            }
        }
    }
    if any_failed {
        exit(ExitCodes::USER_ERROR);
    }
    println!(
        "bless: done ({} target{})",
        targets.len(),
        if targets.len() == 1 { "" } else { "s" }
    );
}

/// Resolve requested target names against `BLESS_TARGETS`; empty `requested`
/// means "all of them". `Err` carries the comma-joined unknown names.
pub(crate) fn resolve_bless_targets(requested: &[String]) -> Result<Vec<&'static str>, String> {
    if requested.is_empty() {
        return Ok(BLESS_TARGETS.to_vec());
    }
    let mut out = Vec::new();
    let mut unknown = Vec::new();
    for r in requested {
        match BLESS_TARGETS.iter().find(|k| **k == r.as_str()) {
            Some(t) => out.push(*t),
            None => unknown.push(r.clone()),
        }
    }
    if !unknown.is_empty() {
        return Err(unknown.join(", "));
    }
    Ok(out)
}

/// Build (never spawns) the `UPDATE_EXPECT=1 cargo test --test <target>`
/// command for one bless target.
pub(crate) fn bless_command(target: &str) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.env("UPDATE_EXPECT", "1");
    cmd.args(["test", "--test", target]);
    cmd
}

fn write_generated_section(path: &str, fresh: &str, quiet: bool) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't read `{}`: {}", path, e);
        exit(ExitCodes::USER_ERROR);
    });
    let start = text
        .find(jet::Syntax::HIGHLIGHT_GENERATED_START)
        .unwrap_or_else(|| {
            crate::cli_error!(
                "E2105",
                "`{}` has no `{}` marker",
                path,
                jet::Syntax::HIGHLIGHT_GENERATED_START
            );
            exit(ExitCodes::USER_ERROR);
        });
    let prefix_start = text[..start].rfind('\n').map_or(0, |idx| idx + 1);
    let after_start = &text[start..];
    let end_rel = after_start
        .find(jet::Syntax::HIGHLIGHT_GENERATED_END)
        .unwrap_or_else(|| {
            crate::cli_error!(
                "E2105",
                "`{}` has no `{}` marker",
                path,
                jet::Syntax::HIGHLIGHT_GENERATED_END
            );
            exit(ExitCodes::USER_ERROR);
        });
    let end_marker = start + end_rel + jet::Syntax::HIGHLIGHT_GENERATED_END.len();
    let suffix_start = text[end_marker..]
        .find('\n')
        .map_or(text.len(), |idx| end_marker + idx + 1);

    let mut out = String::new();
    out.push_str(&text[..prefix_start]);
    out.push_str(fresh.trim_end());
    out.push('\n');
    out.push_str(&text[suffix_start..]);
    fs::write(path, out).unwrap_or_else(|e| {
        crate::cli_error!("E2105", "couldn't write `{}`: {}", path, e);
        exit(ExitCodes::USER_ERROR);
    });
    // #1659 criterion 3 (round 2): the file is still written; `--quiet` only
    // mutes this per-file progress line (same rule as the summary line below).
    if !quiet {
        println!("wrote {}", path);
    }
}

/// `jet self doctor` — environment self-diagnosis with actionable fixes (D-DX2,
/// D-BUILD1). Offline by default; `--online` enables network checks; `--fix`
/// applies the auto-fixable problems. The advisory code for rustc/cache/PATH
/// problems is L2101.
pub(crate) fn run_doctor(online: bool, apply: bool, mode: OutputMode, cross_target: Option<&str>) {
    // E2-M15: main parses both `--target=<triple>` and `--target <triple>`
    // before dispatch, so Doctor receives the same effective target as build.
    let checks = jet::Doctor::run(jet::Doctor::Options {
        online,
        cross_target: cross_target.map(str::to_string),
    });
    let color = mode.color_stderr_for(std::io::stdout().is_terminal());

    if apply {
        let fixed = jet::Doctor::apply_fixes(&checks);
        if fixed.is_empty() {
            println!("doctor: nothing to auto-fix");
        } else {
            for f in &fixed {
                println!("fixed: {}", f);
            }
        }
        // Re-run so the report reflects the world after fixes.
        return run_doctor(online, false, mode, cross_target);
    }

    use jet::Doctor::Health;
    let bold = |s: &str| {
        if color {
            format!("\x1b[1m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    };
    let green = |s: &str| {
        if color {
            format!("\x1b[32m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    };
    let yellow = |s: &str| {
        if color {
            format!("\x1b[33m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    };

    println!("{}", bold(&format!("{} doctor", jet::Syntax::BINARY_NAME)));
    let mut last_section = "";
    for c in &checks {
        if c.section != last_section {
            println!();
            println!("{}", bold(c.section));
            last_section = c.section;
        }
        let (mark, label) = match c.health {
            Health::Ok => (green("ok  "), c.label.clone()),
            Health::Note => ("note".to_string(), c.label.clone()),
            Health::Problem => (yellow("warn"), c.label.clone()),
        };
        println!("  [{}] {}: {}", mark, label, c.detail);
        if let Some(fix) = &c.fix {
            println!("        Fix: {}", fix);
            if c.auto_fixable {
                println!(
                    "        (auto-fixable: run `{} doctor --fix`)",
                    jet::Syntax::BINARY_NAME
                );
            }
        }
    }
    println!();
    if jet::Doctor::has_problem(&checks) {
        println!("Warning [L2101] (doctor_advisory): Toolchain checks need attention");
        println!(" Why: One or more required tools or paths are unavailable");
        println!(" Fix: Follow the fixes above, then run `jet self doctor` again");
        println!(" {}", jet::Explain::pointer_line("L2101", color));
        println!("More: jet-lang.dev/e/L2101");
        exit(ExitCodes::USER_ERROR);
    } else {
        println!("everything looks good.");
    }
}

/// `jet explain --web-graph <file>` — print the sema-known application graph.
pub(crate) fn run_explain_web_graph(args: &[String], mode: OutputMode) {
    let mut file: Option<&str> = None;
    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        file = Some(arg.as_str());
        break;
    }
    let Some(path) = file else {
        eprintln!(
            "usage: {} explain --web-graph <file.{}>",
            jet::Syntax::BINARY_NAME,
            jet::Syntax::FILE_EXT
        );
        exit(ExitCodes::USAGE);
    };
    let abs = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let entry = abs.display().to_string();
    let (diags, _bundle, facts) = jet::Driver::check_file_with_effect_facts(&entry, None, false);
    let Some(graph) = facts.web_app.as_ref() else {
        if diags
            .iter()
            .any(|d| d.severity == jet::Diagnostics::Severity::Error)
        {
            for d in &diags {
                eprintln!(
                    "{}",
                    jet::render_diagnostics(&entry, "", std::slice::from_ref(d))
                );
            }
            exit(ExitCodes::USER_ERROR);
        }
        if mode.json {
            println!(
                "{}",
                StatusEnvelope::new("inspect.web", true)
                    .with_field("web_app", StatusValue::Null)
                    .json()
            );
        } else {
            println!("(none)");
        }
        return;
    };
    let empty = graph.entry_file.is_empty()
        && graph.routes.is_empty()
        && graph.actions.is_empty()
        && graph.mounts.is_empty()
        && graph.routes_from.is_empty();
    if empty {
        if diags
            .iter()
            .any(|d| d.severity == jet::Diagnostics::Severity::Error)
        {
            for d in &diags {
                eprintln!(
                    "{}",
                    jet::render_diagnostics(&entry, "", std::slice::from_ref(d))
                );
            }
        }
        if mode.json {
            println!(
                "{}",
                StatusEnvelope::new("inspect.web", true)
                    .with_field("web_app", StatusValue::Null)
                    .json()
            );
        } else {
            println!("(none)");
        }
        return;
    }
    if mode.json {
        let web_app =
            StatusValue::parse(&graph.to_json()).expect("web graph projection must be valid JSON");
        println!(
            "{}",
            StatusEnvelope::new("inspect.web", true)
                .with_field("web_app", web_app)
                .json()
        );
    } else {
        for line in graph.explain_lines() {
            println!("{line}");
        }
    }
}

/// `jet explain <CODE|FACT> [file]` — print a diagnostic essay or one complete
/// build-fact writer chain.
pub(crate) fn run_explain(
    code: Option<&str>,
    fact_file: Option<&str>,
    mode: OutputMode,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) {
    let code = match code {
        Some(c) => c,
        None => {
            eprintln!(
                "usage: {} explain <CODE|FACT> [file]\n       {} explain marker <file>:<line> <policy-key>",
                jet::Syntax::BINARY_NAME,
                jet::Syntax::BINARY_NAME
            );
            exit(ExitCodes::USAGE);
        }
    };
    if let Some(name) = jet::Explain::build_fact_name(code) {
        run_explain_fact(&name, fact_file, mode, profile, setting_overrides);
        return;
    }
    if let Some(key) = jet::Policy::PolicyKey::parse(code.trim_start_matches('@')) {
        if fact_file.is_some() {
            run_explain_policy(key, fact_file, mode);
            return;
        }
    }
    match jet::Explain::lookup(code) {
        Some(ex) => print_explanation(&ex, mode),
        None => {
            if jet::Explain::is_syntax_query(code) {
                let closest = jet::Explain::nearest_syntax(code)
                    .unwrap_or_else(|| "a registered syntax token".to_string());
                // No typed edit: E2106 is an argv `explain` query, not a Jet source diagnostic.
                crate::emit_cli_row(
                    "E2106",
                    &[("token", code), ("closest", closest.as_str())],
                    mode.json,
                );
                exit(ExitCodes::USER_ERROR);
            }
            crate::cli_error!(@fix "E2104", format!("no diagnostic code `{}` exists", code), format!("run a command that reports an error to see its code, e.g. `{} check file.{}`", jet::Syntax::BINARY_NAME, jet::Syntax::FILE_EXT));
            exit(ExitCodes::USER_ERROR);
        }
    }
}
/// `jet explain --cost <file>` — print the typed TIR cost projection.  This
/// deliberately reports both semantic remainders and optimizer-proven
/// removals; the latter are evidence for why no lint is emitted.
pub(crate) fn run_explain_cost(
    file: Option<&str>,
    mode: OutputMode,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) {
    let Some(file) = file else {
        eprintln!(
            "usage: {} explain --cost <file.{}>",
            jet::Syntax::BINARY_NAME,
            jet::Syntax::FILE_EXT
        );
        exit(ExitCodes::USAGE);
    };
    let src = match fs::read_to_string(file) {
        Ok(src) => src,
        Err(_) => {
            crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
            exit(ExitCodes::USER_ERROR);
        }
    };
    let projection = match crate::CmdInspect::check_projection_for_effects(
        Path::new(file),
        profile,
        setting_overrides,
    ) {
        Ok(projection) => projection,
        Err(diagnostics) => {
            let errors: Vec<_> = diagnostics
                .iter()
                .filter(|diagnostic| {
                    matches!(diagnostic.severity, jet::Diagnostics::Severity::Error)
                })
                .cloned()
                .collect();
            if !errors.is_empty() {
                report_problems(mode, file, &src, &errors);
                exit(ExitCodes::USER_ERROR);
            }
            crate::cli_error!("E2104", "cost projection could not check `{}`", file);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let report = match jet::Codegen::TIR::cost_report(&projection.bundle) {
        Ok(report) => report,
        Err(error) => exit_cost_projection_error(file, &error),
    };
    if mode.json {
        let sites = StatusValue::array(report.sites.iter().map(|site| {
            StatusValue::object(
                StatusFields::new()
                    .with("function", site.function.clone())
                    .with("line", source_line(&src, site.span))
                    .with("kind", site.kind.label())
                    .with("state", cost_state_label(site.state))
                    .with("loop_depth", site.loop_depth)
                    .with("tier", COST_TIER),
            )
        }));
        println!(
            "{}",
            StatusEnvelope::new("explain.cost", true)
                .with_field("file", file)
                .with_field("sites", sites)
                .json()
        );
        return;
    }
    if report.sites.is_empty() {
        println!("cost: `{}` has no typed cost sites", file);
        return;
    }
    for site in report.sites {
        println!(
            "{}:{} fn {} — {} — {} (loop depth {}; tier={})",
            file,
            source_line(&src, site.span),
            site.function,
            site.kind.label(),
            cost_state_label(site.state),
            site.loop_depth,
            COST_TIER,
        );
    }
}

const COST_TIER: &str = "shared-tir";

pub(crate) fn exit_cost_projection_error(
    file: &str,
    error: &jet::Codegen::TIR::TCostReportError,
) -> ! {
    let fix = match error {
        jet::Codegen::TIR::TCostReportError::Incomplete { surfaces } => format!(
            "report the missing checked specialization for {} with this source file; its cost cannot be proved until the compiler lowers that specialization",
            surfaces.join("; ")
        ),
        jet::Codegen::TIR::TCostReportError::Lowering { .. } => {
            "report this checked-TIR lowering failure with the source file".to_string()
        }
    };
    crate::cli_error!(
        @full "E2104",
        format!("cost projection for `{file}` failed"),
        error.message(),
        fix
    );
    exit(ExitCodes::USER_ERROR);
}

fn source_line(source: &str, span: jet::Diagnostics::Span) -> usize {
    source
        .get(..span.start.min(source.len()))
        .map_or(1, |prefix| {
            prefix.bytes().filter(|byte| *byte == b'\n').count() + 1
        })
}

fn cost_state_label(state: jet::Codegen::TIR::TCostState) -> &'static str {
    match state {
        jet::Codegen::TIR::TCostState::SemanticRemainder => "semantic remainder",
        jet::Codegen::TIR::TCostState::OptimizerRemoved => "optimizer-proven removed",
    }
}

fn explain_source_file(fact_file: Option<&str>) -> PathBuf {
    fact_file
        .map(PathBuf::from)
        .or_else(|| {
            let cwd = std::env::current_dir().ok()?;
            let mode = OutputMode {
                json: false,
                color: ColorChoice::Never,
                quiet: false,
            };
            crate::resolve_bare_entry("run", &cwd, None, mode, false).map(|entry| entry.path)
        })
        .unwrap_or_else(|| {
            crate::cli_error!(
                "E2104",
                "a source file is required to explain this build fact"
            );
            exit(ExitCodes::USAGE);
        })
}

fn explain_bundle(
    fact_file: Option<&str>,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> jet_foundation::Facts::BuildFactSnapshot {
    let file = explain_source_file(fact_file);
    jet::Driver::query_build_facts(file.to_string_lossy().as_ref(), profile, setting_overrides)
        .unwrap_or_else(|diags| {
            for diagnostic in diags {
                eprintln!("{}", diagnostic.what);
            }
            exit(ExitCodes::USER_ERROR);
        })
}

fn explain_policy_bundle(fact_file: Option<&str>) -> jet::AST::ProgramBundle {
    let file = explain_source_file(fact_file);
    jet::Loader::load_entry(file.to_string_lossy().as_ref()).unwrap_or_else(|diags| {
        for diagnostic in diags {
            eprintln!("{}", diagnostic.what);
        }
        exit(ExitCodes::USER_ERROR);
    })
}

fn print_explanation(explanation: &jet::Explain::Explanation, mode: OutputMode) {
    if mode.json {
        let what = explanation
            .what
            .as_deref()
            .unwrap_or(explanation.meaning.as_str());
        let optional = |value: Option<&String>| {
            value
                .map(|value| StatusValue::from(value.clone()))
                .unwrap_or(StatusValue::Null)
        };
        let fields = StatusFields::new()
            .with("code", explanation.code.clone())
            .with("stage", explanation.stage.clone())
            .with("what", what)
            .with("why", optional(explanation.why.as_ref()))
            .with("fix", optional(explanation.fix.as_ref()))
            .with("example", optional(explanation.example.as_ref()));
        println!(
            "{}",
            StatusEnvelope::new("explain", true)
                .with_fields(fields)
                .json()
        );
        return;
    }
    let color = ColorChoice::resolve(mode.color, std::io::stdout().is_terminal());
    print!("{}", jet::Explain::render(explanation, color));
}

fn run_explain_fact(
    name: &str,
    fact_file: Option<&str>,
    mode: OutputMode,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) {
    let build_facts = explain_bundle(fact_file, profile, setting_overrides);
    let Some(fact) = build_facts.contribution(name) else {
        if let Some(key) = name.strip_prefix("Build.Settings.") {
            crate::cli_error!(@full "E0302", format!("`build.settings.{key}` is undeclared"), "settings are declared in the package manifest", format!("add `{key}: Type = default` to `package.jet`"));
        } else {
            crate::cli_error!("E2104", "the selected build has no `{name}` fact");
        }
        exit(ExitCodes::USER_ERROR);
    };
    let Some(explanation) = jet::Explain::lookup_fact(fact.key.clone(), fact.provenance.clone())
    else {
        crate::cli_error!(
            @full "E3521",
            "the build fact contribution chain could not be resolved",
            "the selected fact writers must pass the shared contribution law",
            "remove the conflicting writer or choose one explicit contribution"
        );
        exit(ExitCodes::USER_ERROR);
    };
    print_explanation(&explanation, mode);
}

fn run_explain_policy(key: jet::Policy::PolicyKey, fact_file: Option<&str>, mode: OutputMode) {
    let bundle = explain_policy_bundle(fact_file);
    let declarations = bundle
        .modules
        .iter()
        .flat_map(|module| module.policy_declarations.iter())
        .filter(|declaration| declaration.key == key)
        .cloned()
        .collect::<Vec<_>>();
    let Some(explanation) = jet::Explain::lookup_policy(key, declarations) else {
        crate::cli_error!(@full "E2104", format!("policy `{}` has no effective declaration", key.name()), "the policy has no applicable writer at this source site", "add one registered policy declaration or explain a concrete marker site");
        exit(ExitCodes::USER_ERROR);
    };
    print_explanation(&explanation, mode);
}

/*
    The settings path deliberately has no separate renderer. `run_explain_fact`
    reads the resolved snapshot and sends every build fact through
    `Policy::explain`, so settings, fixed facts, and policy explanations keep
    one writer per line and one effective marker.
*/

/// D-MARK-SCOPE1: `jet explain marker <file>:<line> <policy-key>`.
pub(crate) fn run_explain_marker(site: Option<&str>, key: Option<&str>, mode: OutputMode) {
    let (Some(site), Some(key)) = (site, key) else {
        eprintln!(
            "usage: {} explain marker <file>:<line> <policy-key>",
            jet::Syntax::BINARY_NAME
        );
        exit(ExitCodes::USAGE);
    };
    let Some((file, line_text)) = site.rsplit_once(':') else {
        crate::cli_error!("E2104", "marker site must be `<file>:<line>`");
        exit(ExitCodes::USAGE);
    };
    let line = line_text
        .parse::<usize>()
        .ok()
        .filter(|line| *line > 0)
        .unwrap_or_else(|| {
            crate::cli_error!("E2104", "marker line must be a positive number");
            exit(ExitCodes::USAGE)
        });
    let Some(policy_key) = jet::Policy::PolicyKey::parse(key) else {
        crate::cli_error!("E2104", "`{key}` is not a registered scoped policy");
        exit(ExitCodes::USER_ERROR)
    };
    let bundle = jet::Loader::load_entry(file).unwrap_or_else(|diags| {
        for diag in diags {
            eprintln!("{}", diag.what);
        }
        exit(ExitCodes::USER_ERROR)
    });
    let module = &bundle.modules[0];
    let offset = if line == 1 {
        0
    } else {
        module
            .source
            .match_indices('\n')
            .nth(line - 2)
            .map(|(at, _)| at + 1)
            .unwrap_or(module.source.len())
    };
    let declarations = module
        .policy_declarations
        .iter()
        .filter(|declaration| match declaration.scope {
            jet::Policy::PolicyScope::Organization
            | jet::Policy::PolicyScope::Package
            | jet::Policy::PolicyScope::Module => true,
            jet::Policy::PolicyScope::Function | jet::Policy::PolicyScope::Block => declaration
                .target
                .is_some_and(|target| target.start <= offset && offset <= target.end),
        })
        .cloned()
        .collect::<Vec<_>>();
    let Some(explanation) = jet::Explain::lookup_policy(policy_key, declarations) else {
        crate::cli_error!("E2104", "`{key}` has no effective declaration at {site}");
        exit(ExitCodes::USER_ERROR)
    };
    print_explanation(&explanation, mode);
}

/// `jet inspect bind <header.h> [--pkg <lib>] [--overlay <path>] [--link <lib>] [-o <out.jet>]` (S59 / E2-M14 Phase 4).
///
/// Generates a `#Bindgen module c.<lib>.__bindgen__` cache from a C header,
/// using the same native std-only backend the compiler invokes on a cache miss.
/// Parses C function prototypes over the bindable type subset; skips and reports
/// what it cannot map (I3). An opaque handle without a close contract is
/// reported as E3208 with the handle name so the caller can supply a typed
/// overlay. E3208 also fires when the header is unreadable or has no bindable
/// prototypes — use `#Import module c.<lib>` for those declarations.
pub(crate) fn run_bind(args: &[&String]) {
    if matches!(
        args.first().map(|arg| arg.as_str()),
        Some("json" | "csv" | "sql" | "xml" | "proto")
    ) {
        let format = args[0].as_str();
        run_data_bind(format, &args[1..]);
        return;
    }
    if binding_plan_command(args) {
        run_binding_plan_command(args);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::CPP_MODULE_ROOT)
    {
        run_cpp_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::COM_MODULE_ROOT)
    {
        run_com_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::COBOL_MODULE_ROOT)
    {
        run_cobol_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::PERL_MODULE_ROOT)
    {
        run_perl_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::RUBY_MODULE_ROOT)
    {
        run_ruby_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::PHP_MODULE_ROOT)
    {
        run_php_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::R_MODULE_ROOT)
    {
        run_r_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::PY_MODULE_ROOT)
    {
        run_python_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::JS_MODULE_ROOT)
    {
        run_javascript_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::OCTAVE_MODULE_ROOT)
    {
        run_octave_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::PWSH_MODULE_ROOT)
    {
        run_powershell_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::DART_MODULE_ROOT)
    {
        run_dart_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::PASCAL_MODULE_ROOT)
    {
        run_pascal_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::ADA_MODULE_ROOT)
    {
        run_ada_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::TCL_MODULE_ROOT)
    {
        run_tcl_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::LUA_MODULE_ROOT)
    {
        run_lua_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::JAVA_MODULE_ROOT)
    {
        run_java_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::CS_MODULE_ROOT)
    {
        run_dotnet_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::GO_MODULE_ROOT)
    {
        run_go_bind(&args[1..]);
        return;
    }
    if args
        .first()
        .is_some_and(|arg| arg.as_str() == jet::Syntax::FORTRAN_MODULE_ROOT)
    {
        run_fortran_bind(&args[1..]);
        return;
    }
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        eprintln!(
            "usage: {} inspect bind <header.h> [--pkg <lib>] [--overlay <path>] [--link <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        );
        eprintln!(
            "       {} inspect bind <json|csv|sql|xml|proto> <input> [--type <Type>] [-o <output>]",
            jet::Syntax::BINARY_NAME
        );
        eprintln!();
        eprintln!("Generate a C binding cache from a header (S59). The output is");
        eprintln!("a `#Bindgen module c.<lib>.__bindgen__` file, by default written");
        eprintln!("to .jet/bindings/c/<lib>.jet. The compiler also runs this");
        eprintln!("automatically on a cache miss; `jet inspect bind` is the manual refresh.");
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        });
    }

    let header = args[0].as_str();
    let mut pkg: Option<String> = None;
    let mut out: Option<String> = None;
    let mut overlay_path: Option<String> = None;
    let mut links: Vec<String> = Vec::new();
    let mut quiet = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                let Some(value) = args.get(i + 1) else {
                    crate::cli_error!("E2102", "`inspect bind` requires a value after `--pkg`");
                    exit(ExitCodes::USAGE);
                };
                pkg = Some(value.to_string());
                i += 2;
            }
            "--overlay" => {
                let Some(value) = args.get(i + 1) else {
                    crate::cli_error!("E2102", "`inspect bind` requires a value after `--overlay`");
                    exit(ExitCodes::USAGE);
                };
                overlay_path = Some(value.to_string());
                i += 2;
            }
            "--link" => {
                let Some(value) = args.get(i + 1) else {
                    crate::cli_error!("E2102", "`inspect bind` requires a value after `--link`");
                    exit(ExitCodes::USAGE);
                };
                links.push(value.to_string());
                i += 2;
            }
            "-o" | "--out" => {
                let Some(value) = args.get(i + 1) else {
                    crate::cli_error!(
                        "E2102",
                        "`inspect bind` requires a value after `{}`",
                        args[i]
                    );
                    exit(ExitCodes::USAGE);
                };
                out = Some(value.to_string());
                i += 2;
            }
            // #1659 c3: --quiet suppresses the `bound …` status line; the
            // skipped-declaration notice stays (it is a warning, not status).
            "--quiet" => {
                quiet = true;
                i += 1;
            }
            other => {
                crate::cli_error!("E2102", "unknown `inspect bind` flag `{}`", other);
                eprintln!(
                    "usage: {} inspect bind <header.h> [--pkg <lib>] [--overlay <path>] [--link <lib>] [-o <out.jet>]",
                    jet::Syntax::BINARY_NAME
                );
                exit(ExitCodes::USAGE);
            }
        }
    }

    // Link key: --pkg if given, else the header basename (header→lib rule).
    let raw_lib = pkg.unwrap_or_else(|| {
        let base = header.rsplit('/').next().unwrap_or(header);
        base.strip_suffix(".h").unwrap_or(base).to_string()
    });
    let lib =
        jet::Syntax::sanitize_generated_name(&raw_lib, jet::Syntax::NameCase::Snake, "library");

    let header_src = match std::fs::read_to_string(header) {
        Ok(s) => s,
        Err(e) => bind_e3208(
            format!("Could not generate bindings from `{header}`."),
            format!("the header file could not be read ({e})."),
            "check the path, or install the library's dev headers.".to_string(),
        ),
    };
    let mut handle_overlay = match overlay_path.as_deref() {
        Some(path) => match read_c_bind_overlay(path, &lib) {
            Ok(overlay) => overlay,
            Err(reason) => bind_e3208(
                format!("Could not read C binding overlay `{path}`."),
                format!("{reason}."),
                format!("write `#Import module c.{lib} {{ … }}` in the overlay file"),
            ),
        },
        None => jet::CBind::HandleOverlay::default(),
    };
    for link in links {
        if !handle_overlay.links.contains(&link) {
            handle_overlay.links.push(link);
        }
    }

    // E2-M14 (owner 2026-06-18, supersedes D-CBIND3=B): native std-only backend.
    let result = match jet::CBind::generate_with_overlay(&header_src, &lib, &handle_overlay) {
        Ok(r) => r,
        Err(why) => bind_e3208(
            format!("Could not generate bindings from `{header}`."),
            format!("{why}."),
            format!("hand-write `#Import module c.{lib} {{ … }}` for the symbols you need."),
        ),
    };
    if !result.handle_skipped.is_empty() {
        let details = result
            .handle_skipped
            .iter()
            .map(|(handle, reason)| format!("`{handle}`: {reason}"))
            .collect::<Vec<_>>()
            .join("; ");
        bind_e3208(
            format!("Could not generate bindings from `{header}`."),
            format!("opaque handle bindings were unresolved: {details}."),
            format!(
                "add `#Close(close_function)` to `{}` in the overlay",
                overlay_path.as_deref().unwrap_or("<overlay>")
            ),
        );
    }

    // Default cache path follows D-CBIND7: .jet/bindings/c/<lib>.jet.
    let out_path =
        out.unwrap_or_else(|| format!(".jet/bindings/c/{}.{}", lib, jet::Syntax::FILE_EXT));
    if let Some(parent) = std::path::Path::new(&out_path).parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            crate::cli_error!("E2105", "could not create `{}`: {}", parent.display(), e);
            exit(ExitCodes::USER_ERROR);
        }
    }
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        crate::cli_error!("E2105", "could not write `{}`: {}", out_path, e);
        exit(ExitCodes::USER_ERROR);
    }

    // Phase 3 (D-CBIND2): write hash and typed handle/link metadata sidecars
    // alongside the cache so compiler identities retain binding provenance.
    // cflags are not yet threaded through `jet inspect bind`; pass "" for now.
    let _ = jet::CBind::write_bind_hash(std::path::Path::new(&out_path), &header_src, "");
    if let Err(error) = jet::CFFI::write_c_binding_metadata(
        std::path::Path::new(&out_path),
        &result.handles,
        &result.link_closure,
    ) {
        bind_e3208(
            format!("Could not publish C binding metadata for `{header}`."),
            format!("{error}."),
            "rerun `jet inspect bind` after checking the output path".to_string(),
        );
    }
    let project_root = std::env::current_dir().unwrap_or_else(|error| {
        bind_e3208(
            format!("Could not publish C binding provenance for `{header}`."),
            format!("the project root could not be resolved ({error})."),
            "rerun the bind command from the project root".to_string(),
        )
    });
    let header_path = std::fs::canonicalize(header).unwrap_or_else(|error| {
        bind_e3208(
            format!("Could not publish C binding provenance for `{header}`."),
            format!("the header path could not be resolved ({error})."),
            "check the header path and rerun `jet inspect bind`".to_string(),
        )
    });
    if let Err(error) = jet::CFFI::write_c_binding_provenance(
        &project_root,
        &lib,
        &header_path,
        std::path::Path::new(&out_path),
    ) {
        bind_e3208(
            format!("Could not publish C binding provenance for `{header}`."),
            format!("{error}."),
            "check the project and local library paths, then rerun `jet inspect bind`".to_string(),
        );
    }

    if !quiet {
        println!(
            "bound {} function{} from `{}` → {}",
            result.bound.len(),
            if result.bound.len() == 1 { "" } else { "s" },
            header,
            out_path
        );
    }
    if !result.skipped.is_empty() {
        println!(
            "skipped {} declaration{} outside the bindable subset (hand-write `#Import` for these):",
            result.skipped.len(),
            if result.skipped.len() == 1 { "" } else { "s" }
        );
        for (name, why) in &result.skipped {
            println!("  - {} — {}", name, why);
        }
    }
}

fn binding_plan_command(args: &[&String]) -> bool {
    args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--shape" | "--freeze" | "--policy" | "--accept" | "--update" | "--explain"
        )
    }) || args.first().is_some_and(|arg| arg.as_str() == "--policy")
}

fn run_binding_plan_command(args: &[&String]) {
    let mut name: Option<String> = None;
    let mut shape = jet::Bindgen::BindingShape::Automatic;
    let mut policy: Option<jet::Bindgen::BindingPolicy> = None;
    let mut explain = false;
    let mut freeze = false;
    let mut update = false;
    let mut preview = false;
    let mut accept: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--shape" => {
                let Some(value) = args.get(i + 1) else {
                    crate::cli_error!("E2102", "`bind` requires a value after `--shape`");
                    exit(ExitCodes::USAGE);
                };
                shape = jet::Bindgen::BindingShape::parse(value).unwrap_or_else(|error| {
                    crate::cli_error!("E2102", "{error}");
                    exit(ExitCodes::USAGE);
                });
                i += 2;
            }
            "--policy" => {
                let Some(value) = args.get(i + 1) else {
                    crate::cli_error!("E2102", "`bind` requires a value after `--policy`");
                    exit(ExitCodes::USAGE);
                };
                policy = Some(
                    jet::Bindgen::BindingPolicy::parse(value).unwrap_or_else(|error| {
                        crate::cli_error!("E2102", "{error}");
                        exit(ExitCodes::USAGE);
                    }),
                );
                i += 2;
            }
            "--explain" => {
                explain = true;
                i += 1;
            }
            "--freeze" => {
                freeze = true;
                i += 1;
            }
            "--update" => {
                update = true;
                i += 1;
            }
            "--preview" => {
                preview = true;
                i += 1;
            }
            "--accept" => {
                let Some(value) = args.get(i + 1) else {
                    crate::cli_error!("E2102", "`bind` requires a value after `--accept`");
                    exit(ExitCodes::USAGE);
                };
                accept = Some(value.to_string());
                update = true;
                i += 2;
            }
            arg if arg.starts_with('-') => {
                crate::cli_error!("E2102", "unknown `bind` flag `{arg}`");
                exit(ExitCodes::USAGE);
            }
            value => {
                if name.replace(value.to_string()).is_some() {
                    crate::cli_error!("E2102", "`bind` accepts one binding name");
                    exit(ExitCodes::USAGE);
                }
                i += 1;
            }
        }
    }

    let package_path = Path::new("package.jet");
    if let Some(policy) = policy {
        if name.is_none() {
            if let Err(error) =
                update_binding_package_config(package_path, None, None, Some(policy))
            {
                crate::cli_error!("E2105", "{error}");
                exit(ExitCodes::USER_ERROR);
            }
            println!("binding policy: {policy}");
            return;
        }
    }
    let Some(name) = name else {
        crate::cli_error!(
            "E2102",
            "usage: jet bind <name> [--shape automatic|native] [--freeze]"
        );
        exit(ExitCodes::USAGE);
    };

    let Some(header) = binding_header_path(&name) else {
        if explain {
            if let Some(record) = read_binding_plan(&name) {
                print!("{record}");
                return;
            }
        }
        crate::cli_error!(
            "E3208",
            "binding `{name}` has no pinned foreign header; pass an exact header through the project binding record"
        );
        exit(ExitCodes::USER_ERROR);
    };
    let header_source = fs::read_to_string(&header).unwrap_or_else(|error| {
        crate::cli_error!(
            "E3208",
            "could not read binding header `{}`: {error}",
            header.display()
        );
        exit(ExitCodes::USER_ERROR);
    });
    let lib = jet::Syntax::sanitize_generated_name(&name, jet::Syntax::NameCase::Snake, "library");
    let generated = jet::CBind::generate_with_overlay(
        &header_source,
        &lib,
        &jet::CBind::HandleOverlay::default(),
    )
    .unwrap_or_else(|error| {
        crate::cli_error!("E3208", "could not generate binding `{name}`: {error}");
        exit(ExitCodes::USER_ERROR);
    });
    let contract = annotate_binding_contract(generated.boundary, &header_source, &header)
        .unwrap_or_else(|error| {
            crate::cli_error!(
                "E3208",
                "binding `{name}` has invalid canonical evidence: {error}"
            );
            exit(ExitCodes::USER_ERROR);
        });
    let operation = binding_operation_from_header(&name, &header_source);
    let target = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
    let inputs = jet::Bindgen::BindingInputs::new(
        target,
        "jet-bindgen-v1",
        format!(
            "{}:sha256-{}",
            header.display(),
            jet::SHA256::sha256_hex(header_source.as_bytes())
        ),
    )
    .with_dependencies(std::iter::empty::<String>())
    .with_compiler_flags(std::iter::empty::<String>());
    let plan = jet::Bindgen::BindingPlan::resolve(&contract, operation, inputs, shape)
        .unwrap_or_else(|error| {
            crate::cli_error!("E3208", "{error}");
            exit(ExitCodes::USER_ERROR);
        });
    if let Some(recorded) = read_binding_plan(&name) {
        if policy == Some(jet::Bindgen::BindingPolicy::Frozen) || freeze {
            if let Err(error) = plan.frozen_drift(recorded_digest(&recorded)) {
                crate::cli_error!("E3208", "{error}");
                exit(ExitCodes::USER_ERROR);
            }
        }
    }
    if explain {
        print!("{}", plan.explain());
        return;
    }
    if preview {
        print!("{}", plan.explain());
        println!("preview: no package, lock, or generated facade files were written");
        return;
    }
    if let Some(expected) = accept {
        if expected != plan.candidate_digest() {
            crate::cli_error!(
                "E3208",
                "`--accept` digest `{expected}` does not match candidate `{}`",
                plan.candidate_digest()
            );
            exit(ExitCodes::USER_ERROR);
        }
    } else if update && !freeze && policy != Some(jet::Bindgen::BindingPolicy::Frozen) {
        // `--update` without `--accept` is intentionally a preview-like refusal:
        // accepting a new identity must name the exact candidate digest.
        crate::cli_error!(
            "E3208",
            "`--update` requires `--preview` or `--accept {}`",
            plan.candidate_digest()
        );
        exit(ExitCodes::USER_ERROR);
    }
    let mut selected_plans = BTreeMap::new();
    selected_plans.insert(name.clone(), plan.clone());
    let projected = jet::CBind::generate_with_contract_and_plans(
        &header_source,
        &lib,
        &jet::CBind::HandleOverlay::default(),
        contract,
        &selected_plans,
    )
    .unwrap_or_else(|error| {
        crate::cli_error!(
            "E3208",
            "could not materialize binding facade `{name}`: {error}"
        );
        exit(ExitCodes::USER_ERROR);
    });
    if let Err(error) = write_binding_cache(&name, &projected.source) {
        crate::cli_error!("E2105", "{error}");
        exit(ExitCodes::USER_ERROR);
    }
    if let Err(error) = write_binding_plan(&name, &plan) {
        crate::cli_error!("E2105", "{error}");
        exit(ExitCodes::USER_ERROR);
    }
    if let Err(error) = update_binding_package_config(
        package_path,
        Some(&name),
        Some(shape),
        policy.or_else(|| freeze.then_some(jet::Bindgen::BindingPolicy::Frozen)),
    ) {
        crate::cli_error!("E2105", "{error}");
        exit(ExitCodes::USER_ERROR);
    }
    if let Err(error) = write_binding_facade(&name, &plan) {
        crate::cli_error!("E2105", "{error}");
        exit(ExitCodes::USER_ERROR);
    }
    println!("binding plan {} ({})", name, plan.candidate_digest());
}

fn binding_header_path(name: &str) -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(name),
        PathBuf::from(format!("{name}.h")),
        PathBuf::from(format!("include/{name}.h")),
        PathBuf::from(format!("ffi/{name}.h")),
        PathBuf::from(format!(".jet/ffi/{name}.h")),
    ];
    candidates.into_iter().find(|path| path.is_file())
}

fn marker_value(source: &str, marker: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let value = line.split_once(marker)?.1.trim();
        let value = value.strip_prefix(':').unwrap_or(value).trim();
        (!value.is_empty()).then(|| value.trim_matches('"').to_string())
    })
}

fn binding_operation_from_header(name: &str, source: &str) -> jet::Bindgen::BindingOperation {
    let declaration = source
        .lines()
        .map(str::trim)
        .find(|line| line.contains(&format!("{name}(")))
        .unwrap_or("");
    let native_signature = declaration.trim_end_matches(';').to_string();
    let result_type = declaration
        .split_once(&format!("{name}("))
        .map(|(prefix, _)| {
            if prefix.contains("char") && prefix.contains('*') {
                return "String".to_string();
            }
            prefix
                .split_whitespace()
                .last()
                .unwrap_or("Unit")
                .replace("const ", "")
        })
        .filter(|value| !value.is_empty())
        .map(|value| match value.as_str() {
            "void" => "Unit".to_string(),
            "uint64_t" | "size_t" => "U64".to_string(),
            "int" | "int32_t" => "Int".to_string(),
            "float" | "double" => "Float".to_string(),
            _ => value,
        })
        .unwrap_or_else(|| "Unit".to_string());
    let mut operation = jet::Bindgen::BindingOperation::new(name, native_signature, result_type);
    if let Some((_, params)) = declaration.split_once(&format!("{name}(")) {
        if let Some(params) = params.split_once(')').map(|(params, _)| params) {
            let parameters = params.split(',').map(str::trim).collect::<Vec<_>>();
            let pointer = parameters
                .iter()
                .find(|parameter| parameter.contains('*'))
                .and_then(|parameter| parameter.split_whitespace().last())
                .map(|value| value.trim_start_matches('*').to_string());
            let count = parameters
                .iter()
                .find(|parameter| {
                    if parameter.contains('*') {
                        return false;
                    }
                    let lower = parameter.to_ascii_lowercase();
                    lower.contains("count")
                        || lower.contains("length")
                        || lower.contains("len")
                        || lower == "n"
                        || lower.ends_with(" n")
                })
                .and_then(|parameter| parameter.split_whitespace().last())
                .map(str::to_string);
            if let (Some(pointer), Some(count)) = (pointer, count) {
                let unit = match marker_value(source, "jet-ffi-count-unit").as_deref() {
                    Some("bytes") => jet::Bindgen::CountUnit::Bytes,
                    Some(value) if value.starts_with("elements:") => {
                        jet::Bindgen::CountUnit::Elements(value[9..].to_string())
                    }
                    _ => jet::Bindgen::CountUnit::Unknown,
                };
                let meaning = match marker_value(source, "jet-ffi-count-meaning").as_deref() {
                    Some("full-extent") => jet::Bindgen::CountMeaning::FullExtent,
                    Some("prefix") => jet::Bindgen::CountMeaning::Prefix,
                    _ => jet::Bindgen::CountMeaning::Unknown,
                };
                let width = marker_value(source, "jet-ffi-count-width")
                    .and_then(|value| value.parse::<u16>().ok())
                    .unwrap_or(0);
                let retention = match marker_value(source, "jet-ffi-retention").as_deref() {
                    Some("borrowed-for-call") => jet::Bindgen::PointerRetention::BorrowedForCall,
                    Some("may-retain") => jet::Bindgen::PointerRetention::MayRetain,
                    _ => jet::Bindgen::PointerRetention::Unknown,
                };
                operation = operation.with_pointer_count(jet::Bindgen::PointerCountFact::new(
                    pointer, count, unit, meaning, width, retention,
                ));
            }
        }
    }
    operation.nullable_return = source.contains("jet-ffi-nullable");
    operation.borrowed_view = source.contains("jet-ffi-borrowed-view");
    operation.status_out = source.contains("jet-ffi-status-out");
    operation.partial_success = source.contains("jet-ffi-partial-success");
    operation.fallible_close = source.contains("jet-ffi-fallible-close");
    operation.callback_transport =
        marker_value(source, "jet-ffi-callback").unwrap_or_else(|| "none".to_string());
    operation.effects =
        marker_value(source, "jet-ffi-effects").unwrap_or_else(|| "foreign".to_string());
    operation.ownership = marker_value(source, "jet-ffi-ownership")
        .unwrap_or_else(|| "signature-declared".to_string());
    operation.failure_mapping =
        marker_value(source, "jet-ffi-failure").unwrap_or_else(|| "preserve".to_string());
    operation.copies = marker_value(source, "jet-ffi-copies").unwrap_or_else(|| "none".to_string());
    operation.placement =
        marker_value(source, "jet-ffi-placement").unwrap_or_else(|| "caller".to_string());
    operation.cleanup =
        marker_value(source, "jet-ffi-cleanup").unwrap_or_else(|| "none".to_string());
    operation
}

fn annotate_binding_contract(
    mut contract: jet::ForeignBridge::ForeignBoundaryContract,
    source: &str,
    header: &Path,
) -> Result<jet::ForeignBridge::ForeignBoundaryContract, String> {
    let target = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
    let coverage = jet::ForeignBridge::ForeignArtifactCoverage::new(
        format!(
            "{}:sha256-{}",
            header.display(),
            jet::SHA256::sha256_hex(source.as_bytes())
        ),
        target,
        "jet-bindgen-v1",
    )
    .with_transitive_dependencies(std::iter::empty::<String>())
    .with_reachable_callbacks(std::iter::empty::<String>())
    .with_compiler_flags(std::iter::empty::<String>());
    contract = contract.with_artifact_coverage(coverage);
    for line in source.lines().filter_map(|line| {
        line.split_once("jet-ffi-obligation:")
            .map(|(_, value)| value.trim())
    }) {
        let mut obligation = None;
        let mut basis = jet::ForeignBridge::ForeignEvidenceBasis::Unknown;
        let mut checker = String::new();
        let mut assumptions = Vec::new();
        for field in line.split(';') {
            let Some((key, value)) = field.split_once('=') else {
                continue;
            };
            match key.trim() {
                "name" | "obligation" => obligation = Some(value.trim().to_string()),
                "basis" => {
                    basis = match value.trim() {
                        "proved" => jet::ForeignBridge::ForeignEvidenceBasis::Proved,
                        "enforced" => jet::ForeignBridge::ForeignEvidenceBasis::Enforced,
                        "contained" => jet::ForeignBridge::ForeignEvidenceBasis::Contained,
                        "trusted" => jet::ForeignBridge::ForeignEvidenceBasis::Trusted,
                        "unknown" => jet::ForeignBridge::ForeignEvidenceBasis::Unknown,
                        other => return Err(format!("unknown evidence basis `{other}`")),
                    }
                }
                "checker" => checker = value.trim().to_string(),
                "assumptions" => {
                    assumptions = value
                        .split('|')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect()
                }
                _ => {}
            }
        }
        let Some(obligation) = obligation else {
            return Err("an obligation marker needs `name=`".to_string());
        };
        contract.set_obligation(obligation, basis, checker, assumptions)?;
    }
    Ok(contract)
}

fn binding_plan_path(name: &str) -> PathBuf {
    PathBuf::from(format!(".jet/lock/bindings/{name}.plan"))
}

fn read_binding_plan(name: &str) -> Option<String> {
    fs::read_to_string(binding_plan_path(name)).ok()
}

fn recorded_digest(record: &str) -> &str {
    record
        .lines()
        .find_map(|line| line.strip_prefix("digest="))
        .unwrap_or("")
}

fn write_binding_plan(name: &str, plan: &jet::Bindgen::BindingPlan) -> Result<(), String> {
    let path = binding_plan_path(name);
    let Some(parent) = path.parent() else {
        return Err("binding lock path has no parent".to_string());
    };
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create `{}`: {error}", parent.display()))?;
    let mut record = format!(
        "schema=jet-ffi-binding-plan-v1\nname={name}\ndigest={}\nshape={}\nboundary={}\ninputs={}\nartifact={}\n",
        plan.digest, plan.shape, plan.boundary_digest, plan.input_digest, plan.artifact
    );
    record.push_str(&plan.explain());
    record.push_str("\nfacade:\n");
    record.push_str(&plan.render_facade());
    fs::write(path, record).map_err(|error| format!("could not write binding lock: {error}"))
}

fn write_binding_facade(name: &str, plan: &jet::Bindgen::BindingPlan) -> Result<(), String> {
    let path = PathBuf::from(format!(".jet/bindings/c/{name}.adapted.jet"));
    let Some(parent) = path.parent() else {
        return Err("binding facade path has no parent".to_string());
    };
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create `{}`: {error}", parent.display()))?;
    fs::write(path, plan.render_facade())
        .map_err(|error| format!("could not write generated binding facade: {error}"))
}

fn write_binding_cache(name: &str, source: &str) -> Result<(), String> {
    let path = PathBuf::from(format!(".jet/bindings/c/{name}.jet"));
    let Some(parent) = path.parent() else {
        return Err("binding cache path has no parent".to_string());
    };
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create `{}`: {error}", parent.display()))?;
    fs::write(path, source)
        .map_err(|error| format!("could not write generated binding cache: {error}"))
}

fn update_binding_package_config(
    package_path: &Path,
    name: Option<&str>,
    shape: Option<jet::Bindgen::BindingShape>,
    policy: Option<jet::Bindgen::BindingPolicy>,
) -> Result<(), String> {
    let mut source = fs::read_to_string(package_path).unwrap_or_default();
    if let (Some(name), Some(shape)) = (name, shape) {
        if !source.contains("bindings:") {
            source.push_str("\n\nbindings: .{\n");
            source.push_str(&format!(
                "    {name}: .{{ shape: .{}, frozen: {} }}\n",
                match shape {
                    jet::Bindgen::BindingShape::Automatic => "Automatic",
                    jet::Bindgen::BindingShape::Native => "Native",
                },
                policy == Some(jet::Bindgen::BindingPolicy::Frozen)
            ));
            source.push_str("}\n");
        } else if !source.contains(&format!("{name}:")) {
            source.push_str(&format!(
                "\n// binding plan: {name} shape={}\n",
                shape.as_str()
            ));
        }
    }
    if let Some(policy) = policy {
        if !source.contains("bindings: .{") || !source.contains("policy:") {
            source.push_str(&format!(
                "\n\npolicy: .{{ bindings: .{} }}\n",
                match policy {
                    jet::Bindgen::BindingPolicy::Automatic => "Automatic",
                    jet::Bindgen::BindingPolicy::Frozen => "Frozen",
                }
            ));
        }
    }
    if source.trim().is_empty() {
        return Err(format!(
            "`{}` is not a package manifest",
            package_path.display()
        ));
    }
    fs::write(package_path, source)
        .map_err(|error| format!("could not write `{}`: {error}", package_path.display()))
}

fn read_c_bind_overlay(path: &str, lib: &str) -> Result<jet::CBind::HandleOverlay, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("the overlay file could not be read ({error})"))?;
    let (tokens, lex_diagnostics) = jet::Lexer::lex(&source);
    if !lex_diagnostics.is_empty() {
        return Err(format!(
            "the overlay has lexer diagnostics: {lex_diagnostics:#?}"
        ));
    }
    let program = jet::Parser::parse_with_source(&tokens, &source)
        .map_err(|diagnostics| format!("the overlay has parser diagnostics: {diagnostics:#?}"))?;
    let mut overlay = jet::CBind::HandleOverlay::default();
    let mut found = false;
    for item in &program.items {
        let jet::AST::Item::CModule(c_module) = item else {
            continue;
        };
        if c_module.kind != jet::AST::CModuleKind::Extern {
            continue;
        }
        if c_module.lib != lib {
            return Err(format!(
                "the overlay declares `c.{}` but this bind targets `c.{lib}`",
                c_module.lib
            ));
        }
        found = true;
        for function in &c_module.functions {
            let Some((close, _)) = &function.close else {
                continue;
            };
            let Some(jet::AST::Type::Named(handle)) = &function.return_type else {
                continue;
            };
            overlay
                .close_functions
                .insert(handle.clone(), close.clone());
        }
    }
    if !found {
        return Err(format!(
            "the overlay has no `#Import module c.{lib} {{ … }}` block"
        ));
    }
    Ok(overlay)
}

fn bind_e3208(what: String, why: String, fix: String) -> ! {
    crate::emit_cli_report("E3208", what, why, fix, false);
    exit(ExitCodes::USER_ERROR);
}

fn run_data_bind(format: &str, args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind {} <input> [--type <Type>] [-o <output>]",
            jet::Syntax::BINARY_NAME,
            format
        );
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        });
    }
    let input_path = args[0].as_str();
    let mut root_type = None;
    let mut output = None;
    let mut index = 1usize;
    while index < args.len() {
        match args[index].as_str() {
            "--type" => {
                let Some(value) = args.get(index + 1) else {
                    crate::cli_error!(
                        "E2102",
                        "inspect bind {format} requires a value after --type"
                    );
                    usage();
                    exit(ExitCodes::USAGE);
                };
                if value.is_empty() {
                    crate::cli_error!("E2102", "inspect bind {format} type name cannot be empty");
                    usage();
                    exit(ExitCodes::USAGE);
                }
                root_type = Some(value.to_string());
                index += 2;
            }
            "-o" | "--out" => {
                let Some(value) = args.get(index + 1) else {
                    crate::cli_error!("E2102", "inspect bind {format} requires a value after -o");
                    usage();
                    exit(ExitCodes::USAGE);
                };
                if value.is_empty() {
                    crate::cli_error!("E2102", "inspect bind {format} output path cannot be empty");
                    usage();
                    exit(ExitCodes::USAGE);
                }
                output = Some(value.to_string());
                index += 2;
            }
            other => {
                crate::cli_error!("E2102", "unknown inspect bind {format} argument {other}");
                usage();
                exit(ExitCodes::USAGE);
            }
        }
    }
    let input = match fs::read_to_string(input_path) {
        Ok(input) => input,
        Err(error) => data_bind_io_error(
            format,
            input_path,
            "read",
            &format!("the input could not be read ({error})"),
        ),
    };
    let default_output = output.is_none();
    let output_path = output.clone().unwrap_or_else(|| {
        let base = input_path
            .rsplit(|ch| ch == '/' || ch == '\\')
            .next()
            .unwrap_or(input_path);
        let stem = base.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(base);
        let safe =
            jet::Syntax::sanitize_generated_name(stem, jet::Syntax::NameCase::Snake, "schema");
        format!("bindings/{safe}.{}", jet::Syntax::FILE_EXT)
    });
    let mut command = vec![
        jet::Syntax::BINARY_NAME.to_string(),
        "inspect".to_string(),
        "bind".to_string(),
        format.to_string(),
        input_path.to_string(),
    ];
    if let Some(root_type) = &root_type {
        command.push("--type".to_string());
        command.push(root_type.clone());
    }
    if let Some(output) = &output {
        command.push("-o".to_string());
        command.push(output.clone());
    }
    let command = command.join(" ");
    let result =
        match jet::CBind::generate_data(format, input_path, &input, root_type.as_deref(), &command)
        {
            Ok(result) => result,
            Err(error) => data_bind_error(format, input_path, &error),
        };
    if default_output {
        match fs::read_to_string(&output_path) {
            Ok(existing) => {
                let generated_command = result
                    .source
                    .lines()
                    .find(|line| line.starts_with("// generated by: "));
                let existing_command = existing
                    .lines()
                    .find(|line| line.starts_with("// generated by: "));
                if generated_command != existing_command {
                    data_bind_output_collision(format, input_path, &output_path);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => data_bind_io_error(
                format,
                &output_path,
                "inspect",
                &format!("the existing output could not be checked ({error})"),
            ),
        }
    }
    if let Some(parent) = Path::new(&output_path).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(error) = fs::create_dir_all(parent) {
                data_bind_io_error(
                    format,
                    &output_path,
                    "create",
                    &format!(
                        "could not create output directory {} ({error})",
                        parent.display()
                    ),
                );
            }
        }
    }
    if let Err(error) = fs::write(&output_path, result.source) {
        data_bind_io_error(
            format,
            &output_path,
            "write",
            &format!("could not write {output_path} ({error})"),
        );
    }
    println!(
        "bound {} {} record{} from {input_path} → {output_path}",
        result.record_count,
        format,
        if result.record_count == 1 { "" } else { "s" }
    );
}

fn data_bind_error(format: &str, path: &str, why: &str) -> ! {
    bind_e3208(
        format!("Could not generate {format} bindings from {path}."),
        format!("{why}."),
        format!("provide a well-formed {format} schema and rerun `jet inspect bind {format}`."),
    )
}

fn data_bind_io_error(format: &str, path: &str, operation: &str, why: &str) -> ! {
    crate::emit_cli_report(
        "E2105",
        format!("Could not {operation} {format} bindings at {path}."),
        format!("{why}."),
        format!("check the input and output paths, permissions, and available disk space, then rerun `jet inspect bind {format}`."),
        false,
    );
    exit(ExitCodes::USER_ERROR);
}

fn data_bind_output_collision(format: &str, input_path: &str, output_path: &str) -> ! {
    crate::emit_cli_report(
        "E2104",
        format!("the default {format} binding output `{output_path}` is already in use"),
        format!("sanitizing `{input_path}` produces the same output name as an existing binding"),
        format!("rerun with `-o <different-output>` to choose an explicit path"),
        false,
    );
    exit(ExitCodes::USER_ERROR);
}

fn run_cpp_bind(args: &[&String]) {
    let usage = || {
        eprintln!("usage: {} inspect bind cpp <header.hpp> --target <triple> --clang <absolute-path> --ar <absolute-path> [--pkg <lib>] [--namespace <name>] [--instantiate <qualified=type:jet-name>] [-I <dir>] [-L <dir>] [-l <lib>] [-o <out.jet>]", jet::Syntax::BINARY_NAME)
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        });
    }
    let header = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut target = None;
    let mut clang = None;
    let mut archiver = None;
    let mut include_dirs = Vec::new();
    let mut library_dirs = Vec::new();
    let mut libraries = Vec::new();
    let mut namespaces = Vec::new();
    let mut templates = Vec::new();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--pkg" => {
                pkg = args.get(index + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                index += 2;
            }
            "-o" | "--out" => {
                out = args.get(index + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                index += 2;
            }
            "--target" => {
                target = args.get(index + 1).map(|v| v.to_string());
                if target.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                index += 2;
            }
            "--clang" => {
                clang = args.get(index + 1).map(|v| v.to_string());
                if clang.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                index += 2;
            }
            "--ar" => {
                archiver = args.get(index + 1).map(|v| v.to_string());
                if archiver.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                index += 2;
            }
            "-I" => {
                let Some(value) = args.get(index + 1) else {
                    usage();
                    exit(ExitCodes::USAGE);
                };
                include_dirs.push(std::path::PathBuf::from(value.as_str()));
                index += 2;
            }
            "-L" => {
                let Some(value) = args.get(index + 1) else {
                    usage();
                    exit(ExitCodes::USAGE);
                };
                library_dirs.push(std::path::PathBuf::from(value.as_str()));
                index += 2;
            }
            "-l" => {
                let Some(value) = args.get(index + 1) else {
                    usage();
                    exit(ExitCodes::USAGE);
                };
                libraries.push(value.to_string());
                index += 2;
            }
            "--namespace" => {
                let Some(value) = args.get(index + 1) else {
                    usage();
                    exit(ExitCodes::USAGE);
                };
                namespaces.push(value.to_string());
                index += 2;
            }
            "--instantiate" => {
                let Some(value) = args.get(index + 1) else {
                    usage();
                    exit(ExitCodes::USAGE);
                };
                let Some((qualified_name, rest)) = value.split_once('=') else {
                    usage();
                    exit(ExitCodes::USAGE);
                };
                let Some((cpp_types, jet_name)) = rest.rsplit_once(':') else {
                    usage();
                    exit(ExitCodes::USAGE);
                };
                templates.push(jet::CppBind::TemplateInstantiation {
                    qualified_name: qualified_name.to_string(),
                    cpp_args: cpp_types.split(',').map(str::to_string).collect(),
                    jet_name: jet_name.to_string(),
                });
                index += 2;
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind cpp` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE);
            }
        }
    }
    let (Some(target), Some(clang), Some(archiver)) = (target, clang, archiver) else {
        usage();
        exit(ExitCodes::USAGE);
    };
    let lib = pkg.unwrap_or_else(|| {
        let base = header.rsplit('/').next().unwrap_or(header);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::CPP_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let canonicalize = |path: std::path::PathBuf| {
        std::fs::canonicalize(&path).unwrap_or_else(|error| {
            cpp_bind_error(
                header,
                &format!(
                    "could not resolve selected native path `{}` ({error})",
                    path.display()
                ),
            )
        })
    };
    let options = jet::CppBind::BindOptions {
        lib: lib.clone(),
        target,
        clang: canonicalize(std::path::PathBuf::from(clang)),
        archiver: canonicalize(std::path::PathBuf::from(archiver)),
        include_dirs: include_dirs.into_iter().map(canonicalize).collect(),
        library_dirs: library_dirs.into_iter().map(canonicalize).collect(),
        libraries,
        namespaces,
        templates,
    };
    let result = jet::CppBind::bind(std::path::Path::new(header), cache, &options)
        .unwrap_or_else(|error| cpp_bind_error(header, &error.to_string()));
    if let Err(error) = std::fs::write(&out_path, &result.source) {
        cpp_bind_error(
            header,
            &format!("the generated cache could not be written ({error})"),
        );
    }
    if let Err(error) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance)
    {
        cpp_bind_error(
            header,
            &format!("the provenance could not be written ({error})"),
        );
    }
    println!(
        "bound {} C++ member{} from `{header}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}

fn cpp_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(
        format!("Could not generate C++ bindings from `{path}`."),
        format!("{why}."),
        "select an explicit target/toolchain, expose public scalar declarations, and request templates with `--instantiate`, then rerun `jet inspect bind cpp`.".to_string(),
    )
}

fn run_tcl_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind tcl <script.tcl> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind tcl` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let b = path.rsplit('/').next().unwrap_or(path);
        b.rsplit_once('.').map(|v| v.0).unwrap_or(b).to_string()
    });
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| tcl_bind_error(path, &format!("the script could not be read ({e})")));
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::TCL_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::TclBind::bind(&source, &lib, cache)
        .unwrap_or_else(|e| tcl_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        tcl_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(
        cache.join(format!("{lib}.tcl-path")),
        format!("{}\n", result.lib_dir.display()),
    ) {
        tcl_bind_error(
            path,
            &format!("the Tcl runtime identity could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), result.provenance) {
        tcl_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!("bound in-process Tcl session from `{path}` → {out_path}")
}
fn tcl_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"use a valid Tcl initialization script and rerun `jet inspect bind tcl` inside the provisioned Jet environment.".to_string())
}

fn run_lua_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind lua <script.lua> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind lua` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let b = path.rsplit('/').next().unwrap_or(path);
        b.rsplit_once('.').map(|v| v.0).unwrap_or(b).to_string()
    });
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| lua_bind_error(path, &format!("the script could not be read ({e})")));
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::LUA_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::LuaBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| lua_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        lua_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(
        cache.join(format!("{lib}.lua-path")),
        format!("{}\n", result.lib_dir.display()),
    ) {
        lua_bind_error(
            path,
            &format!("the Lua runtime identity could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), result.provenance) {
        lua_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} in-process Lua function{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn lua_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level `function name(input)` routines and rerun `jet inspect bind lua` inside the provisioned Jet environment.".to_string())
}

/// D-FFI-ADA1=A: compile exported GNAT functions and preserve scalar subtype
/// ranges as checked Jet wrapper boundaries.
fn run_ada_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind ada <package.ads> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind ada` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let spec = std::fs::read_to_string(path).unwrap_or_else(|e| {
        ada_bind_error(path, &format!("the package spec could not be read ({e})"))
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::ADA_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::AdaBind::bind(std::path::Path::new(path), &spec, &lib, cache)
        .unwrap_or_else(|e| ada_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        ada_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(
        cache.join(format!("{lib}.ada-path")),
        format!("{}\n", result.runtime_dir.display()),
    ) {
        ada_bind_error(
            path,
            &format!("the GNAT runtime identity could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), result.provenance) {
        ada_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} GNAT export{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn ada_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"export `Interfaces.C.long_long` or `Interfaces.C.double` functions with `Convention => C` and `External_Name`, then rerun `jet inspect bind ada`.".to_string())
}

/// D-FFI-PASCAL1=A: compile FreePascal cdecl exports and generate bounded,
/// consuming opaque-handle wrappers for one Object Pascal class estate.
fn run_pascal_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind pascal <library.pas> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind pascal` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        pascal_bind_error(path, &format!("the Pascal source could not be read ({e})"))
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::PASCAL_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::PascalBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| pascal_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        pascal_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), result.provenance) {
        pascal_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} FreePascal export{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn pascal_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"export `Int64`/`Double` cdecl routines plus `<class>_new`, `<class>_free`, and pointer-first scalar method wrappers, then rerun `jet inspect bind pascal`.".to_string())
}

/// D-FFI-DART1=A: Dart/Flutter owns the isolate. Generate and compile the
/// dart_api_dl callback bridge plus a native Jet plugin loaded by `dart:ffi`.
fn run_dart_bind(args: &[&String]) {
    let usage = || {
        eprintln!("usage: {} inspect bind dart <contract.dart> --jet <compute.jet> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME)
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut compute = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "--jet" => {
                compute = args.get(i + 1).map(|v| v.to_string());
                if compute.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind dart` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let Some(compute) = compute else {
        crate::cli_error!("E2104", "`inspect bind dart` requires `--jet <compute.jet>` so the Dart host has real native Jet code to load");
        usage();
        exit(ExitCodes::USAGE)
    };
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        dart_bind_error(path, &format!("the Dart contract could not be read ({e})"))
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::DART_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let host = cache.join(format!("{lib}_host.dart"));
    let archive = cache.join(format!("libjet_dart_{lib}.a"));
    let provenance = cache.join(format!("{lib}.provenance"));
    let compute_artifact = cache.join(format!("{lib}_compute.rs"));
    let clear_outputs = || {
        let _ = std::fs::remove_file(&out_path);
        let _ = std::fs::remove_file(&host);
        let _ = std::fs::remove_file(&provenance);
        let _ = std::fs::remove_file(&compute_artifact);
        for ext in ["so", "dylib", "dll"] {
            let _ = std::fs::remove_file(cache.join(format!("libjet_dart_{lib}_compute.{ext}")));
        }
    };
    clear_outputs();
    let _ = std::fs::remove_file(&archive);
    let cleanup = || {
        clear_outputs();
        let _ = std::fs::remove_file(&archive);
    };
    let mut result = jet::DartBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| {
            cleanup();
            dart_bind_error(path, &e.to_string())
        });
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        cleanup();
        dart_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(&host, &result.host_source) {
        cleanup();
        dart_bind_error(
            path,
            &format!("the Dart host wrapper could not be written ({e})"),
        )
    }
    let compute_source = std::fs::read_to_string(&compute).unwrap_or_else(|e| {
        cleanup();
        dart_bind_error(
            path,
            &format!("the Jet compute source could not be read ({e})"),
        )
    });
    result.provenance = jet::DartBind::bind_compute_provenance(
        &result.provenance,
        std::path::Path::new(&compute),
        &compute_source,
    )
    .unwrap_or_else(|e| {
        cleanup();
        dart_bind_error(path, &e.to_string())
    });
    let compiled =
        jet::Driver::compile_bundle_path_opts_plugin(&compute, jet::Sema::CompileMode::Check, None)
            .unwrap_or_else(|_| {
                cleanup();
                dart_bind_error(
                    path,
                    "the Jet compute source did not pass Jet front-end checks",
                )
            });
    let plugin = compiled.plugin.as_ref().unwrap_or_else(|| {
        cleanup();
        dart_bind_error(
            path,
            "the Jet compute source produced no native plugin export artifact",
        )
    });
    let native = jet::DartBind::build_compute(
        &plugin.guest_rust,
        &result.host_rust,
        compiled.ffi.as_ref(),
        &compiled.clinks,
        &lib,
        cache,
    )
    .unwrap_or_else(|e| {
        cleanup();
        dart_bind_error(path, &e.to_string())
    });
    if let Err(e) = std::fs::write(&provenance, &result.provenance) {
        cleanup();
        dart_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} Dart callback{} and native Jet compute `{}` from `{}` → {}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" },
        native.display(),
        path,
        out_path
    );
}
fn dart_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"mark top-level scalar Dart callbacks with `@pragma('vm:entry-point')`, pass a valid Jet plugin source with `--jet`, and rerun inside the provisioned Jet environment.".to_string())
}

/// D-FFI-PWSH1=A: validate named script functions, then generate a persistent
/// PowerShell worker whose object pipeline crosses as canonical DataTree.
fn run_powershell_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind pwsh <script.ps1> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind pwsh` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        powershell_bind_error(
            path,
            &format!("the PowerShell script could not be read ({e})"),
        )
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::PWSH_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::PowerShellBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| powershell_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        powershell_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        powershell_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} persistent PowerShell function{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn powershell_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define named PowerShell functions with Jet-compatible identifiers and rerun `jet inspect bind pwsh` inside the provisioned Jet environment.".to_string())
}

/// D-FFI-PERL1=A: compile named Perl subs into a supervised persistent worker.
fn run_perl_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind perl <script.pl> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind perl` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        perl_bind_error(path, &format!("the Perl script could not be read ({e})"))
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::PERL_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::PerlBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| perl_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        perl_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        perl_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} persistent Perl function{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn perl_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define named main-package Perl functions with Jet-compatible identifiers and rerun `jet inspect bind perl` inside the provisioned Jet environment.".to_string())
}

/// D-FFI-RUBY1=A: statically discover top-level methods and generate a
/// persistent supervised Ruby worker. Ruby source never runs during discovery.
fn run_ruby_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind ruby <script.rb> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind ruby` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        ruby_bind_error(path, &format!("the Ruby script could not be read ({e})"))
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::RUBY_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::RubyBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| ruby_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        ruby_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        ruby_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} persistent Ruby method{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn ruby_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level Ruby methods with one required positional argument and Jet-compatible names, then rerun `jet inspect bind ruby`.".to_string())
}

/// D-FFI-PHP1=A: statically discover top-level functions and generate a
/// persistent supervised PHP worker pool. PHP source never runs during discovery.
fn run_php_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind php <script.php> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind php` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        php_bind_error(path, &format!("the PHP script could not be read ({e})"))
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::PHP_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::PhpBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| php_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        php_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        php_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} PHP function{} into a four-worker pool from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn php_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level PHP functions with one required positional argument and Jet-compatible names, then rerun `jet inspect bind php` inside the provisioned Jet environment.".to_string())
}

/// D-FFI-R1=A: parse top-level function metadata without evaluating source,
/// then generate a persistent supervised R worker.
fn run_r_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind r <script.R> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind r` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| r_bind_error(path, &format!("the R script could not be read ({e})")));
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::R_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::RBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| r_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        r_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        r_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} R function{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn r_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level R functions with one required positional argument and Jet-compatible names, then rerun `jet inspect bind r`.".to_string())
}

/// D-FFI-PY1=A: statically discover top-level Python functions and generate a
/// persistent supervised CPython sidecar with the common checked boundary.
fn run_python_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind py <script.py> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind py` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        python_bind_error(path, &format!("the Python script could not be read ({e})"))
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::PY_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::PythonBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| python_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        python_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        );
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        python_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        );
    }
    println!(
        "bound {} persistent Python function{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn python_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level Python functions with Jet-compatible names and required scalar annotations (`int`, `float`, or `bool`), then rerun `jet inspect bind py` inside the provisioned Jet environment.".to_string())
}

/// D-FFI-JS1=A: bind a typed TypeScript declaration file to a target-selected
/// native JavaScript module through the common checked scalar sidecar.
fn run_javascript_bind(args: &[&String]) {
    let usage = || {
        eprintln!("usage: {} inspect bind js <module.d.ts> --runtime <module.js> [--pkg <lib>] [-o <out.jet>]",jet::Syntax::BINARY_NAME)
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let declaration = args[0].as_str();
    let mut runtime = None;
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--runtime" => {
                runtime = args.get(i + 1).map(|v| v.to_string());
                if runtime.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind js` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = declaration.rsplit('/').next().unwrap_or(declaration);
        base.strip_suffix(".d.ts")
            .or_else(|| base.rsplit_once('.').map(|v| v.0))
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(declaration).unwrap_or_else(|e| {
        javascript_bind_error(
            declaration,
            &format!("the TypeScript declaration file could not be read ({e})"),
        )
    });
    let Some(runtime) = runtime else {
        javascript_bind_error(
            declaration,
            "the Node broker is opt-in; provide the matching runtime with `--runtime <module.js>`",
        )
    };
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::JS_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::JavaScriptBind::bind(
        std::path::Path::new(declaration),
        &source,
        std::path::Path::new(&runtime),
        &lib,
        cache,
    )
    .unwrap_or_else(|e| javascript_bind_error(declaration, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        javascript_bind_error(
            declaration,
            &format!("the generated cache could not be written ({e})"),
        );
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.d.ts")), &source) {
        javascript_bind_error(
            declaration,
            &format!("the typed declaration cache could not be written ({e})"),
        );
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        javascript_bind_error(
            declaration,
            &format!("the binding provenance could not be written ({e})"),
        );
    }
    println!(
        "bound {} JavaScript export{} from `{declaration}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn javascript_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"export typed scalar functions in a `.d.ts` file, provide the matching JavaScript module with `--runtime`, and rerun `jet inspect bind js` inside the provisioned Jet environment.".to_string())
}

/// D-FFI-OCTAVE1=A: parse one-input/one-output matrix functions without
/// evaluating source, then generate a persistent checked Octave sidecar.
fn run_octave_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind octave <script.m> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind octave` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        octave_bind_error(path, &format!("the Octave script could not be read ({e})"))
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::OCTAVE_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::OctaveBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| octave_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        octave_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        octave_bind_error(
            path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} Octave matrix function{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn octave_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"define top-level Octave functions with one matrix input and one matrix output, then rerun `jet inspect bind octave` in the provisioned Jet environment.".to_string())
}

/// D-FFI-COM1=A: inspect a Windows type library and generate typed IDispatch
/// automation stubs. Non-Windows hosts reject before touching the input.
fn run_com_bind(args: &[&String]) {
    let usage = || {
        eprintln!("usage: {} inspect bind com <library.tlb> --pkg <lib>\n       {} inspect bind com --registered <guid> --major <n> --minor <n> [--lcid <n>] --pkg <lib>",jet::Syntax::BINARY_NAME,jet::Syntax::BINARY_NAME)
    };
    if !cfg!(target_os = "windows") {
        eprintln!("Error [E3260]: `com.*` needs a Windows host.");
        eprintln!(" Why: COM type libraries, apartments, and IDispatch are Windows facilities.");
        eprintln!(" Fix: run `jet inspect bind com` and build the COM module on a Windows host.");
        eprintln!("More: jet-lang.dev/e/E3260");
        exit(ExitCodes::USER_ERROR)
    }
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let mut file = None;
    let mut guid = None;
    let mut major = None;
    let mut minor = None;
    let mut lcid = 0u32;
    let mut pkg = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--registered" => {
                guid = args.get(i + 1).map(|v| v.to_string());
                i += 2
            }
            "--major" => {
                major = args.get(i + 1).and_then(|v| v.parse::<u16>().ok());
                i += 2
            }
            "--minor" => {
                minor = args.get(i + 1).and_then(|v| v.parse::<u16>().ok());
                i += 2
            }
            "--lcid" => {
                lcid = args
                    .get(i + 1)
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(u32::MAX);
                i += 2
            }
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                i += 2
            }
            "-o" | "--out" => {
                crate::cli_error!(
                    "E2102",
                    "`inspect bind com` writes the checked binding to `.jet/bindings/com`; custom output paths are not supported"
                );
                usage();
                exit(ExitCodes::USAGE)
            }
            value if !value.starts_with('-') && file.is_none() => {
                file = Some(value.to_string());
                i += 1
            }
            _ => {
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let Some(lib) = pkg else {
        usage();
        exit(ExitCodes::USAGE)
    };
    let input = if let Some(path) = file {
        if guid.is_some() {
            usage();
            exit(ExitCodes::USAGE)
        }
        jet::ComBind::TypeLibraryInput::File(path.into())
    } else {
        let (Some(guid), Some(major), Some(minor)) = (guid, major, minor) else {
            usage();
            exit(ExitCodes::USAGE)
        };
        if lcid == u32::MAX {
            usage();
            exit(ExitCodes::USAGE)
        }
        jet::ComBind::TypeLibraryInput::Registered {
            guid,
            major,
            minor,
            lcid,
        }
    };
    let out_path = format!(
        ".jet/bindings/{}/{}.{}",
        jet::Syntax::COM_MODULE_ROOT,
        lib,
        jet::Syntax::FILE_EXT
    );
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result =
        jet::ComBind::bind(&input, &lib, cache).unwrap_or_else(|e| com_bind_error(&e.to_string()));
    let provenance_path = cache.join(format!("{lib}.provenance"));
    if let Err(e) = publish_com_binding_artifacts(
        Path::new(&out_path),
        &result.source,
        &provenance_path,
        &result.provenance,
    ) {
        com_bind_error(&e)
    }
    println!(
        "bound {} typed COM member{} → {out_path}",
        result.methods.len(),
        if result.methods.len() == 1 { "" } else { "s" }
    );
}
fn com_bind_error(why: &str) -> ! {
    bind_e3208("Could not generate COM bindings.".to_string(),format!("{why}."),"select a registered or file-backed type library with IDispatch metadata and rerun on Windows.".to_string())
}

/// Publish generated source and provenance as one recoverable pair. A failed
/// write must leave either the previous pair or no pair, never a source whose
/// ABI record describes different bytes.
fn publish_com_binding_artifacts(
    source_path: &Path,
    source: &str,
    provenance_path: &Path,
    provenance: &str,
) -> Result<(), String> {
    if source_path == provenance_path {
        return Err("generated COM source and provenance paths must differ".into());
    }
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| format!("could not create COM output transaction: {error}"))?
            .as_nanos()
    );
    let source_stage = source_path.with_file_name(format!(
        ".{}..com-stage-{suffix}",
        source_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "generated COM source has no valid file name".to_string())?
    ));
    let provenance_stage = provenance_path.with_file_name(format!(
        ".{}..com-stage-{suffix}",
        provenance_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "generated COM provenance has no valid file name".to_string())?
    ));
    let source_backup = source_path.with_file_name(format!(
        ".{}..com-previous-{suffix}",
        source_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "generated COM source has no valid file name".to_string())?
    ));
    let provenance_backup = provenance_path.with_file_name(format!(
        ".{}..com-previous-{suffix}",
        provenance_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "generated COM provenance has no valid file name".to_string())?
    ));
    let cleanup_staged = || {
        let _ = fs::remove_file(&source_stage);
        let _ = fs::remove_file(&provenance_stage);
    };
    let cleanup_backups = || {
        let _ = fs::remove_file(&source_backup);
        let _ = fs::remove_file(&provenance_backup);
    };
    if let Err(error) = fs::write(&source_stage, source) {
        cleanup_staged();
        return Err(format!(
            "the generated COM cache could not be staged ({error})"
        ));
    }
    if let Err(error) = fs::write(&provenance_stage, provenance) {
        cleanup_staged();
        return Err(format!("the COM provenance could not be staged ({error})"));
    }
    let had_source = match fs::symlink_metadata(source_path) {
        Ok(metadata) if metadata.file_type().is_file() => {
            if let Err(error) = fs::rename(source_path, &source_backup) {
                cleanup_staged();
                return Err(format!("could not stage the previous COM cache ({error})"));
            }
            true
        }
        Ok(_) => {
            cleanup_staged();
            return Err("the generated COM cache path is not a regular file".into());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            cleanup_staged();
            return Err(format!(
                "could not inspect the generated COM cache ({error})"
            ));
        }
    };
    let had_provenance = match fs::symlink_metadata(provenance_path) {
        Ok(metadata) if metadata.file_type().is_file() => {
            if let Err(error) = fs::rename(provenance_path, &provenance_backup) {
                if had_source {
                    let _ = fs::rename(&source_backup, source_path);
                }
                cleanup_staged();
                return Err(format!(
                    "could not stage the previous COM provenance ({error})"
                ));
            }
            true
        }
        Ok(_) => {
            if had_source {
                let _ = fs::rename(&source_backup, source_path);
            }
            cleanup_staged();
            return Err("the generated COM provenance path is not a regular file".into());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            if had_source {
                let _ = fs::rename(&source_backup, source_path);
            }
            cleanup_staged();
            return Err(format!(
                "could not inspect the generated COM provenance ({error})"
            ));
        }
    };
    if let Err(error) = fs::rename(&source_stage, source_path) {
        if had_source {
            let _ = fs::rename(&source_backup, source_path);
        }
        if had_provenance {
            let _ = fs::rename(&provenance_backup, provenance_path);
        }
        cleanup_staged();
        return Err(format!(
            "could not publish the generated COM cache ({error})"
        ));
    }
    if let Err(error) = fs::rename(&provenance_stage, provenance_path) {
        let _ = fs::remove_file(source_path);
        if had_source {
            let _ = fs::rename(&source_backup, source_path);
        }
        if had_provenance {
            let _ = fs::rename(&provenance_backup, provenance_path);
        }
        cleanup_staged();
        return Err(format!("could not publish the COM provenance ({error})"));
    }
    cleanup_staged();
    cleanup_backups();
    Ok(())
}

/// D-FFI-JVM1=A: compile Java bytecode, discover its public ABI with javap,
/// then build an in-process JNI invocation bridge.
fn run_java_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind java <source.java> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        });
    }
    let source_path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind java` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = source_path.rsplit('/').next().unwrap_or(source_path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_string()
    });
    let source = std::fs::read_to_string(source_path).unwrap_or_else(|e| {
        java_bind_error(
            source_path,
            &format!("the source file could not be read ({e})"),
        )
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::JAVA_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::JavaBind::bind(std::path::Path::new(source_path), &source, &lib, cache)
        .unwrap_or_else(|e| java_bind_error(source_path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        java_bind_error(
            source_path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(
        cache.join(format!("{lib}.jvm-path")),
        format!("{}\n", result.jvm_dir.display()),
    ) {
        java_bind_error(
            source_path,
            &format!("the JVM runtime identity could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        java_bind_error(
            source_path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    println!(
        "bound {} JVM member{} from `{}` → {}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" },
        source_path,
        out_path
    );
}

fn java_bind_error(source: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{source}`."),format!("{why}."),"use a public Java class with one public long/double constructor and non-overloaded long/double methods, then rerun `jet inspect bind java`.".to_string())
}

/// D-FFI-DOTNET1=A: reflect a C# assembly surface, then generate an in-process
/// hostfxr bridge. Managed state remains behind consuming GCHandle ownership.
fn run_dotnet_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind cs <source.cs> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind cs` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = path.rsplit('/').next().unwrap_or(path);
        base.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(base)
            .to_ascii_lowercase()
    });
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| {
        dotnet_bind_error(path, &format!("the C# source could not be read ({e})"))
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::CS_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::DotNetBind::bind(std::path::Path::new(path), &source, &lib, cache)
        .unwrap_or_else(|e| dotnet_bind_error(path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        dotnet_bind_error(
            path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        dotnet_bind_error(path, &format!("the provenance could not be written ({e})"))
    }
    println!(
        "bound {} .NET member{} from `{path}` → {out_path}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" }
    );
}
fn dotnet_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(format!("Could not generate bindings from `{path}`."),format!("{why}."),"use one public C# class with one public long/double constructor and non-overloaded public long/double methods, then rerun `jet inspect bind cs`.".to_string())
}

/// D-FFI-GO1=A: compile exported scalar Go functions and move-only `uintptr`
/// handles into an in-process c-archive and emit a typed `go.<lib>` Jet module.
fn run_go_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind go <source.go> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        )
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        eprintln!();
        eprintln!("Generate typed Jet bindings for scalar and uintptr //export Go functions.");
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        });
    }
    let source_path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|value| value.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                i += 2;
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|value| value.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                i += 2;
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind go` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE);
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = source_path.rsplit('/').next().unwrap_or(source_path);
        base.rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(base)
            .to_string()
    });
    let source = std::fs::read_to_string(source_path).unwrap_or_else(|error| {
        go_bind_error(
            source_path,
            &format!("the source file could not be read ({error})"),
        )
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::GO_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache_dir = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::GoBind::bind(std::path::Path::new(source_path), &source, &lib, cache_dir)
        .unwrap_or_else(|error| go_bind_error(source_path, &error.to_string()));
    if let Err(error) = std::fs::write(&out_path, &result.source) {
        go_bind_error(
            source_path,
            &format!("the generated cache could not be written ({error})"),
        );
    }
    if let Err(error) = std::fs::write(
        cache_dir.join(format!("{lib}.provenance")),
        &result.provenance,
    ) {
        go_bind_error(
            source_path,
            &format!("the binding provenance could not be written ({error})"),
        );
    }
    println!(
        "bound {} Go export{} from `{}` → {}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" },
        source_path,
        out_path
    );
}

fn go_bind_error(source: &str, why: &str) -> ! {
    bind_e3208(
        format!("Could not generate bindings from `{source}`."),
        format!("{why}."),
        "export `int64`/`float64` scalars or move-only `uintptr` handles with `//export Name`, then rerun `jet inspect bind go`.".to_string(),
    )
}

/// D-FFI-FORTRAN1=A: discover scalar and fixed-shape ISO_C_BINDING functions, compile them
/// with the provisioned gfortran toolchain, and emit a typed `fortran.<lib>`
/// Jet module backed by the shared C ABI linker.
fn run_fortran_bind(args: &[&String]) {
    let usage = || {
        eprintln!(
            "usage: {} inspect bind fortran <source.f90> [--pkg <lib>] [-o <out.jet>]",
            jet::Syntax::BINARY_NAME
        );
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        eprintln!();
        eprintln!(
            "Generate typed Jet bindings for scalar and fixed-shape input ISO_C_BINDING functions."
        );
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        });
    }

    let source_path = args[0].as_str();
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--pkg" => {
                pkg = args.get(i + 1).map(|value| value.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                i += 2;
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|value| value.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE);
                }
                i += 2;
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind fortran` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE);
            }
        }
    }
    let lib = pkg.unwrap_or_else(|| {
        let base = source_path.rsplit('/').next().unwrap_or(source_path);
        base.rsplit_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(base)
            .to_string()
    });
    let source = match std::fs::read_to_string(source_path) {
        Ok(source) => source,
        Err(error) => fortran_bind_error(
            source_path,
            &format!("the source file could not be read ({error})"),
        ),
    };
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::FORTRAN_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache_dir = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result =
        match jet::FortranBind::bind(std::path::Path::new(source_path), &source, &lib, cache_dir) {
            Ok(result) => result,
            Err(error) => fortran_bind_error(source_path, &error.to_string()),
        };
    if let Err(error) = std::fs::write(&out_path, &result.source) {
        fortran_bind_error(
            source_path,
            &format!("the generated cache could not be written ({error})"),
        );
    }
    if let Err(error) = std::fs::write(
        cache_dir.join(format!("{lib}.provenance")),
        &result.provenance,
    ) {
        fortran_bind_error(
            source_path,
            &format!("the binding provenance could not be written ({error})"),
        );
    }
    println!(
        "bound {} ISO_C_BINDING routine{} from `{}` → {}",
        result.bound.len(),
        if result.bound.len() == 1 { "" } else { "s" },
        source_path,
        out_path
    );
    for layout in &result.layouts {
        println!(
            "  layout: {}.{} {} {}",
            layout.routine,
            layout.parameter,
            layout.order,
            layout
                .extents
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join("x")
        );
    }
}

fn fortran_bind_error(source: &str, why: &str) -> ! {
    bind_e3208(
        format!("Could not generate bindings from `{source}`."),
        format!("{why}."),
        "use explicit `bind(C, name=\"...\")` routines with ISO_C_BINDING scalar `value` inputs or fixed-shape `intent(in)` arrays, then rerun `jet inspect bind fortran`.".to_string(),
    )
}

fn run_cobol_bind(args: &[&String]) {
    let usage = || {
        eprintln!("usage: {} inspect bind cobol <program.cob> --copybook <record.cpy> [--pkg <lib>] [-o <out.jet>]", jet::Syntax::BINARY_NAME)
    };
    if args.is_empty() || jet::CLI::is_help_flag(args[0]) {
        usage();
        exit(if args.is_empty() {
            ExitCodes::USAGE
        } else {
            ExitCodes::OK
        })
    }
    let source_path = args[0].as_str();
    let mut copybook = None;
    let mut pkg = None;
    let mut out = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--copybook" => {
                copybook = args.get(i + 1).map(|v| v.to_string());
                if copybook.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "--pkg" => {
                pkg = args.get(i + 1).map(|v| v.to_string());
                if pkg.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            "-o" | "--out" => {
                out = args.get(i + 1).map(|v| v.to_string());
                if out.is_none() {
                    usage();
                    exit(ExitCodes::USAGE)
                }
                i += 2
            }
            flag => {
                crate::cli_error!("E2102", "unknown `inspect bind cobol` flag `{flag}`");
                usage();
                exit(ExitCodes::USAGE)
            }
        }
    }
    let Some(copybook_path) = copybook else {
        usage();
        exit(ExitCodes::USAGE)
    };
    let lib = pkg.unwrap_or_else(|| {
        let b = source_path.rsplit('/').next().unwrap_or(source_path);
        b.rsplit_once('.')
            .map(|v| v.0)
            .unwrap_or(b)
            .to_ascii_lowercase()
            .replace('-', "_")
    });
    let source = std::fs::read_to_string(source_path).unwrap_or_else(|e| {
        cobol_bind_error(source_path, &format!("the program could not be read ({e})"))
    });
    let copybook = std::fs::read_to_string(&copybook_path).unwrap_or_else(|e| {
        cobol_bind_error(
            &copybook_path,
            &format!("the copybook could not be read ({e})"),
        )
    });
    let out_path = out.unwrap_or_else(|| {
        format!(
            ".jet/bindings/{}/{}.{}",
            jet::Syntax::COBOL_MODULE_ROOT,
            lib,
            jet::Syntax::FILE_EXT
        )
    });
    let cache = std::path::Path::new(&out_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let result = jet::CobolBind::bind(
        std::path::Path::new(source_path),
        &source,
        std::path::Path::new(&copybook_path),
        &copybook,
        &lib,
        cache,
    )
    .unwrap_or_else(|e| cobol_bind_error(source_path, &e.to_string()));
    if let Err(e) = std::fs::write(&out_path, &result.source) {
        cobol_bind_error(
            source_path,
            &format!("the generated cache could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(cache.join(format!("{lib}.provenance")), &result.provenance) {
        cobol_bind_error(
            source_path,
            &format!("the binding provenance could not be written ({e})"),
        )
    }
    if let Err(e) = std::fs::write(
        cache.join(format!("{lib}.cobol-path")),
        format!("{}\n", result.runtime_dir.display()),
    ) {
        cobol_bind_error(
            source_path,
            &format!("the libcob runtime identity could not be written ({e})"),
        )
    }
    println!(
        "bound GnuCOBOL program `{}` and {}-byte copybook `{}` → {out_path}",
        result.program, result.layout.width, result.layout.name
    );
    for field in &result.layout.fields {
        println!(
            "  layout: {} offset={} width={} type={}",
            field.name,
            field.offset,
            field.width,
            field.kind.jet_type()
        )
    }
}

fn cobol_bind_error(path: &str, why: &str) -> ! {
    bind_e3208(
        format!("Could not generate bindings from `{path}`."),
        format!("{why}."),
        "use one GnuCOBOL PROGRAM-ID with a level-01 copybook containing level-05 X(n), COMP-5, or COMP-3 fields, then rerun `jet inspect bind cobol`.".to_string(),
    )
}

/// S60 / D-PURE1 (E2-M16): evaluate a `pure fn run()` program.
/// D-EVAL1=A: pretty output by default; `--json` for stable machine JSON.
///
/// When `--pure` is given, the entire call graph from `run` is checked for
/// purity violations (E3401 with the full transitive chain, not just the
/// direct callee). This replaces the old hand-rolled `impure_fns` check.
pub(crate) fn run_eval(file: &str, pure_required: bool, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            crate::cli_error!("E2105", "couldn't read `{}`: {}", file, e);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let source_for_parse = match jet::Package::mask_inline_package_source(&src) {
        Ok((masked, _)) => masked,
        Err(error) => {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &[error.diagnostic()], mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Lex + parse (needed for purity walk and for eval).
    let (toks, lex_diags) = jet::Lexer::lex(&source_for_parse);
    if !lex_diags.is_empty() {
        eprint!(
            "{}",
            jet::render_all_colored(file, &src, &lex_diags, mode.color_stderr())
        );
        exit(ExitCodes::USER_ERROR);
    }
    let prog = match jet::Parser::parse_with_source(&toks, &source_for_parse) {
        Ok(p) => p,
        Err(ds) => {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &ds, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    };

    // Transitive purity check via the real sema root-walker (E3401 with full
    // call chain). Replaces the old hand-rolled top-level `is_pure` scan.
    if pure_required {
        // Build a FuncSig map from the parsed program for purity flags.
        use std::collections::HashMap;
        let mut funcs_sig: HashMap<String, jet::Sema::FuncSig> = HashMap::new();
        let mut ast_funcs: HashMap<String, &jet::AST::Func> = HashMap::new();
        for item in &prog.items {
            if let jet::AST::Item::Func(f) = item {
                funcs_sig.insert(
                    f.name.clone(),
                    jet::Sema::FuncSig {
                        deprecation: None,
                        params: f
                            .params
                            .iter()
                            .map(|p| (p.convention.clone(), p.ty.clone()))
                            .collect(),
                        root_param: f.params.first().is_some_and(|p| p.root),
                        return_type: f.return_type.clone(),
                        return_view_provenance: f
                            .return_view_provenance
                            .clone()
                            .map(|provenance| {
                                let cell = jet::AST::ViewProvenanceCell::new();
                                cell.set(provenance);
                                cell
                            })
                            .unwrap_or_default(),
                        is_extern: false,
                        is_c_abi: false,
                        c_abi_name: None,
                        callback_transport: None,
                        callback_plan_digest: None,
                        callback_identity: None,
                        foreign_effect_root: None,
                        undo: None,
                        is_unsafe: f.is_unsafe,
                        is_pure: f.is_pure,
                        memo_bound: None,
                        is_foreign_thread_safe: false,
                        is_sanitizer: f.is_sanitizer,
                        is_must_use: f.is_must_use,
                        param_info: f
                            .params
                            .iter()
                            .map(|p| (p.name.clone(), p.default.is_some()))
                            .collect(),
                        param_call: f
                            .params
                            .iter()
                            .map(|p| (p.call_label().to_string(), p.zone))
                            .collect(),
                        defaults: f
                            .params
                            .iter()
                            .map(|p| p.default.as_ref().map(|d| *d.clone()))
                            .collect(),
                        param_variadic: f.params.iter().map(|p| p.variadic).collect(),
                        variadic_bounds: f
                            .params
                            .last()
                            .and_then(|p| p.variadic_bound_list.clone()),
                        param_view_from_names: f
                            .params
                            .iter()
                            .map(|p| p.declared_view_from_names.clone())
                            .collect(),
                        callable_policies: Default::default(),
                    },
                );
                ast_funcs.insert(f.name.clone(), f);
            }
        }
        let diags = jet::check_pure_program_root("run", &funcs_sig, &ast_funcs);
        if !diags.is_empty() {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }

    // Full sema type-check with CompileMode::Eval — runs all type/ownership
    // checks and accepts value-returning `pure fn run() => T`
    // is accepted. This ensures type errors (e.g. `"string" + 5`) surface with
    // their precise diagnostics rather than falling through to E0956.
    {
        let type_diags = jet::check_for_eval(&source_for_parse, file);
        if !type_diags.is_empty() {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &type_diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }

    // Evaluate via comptime and render. D-EVAL1=A: pretty by default, JSON with --json.
    match jet::eval_pure_program_value(&source_for_parse, file) {
        Ok((value, printed)) => {
            // #2068: what the program printed is program output, not noise.
            // In `--json` mode it goes to stderr so stdout stays exactly one
            // JSON document for a machine consumer; otherwise it precedes the
            // value, in the order the program produced it.
            if mode.json {
                eprint!("{printed}");
                println!("{}", render_eval_json(&value));
            } else {
                print!("{printed}");
                if let Some(value) = render_eval_value(&value) {
                    println!("{value}");
                }
            }
        }
        Err(diags) => {
            eprint!(
                "{}",
                jet::render_all_colored(file, &src, &diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn render_eval_value(value: &jet::CtValue) -> Option<String> {
    match value {
        jet::CtValue::Present(inner) => render_eval_value(inner),
        jet::CtValue::Unit => None,
        value => Some(value.jet_show()),
    }
}

/// Label diagnostics carry when the evaluated source came from the command
/// line rather than a file, so a caret can point into the argument text.
const EVAL_EXPRESSION_LABEL: &str = "<eval>";

fn render_eval_json(value: &jet::CtValue) -> String {
    let value = StatusValue::parse(&value.to_json()).expect("evaluated value must be valid JSON");
    StatusEnvelope::new("eval", true)
        .with_field("value", value)
        .json()
}

/// S60 / D-PURE1: `jet eval "<expression>"` — the expression form of the same
/// verb (#2068). Before this, an expression fell through to the file reader
/// and died as `E2105 couldn't read '1 + 2'`, contradicting the registry
/// summary ("Evaluate pure Jet and print JSON").
///
/// Evaluation runs through `jet::REPL::eval_once`, the shipped one-turn
/// expression pipeline, so a command-line expression and the same text typed
/// into `jet repl` cannot disagree — there is no second expression evaluator
/// (I8) and no second set of semantics (I9). This function only marshals the
/// result into D-EVAL1=A's rendering: pretty by default, `--json` for stable
/// machine JSON with program output kept off stdout.
///
/// Effects are refused unconditionally (E1803 — `eval_once` attaches no
/// prompt), which is strictly stronger than `--pure`, so the expression form
/// needs no purity walk of its own.
pub(crate) fn run_eval_expression(source: &str, mode: OutputMode) {
    let base_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match jet::REPL::eval_once(source, &base_dir, jet::REPL::ReplFlags::default()) {
        Ok(evaluated) => {
            // An input that only binds or declares has no value of its own;
            // render it as Unit, the same way a `run()` that hands nothing
            // back renders in the file form. One canonical rendering.
            let value = evaluated.value.unwrap_or(jet::CtValue::Unit);
            if mode.json {
                eprint!("{}{}", evaluated.stdout, evaluated.stderr);
                println!("{}", render_eval_json(&value));
            } else {
                print!("{}", evaluated.stdout);
                eprint!("{}", evaluated.stderr);
                println!("{}", value.render_pretty());
            }
        }
        Err(diags) => {
            eprint!(
                "{}",
                jet::render_all_colored(EVAL_EXPRESSION_LABEL, source, &diags, mode.color_stderr())
            );
            exit(ExitCodes::USER_ERROR);
        }
    }
}

/// D-A11YGATE1=B (c134 Phase 6): the a11y lint codes (E2930/E2931). These are
/// always computed during sema (same as any other `Severity::Lint`), but per
/// the ratified decision they only *surface* under `jet lint --a11y` — never
/// as ordinary build/run/emit warnings. `visible_lints` is the one filter
/// every normal compile-flavored command applies before printing `out.lints`.
const A11Y_LINT_CODES: [&str; 2] = ["E2930", "E2931"];

pub(crate) fn visible_lints(
    lints: &[jet::Diagnostics::Diagnostic],
) -> Vec<jet::Diagnostics::Diagnostic> {
    lints
        .iter()
        .filter(|d| !A11Y_LINT_CODES.contains(&d.code.as_str()))
        .cloned()
        .collect()
}

/// D-TOOL3 (E2-M11): `jet emit --rust` — print Rust from optimized canonical
/// MIR for a checked Jet file. `--metadata` adds the canonical MIR identity
/// header used by artifact witnesses.
pub(crate) fn run_emit_rust(file: &str, mode: OutputMode, emit_metadata: bool) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
            exit(ExitCodes::USER_ERROR);
        }
    };
    let (diagnostics, bundle, _facts) =
        jet::Driver::check_file_with_effect_facts_for_run(file, "dev", &BTreeMap::new());
    let (errors, lints): (Vec<_>, Vec<_>) = diagnostics
        .into_iter()
        .partition(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error);
    if !errors.is_empty() {
        report_problems(mode, file, &src, &errors);
        exit(ExitCodes::USER_ERROR);
    }
    let bundle = match bundle {
        Some(bundle) => bundle,
        None => {
            report_problems(mode, file, &src, &[]);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let lints = visible_lints(&lints);
    if !lints.is_empty() {
        eprint!(
            "{}",
            jet::render_all_colored(file, &src, &lints, mode.color_stderr())
        );
    }
    let target = if bundle.build_facts.target_triple.is_empty() {
        jet_foundation::Layout::TargetLayout::host()
    } else {
        jet_foundation::Layout::TargetLayout::from_triple(bundle.build_facts.target_triple.clone())
    };
    let (mir, artifact) = jet::lower_checked_semantic_mir_program_for(
        &bundle,
        jet_foundation::MIR::MirArtifactRequest::new(
            jet_foundation::MIR::MirArtifactTarget::RustAot,
            jet_foundation::MIR::MirArtifactKind::NativeExecutable,
            jet_foundation::MIR::MirArtifactBuildMode::Dev,
        ),
    );
    let mir_digest = jet_foundation::MIR::mir_program_digest(&mir);
    let mut execution = jet::Codegen::MIRRust::MirRustExecutionConfig::for_artifact(artifact);
    execution.emit_metadata = emit_metadata;
    execution.semantic_digest = emit_metadata.then_some(mir_digest);
    let rust = jet::Codegen::MIRRust::emit_mir_program(
        &mir,
        &jet::Codegen::MIRRust::MirRustConfig {
            target,
            target_kind: jet::Codegen::MIRRust::MirRustTarget::Native,
            root_prefix: String::new(),
            execution,
        },
    );
    print!("{rust}");
}
/// Project the shared cost diagnostics consumed by `jet check` and
/// `jet lint --cost`. Sema owns view-copy diagnostics; typed TIR owns the
/// remaining cost kinds. Optimizer-proven removals stay in `jet explain --cost`
/// only.
pub(crate) fn cost_diagnostics(
    bundle: &jet::AST::ProgramBundle,
    sema_diagnostics: &[jet::Diagnostics::Diagnostic],
) -> Result<Vec<jet::Diagnostics::Diagnostic>, jet::Codegen::TIR::TCostReportError> {
    let report = jet::Codegen::TIR::cost_report(bundle)?;
    Ok(cost_diagnostics_from_report(&report, sema_diagnostics))
}

fn cost_diagnostics_from_report(
    report: &jet::Codegen::TIR::TCostReport,
    sema_diagnostics: &[jet::Diagnostics::Diagnostic],
) -> Vec<jet::Diagnostics::Diagnostic> {
    let mut diagnostics = sema_diagnostics
        .iter()
        .filter(|diagnostic| is_cost_diagnostic(diagnostic))
        .cloned()
        .collect::<Vec<_>>();
    diagnostics.extend(
        report
            .sites
            .iter()
            .filter(|site| {
                site.loop_depth > 0
                    && matches!(site.state, jet::Codegen::TIR::TCostState::SemanticRemainder)
                    && !matches!(site.kind, jet::Codegen::TIR::TCostKind::ViewMaterialization)
            })
            .map(|site| {
                jet::Diagnostics::Diagnostic::from_row(
                    "L2510",
                    &[
                        ("operation", site.kind.operation()),
                        ("fix", site.kind.fix()),
                    ],
                    Some(site.span),
                )
            }),
    );
    dedup_cost_diagnostics(&mut diagnostics);
    diagnostics
}

fn is_cost_diagnostic(diagnostic: &jet::Diagnostics::Diagnostic) -> bool {
    diagnostic.code == "L2510"
}

fn same_cost_diagnostic(
    left: &jet::Diagnostics::Diagnostic,
    right: &jet::Diagnostics::Diagnostic,
) -> bool {
    left.code == right.code
        && left.severity == right.severity
        && left.span == right.span
        && left.what == right.what
        && left.why == right.why
        && left.fix == right.fix
}

fn dedup_cost_diagnostics(diagnostics: &mut Vec<jet::Diagnostics::Diagnostic>) {
    let mut seen = Vec::new();
    diagnostics.retain(|diagnostic| {
        if !is_cost_diagnostic(diagnostic) {
            return true;
        }
        if seen
            .iter()
            .any(|previous| same_cost_diagnostic(previous, diagnostic))
        {
            false
        } else {
            seen.push(diagnostic.clone());
            true
        }
    });
}

pub(crate) fn merge_cost_diagnostics(
    bundle: &jet::AST::ProgramBundle,
    diagnostics: &mut Vec<jet::Diagnostics::Diagnostic>,
) -> Result<(), jet::Codegen::TIR::TCostReportError> {
    dedup_cost_diagnostics(diagnostics);
    let cost_diagnostics = cost_diagnostics(bundle, diagnostics)?;
    for diagnostic in cost_diagnostics {
        if !diagnostics
            .iter()
            .any(|existing| same_cost_diagnostic(existing, &diagnostic))
        {
            diagnostics.push(diagnostic);
        }
    }
    Ok(())
}

/// `jet lint --cost <file>` — surface semantic costs that remain on a hot
/// loop.  The report is opt-in and fails only when a typed cost site exists.
pub(crate) fn run_lint_cost(file: &str, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
            exit(ExitCodes::USER_ERROR);
        }
    };
    let projection = match crate::CmdInspect::check_projection_for_effects(
        Path::new(file),
        "dev",
        &BTreeMap::new(),
    ) {
        Ok(projection) => projection,
        Err(diagnostics) => {
            report_problems(mode, file, &src, &diagnostics);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let lints = match cost_diagnostics(&projection.bundle, &projection.diagnostics) {
        Ok(lints) => lints,
        Err(error) => exit_cost_projection_error(file, &error),
    };
    if lints.is_empty() {
        if mode.json {
            let machine_file = crate::machine_report_path_for_process(file);
            print!("{}", jet::render_all_json(&machine_file, &src, &[]));
        } else {
            println!("ok: `{}` has no hidden dynamic costs in loops", file);
        }
        return;
    }
    if mode.json {
        let machine_file = crate::machine_report_path_for_process(file);
        eprint!("{}", jet::render_all_json(&machine_file, &src, &lints));
    } else {
        eprint!(
            "{}",
            jet::render_all_colored(file, &src, &lints, mode.color_stderr())
        );
        let n = lints.len();
        eprintln!(
            "\n{} hidden dynamic cost warning{} found",
            n,
            if n == 1 { "" } else { "s" }
        );
    }
    exit(ExitCodes::USER_ERROR);
}

/// D-A11YGATE1=B (c134 Phase 6): `jet lint --a11y <file>` — the opt-in
/// surface for accessibility lints (E2930 unlabeled control, E2931 duplicate
/// label). Never runs during `jet build`/`jet run`/`jet check`; exits nonzero
/// when it finds something so a project can gate CI on "zero a11y warnings"
/// without those warnings ever blocking ordinary compilation.
pub(crate) fn run_lint_a11y(file: &str, mode: OutputMode) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
            exit(ExitCodes::USER_ERROR);
        }
    };
    let diags = jet::check_with_path(file);
    let errors: Vec<jet::Diagnostics::Diagnostic> = diags
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .cloned()
        .collect();
    if !errors.is_empty() {
        report_problems(mode, file, &src, &errors);
        exit(ExitCodes::USER_ERROR);
    }
    let a11y_lints: Vec<jet::Diagnostics::Diagnostic> = diags
        .into_iter()
        .filter(|d| A11Y_LINT_CODES.contains(&d.code.as_str()))
        .collect();
    if a11y_lints.is_empty() {
        if mode.json {
            let machine_file = crate::machine_report_path_for_process(file);
            print!("{}", jet::render_all_json(&machine_file, &src, &[]));
        } else {
            println!("ok: `{}` has no accessibility problems", file);
        }
        return;
    }
    if mode.json {
        let machine_file = crate::machine_report_path_for_process(file);
        eprint!("{}", jet::render_all_json(&machine_file, &src, &a11y_lints));
    } else {
        eprint!(
            "{}",
            jet::render_all_colored(file, &src, &a11y_lints, mode.color_stderr())
        );
        let n = a11y_lints.len();
        eprintln!(
            "\n{} accessibility warning{} found",
            n,
            if n == 1 { "" } else { "s" }
        );
    }
    exit(ExitCodes::USER_ERROR);
}

/// `jet lint --complexity <file>` — deterministic per-function scores. The
/// category reports by default; `--max=<n>` is the explicit failure gate.
pub(crate) fn run_lint_complexity(file: &str, mode: OutputMode, max_budget: Option<u32>) {
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(_) => {
            crate::cli_error!(@fix "E2105", format!("can't find the file `{}`", file), format!("check the spelling, or run {} from the folder that contains it", jet::Syntax::BINARY_NAME));
            exit(ExitCodes::USER_ERROR);
        }
    };
    let diags = jet::check_with_path(file);
    let errors: Vec<jet::Diagnostics::Diagnostic> = diags
        .iter()
        .filter(|d| matches!(d.severity, jet::Diagnostics::Severity::Error))
        .cloned()
        .collect();
    if !errors.is_empty() {
        report_problems(mode, file, &src, &errors);
        exit(ExitCodes::USER_ERROR);
    }
    let source_for_parse = match jet::Package::mask_inline_package_source(&src) {
        Ok((masked, _)) => masked,
        Err(error) => {
            report_problems(mode, file, &src, &[error.diagnostic()]);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let (tokens, lex_errors) = jet::Lexer::lex(&source_for_parse);
    if !lex_errors.is_empty() {
        report_problems(mode, file, &src, &lex_errors);
        exit(ExitCodes::USER_ERROR);
    }
    let program = match jet::Parser::parse_with_source(&tokens, &source_for_parse) {
        Ok(program) => program,
        Err(parse_errors) => {
            report_problems(mode, file, &src, &parse_errors);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let reports = jet::Sema::cognitive_complexity_reports(&program);
    let rows: Vec<_> = reports
        .iter()
        .map(|report| {
            let line = src[..report.span.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            let over = max_budget.is_some_and(|budget| report.score > budget);
            (line, over)
        })
        .collect();
    if mode.json {
        let json_rows: Vec<String> = reports
            .iter()
            .zip(&rows)
            .map(|(report, (line, over))| {
                let budget =
                    max_budget.map_or_else(|| "null".to_string(), |value| value.to_string());
                format!(
                    "{{\"file\":{},\"fn\":{},\"line\":{},\"score\":{},\"budget\":{},\"over\":{}}}",
                    json_string(file),
                    json_string(&report.name),
                    line,
                    report.score,
                    budget,
                    over
                )
            })
            .collect();
        println!("[{}]", json_rows.join(","));
    } else {
        for (report, (line, _)) in reports.iter().zip(&rows) {
            match max_budget {
                Some(budget) => println!(
                    "{}:{} fn {} — score {} (budget {})",
                    file, line, report.name, report.score, budget
                ),
                None => println!(
                    "{}:{} fn {} — score {}",
                    file, line, report.name, report.score
                ),
            }
        }
    }
    let Some(budget) = max_budget else { return };
    let exceeded: Vec<_> = reports
        .iter()
        .filter(|report| report.score > budget)
        .map(|report| {
            jet::Diagnostics::Diagnostic::lint(
                "L2902",
                format!(
                    "function `{}` has cognitive complexity {} above budget {}",
                    report.name, report.score, budget
                ),
                "the function's control flow is harder to read and maintain than the selected budget allows".to_string(),
                "split the function, reduce nesting, or replace mixed control flow with a focused helper".to_string(),
                Some(report.span),
            )
        })
        .collect();
    if exceeded.is_empty() {
        return;
    }
    if !mode.json {
        eprint!(
            "{}",
            jet::render_all_colored(file, &src, &exceeded, mode.color_stderr())
        );
    }
    exit(ExitCodes::USER_ERROR);
}

#[derive(Clone, Debug)]
pub(crate) struct BenchEvidence {
    pub(crate) name: String,
    pub(crate) samples: Vec<(u128, u64)>,
    /// One exact `(jet_mem allocation events, requested bytes, iterations)`
    /// row per measured trial. Calibration/warmup runs are outside the reset
    /// boundary and never enter these facts.
    pub(crate) allocation_samples: Vec<(u128, u128, u64)>,
}

/// D-CLAIM-BENCH1=A: collect the same exact twenty samples through the test
/// harness used by `jet test --measure`. Budget providers call this seam; the
/// retired benchmark command is not part of measurement anymore.
pub(crate) fn collect_measure_evidence(
    file: &str,
    src: &str,
    mode: OutputMode,
    filter: Option<&str>,
) -> Vec<BenchEvidence> {
    let (rust_code, ffi_link) = match jet::compile_tests_with_path(src, file) {
        Ok(value) => value,
        Err(diags) => {
            report_problems(mode, file, src, &diags);
            exit(ExitCodes::USER_ERROR);
        }
    };
    let bin = PathBuf::from("build").join(format!("test_measure_{}", stem(file)));
    build(
        file,
        &rust_code,
        None,
        bin.clone(),
        BuildProfile::Release,
        ffi_link.as_ref(),
        &[],
        false,
        None,
        None,
        None,
        mode,
        false,
        None,
    );
    let mut command = Command::new(&bin);
    command
        .env("JET_TEST_MEASURE", "1")
        .env("JET_TEST_MEASURE_EVIDENCE", "1");
    if let Some(filter) = filter {
        command.env("JET_TEST_FILTER", filter);
    }
    let output = command.output().unwrap_or_else(|error| {
        eprintln!("measure: couldn't run `{}`: {}", bin.display(), error);
        exit(ExitCodes::USER_ERROR);
    });
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        exit(crate::CmdCompile::child_exit_code(output.status));
    }
    let stdout = String::from_utf8(output.stdout).unwrap_or_else(|_| {
        eprintln!("measure: harness emitted non-UTF-8 evidence");
        exit(ExitCodes::USER_ERROR);
    });
    let mut evidence = Vec::new();
    for line in stdout.lines() {
        if let Some(wire) = line.strip_prefix("JETALLOC1\t") {
            let mut fields = wire.split('\t');
            let name = fields.next().and_then(decode_hex).unwrap_or_else(|| {
                eprintln!("measure: harness emitted malformed allocation identity");
                exit(ExitCodes::USER_ERROR);
            });
            let tier = fields
                .next()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    eprintln!("measure: harness emitted no execution tier for allocation evidence");
                    exit(ExitCodes::USER_ERROR);
                });
            let profile = fields
                .next()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| {
                    eprintln!("measure: harness emitted no build profile for allocation evidence");
                    exit(ExitCodes::USER_ERROR);
                });
            let warmups = fields
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .unwrap_or_else(|| {
                    eprintln!(
                        "measure: harness emitted invalid warmup count for allocation evidence"
                    );
                    exit(ExitCodes::USER_ERROR);
                });
            let iters = fields
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .unwrap_or_else(|| {
                    eprintln!("measure: harness emitted invalid allocation iteration count");
                    exit(ExitCodes::USER_ERROR);
                });
            let serial = fields
                .next()
                .and_then(|value| value.parse::<bool>().ok())
                .unwrap_or_else(|| {
                    eprintln!("measure: harness emitted non-serial allocation evidence");
                    exit(ExitCodes::USER_ERROR);
                });
            if tier != "aot" || profile != "release" || warmups != 5 || !serial {
                eprintln!("measure: harness emitted allocation evidence without the optimized serial AOT profile");
                exit(ExitCodes::USER_ERROR);
            }
            let samples = fields
                .map(|value| {
                    let (count, bytes) = value.split_once(':').ok_or(())?;
                    Ok((
                        count.parse::<u128>().map_err(|_| ())?,
                        bytes.parse::<u128>().map_err(|_| ())?,
                        iters,
                    ))
                })
                .collect::<Result<Vec<_>, ()>>()
                .unwrap_or_else(|_| {
                    eprintln!("measure: harness emitted malformed allocation evidence");
                    exit(ExitCodes::USER_ERROR);
                });
            if samples.len() != 20 {
                eprintln!(
                    "measure: harness emitted {} allocation samples; policy requires 20",
                    samples.len()
                );
                exit(ExitCodes::USER_ERROR);
            }
            let entry = evidence
                .iter_mut()
                .find(|entry: &&mut BenchEvidence| entry.name == name)
                .unwrap_or_else(|| {
                    eprintln!("measure: allocation evidence preceded its named claim");
                    exit(ExitCodes::USER_ERROR);
                });
            entry.allocation_samples = samples;
            continue;
        }
        let Some(wire) = line.strip_prefix("JETTESTMEASURE1\t") else {
            continue;
        };
        let mut fields = wire.split('\t');
        let name = fields.next().and_then(decode_hex).unwrap_or_else(|| {
            eprintln!("measure: harness emitted malformed claim identity");
            exit(ExitCodes::USER_ERROR);
        });
        let tier = fields
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                eprintln!("measure: harness emitted no execution tier");
                exit(ExitCodes::USER_ERROR);
            });
        let profile = fields
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                eprintln!("measure: harness emitted no build profile");
                exit(ExitCodes::USER_ERROR);
            });
        let warmups = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(|| {
                eprintln!("measure: harness emitted invalid warmup count");
                exit(ExitCodes::USER_ERROR);
            });
        let iters = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or_else(|| {
                eprintln!("measure: harness emitted invalid iteration count");
                exit(ExitCodes::USER_ERROR);
            });
        let serial = fields
            .next()
            .and_then(|value| value.parse::<bool>().ok())
            .unwrap_or_else(|| {
                eprintln!("measure: harness emitted non-serial timing evidence");
                exit(ExitCodes::USER_ERROR);
            });
        if tier != "aot" || profile != "release" || warmups != 5 || !serial {
            eprintln!(
                "measure: harness emitted timing evidence without the optimized serial AOT profile"
            );
            exit(ExitCodes::USER_ERROR);
        }
        let samples = fields
            .map(|value| value.parse::<u128>().map(|elapsed| (elapsed, iters)))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|_| {
                eprintln!("measure: harness emitted invalid exact sample");
                exit(ExitCodes::USER_ERROR);
            });
        if samples.len() != 20 {
            eprintln!(
                "measure: harness emitted {} samples; policy requires 20",
                samples.len()
            );
            exit(ExitCodes::USER_ERROR);
        }
        evidence.push(BenchEvidence {
            name,
            samples,
            allocation_samples: Vec::new(),
        });
    }
    evidence
}

/// Collect `ServiceProbe` evidence by cycling each named service down→up→ready
/// 20 times. Reads `env.jet` from the project root to resolve `DevServicePlan`.
/// Returns one `ServiceEvidence` entry per service name present in `specs`.
pub(crate) fn collect_service_evidence(
    root: &std::path::Path,
    specs: &[jet::Sema::LocatedBudgetSpec],
) -> Vec<crate::CmdBudget::ServiceEvidence> {
    // Names of services that have a ServiceProbe budget.
    let service_names: std::collections::BTreeSet<String> = specs
        .iter()
        .filter(|s| {
            let kind = s
                .spec
                .provider
                .split_once('(')
                .map(|(k, _)| k)
                .unwrap_or(&s.spec.provider);
            kind == "ServiceProbe"
        })
        .map(|s| {
            s.spec
                .provider
                .split_once('(')
                .and_then(|(_, rest)| rest.strip_suffix(')'))
                .unwrap_or("")
                .to_string()
        })
        .collect();

    if service_names.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    for name in &service_names {
        let argv = vec![
            "__service-probe".to_string(),
            name.clone(),
            "--no-color".to_string(),
        ];
        let output = match crate::EngineDispatch::capture(
            jet::Syntax::JETPACK_BINARY_NAME,
            "ServiceProbe",
            &argv,
            root,
        ) {
            Ok(output) => output,
            Err(_) => continue,
        };
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "budget: ServiceProbe `{name}` measurement failed: {}",
                stderr.trim()
            );
            continue;
        }
        let stdout = match String::from_utf8(output.stdout) {
            Ok(stdout) => stdout,
            Err(_) => {
                eprintln!("budget: ServiceProbe `{name}` returned non-UTF-8 evidence");
                continue;
            }
        };
        let expected_name: String = name.bytes().map(|byte| format!("{byte:02x}")).collect();
        let mut rows = stdout.lines();
        let Some(row) = rows.next() else {
            eprintln!("budget: ServiceProbe `{name}` returned no evidence");
            continue;
        };
        if rows.next().is_some() {
            eprintln!("budget: ServiceProbe `{name}` returned extra evidence rows");
            continue;
        }
        let mut fields = row.split('\t');
        if fields.next() != Some("JETSERVICE1") || fields.next() != Some(expected_name.as_str()) {
            eprintln!("budget: ServiceProbe `{name}` returned incompatible evidence");
            continue;
        }
        let samples_ns: Option<Vec<u64>> = fields.map(|field| field.parse().ok()).collect();
        let Some(samples_ns) = samples_ns.filter(|samples| samples.len() == 20) else {
            eprintln!("budget: ServiceProbe `{name}` did not return exactly 20 samples");
            continue;
        };
        result.push(crate::CmdBudget::ServiceEvidence {
            name: name.clone(),
            samples_ns,
        });
    }
    result
}

/// Collect `SceneProbe` evidence by compiling the entry file, running it with
/// `JET_SCENE_PROBE=<name>` for each named scene, and parsing JETSCENE1 rows.
/// Returns one `SceneEvidence` per scene present in `specs`.
pub(crate) fn collect_scene_evidence(
    file: &str,
    src: &str,
    mode: OutputMode,
    specs: &[jet::Sema::LocatedBudgetSpec],
) -> Vec<crate::CmdBudget::SceneEvidence> {
    // Names of scenes that have a SceneProbe budget.
    let scene_names: std::collections::BTreeSet<String> = specs
        .iter()
        .filter(|s| {
            let kind = s
                .spec
                .provider
                .split_once('(')
                .map(|(k, _)| k)
                .unwrap_or(&s.spec.provider);
            kind == "SceneProbe"
        })
        .map(|s| {
            s.spec
                .provider
                .split_once('(')
                .and_then(|(_, rest)| rest.strip_suffix(')'))
                .unwrap_or("")
                .to_string()
        })
        .collect();

    if scene_names.is_empty() {
        return Vec::new();
    }

    // Compile the program once.
    let compiled = match jet::compile_with_path(src, file) {
        Ok(out) => out,
        Err(diags) => {
            report_problems(mode, file, src, &diags);
            return Vec::new();
        }
    };
    let bin = PathBuf::from("build").join(format!("scene_probe_{}", stem(file)));
    build(
        file,
        &compiled.rust,
        None,
        bin.clone(),
        BuildProfile::Release,
        compiled.ffi.as_ref(),
        &[],
        false,
        None,
        None,
        None,
        mode,
        false,
        None,
    );

    let mut result = Vec::new();
    for scene_name in &scene_names {
        let out = match std::process::Command::new(&bin)
            .env("JET_SCENE_PROBE", scene_name)
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("budget: SceneProbe `{scene_name}` run failed: {e}");
                continue;
            }
        };
        if !out.status.success() {
            eprintln!("budget: SceneProbe `{scene_name}` exited with non-zero status");
            continue;
        }
        let stdout = String::from_utf8(out.stdout).unwrap_or_default();
        let mut frame_ns = Vec::new();
        let mut draw_calls = Vec::new();
        let mut asset_bytes = Vec::new();
        let mut rss_hwm = Vec::new();
        let expected_hex: String = scene_name.bytes().map(|b| format!("{:02x}", b)).collect();
        for line in stdout.lines() {
            let Some(wire) = line.strip_prefix("JETSCENE1\t") else {
                continue;
            };
            let mut fields = wire.splitn(4, '\t');
            let hex = fields.next().unwrap_or("");
            let metric = fields.next().unwrap_or("");
            let value_str = fields.next().unwrap_or("");
            if hex != expected_hex {
                continue;
            }
            let value: u64 = match value_str.parse() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("budget: SceneProbe `{scene_name}` malformed value `{value_str}`");
                    continue;
                }
            };
            match metric {
                "FrameTime" => frame_ns.push(value),
                "DrawCalls" => draw_calls.push(value),
                "SceneAssetBytes" => asset_bytes.push(value),
                "MemoryHighWater" => rss_hwm.push(value),
                _ => {}
            }
        }
        // Require exactly 600 measured samples per metric (120 warmup omitted).
        if frame_ns.len() != 600
            || draw_calls.len() != 600
            || asset_bytes.len() != 600
            || rss_hwm.len() != 600
        {
            eprintln!(
                "budget: SceneProbe `{scene_name}` emitted {}/{}/{}/{} samples; need 600 for each metric",
                frame_ns.len(), draw_calls.len(), asset_bytes.len(), rss_hwm.len()
            );
            continue;
        }
        result.push(crate::CmdBudget::SceneEvidence {
            name: scene_name.clone(),
            frame_ns,
            draw_calls,
            asset_bytes,
            rss_hwm,
        });
    }
    result
}

/// `jet devtools probe <file>` — internal test-only single-shot dev probe.
/// Collects SceneProbe/ServiceProbe evidence for the given file and triggers
/// a budget report refresh. Exits 0 if the report is built and all gates pass,
/// 1 otherwise. Not user-documented; used by CI tests.
pub(crate) fn run_devtools_probe(args: &[&String]) {
    use std::process::exit;
    let file = match args.first() {
        Some(f) => f.as_str(),
        None => {
            eprintln!("usage: jet devtools probe <file.jet>");
            exit(jet::ExitCodes::USAGE);
        }
    };
    let src = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("probe: cannot read `{file}`: {e}");
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let mode = OutputMode {
        json: false,
        color: jet::Diagnostics::ColorChoice::Never,
        quiet: false,
    };
    let bundle = match jet::Loader::load_entry(file) {
        Ok(mut b) => {
            let _ = jet::Sema::check_bundle(&mut b, jet::Sema::CompileMode::Run);
            b
        }
        Err(diags) => {
            report_problems(mode, file, &src, &diags);
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let specs = match jet::Sema::collect_located_budget_specs_bundle(&bundle) {
        Ok(s) => s,
        Err(_) => {
            exit(jet::ExitCodes::USER_ERROR);
        }
    };
    let root = Path::new(file)
        .canonicalize()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .and_then(|d| jet::Loader::find_manifest_root(&d).or_else(|| Some(d)))
        .unwrap_or_else(|| PathBuf::from("."));
    let service_evidence = collect_service_evidence(&root, &specs);
    let scene_evidence = collect_scene_evidence(file, &src, mode, &specs);
    let status = crate::CmdBudget::run_dev_refresh(file, &service_evidence, &scene_evidence);
    exit(status);
}

fn decode_hex(value: &str) -> Option<String> {
    if value.len() % 2 != 0 {
        return None;
    }
    let nibble = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    };
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Some((nibble(pair[0])? << 4) | nibble(pair[1])?))
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod schedule_tests {
    use super::JobClock;

    /// D-SCHEDULE1 (card #505): an interval job fires the first time it's
    /// checked (no prior run), then not again immediately after.
    #[test]
    fn interval_fires_once_then_waits() {
        let mut clock = JobClock::new();
        let jobs = vec![(
            "prune".to_string(),
            jet::AST::EverySchedule::Duration {
                nanos: 60 * 1_000_000_000,
            },
        )];
        assert_eq!(clock.due(&jobs), vec!["prune".to_string()]);
        assert!(
            clock.due(&jobs).is_empty(),
            "must not re-fire on the very next tick"
        );
    }

    /// A daily job fires when `unix_secs` lands inside its target minute,
    /// stays quiet outside that window, and does not re-fire later the same
    /// day even if checked again inside the window.
    #[test]
    fn daily_fires_in_window_then_dedupes_same_day() {
        let mut clock = JobClock::new();
        let jobs = vec![(
            "nightly".to_string(),
            jet::AST::EverySchedule::WallClockTime { hour: 3, minute: 0 },
        )];
        let day0_before_window = 10 * 86_400 + 2 * 3600 + 59 * 60; // 02:59 on day 10
        let day0_in_window = 10 * 86_400 + 3 * 3600 + 0 * 60 + 30; // 03:00:30 on day 10
        let day0_after_window = 10 * 86_400 + 3 * 3600 + 5 * 60; // 03:05 on day 10
        let day1_in_window = 11 * 86_400 + 3 * 3600; // 03:00 on day 11

        assert!(
            clock.due_at(&jobs, day0_before_window).is_empty(),
            "must not fire before the target minute"
        );
        assert_eq!(
            clock.due_at(&jobs, day0_in_window),
            vec!["nightly".to_string()],
            "must fire inside the target minute"
        );
        assert!(
            clock.due_at(&jobs, day0_after_window).is_empty(),
            "must not re-fire later the same day, even outside the window"
        );
        assert_eq!(
            clock.due_at(&jobs, day1_in_window),
            vec!["nightly".to_string()],
            "must fire again the next day's matching window"
        );
    }
}
