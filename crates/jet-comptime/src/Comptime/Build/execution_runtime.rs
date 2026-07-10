use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProbeFact {
    pub name: String,
    pub success: bool,
    pub detail: String,
    pub reproducibility: ReproducibilityClass,
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
    Io { action: String, detail: String },
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
    fs::create_dir_all(&records).map_err(|e| BuildExecutionError::Io {
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
                            execute_one_action(plan, action, handle, project_root, cas, records, grants, probe_facts)
                        }))
                    })
                    .collect::<Vec<_>>();
                jobs.into_iter()
                    .map(|(handle, job)| {
                        let result = job.join().map_err(|_| BuildExecutionError::Io {
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
    let mut held: Vec<BTreeSet<String>> = Vec::new();
    for action_id in actions {
        let exclusive = action_pools(&plan.actions[action_id.0])
            .into_iter()
            .filter(|pool| !matches!(pool, BuildResourcePool::Cpu | BuildResourcePool::Memory))
            .map(|pool| pool.as_str().to_string())
            .collect::<BTreeSet<_>>();
        let slot = held
            .iter()
            .position(|used| used.is_disjoint(&exclusive))
            .unwrap_or_else(|| {
                batches.push(Vec::new());
                held.push(BTreeSet::new());
                batches.len() - 1
            });
        held[slot].extend(exclusive);
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
) -> Result<ActionOutcome, BuildExecutionError> {
    let snapshots = cas.snapshot_declared_inputs(project_root, action).map_err(|e| io_action(action, e))?;
    let executable = find_program_path(&action.argv[0]).ok_or_else(|| BuildExecutionError::Io {
        action: action.name.clone(), detail: format!("tool `{}` was not found", action.argv[0])
    })?;
    let executable_bytes = fs::read(&executable).map_err(|e| io_action(action, e))?;
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
    if action.cache == ActionCache::Cached {
        if let Some(record) = read_action_record(records, &record_path, key.clone()) {
            if cas.restore_action_outputs(project_root, action, &record).is_ok() {
                return Ok(ActionOutcome::RestoredFromCache);
            }
        }
    }

    let sandbox = project_root.join(".jet/build-sandbox").join(format!(
        "{}-{}-{}", std::process::id(), action.id.0, key.as_str().trim_start_matches("act-sha256:")
    ));
    if sandbox.exists() { fs::remove_dir_all(&sandbox).map_err(|e| io_action(action, e))?; }
    fs::create_dir_all(&sandbox).map_err(|e| io_action(action, e))?;
    for input in &action.inputs {
        let from = resolve_under(project_root, input.as_str()).map_err(|e| io_action(action, e))?;
        let to = resolve_under(&sandbox, input.as_str()).map_err(|e| io_action(action, e))?;
        if let Some(parent) = to.parent() { fs::create_dir_all(parent).map_err(|e| io_action(action, e))?; }
        fs::copy(from, to).map_err(|e| io_action(action, e))?;
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
    for (key, value) in &action.env {
        command.arg("--setenv").arg(key).arg(value);
    }
    command.arg(executable).args(&action.argv[1..]);
    let output = command.output().map_err(|e| io_action(action, e))?;
    let code = output.status.code().unwrap_or(1);
    if !output.status.success() {
        let _ = fs::remove_dir_all(&sandbox);
        return Err(BuildExecutionError::ActionFailed {
            action: action.name.clone(), exit_code: code,
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    for declared in &action.outputs {
        let from = resolve_under(&sandbox, declared.as_str()).map_err(|e| io_action(action, e))?;
        let to = resolve_under(project_root, declared.as_str()).map_err(|e| io_action(action, e))?;
        prepare_output_destination(project_root, &to).map_err(|e| io_action(action, e))?;
        fs::copy(from, to).map_err(|e| io_action(action, e))?;
    }
    let outcome = ActionOutcome::Succeeded { exit_code: code };
    if action.cache == ActionCache::Cached {
        let record = cas.capture_declared_outputs(
            project_root, action, key, outcome,
            ActionCacheProvenance::miss(CacheMissReason::NoLocalActionRecord),
        ).map_err(|e| io_action(action, e))?;
        write_action_record(&record_path, &record).map_err(|e| io_action(action, e))?;
    }
    fs::remove_dir_all(&sandbox).map_err(|e| io_action(action, e))?;
    Ok(outcome)
}

fn prepare_output_destination(root: &Path, output: &Path) -> io::Result<()> {
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
            ProbeKind::FindProgram { program } => match find_program_path(program) {
                Some(path) => (true, path.display().to_string()),
                None => (false, format!("program `{program}` not found")),
            },
            ProbeKind::PkgConfig { package, min_version } => {
                let Some(pkg_config) = find_program_path("pkg-config") else {
                    return Err(BuildExecutionError::ProbeFailed { probe: probe.name.clone(), detail: "pkg-config not found".to_string() });
                };
                let mut cmd = Command::new(pkg_config);
                cmd.arg("--exists");
                if let Some(version) = min_version { cmd.arg(format!("{package} >= {version}")); } else { cmd.arg(package); }
                match cmd.status() { Ok(status) => (status.success(), format!("pkg-config exit {}", status.code().unwrap_or(1))), Err(e) => (false, e.to_string()) }
            }
            ProbeKind::HeaderCheck { header } => (false, format!("header check `{header}` needs an explicit compile-check toolchain")),
            ProbeKind::CompileCheck { name, .. } => (false, format!("compile check `{name}` needs an explicit compiler action")),
        };
        facts.push(BuildProbeFact { name: probe.name.clone(), success, detail: detail.clone(), reproducibility: probe.reproducibility });
        if !success { return Err(BuildExecutionError::ProbeFailed { probe: probe.name.clone(), detail }); }
    }
    Ok(facts)
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

fn write_action_record(path: &Path, record: &ActionResultRecord) -> io::Result<()> {
    let mut text = format!("{}\n", record.key.as_str());
    for output in &record.outputs {
        text.push_str(&format!("{}\t{}\t{}\n", output.path.as_str(), output.digest.as_str(), output.byte_len));
    }
    let root = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "action record has no cache directory"))?;
    atomic_restore_file(root, path, text.as_bytes())
}

fn read_action_record(root: &Path, path: &Path, key: ActionKey) -> Option<ActionResultRecord> {
    let text = String::from_utf8(secure_read_file(root, path).ok()?).ok()?;
    let mut lines = text.lines();
    if lines.next()? != key.as_str() { return None; }
    let mut outputs = Vec::new();
    for line in lines {
        let mut parts = line.split('\t');
        let path = BuildPath::new(parts.next()?).ok()?;
        let digest = ContentDigest::parse(parts.next()?).ok()?;
        let byte_len = parts.next()?.parse().ok()?;
        outputs.push(ActionOutputRecord { path, digest, byte_len });
    }
    Some(ActionResultRecord {
        key, outcome: ActionOutcome::RestoredFromCache, outputs,
        provenance: ActionCacheProvenance::hit(CacheHitReason::LocalActionRecordMatched),
    })
}

fn io_action(action: &BuildAction, error: io::Error) -> BuildExecutionError {
    BuildExecutionError::Io { action: action.name.clone(), detail: error.to_string() }
}
