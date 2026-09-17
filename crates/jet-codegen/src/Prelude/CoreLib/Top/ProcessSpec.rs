const JET_PROCESS_DEFAULT_OUTPUT_LIMIT_BYTES: usize = 64 * 1024 * 1024;

fn jet_std_process_cmd(cmd: &Vec<String>) -> jet_std::ProcessSpec {
    jet_std_process_cmd_owned(cmd.clone())
}

/// Construct a process spec without cloning an already-owned argv.
///
/// The AOT Core call receives a borrowed list, while resident callers have
/// already copied the heap list into an owned `Vec<String>`. Keep both paths
/// on the same defaults but let the resident path move its argv directly.
fn jet_std_process_cmd_owned(cmd: Vec<String>) -> jet_std::ProcessSpec {
    jet_std::ProcessSpec {
        cmd,
        cwd: None,
        env_clear: false,
        env_set: Vec::new(),
        env_remove: Vec::new(),
        stdin: None,
        stdout: jet_std::ProcessStreamMode::Capture,
        stderr: jet_std::ProcessStreamMode::Capture,
        timeout_ms: None,
        output_limit: Some(JET_PROCESS_DEFAULT_OUTPUT_LIMIT_BYTES as i64),
        cpu_time_limit_ms: None,
        memory_limit_bytes: None,
        open_file_limit: None,
        detached: false,
        terminal: None,
        policy_wire: None,
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

fn jet_process_spec_cwd(mut spec: jet_std::ProcessSpec, cwd: &String) -> jet_std::ProcessSpec {
    spec.cwd = Some(cwd.clone());
    spec
}
fn jet_process_spec_env(
    mut spec: jet_std::ProcessSpec,
    name: &String,
    value: &String,
) -> jet_std::ProcessSpec {
    spec.env_set.push((name.clone(), value.clone()));
    spec
}
fn jet_process_spec_env_remove(
    mut spec: jet_std::ProcessSpec,
    name: &String,
) -> jet_std::ProcessSpec {
    spec.env_remove.push(name.clone());
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
