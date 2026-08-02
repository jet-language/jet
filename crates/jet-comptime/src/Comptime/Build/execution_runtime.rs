use super::actions_policy::{ActionCache, BuildAction, BuildCapability};
use super::cache_cas::{
    ActionCacheProvenance, ActionCacheStatus, ActionInputSnapshot, ActionKey, ActionOutcome,
    ActionOutputRecord, ActionResultRecord, CacheHitReason, CacheMissReason, ContentDigest,
    FrontEndCompletion, LocalCas, RemoteBuildBinding, RemoteCacheError, RemoteCachePolicy,
    RemoteCacheTransport, RemoteDeniedReason, RemoteExecutionRequest, atomic_restore_file,
    ensure_real_directory, secure_read_file,
};
use super::errors_keys::BuildError;
use super::execution_helpers::action_pools;
use super::handles::{ActionHandle, ActionId, ProbeId, ToolchainHandle};
use super::plan_graph::{BuildExecutionReport, BuildPlan};
use super::provenance_toolchains::{ProbeKind, ReproducibilityClass};
use super::targets::BuildPath;
use super::validation::resolve_under;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, io, time::{Duration, Instant}};

static REMOTE_ATTEMPT: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProbeFact {
    pub name: String,
    pub success: bool,
    pub detail: String,
    pub reproducibility: ReproducibilityClass,
    pub toolchain: ToolchainHandle,
    pub toolchain_provenance: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildExecutionResult {
    pub report: BuildExecutionReport,
    pub probes: Vec<BuildProbeFact>,
}

#[derive(Debug)]
pub enum BuildExecutionError {
    MissingGrant { action: String, capability: BuildCapability },
    SandboxUnavailable,
    IO { action: String, detail: String },
    ActionFailed { action: String, exit_code: i32, stderr: String },
    ProbeFailed { probe: String, detail: String },
    InvalidGraph(BuildError),
}

/// Execute canonical action graph. Linux uses bubblewrap with a private mount,
/// PID, IPC, UTS and network namespace (network shared only when granted).
/// There is no unsandboxed fallback.
pub fn execute_build_plan(
    plan: &BuildPlan,
    project_root: &Path,
    grants: &BTreeSet<BuildCapability>,
) -> Result<BuildExecutionResult, BuildExecutionError> {
    execute_build_plan_with_front_end_and_remote(
        plan,
        project_root,
        grants,
        FrontEndCompletion::all_complete(),
        None,
    )
}

/// Same as [`execute_build_plan`], but cache lookup requires a complete
/// parser/sema/policy/diagnostics front end (E4-JP2 / #419).
pub fn execute_build_plan_with_front_end(
    plan: &BuildPlan,
    project_root: &Path,
    grants: &BTreeSet<BuildCapability>,
    front_end: FrontEndCompletion,
) -> Result<BuildExecutionResult, BuildExecutionError> {
    execute_build_plan_with_front_end_and_remote(
        plan,
        project_root,
        grants,
        front_end,
        None,
    )
}

/// Execute with an explicitly selected host-owned builder binding. No
/// environment variable, repository field, or command-line endpoint can
/// activate this path.
pub fn execute_build_plan_with_front_end_and_remote(
    plan: &BuildPlan,
    project_root: &Path,
    grants: &BTreeSet<BuildCapability>,
    front_end: FrontEndCompletion,
    remote_binding: Option<&RemoteBuildBinding>,
) -> Result<BuildExecutionResult, BuildExecutionError> {
    let selected_actions = plan.selected_action_ids().map_err(BuildExecutionError::InvalidGraph)?;
    for action in plan.actions.iter().filter(|action| selected_actions.contains(&action.id)) {
        for cap in &action.caps {
            if !grants.contains(cap) {
                return Err(BuildExecutionError::MissingGrant {
                    action: action.name.clone(), capability: *cap
                });
            }
        }
    }
    let selected_probes = plan.selected_probe_ids().map_err(BuildExecutionError::InvalidGraph)?;
    if !selected_probes.is_empty() && !grants.contains(&BuildCapability::Exec) {
        let probe = &plan.probes[selected_probes.iter().next().unwrap().0];
        return Err(BuildExecutionError::MissingGrant {
            action: format!("probe {}", probe.name),
            capability: BuildCapability::Exec,
        });
    }
    let probes = execute_probes(plan, &selected_probes)?;
    let model = plan.execution_model().map_err(BuildExecutionError::InvalidGraph)?;
    let cas = LocalCas::new(project_root.join(".jet/build-cache/cas"));
    let records = project_root.join(".jet/build-cache/actions");
    ensure_real_directory(&records).map_err(|e| BuildExecutionError::IO {
        action: "cache".to_string(), detail: e.to_string()
    })?;
    let mut outcomes = Vec::new();
    for stage in model.stages {
        for batch in execution_batches(plan, &stage.actions) {
            for action_id in &batch {
                let action = &plan.actions[action_id.0];
                for cap in &action.caps {
                    if !grants.contains(cap) {
                        return Err(BuildExecutionError::MissingGrant {
                            action: action.name.clone(), capability: cap.clone()
                        });
                    }
                }
            }
            let mut completed = std::thread::scope(|scope| {
                let cas = &cas;
                let records = &records;
                let probe_facts = &probes;
                let jobs = batch
                    .iter()
                    .map(|action_id| {
                        let action = &plan.actions[action_id.0];
                        let handle = ActionHandle { id: action.id, context: plan.context };
                        (handle, scope.spawn(move || {
                            execute_one_action(
                                plan,
                                action,
                                handle,
                                project_root,
                                cas,
                                records,
                                grants,
                                probe_facts,
                                front_end,
                                remote_binding,
                            )
                        }))
                    })
                    .collect::<Vec<_>>();
                jobs.into_iter()
                    .map(|(handle, job)| {
                        let result = job.join().map_err(|_| BuildExecutionError::IO {
                            action: plan.actions[handle.id.0].name.clone(),
                            detail: "sandbox worker panicked".to_string(),
                        })?;
                        Ok((handle, result?))
                    })
                    .collect::<Result<Vec<_>, BuildExecutionError>>()
            })?;
            completed.sort_by_key(|(handle, _)| handle.id);
            outcomes.extend(completed);
        }
    }
    let report = plan.execution_report(&outcomes).map_err(BuildExecutionError::InvalidGraph)?;
    Ok(BuildExecutionResult { report, probes })
}

/// Deterministic resource-pool packing. CPU/memory pools use automatic
/// capacity; linker/console/GPU/custom pools serialize by name.
fn execution_batches(plan: &BuildPlan, actions: &[ActionId]) -> Vec<Vec<ActionId>> {
    let mut batches: Vec<Vec<ActionId>> = Vec::new();
    let limits = plan
        .resource_pools()
        .into_iter()
        .map(|spec| (spec.pool.as_str().to_string(), spec.slots))
        .collect::<BTreeMap<_, _>>();
    let mut held: Vec<BTreeMap<String, usize>> = Vec::new();
    for action_id in actions {
        let pools = action_pools(&plan.actions[action_id.0])
            .into_iter()
            .map(|pool| pool.as_str().to_string())
            .collect::<BTreeSet<_>>();
        let slot = held
            .iter()
            .position(|used| {
                pools.iter().all(|pool| {
                    let limit = limits.get(pool).copied().unwrap_or(1);
                    limit == 0 || used.get(pool).copied().unwrap_or(0) < limit
                })
            })
            .unwrap_or_else(|| {
                batches.push(Vec::new());
                held.push(BTreeMap::new());
                batches.len() - 1
            });
        for pool in pools {
            *held[slot].entry(pool).or_default() += 1;
        }
        batches[slot].push(*action_id);
    }
    batches
}

fn execute_one_action(
    plan: &BuildPlan,
    action: &BuildAction,
    handle: ActionHandle,
    project_root: &Path,
    cas: &LocalCas,
    records: &Path,
    grants: &BTreeSet<BuildCapability>,
    probe_facts: &[BuildProbeFact],
    front_end: FrontEndCompletion,
    remote_binding: Option<&RemoteBuildBinding>,
) -> Result<ActionOutcome, BuildExecutionError> {
    let snapshots = cas.snapshot_declared_inputs(project_root, action).map_err(|e| io_action(action, e))?;
    let remote_requested = remote_binding.is_some_and(RemoteBuildBinding::is_enabled);
    let executable = resolve_program_path(plan, action.toolchain, &action.argv[0])
        .or_else(|| {
            // A remote cache hit also needs a stable command identity, but it
            // must not require the local machine to have the target toolchain.
            // The declared argv spelling is the remote identity fallback; a
            // local miss still fails below instead of silently running it from
            // PATH.
            remote_requested
                .then(|| PathBuf::from(&action.argv[0]))
        })
        .ok_or_else(|| BuildExecutionError::IO {
            action: action.name.clone(),
            detail: format!("tool {} was not found", action.argv[0]),
        })?;
    let executable_bytes = if executable.is_file() {
        fs::read(&executable).map_err(|e| io_action(action, e))?
    } else {
        action.argv[0].as_bytes().to_vec()
    };
    let executable_digest = ContentDigest::from_bytes(&executable_bytes);
    let action_probe_names = action.probes.iter().map(|probe| plan.probes[probe.id.0].name.as_str()).collect::<BTreeSet<_>>();
    let effective_probe_facts = probe_facts.iter().filter(|fact| action_probe_names.contains(fact.name.as_str())).cloned().collect::<Vec<_>>();
    let key = plan.effective_action_key(
        handle,
        &snapshots,
        grants,
        &executable,
        &executable_digest,
        &effective_probe_facts,
    ).map_err(BuildExecutionError::InvalidGraph)?;
    let record_path = records.join(key.as_str().trim_start_matches("act-sha256:"));
    let remote = remote_for_action(plan, action, &key, grants, remote_binding)?;
    let previous_key = read_last_rebuild_record(project_root, action.id, &action.name)
        .map_err(|error| io_action(action, error))?
        .map(|record| record.key);
    let mut restore_failure = None;
    if action.cache == ActionCache::Cached {
        // E4-JP2: no cache lookup may bypass parser/sema/policy/diagnostics.
        match front_end.authorize_cache_lookup() {
            Ok(()) => match read_action_record(records, &record_path, key.clone()) {
                Ok(Some(record)) => match cas.restore_action_outputs(project_root, action, &record) {
                    Ok(()) => {
                        write_last_rebuild_record(
                            project_root,
                            action,
                            &key,
                            ActionCacheStatus::Hit(CacheHitReason::LocalActionRecordMatched),
                            None,
                        )?;
                        return Ok(ActionOutcome::RestoredFromCache);
                    }
                    Err(error) => {
                        restore_failure = Some(cache_restore_miss_reason(&error));
                    }
                },
                Ok(None) => {}
                Err(error) => restore_failure = Some(cache_restore_miss_reason(&error)),
            },
            Err(_) => {
                restore_failure = Some(CacheMissReason::FrontEndIncomplete);
            }
        }
    }
    if action.cache == ActionCache::Cached
        && front_end.authorize_cache_lookup().is_ok()
    {
        if let Some((transport, policy, _execute)) = &remote {
            match transport.download_action_record(&key, policy) {
                Ok(record) => {
                    if !matches!(
                        record.outcome,
                        ActionOutcome::Succeeded { .. } | ActionOutcome::RestoredFromCache
                    ) {
                        return Err(remote_action(
                            action,
                            "remote cache record contains a failed outcome".to_string(),
                        ));
                    }
                    match restore_remote_outputs(
                        transport,
                        policy,
                        project_root,
                        action,
                        &record,
                        super::cache_cas::RemoteActionRequest::CacheRead,
                    ) {
                        Ok(()) => {
                            write_last_rebuild_record(
                                project_root,
                                action,
                                &key,
                                ActionCacheStatus::Hit(CacheHitReason::LocalActionRecordMatched),
                                None,
                            )?;
                            return Ok(ActionOutcome::RestoredFromCache);
                        }
                        Err(_detail)
                            if remote_binding
                                .is_some_and(|binding| binding.fallback_local) => {}
                        Err(detail) => return Err(remote_action(action, detail)),
                    }
                }
                Err(RemoteCacheError::Io(error))
                    if error.kind() == io::ErrorKind::NotFound => {}
                Err(RemoteCacheError::Denied(denied))
                    if denied.reason == RemoteDeniedReason::GrantNotAllowed => {}
                Err(_error)
                    if remote_binding.is_some_and(|binding| binding.fallback_local) => {}
                Err(error) => return Err(remote_action(action, error.to_string())),
            }
        }
    }

    let rebuild_status = if action.cache == ActionCache::UncachedPhony {
        ActionCacheStatus::Miss(CacheMissReason::UncachedAction)
    } else if let Some(reason) = restore_failure {
        ActionCacheStatus::Miss(reason)
    } else if previous_key.as_ref().is_some_and(|old| old != &key) {
        ActionCacheStatus::Miss(CacheMissReason::ActionKeyChanged)
    } else {
        ActionCacheStatus::Miss(CacheMissReason::NoLocalActionRecord)
    };

    if let Some((transport, policy, true)) = &remote {
        let timeout_ms = remote_binding
            .map(|binding| binding.timeout_ms)
            .unwrap_or(30_000);
        let remote_result = execute_remote_action(
            plan,
            action,
            project_root,
            cas,
            &record_path,
            &snapshots,
            &key,
            transport,
            policy,
            rebuild_status,
            timeout_ms,
        );
        match remote_result {
            Ok(outcome) => return Ok(outcome),
            Err(_error) if remote_binding.is_some_and(|binding| binding.fallback_local) => {}
            Err(error) => return Err(error),
        }
    }

    if remote_binding.is_some_and(|binding| {
        binding.cache_read && !binding.execute && !binding.fallback_local
    }) {
        return Err(remote_action(
            action,
            "remote cache miss cannot fall back to local execution".to_string(),
        ));
    }

    let sandbox = project_root.join(".jet/build-sandbox").join(format!(
        "{}-{}-{}", std::process::id(), action.id.0, key.as_str().trim_start_matches("act-sha256:")
    ));
    let sandbox_root = sandbox
        .parent()
        .ok_or_else(|| io_action(action, io::Error::new(io::ErrorKind::InvalidInput, "sandbox has no parent")))?;
    ensure_real_directory(sandbox_root).map_err(|e| io_action(action, e))?;
    if let Ok(metadata) = fs::symlink_metadata(&sandbox) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io_action(
                action,
                io::Error::new(io::ErrorKind::PermissionDenied, "action sandbox is not a real directory"),
            ));
        }
        fs::remove_dir_all(&sandbox).map_err(|e| io_action(action, e))?;
    }
    ensure_real_directory(&sandbox).map_err(|e| io_action(action, e))?;
    for input in &action.inputs {
        let from = resolve_under(project_root, input.as_str()).map_err(|e| io_action(action, e))?;
        let to = resolve_under(&sandbox, input.as_str()).map_err(|e| io_action(action, e))?;
        if let Some(parent) = to.parent() { fs::create_dir_all(parent).map_err(|e| io_action(action, e))?; }
        let bytes = super::cache_cas::secure_read_file(project_root, &from)
            .map_err(|e| io_action(action, e))?;
        fs::write(to, bytes).map_err(|e| io_action(action, e))?;
    }
    for output in &action.outputs {
        let path = resolve_under(&sandbox, output.as_str()).map_err(|e| io_action(action, e))?;
        if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| io_action(action, e))?; }
    }

    let bwrap = find_program_path("bwrap").ok_or(BuildExecutionError::SandboxUnavailable)?;
    let mut command = Command::new(bwrap);
    command
        .arg("--die-with-parent").arg("--new-session").arg("--unshare-all")
        .arg("--ro-bind").arg("/nix/store").arg("/nix/store")
        .arg("--proc").arg("/proc").arg("--dev").arg("/dev")
        .arg("--tmpfs").arg("/tmp")
        .arg("--bind").arg(&sandbox).arg("/work")
        .arg("--chdir").arg("/work").arg("--clearenv");
    if grants.contains(&BuildCapability::Net) && action.caps.contains(&BuildCapability::Net) {
        command.arg("--share-net");
    }
    command.arg("--setenv").arg("PATH").arg("/nix/store");
    for (key, value) in action
        .env
        .iter()
        .filter(|(key, _)| action.env_allowlist.is_empty() || action.env_allowlist.contains(key.as_str()))
    {
        command.arg("--setenv").arg(key).arg(value);
    }
    command.arg(executable).args(&action.argv[1..]);
    let output = command.output().map_err(|e| io_action(action, e))?;
    let code = output.status.code().unwrap_or(1);
    if !output.status.success() {
        let _ = fs::remove_dir_all(&sandbox);
        write_last_rebuild_record(project_root, action, &key, rebuild_status, Some(code))?;
        return Err(BuildExecutionError::ActionFailed {
            action: action.name.clone(), exit_code: code,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    for declared in &action.outputs {
        let from = resolve_under(&sandbox, declared.as_str()).map_err(|e| io_action(action, e))?;
        let to = resolve_under(project_root, declared.as_str()).map_err(|e| io_action(action, e))?;
        let bytes = super::cache_cas::secure_read_file(&sandbox, &from)
            .map_err(|e| io_action(action, e))?;
        prepare_output_destination(project_root, &to).map_err(|e| io_action(action, e))?;
        super::cache_cas::atomic_restore_file(project_root, &to, &bytes)
            .map_err(|e| io_action(action, e))?;
    }
    let outcome = ActionOutcome::Succeeded { exit_code: code };
    if action.cache == ActionCache::Cached {
        let record = cas.capture_declared_outputs(
            project_root, action, key.clone(), outcome,
            ActionCacheProvenance::miss(CacheMissReason::NoLocalActionRecord),
        ).map_err(|e| io_action(action, e))?;
        if let Some((transport, policy, _)) = &remote {
            if policy
                .check(super::cache_cas::RemoteActionRequest::CacheWrite)
                .is_ok()
            {
                publish_remote_outputs(transport, policy, project_root, &record)
                    .map_err(|detail| remote_action(action, detail))?;
            }
        }
        write_action_record(&record_path, &record).map_err(|e| io_action(action, e))?;
    }
    write_last_rebuild_record(project_root, action, &key, rebuild_status, None)?;
    fs::remove_dir_all(&sandbox).map_err(|e| io_action(action, e))?;
    Ok(outcome)
}

fn execute_remote_action(
    plan: &BuildPlan,
    action: &BuildAction,
    project_root: &Path,
    cas: &LocalCas,
    record_path: &Path,
    snapshots: &[ActionInputSnapshot],
    key: &ActionKey,
    transport: &RemoteCacheTransport,
    policy: &RemoteCachePolicy,
    rebuild_status: ActionCacheStatus,
    timeout_ms: u64,
) -> Result<ActionOutcome, BuildExecutionError> {
    let proof = policy
        .proof()
        .cloned()
        .ok_or_else(|| remote_action(action, "remote sandbox proof is missing".to_string()))?;
    for snapshot in snapshots {
        let path = resolve_under(project_root, snapshot.path.as_str())
            .map_err(|e| io_action(action, e))?;
        let bytes = secure_read_file(project_root, &path).map_err(|e| io_action(action, e))?;
        let digest = transport
            .upload_execution_blob(&bytes, policy)
            .map_err(|error| remote_action(action, error.to_string()))?;
        if digest != snapshot.digest || bytes.len() as u64 != snapshot.byte_len {
            return Err(remote_action(
                action,
                format!(
                    "remote input CAS identity changed for {}",
                    snapshot.path.as_str()
                ),
            ));
        }
    }
    let request = RemoteExecutionRequest {
        key: key.clone(),
        argv: action.argv.clone(),
        inputs: snapshots.to_vec(),
        outputs: action.outputs.clone(),
        toolchain_digest: toolchain_provenance_digest(plan, action.toolchain),
        sandbox: proof,
    };
    transport
        .submit_execution(&request, policy)
        .map_err(|error| remote_action(action, error.to_string()))?;
    let result = wait_remote_execution_result(transport, policy, key, action, timeout_ms)?;
    match result.outcome {
        ActionOutcome::Succeeded { exit_code } => {
            restore_remote_outputs(
                transport,
                policy,
                project_root,
                action,
                &ActionResultRecord {
                    key: result.key.clone(),
                    outcome: result.outcome,
                    outputs: result.outputs.clone(),
                    provenance: ActionCacheProvenance::miss(CacheMissReason::RemoteDenied),
                },
                super::cache_cas::RemoteActionRequest::Execute,
            )
            .map_err(|detail| remote_action(action, detail))?;
            if action.cache == ActionCache::Cached {
                let record = cas
                    .capture_declared_outputs(
                        project_root,
                        action,
                        key.clone(),
                        ActionOutcome::Succeeded { exit_code },
                        ActionCacheProvenance::miss(CacheMissReason::RemoteDenied),
                    )
                    .map_err(|e| io_action(action, e))?;
                if policy
                    .check(super::cache_cas::RemoteActionRequest::CacheWrite)
                    .is_ok()
                {
                    publish_remote_outputs(transport, policy, project_root, &record)
                        .map_err(|detail| remote_action(action, detail))?;
                }
                write_action_record(record_path, &record).map_err(|e| io_action(action, e))?;
            }
            write_last_rebuild_record(project_root, action, key, rebuild_status, None)?;
            Ok(ActionOutcome::Succeeded { exit_code })
        }
        ActionOutcome::Failed { exit_code } => {
            write_last_rebuild_record(project_root, action, key, rebuild_status, Some(exit_code))?;
            Err(BuildExecutionError::ActionFailed {
                action: action.name.clone(),
                exit_code,
                stderr: "remote execution failed".to_string(),
            })
        }
        ActionOutcome::RestoredFromCache => Err(remote_action(
            action,
            "remote execution returned a cache-only outcome".to_string(),
        )),
    }
}

fn remote_for_action(
    plan: &BuildPlan,
    action: &BuildAction,
    key: &ActionKey,
    grants: &BTreeSet<BuildCapability>,
    binding: Option<&RemoteBuildBinding>,
) -> Result<Option<(RemoteCacheTransport, RemoteCachePolicy, bool)>, BuildExecutionError> {
    let Some(binding) = binding.filter(|binding| binding.is_enabled()) else {
        return Ok(None);
    };
    if binding.trust_domain.trim().is_empty() {
        return Err(remote_action(
            action,
            "remote builder binding has no trust domain".to_string(),
        ));
    }
    if binding.trust_domain.len() > 256
        || binding.trust_domain.chars().any(|character| character.is_control())
    {
        return Err(remote_action(
            action,
            "remote builder binding has an invalid trust domain".to_string(),
        ));
    }
    if !grants.contains(&BuildCapability::Net) {
        return Err(BuildExecutionError::MissingGrant {
            action: format!("remote build transport for {}", action.name),
            capability: BuildCapability::Net,
        });
    }
    let transport = RemoteCacheTransport::for_binding(binding)
        .map_err(|detail| remote_action(action, detail))?;
    let proof = transport
        .sandbox_proof(
            format!(
                "remote:{}:{}:local-{}-{}-{}",
                binding.builder,
                binding.trust_domain,
                std::process::id(),
                action.id.0,
                REMOTE_ATTEMPT.fetch_add(1, Ordering::Relaxed)
            ),
            key.as_str(),
            toolchain_provenance_digest(plan, action.toolchain),
        )
        .map_err(|detail| remote_action(action, detail))?;
    if !action.caps.contains(&BuildCapability::Net) {
        return Err(BuildExecutionError::MissingGrant {
            action: format!("remote build transport for {}", action.name),
            capability: BuildCapability::Net,
        });
    }
    Ok(Some((
        transport,
        RemoteCachePolicy::with_grants(
            binding.cache_read,
            binding.cache_write,
            binding.execute,
            proof,
        ),
        binding.execute,
    )))
}

fn wait_remote_execution_result(
    transport: &RemoteCacheTransport,
    policy: &RemoteCachePolicy,
    key: &ActionKey,
    action: &BuildAction,
    timeout_ms: u64,
) -> Result<super::cache_cas::RemoteExecutionResult, BuildExecutionError> {
    let timeout = Duration::from_millis(timeout_ms);
    let deadline = Instant::now() + timeout;
    loop {
        match transport.download_execution_result(key, policy) {
            Ok(result) => return Ok(result),
            Err(RemoteCacheError::Io(error))
                if error.kind() == io::ErrorKind::NotFound
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(RemoteCacheError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                let _ = transport.cancel_execution(key, policy);
                return Err(remote_action(
                    action,
                    format!(
                        "remote worker did not publish a result within {}ms",
                        timeout.as_millis()
                    ),
                ));
            }
            Err(error) => {
                let _ = transport.cancel_execution(key, policy);
                return Err(remote_action(action, error.to_string()));
            }
        }
    }
}

fn restore_remote_outputs(
    transport: &RemoteCacheTransport,
    policy: &RemoteCachePolicy,
    project_root: &Path,
    action: &BuildAction,
    record: &ActionResultRecord,
    blob_request: super::cache_cas::RemoteActionRequest,
) -> Result<(), String> {
    if record.outputs.len() != action.outputs.len()
        || record
            .outputs
            .iter()
            .zip(&action.outputs)
            .any(|(output, declared)| output.path != *declared)
    {
        return Err("remote output record does not exactly match action declarations".to_string());
    }
    let mut staged = Vec::with_capacity(record.outputs.len());
    let mut backups = Vec::with_capacity(record.outputs.len());
    for output in &record.outputs {
        let bytes = match blob_request {
            super::cache_cas::RemoteActionRequest::CacheRead => transport.download_blob(&output.digest, policy),
            super::cache_cas::RemoteActionRequest::Execute => {
                transport.download_execution_blob(&output.digest, policy)
            }
            super::cache_cas::RemoteActionRequest::CacheWrite => {
                return Err("remote output restore cannot use a cache-write grant".to_string());
            }
        }
        .map_err(|error| error.to_string())?;
        if bytes.len() as u64 != output.byte_len
            || ContentDigest::from_bytes(&bytes) != output.digest
        {
            return Err(format!(
                "remote output blob {} failed digest or length verification",
                output.path.as_str()
            ));
        }
        let path = resolve_under(project_root, output.path.as_str())
            .map_err(|error| error.to_string())?;
        let previous = match secure_read_file(project_root, &path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        backups.push((path.clone(), previous));
        staged.push((path, bytes));
    }
    for (path, bytes) in &staged {
        if let Err(error) = prepare_output_destination(project_root, path)
            .and_then(|()| atomic_restore_file(project_root, path, bytes))
        {
            rollback_output_restore(project_root, &backups);
            return Err(error.to_string());
        }
    }
    Ok(())
}

fn rollback_output_restore(project_root: &Path, backups: &[(PathBuf, Option<Vec<u8>>)]) {
    for (path, previous) in backups.iter().rev() {
        match previous {
            Some(bytes) => {
                let _ = prepare_output_destination(project_root, path)
                    .and_then(|()| atomic_restore_file(project_root, path, bytes));
            }
            None => match fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_symlink() => {}
                Ok(metadata) if metadata.is_file() => {
                    let _ = fs::remove_file(path);
                }
                _ => {}
            },
        }
    }
}

fn publish_remote_outputs(
    transport: &RemoteCacheTransport,
    policy: &RemoteCachePolicy,
    project_root: &Path,
    record: &ActionResultRecord,
) -> Result<(), String> {
    let mut outputs = Vec::with_capacity(record.outputs.len());
    for output in &record.outputs {
        let path = resolve_under(project_root, output.path.as_str())
            .map_err(|error| error.to_string())?;
        let bytes = secure_read_file(project_root, &path).map_err(|error| error.to_string())?;
        let digest = transport
            .upload_blob(&bytes, policy)
            .map_err(|error| error.to_string())?;
        if digest != output.digest || bytes.len() as u64 != output.byte_len {
            return Err(format!(
                "remote output CAS identity changed for {}",
                output.path.as_str()
            ));
        }
        outputs.push(ActionOutputRecord {
            path: output.path.clone(),
            digest,
            byte_len: bytes.len() as u64,
        });
    }
    transport
        .upload_action_record(
            &ActionResultRecord {
                key: record.key.clone(),
                outcome: record.outcome,
                outputs,
                provenance: record.provenance.clone(),
            },
            policy,
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn remote_action(action: &BuildAction, detail: String) -> BuildExecutionError {
    BuildExecutionError::IO {
        action: format!("remote {}", action.name),
        detail,
    }
}

pub(super) struct LastRebuildRecord {
    pub(super) key: ActionKey,
    pub(super) status: ActionCacheStatus,
    pub(super) failed_exit_code: Option<i32>,
}

fn rebuild_record_path(project_root: &Path, action: ActionId) -> PathBuf {
    project_root
        .join(".jet/build-cache/explanations")
        .join(action.0.to_string())
}

fn rebuild_status_code(status: ActionCacheStatus) -> &'static str {
    match status {
        ActionCacheStatus::Hit(CacheHitReason::LocalActionRecordMatched) => "hit-local",
        ActionCacheStatus::Hit(CacheHitReason::DeclaredOutputsRestored) => "hit-output",
        ActionCacheStatus::Miss(CacheMissReason::NoLocalActionRecord) => "miss-new",
        ActionCacheStatus::Miss(CacheMissReason::ActionKeyChanged) => "miss-key",
        ActionCacheStatus::Miss(CacheMissReason::DeclaredOutputMissing) => "miss-output",
        ActionCacheStatus::Miss(CacheMissReason::CacheRecordInvalid) => "miss-invalid",
        ActionCacheStatus::Miss(CacheMissReason::CacheRestoreFailed) => "miss-restore",
        ActionCacheStatus::Miss(CacheMissReason::RemoteDenied) => "miss-remote",
        ActionCacheStatus::Miss(CacheMissReason::UncachedAction) => "miss-uncached",
        ActionCacheStatus::Miss(CacheMissReason::FrontEndIncomplete) => "miss-frontend",
    }
}

fn parse_rebuild_status(code: &str) -> Option<ActionCacheStatus> {
    Some(match code {
        "hit-local" => ActionCacheStatus::Hit(CacheHitReason::LocalActionRecordMatched),
        "hit-output" => ActionCacheStatus::Hit(CacheHitReason::DeclaredOutputsRestored),
        "miss-new" => ActionCacheStatus::Miss(CacheMissReason::NoLocalActionRecord),
        "miss-key" => ActionCacheStatus::Miss(CacheMissReason::ActionKeyChanged),
        "miss-output" => ActionCacheStatus::Miss(CacheMissReason::DeclaredOutputMissing),
        "miss-invalid" => ActionCacheStatus::Miss(CacheMissReason::CacheRecordInvalid),
        "miss-restore" => ActionCacheStatus::Miss(CacheMissReason::CacheRestoreFailed),
        "miss-remote" => ActionCacheStatus::Miss(CacheMissReason::RemoteDenied),
        "miss-uncached" => ActionCacheStatus::Miss(CacheMissReason::UncachedAction),
        "miss-frontend" => ActionCacheStatus::Miss(CacheMissReason::FrontEndIncomplete),
        _ => return None,
    })
}

pub(super) fn read_last_rebuild_record(
    project_root: &Path,
    action: ActionId,
    action_name: &str,
) -> io::Result<Option<LastRebuildRecord>> {
    let path = rebuild_record_path(project_root, action);
    let bytes = match secure_read_file(project_root, &path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let text = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "rebuild explanation is not UTF-8"))?;
    let mut lines = text.lines();
    let Some(key) = lines.next() else { return Ok(None) };
    let action_digest = ContentDigest::from_bytes(action_name.as_bytes());
    if lines.next() != Some(action_digest.as_str()) {
        return Ok(None);
    }
    let Some(status) = lines.next().and_then(parse_rebuild_status) else { return Ok(None) };
    Ok(Some(LastRebuildRecord {
        key: ActionKey(key.to_string()),
        status,
        failed_exit_code: lines
            .next()
            .and_then(|line| line.strip_prefix("failed:"))
            .and_then(|code| code.parse().ok()),
    }))
}

fn write_last_rebuild_record(
    project_root: &Path,
    action: &BuildAction,
    key: &ActionKey,
    status: ActionCacheStatus,
    failed_exit_code: Option<i32>,
) -> Result<(), BuildExecutionError> {
    let path = rebuild_record_path(project_root, action.id);
    let text = format!(
        "{}\n{}\n{}\n{}",
        key.as_str(),
        ContentDigest::from_bytes(action.name.as_bytes()).as_str(),
        rebuild_status_code(status),
        failed_exit_code
            .map(|code| format!("failed:{code}\n"))
            .unwrap_or_default()
    );
    atomic_restore_file(project_root, &path, text.as_bytes()).map_err(|error| BuildExecutionError::IO {
        action: format!("rebuild explanation {}", action.name),
        detail: error.to_string(),
    })
}

pub(super) fn prepare_output_destination(root: &Path, output: &Path) -> io::Result<()> {
    let parent = output.parent().unwrap_or(root);
    let relative = parent.strip_prefix(root).map_err(|_| io::Error::new(
        io::ErrorKind::InvalidInput,
        "build output parent escapes project root",
    ))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if let Component::Normal(part) = component {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("build output parent `{}` is a symlink", current.display()),
                )),
                Ok(meta) if !meta.is_dir() => return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    format!("build output parent `{}` is not a directory", current.display()),
                )),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&current)?,
                Err(error) => return Err(error),
            }
        }
    }
    if fs::symlink_metadata(output).is_ok_and(|meta| meta.file_type().is_symlink()) {
        fs::remove_file(output)?;
    }
    Ok(())
}

