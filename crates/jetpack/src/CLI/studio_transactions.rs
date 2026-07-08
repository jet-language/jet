fn handle_studio_transaction(
    body: &str,
    context: Option<&StudioContext>,
) -> (&'static str, String) {
    let Some(context) = context else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing Studio project context\"}".to_string(),
        );
    };
    let op = json_string_field(body, "op").unwrap_or_default();
    if op != "set-option" {
        return (
            "400 Bad Request",
            "{\"error\":\"unsupported Studio transaction\"}".to_string(),
        );
    }
    let Some(key) = json_string_field(body, "key") else {
        return ("400 Bad Request", "{\"error\":\"missing key\"}".to_string());
    };
    let Some(value) = json_string_field(body, "value") else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing value\"}".to_string(),
        );
    };
    let write = json_bool_field(body, "write");
    let source = match std::fs::read_to_string(&context.config) {
        Ok(source) => source,
        Err(e) => {
            return (
                "500 Internal Server Error",
                format!(
                    "{{\"error\":{},\"path\":{}}}",
                    JSON::quote(&format!("reading config failed: {e}")),
                    JSON::quote(&context.config.display().to_string())
                ),
            )
        }
    };
    let (next, changed) = match apply_option_transaction(&source, &key, &value) {
        Ok(result) => result,
        Err(e) => {
            return (
                "400 Bad Request",
                format!("{{\"error\":{}}}", JSON::quote(&e)),
            )
        }
    };
    if write && changed {
        if let Err(e) = std::fs::write(&context.config, &next) {
            return (
                "500 Internal Server Error",
                format!(
                    "{{\"error\":{},\"path\":{}}}",
                    JSON::quote(&format!("writing config failed: {e}")),
                    JSON::quote(&context.config.display().to_string())
                ),
            );
        }
    }
    let diff = source_diff(&context.config, &source, &next);
    (
        "200 OK",
        format!(
            "{{\"host\":{},\"path\":{},\"op\":\"set-option\",\"key\":{},\"value\":{},\"write\":{},\"changed\":{},\"diff\":{}}}",
            JSON::quote(&context.host),
            JSON::quote(&context.config.display().to_string()),
            JSON::quote(&key),
            JSON::quote(&value),
            if write { "true" } else { "false" },
            if changed { "true" } else { "false" },
            JSON::quote(&diff)
        ),
    )
}

fn handle_studio_run(body: &str, context: Option<&StudioContext>) -> (&'static str, String) {
    let Some(context) = context else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing Studio project context\"}".to_string(),
        );
    };
    let Some(action) = json_string_field(body, "action") else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing action\"}".to_string(),
        );
    };
    if !["check", "plan", "build", "proof", "generations"].contains(&action.as_str()) {
        return (
            "400 Bad Request",
            "{\"error\":\"unsupported Studio run action\"}".to_string(),
        );
    }
    let Some(jet) = sibling_binary("jet") else {
        return (
            "500 Internal Server Error",
            "{\"error\":\"could not find sibling jet binary\"}".to_string(),
        );
    };
    let cwd = context
        .config
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut cmd = std::process::Command::new(jet);
    cmd.arg("os")
        .arg(&action)
        .arg(&context.host)
        .arg("--no-color");
    if action == "plan" || action == "proof" {
        cmd.arg("--json");
    }
    if action == "build" {
        cmd.arg("--name").arg("zz-studio-candidate");
    }
    let output = match cmd.current_dir(&cwd).output() {
        Ok(output) => output,
        Err(e) => {
            return (
                "500 Internal Server Error",
                format!(
                    "{{\"error\":{}}}",
                    JSON::quote(&format!("running jet failed: {e}"))
                ),
            )
        }
    };
    (
        "200 OK",
        format!(
            "{{\"host\":{},\"action\":{},\"status\":{},\"success\":{},\"stdout\":{},\"stderr\":{}}}",
            JSON::quote(&context.host),
            JSON::quote(&action),
            output.status.code().unwrap_or(1),
            if output.status.success() { "true" } else { "false" },
            JSON::quote(&String::from_utf8_lossy(&output.stdout)),
            JSON::quote(&String::from_utf8_lossy(&output.stderr))
        ),
    )
}

fn sibling_binary(name: &str) -> Option<PathBuf> {
    let current = std::env::current_exe().ok()?;
    let dir = current.parent()?;
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    #[cfg(unix)]
    {
        let debug = dir.parent()?.join(name);
        if debug.is_file() {
            return Some(debug);
        }
    }
    Some(PathBuf::from(name))
}

fn apply_option_transaction(
    source: &str,
    key: &str,
    value: &str,
) -> Result<(String, bool), String> {
    let mut lines = source.lines().map(str::to_string).collect::<Vec<_>>();
    let mut in_options = false;
    let mut insert_at = None;
    for (idx, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("options:") && trimmed.contains('[') {
            in_options = true;
            continue;
        }
        if in_options && trimmed.starts_with(']') {
            insert_at = Some(idx);
            break;
        }
        if in_options && trimmed.starts_with(&format!("{key}:")) {
            let indent_len = line.len() - trimmed.len();
            let indent = line[..indent_len].to_string();
            let comma = if trimmed.trim_end().ends_with(',') {
                ","
            } else {
                ""
            };
            let next = format!("{indent}{key}: {value}{comma}");
            let changed = *line != next;
            *line = next;
            let mut output = lines.join("\n");
            if source.ends_with('\n') {
                output.push('\n');
            }
            return Ok((output, changed));
        }
    }
    let Some(idx) = insert_at else {
        return Err("Studio could not find an options block in config.jet".to_string());
    };
    lines.insert(idx, format!("            {key}: {value},"));
    let mut output = lines.join("\n");
    if source.ends_with('\n') {
        output.push('\n');
    }
    Ok((output, true))
}

fn source_diff(path: &Path, before: &str, after: &str) -> String {
    if before == after {
        return format!("diff -- {}\n(no changes)\n", path.display());
    }
    let mut diff = format!("diff -- {}\n", path.display());
    let before_lines = before.lines().collect::<Vec<_>>();
    let after_lines = after.lines().collect::<Vec<_>>();
    let max = before_lines.len().max(after_lines.len());
    for idx in 0..max {
        let old = before_lines.get(idx).copied();
        let new = after_lines.get(idx).copied();
        if old == new {
            continue;
        }
        if let Some(old) = old {
            diff.push_str(&format!("-{old}\n"));
        }
        if let Some(new) = new {
            diff.push_str(&format!("+{new}\n"));
        }
    }
    diff
}

fn json_string_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let rest = body.split_once(&needle)?.1;
    let rest = rest.split_once(':')?.1.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn json_bool_field(body: &str, key: &str) -> bool {
    let needle = format!("\"{key}\"");
    let Some(rest) = body.split_once(&needle).map(|(_, rest)| rest) else {
        return false;
    };
    let Some(rest) = rest.split_once(':').map(|(_, rest)| rest.trim_start()) else {
        return false;
    };
    rest.starts_with("true")
}
