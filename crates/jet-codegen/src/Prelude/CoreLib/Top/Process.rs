fn jet_process_spec_timeout(
    mut spec: jet_std::ProcessSpec,
    timeout: &jet_std::Duration,
) -> jet_std::ProcessSpec {
    spec.timeout_ms = Some(timeout.as_millis().max(0));
    spec
}
fn jet_process_spec_output_limit(
    mut spec: jet_std::ProcessSpec,
    output_limit: i64,
) -> jet_std::ProcessSpec {
    spec.output_limit = Some(output_limit.max(0));
    spec
}
fn jet_process_spec_detached(mut spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    spec.detached = true;
    spec
}
// D-PROCESS-SESSION1=A: one opt-in for a terminal-backed session. It stays on
// the same `ProcessSpec`, so cwd, environment, streams, timeout, and the child
// lifecycle keep one model.
fn jet_process_spec_terminal(mut spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    spec.terminal = Some(jet_std::TerminalPolicy::default());
    spec
}
fn jet_process_spec_terminal_with_policy(
    mut spec: jet_std::ProcessSpec,
    policy: &jet_std::TerminalPolicy,
) -> jet_std::ProcessSpec {
    spec.terminal = Some(policy.clone());
    spec
}
// D-PROCESS-SESSION2=D: an open keyed report lets new preview facts use String
// keys without changing a public report type. Known facts come from the
// checked TerminalFact namespace. Unix PTY support advertises the three
// stable facts; unsupported targets keep the set empty.
fn jet_process_spec_capabilities(
    _spec: &jet_std::ProcessSpec,
) -> std::collections::HashSet<String> {
    let mut facts = std::collections::HashSet::new();
    if jet_process_pty::supported() {
        facts.insert("terminal".to_string());
        facts.insert("resize".to_string());
        facts.insert("raw".to_string());
    }
    facts
}
// D-PROCESS-SESSION1=A: a terminal session needs a native backend. Running the
// child on plain pipes instead would change what an interactive program prints,
// so the launch fails rather than silently dropping the requested terminal.
fn jet_process_terminal_backend_check(
    spec: &jet_std::ProcessSpec,
) -> Result<(), jet_std::IOError> {
    if spec.terminal.is_none() {
        return Ok(());
    }
    if jet_process_pty::supported() {
        return Ok(());
    }
    Err(jet_std::IOError::other(
        jet_std::IOOperation::Resolve,
        spec.cmd.first().cloned(),
        "terminal sessions need a native PTY or ConPTY backend, and this build has none",
    ))
}
fn jet_process_stdio(mode: &jet_std::ProcessStreamMode) -> std::process::Stdio {
    match mode {
        // `Stream` and `Capture` both pipe — they differ only in which Jet API
        // is meant to drain the pipe (see `ProcessStreamMode` in CommonTypes.rs).
        jet_std::ProcessStreamMode::Stream | jet_std::ProcessStreamMode::Capture => {
            std::process::Stdio::piped()
        }
        jet_std::ProcessStreamMode::Inherit => std::process::Stdio::inherit(),
    }
}
fn jet_process_command_base(
    spec: &jet_std::ProcessSpec,
) -> Result<std::process::Command, jet_std::IOError> {
    if spec.cmd.is_empty() {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve,
            None,
            None,
            Some("process command needs at least one word".to_string()),
        )));
    }
    let mut command = std::process::Command::new(&spec.cmd[0]);
    command.args(&spec.cmd[1..]);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    // D-ENV-MUTATE1=A: clone one logical-environment snapshot under its read
    // lock, then compose ProcessSpec overrides in owned memory. Every launch is
    // untorn and never rereads the mutable host environment.
    let mut child_env = if spec.env_clear {
        Vec::new()
    } else {
        jet_std_env_snapshot_raw()
    };
    for (name, value) in &spec.env_set {
        jet_env_validate_name(name).map_err(|error| jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve, Some(name.clone()), None, Some(error.jet_show()),
        )))?;
        jet_env_validate_value(value).map_err(|error| jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve, Some(name.clone()), None, Some(error.jet_show()),
        )))?;
        let os_name = std::ffi::OsString::from(name);
        child_env.retain(|(candidate, _)| {
            !jet_env_key_eq(candidate.as_os_str(), os_name.as_os_str())
        });
        child_env.push((os_name, std::ffi::OsString::from(value)));
    }
    for name in &spec.env_remove {
        jet_env_validate_name(name).map_err(|error| jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve, Some(name.clone()), None, Some(error.jet_show()),
        )))?;
        let name = std::ffi::OsStr::new(name);
        child_env.retain(|(candidate, _)| !jet_env_key_eq(candidate.as_os_str(), name));
    }
    command.env_clear();
    command.envs(child_env);
    // D-PROCESS1=A: no `.stdin(...)` call (default) closes the child's stdin —
    // no accidental terminal/parent-stdin inheritance.
    command.stdin(match &spec.stdin {
        Some(mode) => jet_process_stdio(mode),
        None => std::process::Stdio::null(),
    });
    command.stdout(jet_process_stdio(&spec.stdout));
    command.stderr(jet_process_stdio(&spec.stderr));
    Ok(command)
}

