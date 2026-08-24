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
fn jet_process_spec_cpu_time_limit(
    mut spec: jet_std::ProcessSpec,
    cpu_time: &jet_std::Duration,
) -> jet_std::ProcessSpec {
    spec.cpu_time_limit_ms = Some(cpu_time.as_millis().max(0));
    spec
}
fn jet_process_spec_memory_limit(
    mut spec: jet_std::ProcessSpec,
    memory_bytes: i64,
) -> jet_std::ProcessSpec {
    spec.memory_limit_bytes = Some(memory_bytes.max(0));
    spec
}
fn jet_process_spec_open_file_limit(
    mut spec: jet_std::ProcessSpec,
    open_files: i64,
) -> jet_std::ProcessSpec {
    spec.open_file_limit = Some(open_files.max(0));
    spec
}

fn jet_process_native_limits(spec: &jet_std::ProcessSpec) -> jet_process_pty::ResourceLimits {
    jet_process_pty::ResourceLimits {
        cpu_time_ms: spec.cpu_time_limit_ms,
        memory_bytes: spec.memory_limit_bytes,
        open_files: spec.open_file_limit,
    }
}

/// Refuse a request before spawn when this target has no honest native
/// enforcement path. Unix uses the child `setrlimit` seam; Windows uses Job
/// Objects for CPU and memory, but has no portable per-process descriptor cap.
fn jet_process_resource_limits_check(
    spec: &jet_std::ProcessSpec,
) -> Result<(), jet_std::IOError> {
    #[cfg(target_os = "macos")]
    if spec.memory_limit_bytes.is_some() || spec.open_file_limit.is_some() {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "macOS can enforce CPU limits here, but cannot report typed memory or open-file exhaustion; refusing those requests before spawn",
        ));
    }
    #[cfg(windows)]
    if spec.open_file_limit.is_some() {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "open_file_limit cannot be enforced by Windows Job Objects; refusing before spawn",
        ));
    }
    #[cfg(windows)]
    if spec.detached && (spec.cpu_time_limit_ms.is_some() || spec.memory_limit_bytes.is_some()) {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "detached Windows children cannot carry CPU or memory Job Object limits; refusing before spawn",
        ));
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows"
    )))]
    if spec.cpu_time_limit_ms.is_some()
        || spec.memory_limit_bytes.is_some()
        || spec.open_file_limit.is_some()
    {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "process CPU, memory, and open-file limits are unsupported on this target; refusing before spawn",
        ));
    }
    let _ = spec;
    Ok(())
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
fn jet_process_spec_abilities(_spec: &jet_std::ProcessSpec) -> std::collections::HashSet<String> {
    let mut facts = std::collections::HashSet::new();
    for fact in jet_process_policy::terminal_facts(jet_process_pty::supported()) {
        facts.insert((*fact).to_string());
    }
    facts
}
// D-PROCESS-SESSION1=A: a terminal session needs a native backend. Running the
// child on plain pipes instead would change what an interactive program prints,
// so the launch fails rather than silently dropping the requested terminal.
fn jet_process_terminal_backend_check(spec: &jet_std::ProcessSpec) -> Result<(), jet_std::IOError> {
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

// D-ENV-MUTATE1=A: one composed snapshot feeds both std::process and the
// Windows CreateProcessW backend. Do not let the terminal path grow a second
// environment policy.
fn jet_process_environment(spec: &jet_std::ProcessSpec) -> Result<JetEnvEntries, jet_std::IOError> {
    let mut child_env = if spec.env_clear {
        Vec::new()
    } else {
        jet_std_env_snapshot_raw()
    };
    for (name, value) in &spec.env_set {
        jet_env_validate_name(name).map_err(|error| {
            jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Resolve,
                Some(name.clone()),
                None,
                Some(error.jet_show()),
            ))
        })?;
        jet_env_validate_value(value).map_err(|error| {
            jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Resolve,
                Some(name.clone()),
                None,
                Some(error.jet_show()),
            ))
        })?;
        let os_name = std::ffi::OsString::from(name);
        child_env
            .retain(|(candidate, _)| !jet_env_key_eq(candidate.as_os_str(), os_name.as_os_str()));
        child_env.push((os_name, std::ffi::OsString::from(value)));
    }
    for name in &spec.env_remove {
        jet_env_validate_name(name).map_err(|error| {
            jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Resolve,
                Some(name.clone()),
                None,
                Some(error.jet_show()),
            ))
        })?;
        let name = std::ffi::OsStr::new(name);
        child_env.retain(|(candidate, _)| !jet_env_key_eq(candidate.as_os_str(), name));
    }
    Ok(child_env)
}

fn jet_process_command_base_with_identity(
    spec: &jet_std::ProcessSpec,
    executable_identity: Option<&str>,
) -> Result<std::process::Command, jet_std::IOError> {
    if spec.cmd.is_empty() {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve,
            None,
            None,
            Some("process command needs at least one word".to_string()),
        )));
    }
    let mut command = std::process::Command::new(executable_identity.unwrap_or(&spec.cmd[0]));
    command.args(&spec.cmd[1..]);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    let child_env = jet_process_environment(spec)?;
    command.env_clear();
    command.envs(child_env);
    // No `.stdin(...)` call (default) closes the child's stdin —
    // no accidental terminal/parent-stdin inheritance.
    command.stdin(match &spec.stdin {
        Some(mode) => jet_process_stdio(mode),
        None => std::process::Stdio::null(),
    });
    command.stdout(jet_process_stdio(&spec.stdout));
    command.stderr(jet_process_stdio(&spec.stderr));
    Ok(command)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn jet_process_sandbox_error(
    spec: &jet_std::ProcessSpec,
    error: jet_process_sandbox::Error,
) -> jet_std::IOError {
    let detail = match error {
        jet_process_sandbox::Error::Unsupported(detail)
        | jet_process_sandbox::Error::Io(detail) => detail,
    };
    jet_std::IOError::other(
        jet_std::IOOperation::Resolve,
        spec.cmd.first().cloned(),
        detail,
    )
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn jet_process_sandbox_env(
    spec: &jet_std::ProcessSpec,
) -> Result<std::collections::BTreeMap<String, String>, jet_std::IOError> {
    let mut environment = std::collections::BTreeMap::new();
    for (name, value) in &spec.env_set {
        jet_env_validate_name(name).map_err(|error| {
            jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Resolve,
                Some(name.clone()),
                None,
                Some(error.jet_show()),
            ))
        })?;
        jet_env_validate_value(value).map_err(|error| {
            jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Resolve,
                Some(name.clone()),
                None,
                Some(error.jet_show()),
            ))
        })?;
        environment.insert(name.clone(), value.clone());
    }
    for name in &spec.env_remove {
        jet_env_validate_name(name).map_err(|error| {
            jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Resolve,
                Some(name.clone()),
                None,
                Some(error.jet_show()),
            ))
        })?;
        environment.remove(name);
    }
    Ok(environment)
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn jet_process_sandbox_policy_scope(
    spec: &jet_std::ProcessSpec,
    executable_identity: &str,
) -> Result<(bool, bool, bool), jet_std::IOError> {
    let rights = spec
        .policy_wire
        .as_deref()
        .unwrap_or_default()
        .lines()
        .collect::<Vec<_>>();
    jet_process_policy_check_executable(spec, executable_identity)?;
    Ok((
        rights.iter().any(|right| *right == "FS.Read:repo"),
        rights.iter().any(|right| *right == "FS.Write:.jet/build"),
        rights.iter().any(|right| *right == "Net"),
    ))
}

fn jet_process_receipt(
    spec: &jet_std::ProcessSpec,
    plan: Option<&jet_std::ProcessPlan>,
    process_group: bool,
    pid: i64,
    code: i64,
    success: bool,
    signal: JetOutcome<i64, JetAbsent>,
    timed_out: bool,
    output: String,
    errors: String,
    extra_secret_values: Option<&[String]>,
) -> jet_std::ProcessReceipt {
    let owned_secret_values;
    let secret_values = if let Some(values) = extra_secret_values {
        values
    } else {
        owned_secret_values = jet_process_policy_secret_values(spec);
        &owned_secret_values
    };
    let executable_identity = plan
        .map(|plan| plan.executable_identity.clone())
        .or_else(|| jet_process_resolve_executable(spec).ok())
        .or_else(|| spec.cmd.first().cloned())
        .unwrap_or_default();
    let argv = plan.map(|plan| plan.argv.clone()).unwrap_or_else(|| {
        spec.cmd
            .iter()
            .map(|word| jet_process_policy_redact(spec, word))
            .collect()
    });
    let policy_digest = plan
        .map(|plan| plan.policy_digest.clone())
        .unwrap_or_else(|| jet_process_policy_digest(spec));
    let input_digest = plan
        .map(|plan| plan.input_digest.clone())
        .unwrap_or_else(|| jet_process_input_digest(spec));
    let backend = plan
        .map(|plan| plan.backend.clone())
        .unwrap_or_else(|| "ambient".to_string());
    let authority = plan
        .map(|plan| plan.authority.clone())
        .unwrap_or_else(|| jet_process_policy_receipt_rights(spec));
    let descendants = if spec.detached {
        "detached".to_string()
    } else if process_group {
        "contained".to_string()
    } else {
        plan.map(|plan| plan.descendants.clone())
            .unwrap_or_else(|| "direct".to_string())
    };
    let limits = plan
        .map(|plan| plan.limits.clone())
        .unwrap_or_else(|| jet_process_policy_limits(spec));
    let mut outputs = plan
        .map(|plan| plan.outputs.clone())
        .unwrap_or_else(|| jet_process_policy_outputs(spec));
    outputs.push(format!("captured-bytes={}", output.len() + errors.len()));
    let redacted = spec.policy_wire.is_some() || !secret_values.is_empty();
    jet_std::ProcessReceipt {
        code,
        output: jet_process_redact_text(&output, secret_values),
        errors: jet_process_redact_text(&errors, secret_values),
        success,
        signal,
        timed_out,
        executable_identity,
        argv,
        input_digest,
        policy_digest,
        backend,
        authority,
        descendants,
        limits,
        outputs,
        redacted,
        pid,
        limit_hit: jet_outcome_of(
            timed_out.then_some(jet_std::ProcessResourceLimit::WallTime),
        ),
    }
}