fn execute_probes(
    plan: &BuildPlan,
    selected: &BTreeSet<ProbeId>,
) -> Result<Vec<BuildProbeFact>, BuildExecutionError> {
    let mut facts = Vec::new();
    for probe in plan.probes.iter().filter(|probe| selected.contains(&probe.id)) {
        let (success, detail) = match &probe.kind {
            ProbeKind::FindProgram { program } => match resolve_program_path(plan, probe.toolchain, program) {
                Some(path) => (true, path.display().to_string()),
                None => (false, format!("program `{program}` not found")),
            },
            ProbeKind::PkgConfig { package, min_version } => {
                let Some(pkg_config) = resolve_program_path(plan, probe.toolchain, "pkg-config") else {
                    return Err(BuildExecutionError::ProbeFailed { probe: probe.name.clone(), detail: "pkg-config not found".to_string() });
                };
                let mut cmd = Command::new(pkg_config);
                cmd.arg("--exists");
                if let Some(version) = min_version { cmd.arg(format!("{package} >= {version}")); } else { cmd.arg(package); }
                match cmd.status() { Ok(status) => (status.success(), format!("pkg-config exit {}", status.code().unwrap_or(1))), Err(e) => (false, e.to_string()) }
            }
            ProbeKind::HeaderCheck { header } => {
                let source = format!("#include <{header}>\nint main(void) {{ return 0; }}\n");
                run_compile_probe(plan, probe.toolchain, &source)
            }
            ProbeKind::CompileCheck { includes, code, .. } => {
                let mut source = String::new();
                for header in includes {
                    source.push_str(&format!("#include <{header}>\n"));
                }
                source.push_str("int main(void) {\n");
                source.push_str(code);
                source.push_str("\nreturn 0;\n}\n");
                run_compile_probe(plan, probe.toolchain, &source)
            }
        };
        facts.push(BuildProbeFact {
            name: probe.name.clone(),
            success,
            detail: detail.clone(),
            reproducibility: probe.reproducibility,
            toolchain: probe.toolchain,
            toolchain_provenance: toolchain_provenance_digest(plan, probe.toolchain),
        });
        if !success { return Err(BuildExecutionError::ProbeFailed { probe: probe.name.clone(), detail }); }
    }
    Ok(facts)
}

