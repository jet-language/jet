// D-PROCESS-SESSION2=D: the stable terminal capability facts have one
// semantic source. Engine adapters only marshal this result into their own
// ProcessSpec/Set representation.
mod jet_process_policy {
    const TERMINAL_FACTS: &[&str] = &["terminal", "resize", "raw"];
    const NO_TERMINAL_FACTS: &[&str] = &[];

    pub fn terminal_facts(pty_supported: bool) -> &'static [&'static str] {
        if pty_supported {
            TERMINAL_FACTS
        } else {
            NO_TERMINAL_FACTS
        }
    }
}

fn jet_process_policy_rights(spec: &jet_std::ProcessSpec) -> Vec<String> {
    let mut rights = spec
        .policy_wire
        .as_deref()
        .unwrap_or_default()
        .lines()
        .filter(|right| !right.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    rights.sort();
    rights.dedup();
    rights
}

/// Secret values are never policy identity. Authority entries may carry a
/// value only in an internal `Secret=...` form; keep the key and redact the
/// value before the shared digest sees it.
fn jet_process_policy_digest_right(right: &str) -> String {
    let lower = right.to_ascii_lowercase();
    if lower.starts_with("secret=") {
        return format!("{}=<redacted>", &right[.."Secret".len().min(right.len())]);
    }
    right.to_owned()
}

fn jet_process_stream_mode_name(mode: &jet_std::ProcessStreamMode) -> &'static str {
    match mode {
        jet_std::ProcessStreamMode::Stream => "stream",
        jet_std::ProcessStreamMode::Inherit => "inherit",
        jet_std::ProcessStreamMode::Capture => "capture",
    }
}

fn jet_process_policy_environment(spec: &jet_std::ProcessSpec) -> String {
    let mut names = spec
        .env_set
        .iter()
        .map(|(name, _)| format!("set:{name}"))
        .chain(spec.env_remove.iter().map(|name| format!("remove:{name}")))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names.join("\n")
}

/// D-AGENT-EXEC1: canonical policy material is deliberately separate from
/// command inputs. The same bytes feed plan and receipt consumers, and secret
/// values never enter the digest material.
fn jet_process_policy_material(spec: &jet_std::ProcessSpec) -> String {
    let cwd = spec
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok().map(|path| path.to_string_lossy().to_string()))
        .unwrap_or_else(|| "<unresolved-cwd>".to_string());
    let timeout_ms = spec
        .timeout_ms
        .map(|value| value.to_string())
        .unwrap_or_default();
    let output_limit = spec
        .output_limit
        .map(|value| value.to_string())
        .unwrap_or_default();
    let rights = jet_process_policy_rights(spec)
        .iter()
        .map(|right| jet_process_policy_digest_right(right))
        .collect::<Vec<_>>()
        .join("\n");
    let environment_keys = jet_process_policy_environment(spec);
    let mut material = String::from("jet-process-policy-v1\n");
    let fields = [
        ("authority.rights", rights.as_str()),
        ("network", "deny"),
        ("home", "deny"),
        ("secrets", "deny"),
        ("devices", "deny"),
        ("inherited-handles", "deny"),
        ("environment", if spec.env_clear { "explicit-only" } else { "inherited" }),
        ("cwd", cwd.as_str()),
        ("timeout-ms", timeout_ms.as_str()),
        ("output-limit", output_limit.as_str()),
        ("environment.keys", environment_keys.as_str()),
        (
            "stdin",
            spec.stdin
                .as_ref()
                .map(jet_process_stream_mode_name)
                .unwrap_or("closed"),
        ),
        ("stdout", jet_process_stream_mode_name(&spec.stdout)),
        ("stderr", jet_process_stream_mode_name(&spec.stderr)),
        ("detached", if spec.detached { "true" } else { "false" }),
        (
            "terminal",
            if spec.terminal.is_some() { "requested" } else { "none" },
        ),
    ];
    for (key, value) in fields {
        material.push_str(key);
        material.push('=');
        material.push_str(&value.len().to_string());
        material.push(':');
        material.push_str(value);
        material.push('\n');
    }
    material
}