fn jet_process_child_from_inner(
    mut child: std::process::Child,
    spec: &jet_std::ProcessSpec,
    process_group: bool,
    plan: Option<jet_std::ProcessPlan>,
) -> Result<jet_std::ProcessChild, jet_std::IOError> {
    #[cfg(windows)]
    let job = if spec.detached {
        None
    } else {
        use std::os::windows::io::AsRawHandle;
        match jet_process_pty::attach_job(
            child.as_raw_handle(),
            jet_process_native_limits(spec),
        ) {
            Ok(job) => Some(std::rc::Rc::new(job)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(jet_std::IOError::other(
                    jet_std::IOOperation::Resolve,
                    spec.cmd.first().cloned(),
                    error,
                ));
            }
        }
    };
    #[cfg(not(windows))]
    let job = None;
    let process_group = process_group || job.is_some();
    let output_limit = spec.output_limit.map(|limit| limit.max(0) as usize);
    let output_budget =
        output_limit.map(|_| std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)));
    let output_limit_hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let output_read_error = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (stdout, stdout_state, stdout_worker) = jet_process_spawn_output_reader(
        child.stdout.take(),
        false,
        output_limit,
        output_budget.clone(),
        output_limit_hit.clone(),
        output_read_error.clone(),
    );
    let (stderr, stderr_state, stderr_worker) = jet_process_spawn_output_reader(
        child.stderr.take(),
        false,
        output_limit,
        output_budget,
        output_limit_hit.clone(),
        output_read_error.clone(),
    );
    Ok(jet_std::ProcessChild {
        wait_result: std::rc::Rc::new(std::cell::RefCell::new(None)),
        cleanup_error: std::rc::Rc::new(std::cell::RefCell::new(None)),
        stdin: std::rc::Rc::new(std::cell::RefCell::new(
            child.stdin.take().map(jet_std::ProcessStdin::Pipe),
        )),
        stdout: std::rc::Rc::new(std::cell::RefCell::new(stdout)),
        stderr: std::rc::Rc::new(std::cell::RefCell::new(stderr)),
        stdout_state,
        stderr_state,
        stdout_worker: std::rc::Rc::new(std::cell::RefCell::new(stdout_worker)),
        stderr_worker: std::rc::Rc::new(std::cell::RefCell::new(stderr_worker)),
        output_limit_hit,
        output_read_error,
        terminal: Err(JetAbsent),
        process_group,
        detached: spec.detached,
        inner: std::rc::Rc::new(std::cell::RefCell::new(Some(jet_std::ProcessHandle::Std {
            child,
            job,
        }))),
        timeout_ms: spec.timeout_ms,
        output_limit: spec.output_limit,
        audit_spec: spec.clone(),
        audit_plan: plan,
        started: std::time::Instant::now(),
    })
}

fn jet_process_verify_launch_plan(
    spec: &jet_std::ProcessSpec,
    plan: &jet_std::ProcessPlan,
) -> Result<(), jet_std::IOError> {
    let Some(backend) = jet_process_isolation_backend() else {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "authority-bound process launch rejected because its isolation backend is unavailable",
        ));
    };
    if plan.backend != backend {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "authority-bound process launch rejected because its isolation backend changed after planning",
        ));
    }
    if plan.policy_digest != jet_process_policy_digest(spec) {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "authority-bound process launch rejected a policy digest change after planning",
        ));
    }
    if plan.input_digest != jet_process_input_digest(spec) {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "authority-bound process launch rejected because its argv inputs changed after planning",
        ));
    }
    Ok(())
}

// Pipelines use ordinary pipe edges. A PTY session is one bidirectional byte
// stream with one controlling process group, so it cannot be silently coerced
// into a pipeline edge. Keep the failure explicit and direct callers to
// `spawn()` for the terminal-backed child.
fn jet_process_command_with_identity(
    spec: &jet_std::ProcessSpec,
    executable_identity: Option<&str>,
) -> Result<std::process::Command, jet_std::IOError> {
    if spec.terminal.is_some() {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "terminal sessions cannot be used as pipeline stages; spawn the session directly",
        ));
    }
    jet_process_command_base_with_identity(spec, executable_identity)
}
fn jet_process_spec_spawn(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessChild, jet_std::IOError> {
    jet_process_resource_limits_check(spec)?;
    let launch_plan = if spec.policy_wire.is_some() {
        Some(jet_process_spec_plan(spec)?)
    } else {
        None
    };
    jet_process_spec_backend_check(spec)?;
    if let Some(plan) = launch_plan.as_ref() {
        jet_process_verify_launch_plan(spec, plan)?;
    }
    jet_process_terminal_backend_check(spec)?;
    if spec.terminal.is_some() {
        return jet_process_terminal_spawn(
            spec,
            launch_plan
                .as_ref()
                .map(|plan| plan.executable_identity.as_str()),
            launch_plan.as_ref(),
        );
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if spec.policy_wire.is_some() {
        let plan = launch_plan
            .as_ref()
            .expect("authority launch must have a plan");
        let cwd = spec
            .cwd
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or_else(|| std::path::Path::new("."));
        let (source_readable, output_writable, share_network) =
            jet_process_sandbox_policy_scope(spec, &plan.executable_identity)?;
        let output_dir = if output_writable {
            Some(
                jet_process_sandbox::agent_output_dir(cwd)
                    .map_err(|error| jet_process_sandbox_error(spec, error))?,
            )
        } else {
            None
        };
        let environment = jet_process_sandbox_env(spec)?;
        let child = jet_process_sandbox::spawn(
            std::path::Path::new(&plan.executable_identity),
            &spec.cmd[1..],
            cwd,
            output_dir.as_deref(),
            &environment,
            share_network,
            source_readable,
            false,
            spec.cmd
                .first()
                .map(|word| std::ffi::OsStr::new(word.as_str())),
            |command| {
                if spec.detached {
                    command.stdin(std::process::Stdio::null());
                    command.stdout(std::process::Stdio::null());
                    command.stderr(std::process::Stdio::null());
                } else {
                    command.stdin(match &spec.stdin {
                        Some(mode) => jet_process_stdio(mode),
                        None => std::process::Stdio::null(),
                    });
                    command.stdout(jet_process_stdio(&spec.stdout));
                    command.stderr(jet_process_stdio(&spec.stderr));
                }
                jet_process_pty::attach_process_group(command, jet_process_native_limits(spec))
                    .map_err(|error| jet_process_sandbox::Error::Io(error.to_string()))?;
                Ok(())
            },
        )
        .map_err(|error| jet_process_sandbox_error(spec, error))?;
        return jet_process_child_from_inner(child, spec, true, launch_plan.clone());
    }

    #[cfg(target_os = "windows")]
    if spec.policy_wire.is_some() {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "authority-bound Windows process children require capture via run(); refusing an unsandboxed spawn",
        ));
    }

    let mut command = jet_process_command_base_with_identity(
        spec,
        launch_plan
            .as_ref()
            .map(|plan| plan.executable_identity.as_str()),
    )?;
    if spec.detached {
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
    }
    #[cfg(unix)]
    jet_process_pty::attach_process_group(&mut command, jet_process_native_limits(spec)).map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            error,
        )
    })?;
    let child = command.spawn().map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            error,
        )
    })?;
    // Ordinary Unix children get the same descendant boundary as terminal
    // sessions. Windows keeps direct-child cleanup until its native job
    // boundary is available.
    jet_process_child_from_inner(child, spec, cfg!(unix), launch_plan)
}