fn run_compile_probe(plan: &BuildPlan, toolchain: ToolchainHandle, source: &str) -> (bool, String) {
    let compiler = ["cc", "clang", "gcc"]
        .into_iter()
        .find_map(|name| resolve_program_path(plan, toolchain, name));
    let Some(compiler) = compiler else {
        return (false, "no C compiler (`cc`, `clang`, or `gcc`) was found".to_string());
    };
    let mut child = match Command::new(&compiler)
        .args(["-x", "c", "-fsyntax-only", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return (false, format!("could not start {}: {error}", compiler.display())),
    };
    let Some(mut stdin) = child.stdin.take() else {
        return (false, "could not open compiler input".to_string());
    };
    if let Err(error) = stdin.write_all(source.as_bytes()) {
        return (false, format!("could not send source to {}: {error}", compiler.display()));
    }
    drop(stdin);
    match child.wait_with_output() {
        Ok(output) if output.status.success() => {
            (true, format!("{} accepted the syntax probe", compiler.display()))
        }
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            (false, if detail.is_empty() {
                format!("{} rejected the syntax probe", compiler.display())
            } else {
                detail
            })
        }
        Err(error) => (false, format!("could not wait for {}: {error}", compiler.display())),
    }
}

fn find_program_path(program: &str) -> Option<PathBuf> {
    let direct = Path::new(program);
    if direct.components().count() > 1 && direct.is_file() { return fs::canonicalize(direct).ok(); }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(program);
            candidate.is_file().then(|| fs::canonicalize(candidate).ok()).flatten()
        })
    })
}