// Pipelines use ordinary pipe edges. A PTY session is one bidirectional byte
// stream with one controlling process group, so it cannot be silently coerced
// into a pipeline edge. Keep the failure explicit and direct callers to
// `spawn()` for the terminal-backed child.
fn jet_process_command(
    spec: &jet_std::ProcessSpec,
) -> Result<std::process::Command, jet_std::IOError> {
    if spec.terminal.is_some() {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "terminal sessions cannot be used as pipeline stages; spawn the session directly",
        ));
    }
    jet_process_command_base(spec)
}
fn jet_process_spec_spawn(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessChild, jet_std::IOError> {
    jet_process_terminal_backend_check(spec)?;
    if spec.terminal.is_some() {
        return jet_process_terminal_spawn(spec);
    }
    let mut command = jet_process_command_base(spec)?;
    if spec.detached {
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Resolve, spec.cmd.first().cloned(), error)
    })?;
    Ok(jet_std::ProcessChild {
        stdin: std::rc::Rc::new(std::cell::RefCell::new(
            child.stdin.take().map(jet_std::ProcessStdin::Pipe),
        )),
        stdout: std::rc::Rc::new(std::cell::RefCell::new(
            child
                .stdout
                .take()
                .map(jet_std::ProcessReader::Stdout)
                .map(std::io::BufReader::new),
        )),
        stderr: std::rc::Rc::new(std::cell::RefCell::new(
            child
                .stderr
                .take()
                .map(jet_std::ProcessReader::Stderr)
                .map(std::io::BufReader::new),
        )),
        terminal: None,
        inner: std::rc::Rc::new(std::cell::RefCell::new(Some(child))),
        timeout_ms: spec.timeout_ms,
        started: std::time::Instant::now(),
    })
}

#[cfg(unix)]
fn jet_process_terminal_spawn(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessChild, jet_std::IOError> {
    if spec.detached {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "terminal sessions cannot be detached",
        ));
    }
    let policy = spec.terminal.as_ref().expect("terminal spawn needs policy");
    let config = jet_process_pty::PtyConfig {
        cols: policy.size.cols,
        rows: policy.size.rows,
        raw: matches!(policy.mode, jet_std::TerminalMode::Raw),
    };
    let pair = jet_process_pty::open(config).map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            error,
        )
    })?;
    let mut command = jet_process_command_base(spec)?;
    let stdin = pair.slave.try_clone().map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Resolve, Some("process terminal".to_string()), error)
    })?;
    let stdout = pair.slave.try_clone().map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Resolve, Some("process terminal".to_string()), error)
    })?;
    let stderr = pair.slave.try_clone().map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Resolve, Some("process terminal".to_string()), error)
    })?;
    command.stdin(std::process::Stdio::from(stdin));
    command.stdout(std::process::Stdio::from(stdout));
    command.stderr(std::process::Stdio::from(stderr));
    jet_process_pty::attach_command(&mut command).map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Resolve, Some("process terminal".to_string()), error)
    })?;
    let child = command.spawn().map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Resolve, spec.cmd.first().cloned(), error)
    })?;
    drop(pair.slave);
    let master = std::rc::Rc::new(pair.master);
    let stdin = master.as_ref().try_clone().map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Resolve, Some("process terminal".to_string()), error)
    })?;
    let stdout = master.as_ref().try_clone().map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Resolve, Some("process terminal".to_string()), error)
    })?;
    Ok(jet_std::ProcessChild {
        stdin: std::rc::Rc::new(std::cell::RefCell::new(Some(
            jet_std::ProcessStdin::Terminal(stdin),
        ))),
        stdout: std::rc::Rc::new(std::cell::RefCell::new(Some(std::io::BufReader::new(
            jet_std::ProcessReader::Terminal(stdout),
        )))),
        // A PTY has one combined output stream. Do not create a second reader
        // on the same master: stderr is represented by the unified stdout
        // stream, matching native terminal behavior.
        stderr: std::rc::Rc::new(std::cell::RefCell::new(None)),
        terminal: Some(jet_std::TerminalSession { master }),
        inner: std::rc::Rc::new(std::cell::RefCell::new(Some(child))),
        timeout_ms: spec.timeout_ms,
        started: std::time::Instant::now(),
    })
}