#[cfg(unix)]
fn jet_process_terminal_spawn(
    spec: &jet_std::ProcessSpec,
    executable_identity: Option<&str>,
    plan: Option<&jet_std::ProcessPlan>,
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
    let stdin = pair.slave.try_clone().map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            Some("process terminal".to_string()),
            error,
        )
    })?;
    let stdout = pair.slave.try_clone().map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            Some("process terminal".to_string()),
            error,
        )
    })?;
    let stderr = pair.slave.try_clone().map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            Some("process terminal".to_string()),
            error,
        )
    })?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let child = if spec.policy_wire.is_some() {
        let executable =
            executable_identity.expect("authority terminal launch must have a resolved executable");
        let cwd = spec
            .cwd
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or_else(|| std::path::Path::new("."));
        let (source_readable, output_writable, share_network) =
            jet_process_sandbox_policy_scope(spec, executable)?;
        let output_dir = if output_writable {
            Some(
                jet_process_sandbox::agent_output_dir(cwd)
                    .map_err(|error| jet_process_sandbox_error(spec, error))?,
            )
        } else {
            None
        };
        let environment = jet_process_sandbox_env(spec)?;
        jet_process_sandbox::spawn(
            std::path::Path::new(executable),
            &spec.cmd[1..],
            cwd,
            output_dir.as_deref(),
            &environment,
            share_network,
            source_readable,
            false,
            spec.cmd
                .first()
                .map(|word| std::ffi::OsStr::new(word.as_str())),
            |command| {
                command.stdin(std::process::Stdio::from(stdin));
                command.stdout(std::process::Stdio::from(stdout));
                command.stderr(std::process::Stdio::from(stderr));
                jet_process_pty::attach_command(command, jet_process_native_limits(spec))
                    .map_err(|error| jet_process_sandbox::Error::Io(error.to_string()))
            },
        )
        .map_err(|error| jet_process_sandbox_error(spec, error))?
    } else {
        let mut command = jet_process_command_base_with_identity(spec, executable_identity)?;
        command.stdin(std::process::Stdio::from(stdin));
        command.stdout(std::process::Stdio::from(stdout));
        command.stderr(std::process::Stdio::from(stderr));
        jet_process_pty::attach_command(&mut command, jet_process_native_limits(spec)).map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Resolve,
                Some("process terminal".to_string()),
                error,
            )
        })?;
        command.spawn().map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Resolve,
                spec.cmd.first().cloned(),
                error,
            )
        })?
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let child = {
        let mut command = jet_process_command_base_with_identity(spec, executable_identity)?;
        command.stdin(std::process::Stdio::from(stdin));
        command.stdout(std::process::Stdio::from(stdout));
        command.stderr(std::process::Stdio::from(stderr));
        jet_process_pty::attach_command(&mut command, jet_process_native_limits(spec)).map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Resolve,
                Some("process terminal".to_string()),
                error,
            )
        })?;
        command.spawn().map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Resolve,
                spec.cmd.first().cloned(),
                error,
            )
        })?
    };
    drop(pair.slave);
    let master = std::rc::Rc::new(pair.master);
    let stdin = master.as_ref().try_clone().map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            Some("process terminal".to_string()),
            error,
        )
    })?;
    let stdout = master.as_ref().try_clone().map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            Some("process terminal".to_string()),
            error,
        )
    })?;
    let output_limit = spec.output_limit.map(|limit| limit.max(0) as usize);
    let output_budget =
        output_limit.map(|_| std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)));
    let output_limit_hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let output_read_error = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (stdout, stdout_state, stdout_worker) = jet_process_spawn_output_reader(
        Some(stdout),
        true,
        output_limit,
        output_budget,
        output_limit_hit.clone(),
        output_read_error.clone(),
    );
    Ok(jet_std::ProcessChild {
        wait_result: std::rc::Rc::new(std::cell::RefCell::new(None)),
        cleanup_error: std::rc::Rc::new(std::cell::RefCell::new(None)),
        stdin: std::rc::Rc::new(std::cell::RefCell::new(Some(
            jet_std::ProcessStdin::Terminal(stdin),
        ))),
        stdout: std::rc::Rc::new(std::cell::RefCell::new(stdout)),
        // A PTY has one combined output stream. Do not create a second reader
        // on the same master: stderr is represented by the unified stdout
        // stream, matching native terminal behavior.
        stderr: std::rc::Rc::new(std::cell::RefCell::new(None)),
        stdout_state,
        stderr_state: None,
        stdout_worker: std::rc::Rc::new(std::cell::RefCell::new(stdout_worker)),
        stderr_worker: std::rc::Rc::new(std::cell::RefCell::new(None)),
        output_limit_hit,
        output_read_error,
        terminal: Ok(jet_std::TerminalSession { master }),
        process_group: true,
        detached: spec.detached,
        inner: std::rc::Rc::new(std::cell::RefCell::new(Some(jet_std::ProcessHandle::Std {
            child,
            job: None,
        }))),
        timeout_ms: spec.timeout_ms,
        output_limit: spec.output_limit,
        audit_spec: spec.clone(),
        audit_plan: plan.cloned(),
        started: std::time::Instant::now(),
    })
}

#[cfg(windows)]
fn jet_process_terminal_spawn(
    spec: &jet_std::ProcessSpec,
    executable_identity: Option<&str>,
    plan: Option<&jet_std::ProcessPlan>,
) -> Result<jet_std::ProcessChild, jet_std::IOError> {
    if spec.detached {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "terminal sessions cannot be detached",
        ));
    }
    let policy = spec.terminal.as_ref().expect("terminal spawn needs policy");
    let environment = jet_process_environment(spec)?;
    let executable = executable_identity
        .or_else(|| spec.cmd.first().map(String::as_str))
        .ok_or_else(|| {
            jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Resolve,
                None,
                None,
                Some("process command needs at least one word".to_string()),
            ))
        })?;
    let native = jet_process_pty::spawn(
        jet_process_pty::PtyConfig {
            cols: policy.size.cols,
            rows: policy.size.rows,
            raw: matches!(policy.mode, jet_std::TerminalMode::Raw),
        },
        executable,
        &spec.cmd[1..],
        spec.cwd.as_deref(),
        &environment,
        jet_process_native_limits(spec),
    )
    .map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            error,
        )
    })?;
    let control = std::rc::Rc::new(jet_std::ConPtyControl::new(native.console));
    let output_limit = spec.output_limit.map(|limit| limit.max(0) as usize);
    let output_budget =
        output_limit.map(|_| std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)));
    let output_limit_hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let output_read_error = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (stdout, stdout_state, stdout_worker) = jet_process_spawn_output_reader(
        Some(native.output),
        true,
        output_limit,
        output_budget,
        output_limit_hit.clone(),
        output_read_error.clone(),
    );
    Ok(jet_std::ProcessChild {
        wait_result: std::rc::Rc::new(std::cell::RefCell::new(None)),
        cleanup_error: std::rc::Rc::new(std::cell::RefCell::new(None)),
        stdin: std::rc::Rc::new(std::cell::RefCell::new(Some(
            jet_std::ProcessStdin::Terminal(native.input),
        ))),
        stdout: std::rc::Rc::new(std::cell::RefCell::new(stdout)),
        stderr: std::rc::Rc::new(std::cell::RefCell::new(None)),
        stdout_state,
        stderr_state: None,
        stdout_worker: std::rc::Rc::new(std::cell::RefCell::new(stdout_worker)),
        stderr_worker: std::rc::Rc::new(std::cell::RefCell::new(None)),
        output_limit_hit,
        output_read_error,
        terminal: Ok(jet_std::TerminalSession { control }),
        process_group: true,
        detached: false,
        inner: std::rc::Rc::new(std::cell::RefCell::new(Some(
            jet_std::ProcessHandle::Native {
                process: native.process,
                job: std::rc::Rc::new(native.job),
                pid: native.pid,
            },
        ))),
        timeout_ms: spec.timeout_ms,
        output_limit: spec.output_limit,
        audit_spec: spec.clone(),
        audit_plan: plan.cloned(),
        started: std::time::Instant::now(),
    })
}

#[cfg(not(any(unix, windows)))]
fn jet_process_terminal_spawn(
    spec: &jet_std::ProcessSpec,
    _executable_identity: Option<&str>,
    _plan: Option<&jet_std::ProcessPlan>,
) -> Result<jet_std::ProcessChild, jet_std::IOError> {
    jet_process_terminal_backend_check(spec)?;
    Err(jet_std::IOError::other(
        jet_std::IOOperation::Resolve,
        spec.cmd.first().cloned(),
        "terminal sessions need a native PTY or ConPTY backend, and this build has none",
    ))
}

fn jet_process_output_state_read(
    state: &std::sync::Arc<jet_std::ProcessOutputState>,
    bytes: &mut [u8],
) -> std::io::Result<usize> {
    if bytes.is_empty() {
        return Ok(0);
    }
    let mut buffer = state
        .bytes
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        if buffer.cursor < buffer.bytes.len() {
            let available = &buffer.bytes[buffer.cursor..];
            let count = available.len().min(bytes.len());
            bytes[..count].copy_from_slice(&available[..count]);
            buffer.cursor += count;
            return Ok(count);
        }
        if let Some((kind, message)) = &buffer.error {
            return Err(std::io::Error::new(*kind, message.clone()));
        }
        if buffer.closed {
            return Ok(0);
        }
        buffer = state
            .ready
            .wait(buffer)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
}

