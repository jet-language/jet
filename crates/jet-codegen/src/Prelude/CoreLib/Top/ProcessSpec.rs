fn jet_std_process_cmd(cmd: &Vec<String>) -> jet_std::ProcessSpec {
    jet_std::ProcessSpec {
        cmd: cmd.clone(),
        cwd: None,
        env_clear: false,
        env_set: Vec::new(),
        env_remove: Vec::new(),
        stdin: None,
        stdout: jet_std::ProcessStreamMode::Capture,
        stderr: jet_std::ProcessStreamMode::Capture,
        timeout_ms: None,
        output_limit: None,
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
fn jet_process_spec_under_wire(
    mut spec: jet_std::ProcessSpec,
    authority_wire: &String,
) -> jet_std::ProcessSpec {
    spec.policy_wire = Some(authority_wire.clone());
    spec.env_clear = true;
    // Capture the launch directory at authority binding. A later cwd change
    // must not alter the policy digest or make plan and launch disagree.
    if spec.cwd.is_none() {
        spec.cwd = std::env::current_dir()
            .ok()
            .map(|path| path.to_string_lossy().to_string());
    }
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
