use super::studio_server::StudioContext;
use crate::JSON;
use std::path::{Path, PathBuf};

pub(super) struct StudioChangeSet {
    session_id: String,
    token: String,
    base_revision: String,
    base_source: String,
    next_source: String,
    changes: Vec<StudioChange>,
}

struct StudioChange {
    op: String,
    key: String,
    value: String,
}

pub(super) struct StudioAppliedChange {
    before_source: String,
    after_source: String,
}

#[derive(Clone)]
pub(super) struct StudioProvedSource {
    source: String,
    revision: String,
    plan_revision: String,
    artifact_plan_revision: String,
    generation: String,
    generation_path: PathBuf,
}

struct StudioCommandResult {
    action: String,
    status: i32,
    success: bool,
    stdout: String,
    stderr: String,
    source_revision: Option<String>,
    source_changed_after: bool,
}

struct StudioSourceSnapshot {
    file: std::fs::File,
    path: PathBuf,
    cleanup_dir: Option<PathBuf>,
    source_revision: String,
    identity: StudioSnapshotIdentity,
}

struct StudioSourceFileLock {
    _file: std::fs::File,
}

struct StudioSourceWriteReceipt {
    identity: StudioSnapshotIdentity,
    revision: String,
}

struct StudioProofArtifact {
    generation: String,
    path: PathBuf,
    plan_revision: String,
}

#[cfg(unix)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct StudioSnapshotIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    user: u32,
    group: u32,
    length: u64,
}

#[cfg(not(unix))]
#[derive(Clone, Copy, PartialEq, Eq)]
struct StudioSnapshotIdentity {
    length: u64,
}

pub(super) fn handle_studio_transaction(
    body: &str,
    context: Option<&StudioContext>,
) -> (&'static str, String) {
    let Some(context) = context else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing Studio project context\"}".to_string(),
        );
    };
    let request = match studio_request(body) {
        Ok(request) => request,
        Err(error) => {
            return (
                "400 Bad Request",
                format!("{{\"error\":{}}}", JSON::quote(&error)),
            )
        }
    };
    let op = studio_request_string(&request, "op").unwrap_or_default();
    if op == "session" {
        let session_id = studio_changeset_token("studio-session", &studio_unique_id());
        return match context.sessions.lock() {
            Ok(mut sessions) => {
                sessions.insert(session_id.clone());
                (
                    "200 OK",
                    format!("{{\"session_id\":{}}}", JSON::quote(&session_id)),
                )
            }
            Err(_) => (
                "500 Internal Server Error",
                "{\"error\":\"Studio session state is unavailable\"}".to_string(),
            ),
        };
    }
    let Some(session_id) = studio_request_string(&request, "session_id") else {
        return (
            "401 Unauthorized",
            "{\"error\":\"Studio transaction requires a server-issued session\"}".to_string(),
        );
    };
    let valid_session = context
        .sessions
        .lock()
        .map(|sessions| sessions.contains(&session_id))
        .unwrap_or(false);
    if !valid_session {
        return (
            "401 Unauthorized",
            "{\"error\":\"Studio session is not valid\"}".to_string(),
        );
    }
    if op == "status" {
        return studio_changeset_status(context, &request);
    }
    if op == "discard" {
        return studio_changeset_discard(context, &request);
    }
    if op == "apply" {
        return studio_changeset_apply(context, &request);
    }
    if op == "stage-rollback" {
        return studio_changeset_stage_rollback(context, &request);
    }
    if op != "set-option" {
        return (
            "400 Bad Request",
            "{\"error\":\"unsupported Studio transaction\"}".to_string(),
        );
    }
    let Some(key) = studio_request_string(&request, "key") else {
        return ("400 Bad Request", "{\"error\":\"missing key\"}".to_string());
    };
    let Some(value) = studio_request_string(&request, "value") else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing value\"}".to_string(),
        );
    };
    if studio_request_bool(&request, "write") {
        return (
            "400 Bad Request",
            "{\"error\":\"direct Studio writes are disabled\",\"fix\":\"stage the edit, review the exact diff, then Apply the Changeset\"}".to_string(),
        );
    }
    let _source_write = match context.source_write.lock() {
        Ok(lock) => lock,
        Err(_) => {
            return (
                "500 Internal Server Error",
                "{\"error\":\"Studio source lock is unavailable\"}".to_string(),
            )
        }
    };
    let _source_file_lock = match acquire_studio_source_file_lock(&context.config) {
        Ok(lock) => lock,
        Err(error) => {
            return (
                "500 Internal Server Error",
                format!("{{\"error\":{}}}", JSON::quote(&error)),
            )
        }
    };
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
        if !studio_request_owns_changeset(&request, staged) {
            return studio_changeset_owner_conflict();
        }
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
            session_id: session_id.clone(),
            token: studio_changeset_token(&source, &session_id),
            base_revision: studio_source_revision(&source),
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