fn jet_process_output_state_snapshot(
    state: Option<&std::sync::Arc<jet_std::ProcessOutputState>>,
    stream: &'static str,
) -> Result<String, jet_std::IOError> {
    let Some(state) = state else {
        return Ok(String::new());
    };
    let mut buffer = state
        .bytes
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((kind, message)) = &buffer.error {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some(stream.to_string()),
            std::io::Error::new(*kind, message.clone()),
        ));
    }
    let bytes = std::mem::take(&mut buffer.bytes);
    String::from_utf8(bytes).map_err(|error| {
        jet_std::IOError::other(jet_std::IOOperation::Read, Some(stream.to_string()), error)
    })
}

fn jet_process_output_worker<R>(
    mut reader: R,
    state: std::sync::Arc<jet_std::ProcessOutputState>,
    terminal_eof: bool,
    limit: Option<usize>,
    budget: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    limit_hit: std::sync::Arc<std::sync::atomic::AtomicBool>,
    read_error: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::io::Result<()>
where
    R: std::io::Read + Send + 'static,
{
    let result = (|| {
        let mut chunk = [0u8; 8192];
        loop {
            let count = match std::io::Read::read(&mut reader, &mut chunk) {
                Ok(count) => count,
                Err(error) if terminal_eof && jet_process_pty::is_terminal_eof(&error) => 0,
                Err(error) => return Err(error),
            };
            if count == 0 {
                return Ok(());
            }
            let kept = match limit {
                None => count,
                Some(limit) => {
                    let budget = budget
                        .as_ref()
                        .expect("bounded process output needs a shared budget");
                    let mut used = budget.load(std::sync::atomic::Ordering::Acquire);
                    loop {
                        let available = limit.saturating_sub(used);
                        let kept = available.min(count);
                        if kept == 0 {
                            break 0;
                        }
                        match budget.compare_exchange(
                            used,
                            used + kept,
                            std::sync::atomic::Ordering::AcqRel,
                            std::sync::atomic::Ordering::Acquire,
                        ) {
                            Ok(_) => break kept,
                            Err(next) => used = next,
                        }
                    }
                }
            };
            if kept != 0 {
                let mut buffer = state
                    .bytes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                buffer.bytes.extend_from_slice(&chunk[..kept]);
                state.ready.notify_all();
            }
            if kept < count {
                limit_hit.store(true, std::sync::atomic::Ordering::Release);
                return Ok(());
            }
        }
    })();
    if let Err(error) = &result {
        read_error.store(true, std::sync::atomic::Ordering::Release);
        let mut buffer = state
            .bytes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if buffer.error.is_none() {
            buffer.error = Some((error.kind(), error.to_string()));
        }
    }
    let mut buffer = state
        .bytes
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    buffer.closed = true;
    state.ready.notify_all();
    result
}

fn jet_process_spawn_output_reader<R>(
    reader: Option<R>,
    terminal_eof: bool,
    limit: Option<usize>,
    budget: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    limit_hit: std::sync::Arc<std::sync::atomic::AtomicBool>,
    read_error: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> (
    Option<std::io::BufReader<jet_std::ProcessReader>>,
    Option<std::sync::Arc<jet_std::ProcessOutputState>>,
    Option<std::thread::JoinHandle<std::io::Result<()>>>,
)
where
    R: std::io::Read + Send + 'static,
{
    let Some(reader) = reader else {
        return (None, None, None);
    };
    let state = std::sync::Arc::new(jet_std::ProcessOutputState {
        bytes: std::sync::Mutex::new(jet_std::ProcessOutputBuffer {
            bytes: Vec::new(),
            cursor: 0,
            closed: false,
            error: None,
        }),
        ready: std::sync::Condvar::new(),
    });
    let worker_state = state.clone();
    let worker = std::thread::spawn(move || {
        jet_process_output_worker(
            reader,
            worker_state,
            terminal_eof,
            limit,
            budget,
            limit_hit,
            read_error,
        )
    });
    (
        Some(std::io::BufReader::new(jet_std::ProcessReader::Shared(
            state.clone(),
        ))),
        Some(state),
        Some(worker),
    )
}

struct JetProcessOutput {
    text: String,
    exceeded: bool,
}

fn jet_process_drain_reader<R>(
    reader: Option<std::io::BufReader<R>>,
    limit: Option<usize>,
    budget: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    limit_hit: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Option<std::thread::JoinHandle<std::io::Result<JetProcessOutput>>>
where
    R: std::io::Read + Send + 'static,
{
    reader.map(|mut reader| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut exceeded = false;
            let mut chunk = [0u8; 8192];
            loop {
                let count = std::io::Read::read(&mut reader, &mut chunk)?;
                if count == 0 {
                    break;
                }
                let kept = match limit {
                    None => {
                        bytes.extend_from_slice(&chunk[..count]);
                        count
                    }
                    Some(limit) => {
                        let budget = budget
                            .as_ref()
                            .expect("bounded process output needs a shared budget");
                        let mut used = budget.load(std::sync::atomic::Ordering::Acquire);
                        loop {
                            let available = limit.saturating_sub(used);
                            let kept = available.min(count);
                            if kept == 0 {
                                break 0;
                            }
                            match budget.compare_exchange(
                                used,
                                used + kept,
                                std::sync::atomic::Ordering::AcqRel,
                                std::sync::atomic::Ordering::Acquire,
                            ) {
                                Ok(_) => {
                                    bytes.extend_from_slice(&chunk[..kept]);
                                    break kept;
                                }
                                Err(next) => used = next,
                            }
                        }
                    }
                };
                if kept < count {
                    exceeded = true;
                    limit_hit.store(true, std::sync::atomic::Ordering::Release);
                    break;
                }
            }
            let text = String::from_utf8(bytes)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
            Ok(JetProcessOutput { text, exceeded })
        })
    })
}
fn jet_process_finish_output_drain(
    drain: Option<std::thread::JoinHandle<std::io::Result<JetProcessOutput>>>,
    stream: &'static str,
) -> Result<JetProcessOutput, jet_std::IOError> {
    let Some(drain) = drain else {
        return Ok(JetProcessOutput {
            text: String::new(),
            exceeded: false,
        });
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
            jet_std::IOError::other(jet_std::IOOperation::Read, Some(stream.to_string()), error)
        })
}
fn jet_process_collect_output(
    drains: (
        Option<std::thread::JoinHandle<std::io::Result<JetProcessOutput>>>,
        Option<std::thread::JoinHandle<std::io::Result<JetProcessOutput>>>,
    ),
) -> Result<(String, String, bool), jet_std::IOError> {
    // Always join both readers. A malformed/closed stdout must not return
    // before the stderr reader is reaped; otherwise its thread can outlive
    // the child wait and retain a pipe/error path indefinitely.
    let output = jet_process_finish_output_drain(drains.0, "process stdout");
    let errors = jet_process_finish_output_drain(drains.1, "process stderr");
    match (output, errors) {
        (Ok(output), Ok(errors)) => {
            Ok((output.text, errors.text, output.exceeded || errors.exceeded))
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

fn jet_process_finish_output_worker(
    worker: Option<std::thread::JoinHandle<std::io::Result<()>>>,
    stream: &'static str,
) -> Result<(), jet_std::IOError> {
    let Some(worker) = worker else {
        return Ok(());
    };
    worker
        .join()
        .map_err(|_| {
            jet_std::IOError::other(
                jet_std::IOOperation::Read,
                Some(stream.to_string()),
                "process output reader panicked",
            )
        })?
        .map_err(|error| {
            jet_std::IOError::other(jet_std::IOOperation::Read, Some(stream.to_string()), error)
        })
}

fn jet_process_collect_child_output(
    child: &jet_std::ProcessChild,
) -> Result<(String, String), jet_std::IOError> {
    jet_process_join_child_output_workers(child)?;
    let output = jet_process_output_state_snapshot(child.stdout_state.as_ref(), "process stdout")?;
    let errors = jet_process_output_state_snapshot(child.stderr_state.as_ref(), "process stderr")?;
    Ok((output, errors))
}

fn jet_process_join_child_output_workers(
    child: &jet_std::ProcessChild,
) -> Result<(), jet_std::IOError> {
    // The worker owns the native pipe. Dropping the public reader releases its
    // local buffering before the worker handles are joined, while the shared
    // state retains every byte for the receipt, including bytes read live.
    *child.stdout.borrow_mut() = None;
    *child.stderr.borrow_mut() = None;
    let stdout_worker = child.stdout_worker.borrow_mut().take();
    let stderr_worker = child.stderr_worker.borrow_mut().take();
    let stdout_result = jet_process_finish_output_worker(stdout_worker, "process stdout");
    let stderr_result = jet_process_finish_output_worker(stderr_worker, "process stderr");
    if let Err(error) = stdout_result {
        return Err(error);
    }
    if let Err(error) = stderr_result {
        return Err(error);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn jet_process_spec_run_windows(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessReceipt, jet_std::IOError> {
    if spec.detached
        || spec.terminal.is_some()
        || spec.stdin.is_some()
        || spec.stdout != jet_std::ProcessStreamMode::Capture
        || spec.stderr != jet_std::ProcessStreamMode::Capture
    {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "authority-bound Windows execution requires non-detached capture streams and closed stdin",
        ));
    }
    let plan = jet_process_spec_plan(spec)?;
    jet_process_verify_launch_plan(spec, &plan)?;
    let cwd = spec
        .cwd
        .as_deref()
        .map(std::path::Path::new)
        .unwrap_or_else(|| std::path::Path::new("."));
    let (source_readable, output_writable, share_network) =
        jet_process_sandbox_policy_scope(spec, &plan.executable_identity)?;
    let output_dir = if output_writable {
        Some(
            jet_process_sandbox::agent_output_dir(cwd)
                .map_err(|error| jet_process_sandbox_error_windows(spec, error))?,
        )
    } else {
        None
    };
    let environment = jet_process_sandbox_env(spec)?;
    let result = jet_process_sandbox::windows_output(
        std::path::Path::new(&plan.executable_identity),
        &spec.cmd[1..],
        cwd,
        output_dir.as_deref(),
        &environment,
        share_network,
        source_readable,
        false,
        spec.timeout_ms,
        spec.output_limit,
    )
    .map_err(|error| jet_process_sandbox_error_windows(spec, error))?;
    if result.mechanism != plan.backend {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "authority-bound Windows launch returned a different isolation backend",
        ));
    }
    let output = String::from_utf8(result.output.stdout).map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("process stdout".to_string()),
            error,
        )
    })?;
    let errors = String::from_utf8(result.output.stderr).map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("process stderr".to_string()),
            error,
        )
    })?;
    Ok(jet_process_receipt(
        spec,
        Some(&plan),
        true,
        result.pid,
        result.output.status.code().unwrap_or(-1) as i64,
        result.output.status.success(),
        Err(JetAbsent),
        result.timed_out,
        output,
        errors,
        None,
    ))
}