#[cfg(not(unix))]
fn jet_process_terminal_spawn(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessChild, jet_std::IOError> {
    jet_process_terminal_backend_check(spec)?;
    Err(jet_std::IOError::other(
        jet_std::IOOperation::Resolve,
        spec.cmd.first().cloned(),
        "terminal sessions need a native PTY or ConPTY backend, and this build has none",
    ))
}
fn jet_process_drain_reader<R>(
    reader: Option<std::io::BufReader<R>>,
) -> Option<std::thread::JoinHandle<std::io::Result<String>>>
where
    R: std::io::Read + Send + 'static,
{
    reader.map(|mut reader| {
        std::thread::spawn(move || {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut reader, &mut text)?;
            Ok(text)
        })
    })
}
fn jet_process_start_output_drain(
    child: &jet_std::ProcessChild,
) -> (
    Option<std::thread::JoinHandle<std::io::Result<String>>>,
    Option<std::thread::JoinHandle<std::io::Result<String>>>,
) {
    let stdout = child.stdout.borrow_mut().take();
    let stderr = child.stderr.borrow_mut().take();
    (
        jet_process_drain_reader(stdout),
        jet_process_drain_reader(stderr),
    )
}
fn jet_process_finish_output_drain(
    drain: Option<std::thread::JoinHandle<std::io::Result<String>>>,
    stream: &'static str,
) -> Result<String, jet_std::IOError> {
    let Some(drain) = drain else {
        return Ok(String::new());
    };
    drain
        .join()
        .map_err(|_| {
            jet_std::IOError::other(
                jet_std::IOOperation::Read,
                Some(stream.to_string()),
                "process output reader panicked",
            )
        })?
        .map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Read,
                Some(stream.to_string()),
                error,
            )
        })
}
fn jet_process_collect_output(
    drains: (
        Option<std::thread::JoinHandle<std::io::Result<String>>>,
        Option<std::thread::JoinHandle<std::io::Result<String>>>,
    ),
) -> Result<(String, String), jet_std::IOError> {
    let output = jet_process_finish_output_drain(drains.0, "process stdout")?;
    let errors = jet_process_finish_output_drain(drains.1, "process stderr")?;
    Ok((output, errors))
}
fn jet_process_spec_run_inner(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessResult, jet_std::IOError> {
    let child = jet_process_spec_spawn(spec)?;
    let result = jet_process_child_wait(&child)?;
    if let Some(limit) = spec.output_limit {
        if (result.output.len() + result.errors.len()) as i64 > limit {
            return Err(jet_std::IOError::other(jet_std::IOOperation::Read, None, "process output exceeded output_limit"));
        }
    }
    Ok(result)
}
fn jet_process_spec_run(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessResult, jet_std::IOError> {
    jet_process_spec_run_inner(spec)
}
fn jet_process_checked_stderr(errors: &str) -> String {
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
fn jet_process_spec_run_checked(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessResult, jet_std::IOError> {
    let result = jet_process_spec_run(spec)?;
    if result.success {
        return Ok(result);
    }
    let mut cause = format!("process exited unsuccessfully: code={}", result.code);
    if let Some(signal) = result.signal {
        cause.push_str(&format!(", signal={signal}"));
    }
    cause.push_str(&format!(
        ", stderr={}",
        jet_process_checked_stderr(&result.errors)
    ));
    Err(jet_std::IOError::other(
        jet_std::IOOperation::Close,
        spec.cmd.first().cloned(),
        cause,
    ))
}
fn jet_process_child_id(child: &jet_std::ProcessChild) -> i64 {
    child
        .inner
        .borrow()
        .as_ref()
        .map(|c| c.id() as i64)
        .unwrap_or(0)
}
fn jet_process_child_wait(
    child: &jet_std::ProcessChild,
) -> Result<jet_std::ProcessResult, jet_std::IOError> {
    // Capture pipes must be drained while the child runs. Waiting first can
    // deadlock when either pipe fills; stdout and stderr need independent
    // readers because a child may fill both concurrently. Stream consumers
    // keep their earlier reads, and wait drains only the remaining bytes.
    let drains = jet_process_start_output_drain(child);
    let mut timed_out = false;
    let status = loop {
        let mut slot = child.inner.borrow_mut();
        let Some(inner) = slot.as_mut() else {
            let (output, errors) = jet_process_collect_output(drains)?;
            return Ok(jet_std::ProcessResult {
                code: 0,
                success: true,
                signal: None,
                timed_out: false,
                output,
                errors,
            });
        };
        if let Some(status) = inner.try_wait().map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))? {
            break status;
        }
        if let Some(timeout) = child.timeout_ms {
            if child.started.elapsed() >= std::time::Duration::from_millis(timeout as u64) {
                #[cfg(unix)]
                if child.terminal.is_some()
                    && jet_process_pty::signal_group(inner.id(), jet_process_signal_kill()).is_err()
                {
                    inner.kill().map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))?;
                }
                #[cfg(not(unix))]
                inner.kill().map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))?;
                #[cfg(unix)]
                if child.terminal.is_none() {
                    inner.kill().map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))?;
                }
                timed_out = true;
                break inner.wait().map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))?;
            }
        }
        drop(slot);
        // D-TASKRUNTIME1=A: process waits are scheduler wait points. Parking
        // here keeps the worker available and makes inherited cancellation and
        // deadlines wake the wait exactly like channel, timer, and I/O waits.
        jet_scheduler_park_ms("process wait", 10);
    };
    child.inner.borrow_mut().take();
    let (output, errors) = jet_process_collect_output(drains)?;
    let code = status.code().unwrap_or(-1) as i64;
    #[cfg(unix)]
    let signal = std::os::unix::process::ExitStatusExt::signal(&status).map(i64::from);
    #[cfg(not(unix))]
    let signal = None;
    Ok(jet_std::ProcessResult {
        code,
        success: status.success(),
        signal,
        timed_out,
        output,
        errors,
    })
}
// #1481 core.process: a non-blocking companion to `wait()` — reports whether
// the child has already exited without draining its output pipes or
// blocking. Peeks the same underlying handle `id`/`kill` already borrow.
fn jet_process_child_exited(child: &jet_std::ProcessChild) -> Result<bool, jet_std::IOError> {
    let mut slot = child.inner.borrow_mut();
    let Some(inner) = slot.as_mut() else {
        return Ok(true);
    };
    inner
        .try_wait()
        .map(|status| status.is_some())
        .map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Close, Some("process".to_string()), error))
}

