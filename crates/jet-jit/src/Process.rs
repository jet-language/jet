//! `core.process` builder / run / pipeline / spawn host shims (#729).
//! Mirrors `jet_std_process_*` / `jet_process_spec_*` in the CoreLib prelude —
//! thin `std::process` wrappers, not a third algorithm.

use super::Concurrency;
use super::CoreHost::{
    jit_env_key_eq, jit_env_snapshot_raw, jit_env_validate_name, jit_env_validate_value,
};
use jet_codegen::process_pty::{self, PtyConfig};
use std::fs::File;
use std::io::{BufRead, Read};
use std::time::Instant;
use crate::Marshal::{clone_string, result_ok, result_err_msg};

include!("../../jet-codegen/src/Prelude/CoreLib/Top/ProcessPolicy.rs");

/// Stream / Inherit / Capture — same order as `jet_std::ProcessStreamMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamMode {
    Stream = 0,
    Inherit = 1,
    Capture = 2,
}

impl StreamMode {
    fn from_disc(disc: i64) -> StreamMode {
        match disc {
            1 => StreamMode::Inherit,
            2 => StreamMode::Capture,
            _ => StreamMode::Stream,
        }
    }

    fn stdio(self) -> std::process::Stdio {
        match self {
            StreamMode::Stream | StreamMode::Capture => std::process::Stdio::piped(),
            StreamMode::Inherit => std::process::Stdio::inherit(),
        }
    }
}

#[derive(Debug)]
enum JitProcessReader {
    Stdout(std::process::ChildStdout),
    Stderr(std::process::ChildStderr),
    Terminal(File),
}

impl Read for JitProcessReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let result = match self {
            Self::Stdout(reader) => reader.read(bytes),
            Self::Stderr(reader) => reader.read(bytes),
            Self::Terminal(reader) => reader.read(bytes),
        };
        match result {
            Err(error)
                if matches!(self, Self::Terminal(_)) && process_pty::is_terminal_eof(&error) =>
            {
                Ok(0)
            }
            other => other,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct JitProcessSpec {
    cmd: Vec<String>,
    stdout: StreamMode,
    stderr: StreamMode,
    timeout_ms: Option<i64>,
    output_limit: Option<i64>,
    env_clear: bool,
    detached: bool,
    terminal: Option<PtyConfig>,
    cwd: Option<String>,
    env_set: Vec<(String, String)>,
    env_remove: Vec<String>,
}

impl JitProcessSpec {
    fn new(cmd: Vec<String>) -> Self {
        Self {
            cmd,
            stdout: StreamMode::Capture,
            stderr: StreamMode::Capture,
            timeout_ms: None,
            output_limit: None,
            env_clear: false,
            detached: false,
            terminal: None,
            cwd: None,
            env_set: Vec::new(),
            env_remove: Vec::new(),
        }
    }
}

pub(crate) struct JitProcessChild {
    inner: Option<std::process::Child>,
    stdout: Option<std::io::BufReader<JitProcessReader>>,
    stderr: Option<std::io::BufReader<JitProcessReader>>,
    terminal_master: Option<File>,
    timeout_ms: Option<i64>,
    started: Instant,
}

struct RunOutcome {
    code: i64,
    output: String,
    errors: String,
    success: bool,
    signal: Option<i64>,
    timed_out: bool,
}

enum WaitPoll {
    Done(RunOutcome),
    Running,
}

impl Default for WaitPoll {
    fn default() -> Self {
        WaitPoll::Running
    }
}

fn clone_string_list(list: i64) -> Vec<String> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let sid = rt.heap.list_get_int(list, i).unwrap_or(0);
            out.push(rt.heap.clone_string(sid).unwrap_or_default());
        }
        out
    })
}

fn alloc_process_result(out: &RunOutcome) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        // Field order: code, output, errors, success, signal, timed_out
        let rec = rt.heap.alloc_record(6);
        let _ = rt.heap.record_set_int(rec, 0, out.code);
        let sid = rt.heap.alloc_string(out.output.clone());
        let _ = rt.heap.record_set_string(rec, 1, sid);
        let eid = rt.heap.alloc_string(out.errors.clone());
        let _ = rt.heap.record_set_string(rec, 2, eid);
        let _ = rt.heap.record_set_bool(rec, 3, out.success);
        let signal = out.signal.map(|value| value.wrapping_add(1)).unwrap_or(0);
        let _ = rt.heap.record_set_int(rec, 4, signal);
        let _ = rt.heap.record_set_bool(rec, 5, out.timed_out);
        rec
    })
}

