use super::actions_policy::{ActionCache, BuildAction, BuildCapability, LegacyWrapperKind};
use super::cache_cas::{
    atomic_restore_file, ensure_real_directory, remote_execution_identity, remote_policy_digest,
    secure_read_file, ActionCacheProvenance, ActionCacheStatus, ActionInputSnapshot, ActionKey,
    ActionOutcome, ActionOutputRecord, ActionResultRecord, CacheHitReason, CacheMissReason,
    ContentDigest, FrontEndCompletion, LocalCas, RemoteBuildBinding, RemoteCacheError,
    RemoteCachePolicy, RemoteCacheTransport, RemoteDeniedReason, RemoteExecutionRequest,
};
use super::errors_keys::BuildError;
use super::execution_helpers::action_pools;
use super::handles::{ActionHandle, ActionId, ProbeId, ToolchainHandle};
use super::plan_graph::{BuildExecutionReport, BuildPlan};
use super::provenance_toolchains::{ProbeKind, ReproducibilityClass};
use super::remote_scheduler::RemoteBuilderLease;
use super::targets::BuildPath;
use super::validation::resolve_under;
use super::{RemoteBuildRequest, RemoteBuilder, RemoteScheduler};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{
    fs, io,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

static REMOTE_ATTEMPT: AtomicU64 = AtomicU64::new(1);
const MAX_REMOTE_EXECUTION_ATTEMPTS: usize = 2;

struct RemoteAttemptFailure {
    error: BuildExecutionError,
    retryable: bool,
}

impl RemoteAttemptFailure {
    fn terminal(error: BuildExecutionError) -> Self {
        Self {
            error,
            retryable: false,
        }
    }

    fn retryable(error: BuildExecutionError) -> Self {
        Self {
            error,
            retryable: true,
        }
    }
}

impl From<BuildExecutionError> for RemoteAttemptFailure {
    fn from(error: BuildExecutionError) -> Self {
        Self::terminal(error)
    }
}

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

/// Driver-owned implementation for a compiler action cache miss. The
/// executor still owns declared-output validation, atomic restore, CAS capture,
/// and action records.
pub type CompilerActionRunner<'a> =
    dyn Fn(&BuildAction, &[ActionInputSnapshot]) -> Result<Vec<Vec<u8>>, String> + Sync + 'a;

#[derive(Debug)]
pub enum BuildExecutionError {
    MissingGrant {
        action: String,
        capability: BuildCapability,
    },
    SandboxUnavailable,
    IO {
        action: String,
        detail: String,
    },
    ActionFailed {
        action: String,
        exit_code: i32,
        stderr: String,
    },
    ProbeFailed {
        probe: String,
        detail: String,
    },
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
    execute_build_plan_with_front_end_and_remote(plan, project_root, grants, front_end, None)
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
    execute_build_plan_with_front_end_and_remote_and_compiler(
        plan,
        project_root,
        grants,
        front_end,
        remote_binding,
        None,
    )
}

/// Execute a plan with compiler-owned package actions. User actions retain
/// the ordinary sandbox path; only compiler-owned cache misses call `runner`.
pub fn execute_build_plan_with_front_end_and_compiler(
    plan: &BuildPlan,
    project_root: &Path,
    grants: &BTreeSet<BuildCapability>,
    front_end: FrontEndCompletion,
    runner: &CompilerActionRunner<'_>,
) -> Result<BuildExecutionResult, BuildExecutionError> {
    execute_build_plan_with_front_end_and_remote_and_compiler(
        plan,
        project_root,
        grants,
        front_end,
        None,
        Some(runner),
    )
}

pub fn execute_build_plan_with_front_end_and_remote_and_compiler(
    plan: &BuildPlan,
    project_root: &Path,
    grants: &BTreeSet<BuildCapability>,
    front_end: FrontEndCompletion,
    remote_binding: Option<&RemoteBuildBinding>,
    compiler: Option<&CompilerActionRunner<'_>>,
) -> Result<BuildExecutionResult, BuildExecutionError> {
    let selected_actions = plan
        .selected_action_ids()
        .map_err(BuildExecutionError::InvalidGraph)?;
    for action in plan
        .actions
        .iter()
        .filter(|action| selected_actions.contains(&action.id))
    {
        for cap in &action.caps {
            if !grants.contains(cap) {
                return Err(BuildExecutionError::MissingGrant {
                    action: action.name.clone(),
                    capability: *cap,
                });
            }
        }
    }
    let selected_probes = plan
        .selected_probe_ids()
        .map_err(BuildExecutionError::InvalidGraph)?;
    if !selected_probes.is_empty() && !grants.contains(&BuildCapability::Exec) {
        let probe = &plan.probes[selected_probes.iter().next().unwrap().0];
        return Err(BuildExecutionError::MissingGrant {
            action: format!("probe {}", probe.name),
            capability: BuildCapability::Exec,
        });
    }
    let probes = execute_probes(plan, &selected_probes)?;
    let model = plan
        .execution_model()
        .map_err(BuildExecutionError::InvalidGraph)?;
    let cas = LocalCas::new(project_root.join(".jet/build-cache/cas"));
    let records = project_root.join(".jet/build-cache/actions");
    let remote_scheduler = remote_binding
        .filter(|binding| binding.is_enabled())
        .map(|binding| RemoteScheduler::new([RemoteBuilder::from_binding(binding.clone())]))
        .transpose()
        .map_err(|error| BuildExecutionError::IO {
            action: "remote scheduler".to_string(),
            detail: error.to_string(),
        })?;
    ensure_real_directory(&records).map_err(|e| BuildExecutionError::IO {
        action: "cache".to_string(),
        detail: e.to_string(),
    })?;
    let mut outcomes = Vec::new();
    for stage in model.stages {
        for batch in execution_batches(plan, &stage.actions) {
            for action_id in &batch {
                let action = &plan.actions[action_id.0];
                for cap in &action.caps {
                    if !grants.contains(cap) {
                        return Err(BuildExecutionError::MissingGrant {
                            action: action.name.clone(),
                            capability: cap.clone(),
                        });
                    }
                }
            }
            let mut completed = std::thread::scope(|scope| {
                let cas = &cas;
                let records = &records;
                let probe_facts = &probes;
                let remote_scheduler = remote_scheduler.as_ref();
                let jobs = batch
                    .iter()
                    .map(|action_id| {
                        let action = &plan.actions[action_id.0];
                        let handle = ActionHandle {
                            id: action.id,
                            context: plan.context,
                        };
                        (
                            handle,
                            scope.spawn(move || {
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
                                    remote_scheduler,
                                    compiler,
                                )
                            }),
                        )
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
    let report = plan
        .execution_report(&outcomes)
        .map_err(BuildExecutionError::InvalidGraph)?;
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
    remote_scheduler: Option<&RemoteScheduler>,
    compiler: Option<&CompilerActionRunner<'_>>,
) -> Result<ActionOutcome, BuildExecutionError> {
    let cache_lookup_allowed = front_end.authorize_cache_lookup().is_ok();
    let snapshots = cas
        .snapshot_declared_inputs(project_root, action)
        .map_err(|e| io_action(action, e))?;
    let remote_requested = remote_binding.is_some_and(RemoteBuildBinding::is_enabled);
    let (executable, executable_digest) = if action.is_compiler_owned() {
        (
            PathBuf::from("<jet-compiler>"),
            ContentDigest::from_bytes(b"jet-compiler"),
        )
    } else {
        let executable = resolve_program_path(plan, action.toolchain, &action.argv[0])
            .or_else(|| {
                // A remote cache hit also needs a stable command identity, but it
                // must not require the local machine to have the target toolchain.
                // The declared argv spelling is the remote identity fallback; a
                // local miss still fails below instead of silently running it from
                // PATH.
                remote_requested.then(|| PathBuf::from(&action.argv[0]))
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
        (executable, ContentDigest::from_bytes(&executable_bytes))
    };
    let action_probe_names = action
        .probes
        .iter()
        .map(|probe| plan.probes[probe.id.0].name.as_str())
        .collect::<BTreeSet<_>>();
    let effective_probe_facts = probe_facts
        .iter()
        .filter(|fact| action_probe_names.contains(fact.name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let key = plan
        .effective_action_key(
            handle,
            &snapshots,
            grants,
            &executable,
            &executable_digest,
            &effective_probe_facts,
        )
        .map_err(BuildExecutionError::InvalidGraph)?;
    let record_path = records.join(key.as_str().trim_start_matches("act-sha256:"));
    let remote = if action.is_compiler_owned() {
        None
    } else {
        remote_for_action(plan, action, &key, grants, remote_binding, remote_scheduler)?
    };
    let previous_key = read_last_rebuild_record(project_root, action.id, &action.name)
        .map_err(|error| io_action(action, error))?
        .map(|record| record.key);
    let mut restore_failure = None;
    if action.cache == ActionCache::Cached {
        // E4-JP2: no cache lookup may bypass parser/sema/policy/diagnostics.
        if cache_lookup_allowed {
            match read_action_record(records, &record_path, key.clone()) {
                Ok(Some(record)) => match cas.restore_action_outputs(project_root, action, &record)
                {
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
            }
        } else {
            restore_failure = Some(CacheMissReason::FrontEndIncomplete);
        }
    }
    if action.cache == ActionCache::Cached && cache_lookup_allowed {
        if let Some((transport, policy, _execute, _lease)) = &remote {
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
                        Ok(restored) => {
                            write_last_rebuild_record(
                                project_root,
                                action,
                                &key,
                                ActionCacheStatus::Hit(CacheHitReason::LocalActionRecordMatched),
                                None,
                            )?;
                            restored.commit();
                            return Ok(ActionOutcome::RestoredFromCache);
                        }
                        Err(_detail)
                            if remote_binding.is_some_and(|binding| binding.fallback_local) => {}
                        Err(detail) => return Err(remote_action(action, detail)),
                    }
                }
                Err(RemoteCacheError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {}
                Err(RemoteCacheError::Denied(denied))
                    if denied.reason == RemoteDeniedReason::GrantNotAllowed => {}
                Err(_error) if remote_binding.is_some_and(|binding| binding.fallback_local) => {}
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

    if action.legacy_wrapper == Some(LegacyWrapperKind::Npm)
        && action
            .labels
            .keys()
            .any(|key| key.starts_with("legacy.dependency."))
    {
        return Err(io_action(
            action,
            io::Error::new(
                io::ErrorKind::Unsupported,
                "npm dependency-bearing imports require a provisioned locked dependency tree",
            ),
        ));
    }

    if action.is_compiler_owned() {
        let runner = compiler.ok_or_else(|| BuildExecutionError::IO {
            action: action.name.clone(),
            detail: "compiler-owned action has no compiler runner".to_string(),
        })?;
        let bytes = runner(action, &snapshots).map_err(|detail| BuildExecutionError::IO {
            action: action.name.clone(),
            detail,
        })?;
        if bytes.len() != action.outputs.len() {
            return Err(BuildExecutionError::IO {
                action: action.name.clone(),
                detail: format!(
                    "compiler runner returned {} outputs for {} declarations",
                    bytes.len(),
                    action.outputs.len()
                ),
            });
        }
        for (output, bytes) in action.outputs.iter().zip(bytes) {
            let path =
                resolve_under(project_root, output.as_str()).map_err(|e| io_action(action, e))?;
            prepare_output_destination(project_root, &path).map_err(|e| io_action(action, e))?;
            super::cache_cas::atomic_restore_file(project_root, &path, &bytes)
                .map_err(|e| io_action(action, e))?;
        }
        let outcome = ActionOutcome::Succeeded { exit_code: 0 };
        if action.cache == ActionCache::Cached {
            let record = cas
                .capture_declared_outputs(
                    project_root,
                    action,
                    key.clone(),
                    outcome,
                    ActionCacheProvenance::miss(rebuild_status_reason(rebuild_status)),
                )
                .map_err(|e| io_action(action, e))?;
            write_action_record(&record_path, &record).map_err(|e| io_action(action, e))?;
        }
        write_last_rebuild_record(project_root, action, &key, rebuild_status, None)?;
        return Ok(outcome);
    }

    if let Some((transport, policy, true, _lease)) = &remote {
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
            remote_binding.expect("enabled remote execution requires its host binding"),
        );
        match remote_result {
            Ok(outcome) => return Ok(outcome),
            Err(_error) if remote_binding.is_some_and(|binding| binding.fallback_local) => {}
            Err(error) => return Err(error),
        }
    }

    if remote_binding
        .is_some_and(|binding| binding.cache_read && !binding.execute && !binding.fallback_local)
    {
        return Err(remote_action(
            action,
            "remote cache miss cannot fall back to local execution".to_string(),
        ));
    }

    let sandbox = project_root.join(".jet/build-sandbox").join(format!(
        "{}-{}-{}",
        std::process::id(),
        action.id.0,
        key.as_str().trim_start_matches("act-sha256:")
    ));
    let sandbox_root = sandbox.parent().ok_or_else(|| {
        io_action(
            action,
            io::Error::new(io::ErrorKind::InvalidInput, "sandbox has no parent"),
        )
    })?;
    ensure_real_directory(sandbox_root).map_err(|e| io_action(action, e))?;
    if let Ok(metadata) = fs::symlink_metadata(&sandbox) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(io_action(
                action,
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "action sandbox is not a real directory",
                ),
            ));
        }
        fs::remove_dir_all(&sandbox).map_err(|e| io_action(action, e))?;
    }
    ensure_real_directory(&sandbox).map_err(|e| io_action(action, e))?;
    for input in &action.inputs {
        let from = resolve_under(project_root, input.as_str()).map_err(|e| io_action(action, e))?;
        let to = resolve_under(&sandbox, input.as_str()).map_err(|e| io_action(action, e))?;
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| io_action(action, e))?;
        }
        let bytes = super::cache_cas::secure_read_file(project_root, &from)
            .map_err(|e| io_action(action, e))?;
        fs::write(to, bytes).map_err(|e| io_action(action, e))?;
    }
    for output in &action.outputs {
        let path = resolve_under(&sandbox, output.as_str()).map_err(|e| io_action(action, e))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_action(action, e))?;
        }
    }

    let bwrap = find_program_path("bwrap").ok_or(BuildExecutionError::SandboxUnavailable)?;
    let mut command = Command::new(bwrap);
    command
        .arg("--die-with-parent")
        .arg("--new-session")
        .arg("--unshare-all")
        .arg("--ro-bind")
        .arg("/nix/store")
        .arg("/nix/store")
        .arg("--proc")
        .arg("/proc")
        .arg("--dev")
        .arg("/dev")
        .arg("--tmpfs")
        .arg("/tmp")
        .arg("--bind")
        .arg(&sandbox)
        .arg("/work")
        .arg("--chdir")
        .arg("/work")
        .arg("--clearenv");
    if grants.contains(&BuildCapability::Net) && action.caps.contains(&BuildCapability::Net) {
        command.arg("--share-net");
    }
    command.arg("--setenv").arg("PATH").arg("/nix/store");
    for (key, value) in action.env.iter().filter(|(key, _)| {
        action.env_allowlist.is_empty() || action.env_allowlist.contains(key.as_str())
    }) {
        command.arg("--setenv").arg(key).arg(value);
    }
    command.arg(executable).args(&action.argv[1..]);
    let output = command.output().map_err(|e| io_action(action, e))?;
    let code = output.status.code().unwrap_or(1);
    if !output.status.success() {
        let _ = fs::remove_dir_all(&sandbox);
        write_last_rebuild_record(project_root, action, &key, rebuild_status, Some(code))?;
        return Err(BuildExecutionError::ActionFailed {
            action: action.name.clone(),
            exit_code: code,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    for declared in &action.outputs {
        let from = resolve_under(&sandbox, declared.as_str()).map_err(|e| io_action(action, e))?;
        let to =
            resolve_under(project_root, declared.as_str()).map_err(|e| io_action(action, e))?;
        let bytes = super::cache_cas::secure_read_file(&sandbox, &from)
            .map_err(|e| io_action(action, e))?;
        prepare_output_destination(project_root, &to).map_err(|e| io_action(action, e))?;
        super::cache_cas::atomic_restore_file(project_root, &to, &bytes)
            .map_err(|e| io_action(action, e))?;
    }
    let outcome = ActionOutcome::Succeeded { exit_code: code };
    if action.cache == ActionCache::Cached {
        let record = cas
            .capture_declared_outputs(
                project_root,
                action,
                key.clone(),
                outcome,
                ActionCacheProvenance::miss(CacheMissReason::NoLocalActionRecord),
            )
            .map_err(|e| io_action(action, e))?;
        if let Some((transport, policy, _, _lease)) = &remote {
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
    binding: &RemoteBuildBinding,
) -> Result<ActionOutcome, BuildExecutionError> {
    let mut policy = policy.clone();
    let mut last_failure = None;
    for attempt in 0..MAX_REMOTE_EXECUTION_ATTEMPTS {
        match execute_remote_attempt(
            plan,
            action,
            project_root,
            cas,
            record_path,
            snapshots,
            key,
            transport,
            &policy,
            rebuild_status,
            timeout_ms,
        ) {
            Ok(outcome) => return Ok(outcome),
            Err(failure) if failure.retryable && attempt + 1 < MAX_REMOTE_EXECUTION_ATTEMPTS => {
                last_failure = Some(failure.error);
                policy = remote_attempt_policy(plan, action, key, binding, transport)?;
            }
            Err(failure) => return Err(failure.error),
        }
    }
    Err(last_failure.expect("remote execution attempt loop must produce a result"))
}

fn execute_remote_attempt(
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
) -> Result<ActionOutcome, RemoteAttemptFailure> {
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
            return Err(RemoteAttemptFailure::terminal(remote_action(
                action,
                format!(
                    "remote input CAS identity changed for {}",
                    snapshot.path.as_str()
                ),
            )));
        }
    }
    let request = RemoteExecutionRequest {
        key: key.clone(),
        attempt_id: proof.attempt_id.clone(),
        argv: action.argv.clone(),
        inputs: snapshots.to_vec(),
        outputs: action.outputs.clone(),
        toolchain_digest: toolchain_provenance_digest(plan, action.toolchain),
        sandbox: proof,
    };
    let expected_execution_id = remote_execution_identity(&request);
    transport
        .submit_execution(&request, policy)
        .map_err(|error| remote_action(action, error.to_string()))?;
    let result = wait_remote_execution_result(
        transport,
        policy,
        key,
        action,
        &expected_execution_id,
        timeout_ms,
    )?;
    match result.outcome {
        ActionOutcome::Succeeded { exit_code } => {
            let restored = restore_remote_outputs(
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
                if record.outputs != result.outputs {
                    return Err(RemoteAttemptFailure::terminal(remote_action(
                        action,
                        "remote output changed before local publication".to_string(),
                    )));
                }
                if policy
                    .check(super::cache_cas::RemoteActionRequest::CacheWrite)
                    .is_ok()
                {
                    publish_remote_outputs(transport, policy, project_root, &record)
                        .map_err(|detail| remote_action(action, detail))?;
                }
                write_action_record(record_path, &record).map_err(|e| io_action(action, e))?;
            }
            write_last_rebuild_record(project_root, action, key, rebuild_status, None)
                .map_err(RemoteAttemptFailure::terminal)?;
            restored.commit();
            Ok(ActionOutcome::Succeeded { exit_code })
        }
        ActionOutcome::Failed { exit_code } => {
            write_last_rebuild_record(project_root, action, key, rebuild_status, Some(exit_code))
                .map_err(RemoteAttemptFailure::terminal)?;
            Err(RemoteAttemptFailure::terminal(
                BuildExecutionError::ActionFailed {
                    action: action.name.clone(),
                    exit_code,
                    stderr: "remote execution failed".to_string(),
                },
            ))
        }
        ActionOutcome::RestoredFromCache => Err(RemoteAttemptFailure::terminal(remote_action(
            action,
            "remote execution returned a cache-only outcome".to_string(),
        ))),
    }
}

fn remote_for_action(
    plan: &BuildPlan,
    action: &BuildAction,
    key: &ActionKey,
    grants: &BTreeSet<BuildCapability>,
    binding: Option<&RemoteBuildBinding>,
    scheduler: Option<&RemoteScheduler>,
) -> Result<
    Option<(
        RemoteCacheTransport,
        RemoteCachePolicy,
        bool,
        RemoteBuilderLease,
    )>,
    BuildExecutionError,
> {
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
        || binding
            .trust_domain
            .chars()
            .any(|character| character.is_control())
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
    if !action.caps.contains(&BuildCapability::Net) {
        return Err(BuildExecutionError::MissingGrant {
            action: format!("remote build transport for {}", action.name),
            capability: BuildCapability::Net,
        });
    }
    // The CLI selects one host-owned binding by name, but the selected binding
    // still crosses the canonical capability scheduler. Multi-builder callers
    // use the same model with more candidates; this adapter keeps the driver
    // entry point's explicit single-name contract intact.
    let request = action_pools(action)
        .into_iter()
        .fold(RemoteBuildRequest::new(key.clone()), |request, pool| {
            request.with_pool(pool)
        })
        .with_platform(binding.platform.clone())
        .with_trust_domain(binding.trust_domain.clone())
        .with_cache_read(binding.cache_read)
        .with_cache_write(binding.cache_write)
        .with_execute(binding.execute)
        .with_local_fallback(binding.fallback_local);
    let scheduler = scheduler.ok_or_else(|| {
        remote_action(
            action,
            "remote builder binding has no canonical scheduler".to_string(),
        )
    })?;
    let selected = match scheduler.select(&request) {
        Ok(selected) => selected,
        Err(_error) if binding.fallback_local => return Ok(None),
        Err(error) => return Err(remote_action(action, error.to_string())),
    };
    let policy = remote_attempt_policy(plan, action, key, binding, &transport)?;
    let lease = scheduler.acquire(selected);
    Ok(Some((transport, policy, binding.execute, lease)))
}

fn remote_attempt_policy(
    plan: &BuildPlan,
    action: &BuildAction,
    key: &ActionKey,
    binding: &RemoteBuildBinding,
    transport: &RemoteCacheTransport,
) -> Result<RemoteCachePolicy, BuildExecutionError> {
    let attempt = REMOTE_ATTEMPT.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let attempt_id = format!("attempt-{}-{timestamp}-{attempt}", std::process::id());
    let proof = transport
        .sandbox_proof(
            format!(
                "remote:{}:{}:local-{}-{}-{attempt}",
                binding.builder,
                binding.trust_domain,
                std::process::id(),
                action.id.0,
            ),
            attempt_id,
            key.as_str(),
            remote_provenance_digest(plan, action),
        )
        .map_err(|detail| remote_action(action, detail))?;
    Ok(RemoteCachePolicy::with_grants(
        binding.cache_read,
        binding.cache_write,
        binding.execute,
        proof,
    ))
}

fn wait_remote_execution_result(
    transport: &RemoteCacheTransport,
    policy: &RemoteCachePolicy,
    key: &ActionKey,
    action: &BuildAction,
    expected_execution_id: &ContentDigest,
    timeout_ms: u64,
) -> Result<super::cache_cas::RemoteExecutionResult, RemoteAttemptFailure> {
    let timeout = Duration::from_millis(timeout_ms);
    let deadline = Instant::now() + timeout;
    loop {
        match transport.download_execution_result(key, policy) {
            Ok(result) if result.execution_id != *expected_execution_id => {
                let detail = format!(
                    "remote execution identity mismatch: expected {}, got {}",
                    expected_execution_id.as_str(),
                    result.execution_id.as_str(),
                );
                let detail = match transport.cancel_execution(key, policy) {
                    Ok(()) => detail,
                    Err(cancel_error) => format!("{detail}; cancellation failed: {cancel_error}"),
                };
                return Err(RemoteAttemptFailure::terminal(remote_action(
                    action, detail,
                )));
            }
            Ok(result) => return Ok(result),
            Err(RemoteCacheError::Io(error))
                if error.kind() == io::ErrorKind::NotFound && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(RemoteCacheError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                return match transport.cancel_execution(key, policy) {
                    Ok(()) => Err(RemoteAttemptFailure::retryable(remote_action(
                        action,
                        format!(
                            "remote worker did not publish a result within {}ms",
                            timeout.as_millis()
                        ),
                    ))),
                    Err(cancel_error) => Err(RemoteAttemptFailure::terminal(remote_action(
                        action,
                        format!(
                            "remote worker did not publish a result within {}ms; cancellation failed: {cancel_error}",
                            timeout.as_millis()
                        ),
                    ))),
                };
            }
            Err(error) => {
                let detail = match transport.cancel_execution(key, policy) {
                    Ok(()) => error.to_string(),
                    Err(cancel_error) => {
                        format!("{}; cancellation failed: {cancel_error}", error)
                    }
                };
                return Err(RemoteAttemptFailure::terminal(remote_action(
                    action, detail,
                )));
            }
        }
    }
}

fn restore_remote_outputs<'a>(
    transport: &RemoteCacheTransport,
    policy: &RemoteCachePolicy,
    project_root: &'a Path,
    action: &BuildAction,
    record: &ActionResultRecord,
    blob_request: super::cache_cas::RemoteActionRequest,
) -> Result<RemoteOutputRestore<'a>, String> {
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
            super::cache_cas::RemoteActionRequest::CacheRead => {
                transport.download_blob(&output.digest, policy)
            }
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
        let path =
            resolve_under(project_root, output.path.as_str()).map_err(|error| error.to_string())?;
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
    Ok(RemoteOutputRestore {
        project_root,
        backups,
        committed: false,
    })
}

struct RemoteOutputRestore<'a> {
    project_root: &'a Path,
    backups: Vec<(PathBuf, Option<Vec<u8>>)>,
    committed: bool,
}

impl RemoteOutputRestore<'_> {
    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for RemoteOutputRestore<'_> {
    fn drop(&mut self) {
        if !self.committed {
            rollback_output_restore(self.project_root, &self.backups);
        }
    }
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
        let path =
            resolve_under(project_root, output.path.as_str()).map_err(|error| error.to_string())?;
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

fn rebuild_status_reason(status: ActionCacheStatus) -> CacheMissReason {
    match status {
        ActionCacheStatus::Miss(reason) => reason,
        ActionCacheStatus::Hit(_) => CacheMissReason::NoLocalActionRecord,
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
    let text = String::from_utf8(bytes).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "rebuild explanation is not UTF-8",
        )
    })?;
    let mut lines = text.lines();
    let Some(key) = lines.next() else {
        return Ok(None);
    };
    let action_digest = ContentDigest::from_bytes(action_name.as_bytes());
    if lines.next() != Some(action_digest.as_str()) {
        return Ok(None);
    }
    let Some(status) = lines.next().and_then(parse_rebuild_status) else {
        return Ok(None);
    };
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
    atomic_restore_file(project_root, &path, text.as_bytes()).map_err(|error| {
        BuildExecutionError::IO {
            action: format!("rebuild explanation {}", action.name),
            detail: error.to_string(),
        }
    })
}

pub(super) fn prepare_output_destination(root: &Path, output: &Path) -> io::Result<()> {
    let parent = output.parent().unwrap_or(root);
    let relative = parent.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "build output parent escapes project root",
        )
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if let Component::Normal(part) = component {
            current.push(part);
            match fs::symlink_metadata(&current) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("build output parent `{}` is a symlink", current.display()),
                    ))
                }
                Ok(meta) if !meta.is_dir() => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        format!(
                            "build output parent `{}` is not a directory",
                            current.display()
                        ),
                    ))
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match fs::create_dir(&current) {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                            let metadata = fs::symlink_metadata(&current)?;
                            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                                return Err(io::Error::new(
                                    io::ErrorKind::PermissionDenied,
                                    format!(
                                        "build output parent `{}` is not a real directory",
                                        current.display()
                                    ),
                                ));
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
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
    for probe in plan
        .probes
        .iter()
        .filter(|probe| selected.contains(&probe.id))
    {
        let (success, detail) = match &probe.kind {
            ProbeKind::FindProgram { program } => {
                match resolve_program_path(plan, probe.toolchain, program) {
                    Some(path) => (true, path.display().to_string()),
                    None => (false, format!("program `{program}` not found")),
                }
            }
            ProbeKind::PkgConfig {
                package,
                min_version,
            } => {
                let Some(pkg_config) = resolve_program_path(plan, probe.toolchain, "pkg-config")
                else {
                    return Err(BuildExecutionError::ProbeFailed {
                        probe: probe.name.clone(),
                        detail: "pkg-config not found".to_string(),
                    });
                };
                let mut cmd = Command::new(pkg_config);
                cmd.arg("--exists");
                if let Some(version) = min_version {
                    cmd.arg(format!("{package} >= {version}"));
                } else {
                    cmd.arg(package);
                }
                match cmd.status() {
                    Ok(status) => (
                        status.success(),
                        format!("pkg-config exit {}", status.code().unwrap_or(1)),
                    ),
                    Err(e) => (false, e.to_string()),
                }
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
        if !success {
            return Err(BuildExecutionError::ProbeFailed {
                probe: probe.name.clone(),
                detail,
            });
        }
    }
    Ok(facts)
}

fn run_compile_probe(plan: &BuildPlan, toolchain: ToolchainHandle, source: &str) -> (bool, String) {
    let compiler = ["cc", "clang", "gcc"]
        .into_iter()
        .find_map(|name| resolve_program_path(plan, toolchain, name));
    let Some(compiler) = compiler else {
        return (
            false,
            "no C compiler (`cc`, `clang`, or `gcc`) was found".to_string(),
        );
    };
    let mut child = match Command::new(&compiler)
        .args(["-x", "c", "-fsyntax-only", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return (
                false,
                format!("could not start {}: {error}", compiler.display()),
            )
        }
    };
    let Some(mut stdin) = child.stdin.take() else {
        return (false, "could not open compiler input".to_string());
    };
    if let Err(error) = stdin.write_all(source.as_bytes()) {
        return (
            false,
            format!("could not send source to {}: {error}", compiler.display()),
        );
    }
    drop(stdin);
    match child.wait_with_output() {
        Ok(output) if output.status.success() => (
            true,
            format!("{} accepted the syntax probe", compiler.display()),
        ),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            (
                false,
                if detail.is_empty() {
                    format!("{} rejected the syntax probe", compiler.display())
                } else {
                    detail
                },
            )
        }
        Err(error) => (
            false,
            format!("could not wait for {}: {error}", compiler.display()),
        ),
    }
}

fn find_program_path(program: &str) -> Option<PathBuf> {
    let direct = Path::new(program);
    if direct.components().count() > 1 && direct.is_file() {
        return fs::canonicalize(direct).ok();
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(program);
            candidate
                .is_file()
                .then(|| fs::canonicalize(candidate).ok())
                .flatten()
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
    let basename = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str());
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
    if matches!(
        toolchain.role,
        super::provenance_toolchains::ToolchainRole::Host
    ) {
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
    let macos = [format!("{arch}-macos"), format!("{arch}-apple-darwin")];
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
    path.is_file()
        .then(|| fs::canonicalize(path).ok())
        .flatten()
}

fn toolchain_provenance_digest(plan: &BuildPlan, toolchain: ToolchainHandle) -> ContentDigest {
    let identity = plan
        .toolchain(toolchain)
        .map(|value| format!("{value:?}"))
        .unwrap_or_else(|| "missing-toolchain".to_string());
    ContentDigest::from_bytes(identity.as_bytes())
}

fn remote_provenance_digest(plan: &BuildPlan, action: &BuildAction) -> ContentDigest {
    let toolchain = toolchain_provenance_digest(plan, action.toolchain);
    let policy = remote_policy_digest(action.caps.iter().cloned());
    let statement = format!(
        "jet.remote-provenance.v1\ntoolchain={toolchain}\npolicy={policy}\n",
        toolchain = toolchain.as_str(),
        policy = policy.as_str(),
    );
    ContentDigest::from_bytes(statement.as_bytes())
}

fn write_action_record(path: &Path, record: &ActionResultRecord) -> io::Result<()> {
    let mut text = format!("{}\n", record.key.as_str());
    for output in &record.outputs {
        text.push_str(&format!(
            "{}\t{}\t{}\n",
            output.path.as_str(),
            output.digest.as_str(),
            output.byte_len
        ));
    }
    let root = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "action record has no cache directory",
        )
    })?;
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
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "action record key does not match its cache path",
        ));
    }
    let mut outputs = Vec::new();
    for line in lines {
        let mut parts = line.split('\t');
        let invalid =
            || io::Error::new(io::ErrorKind::InvalidData, "malformed action output record");
        let path = BuildPath::new(parts.next().ok_or_else(invalid)?).map_err(|_| invalid())?;
        let digest = ContentDigest::parse(parts.next().ok_or_else(invalid)?)?;
        let byte_len = parts
            .next()
            .ok_or_else(invalid)?
            .parse()
            .map_err(|_| invalid())?;
        if parts.next().is_some() {
            return Err(invalid());
        }
        outputs.push(ActionOutputRecord {
            path,
            digest,
            byte_len,
        });
    }
    Ok(Some(ActionResultRecord {
        key,
        outcome: ActionOutcome::RestoredFromCache,
        outputs,
        provenance: ActionCacheProvenance::hit(CacheHitReason::LocalActionRecordMatched),
    }))
}

