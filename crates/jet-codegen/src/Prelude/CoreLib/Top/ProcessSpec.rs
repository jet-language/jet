const JET_PROCESS_DEFAULT_OUTPUT_LIMIT_BYTES: usize = 64 * 1024 * 1024;

// Layout transfer only. Core/process/process.jet supplies every field,
// including all absence flags and default values.
#[allow(clippy::too_many_arguments)]
fn jet_process_spec_fields_raw(
    cmd: Vec<String>,
    has_cwd: bool, cwd: String,
    env_clear: bool,
    env_names: Vec<String>,
    env_values: Vec<String>,
    env_remove: Vec<String>,
    has_stdin: bool, stdin: jet_std::ProcessStreamMode,
    stdout: jet_std::ProcessStreamMode,
    stderr: jet_std::ProcessStreamMode,
    has_timeout: bool, timeout_ms: i64,
    has_output_limit: bool, output_limit: i64,
    has_cpu_limit: bool, cpu_time_limit_ms: i64,
    has_memory_limit: bool, memory_limit_bytes: i64,
    has_open_limit: bool, open_file_limit: i64,
    detached: bool,
    has_terminal: bool, terminal: jet_std::TerminalPolicy,
    has_policy: bool, policy_wire: String,
) -> jet_std::ProcessSpec {
    jet_std::ProcessSpec {
        cmd, cwd: has_cwd.then_some(cwd), env_clear,
        inherited_directories: Vec::new(),
        env_set: env_names.into_iter().zip(env_values).collect(),
        env_remove, stdin: has_stdin.then_some(stdin), stdout, stderr,
        timeout_ms: has_timeout.then_some(timeout_ms),
        output_limit: has_output_limit.then_some(output_limit),
        cpu_time_limit_ms: has_cpu_limit.then_some(cpu_time_limit_ms),
        memory_limit_bytes: has_memory_limit.then_some(memory_limit_bytes),
        open_file_limit: has_open_limit.then_some(open_file_limit),
        detached, terminal: has_terminal.then_some(terminal),
        policy_wire: has_policy.then_some(policy_wire),
    }
}

// Compiler-private carrier attachment. Core keeps the Directory borrowed while
// this leaf duplicates its lent descriptor; there is no pathname lookup.
fn jet_process_spec_with_directory_raw(
    mut spec: jet_std::ProcessSpec, source_fd: i64, child_fd: i64, as_cwd: bool,
) -> (jet_std::ProcessSpec, bool, jet_foundation::Numeric::JetInt, jet_foundation::Numeric::JetInt) {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::fd::AsRawFd;
        match jet_std::jet_fs_process_fd_duplicate(source_fd as i32, child_fd as i32) {
            Ok(file) => {
                let assigned_fd = file.as_raw_fd();
                spec.inherited_directories.push(jet_std::JetProcessDirectory {
                    file: std::sync::Arc::new(file), child_fd: child_fd as i32, as_cwd,
                });
                (spec, true, jet_foundation::Numeric::JetInt::from_i64(assigned_fd as i64),
                    jet_foundation::Numeric::JetInt::from_i64(0))
            }
            Err(error) => (spec, true, jet_foundation::Numeric::JetInt::from_i64(-1),
                jet_foundation::Numeric::JetInt::from_i64(error.raw_os_error().unwrap_or(0) as i64)),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (source_fd, child_fd, as_cwd);
        (spec, false, jet_foundation::Numeric::JetInt::from_i64(-1), jet_foundation::Numeric::JetInt::from_i64(0))
    }
}

/// D-AGENT-EXEC1: attach the one ordinary authority carrier to the existing
/// ProcessSpec. Binding the policy also closes ambient environment inheritance;
/// later builders may add explicit values, but cannot restore the host snapshot.
/// The launch directory is deliberately not inferred here: authority-bound
/// execution requires the caller to provide an explicit cwd/PathAuthority.
fn jet_process_spec_under_wire(
    mut spec: jet_std::ProcessSpec,
    authority_facts: &str,
) -> jet_std::ProcessSpec {
    spec.policy_wire = Some(authority_facts.to_owned());
    spec.env_clear = true;
    spec
}

fn jet_process_spec_arg(mut spec: jet_std::ProcessSpec, extra: &str) -> jet_std::ProcessSpec {
    spec.cmd.push(extra.to_owned());
    spec
}

fn jet_process_spec_args_extend(
    mut spec: jet_std::ProcessSpec,
    extra: &Vec<String>,
) -> jet_std::ProcessSpec {
    spec.cmd.extend(extra.iter().cloned());
    spec
}

fn jet_process_spec_cwd(mut spec: jet_std::ProcessSpec, cwd: &str) -> jet_std::ProcessSpec {
    spec.cwd = Some(cwd.to_owned());
    spec
}
fn jet_process_spec_env(
    mut spec: jet_std::ProcessSpec,
    name: &str,
    value: &str,
) -> jet_std::ProcessSpec {
    spec.env_set.push((name.to_owned(), value.to_owned()));
    spec
}
fn jet_process_spec_env_remove(
    mut spec: jet_std::ProcessSpec,
    name: &str,
) -> jet_std::ProcessSpec {
    spec.env_remove.push(name.to_owned());
    spec
}
fn jet_process_spec_env_clear(mut spec: jet_std::ProcessSpec) -> jet_std::ProcessSpec {
    spec.env_clear = true;
    spec
}
fn jet_process_spec_stdin(
    mut spec: jet_std::ProcessSpec,
    mode: &jet_std::ProcessStreamMode,
) -> jet_std::ProcessSpec {
    spec.stdin = Some(mode.clone());
    spec
}
fn jet_process_spec_stdout(
    mut spec: jet_std::ProcessSpec,
    mode: &jet_std::ProcessStreamMode,
) -> jet_std::ProcessSpec {
    spec.stdout = mode.clone();
    spec
}
fn jet_process_spec_stderr(
    mut spec: jet_std::ProcessSpec,
    mode: &jet_std::ProcessStreamMode,
) -> jet_std::ProcessSpec {
    spec.stderr = mode.clone();
    spec
}