fn outcome_to_result(out: RunOutcome) -> i64 {
    let rec = alloc_process_result(&out);
    result_ok(rec as u64)
}

fn push_spec(spec: JitProcessSpec) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.process_specs.push(spec);
        rt.process_specs.len() as i64
    })
}

fn with_spec_mut(handle: i64, f: impl FnOnce(&mut JitProcessSpec)) -> Option<()> {
    if handle <= 0 {
        return None;
    }
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).saturating_sub(1);
        rt.process_specs.get_mut(idx).map(f)
    })
}

fn clone_spec(handle: i64) -> Option<JitProcessSpec> {
    if handle <= 0 {
        return None;
    }
    Concurrency::with_runtime_mut(|rt| {
        let idx = (handle as usize).saturating_sub(1);
        rt.process_specs.get(idx).cloned()
    })
}

fn build_command_base(spec: &JitProcessSpec) -> Result<std::process::Command, String> {
    if spec.cmd.is_empty() {
        return Err("process command needs at least one word".to_string());
    }
    let first = &spec.cmd[0];
    let mut command = if first.ends_with(".jet") {
        // Resident/interpreter runs name the current program via argv[0]=.jet;
        // re-exec through the jet driver so `--watch-child` hits Jet entry.
        let jet = std::env::var("JET_BIN").unwrap_or_else(|_| {
            std::env::current_exe()
                .ok()
                .and_then(|p| {
                    p.parent()
                        .map(|dir| dir.join("jet"))
                        .filter(|c| c.exists())
                        .map(|c| c.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| "jet".to_string())
        });
        let mut command = std::process::Command::new(jet);
        command.arg("run").arg(first).arg("--");
        command.args(&spec.cmd[1..]);
        command
    } else {
        let mut command = std::process::Command::new(first);
        command.args(&spec.cmd[1..]);
        command
    };
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    // D-ENV-MUTATE1=A: clone the process-global logical environment under its
    // lock. Compose this launch in owned memory, then replace the host
    // environment atomically from the child's point of view.
    let mut child_env = if spec.env_clear {
        Vec::new()
    } else {
        jit_env_snapshot_raw()
    };
    for (name, value) in &spec.env_set {
        jit_env_validate_name(name)
            .map_err(|error| format!("invalid input during resolve `{name}`: {error}"))?;
        jit_env_validate_value(value)
            .map_err(|error| format!("invalid input during resolve `{name}`: {error}"))?;
        let name = std::ffi::OsString::from(name);
        child_env.retain(|(candidate, _)| !jit_env_key_eq(candidate.as_os_str(), name.as_os_str()));
        child_env.push((name, std::ffi::OsString::from(value)));
    }
    for name in &spec.env_remove {
        jit_env_validate_name(name)
            .map_err(|error| format!("invalid input during resolve `{name}`: {error}"))?;
        let name = std::ffi::OsStr::new(name);
        child_env.retain(|(candidate, _)| !jit_env_key_eq(candidate.as_os_str(), name));
    }
    command.env_clear();
    command.envs(child_env);
    command.stdin(std::process::Stdio::null());
    command.stdout(spec.stdout.stdio());
    command.stderr(spec.stderr.stdio());
    Ok(command)
}

fn build_command(spec: &JitProcessSpec) -> Result<std::process::Command, String> {
    if spec.terminal.is_some() {
        return Err(
            "I/O error during resolve: terminal sessions cannot be used as pipeline stages; spawn the session directly"
                .to_string(),
        );
    }
    build_command_base(spec)
}

/// `jet_process_spec_spawn` in the AOT prelude drops every stream for a
/// detached child. Same rule here, so `run()` and `spawn()` report the same
/// empty output under both lenses.
fn apply_detached(spec: &JitProcessSpec, command: &mut std::process::Command) {
    if spec.detached {
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
    }
}

fn drain_reader<R: Read + Send + 'static>(
    reader: Option<std::io::BufReader<R>>,
) -> Option<std::thread::JoinHandle<std::io::Result<String>>> {
    reader.map(|mut reader| {
        std::thread::spawn(move || {
            let mut text = String::new();
            reader.read_to_string(&mut text)?;
            Ok(text)
        })
    })
}

fn join_drain(
    drain: Option<std::thread::JoinHandle<std::io::Result<String>>>,
    stream: &str,
) -> Result<String, String> {
    let Some(drain) = drain else {
        return Ok(String::new());
    };
    drain
        .join()
        .map_err(|_| format!("process {stream} reader panicked"))?
        .map_err(|e| format!("process {stream}: {e}"))
}

fn spawn_process(
    spec: &JitProcessSpec,
) -> Result<(
    std::process::Child,
    Option<std::io::BufReader<JitProcessReader>>,
    Option<std::io::BufReader<JitProcessReader>>,
    Option<File>,
), String> {
    if let Some(config) = spec.terminal {
        if spec.detached {
            return Err("terminal sessions cannot be detached".to_string());
        }
        let pair = process_pty::open(config).map_err(|error| {
            format!(
                "I/O error during resolve `{}`: {error}",
                spec.cmd.first().cloned().unwrap_or_default()
            )
        })?;
        let mut command = build_command_base(spec)?;
        let stdin = pair
            .slave
            .try_clone()
            .map_err(|error| format!("process terminal stdin: {error}"))?;
        let stdout = pair
            .slave
            .try_clone()
            .map_err(|error| format!("process terminal stdout: {error}"))?;
        let stderr = pair
            .slave
            .try_clone()
            .map_err(|error| format!("process terminal stderr: {error}"))?;
        command.stdin(std::process::Stdio::from(stdin));
        command.stdout(std::process::Stdio::from(stdout));
        command.stderr(std::process::Stdio::from(stderr));
        process_pty::attach_command(&mut command)
            .map_err(|error| format!("process terminal setup: {error}"))?;
        let child = command
            .spawn()
            .map_err(|error| format!("spawn {}: {error}", spec.cmd.first().cloned().unwrap_or_default()))?;
        drop(pair.slave);
        let terminal_output = pair
            .master
            .try_clone()
            .map_err(|error| format!("process terminal reader: {error}"))?;
        Ok((
            child,
            Some(std::io::BufReader::new(JitProcessReader::Terminal(
                terminal_output,
            ))),
            None,
            Some(pair.master),
        ))
    } else {
        let mut command = build_command(spec)?;
        apply_detached(spec, &mut command);
        let mut child = command
            .spawn()
            .map_err(|error| format!("spawn {}: {error}", spec.cmd.first().cloned().unwrap_or_default()))?;
        let stdout = child
            .stdout
            .take()
            .map(JitProcessReader::Stdout)
            .map(std::io::BufReader::new);
        let stderr = child
            .stderr
            .take()
            .map(JitProcessReader::Stderr)
            .map(std::io::BufReader::new);
        Ok((child, stdout, stderr, None))
    }
}

fn run_spec(spec: &JitProcessSpec) -> Result<RunOutcome, String> {
    let (mut child, stdout, stderr, _terminal_master) = spawn_process(spec)?;
    let stdout_drain = drain_reader(stdout);
    let stderr_drain = drain_reader(stderr);
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if let Some(timeout) = spec.timeout_ms {
                    if started.elapsed() >= std::time::Duration::from_millis(timeout.max(0) as u64)
                    {
                        if spec.terminal.is_some() {
                            let _ = process_pty::signal_group(child.id(), process_pty::SIGKILL)
                                .or_else(|_| child.kill());
                        } else {
                            let _ = child.kill();
                        }
                        timed_out = true;
                        break child.wait().map_err(|e| format!("wait after kill: {e}"))?;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return Err(format!("wait: {e}")),
        }
    };
    let output = join_drain(stdout_drain, "stdout")?;
    let errors = join_drain(stderr_drain, "stderr")?;
    if let Some(limit) = spec.output_limit {
        if (output.len() + errors.len()) as i64 > limit {
            return Err("process output exceeded output_limit".to_string());
        }
    }
    #[cfg(unix)]
    let signal = std::os::unix::process::ExitStatusExt::signal(&status).map(i64::from);
    #[cfg(not(unix))]
    let signal = None;
    Ok(RunOutcome {
        code: status.code().unwrap_or(-1) as i64,
        output,
        errors,
        success: status.success(),
        signal,
        timed_out,
    })
}

fn run_pipeline(specs: &[JitProcessSpec]) -> Result<RunOutcome, String> {
    if specs.is_empty() {
        return Err("process.pipeline needs at least one command".to_string());
    }
    let mut children: Vec<std::process::Child> = Vec::new();
    let mut prev_stdout: Option<std::process::ChildStdout> = None;
    for spec in specs {
        let mut command = build_command(spec)?;
        if let Some(stdout) = prev_stdout.take() {
            command.stdin(std::process::Stdio::from(stdout));
        } else {
            command.stdin(std::process::Stdio::null());
        }
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", spec.cmd.first().cloned().unwrap_or_default()))?;
        prev_stdout = child.stdout.take();
        children.push(child);
    }
    let last = children
        .last_mut()
        .ok_or_else(|| "empty pipeline".to_string())?;
    let stdout = prev_stdout.take().map(std::io::BufReader::new);
    let stderr = last.stderr.take().map(std::io::BufReader::new);
    let stdout_drain = drain_reader(stdout);
    let stderr_drain = drain_reader(stderr);
    let mut last_status = None;
    for child in &mut children {
        last_status = Some(child.wait().map_err(|e| format!("pipeline wait: {e}"))?);
    }
    let status = last_status.ok_or_else(|| "empty pipeline".to_string())?;
    let output = join_drain(stdout_drain, "stdout")?;
    let errors = join_drain(stderr_drain, "stderr")?;
    #[cfg(unix)]
    let signal = std::os::unix::process::ExitStatusExt::signal(&status).map(i64::from);
    #[cfg(not(unix))]
    let signal = None;
    Ok(RunOutcome {
        code: status.code().unwrap_or(-1) as i64,
        output,
        errors,
        success: status.success(),
        signal,
        timed_out: false,
    })
}

extern "C" fn jet_jit_process_cmd(cmd_list: i64) -> i64 {
    push_spec(JitProcessSpec::new(clone_string_list(cmd_list)))
}

extern "C" fn jet_jit_process_run(cmd_list: i64) -> i64 {
    let spec = JitProcessSpec::new(clone_string_list(cmd_list));
    match run_spec(&spec) {
        Ok(out) => outcome_to_result(out),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_process_pipeline(spec_list: i64) -> i64 {
    let handles = Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(spec_list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(spec_list, i).unwrap_or(0));
        }
        out
    });
    let mut specs = Vec::with_capacity(handles.len());
    for h in handles {
        match clone_spec(h) {
            Some(s) => specs.push(s),
            None => return result_err_msg("process.pipeline: invalid ProcessSpec"),
        }
    }
    match run_pipeline(&specs) {
        Ok(out) => outcome_to_result(out),
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_process_spec_stdout(spec: i64, mode: i64) -> i64 {
    match with_spec_mut(spec, |s| {
        s.stdout = StreamMode::from_disc(mode);
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_stderr(spec: i64, mode: i64) -> i64 {
    match with_spec_mut(spec, |s| {
        s.stderr = StreamMode::from_disc(mode);
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_timeout(spec: i64, timeout_ms: i64) -> i64 {
    match with_spec_mut(spec, |s| {
        s.timeout_ms = Some(timeout_ms.max(0));
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_output_limit(spec: i64, limit: i64) -> i64 {
    match with_spec_mut(spec, |s| {
        s.output_limit = Some(limit.max(0));
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_run(spec: i64) -> i64 {
    let Some(s) = clone_spec(spec) else {
        return result_err_msg("invalid ProcessSpec");
    };
    match run_spec(&s) {
        Ok(out) => outcome_to_result(out),
        Err(e) => result_err_msg(&e),
    }
}

fn checked_stderr(errors: &str) -> String {
    const LIMIT: usize = 4096;
    if errors.len() <= LIMIT {
        return errors.to_string();
    }
    let mut end = LIMIT;
    while !errors.is_char_boundary(end) {
        end -= 1;
    }
    errors[..end].to_string()
}

extern "C" fn jet_jit_process_spec_run_checked(spec: i64) -> i64 {
    let Some(s) = clone_spec(spec) else {
        return result_err_msg("invalid ProcessSpec");
    };
    match run_spec(&s) {
        Ok(out) if out.success => outcome_to_result(out),
        Ok(out) => {
            let mut cause = format!("process exited unsuccessfully: code={}", out.code);
            if let Some(signal) = out.signal {
                cause.push_str(&format!(", signal={signal}"));
            }
            cause.push_str(&format!(", stderr={}", checked_stderr(&out.errors)));
            let command = s.cmd.first().cloned().unwrap_or_default();
            result_err_msg(&format!("I/O error during close `{command}`: {cause}"))
        }
        Err(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_process_spec_spawn(spec: i64) -> i64 {
    let Some(s) = clone_spec(spec) else {
        return result_err_msg("invalid ProcessSpec");
    };
    let (child, stdout, stderr, terminal_master) = match spawn_process(&s) {
        Ok(value) => value,
        Err(error) => return result_err_msg(&error),
    };
    let handle = Concurrency::with_runtime_mut(|rt| {
        rt.process_children.push(JitProcessChild {
            inner: Some(child),
            stdout,
            stderr,
            terminal_master,
            timeout_ms: s.timeout_ms,
            started: Instant::now(),
        });
        rt.process_children.len() as i64
    });
    result_ok(handle as u64)
}

extern "C" fn jet_jit_process_spec_env_clear(spec: i64) -> i64 {
    match with_spec_mut(spec, |s| {
        s.env_clear = true;
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_detached(spec: i64) -> i64 {
    match with_spec_mut(spec, |s| {
        s.detached = true;
    }) {
        Some(()) => spec,
        None => 0,
    }
}

fn terminal_config_from_policy(policy: i64) -> PtyConfig {
    // `with_runtime_mut` needs a `Default` result, and `PtyConfig` has no
    // meaningful zero, so read the fields out and build the config here.
    let (cols, rows, raw) = Concurrency::with_runtime_mut(|rt| {
        let size = rt.heap.record_get_int(policy, 0).unwrap_or(0);
        (
            rt.heap.record_get_int(size, 0).unwrap_or(0),
            rt.heap.record_get_int(size, 1).unwrap_or(0),
            rt.heap.record_get_int(policy, 1).unwrap_or(1) == 0,
        )
    });
    PtyConfig { cols, rows, raw }
}

fn terminal_size_from_handle(size: i64) -> (i64, i64) {
    Concurrency::with_runtime_mut(|rt| {
        (
            rt.heap.record_get_int(size, 0).unwrap_or(0),
            rt.heap.record_get_int(size, 1).unwrap_or(0),
        )
    })
}

extern "C" fn jet_jit_process_spec_terminal(spec: i64) -> i64 {
    match with_spec_mut(spec, |s| {
        s.terminal = Some(PtyConfig::default());
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_terminal_with_policy(spec: i64, policy: i64) -> i64 {
    let config = terminal_config_from_policy(policy);
    match with_spec_mut(spec, |s| {
        s.terminal = Some(config);
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_capabilities(_spec: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        // A resident string set is a set of heap string handles tagged
        // `string_kind = true`.
        let mut facts = std::collections::HashSet::new();
        for fact in jet_process_policy::terminal_facts(process_pty::supported()) {
            facts.insert(rt.heap.alloc_string((*fact).to_string()));
        }
        rt.sets.push(facts);
        rt.set_string_kinds.push(true);
        rt.sets.len() as i64
    })
}

extern "C" fn jet_jit_process_child_terminal(child: i64) -> i64 {
    if child <= 0 {
        return 0;
    }
    let idx = (child as usize).saturating_sub(1);
    Concurrency::with_runtime_mut(|rt| {
        rt.process_children
            .get(idx)
            .and_then(|slot| slot.terminal_master.as_ref())
            .map(|_| child)
            .unwrap_or(0)
    })
}

extern "C" fn jet_jit_terminal_session_resize(session: i64, size: i64) -> i64 {
    if session <= 0 {
        return result_err_msg("I/O error during resolve `process terminal`: this child has no terminal session");
    }
    let (cols, rows) = terminal_size_from_handle(size);
    let idx = (session as usize).saturating_sub(1);
    // `with_runtime_mut` needs a `Default` result, so report failure as
    // `Some(message)` rather than a `Result`.
    let failure = Concurrency::with_runtime_mut(|rt| {
        let Some(master) = rt
            .process_children
            .get(idx)
            .and_then(|slot| slot.terminal_master.as_ref())
        else {
            return Some("this child has no terminal session".to_string());
        };
        process_pty::resize(
            master,
            PtyConfig {
                cols,
                rows,
                raw: false,
            },
        )
        .err()
        .map(|error| error.to_string())
    });
    match failure {
        None => result_ok(0),
        Some(error) => {
            result_err_msg(&format!("I/O error during resolve `process terminal`: {error}"))
        }
    }
}

extern "C" fn jet_jit_process_spec_stdin(spec: i64, _mode: i64) -> i64 {
    spec
}

extern "C" fn jet_jit_process_spec_cwd(spec: i64, cwd: i64) -> i64 {
    let path = clone_string(cwd);
    match with_spec_mut(spec, |s| {
        s.cwd = Some(path);
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_env(spec: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    match with_spec_mut(spec, |s| {
        s.env_set.push((name, value));
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_env_remove(spec: i64, name: i64) -> i64 {
    let name = clone_string(name);
    match with_spec_mut(spec, |s| {
        s.env_remove.push(name);
    }) {
        Some(()) => spec,
        None => 0,
    }
}

/// `tag`: 0 = stdout, 1 = stderr.
extern "C" fn jet_jit_process_stream_lines(child: i64, tag: i64) -> i64 {
    if child <= 0 {
        return Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
    }
    let idx = (child as usize).saturating_sub(1);
    let lines = Concurrency::with_runtime_mut(|rt| {
        let Some(slot) = rt.process_children.get_mut(idx) else {
            return None;
        };
        let mut out = Vec::new();
        if tag == 1 {
            if let Some(reader) = slot.stderr.as_mut() {
                for line in reader.lines() {
                    out.push(line.ok()?);
                }
            }
            slot.stderr = None;
        } else if let Some(reader) = slot.stdout.as_mut() {
            for line in reader.lines() {
                out.push(line.ok()?);
            }
            slot.stdout = None;
        }
        Some(out)
    });
    match lines {
        Some(lines) => Concurrency::with_runtime_mut(|rt| {
            let list = rt.heap.alloc_empty_list();
            for line in lines {
                let sid = rt.heap.alloc_string(line);
                let _ = rt.heap.list_push_int(list, sid);
            }
            list
        }),
        None => Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list()),
    }
}

extern "C" fn jet_jit_process_child_id(child: i64) -> i64 {
    if child <= 0 {
        return 0;
    }
    let idx = (child as usize).saturating_sub(1);
    Concurrency::with_runtime_mut(|rt| {
        rt.process_children
            .get(idx)
            .and_then(|slot| slot.inner.as_ref())
            .map(|inner| inner.id() as i64)
            .unwrap_or(0)
    })
}

// #1481 core.process: a non-blocking companion to `child_wait` — peeks the
// same registry slot without draining pipes or blocking.
extern "C" fn jet_jit_process_child_exited(child: i64) -> i64 {
    if child <= 0 {
        return result_err_msg("invalid ProcessChild");
    }
    let idx = (child as usize).saturating_sub(1);
    Concurrency::with_runtime_mut(|rt| {
        let Some(slot) = rt.process_children.get_mut(idx) else {
            return result_err_msg("invalid ProcessChild");
        };
        let Some(inner) = slot.inner.as_mut() else {
            return result_ok(1);
        };
        match inner.try_wait() {
            Ok(status) => result_ok(if status.is_some() { 1 } else { 0 }),
            Err(e) => result_err_msg(&e.to_string()),
        }
    })
}

fn process_child_signal(child: i64, signal: i32) -> Option<String> {
    if child <= 0 {
        return Some("invalid ProcessChild".to_string());
    }
    let idx = (child as usize).saturating_sub(1);
    Concurrency::with_runtime_mut(|rt| {
        let Some(slot) = rt.process_children.get_mut(idx) else {
            return Some("invalid ProcessChild".to_string());
        };
        let Some(inner) = slot.inner.as_mut() else {
            return None;
        };
        let result = if slot.terminal_master.is_some() {
            process_pty::signal_group(inner.id(), signal).or_else(|_| inner.kill())
        } else {
            inner.kill()
        };
        match result {
            Ok(()) => None,
            Err(e) => Some(e.to_string()),
        }
    })
}

extern "C" fn jet_jit_process_child_kill(child: i64) -> i64 {
    let err = process_child_signal(child, process_pty::SIGKILL);
    match err {
        None => result_ok(0),
        Some(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_process_child_terminate(child: i64) -> i64 {
    let err = process_child_signal(child, process_pty::SIGTERM);
    match err {
        None => result_ok(0),
        Some(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_process_child_interrupt(child: i64) -> i64 {
    let err = process_child_signal(child, process_pty::SIGINT);
    match err {
        None => result_ok(0),
        Some(e) => result_err_msg(&e),
    }
}

extern "C" fn jet_jit_process_child_wait(child: i64) -> i64 {
    if child <= 0 {
        return result_err_msg("invalid ProcessChild");
    }
    let idx = (child as usize).saturating_sub(1);
    loop {
        let poll = Concurrency::with_runtime_mut(|rt| -> WaitPoll {
            let Some(slot) = rt.process_children.get_mut(idx) else {
                return WaitPoll::Done(RunOutcome {
                    code: 0,
                    output: String::new(),
                    errors: String::new(),
                    success: true,
                    signal: None,
                    timed_out: false,
                });
            };
            let Some(inner) = slot.inner.as_mut() else {
                return WaitPoll::Done(RunOutcome {
                    code: 0,
                    output: String::new(),
                    errors: String::new(),
                    success: true,
                    signal: None,
                    timed_out: false,
                });
            };
            match inner.try_wait() {
                Ok(Some(status)) => {
                    slot.inner.take();
                    let stdout = slot.stdout.take();
                    let stderr = slot.stderr.take();
                    // Drain outside? Need strings now — join while unlocked by taking first.
                    // Readers owned locally after take.
                    let stdout_drain = drain_reader(stdout);
                    let stderr_drain = drain_reader(stderr);
                    // Cannot join inside runtime borrow if drain needs nothing from rt — ok.
                    let output = match join_drain(stdout_drain, "stdout") {
                        Ok(o) => o,
                        Err(e) => {
                            return WaitPoll::Done(RunOutcome {
                                code: -1,
                                output: e,
                                errors: String::new(),
                                success: false,
                                signal: None,
                                timed_out: false,
                            })
                        }
                    };
                    let errors = match join_drain(stderr_drain, "stderr") {
                        Ok(o) => o,
                        Err(e) => {
                            return WaitPoll::Done(RunOutcome {
                                code: -1,
                                output,
                                errors: e,
                                success: false,
                                signal: None,
                                timed_out: false,
                            })
                        }
                    };
                    #[cfg(unix)]
                    let signal =
                        std::os::unix::process::ExitStatusExt::signal(&status).map(i64::from);
                    #[cfg(not(unix))]
                    let signal = None;
                    WaitPoll::Done(RunOutcome {
                        code: status.code().unwrap_or(-1) as i64,
                        output,
                        errors,
                        success: status.success(),
                        signal,
                        timed_out: false,
                    })
                }
                Ok(None) => {
                    if let Some(timeout) = slot.timeout_ms {
                        if slot.started.elapsed()
                            >= std::time::Duration::from_millis(timeout.max(0) as u64)
                        {
                            if slot.terminal_master.is_some() {
                                let _ = process_pty::signal_group(inner.id(), process_pty::SIGKILL)
                                    .or_else(|_| inner.kill());
                            } else {
                                let _ = inner.kill();
                            }
                            let status = match inner.wait() {
                                Ok(s) => s,
                                Err(_) => {
                                    return WaitPoll::Done(RunOutcome {
                                        code: -1,
                                        output: String::new(),
                                        errors: String::new(),
                                        success: false,
                                        signal: None,
                                        timed_out: true,
                                    })
                                }
                            };
                            slot.inner.take();
                            let stdout = slot.stdout.take();
                            let stderr = slot.stderr.take();
                            let output = join_drain(drain_reader(stdout), "stdout").unwrap_or_default();
                            let errors = join_drain(drain_reader(stderr), "stderr").unwrap_or_default();
                            #[cfg(unix)]
                            let signal = std::os::unix::process::ExitStatusExt::signal(&status)
                                .map(i64::from);
                            #[cfg(not(unix))]
                            let signal = None;
                            return WaitPoll::Done(RunOutcome {
                                code: status.code().unwrap_or(-1) as i64,
                                output,
                                errors,
                                success: false,
                                signal,
                                timed_out: true,
                            });
                        }
                    }
                    WaitPoll::Running
                }
                Err(_) => WaitPoll::Done(RunOutcome {
                    code: -1,
                    output: String::new(),
                    errors: String::new(),
                    success: false,
                    signal: None,
                    timed_out: false,
                }),
            }
        });
        match poll {
            WaitPoll::Done(out) => return outcome_to_result(out),
            WaitPoll::Running => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
}

host_fns! {
    struct ProcessHostFns;
    register: register_process_symbols;
    declare: declare_process_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::{Linkage, Module};
        let cc = module.target_config().default_call_conv;
        let mut sig_unary = Signature::new(cc);
        sig_unary.params.push(AbiParam::new(types::I64));
        sig_unary.returns.push(AbiParam::new(types::I64));
        let mut sig_binary = Signature::new(cc);
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.params.push(AbiParam::new(types::I64));
        sig_binary.returns.push(AbiParam::new(types::I64));
        let mut sig_ternary = Signature::new(cc);
        sig_ternary.params.push(AbiParam::new(types::I64));
        sig_ternary.params.push(AbiParam::new(types::I64));
        sig_ternary.params.push(AbiParam::new(types::I64));
        sig_ternary.returns.push(AbiParam::new(types::I64));


    }
    cmd: "jet_jit_process_cmd" => jet_jit_process_cmd: sig_unary;
    run: "jet_jit_process_run" => jet_jit_process_run: sig_unary;
    pipeline: "jet_jit_process_pipeline" => jet_jit_process_pipeline: sig_unary;
    spec_stdout: "jet_jit_process_spec_stdout" => jet_jit_process_spec_stdout: sig_binary;
    spec_stderr: "jet_jit_process_spec_stderr" => jet_jit_process_spec_stderr: sig_binary;
    spec_stdin: "jet_jit_process_spec_stdin" => jet_jit_process_spec_stdin: sig_binary;
    spec_timeout: "jet_jit_process_spec_timeout" => jet_jit_process_spec_timeout: sig_binary;
    spec_output_limit: "jet_jit_process_spec_output_limit" => jet_jit_process_spec_output_limit: sig_binary;
    spec_cwd: "jet_jit_process_spec_cwd" => jet_jit_process_spec_cwd: sig_binary;
    spec_env: "jet_jit_process_spec_env" => jet_jit_process_spec_env: sig_ternary;
    spec_env_remove: "jet_jit_process_spec_env_remove" => jet_jit_process_spec_env_remove: sig_binary;
    spec_env_clear: "jet_jit_process_spec_env_clear" => jet_jit_process_spec_env_clear: sig_unary;
    spec_detached: "jet_jit_process_spec_detached" => jet_jit_process_spec_detached: sig_unary;
    spec_terminal: "jet_jit_process_spec_terminal" => jet_jit_process_spec_terminal: sig_unary;
    spec_terminal_with_policy: "jet_jit_process_spec_terminal_with_policy" => jet_jit_process_spec_terminal_with_policy: sig_binary;
    spec_capabilities: "jet_jit_process_spec_capabilities" => jet_jit_process_spec_capabilities: sig_unary;
    spec_run: "jet_jit_process_spec_run" => jet_jit_process_spec_run: sig_unary;
    spec_run_checked: "jet_jit_process_spec_run_checked" => jet_jit_process_spec_run_checked: sig_unary;
    spec_spawn: "jet_jit_process_spec_spawn" => jet_jit_process_spec_spawn: sig_unary;
    child_id: "jet_jit_process_child_id" => jet_jit_process_child_id: sig_unary;
    child_exited: "jet_jit_process_child_exited" => jet_jit_process_child_exited: sig_unary;
    child_terminal: "jet_jit_process_child_terminal" => jet_jit_process_child_terminal: sig_unary;
    child_kill: "jet_jit_process_child_kill" => jet_jit_process_child_kill: sig_unary;
    child_terminate: "jet_jit_process_child_terminate" => jet_jit_process_child_terminate: sig_unary;
    child_interrupt: "jet_jit_process_child_interrupt" => jet_jit_process_child_interrupt: sig_unary;
    child_wait: "jet_jit_process_child_wait" => jet_jit_process_child_wait: sig_unary;
    terminal_resize: "jet_jit_terminal_session_resize" => jet_jit_terminal_session_resize: sig_binary;
    stream_lines: "jet_jit_process_stream_lines" => jet_jit_process_stream_lines: sig_binary;
}