fn resolve_program_path(
    plan: &BuildPlan,
    toolchain_handle: ToolchainHandle,
    program: &str,
) -> Option<PathBuf> {
    let toolchain = plan.toolchain(toolchain_handle)?;
    if let Some(declared) = toolchain.tools.get(program) {
        return canonical_executable_path(declared);
    }
    let basename = Path::new(program).file_name().and_then(|name| name.to_str());
    if let Some(basename) = basename {
        if let Some(declared) = toolchain.tools.get(basename) {
            return canonical_executable_path(declared);
        }
    }
    if Path::new(program).components().count() > 1 {
        if let Some(path) = canonical_executable_path(program) {
            return Some(path);
        }
    }
    // The host toolchain is explicitly ambient. A target toolchain must name
    // the executable in its declaration; falling through to PATH here would
    // make a cross build depend on whichever host tool happens to be installed.
    if matches!(toolchain.role, super::provenance_toolchains::ToolchainRole::Host) {
        find_program_path(program)
    } else if target_matches_running_host(&toolchain.target_triple) {
        // An explicit native target still runs on the host. Keep the target
        // identity in the graph, but resolve its tools from the ambient host
        // toolchain just as the documented `native` build example requires.
        find_program_path(program)
    } else {
        None
    }
}

fn target_matches_running_host(triple: &str) -> bool {
    let arch = std::env::consts::ARCH;
    let linux = [
        format!("{arch}-linux"),
        format!("{arch}-linux-gnu"),
        format!("{arch}-unknown-linux-gnu"),
    ];
    let macos = [
        format!("{arch}-macos"),
        format!("{arch}-apple-darwin"),
    ];
    let windows = [
        format!("{arch}-windows"),
        format!("{arch}-pc-windows-msvc"),
        format!("{arch}-pc-windows-gnu"),
    ];
    match std::env::consts::OS {
        "linux" => linux.iter().any(|candidate| candidate == triple),
        "macos" => macos.iter().any(|candidate| candidate == triple),
        "windows" => windows.iter().any(|candidate| candidate == triple),
        _ => false,
    }
}