/// D-AGENT-EXEC1: the shared policy digest contract. Receipt implementation
/// (#1179) must consume this function rather than reconstructing policy bytes.
pub(crate) fn jet_process_policy_digest(spec: &jet_std::ProcessSpec) -> String {
    let digest = jet_sha256_raw(jet_process_policy_material(spec).as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn jet_process_resolve_executable(
    spec: &jet_std::ProcessSpec,
) -> Result<String, jet_std::IOError> {
    let Some(command) = spec.cmd.first() else {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve,
            None,
            None,
            Some("process command needs at least one word".to_string()),
        )));
    };
    let candidate = std::path::Path::new(command);
    let has_path = candidate.is_absolute() || command.contains('/') || command.contains('\\');
    if has_path {
        return std::fs::canonicalize(candidate)
            .map(|path| path.to_string_lossy().to_string())
            .map_err(|error| {
                jet_std::IOError::other(jet_std::IOOperation::Resolve, Some(command.clone()), error)
            });
    }
    let path_value = jet_std_env_snapshot_raw()
        .into_iter()
        .find(|(name, _)| name == std::ffi::OsStr::new("PATH"))
        .map(|(_, value)| value);
    let Some(path_value) = path_value else {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            Some(command.clone()),
            "cannot resolve executable without a PATH snapshot",
        ));
    };
    for directory in std::env::split_paths(&path_value) {
        let path = directory.join(command);
        if path.is_file() {
            return std::fs::canonicalize(&path)
                .map(|path| path.to_string_lossy().to_string())
                .map_err(|error| {
                    jet_std::IOError::other(jet_std::IOOperation::Resolve, Some(command.clone()), error)
                });
        }
    }
    Err(jet_std::IOError::other(
        jet_std::IOOperation::Resolve,
        Some(command.clone()),
        "executable was not found in the captured PATH",
    ))
}

/// D-AGENT-EXEC1/#398: authority-bound process execution enters the native
/// child boundary already used by hermetic build actions. The consumer only
/// selects the backend; profile construction and launch stay in the shared
/// ProcessSandbox Prelude fragment.
fn jet_process_isolation_backend() -> Option<&'static str> {
    #[cfg(target_os = "macos")]
    {
        return jet_process_sandbox::status()
            .available
            .then_some("macos-seatbelt");
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn jet_process_spec_backend_check(
    spec: &jet_std::ProcessSpec,
) -> Result<(), jet_std::IOError> {
    if spec.policy_wire.is_none() {
        return Ok(());
    }
    if jet_process_isolation_backend().is_some() {
        return Ok(());
    }
    Err(jet_std::IOError::other(
        jet_std::IOOperation::Resolve,
        spec.cmd.first().cloned(),
        "authority-bound process execution requires the #398/#893 isolation backend; refusing before spawn",
    ))
}

fn jet_process_spec_plan(
    spec: &jet_std::ProcessSpec,
) -> Result<jet_std::ProcessPlan, jet_std::IOError> {
    if spec.policy_wire.is_none() {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            None,
            Some("ProcessSpec.plan() requires an authority policy from .under(...)".to_string()),
        )));
    }
    let executable_identity = jet_process_resolve_executable(spec)?;
    let backend = jet_process_isolation_backend().unwrap_or("unavailable");
    let plan = jet_std::ProcessPlan {
        executable_identity,
        argv: spec.cmd.clone(),
        policy_digest: jet_process_policy_digest(spec),
        backend: backend.to_string(),
    };
    if backend == "unavailable" {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            format!(
                "authority-bound process plan refused before spawn: isolation backend unavailable (policy digest {})",
                plan.policy_digest
            ),
        ));
    }
    Ok(plan)
}