#[cfg(target_os = "windows")]
fn jet_process_sandbox_error_windows(
    spec: &jet_std::ProcessSpec,
    error: jet_process_sandbox::WindowsSandboxError,
) -> jet_std::IOError {
    let detail = match error {
        jet_process_sandbox::WindowsSandboxError::Unsupported(detail)
        | jet_process_sandbox::WindowsSandboxError::Io(detail) => detail,
    };
    jet_std::IOError::other(
        jet_std::IOOperation::Resolve,
        spec.cmd.first().cloned(),
        detail,
    )
}

fn jet_process_spec_run_inner(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessReceipt, jet_std::IOError> {
    #[cfg(target_os = "windows")]
    if spec.policy_wire.is_some() {
        return jet_process_spec_run_windows(spec);
    }
    let child = jet_process_spec_spawn(spec)?;
    jet_process_child_wait(&child)
}
fn jet_process_spec_run(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessReceipt, jet_std::IOError> {
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
) -> Result<jet_std::ProcessReceipt, jet_std::IOError> {
    let result = jet_process_spec_run(spec)?;
    if result.success {
        return Ok(result);
    }
    let mut cause = format!("process exited unsuccessfully: code={}", result.code);
    if let Ok(signal) = result.signal {
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn jet_process_sandbox_pipeline_spawn(
    spec: &jet_std::ProcessSpec,
    launch_plan: &jet_std::ProcessPlan,
    input: Option<std::process::ChildStdout>,
) -> Result<std::process::Child, jet_std::IOError> {
    if spec.terminal.is_some() {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "terminal sessions cannot be used as pipeline stages; spawn the session directly",
        ));
    }
    let cwd = spec
        .cwd
        .as_deref()
        .map(std::path::Path::new)
        .unwrap_or_else(|| std::path::Path::new("."));
    let (source_readable, output_writable, share_network) =
        jet_process_sandbox_policy_scope(spec, &launch_plan.executable_identity)?;
    let output_dir = if output_writable {
        Some(
            jet_process_sandbox::agent_output_dir(cwd)
                .map_err(|error| jet_process_sandbox_error(spec, error))?,
        )
    } else {
        None
    };
    let environment = jet_process_sandbox_env(spec)?;
    jet_process_sandbox::spawn(
        std::path::Path::new(&launch_plan.executable_identity),
        &spec.cmd[1..],
        cwd,
        output_dir.as_deref(),
        &environment,
        share_network,
        source_readable,
        false,
        spec.cmd
            .first()
            .map(|word| std::ffi::OsStr::new(word.as_str())),
        |command| {
            if let Some(stdout) = input {
                command.stdin(std::process::Stdio::from(stdout));
            } else {
                command.stdin(match &spec.stdin {
                    Some(mode) => jet_process_stdio(mode),
                    None => std::process::Stdio::null(),
                });
            }
            command.stdout(std::process::Stdio::piped());
            command.stderr(std::process::Stdio::piped());
            jet_process_pty::attach_process_group(command, jet_process_native_limits(spec))
                .map_err(|error| jet_process_sandbox::Error::Io(error.to_string()))?;
            Ok(())
        },
    )
    .map_err(|error| jet_process_sandbox_error(spec, error))
}

fn jet_process_pipeline_cleanup(children: &mut [std::process::Child]) {
    for child in children.iter_mut() {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        let _ = jet_process_pty::signal_group(child.id(), jet_process_signal_kill());
        let _ = child.kill();
    }
    for child in children.iter_mut() {
        let _ = child.wait();
    }
}

fn jet_process_spec_pipeline(
    specs: &Vec<jet_std::ProcessSpec>,
) -> Result<jet_std::ProcessReceipt, jet_std::IOError> {
    if specs.is_empty() {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve,
            None,
            None,
            Some("process.pipeline needs at least one command".to_string()),
        )));
    }
    if specs.iter().any(|spec| spec.policy_wire.is_some()) {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            Some("process.pipeline".to_string()),
            "authority-bound pipelines need one auditable launch transaction; refusing before spawn",
        ));
    }
    let launch_plans = specs
        .iter()
        .map(|spec| {
            if spec.policy_wire.is_some() {
                jet_process_spec_plan(spec).map(Some)
            } else {
                Ok(None)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (spec, launch_plan) in specs.iter().zip(launch_plans.iter()) {
        if let Some(plan) = launch_plan.as_ref() {
            jet_process_verify_launch_plan(spec, plan)?;
        }
    }
    let mut children: Vec<std::process::Child> = Vec::new();
    let mut stage_started = Vec::with_capacity(specs.len());
    let mut prev_stdout: Option<std::process::ChildStdout> = None;
    for (index, (spec, launch_plan)) in specs.iter().zip(launch_plans.iter()).enumerate() {
        let is_last = index + 1 == specs.len();
        let input = prev_stdout.take();
        let child = {
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            if spec.policy_wire.is_some() {
                jet_process_sandbox_pipeline_spawn(
                    spec,
                    launch_plan
                        .as_ref()
                        .expect("authority pipeline stage must have a plan"),
                    input,
                )
            } else {
                let mut command = jet_process_command_with_identity(
                    spec,
                    launch_plan
                        .as_ref()
                        .map(|plan| plan.executable_identity.as_str()),
                )?;
                if let Some(stdout) = input {
                    command.stdin(std::process::Stdio::from(stdout));
                }
                if is_last {
                    command.stdout(jet_process_stdio(&spec.stdout));
                } else {
                    command.stdout(std::process::Stdio::piped());
                }
                command.stderr(jet_process_stdio(&spec.stderr));
                #[cfg(unix)]
                jet_process_pty::attach_process_group(&mut command, jet_process_native_limits(spec)).map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Resolve,
                        spec.cmd.first().cloned(),
                        error,
                    )
                })?;
                command.spawn().map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Resolve,
                        spec.cmd.first().cloned(),
                        error,
                    )
                })
            }
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            {
                let mut command = jet_process_command_with_identity(
                    spec,
                    launch_plan
                        .as_ref()
                        .map(|plan| plan.executable_identity.as_str()),
                )?;
                if let Some(stdout) = input {
                    command.stdin(std::process::Stdio::from(stdout));
                }
                if is_last {
                    command.stdout(jet_process_stdio(&spec.stdout));
                } else {
                    command.stdout(std::process::Stdio::piped());
                }
                command.stderr(jet_process_stdio(&spec.stderr));
                #[cfg(unix)]
                jet_process_pty::attach_process_group(&mut command, jet_process_native_limits(spec)).map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Resolve,
                        spec.cmd.first().cloned(),
                        error,
                    )
                })?;
                command.spawn().map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Resolve,
                        spec.cmd.first().cloned(),
                        error,
                    )
                })
            }
        };
        let mut child = match child {
            Ok(child) => child,
            Err(error) => {
                jet_process_pipeline_cleanup(&mut children);
                return Err(error);
            }
        };
        prev_stdout = child.stdout.take();
        children.push(child);
        stage_started.push(std::time::Instant::now());
    }
    let stage_budgets = specs
        .iter()
        .map(|spec| {
            spec.output_limit
                .map(|_| std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)))
        })
        .collect::<Vec<_>>();
    let pipeline_limit_hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let final_limit = specs
        .last()
        .and_then(|spec| spec.output_limit)
        .map(|limit| limit.max(0) as usize);
    let output_drain = prev_stdout.take().and_then(|stdout| {
        jet_process_drain_reader(
            Some(std::io::BufReader::new(stdout)),
            final_limit,
            stage_budgets.last().cloned().flatten(),
            pipeline_limit_hit.clone(),
        )
    });
    let stderr_drains = children
        .iter_mut()
        .enumerate()
        .map(|(index, child)| {
            jet_process_drain_reader(
                child.stderr.take().map(std::io::BufReader::new),
                specs[index].output_limit.map(|limit| limit.max(0) as usize),
                stage_budgets[index].clone(),
                pipeline_limit_hit.clone(),
            )
        })
        .collect::<Vec<_>>();
    let mut code = 0;
    let mut success = true;
    let mut timed_out = false;
    let mut output_limit_exceeded = false;
    let mut stage_finished = vec![false; children.len()];
    'stages: for index in 0..children.len() {
        if stage_finished[index] {
            continue;
        }
        let status = loop {
            if pipeline_limit_hit.load(std::sync::atomic::Ordering::Acquire) {
                jet_process_pipeline_cleanup(&mut children);
                output_limit_exceeded = true;
                break None;
            }
            let mut current_status = None;
            for stage in 0..children.len() {
                if stage_finished[stage] {
                    continue;
                }
                if let Some(status) = children[stage].try_wait().map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Close,
                        Some("pipeline process".to_string()),
                        error,
                    )
                })? {
                    stage_finished[stage] = true;
                    #[cfg(unix)]
                    let _ = jet_process_pty::signal_group(
                        children[stage].id(),
                        jet_process_signal_kill(),
                    );
                    if !status.success() {
                        success = false;
                        code = status.code().unwrap_or(-1) as i64;
                    }
                    if stage == index {
                        current_status = Some(status);
                    }
                }
            }
            if current_status.is_some() {
                break current_status;
            }
            if specs.iter().enumerate().any(|(stage, spec)| {
                !stage_finished[stage]
                    && spec.timeout_ms.is_some_and(|timeout| {
                        stage_started[stage].elapsed()
                            >= std::time::Duration::from_millis(timeout.max(0) as u64)
                    })
            }) {
                jet_process_pipeline_cleanup(&mut children);
                timed_out = true;
                success = false;
                break None;
            }
            jet_scheduler_park_ms("process wait", 10);
        };
        let Some(_status) = status else {
            break 'stages;
        };
    }
    let output = jet_process_finish_output_drain(output_drain, "pipeline stdout")?;
    let mut output_exceeded = output.exceeded;
    let mut errors = String::new();
    for drain in stderr_drains {
        let result = jet_process_finish_output_drain(drain, "pipeline stderr")?;
        output_exceeded |= result.exceeded;
        errors.push_str(&result.text);
    }
    if output_exceeded || output_limit_exceeded {
        return Err(jet_std::IOError::ResourceLimit(
            jet_std::ProcessResourceLimit::Output,
        ));
    }
    let output_text = output.text;
    let mut secret_values = specs
        .iter()
        .flat_map(jet_process_policy_secret_values)
        .collect::<Vec<_>>();
    secret_values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    secret_values.dedup();
    Ok(jet_process_receipt(
        specs.last().expect("nonempty pipeline"),
        launch_plans.last().and_then(Option::as_ref),
        true,
        0,
        code,
        success,
        Err(JetAbsent),
        timed_out,
        output_text,
        errors,
        Some(&secret_values),
    ))
}