fn studio_changeset_status(
    context: &StudioContext,
    request: &std::collections::BTreeMap<String, JSON::JSONValue>,
) -> (&'static str, String) {
    match context.changeset.lock() {
        Ok(changeset) => {
            if let Some(staged) = changeset.as_ref() {
                if !studio_request_owns_changeset(request, staged) {
                    return studio_changeset_owner_conflict();
                }
            }
            (
                "200 OK",
                studio_changeset_response(
                    context,
                    if changeset.is_some() { "staged" } else { "empty" },
                    changeset.is_some(),
                    false,
                    changeset.as_ref(),
                ),
            )
        }
        Err(_) => (
            "500 Internal Server Error",
            "{\"error\":\"Studio changeset state is unavailable\"}".to_string(),
        ),
    }
}

fn studio_changeset_discard(
    context: &StudioContext,
    request: &std::collections::BTreeMap<String, JSON::JSONValue>,
) -> (&'static str, String) {
    match context.changeset.lock() {
        Ok(mut changeset) => {
            if let Some(staged) = changeset.as_ref() {
                if !studio_request_owns_changeset(request, staged) {
                    return studio_changeset_owner_conflict();
                }
            }
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

fn studio_changeset_apply(
    context: &StudioContext,
    request: &std::collections::BTreeMap<String, JSON::JSONValue>,
) -> (&'static str, String) {
    let _source_write = match context.source_write.lock() {
        Ok(lock) => lock,
        Err(_) => {
            return (
                "500 Internal Server Error",
                "{\"error\":\"Studio source lock is unavailable\"}".to_string(),
            )
        }
    };
    let _source_file_lock = match acquire_studio_source_file_lock(&context.config) {
        Ok(lock) => lock,
        Err(error) => {
            return (
                "500 Internal Server Error",
                format!("{{\"error\":{}}}", JSON::quote(&error)),
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
    let Some(staged) = changeset.as_ref() else {
        return (
            "409 Conflict",
            "{\"error\":\"no staged Changeset to apply\"}".to_string(),
        );
    };
    if !studio_request_owns_changeset(request, staged) {
        return studio_changeset_owner_conflict();
    }
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
    if current != staged.base_source || studio_source_revision(&current) != staged.base_revision {
        return (
            "409 Conflict",
            "{\"error\":\"config.jet changed after this Changeset was staged\",\"fix\":\"discard or review the Changeset against current source\"}".to_string(),
        );
    }
    let receipt = match atomic_write_studio_source_if_revision(
        &context.config,
        &staged.next_source,
        &staged.base_revision,
    ) {
        Ok(receipt) => receipt,
        Err(e) => {
            let status = if e.starts_with("source revision changed") {
                "409 Conflict"
            } else {
                "500 Internal Server Error"
            };
            return (
                status,
                format!(
                    "{{\"error\":{},\"reprojected\":false}}",
                    JSON::quote(&format!("applying Changeset failed: {e}"))
                ),
            );
        }
    };
    if let Err(error) = rebuild_studio_projection(context) {
        let restore = if receipt.matches(&context.config) {
            atomic_write_studio_source_if_revision(
                &context.config,
                &staged.base_source,
                &receipt.revision,
            )
            .map(|_| ())
        } else {
            Err("source changed after Studio rename; external content preserved".to_string())
        };
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
    if receipt.matches(&context.config) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if !receipt.matches(&context.config) {
        return (
            "409 Conflict",
            "{\"error\":\"config.jet changed during Apply; external content was preserved\",\"reprojected\":false}".to_string(),
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
    *changeset = None;
    let response = studio_changeset_response(context, "applied", true, true, None);
    ("200 OK", response)
}

fn studio_changeset_stage_rollback(
    context: &StudioContext,
    request: &std::collections::BTreeMap<String, JSON::JSONValue>,
) -> (&'static str, String) {
    let _source_write = match context.source_write.lock() {
        Ok(lock) => lock,
        Err(_) => {
            return (
                "500 Internal Server Error",
                "{\"error\":\"Studio source lock is unavailable\"}".to_string(),
            )
        }
    };
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
    let session_id = studio_request_string(request, "session_id").unwrap_or_default();
    *changeset = Some(StudioChangeSet {
        session_id: session_id.clone(),
        token: studio_changeset_token(&current, &session_id),
        base_revision: studio_source_revision(&current),
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
    let (count, token, base_revision, diff, source, edits) = match changeset {
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
                staged.token.clone(),
                staged.base_revision.clone(),
                source_diff(&context.config, &staged.base_source, &staged.next_source),
                staged.next_source.clone(),
                edits,
            )
        }
        None => (
            0,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
        ),
    };
    format!(
        "{{\"host\":{},\"path\":{},\"state\":{},\"write\":false,\"changed\":{},\"staged_count\":{},\"token\":{},\"base_revision\":{},\"reprojected\":{},\"diff\":{},\"source\":{},\"edits\":[{}]}}",
        JSON::quote(&context.host),
        JSON::quote(&context.config.display().to_string()),
        JSON::quote(state),
        if changed { "true" } else { "false" },
        if state == "applied" { 0 } else { count },
        if token.is_empty() { "null".to_string() } else { JSON::quote(&token) },
        if base_revision.is_empty() { "null".to_string() } else { JSON::quote(&base_revision) },
        if reprojected { "true" } else { "false" },
        JSON::quote(&diff),
        JSON::quote(&source),
        edits,
    )
}

fn atomic_write_studio_source_if_revision(
    path: &Path,
    source: &str,
    expected_revision: &str,
) -> Result<StudioSourceWriteReceipt, String> {
    use std::io::Write;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.jet");
    let temp = parent.join(format!(".{name}.studio-{}.tmp", studio_unique_id()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
            options.mode(metadata.permissions().mode());
        }
        let mut file = options.open(&temp).map_err(|error| error.to_string())?;
        file.write_all(source.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        let current = std::fs::read(path).map_err(|error| error.to_string())?;
        let current_revision = crate::SHA256::sha256_hex(&current);
        if current_revision != expected_revision {
            return Err(format!(
                "source revision changed: expected {expected_revision}, found {current_revision}"
            ));
        }
        std::fs::rename(&temp, path).map_err(|error| error.to_string())?;
        if let Ok(parent_dir) = std::fs::File::open(parent) {
            let _ = parent_dir.sync_all();
        }
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        Ok(StudioSourceWriteReceipt {
            identity: studio_snapshot_identity(&metadata),
            revision: studio_source_revision(source),
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub(super) fn handle_studio_run(body: &str, context: Option<&StudioContext>) -> (&'static str, String) {
    let Some(context) = context else {
        return (
            "400 Bad Request",
            "{\"error\":\"missing Studio project context\"}".to_string(),
        );
    };
    let request = match studio_request(body) {
        Ok(request) => request,
        Err(error) => {
            return (
                "400 Bad Request",
                format!("{{\"error\":{}}}", JSON::quote(&error)),
            )
        }
    };
    let Some(action) = studio_request_string(&request, "action") else {
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
    let command = if action == "proof" {
        run_studio_proof_action(context)
    } else if action == "switch" {
        run_studio_switch_action(context)
    } else {
        run_studio_command(context, &action)
    };
    let result = match command {
        Ok(result) => result,
        Err(error) => {
            refresh_studio_projection(context);
            let status = if error.starts_with("current config.jet has no matching") {
                "409 Conflict"
            } else {
                "500 Internal Server Error"
            };
            return (status, format!("{{\"error\":{}}}", JSON::quote(&error)));
        }
    };
    refresh_studio_projection(context);
    ("200 OK", studio_command_json(context, &result))
}

fn run_studio_proof_action(context: &StudioContext) -> Result<StudioCommandResult, String> {
    let _source_write = context
        .source_write
        .lock()
        .map_err(|_| "Studio source lock is unavailable".to_string())?;
    let _source_file_lock = acquire_studio_source_file_lock(&context.config)?;
    if let Ok(mut proved) = context.proved_source.lock() {
        *proved = None;
    }
    let captured = std::fs::read_to_string(&context.config)
        .map_err(|error| format!("reading config failed: {error}"))?;
    let source_revision = studio_source_revision(&captured);
    let snapshot = create_studio_source_snapshot(context, &captured)?;
    let mut plan = run_studio_snapshot_command(context, &snapshot, "plan", None, None)?;
    if !plan.success {
        plan.action = "proof".to_string();
        plan.source_revision = Some(source_revision);
        return Ok(plan);
    }
    let input_plan_revision = studio_source_revision(plan.stdout.trim());
    let generation_name = format!(
        "zz-studio-proof-{}",
        &studio_unique_id()[..16]
    );
    let mut build = run_studio_snapshot_command(
        context,
        &snapshot,
        "build",
        Some(&generation_name),
        Some(&input_plan_revision),
    )?;
    if !build.success {
        build.action = "proof".to_string();
        build.source_revision = Some(source_revision);
        return Ok(build);
    }
    let mut result = run_studio_snapshot_command(
        context,
        &snapshot,
        "proof",
        Some(&generation_name),
        None,
    )?;
    result.source_revision = Some(source_revision.clone());
    let artifact = if result.success {
        match validate_studio_proof_artifact(
            &result.stdout,
            &source_revision,
            &input_plan_revision,
        ) {
            Ok(artifact) => Some(artifact),
            Err(error) => {
                result.success = false;
                result.status = 3;
                result.stderr = error;
                None
            }
        }
    } else {
        None
    };
    let unchanged = std::fs::read_to_string(&context.config)
        .map(|source| source == captured)
        .unwrap_or(false);
    result.source_changed_after = !unchanged;
    if result.success && unchanged {
        if let (Some(artifact), Ok(mut proved)) =
            (artifact, context.proved_source.lock())
        {
            *proved = Some(StudioProvedSource {
                source: captured,
                revision: source_revision,
                plan_revision: input_plan_revision,
                artifact_plan_revision: artifact.plan_revision,
                generation: artifact.generation,
                generation_path: artifact.path,
            });
        }
    } else if !unchanged {
        result.success = false;
        result.status = 3;
        result.stderr =
            "config.jet changed while proof was running; proof was not bound".to_string();
    }
    Ok(result)
}

fn run_studio_switch_action(context: &StudioContext) -> Result<StudioCommandResult, String> {
    let _source_write = context
        .source_write
        .lock()
        .map_err(|_| "Studio source lock is unavailable".to_string())?;
    let _source_file_lock = acquire_studio_source_file_lock(&context.config)?;
    let current = std::fs::read_to_string(&context.config).unwrap_or_default();
    let current_identity = std::fs::metadata(&context.config)
        .map(|metadata| studio_snapshot_identity(&metadata))
        .map_err(|error| format!("reading config.jet metadata failed: {error}"))?;
    let proved = context
        .proved_source
        .lock()
        .ok()
        .and_then(|proved| proved.clone())
        .ok_or_else(|| {
            "current config.jet has no matching successful proof; build and prove this exact source before switching".to_string()
        })?;
    if current != proved.source || studio_source_revision(&current) != proved.revision {
        return Err(
            "current config.jet has no matching successful proof; build and prove this exact source before switching".to_string(),
        );
    }
    validate_studio_generation_binding(&proved)?;
    let snapshot = create_studio_source_snapshot(context, &proved.source)?;
    let plan = run_studio_snapshot_command(context, &snapshot, "plan", None, None)?;
    if !plan.success || studio_source_revision(plan.stdout.trim()) != proved.plan_revision {
        return Err(
            "current config.jet plan no longer matches its successful proof; prove again before switching".to_string(),
        );
    }
    let previous_generation = studio_current_generation_name();
    let mut result = run_studio_snapshot_command(
        context,
        &snapshot,
        "switch",
        Some(&proved.generation),
        None,
    )?;
    result.source_revision = Some(proved.revision);
    let source_unchanged = std::fs::read_to_string(&context.config)
        .map(|source| source == proved.source)
        .unwrap_or(false)
        && std::fs::metadata(&context.config)
            .map(|metadata| studio_snapshot_identity(&metadata) == current_identity)
            .unwrap_or(false);
    result.source_changed_after = !source_unchanged;
    if !source_unchanged {
        let rollback = previous_generation
            .as_deref()
            .map(|name| run_studio_generation_rollback(context, name))
            .transpose();
        result.success = false;
        result.status = 3;
        result.stderr = match rollback {
            Ok(Some(rollback)) if rollback.success => {
                "config.jet changed during switch; proved generation activation was rolled back".to_string()
            }
            Ok(Some(rollback)) => format!(
                "config.jet changed during switch; rollback failed: {}",
                rollback.stderr.trim()
            ),
            Ok(None) => "config.jet changed during switch; no previous generation was available for rollback".to_string(),
            Err(error) => format!("config.jet changed during switch; rollback failed: {error}"),
        };
    }
    Ok(result)
}

fn validate_studio_generation_binding(proved: &StudioProvedSource) -> Result<(), String> {
    let source_proof = std::fs::read_to_string(proved.generation_path.join("source-proof.json"))
        .map_err(|error| format!("reading proved generation source proof failed: {error}"))?;
    let parsed = JSON::parse(&source_proof)
        .map_err(|error| format!("proved generation source proof is invalid: {error}"))?;
    let JSON::JSONValue::Object(fields) = parsed else {
        return Err("proved generation source proof is not an object".to_string());
    };
    let string = |name: &str| match fields.get(name) {
        Some(JSON::JSONValue::Str(value)) => Some(value.as_str()),
        _ => None,
    };
    let plan = std::fs::read(proved.generation_path.join("plan.json"))
        .map_err(|error| format!("reading proved generation plan failed: {error}"))?;
    if string("source_sha256") != Some(proved.revision.as_str())
        || string("input_plan_sha256") != Some(proved.plan_revision.as_str())
        || string("plan_sha256") != Some(proved.artifact_plan_revision.as_str())
        || crate::SHA256::sha256_hex(&plan) != proved.artifact_plan_revision
        || proved.generation_path.file_name().and_then(|name| name.to_str())
            != Some(proved.generation.as_str())
    {
        return Err("proved generation source or plan binding changed".to_string());
    }
    Ok(())
}

fn invalidate_studio_projection(context: &StudioContext) {
    if let Ok(mut projection) = context.live_projection.lock() {
        *projection = None;
    }
}

fn refresh_studio_projection(context: &StudioContext) {
    invalidate_studio_projection(context);
    let _ = rebuild_studio_projection(context);
}

fn validate_studio_proof_artifact(
    proof: &str,
    source_revision: &str,
    input_plan_revision: &str,
) -> Result<StudioProofArtifact, String> {
    let parsed = JSON::parse(proof.trim())
        .map_err(|error| format!("generation proof JSON is invalid: {error}"))?;
    let JSON::JSONValue::Object(root) = parsed else {
        return Err("generation proof is not a JSON object".to_string());
    };
    let Some(JSON::JSONValue::Object(source_proof)) = root.get("source_proof") else {
        return Err("generation proof is missing source_proof".to_string());
    };
    let field = |name: &str| match source_proof.get(name) {
        Some(JSON::JSONValue::Str(value)) => Some(value.as_str()),
        _ => None,
    };
    if field("source_sha256") != Some(source_revision) {
        return Err("generation proof source hash does not match captured config.jet".to_string());
    }
    if field("input_plan_sha256") != Some(input_plan_revision) {
        return Err("generation proof input plan hash does not match captured plan".to_string());
    }
    let Some(JSON::JSONValue::Str(plan)) = root.get("plan") else {
        return Err("generation proof is missing its plan artifact".to_string());
    };
    let artifact_plan_revision = studio_source_revision(plan);
    if field("plan_sha256") != Some(artifact_plan_revision.as_str()) {
        return Err("generation proof plan hash does not match its plan artifact".to_string());
    }
    let generation = match root.get("generation") {
        Some(JSON::JSONValue::Str(value)) => value.clone(),
        _ => return Err("generation proof is missing its generation ID".to_string()),
    };
    let path = match root.get("path") {
        Some(JSON::JSONValue::Str(value)) => PathBuf::from(value),
        _ => return Err("generation proof is missing its generation path".to_string()),
    };
    Ok(StudioProofArtifact {
        generation,
        path,
        plan_revision: artifact_plan_revision,
    })
}

fn run_studio_command(
    context: &StudioContext,
    action: &str,
) -> Result<StudioCommandResult, String> {
    run_studio_command_target(context, action, &context.host, None, None, None)
}

fn studio_current_generation_name() -> Option<String> {
    let root = std::env::var_os("JETPACK_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".jetpack")
        });
    let current = root.join("systems/current");
    #[cfg(unix)]
    let target = std::fs::read_link(current).ok()?;
    #[cfg(not(unix))]
    let target = PathBuf::from(std::fs::read_to_string(current).ok()?.trim());
    target
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

fn run_studio_generation_rollback(
    context: &StudioContext,
    generation: &str,
) -> Result<StudioCommandResult, String> {
    let Some(jet) = sibling_binary("jet") else {
        return Err("could not find sibling jet binary".to_string());
    };
    let output = std::process::Command::new(jet)
        .arg("os")
        .arg("rollback")
        .arg(&context.host)
        .arg(generation)
        .arg("--no-color")
        .current_dir(context.config.parent().unwrap_or_else(|| Path::new(".")))
        .output()
        .map_err(|error| format!("running jet rollback failed: {error}"))?;
    Ok(StudioCommandResult {
        action: "rollback".to_string(),
        status: output.status.code().unwrap_or(1),
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        source_revision: None,
        source_changed_after: false,
    })
}

fn create_studio_source_snapshot(
    context: &StudioContext,
    source: &str,
) -> Result<StudioSourceSnapshot, String> {
    create_studio_source_snapshot_platform(context, source)
}

#[cfg(target_os = "linux")]
fn create_studio_source_snapshot_platform(
    _context: &StudioContext,
    source: &str,
) -> Result<StudioSourceSnapshot, String> {
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::PermissionsExt;
    use std::ffi::CString;
    const MFD_ALLOW_SEALING: u32 = 0x0002;
    const F_ADD_SEALS: i32 = 1033;
    const REQUIRED_SEALS: i32 = 0x0001 | 0x0002 | 0x0004 | 0x0008;
    extern "C" {
        fn memfd_create(name: *const std::ffi::c_char, flags: u32) -> i32;
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    let name = CString::new(format!("jetos-studio-source-{}", studio_unique_id()))
        .map_err(|_| "creating sealed Studio source name failed".to_string())?;
    let descriptor = unsafe { memfd_create(name.as_ptr(), MFD_ALLOW_SEALING) };
    if descriptor < 0 {
        return Err(format!(
            "creating sealed Studio source failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut file = unsafe { std::fs::File::from_raw_fd(descriptor) };
    file.write_all(source.as_bytes())
        .map_err(|error| format!("writing sealed Studio source failed: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("syncing sealed Studio source failed: {error}"))?;
    file.set_permissions(std::fs::Permissions::from_mode(0o400))
        .map_err(|error| format!("protecting sealed Studio source failed: {error}"))?;
    if unsafe { fcntl(file.as_raw_fd(), F_ADD_SEALS, REQUIRED_SEALS) } != 0 {
        return Err(format!(
            "sealing Studio source failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    let metadata = file
        .metadata()
        .map_err(|error| format!("reading sealed Studio source metadata failed: {error}"))?;
    let snapshot = StudioSourceSnapshot {
        path: PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())),
        file,
        cleanup_dir: None,
        source_revision: studio_source_revision(source),
        identity: studio_snapshot_identity(&metadata),
    };
    snapshot.validate()?;
    Ok(snapshot)
}

#[cfg(not(target_os = "linux"))]
fn create_studio_source_snapshot_platform(
    context: &StudioContext,
    source: &str,
) -> Result<StudioSourceSnapshot, String> {
    use std::io::Write;
    let parent = context.config.parent().unwrap_or_else(|| Path::new("."));
    let dir = parent.join(format!(".jetos-studio-snapshot-{}", studio_unique_id()));
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut dir_builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        dir_builder.mode(0o700);
    }
    dir_builder
        .create(&dir)
        .map_err(|error| format!("creating protected Studio snapshot failed: {error}"))?;
    let path = dir.join("config.jet");
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|error| format!("creating protected Studio source failed: {error}"))?;
        file.write_all(source.as_bytes())
            .map_err(|error| format!("writing protected Studio source failed: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("syncing protected Studio source failed: {error}"))?;
        let metadata = std::fs::metadata(&path)
            .map_err(|error| format!("reading protected Studio source metadata failed: {error}"))?;
        let snapshot = StudioSourceSnapshot {
            file,
            path,
            cleanup_dir: Some(dir.clone()),
            source_revision: studio_source_revision(source),
            identity: studio_snapshot_identity(&metadata),
        };
        snapshot.validate()?;
        Ok(snapshot)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

fn run_studio_snapshot_command(
    context: &StudioContext,
    snapshot: &StudioSourceSnapshot,
    action: &str,
    generation_name: Option<&str>,
    input_plan_revision: Option<&str>,
) -> Result<StudioCommandResult, String> {
    snapshot.validate()?;
    let target = format!("{}@{}", snapshot.path.display(), context.host);
    let result = run_studio_command_target(
        context,
        action,
        &target,
        generation_name,
        Some(context.config.parent().unwrap_or_else(|| Path::new("."))),
        input_plan_revision,
    );
    snapshot.validate()?;
    result
}

fn run_studio_command_target(
    context: &StudioContext,
    action: &str,
    target: &str,
    generation_name: Option<&str>,
    source_base: Option<&Path>,
    input_plan_revision: Option<&str>,
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
        .arg(target)
        .arg("--no-color");
    if action == "plan" || action == "proof" {
        cmd.arg("--json");
    }
    if context.offline {
        cmd.arg("--offline");
    }
    if let Some(source_base) = source_base {
        cmd.env("JETOS_STUDIO_SOURCE_BASE", source_base);
    }
    if let Some(input_plan_revision) = input_plan_revision {
        cmd.env("JETOS_STUDIO_INPUT_PLAN_SHA256", input_plan_revision);
    }
    if action == "build" || action == "switch" || (action == "proof" && generation_name.is_some()) {
        cmd.arg("--name")
            .arg(generation_name.unwrap_or("zz-studio-candidate"));
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
        source_revision: None,
        source_changed_after: false,
    })
}

fn studio_command_json(context: &StudioContext, result: &StudioCommandResult) -> String {
    format!(
        "{{\"host\":{},\"action\":{},\"status\":{},\"success\":{},\"source_revision\":{},\"source_changed_after\":{},\"stdout\":{},\"stderr\":{}}}",
        JSON::quote(&context.host),
        JSON::quote(&result.action),
        result.status,
        if result.success { "true" } else { "false" },
        result
            .source_revision
            .as_deref()
            .map(JSON::quote)
            .unwrap_or_else(|| "null".to_string()),
        if result.source_changed_after { "true" } else { "false" },
        JSON::quote(&result.stdout),
        JSON::quote(&result.stderr)
    )
}

pub(super) fn studio_live_projection(context: &StudioContext, generation_data: &Path) -> Result<String, String> {
    if !context.config.is_file() {
        return std::fs::read_to_string(generation_data)
            .map_err(|e| format!("reading installed Studio projection failed: {e}"));
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
    let generations = run_studio_command(context, "generations")
        .map(|result| if result.success { result.stdout } else { result.stderr })
        .unwrap_or_default();
    let current_source = std::fs::read_to_string(&context.config).unwrap_or_default();
    let proof_revision = context
        .proved_source
        .lock()
        .ok()
        .and_then(|proved| proved.clone())
        .filter(|proved| {
            proved.source == current_source
                && proved.revision == crate::SHA256::sha256_hex(current_source.as_bytes())
        })
        .map(|proved| proved.revision);
    let projection = format!(
        "{{\"kind\":\"jetos-studio-projection\",\"source_truth\":\"live-checked-plan\",\"host\":{},\"page_registry\":[{}],\"system_plan\":{},\"proof_state\":{{\"state\":{},\"source_revision\":{}}},\"generations\":{},\"generation_projection\":{}}}",
        JSON::quote(&context.host),
        crate::JetOS::studio_pages_json(),
        plan.stdout.trim(),
        JSON::quote(if proof_revision.is_some() { "proved" } else { "unproved" }),
        proof_revision
            .as_deref()
            .map(JSON::quote)
            .unwrap_or_else(|| "null".to_string()),
        JSON::quote(&generations),
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

fn studio_request(
    body: &str,
) -> Result<std::collections::BTreeMap<String, JSON::JSONValue>, String> {
    let parsed = JSON::parse(body).map_err(|error| format!("invalid Studio JSON request: {error}"))?;
    match parsed {
        JSON::JSONValue::Object(object) => Ok(object),
        _ => Err("Studio request must be a JSON object".to_string()),
    }
}

fn studio_request_string(
    request: &std::collections::BTreeMap<String, JSON::JSONValue>,
    key: &str,
) -> Option<String> {
    match request.get(key) {
        Some(JSON::JSONValue::Str(value)) => Some(value.clone()),
        _ => None,
    }
}

fn studio_request_bool(
    request: &std::collections::BTreeMap<String, JSON::JSONValue>,
    key: &str,
) -> bool {
    matches!(request.get(key), Some(JSON::JSONValue::Bool(true)))
}

fn studio_source_revision(source: &str) -> String {
    crate::SHA256::sha256_hex(source.as_bytes())
}

fn studio_unique_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    crate::SHA256::sha256_hex(
        format!("{}:{now}:{nonce}", std::process::id()).as_bytes(),
    )
}

impl StudioSourceSnapshot {
    fn validate(&self) -> Result<(), String> {
        use std::io::{Read, Seek};
        let metadata = self
            .file
            .metadata()
            .map_err(|error| format!("protected Studio source changed: {error}"))?;
        if !metadata.file_type().is_file() || studio_snapshot_identity(&metadata) != self.identity {
            return Err("protected Studio source was mutated or replaced".to_string());
        }
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::PermissionsExt;
            const F_GET_SEALS: i32 = 1034;
            const REQUIRED_SEALS: i32 = 0x0001 | 0x0002 | 0x0004 | 0x0008;
            extern "C" {
                fn fcntl(fd: i32, command: i32, ...) -> i32;
            }
            if metadata.permissions().mode() & 0o777 != 0o400 {
                return Err("protected Studio source permissions changed".to_string());
            }
            if unsafe { fcntl(self.file.as_raw_fd(), F_GET_SEALS) } & REQUIRED_SEALS
                != REQUIRED_SEALS
            {
                return Err("sealed Studio source lost write seals".to_string());
            }
        }
        let mut source_file = self
            .file
            .try_clone()
            .map_err(|error| format!("cloning protected Studio source failed: {error}"))?;
        source_file
            .rewind()
            .map_err(|error| format!("seeking protected Studio source failed: {error}"))?;
        let mut source = Vec::new();
        source_file
            .read_to_end(&mut source)
            .map_err(|error| format!("reading protected Studio source failed: {error}"))?;
        if crate::SHA256::sha256_hex(&source) != self.source_revision {
            return Err("protected Studio source content changed".to_string());
        }
        Ok(())
    }
}

impl Drop for StudioSourceSnapshot {
    fn drop(&mut self) {
        if let Some(dir) = self.cleanup_dir.as_ref() {
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_dir(dir);
        }
    }
}

impl StudioSourceWriteReceipt {
    fn matches(&self, path: &Path) -> bool {
        let Ok(metadata) = std::fs::metadata(path) else {
            return false;
        };
        if studio_snapshot_identity(&metadata) != self.identity {
            return false;
        }
        std::fs::read(path)
            .map(|source| crate::SHA256::sha256_hex(&source) == self.revision)
            .unwrap_or(false)
    }
}

#[cfg(unix)]
fn acquire_studio_source_file_lock(path: &Path) -> Result<StudioSourceFileLock, String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    const O_NOFOLLOW: i32 = 0x20000;
    const LOCK_EX: i32 = 2;
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config.jet");
    let lock_path = parent.join(format!(".{name}.studio.lock"));
    if std::fs::symlink_metadata(&lock_path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err("Studio source lock path is a symlink".to_string());
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW)
        .open(&lock_path)
        .map_err(|error| format!("opening Studio source lock failed: {error}"))?;
    if unsafe { flock(file.as_raw_fd(), LOCK_EX) } != 0 {
        return Err(format!(
            "locking Studio source failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(StudioSourceFileLock { _file: file })
}

#[cfg(not(unix))]
fn acquire_studio_source_file_lock(_path: &Path) -> Result<StudioSourceFileLock, String> {
    Err("cross-process Studio source locks require Unix".to_string())
}

#[cfg(unix)]
fn studio_snapshot_identity(metadata: &std::fs::Metadata) -> StudioSnapshotIdentity {
    use std::os::unix::fs::MetadataExt;
    StudioSnapshotIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
        mode: metadata.mode(),
        user: metadata.uid(),
        group: metadata.gid(),
        length: metadata.len(),
    }
}

#[cfg(not(unix))]
fn studio_snapshot_identity(metadata: &std::fs::Metadata) -> StudioSnapshotIdentity {
    StudioSnapshotIdentity {
        length: metadata.len(),
    }
}

fn studio_changeset_token(source: &str, session_id: &str) -> String {
    crate::SHA256::sha256_hex(
        format!(
            "{}:{session_id}:{}",
            studio_source_revision(source),
            studio_unique_id()
        )
        .as_bytes(),
    )
}

fn studio_request_owns_changeset(
    request: &std::collections::BTreeMap<String, JSON::JSONValue>,
    changeset: &StudioChangeSet,
) -> bool {
    studio_request_string(request, "session_id").as_deref()
        == Some(changeset.session_id.as_str())
        && studio_request_string(request, "token").as_deref() == Some(changeset.token.as_str())
        && studio_request_string(request, "base_revision").as_deref()
            == Some(changeset.base_revision.as_str())
}

fn studio_changeset_owner_conflict() -> (&'static str, String) {
    (
        "409 Conflict",
        "{\"error\":\"Changeset token or base revision does not own the staged transaction\",\"fix\":\"refresh Studio and use the token returned by stage\"}".to_string(),
    )
}
