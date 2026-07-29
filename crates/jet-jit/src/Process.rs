//! `core.process` builder / run / pipeline / spawn host shims (#729).
//! Mirrors `jet_std_process_*` / `jet_process_spec_*` in the CoreLib prelude —
//! thin `std::process` wrappers, not a third algorithm.

use super::Concurrency;
use super::CoreHost::{
    jit_env_key_eq, jit_env_snapshot_raw, jit_env_validate_name, jit_env_validate_value,
};
use std::io::{BufRead, Read};
use std::time::Instant;

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

#[derive(Clone, Debug)]
pub(crate) struct JitProcessSpec {
    cmd: Vec<String>,
    stdout: StreamMode,
    stderr: StreamMode,
    timeout_ms: Option<i64>,
    output_limit: Option<i64>,
    env_clear: bool,
    detached: bool,
    terminal: bool,
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
            terminal: false,
            cwd: None,
            env_set: Vec::new(),
            env_remove: Vec::new(),
        }
    }
}

pub(crate) struct JitProcessChild {
    inner: Option<std::process::Child>,
    stdout: Option<std::io::BufReader<std::process::ChildStdout>>,
    stderr: Option<std::io::BufReader<std::process::ChildStderr>>,
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

fn clone_string(sid: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(sid).unwrap_or_default())
}