fn jet_process_inner_id(inner: &jet_std::ProcessHandle) -> u32 {
    match inner {
        jet_std::ProcessHandle::Std { child, .. } => child.id(),
        #[cfg(windows)]
        jet_std::ProcessHandle::Native { pid, .. } => *pid,
    }
}

fn jet_process_inner_try_wait(
    inner: &mut jet_std::ProcessHandle,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    match inner {
        jet_std::ProcessHandle::Std { child, .. } => child.try_wait(),
        #[cfg(windows)]
        jet_std::ProcessHandle::Native { process, .. } => {
            use std::os::windows::process::ExitStatusExt;
            jet_process_pty::try_wait(process)
                .map(|code| code.map(std::process::ExitStatus::from_raw))
        }
    }
}

fn jet_process_inner_wait(
    inner: &mut jet_std::ProcessHandle,
) -> std::io::Result<std::process::ExitStatus> {
    match inner {
        jet_std::ProcessHandle::Std { child, .. } => child.wait(),
        #[cfg(windows)]
        jet_std::ProcessHandle::Native { process, .. } => {
            use std::os::windows::process::ExitStatusExt;
            jet_process_pty::wait(process).map(std::process::ExitStatus::from_raw)
        }
    }
}

fn jet_process_inner_kill(inner: &mut jet_std::ProcessHandle) -> std::io::Result<()> {
    match inner {
        jet_std::ProcessHandle::Std { child, job } => {
            #[cfg(windows)]
            if let Some(job) = job {
                return jet_process_pty::terminate(job);
            }
            #[cfg(not(windows))]
            let _ = job;
            child.kill()
        }
        #[cfg(windows)]
        jet_std::ProcessHandle::Native { job, .. } => jet_process_pty::terminate(job),
    }
}

#[cfg(target_os = "linux")]
fn jet_process_linux_vm_size(pid: u32) -> Option<u64> {
    let path = format!("/proc/{pid}/status");
    let status = std::fs::read_to_string(path).ok()?;
    status.lines().find_map(|line| {
        let value = line.strip_prefix("VmSize:")?.trim();
        let kilobytes = value.strip_suffix("kB")?.trim().parse::<u64>().ok()?;
        Some(kilobytes.saturating_mul(1024))
    })
}

#[cfg(target_os = "linux")]
fn jet_process_linux_open_files(pid: u32) -> Option<u64> {
    let path = format!("/proc/{pid}/fd");
    Some(std::fs::read_dir(path).ok()?.count() as u64)
}

/// Linux's rlimits make the ceiling real; this parent-side observation turns
/// the two limits whose native failure is normally `ENOMEM`/`EMFILE` into the
/// same typed process outcome as CPU/output. `/proc` is a Linux contract, so a
/// read race simply leaves the kernel-enforced failure to the child and does
/// not invent a classification.
fn jet_process_live_resource_limit(
    pid: u32,
    spec: &jet_std::ProcessSpec,
) -> Option<jet_std::ProcessResourceLimit> {
    #[cfg(target_os = "linux")]
    {
        if let Some(limit) = spec.memory_limit_bytes {
            if jet_process_linux_vm_size(pid)
                .is_some_and(|used| used >= limit.max(0) as u64)
            {
                return Some(jet_std::ProcessResourceLimit::Memory);
            }
        }
        if let Some(limit) = spec.open_file_limit {
            if jet_process_linux_open_files(pid)
                .is_some_and(|used| used >= limit.max(0) as u64)
            {
                return Some(jet_std::ProcessResourceLimit::OpenFiles);
            }
        }
    }
    let _ = (pid, spec);
    None
}

#[cfg(windows)]
fn jet_process_inner_resource_limit(
    inner: &jet_std::ProcessHandle,
    spec: &jet_std::ProcessSpec,
) -> std::io::Result<Option<jet_std::ProcessResourceLimit>> {
    let job = match inner {
        jet_std::ProcessHandle::Std { job, .. } => job.as_deref(),
        jet_std::ProcessHandle::Native { job, .. } => Some(job.as_ref()),
    };
    let Some(job) = job else {
        return Ok(None);
    };
    jet_process_pty::resource_limit_hit(job, jet_process_native_limits(spec)).map(|hit| {
        hit.map(|hit| match hit {
            jet_process_pty::ResourceLimitKind::CpuTime => {
                jet_std::ProcessResourceLimit::CpuTime
            }
            jet_process_pty::ResourceLimitKind::Memory => jet_std::ProcessResourceLimit::Memory,
        })
    })
}

fn jet_process_status_resource_limit(
    status: &std::process::ExitStatus,
    spec: &jet_std::ProcessSpec,
) -> Option<jet_std::ProcessResourceLimit> {
    #[cfg(unix)]
    if spec.cpu_time_limit_ms.is_some()
        && std::os::unix::process::ExitStatusExt::signal(status) == Some(24)
    {
        // SIGXCPU is the POSIX rlimit CPU exhaustion signal on Linux and macOS.
        return Some(jet_std::ProcessResourceLimit::CpuTime);
    }
    let _ = (status, spec);
    None
}

