struct StudioChangeSet {
    base_source: String,
    next_source: String,
    changes: Vec<StudioChange>,
}

struct StudioChange {
    op: String,
    key: String,
    value: String,
}

struct StudioAppliedChange {
    before_source: String,
    after_source: String,
}

struct StudioCommandResult {
    action: String,
    status: i32,
    success: bool,
    stdout: String,
    stderr: String,
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
    if op == "stage-rollback" {
        return studio_changeset_stage_rollback(context);
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
    if json_bool_field(body, "write") {
        return (
            "400 Bad Request",
            "{\"error\":\"direct Studio writes are disabled\",\"fix\":\"stage the edit, review the exact diff, then Apply the Changeset\"}".to_string(),
        );
    }
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
            op: "set-option".to_string(),
            key: key.clone(),
            value: value.clone(),
        });
    }
    let response = studio_changeset_response(
        context,
        if changeset.is_some() { "staged" } else { "empty" },
        changed,
        false,
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
                changeset.is_some(),
                false,
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
                studio_changeset_response(context, "discarded", changed, false, None),
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
                "{{\"error\":{},\"reprojected\":false}}",
                JSON::quote(&format!("applying Changeset failed: {e}"))
            ),
        );
    }
    if let Err(error) = rebuild_studio_projection(context) {
        let restore = atomic_write_studio_source(&context.config, &staged.base_source);
        let _ = rebuild_studio_projection(context);
        return (
            "422 Unprocessable Entity",
            format!(
                "{{\"error\":{},\"restored\":{},\"reprojected\":false}}",
                JSON::quote(&error),
                if restore.is_ok() { "true" } else { "false" }
            ),
        );
    }
    if let Ok(mut applied) = context.last_applied.lock() {
        *applied = Some(StudioAppliedChange {
            before_source: staged.base_source.clone(),
            after_source: staged.next_source.clone(),
        });
    }
    if let Ok(mut proved) = context.proved_source.lock() {
        *proved = None;
    }
    let response = studio_changeset_response(context, "applied", true, true, changeset.as_ref());
    *changeset = None;
    ("200 OK", response)
}

fn studio_changeset_stage_rollback(context: &StudioContext) -> (&'static str, String) {
    let applied = match context.last_applied.lock() {
        Ok(applied) => applied,
        Err(_) => {
            return (
                "500 Internal Server Error",
                "{\"error\":\"Studio rollback state is unavailable\"}".to_string(),
            )
        }
    };
    let Some(applied) = applied.as_ref() else {
        return (
            "409 Conflict",
            "{\"error\":\"no applied source Changeset is available to roll back\"}".to_string(),
        );
    };
    let current = match std::fs::read_to_string(&context.config) {
        Ok(source) => source,
        Err(e) => {
            return (
                "500 Internal Server Error",
                format!("{{\"error\":{}}}", JSON::quote(&format!("reading config failed: {e}"))),
            )
        }
    };
    if current != applied.after_source {
        return (
            "409 Conflict",
            "{\"error\":\"config.jet changed after the last applied Changeset\",\"fix\":\"review current source before staging a rollback\"}".to_string(),
        );
    }
    let mut changeset = match context.changeset.lock() {
        Ok(changeset) => changeset,
        Err(_) => {
            return (
                "500 Internal Server Error",
                "{\"error\":\"Studio changeset state is unavailable\"}".to_string(),
            )
        }
    };
    if changeset.is_some() {
        return (
            "409 Conflict",
            "{\"error\":\"discard or apply the current Changeset before staging rollback\"}".to_string(),
        );
    }
    *changeset = Some(StudioChangeSet {
        base_source: current,
        next_source: applied.before_source.clone(),
        changes: vec![StudioChange {
            op: "restore-source".to_string(),
            key: "config.jet".to_string(),
            value: "previous applied source".to_string(),
        }],
    });
    (
        "200 OK",
        studio_changeset_response(context, "staged", true, false, changeset.as_ref()),
    )
}

