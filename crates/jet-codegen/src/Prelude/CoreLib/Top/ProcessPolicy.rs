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
        let equals = right.find('=').unwrap_or(right.len());
        return format!("{}=<redacted>", &right[..equals]);
    }
    right.to_owned()
}

fn jet_process_policy_secret_values(spec: &jet_std::ProcessSpec) -> Vec<String> {
    let mut values = jet_process_policy_rights(spec)
        .into_iter()
        .filter_map(|right| {
            let (name, value) = right.split_once('=')?;
            (name.eq_ignore_ascii_case("secret") && !value.is_empty())
                .then(|| value.to_owned())
        })
        .collect::<Vec<_>>();
    values.extend(
        spec.env_set
            .iter()
            .filter(|(name, value)| {
                !value.is_empty()
                    && ["secret", "token", "password", "passwd", "credential", "key"]
                        .iter()
                        .any(|part| name.to_ascii_lowercase().contains(part))
            })
            .map(|(_, value)| value.clone()),
    );
    values.sort_by_key(|value| std::cmp::Reverse(value.len()));
    values.dedup();
    values
}

fn jet_process_redact_text(text: &str, values: &[String]) -> String {
    values.iter().fold(text.to_owned(), |text, value| {
        if value.is_empty() {
            text
        } else {
            text.replace(value, "<redacted>")
        }
    })
}

fn jet_process_policy_receipt_rights(spec: &jet_std::ProcessSpec) -> Vec<String> {
    let values = jet_process_policy_secret_values(spec);
    jet_process_policy_rights(spec)
        .into_iter()
        .map(|right| jet_process_redact_text(&jet_process_policy_digest_right(&right), &values))
        .collect()
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

fn jet_process_backend_limits() -> Vec<String> {
    #[cfg(target_os = "linux")]
    let limits = vec!["private-tmpfs-bytes=67108864".to_string()];
    #[cfg(target_os = "windows")]
    let mut limits = Vec::new();
    #[cfg(target_os = "windows")]
    limits.extend([
        "job-kill-on-close=true".to_string(),
        "active-processes=256".to_string(),
        "memory-bytes=2147483648".to_string(),
    ]);
    #[cfg(target_os = "macos")]
    let limits = Vec::new();
    #[cfg(all(
        not(target_os = "linux"),
        not(target_os = "macos"),
        not(target_os = "windows")
    ))]
    let limits = Vec::new();
    limits
}

fn jet_process_policy_limits(spec: &jet_std::ProcessSpec) -> Vec<String> {
    let mut limits = jet_process_backend_limits();
    limits.extend([
        format!(
            "timeout-ms={}",
            spec.timeout_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "output-limit={}",
            spec.output_limit
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "cpu-time-limit-ms={}",
            spec.cpu_time_limit_ms
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "memory-limit-bytes={}",
            spec.memory_limit_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "open-file-limit={}",
            spec.open_file_limit
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
    ]);
    limits
}

fn jet_process_policy_outputs(spec: &jet_std::ProcessSpec) -> Vec<String> {
    if spec.detached {
        return vec!["stdout=discarded".to_string(), "stderr=discarded".to_string()];
    }
    vec![
        format!("stdout={}", jet_process_stream_mode_name(&spec.stdout)),
        format!("stderr={}", jet_process_stream_mode_name(&spec.stderr)),
    ]
}

fn jet_process_policy_descendants(spec: &jet_std::ProcessSpec) -> String {
    if spec.detached {
        "detached".to_string()
    } else if spec.policy_wire.is_some() {
        "contained".to_string()
    } else {
        "direct".to_string()
    }
}

fn jet_process_input_digest(spec: &jet_std::ProcessSpec) -> String {
    let mut material = String::from("jet-process-input-v1\n");
    for word in &spec.cmd {
        material.push_str(&word.len().to_string());
        material.push(':');
        material.push_str(word);
        material.push('\n');
    }
    let digest = jet_sha256_raw(material.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

fn jet_process_policy_network(spec: &jet_std::ProcessSpec) -> bool {
    jet_process_policy_rights(spec)
        .iter()
        .any(|right| *right == "Net")
}

fn jet_process_policy_safe_right_name(right: &str) -> &str {
    right
        .split(|character| character == ':' || character == '=')
        .next()
        .unwrap_or("unknown")
}

/// The process boundary has a deliberately small enforcement vocabulary. A
/// grant that is only recorded in the digest but ignored by the backend would
/// turn an expert policy into ambient authority, so planning rejects every
/// right the shared child boundary cannot enforce.
fn jet_process_policy_check(spec: &jet_std::ProcessSpec) -> Result<(), jet_std::IOError> {
    for right in jet_process_policy_rights(spec) {
        let supported = right == "FS.Read:repo"
            || right == "FS.Write:.jet/build"
            || right == "Net"
            || right
                .strip_prefix("Exec:")
                .is_some_and(|executable| !executable.is_empty());
        if supported {
            continue;
        }
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            format!(
                "authority grant `{}` cannot be enforced by the native process boundary; refusing before spawn",
                jet_process_policy_safe_right_name(&right),
            ),
        ));
    }
    // A live stream, inherited descriptor, or terminal session bypasses the
    // receipt redactor.
    // Keep the authority path fail-closed until a streaming audit transport
    // can carry the same secret-redaction contract as captured output.
    if spec.terminal.is_some()
        || (!spec.detached
            && (spec.stdout != jet_std::ProcessStreamMode::Capture
                || spec.stderr != jet_std::ProcessStreamMode::Capture))
    {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "authority-bound process receipts require captured stdout and stderr without a terminal; live terminal, streaming, or inherited output would bypass redaction, refusing before spawn",
        ));
    }
    Ok(())
}