fn jet_process_tree_signal(
    inner: &mut jet_std::ProcessHandle,
    process_group: bool,
    signal: i32,
) -> std::io::Result<()> {
    #[cfg(unix)]
    if process_group {
        return match jet_process_pty::signal_group(jet_process_inner_id(inner), signal) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
    }
    let _ = (process_group, signal);
    jet_process_inner_kill(inner)
}

fn jet_process_cleanup_io_error(error: std::io::Error) -> jet_std::IOError {
    jet_std::IOError::other(
        jet_std::IOOperation::Close,
        Some("process".to_string()),
        error,
    )
}

fn jet_process_record_cleanup_error(child: &jet_std::ProcessChild, error: jet_std::IOError) {
    if let Ok(mut slot) = child.cleanup_error.try_borrow_mut() {
        if slot.is_none() {
            *slot = Some(error);
        }
    }
}

fn jet_process_child_id(child: &jet_std::ProcessChild) -> i64 {
    child
        .inner
        .borrow()
        .as_ref()
        .map(|inner| jet_process_inner_id(inner) as i64)
        .unwrap_or(0)
}
fn jet_process_child_wait(
    child: &jet_std::ProcessChild,
) -> Result<jet_std::ProcessReceipt, jet_std::IOError> {
    if let Some(error) = child.cleanup_error.borrow().clone() {
        return Err(error);
    }
    if let Some(result) = child.wait_result.borrow().clone() {
        return Ok(result);
    }
    let _cleanup = JetProcessWaitCleanup { child };
    // Every piped stream has a worker from spawn onward. Waiting first can
    // deadlock when either pipe fills; independent workers keep stdout and
    // stderr flowing even when the caller consumes only one live stream.
    let output_limit_hit = child.output_limit_hit.clone();
    let output_read_error = child.output_read_error.clone();
    let mut timed_out = false;
    let mut resource_limit = None;
    let status = loop {
        let mut slot = child.inner.borrow_mut();
        let Some(inner) = slot.as_mut() else {
            return Err(jet_std::IOError::Closed(jet_std::IOContext::new(
                jet_std::IOOperation::Close,
                Some("process".to_string()),
                None,
                Some("process child wait result is unavailable".to_string()),
            )));
        };
        #[cfg(windows)]
        if let Some(limit) = jet_process_inner_resource_limit(inner, &child.audit_spec).map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Close,
                Some("process".to_string()),
                error,
            )
        })? {
            jet_process_tree_signal(inner, child.process_group, jet_process_signal_kill())
                .map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Close,
                        Some("process".to_string()),
                        error,
                    )
                })?;
            resource_limit = Some(limit);
            break jet_process_inner_wait(inner).map_err(|error| {
                jet_std::IOError::other(
                    jet_std::IOOperation::Close,
                    Some("process".to_string()),
                    error,
                )
            })?;
        }
        if let Some(limit) = jet_process_live_resource_limit(jet_process_inner_id(inner), &child.audit_spec) {
            jet_process_tree_signal(inner, child.process_group, jet_process_signal_kill())
                .map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Close,
                        Some("process".to_string()),
                        error,
                    )
                })?;
            resource_limit = Some(limit);
            break jet_process_inner_wait(inner).map_err(|error| {
                jet_std::IOError::other(
                    jet_std::IOOperation::Close,
                    Some("process".to_string()),
                    error,
                )
            })?;
        }
        if let Some(status) = jet_process_inner_try_wait(inner).map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Close,
                Some("process".to_string()),
                error,
            )
        })? {
            break status;
        }
        if output_limit_hit.load(std::sync::atomic::Ordering::Acquire)
            || output_read_error.load(std::sync::atomic::Ordering::Acquire)
        {
            jet_process_tree_signal(inner, child.process_group, jet_process_signal_kill())
                .map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Close,
                        Some("process".to_string()),
                        error,
                    )
                })?;
            break jet_process_inner_wait(inner).map_err(|error| {
                jet_std::IOError::other(
                    jet_std::IOOperation::Close,
                    Some("process".to_string()),
                    error,
                )
            })?;
        }
        if let Some(timeout) = child.timeout_ms {
            if child.started.elapsed() >= std::time::Duration::from_millis(timeout as u64) {
                jet_process_tree_signal(inner, child.process_group, jet_process_signal_kill())
                    .map_err(|error| {
                        jet_std::IOError::other(
                            jet_std::IOOperation::Close,
                            Some("process".to_string()),
                            error,
                        )
                    })?;
                timed_out = true;
                break jet_process_inner_wait(inner).map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Close,
                        Some("process".to_string()),
                        error,
                    )
                })?;
            }
        }
        drop(slot);
        // D-TASKRUNTIME1=A: process waits are scheduler wait points. Parking
        // here keeps the worker available and makes inherited cancellation and
        // deadlines wake the wait exactly like channel, timer, and I/O waits.
        jet_scheduler_park_ms("process wait", 10);
    };
    // Unix process groups need one final sweep after the leader exits because
    // the group has no close-time kill contract. Windows Job Objects already
    // carry `KILL_ON_JOB_CLOSE`; calling TerminateJobObject again after the
    // leader exits races an empty/signaled job and can turn a successful wait
    // into a cleanup error. Dropping the native handle below closes that job
    // and kills any remaining descendants.
    #[cfg(unix)]
    let cleanup_error = if child.process_group {
        if let Some(inner) = child.inner.borrow_mut().as_mut() {
            jet_process_tree_signal(inner, true, jet_process_signal_kill())
                .err()
                .map(jet_process_cleanup_io_error)
        } else {
            None
        }
    } else {
        None
    };
    #[cfg(not(unix))]
    let cleanup_error: Option<jet_std::IOError> = None;
    if let Some(error) = cleanup_error.as_ref() {
        jet_process_record_cleanup_error(child, error.clone());
    }
    let pid = child
        .inner
        .borrow()
        .as_ref()
        .map(|inner| jet_process_inner_id(inner) as i64)
        .unwrap_or(0);
    child.inner.borrow_mut().take();
    *child.stdin.borrow_mut() = None;
    // ConPTY keeps its output pipe live until the pseudoconsole is released.
    // Close it after the child exits, before joining the reader, or `wait()`
    // can block forever on an otherwise finished terminal child.
    #[cfg(windows)]
    if let Some(session) = child.terminal.as_ref().ok() {
        session.control.close();
    }
    let (output, errors) = jet_process_collect_child_output(child)?;
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    if output_limit_hit.load(std::sync::atomic::Ordering::Acquire) {
        return Err(jet_std::IOError::ResourceLimit(
            jet_std::ProcessResourceLimit::Output,
        ));
    }
    if let Some(limit) = resource_limit {
        return Err(jet_std::IOError::ResourceLimit(limit));
    }
    if let Some(limit) = jet_process_status_resource_limit(&status, &child.audit_spec) {
        return Err(jet_std::IOError::ResourceLimit(limit));
    }
    let code = status.code().unwrap_or(-1) as i64;
    #[cfg(unix)]
    let signal =
        jet_outcome_of(std::os::unix::process::ExitStatusExt::signal(&status).map(i64::from));
    #[cfg(not(unix))]
    let signal = Err(JetAbsent);
    let result = jet_process_receipt(
        &child.audit_spec,
        child.audit_plan.as_ref(),
        child.process_group,
        pid,
        code,
        status.success(),
        signal,
        timed_out,
        output,
        errors,
        None,
    );
    *child.wait_result.borrow_mut() = Some(result.clone());
    Ok(result)
}
// #1481 core.process: a non-blocking companion to `wait()` — reports whether
// the child has already exited without draining its output pipes or
// blocking. Peeks the same underlying handle `id`/`kill` already borrow.
fn jet_process_child_exited(child: &jet_std::ProcessChild) -> Result<bool, jet_std::IOError> {
    let mut slot = child.inner.borrow_mut();
    let Some(inner) = slot.as_mut() else {
        return Ok(true);
    };
    jet_process_inner_try_wait(inner)
        .map(|status| status.is_some())
        .map_err(|error| {
            jet_std::IOError::other(
                jet_std::IOOperation::Close,
                Some("process".to_string()),
                error,
            )
        })
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
    let result = {
        #[cfg(unix)]
        {
            jet_process_pty::resize(
                session.master.as_ref(),
                jet_process_pty::PtyConfig {
                    cols: size.cols,
                    rows: size.rows,
                    raw: false,
                },
            )
        }
        #[cfg(windows)]
        {
            jet_process_pty::resize_console(
                session.control.raw(),
                jet_process_pty::PtyConfig {
                    cols: size.cols,
                    rows: size.rows,
                    raw: false,
                },
            )
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "terminal resize is unavailable on this target",
            ))
        }
    };
    result.map_err(|error| {
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
            Self::Shared(state) => jet_process_output_state_read(state, bytes),
        };
        match result {
            Err(error)
                if matches!(self, Self::Terminal(_))
                    && jet_process_pty::is_terminal_eof(&error) =>
            {
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

#[cfg(windows)]
fn jet_process_signal_interrupt() -> i32 {
    2
}

#[cfg(not(any(unix, windows)))]
fn jet_process_signal_interrupt() -> i32 {
    0
}

#[cfg(unix)]
fn jet_process_signal_terminate() -> i32 {
    jet_process_pty::SIGTERM
}

#[cfg(windows)]
fn jet_process_signal_terminate() -> i32 {
    15
}

#[cfg(not(any(unix, windows)))]
fn jet_process_signal_terminate() -> i32 {
    0
}

#[cfg(unix)]
fn jet_process_signal_kill() -> i32 {
    jet_process_pty::SIGKILL
}

#[cfg(windows)]
fn jet_process_signal_kill() -> i32 {
    9
}

#[cfg(not(any(unix, windows)))]
fn jet_process_signal_kill() -> i32 {
    0
}

fn jet_process_child_signal(
    child: &jet_std::ProcessChild,
    signal: i32,
) -> Result<(), jet_std::IOError> {
    if let Some(inner) = child.inner.borrow_mut().as_mut() {
        #[cfg(windows)]
        if signal == jet_process_signal_interrupt() {
            if let jet_std::ProcessHandle::Native { pid, job, .. } = inner {
                let input = child.stdin.borrow();
                let input = input.as_ref().and_then(|input| match input {
                    jet_std::ProcessStdin::Terminal(input) => Some(input),
                    jet_std::ProcessStdin::Pipe(_) => None,
                });
                return jet_process_pty::interrupt(*pid, job, input).map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Close,
                        Some("process".to_string()),
                        error,
                    )
                });
            }
        }
        jet_process_tree_signal(inner, child.terminal.is_ok() || child.process_group, signal)
            .map_err(|error| {
                jet_std::IOError::other(
                    jet_std::IOOperation::Close,
                    Some("process".to_string()),
                    error,
                )
            })?;
    }
    Ok(())
}