fn canonical_executable_path(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    path.is_file().then(|| fs::canonicalize(path).ok()).flatten()
}

fn toolchain_provenance_digest(plan: &BuildPlan, toolchain: ToolchainHandle) -> ContentDigest {
    let identity = plan
        .toolchain(toolchain)
        .map(|value| format!("{value:?}"))
        .unwrap_or_else(|| "missing-toolchain".to_string());
    ContentDigest::from_bytes(identity.as_bytes())
}

fn write_action_record(path: &Path, record: &ActionResultRecord) -> io::Result<()> {
    let mut text = format!("{}\n", record.key.as_str());
    for output in &record.outputs {
        text.push_str(&format!("{}\t{}\t{}\n", output.path.as_str(), output.digest.as_str(), output.byte_len));
    }
    let root = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "action record has no cache directory"))?;
    atomic_restore_file(root, path, text.as_bytes())
}

fn cache_restore_miss_reason(error: &io::Error) -> CacheMissReason {
    match error.kind() {
        io::ErrorKind::NotFound => CacheMissReason::DeclaredOutputMissing,
        io::ErrorKind::InvalidData => CacheMissReason::CacheRecordInvalid,
        _ => CacheMissReason::CacheRestoreFailed,
    }
}

pub(super) fn read_action_record(
    root: &Path,
    path: &Path,
    key: ActionKey,
) -> io::Result<Option<ActionResultRecord>> {
    let bytes = match secure_read_file(root, path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let text = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "action record is not UTF-8"))?;
    let mut lines = text.lines();
    if lines.next() != Some(key.as_str()) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "action record key does not match its cache path"));
    }
    let mut outputs = Vec::new();
    for line in lines {
        let mut parts = line.split('\t');
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "malformed action output record");
        let path = BuildPath::new(parts.next().ok_or_else(invalid)?).map_err(|_| invalid())?;
        let digest = ContentDigest::parse(parts.next().ok_or_else(invalid)?)?;
        let byte_len = parts.next().ok_or_else(invalid)?.parse().map_err(|_| invalid())?;
        if parts.next().is_some() {
            return Err(invalid());
        }
        outputs.push(ActionOutputRecord { path, digest, byte_len });
    }
    Ok(Some(ActionResultRecord {
        key, outcome: ActionOutcome::RestoredFromCache, outputs,
        provenance: ActionCacheProvenance::hit(CacheHitReason::LocalActionRecordMatched),
    }))
}

fn io_action(action: &BuildAction, error: io::Error) -> BuildExecutionError {
    BuildExecutionError::IO { action: action.name.clone(), detail: error.to_string() }
}