fn io_action(action: &BuildAction, error: io::Error) -> BuildExecutionError {
    BuildExecutionError::IO {
        action: action.name.clone(),
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_output_restore_rolls_back_until_committed() {
        let root = std::env::temp_dir().join(format!(
            "jet-remote-restore-guard-{}-{}",
            std::process::id(),
            REMOTE_ATTEMPT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("build/out");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"old").unwrap();

        fs::write(&path, b"new").unwrap();
        {
            let _restore = RemoteOutputRestore {
                project_root: &root,
                backups: vec![(path.clone(), Some(b"old".to_vec()))],
                committed: false,
            };
        }
        assert_eq!(fs::read(&path).unwrap(), b"old");

        fs::write(&path, b"new").unwrap();
        let restore = RemoteOutputRestore {
            project_root: &root,
            backups: vec![(path.clone(), Some(b"old".to_vec()))],
            committed: false,
        };
        restore.commit();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn remote_ineligible_builder_honors_declared_local_fallback() {
        use crate::Comptime::Build::{ActionSpec, BuildContext, BuildResourcePool, TargetSpec};

        let root = std::env::temp_dir().join(format!(
            "jet-remote-ineligible-fallback-{}",
            std::process::id()
        ));
        let mut context = BuildContext::new();
        let action = context
            .action(
                "gpu-action",
                ActionSpec::cached(["remote-tool"])
                    .with_cap(BuildCapability::Net)
                    .with_pool(BuildResourcePool::GPU),
            )
            .unwrap();
        let target = context
            .add_executable("gpu-target", TargetSpec::new().with_action(action))
            .unwrap();
        let plan = context.plan_with_default(target).unwrap();
        let binding = RemoteBuildBinding::new("cpu-builder", &root, b"fallback-key")
            .unwrap()
            .with_trust_domain("trusted")
            .with_execute(true)
            .with_local_fallback(true);
        let scheduler =
            RemoteScheduler::new([RemoteBuilder::from_binding(binding.clone())]).unwrap();
        let grants = [BuildCapability::Net].into_iter().collect();

        let result = remote_for_action(
            &plan,
            plan.action(action).unwrap(),
            &ActionKey::new("gpu-action-key"),
            &grants,
            Some(&binding),
            Some(&scheduler),
        )
        .unwrap();

        assert!(result.is_none());
        let _ = fs::remove_dir_all(root);
    }
}