fn studio_changeset_response(
    context: &StudioContext,
    state: &str,
    changed: bool,
    reprojected: bool,
    changeset: Option<&StudioChangeSet>,
) -> String {
    let (count, diff, source, edits) = match changeset {
        Some(staged) => {
            let edits = staged
                .changes
                .iter()
                .map(|change| {
                    format!(
                        "{{\"op\":{},\"key\":{},\"value\":{}}}",
                        JSON::quote(&change.op),
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
        "{{\"host\":{},\"path\":{},\"state\":{},\"write\":false,\"changed\":{},\"staged_count\":{},\"reprojected\":{},\"diff\":{},\"source\":{},\"edits\":[{}]}}",
        JSON::quote(&context.host),
        JSON::quote(&context.config.display().to_string()),
        JSON::quote(state),
        if changed { "true" } else { "false" },
        if state == "applied" { 0 } else { count },
        if reprojected { "true" } else { "false" },
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
    if !["check", "plan", "build", "proof", "switch", "generations"]
        .contains(&action.as_str())
    {
        return (
            "400 Bad Request",
            "{\"error\":\"unsupported Studio run action\"}".to_string(),
        );
    }
    if ["build", "proof", "switch"].contains(&action.as_str())
        && context
            .changeset
            .lock()
            .map(|changeset| changeset.is_some())
            .unwrap_or(true)
    {
        return (
            "409 Conflict",
            "{\"error\":\"apply or discard the staged Changeset before build, proof, or switch\"}".to_string(),
        );
    }
    if action == "switch" {
        let source = std::fs::read_to_string(&context.config).unwrap_or_default();
        let proved = context
            .proved_source
            .lock()
            .map(|proved| proved.as_deref() == Some(source.as_str()))
            .unwrap_or(false);
        if !proved {
            return (
                "409 Conflict",
                "{\"error\":\"current config.jet has no matching successful proof\",\"fix\":\"build and prove this exact source before switching\"}".to_string(),
            );
        }
    }
    let result = match run_studio_command(context, &action) {
        Ok(result) => result,
        Err(error) => {
            return (
                "500 Internal Server Error",
                format!("{{\"error\":{}}}", JSON::quote(&error)),
            )
        }
    };
    if action == "proof" && result.success {
        if let (Ok(source), Ok(mut proved)) = (
            std::fs::read_to_string(&context.config),
            context.proved_source.lock(),
        ) {
            *proved = Some(source);
        }
    }
    ("200 OK", studio_command_json(context, &result))
}

fn run_studio_command(
    context: &StudioContext,
    action: &str,
) -> Result<StudioCommandResult, String> {
    let Some(jet) = sibling_binary("jet") else {
        return Err("could not find sibling jet binary".to_string());
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
    if action == "build" || action == "switch" {
        cmd.arg("--name").arg("zz-studio-candidate");
    }
    if action == "switch" {
        cmd.arg("--yes");
    }
    let output = cmd
        .current_dir(&cwd)
        .output()
        .map_err(|e| format!("running jet failed: {e}"))?;
    Ok(StudioCommandResult {
        action: action.to_string(),
        status: output.status.code().unwrap_or(1),
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn studio_command_json(context: &StudioContext, result: &StudioCommandResult) -> String {
    format!(
        "{{\"host\":{},\"action\":{},\"status\":{},\"success\":{},\"stdout\":{},\"stderr\":{}}}",
        JSON::quote(&context.host),
        JSON::quote(&result.action),
        result.status,
        if result.success { "true" } else { "false" },
        JSON::quote(&result.stdout),
        JSON::quote(&result.stderr)
    )
}

fn studio_live_projection(context: &StudioContext, generation_data: &Path) -> Result<String, String> {
    if !context.config.is_file() {
        return std::fs::read_to_string(generation_data)
            .map_err(|e| format!("reading installed Studio projection failed: {e}"));
    }
    if let Ok(projection) = context.live_projection.lock() {
        if let Some(projection) = projection.as_ref() {
            return Ok(projection.clone());
        }
    }
    rebuild_studio_projection_from(context, generation_data)
}

fn rebuild_studio_projection(context: &StudioContext) -> Result<String, String> {
    let root = std::env::var_os("JETOS_STUDIO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/current-system"));
    rebuild_studio_projection_from(context, &root.join("studio/data.json"))
}

fn rebuild_studio_projection_from(
    context: &StudioContext,
    generation_data: &Path,
) -> Result<String, String> {
    let check = run_studio_command(context, "check")?;
    if !check.success {
        return Err(format!("jet os check failed: {}", check.stderr.trim()));
    }
    let plan = run_studio_command(context, "plan")?;
    if !plan.success {
        return Err(format!("jet os plan failed: {}", plan.stderr.trim()));
    }
    let generation = std::fs::read_to_string(generation_data)
        .unwrap_or_else(|_| "null".to_string());
    let projection = format!(
        "{{\"kind\":\"jetos-studio-projection\",\"source_truth\":\"live-checked-plan\",\"host\":{},\"page_registry\":[{}],\"system_plan\":{},\"generation_projection\":{}}}",
        JSON::quote(&context.host),
        super::JetOS::studio_pages_json(),
        plan.stdout.trim(),
        generation.trim(),
    );
    let mut live = context
        .live_projection
        .lock()
        .map_err(|_| "Studio live projection state is unavailable".to_string())?;
    *live = Some(projection.clone());
    Ok(projection)
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
    let mut diff = format!(
        "diff --git a/{0} b/{0}\n--- a/{0}\n+++ b/{0}\n",
        path.display()
    );
    let before_lines = before.lines().collect::<Vec<_>>();
    let after_lines = after.lines().collect::<Vec<_>>();
    let width = after_lines.len() + 1;
    let mut lcs = vec![0_usize; (before_lines.len() + 1) * width];
    for old in (0..before_lines.len()).rev() {
        for new in (0..after_lines.len()).rev() {
            lcs[old * width + new] = if before_lines[old] == after_lines[new] {
                1 + lcs[(old + 1) * width + new + 1]
            } else {
                lcs[(old + 1) * width + new].max(lcs[old * width + new + 1])
            };
        }
    }
    diff.push_str(&format!(
        "@@ -1,{} +1,{} @@\n",
        before_lines.len(),
        after_lines.len()
    ));
    let (mut old, mut new) = (0, 0);
    while old < before_lines.len() || new < after_lines.len() {
        if old < before_lines.len()
            && new < after_lines.len()
            && before_lines[old] == after_lines[new]
        {
            diff.push_str(&format!(" {}\n", before_lines[old]));
            old += 1;
            new += 1;
        } else if new < after_lines.len()
            && (old == before_lines.len()
                || lcs[old * width + new + 1] >= lcs[(old + 1) * width + new])
        {
            diff.push_str(&format!("+{}\n", after_lines[new]));
            new += 1;
        } else {
            diff.push_str(&format!("-{}\n", before_lines[old]));
            old += 1;
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
