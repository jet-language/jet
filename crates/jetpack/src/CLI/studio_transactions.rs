struct StudioChangeSet {
    base_source: String,
    next_source: String,
    changes: Vec<StudioChange>,
}

struct StudioChange {
    key: String,
    value: String,
}

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
    if op == "status" {
        return studio_changeset_status(context);
    }
    if op == "discard" {
        return studio_changeset_discard(context);
    }
    if op == "apply" {
        return studio_changeset_apply(context);
    }
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
    let mut changeset = match context.changeset.lock() {
        Ok(changeset) => changeset,
        Err(_) => {
            return (
                "500 Internal Server Error",
                "{\"error\":\"Studio changeset state is unavailable\"}".to_string(),
            )
        }
    };
    if let Some(staged) = changeset.as_ref() {
        if staged.base_source != source {
            return (
                "409 Conflict",
                "{\"error\":\"config.jet changed after this Changeset was staged\",\"fix\":\"discard or review the Changeset against current source\"}".to_string(),
            );
        }
    }
    let transaction_source = changeset
        .as_ref()
        .map(|staged| staged.next_source.as_str())
        .unwrap_or(&source);
    let (next, changed) = match apply_option_transaction(transaction_source, &key, &value) {
        Ok(result) => result,
        Err(e) => {
            return (
                "400 Bad Request",
                format!("{{\"error\":{}}}", JSON::quote(&e)),
            )
        }
    };
    if changed {
        let staged = changeset.get_or_insert_with(|| StudioChangeSet {
            base_source: source.clone(),
            next_source: source.clone(),
            changes: Vec::new(),
        });
        staged.next_source = next;
        staged.changes.push(StudioChange {
            key: key.clone(),
            value: value.clone(),
        });
    }
    if write {
        let Some(staged) = changeset.as_ref() else {
            return (
                "200 OK",
                studio_changeset_response(context, "empty", true, false, None),
            );
        };
        if let Err(e) = atomic_write_studio_source(&context.config, &staged.next_source) {
            return (
                "500 Internal Server Error",
                format!(
                    "{{\"error\":{},\"path\":{}}}",
                    JSON::quote(&format!("applying Changeset failed: {e}")),
                    JSON::quote(&context.config.display().to_string())
                ),
            );
        }
        let response =
            studio_changeset_response(context, "applied", true, true, changeset.as_ref());
        *changeset = None;
        return ("200 OK", response);
    }
    let response = studio_changeset_response(
        context,
        if changeset.is_some() { "staged" } else { "empty" },
        false,
        changed,
        changeset.as_ref(),
    );
    ("200 OK", response)
}

fn studio_changeset_status(context: &StudioContext) -> (&'static str, String) {
    match context.changeset.lock() {
        Ok(changeset) => (
            "200 OK",
            studio_changeset_response(
                context,
                if changeset.is_some() { "staged" } else { "empty" },
                false,
                changeset.is_some(),
                changeset.as_ref(),
            ),
        ),
        Err(_) => (
            "500 Internal Server Error",
            "{\"error\":\"Studio changeset state is unavailable\"}".to_string(),
        ),
    }
}

fn studio_changeset_discard(context: &StudioContext) -> (&'static str, String) {
    match context.changeset.lock() {
        Ok(mut changeset) => {
            let changed = changeset.is_some();
            *changeset = None;
            (
                "200 OK",
                studio_changeset_response(context, "discarded", false, changed, None),
            )
        }
        Err(_) => (
            "500 Internal Server Error",
            "{\"error\":\"Studio changeset state is unavailable\"}".to_string(),
        ),
    }
}

fn studio_changeset_apply(context: &StudioContext) -> (&'static str, String) {
    let mut changeset = match context.changeset.lock() {
        Ok(changeset) => changeset,
        Err(_) => {
            return (
                "500 Internal Server Error",
                "{\"error\":\"Studio changeset state is unavailable\"}".to_string(),
            )
        }
    };
    let Some(staged) = changeset.as_ref() else {
        return (
            "409 Conflict",
            "{\"error\":\"no staged Changeset to apply\"}".to_string(),
        );
    };
    let current = match std::fs::read_to_string(&context.config) {
        Ok(current) => current,
        Err(e) => {
            return (
                "500 Internal Server Error",
                format!(
                    "{{\"error\":{}}}",
                    JSON::quote(&format!("reading config failed: {e}"))
                ),
            )
        }
    };
    if current != staged.base_source {
        return (
            "409 Conflict",
            "{\"error\":\"config.jet changed after this Changeset was staged\",\"fix\":\"discard or review the Changeset against current source\"}".to_string(),
        );
    }
    if let Err(e) = atomic_write_studio_source(&context.config, &staged.next_source) {
        return (
            "500 Internal Server Error",
            format!(
                "{{\"error\":{}}}",
                JSON::quote(&format!("applying Changeset failed: {e}"))
            ),
        );
    }
    let response =
        studio_changeset_response(context, "applied", true, true, changeset.as_ref());
    *changeset = None;
    ("200 OK", response)
}

fn studio_changeset_response(
    context: &StudioContext,
    state: &str,
    write: bool,
    changed: bool,
    changeset: Option<&StudioChangeSet>,
) -> String {
    let (count, diff, source, edits) = match changeset {
        Some(staged) => {
            let edits = staged
                .changes
                .iter()
                .map(|change| {
                    format!(
                        "{{\"op\":\"set-option\",\"key\":{},\"value\":{}}}",
                        JSON::quote(&change.key),
                        JSON::quote(&change.value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            (
                staged.changes.len(),
                source_diff(&context.config, &staged.base_source, &staged.next_source),
                staged.next_source.clone(),
                edits,
            )
        }
        None => (0, String::new(), String::new(), String::new()),
    };
    format!(
        "{{\"host\":{},\"path\":{},\"state\":{},\"write\":{},\"changed\":{},\"staged_count\":{},\"reprojected\":{},\"diff\":{},\"source\":{},\"edits\":[{}]}}",
        JSON::quote(&context.host),
        JSON::quote(&context.config.display().to_string()),
        JSON::quote(state),
        if write { "true" } else { "false" },
        if changed { "true" } else { "false" },
        if state == "applied" { 0 } else { count },
        if state == "applied" { "true" } else { "false" },
        JSON::quote(&diff),
        JSON::quote(&source),
        edits,
    )
}

fn atomic_write_studio_source(path: &Path, source: &str) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.jet");
    let temp = parent.join(format!(".{name}.studio-{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(source.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
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
    if context.offline {
        cmd.arg("--offline");
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
