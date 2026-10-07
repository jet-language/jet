const JET_PROCESS_DEFAULT_OUTPUT_LIMIT_BYTES: usize = 64 * 1024 * 1024;

// Layout transfer only. Core/process/process.jet supplies every field,
// including all absence flags and default values.
#[allow(clippy::too_many_arguments)]
fn jet_process_spec_fields_raw(
    cmd: Vec<String>,
    cwd: Option<String>,
    env_clear: bool,
    env_names: Vec<String>,
    env_values: Vec<String>,
    env_remove: Vec<String>,
    stdin: Option<jet_std::ProcessStreamMode>,
    stdout: jet_std::ProcessStreamMode,
    stderr: jet_std::ProcessStreamMode,
    timeout_ms: Option<i64>,
    output_limit: Option<i64>,
    cpu_time_limit_ms: Option<i64>,
    memory_limit_bytes: Option<i64>,
    open_file_limit: Option<i64>,
    detached: bool,
    terminal: Option<jet_std::TerminalPolicy>,
    policy_wire: Option<String>,
) -> jet_std::ProcessSpec {
    jet_std::ProcessSpec {
        cmd, cwd, env_clear,
        env_set: env_names.into_iter().zip(env_values).collect(),
        env_remove, stdin, stdout, stderr, timeout_ms, output_limit,
        cpu_time_limit_ms, memory_limit_bytes, open_file_limit,
        detached, terminal, policy_wire,
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

fn jet_process_spec_arg(mut spec: jet_std::ProcessSpec, extra: &String) -> jet_std::ProcessSpec {
    spec.cmd.push(extra.clone());
    spec
}

fn jet_process_spec_args_extend(
    mut spec: jet_std::ProcessSpec,
    extra: &Vec<String>,
) -> jet_std::ProcessSpec {
    spec.cmd.extend(extra.iter().cloned());
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