fn jet_process_policy_check_executable(
    spec: &jet_std::ProcessSpec,
    executable_identity: &str,
) -> Result<(), jet_std::IOError> {
    let executable_rights = jet_process_policy_rights(spec)
        .into_iter()
        .filter(|right| right.starts_with("Exec:"))
        .collect::<Vec<_>>();
    if !executable_rights.is_empty()
        && !executable_rights
            .iter()
            .any(|right| right.strip_prefix("Exec:") == Some(executable_identity))
    {
        return Err(jet_std::IOError::other(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            "authority policy does not grant the resolved executable",
        ));
    }
    Ok(())
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
    let cpu_time_limit_ms = spec
        .cpu_time_limit_ms
        .map(|value| value.to_string())
        .unwrap_or_default();
    let memory_limit_bytes = spec
        .memory_limit_bytes
        .map(|value| value.to_string())
        .unwrap_or_default();
    let open_file_limit = spec
        .open_file_limit
        .map(|value| value.to_string())
        .unwrap_or_default();
    let rights = jet_process_policy_rights(spec)
        .iter()
        .map(|right| jet_process_policy_digest_right(right))
        .collect::<Vec<_>>()
        .join("\n");
    let environment_keys = jet_process_policy_environment(spec);
    let backend_limits = jet_process_backend_limits().join("\n");
    let mut material = String::from("jet-process-policy-v1\n");
    let fields = [
        ("authority.rights", rights.as_str()),
        ("network", if jet_process_policy_network(spec) { "allow" } else { "deny" }),
        ("home", "deny"),
        ("secrets", "deny"),
        ("devices", "deny"),
        ("inherited-handles", "deny"),
        // Authority-bound native launches always clear the host environment;
        // the sandbox adapter receives only the explicit env_set/env_remove
        // projection even when the caller did not spell env_clear(). Keep the
        // digest honest about enforced authority. Unbound ProcessSpec retains
        // its ordinary inherited-environment meaning.
        (
            "environment",
            if spec.policy_wire.is_some() || spec.env_clear {
                "explicit-only"
            } else {
                "inherited"
            },
        ),
        ("cwd", cwd.as_str()),
        ("timeout-ms", timeout_ms.as_str()),
        ("output-limit", output_limit.as_str()),
        ("cpu-time-limit-ms", cpu_time_limit_ms.as_str()),
        ("memory-limit-bytes", memory_limit_bytes.as_str()),
        ("open-file-limit", open_file_limit.as_str()),
        ("backend.limits", backend_limits.as_str()),
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

pub(crate) fn jet_process_policy_redact(spec: &jet_std::ProcessSpec, text: &str) -> String {
    jet_process_redact_text(text, &jet_process_policy_secret_values(spec))
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
    #[cfg(target_os = "linux")]
    {
        return jet_process_sandbox::status()
            .available
            .then_some("linux-bwrap");
    }

    #[cfg(target_os = "macos")]
    {
        return jet_process_sandbox::status()
            .available
            .then_some("macos-seatbelt");
    }

    #[cfg(target_os = "windows")]
    {
        return jet_process_sandbox::status()
            .available
            .then_some("windows-appcontainer");
    }

    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows"
    )))]
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
    jet_process_resource_limits_check(spec)?;
    if spec.policy_wire.is_none() {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Resolve,
            spec.cmd.first().cloned(),
            None,
            Some("ProcessSpec.plan() requires an authority policy from .under(...)".to_string()),
        )));
    }
    let executable_identity = jet_process_resolve_executable(spec)?;
    jet_process_policy_check(spec)?;
    jet_process_policy_check_executable(spec, &executable_identity)?;
    let backend = jet_process_isolation_backend().unwrap_or("unavailable");
    let plan = jet_std::ProcessPlan {
        executable_identity,
        argv: spec
            .cmd
            .iter()
            .map(|word| jet_process_policy_redact(spec, word))
            .collect(),
        input_digest: jet_process_input_digest(spec),
        policy_digest: jet_process_policy_digest(spec),
        backend: backend.to_string(),
        authority: jet_process_policy_receipt_rights(spec),
        descendants: jet_process_policy_descendants(spec),
        limits: jet_process_policy_limits(spec),
        outputs: jet_process_policy_outputs(spec),
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