fn jet_process_child_kill(child: &jet_std::ProcessChild) -> Result<(), jet_std::IOError> {
    jet_process_child_signal(child, jet_process_signal_kill())
}
fn jet_process_child_terminate(child: &jet_std::ProcessChild) -> Result<(), jet_std::IOError> {
    jet_process_child_signal(child, jet_process_signal_terminate())
}
fn jet_process_child_interrupt(child: &jet_std::ProcessChild) -> Result<(), jet_std::IOError> {
    jet_process_child_signal(child, jet_process_signal_interrupt())
}
fn jet_terminal_session_resize(
    session: &jet_std::TerminalSession,
    size: &jet_std::TerminalSize,
) -> Result<(), jet_std::IOError> {
    jet_process_pty::resize(
        session.master.as_ref(),
        jet_process_pty::PtyConfig {
            cols: size.cols,
            rows: size.rows,
            raw: false,
        },
    )
    .map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            Some("process terminal".to_string()),
            error,
        )
    })
}

impl std::io::Read for jet_std::ProcessReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let result = match self {
            Self::Stdout(reader) => std::io::Read::read(reader, bytes),
            Self::Stderr(reader) => std::io::Read::read(reader, bytes),
            Self::Terminal(reader) => std::io::Read::read(reader, bytes),
        };
        match result {
            Err(error) if matches!(self, Self::Terminal(_)) && jet_process_pty::is_terminal_eof(&error) => {
                Ok(0)
            }
            other => other,
        }
    }
}