fn jet_process_reap_unfinished(child: &jet_std::ProcessChild) -> Result<(), jet_std::IOError> {
    if child.detached
        || child
            .wait_result
            .try_borrow()
            .map_or(false, |result| result.is_some())
    {
        return Ok(());
    }
    let mut slot = child.inner.try_borrow_mut().map_err(|_| {
        jet_std::IOError::other(
            jet_std::IOOperation::Close,
            Some("process".to_string()),
            "process child cleanup is already in progress",
        )
    })?;
    let Some(inner) = slot.as_mut() else {
        drop(slot);
        return jet_process_join_child_output_workers(child);
    };
    let signal_error =
        jet_process_tree_signal(inner, child.process_group, jet_process_signal_kill())
            .err()
            .map(jet_process_cleanup_io_error);
    #[cfg(windows)]
    if let Some(session) = child.terminal.as_ref().ok() {
        session.control.close();
    }
    let wait_error = jet_process_inner_wait(inner)
        .err()
        .map(jet_process_cleanup_io_error);
    drop(slot);
    let output_error = jet_process_join_child_output_workers(child).err();
    if let Some(error) = signal_error {
        Err(error)
    } else if let Some(error) = wait_error {
        Err(error)
    } else if let Some(error) = output_error {
        Err(error)
    } else {
        Ok(())
    }
}

struct JetProcessWaitCleanup<'a> {
    child: &'a jet_std::ProcessChild,
}

impl Drop for JetProcessWaitCleanup<'_> {
    fn drop(&mut self) {
        if let Err(error) = jet_process_reap_unfinished(self.child) {
            jet_process_record_cleanup_error(self.child, error);
        }
    }
}

impl Drop for jet_std::ProcessChild {
    fn drop(&mut self) {
        if std::rc::Rc::strong_count(&self.inner) == 1 {
            if let Err(error) = jet_process_reap_unfinished(self) {
                jet_process_record_cleanup_error(self, error);
            }
        }
    }
}

fn jet_process_stdin_closed() -> jet_std::IOError {
    jet_std::IOError::Closed(jet_std::IOContext::new(
        jet_std::IOOperation::Write,
        Some("process stdin".to_string()),
        None,
        Some("process stdin closed".to_string()),
    ))
}

fn jet_process_stdin_error(error: std::io::Error) -> jet_std::IOError {
    if matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::UnexpectedEof
    ) {
        jet_process_stdin_closed()
    } else {
        jet_std::IOError::other(
            jet_std::IOOperation::Write,
            Some("process stdin".to_string()),
            error,
        )
    }
}

// `child.stdin` is a writer handle (`.write(text)`); `child.stdout`/
// `child.stderr` are streaming reader handles consumed only via
// `loop line in child.stdout.lines() { ... }` (mirrors `FileReader`/`StdinHandle`
// — sema restricts the field access + `.lines()` result to that position, E2502).
#[cfg(unix)]
fn jet_process_stdin_write(
    handle: &std::rc::Rc<std::cell::RefCell<Option<jet_std::ProcessStdin>>>,
    text: &String,
) -> Result<(), jet_std::IOError> {
    use std::io::ErrorKind;
    use std::os::fd::AsRawFd;

    let bytes = text.as_bytes();
    {
        let mut stdin = handle.borrow_mut();
        if let Some(jet_std::ProcessStdin::Terminal(writer)) = stdin.as_mut() {
            // PTY input and output are independent File clones of one master
            // open file description. Setting O_NONBLOCK on the input clone
            // would also make the output drain observe EAGAIN. Keep the PTY
            // master blocking; terminal writes are byte-stream writes, while
            // pipe writes below retain scheduler-aware nonblocking behavior.
            std::io::Write::write_all(writer, bytes).map_err(jet_process_stdin_error)?;
            return Ok(());
        }
    }
    let mut offset = 0;
    while offset < bytes.len() {
        let wait_fd = {
            let mut stdin = handle.borrow_mut();
            let Some(stdin) = stdin.as_mut() else {
                return Err(jet_process_stdin_closed());
            };
            let (fd, nonblocking) = match stdin {
                jet_std::ProcessStdin::Pipe(writer) => (writer.as_raw_fd(), true),
                // PTY input/output clones share one open-file description.
                // Do not set O_NONBLOCK on the input clone: it would also
                // make the terminal output reader return WouldBlock.
                jet_std::ProcessStdin::Terminal(writer) => (writer.as_raw_fd(), false),
            };
            if nonblocking {
                jet_scheduler_raw_io_set_nonblocking(fd).map_err(|error| {
                    jet_std::IOError::other(
                        jet_std::IOOperation::Write,
                        Some("process stdin".to_string()),
                        error,
                    )
                })?;
            }
            match std::io::Write::write(stdin, &bytes[offset..]) {
                Ok(0) => {
                    return Err(jet_process_stdin_closed());
                }
                Ok(written) => {
                    offset += written;
                    None
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) if error.kind() == ErrorKind::WouldBlock => Some(fd),
                Err(error) => {
                    return Err(jet_process_stdin_error(error));
                }
            }
        };
        let Some(fd) = wait_fd else {
            continue;
        };
        let scheduler = jet_scheduler_raw_io_handle(fd);
        match jet_scheduler_wait_without_unwind(|| {
            jet_scheduler_raw_io_write_wait(&scheduler, "process stdin")
        }) {
            JetSchedulerWait::Ready(()) => {}
            JetSchedulerWait::Cancelled => {
                return Err(jet_std::IOError::Cancelled(jet_std::IOContext::new(
                    jet_std::IOOperation::Write,
                    Some("process stdin".to_string()),
                    None,
                    Some("process stdin cancelled".to_string()),
                )));
            }
            JetSchedulerWait::Deadline(_) => {
                return Err(jet_std::IOError::TimedOut(jet_std::IOContext::new(
                    jet_std::IOOperation::Write,
                    Some("process stdin".to_string()),
                    None,
                    Some("deadline exceeded while waiting in process stdin".to_string()),
                )));
            }
            JetSchedulerWait::Panicked(message) => {
                return Err(jet_std::IOError::other(
                    jet_std::IOOperation::Write,
                    Some("process stdin".to_string()),
                    format!("process stdin scheduler wait failed: {message}"),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn jet_process_stdin_write(
    handle: &std::rc::Rc<std::cell::RefCell<Option<jet_std::ProcessStdin>>>,
    text: &String,
) -> Result<(), jet_std::IOError> {
    let mut handle = handle.borrow_mut();
    let Some(stdin) = handle.as_mut() else {
        return Err(jet_process_stdin_closed());
    };
    std::io::Write::write_all(stdin, text.as_bytes()).map_err(jet_process_stdin_error)
}
fn jet_process_child_read_line<R: std::io::Read>(
    reader: &mut Option<std::io::BufReader<R>>,
) -> Result<Option<String>, jet_std::IOError> {
    let Some(reader) = reader.as_mut() else {
        return Ok(None);
    };
    let mut line = String::new();
    let n = std::io::BufRead::read_line(reader, &mut line).map_err(|error| {
        jet_std::IOError::other(
            jet_std::IOOperation::Read,
            Some("process output".to_string()),
            error,
        )
    })?;
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