fn result_ok_bits(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn result_err_msg(msg: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.alloc_string(msg.to_string());
        rt.results.push(super::JitResultValue {
            ok: false,
            bits: sid as u64,
        });
        rt.results.len() as i64
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
    result_ok_bits(rec as u64)
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

fn build_command(spec: &JitProcessSpec) -> Result<std::process::Command, String> {
    if spec.cmd.is_empty() {
        return Err("process command needs at least one word".to_string());
    }
    // D-PROCESS-SESSION1=A: same refusal as `jet_process_terminal_backend_check`
    // in the AOT prelude, and the same text `IOError::jet_show` prints there, so
    // both lenses report one message. No PTY/ConPTY backend exists in either.
    if spec.terminal {
        return Err(format!(
            "I/O error during resolve `{}`: \
             terminal sessions need a PTY or ConPTY backend, and this build has none",
            spec.cmd[0]
        ));
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

fn run_spec(spec: &JitProcessSpec) -> Result<RunOutcome, String> {
    let mut command = build_command(spec)?;
    apply_detached(spec, &mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", spec.cmd.first().cloned().unwrap_or_default()))?;
    let stdout = child.stdout.take().map(std::io::BufReader::new);
    let stderr = child.stderr.take().map(std::io::BufReader::new);
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
                        let _ = child.kill();
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
    let mut command = match build_command(&s) {
        Ok(c) => c,
        Err(e) => return result_err_msg(&e),
    };
    apply_detached(&s, &mut command);
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            return result_err_msg(&format!(
                "spawn {}: {e}",
                s.cmd.first().cloned().unwrap_or_default()
            ))
        }
    };
    let stdout = child.stdout.take().map(std::io::BufReader::new);
    let stderr = child.stderr.take().map(std::io::BufReader::new);
    let handle = Concurrency::with_runtime_mut(|rt| {
        rt.process_children.push(JitProcessChild {
            inner: Some(child),
            stdout,
            stderr,
            timeout_ms: s.timeout_ms,
            started: Instant::now(),
        });
        rt.process_children.len() as i64
    });
    result_ok_bits(handle as u64)
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

extern "C" fn jet_jit_process_spec_terminal(spec: i64) -> i64 {
    match with_spec_mut(spec, |s| {
        s.terminal = true;
    }) {
        Some(()) => spec,
        None => 0,
    }
}

extern "C" fn jet_jit_process_spec_terminal_with_policy(spec: i64, _policy: i64) -> i64 {
    jet_jit_process_spec_terminal(spec)
}

extern "C" fn jet_jit_process_spec_capabilities(_spec: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.sets.push(std::collections::HashSet::new());
        rt.sets.len() as i64
    })
}

extern "C" fn jet_jit_terminal_session_resize(_session: i64, _size: i64) -> i64 {
    result_err_msg("I/O error during resolve `process terminal`: this child has no terminal session")
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

extern "C" fn jet_jit_process_child_kill(child: i64) -> i64 {
    if child <= 0 {
        return result_err_msg("invalid ProcessChild");
    }
    let idx = (child as usize).saturating_sub(1);
    let err = Concurrency::with_runtime_mut(|rt| {
        let Some(slot) = rt.process_children.get_mut(idx) else {
            return Some("invalid ProcessChild".to_string());
        };
        let Some(inner) = slot.inner.as_mut() else {
            return None;
        };
        match inner.kill() {
            Ok(()) => None,
            Err(e) => Some(e.to_string()),
        }
    });
    match err {
        None => result_ok_bits(0),
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
                            let _ = inner.kill();
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

pub(crate) struct ProcessHostFns {
    pub cmd: cranelift_module::FuncId,
    pub run: cranelift_module::FuncId,
    pub pipeline: cranelift_module::FuncId,
    pub spec_stdout: cranelift_module::FuncId,
    pub spec_stderr: cranelift_module::FuncId,
    pub spec_stdin: cranelift_module::FuncId,
    pub spec_timeout: cranelift_module::FuncId,
    pub spec_output_limit: cranelift_module::FuncId,
    pub spec_cwd: cranelift_module::FuncId,
    pub spec_env: cranelift_module::FuncId,
    pub spec_env_remove: cranelift_module::FuncId,
    pub spec_env_clear: cranelift_module::FuncId,
    pub spec_detached: cranelift_module::FuncId,
    pub spec_terminal: cranelift_module::FuncId,
    pub spec_terminal_with_policy: cranelift_module::FuncId,
    pub spec_capabilities: cranelift_module::FuncId,
    pub spec_run: cranelift_module::FuncId,
    pub spec_run_checked: cranelift_module::FuncId,
    pub spec_spawn: cranelift_module::FuncId,
    pub child_id: cranelift_module::FuncId,
    pub child_kill: cranelift_module::FuncId,
    pub child_wait: cranelift_module::FuncId,
    pub terminal_resize: cranelift_module::FuncId,
    pub stream_lines: cranelift_module::FuncId,
}

pub(crate) fn register_process_symbols(builder: &mut cranelift_jit::JITBuilder) {
    builder.symbol("jet_jit_process_cmd", jet_jit_process_cmd as *const u8);
    builder.symbol("jet_jit_process_run", jet_jit_process_run as *const u8);
    builder.symbol("jet_jit_process_pipeline", jet_jit_process_pipeline as *const u8);
    builder.symbol("jet_jit_process_spec_stdout", jet_jit_process_spec_stdout as *const u8);
    builder.symbol("jet_jit_process_spec_stderr", jet_jit_process_spec_stderr as *const u8);
    builder.symbol("jet_jit_process_spec_stdin", jet_jit_process_spec_stdin as *const u8);
    builder.symbol("jet_jit_process_spec_timeout", jet_jit_process_spec_timeout as *const u8);
    builder.symbol(
        "jet_jit_process_spec_output_limit",
        jet_jit_process_spec_output_limit as *const u8,
    );
    builder.symbol("jet_jit_process_spec_cwd", jet_jit_process_spec_cwd as *const u8);
    builder.symbol("jet_jit_process_spec_env", jet_jit_process_spec_env as *const u8);
    builder.symbol(
        "jet_jit_process_spec_env_remove",
        jet_jit_process_spec_env_remove as *const u8,
    );
    builder.symbol(
        "jet_jit_process_spec_env_clear",
        jet_jit_process_spec_env_clear as *const u8,
    );
    builder.symbol(
        "jet_jit_process_spec_detached",
        jet_jit_process_spec_detached as *const u8,
    );
    builder.symbol(
        "jet_jit_process_spec_terminal",
        jet_jit_process_spec_terminal as *const u8,
    );
    builder.symbol(
        "jet_jit_process_spec_terminal_with_policy",
        jet_jit_process_spec_terminal_with_policy as *const u8,
    );
    builder.symbol(
        "jet_jit_process_spec_capabilities",
        jet_jit_process_spec_capabilities as *const u8,
    );
    builder.symbol(
        "jet_jit_terminal_session_resize",
        jet_jit_terminal_session_resize as *const u8,
    );
    builder.symbol("jet_jit_process_spec_run", jet_jit_process_spec_run as *const u8);
    builder.symbol(
        "jet_jit_process_spec_run_checked",
        jet_jit_process_spec_run_checked as *const u8,
    );
    builder.symbol("jet_jit_process_spec_spawn", jet_jit_process_spec_spawn as *const u8);
    builder.symbol("jet_jit_process_child_id", jet_jit_process_child_id as *const u8);
    builder.symbol("jet_jit_process_child_kill", jet_jit_process_child_kill as *const u8);
    builder.symbol("jet_jit_process_child_wait", jet_jit_process_child_wait as *const u8);
    builder.symbol("jet_jit_process_stream_lines", jet_jit_process_stream_lines as *const u8);
}

pub(crate) fn declare_process_host_fns(
    module: &mut cranelift_jit::JITModule,
) -> Result<ProcessHostFns, String> {
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

    let mut import = |name: &str, sig: &Signature| -> Result<cranelift_module::FuncId, String> {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };

    Ok(ProcessHostFns {
        cmd: import("jet_jit_process_cmd", &sig_unary)?,
        run: import("jet_jit_process_run", &sig_unary)?,
        pipeline: import("jet_jit_process_pipeline", &sig_unary)?,
        spec_stdout: import("jet_jit_process_spec_stdout", &sig_binary)?,
        spec_stderr: import("jet_jit_process_spec_stderr", &sig_binary)?,
        spec_stdin: import("jet_jit_process_spec_stdin", &sig_binary)?,
        spec_timeout: import("jet_jit_process_spec_timeout", &sig_binary)?,
        spec_output_limit: import("jet_jit_process_spec_output_limit", &sig_binary)?,
        spec_cwd: import("jet_jit_process_spec_cwd", &sig_binary)?,
        spec_env: import("jet_jit_process_spec_env", &sig_ternary)?,
        spec_env_remove: import("jet_jit_process_spec_env_remove", &sig_binary)?,
        spec_env_clear: import("jet_jit_process_spec_env_clear", &sig_unary)?,
        spec_detached: import("jet_jit_process_spec_detached", &sig_unary)?,
        spec_terminal: import("jet_jit_process_spec_terminal", &sig_unary)?,
        spec_terminal_with_policy: import(
            "jet_jit_process_spec_terminal_with_policy",
            &sig_binary,
        )?,
        spec_capabilities: import("jet_jit_process_spec_capabilities", &sig_unary)?,
        spec_run: import("jet_jit_process_spec_run", &sig_unary)?,
        spec_run_checked: import("jet_jit_process_spec_run_checked", &sig_unary)?,
        spec_spawn: import("jet_jit_process_spec_spawn", &sig_unary)?,
        child_id: import("jet_jit_process_child_id", &sig_unary)?,
        child_kill: import("jet_jit_process_child_kill", &sig_unary)?,
        child_wait: import("jet_jit_process_child_wait", &sig_unary)?,
        terminal_resize: import("jet_jit_terminal_session_resize", &sig_binary)?,
        stream_lines: import("jet_jit_process_stream_lines", &sig_binary)?,
    })
}