impl std::io::Write for jet_std::ProcessStdin {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Pipe(writer) => std::io::Write::write(writer, bytes),
            Self::Terminal(writer) => std::io::Write::write(writer, bytes),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Pipe(writer) => std::io::Write::flush(writer),
            Self::Terminal(writer) => std::io::Write::flush(writer),
        }
    }
}

#[cfg(unix)]
fn jet_process_signal_interrupt() -> i32 {
    jet_process_pty::SIGINT
}

#[cfg(not(unix))]
fn jet_process_signal_interrupt() -> i32 {
    0
}

#[cfg(unix)]
fn jet_process_signal_terminate() -> i32 {
    jet_process_pty::SIGTERM
}

#[cfg(not(unix))]
fn jet_process_signal_terminate() -> i32 {
    0
}

#[cfg(unix)]
fn jet_process_signal_kill() -> i32 {
    jet_process_pty::SIGKILL
}

#[cfg(not(unix))]
fn jet_process_signal_kill() -> i32 {
    0
}

fn jet_process_child_signal(
    child: &jet_std::ProcessChild,
    signal: i32,
) -> Result<(), jet_std::IOError> {
    if let Some(inner) = child.inner.borrow_mut().as_mut() {
        #[cfg(unix)]
        if child.terminal.is_some() {
            if jet_process_pty::signal_group(inner.id(), signal).is_ok() {
                return Ok(());
            }
        }
        inner.kill().map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Close,
                Some("process".to_string()),
                error,
            )
        })?;
    }
    Ok(())
}
// D-PROCESS1=A: `child.stdin` is a writer handle (`.write(text)`); `child.stdout`/
// `child.stderr` are streaming reader handles consumed only via
// `loop line; child.stdout.lines() { ... }` (mirrors `FileReader`/`StdinHandle`
// — sema restricts the field access + `.lines()` result to that position, E2502).
fn jet_process_stdin_write(
    handle: &std::rc::Rc<std::cell::RefCell<Option<jet_std::ProcessStdin>>>,
    text: &String,
) -> Result<(), jet_std::IOError> {
    if let Some(stdin) = handle.borrow_mut().as_mut() {
        std::io::Write::write_all(stdin, text.as_bytes()).map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Write, Some("process stdin".to_string()), error))?;
    }
    Ok(())
}
fn jet_process_child_read_line<R: std::io::Read>(
    reader: &mut Option<std::io::BufReader<R>>,
) -> Result<Option<String>, jet_std::IOError> {
    let Some(reader) = reader.as_mut() else {
        return Ok(None);
    };
    let mut line = String::new();
    let n = std::io::BufRead::read_line(reader, &mut line).map_err(|error| jet_std::IOError::other(jet_std::IOOperation::Read, Some("process output".to_string()), error))?;
    if n == 0 {
        Ok(None)
    } else {
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        Ok(Some(line))
    }
}
fn jet_process_stream_next_line<R: std::io::Read>(
    handle: &std::rc::Rc<std::cell::RefCell<Option<std::io::BufReader<R>>>>,
) -> Result<Option<String>, jet_std::IOError> {
    jet_process_child_read_line(&mut handle.borrow_mut())
}
