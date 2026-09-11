//! D-JREPLAY1=A / D-PROVE-REPLAY1=A: `.jetproof-replay` capture and `--replay`.
//!
//! Safe capture records Time plus optional causal act snapshots. Sensitive roots require `--capture-sensitive`
//! (consent path). Artifacts use the ratified binary envelope (magic `JREPLAY\0`,
//! canonical JSON header, framed records, `JEND` footer).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};

use jet::ExitCodes;
use jet::RecordIndex::RecordIdentity;
use jet::SHA256;
use jet_foundation::DataTree::DataTree;
use jet_foundation::JSON::parse_json;

const MAGIC: &[u8; 8] = b"JREPLAY\0";
const KIND_TIME_WALL: u16 = 0x0001;
const KIND_RUN_ACT: u16 = 0x1001;
const MAX_REPLAY_BYTES: u64 = 256 * 1024 * 1024;
const MAX_EXACT_JSON_INTEGER: u64 = (1 << 53) - 1;

#[derive(Clone, Debug)]
pub(crate) struct CaptureOpts {
    pub path: Option<String>,
    pub sensitive: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ReplayIdentity {
    pub entry: String,
    pub source_digest: String,
    pub execution_adapter: String,
    pub target_triple: String,
    pub abi: String,
    pub build_digest: String,
    pub core_abi: String,
    pub lock_digest: String,
    pub profile: String,
    pub semantic_mir_hash: String,
    pub optimized_mir_hash: String,
    pub mir_schema: String,
    pub mir_identity: String,
    /// Stable identity of the one statically authorized `core.time.now` call.
    /// The zero placeholder is no longer accepted in a replay frame.
    pub time_site_id: String,
}

impl ReplayIdentity {
    /// Every indexed artifact uses the target's resolved input closure as its
    /// identity component.  Replay header fields remain the execution
    /// adapter's semantic identity and are checked separately.
    pub(crate) fn record_identity(&self) -> Result<RecordIdentity, String> {
        let target_inputs_sha256 = crate::CmdProve::target_input_sha256_for_file(&self.entry)?;
        RecordIdentity::new(
            target_inputs_sha256,
            env!("CARGO_PKG_VERSION"),
            self.execution_adapter.clone(),
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CaptureAuthority {
    unix_ns: u64,
    explicit_path: Option<PathBuf>,
}

impl CaptureAuthority {
    pub(crate) fn time_ms(&self) -> i64 {
        i64::try_from(self.unix_ns / 1_000_000).unwrap_or(i64::MAX)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ReplayAuthority {
    pub time_ms: i64,
    pub expected_outcome: String,
    pub expected_status: i32,
    time_values: Vec<i64>,
    next_time: usize,
    pub recorded_run: jet::Debug::RecordedRun,
    pub decision_ledger: Option<jet_foundation::MIR::MirDecisionLedger>,
}

impl ReplayAuthority {
    /// Install one recorded Time value at a time. The proof command has one
    /// bounded Time adapter today; keeping a cursor makes extra, reordered, or
    /// silently ignored records a divergence instead of harmless metadata.
    pub(crate) fn consume_time(&mut self) -> Result<i64, String> {
        let value = self
            .time_values
            .get(self.next_time)
            .copied()
            .ok_or_else(|| "replay requested Time after the artifact was exhausted".to_string())?;
        self.next_time = self
            .next_time
            .checked_add(1)
            .ok_or_else(|| "replay Time record cursor overflowed".to_string())?;
        self.time_ms = value;
        Ok(value)
    }

    pub(crate) fn finish(&self) -> Result<(), String> {
        if self.next_time == self.time_values.len() {
            Ok(())
        } else {
            Err(format!(
                "replay stopped with {} unconsumed Time record(s)",
                self.time_values.len() - self.next_time
            ))
        }
    }
}

pub(crate) fn parse_capture_flag(arg: &str) -> Option<CaptureOpts> {
    if arg == "--capture" {
        return Some(CaptureOpts {
            path: None,
            sensitive: false,
        });
    }
    if let Some(path) = arg.strip_prefix("--capture=") {
        return Some(CaptureOpts {
            path: Some(path.to_string()),
            sensitive: false,
        });
    }
    if arg == "--capture-sensitive" {
        return Some(CaptureOpts {
            path: None,
            sensitive: true,
        });
    }
    if let Some(path) = arg.strip_prefix("--capture-sensitive=") {
        return Some(CaptureOpts {
            path: Some(path.to_string()),
            sensitive: true,
        });
    }
    None
}

pub(crate) fn parse_replay_flag(arg: &str, next: Option<&str>) -> Option<Result<String, String>> {
    if let Some(path) = arg.strip_prefix("--replay=") {
        return Some(Ok(path.to_string()));
    }
    if arg == "--replay" {
        return match next {
            Some(path) if !path.starts_with('-') => Some(Ok(path.to_string())),
            _ => Some(Err(
                "`--replay` needs a `.jetproof-replay` artifact path".to_string()
            )),
        };
    }
    None
}

/// D-RUN-RECORD1=A: run/dev/test use a closed `--record=NAME` grammar. The
/// name is deliberately not a path; the replay directory is the one artifact
/// home and the extension is owned by the envelope.
pub(crate) fn parse_record_flag(arg: &str) -> Option<Result<String, String>> {
    let name = arg.strip_prefix("--record=")?;
    Some(match replay_path_for_name(name) {
        Ok(_) => Ok(name.to_string()),
        Err(message) => Err(message),
    })
}

/// D-RUN-RECORD1=A: debug consumes the same closed spelling. A bare name
/// addresses the standard replay directory; an explicit `.jetproof-replay`
/// path keeps the existing prove artifact reader useful for handoff scripts.
pub(crate) fn parse_closed_replay_flag(arg: &str) -> Option<Result<String, String>> {
    let value = arg.strip_prefix("--replay=")?;
    if value.ends_with(".jetproof-replay") || value.contains('/') {
        return Some(Ok(value.to_string()));
    }
    Some(replay_path_for_name(value))
}

fn replay_path_for_name(name: &str) -> Result<String, String> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(
            "replay NAME must be non-empty and contain only ASCII letters, digits, `-`, or `_`"
                .to_string(),
        );
    }
    Ok(format!(".jet/replays/{name}.jetproof-replay"))
}

#[derive(Clone, Debug)]
pub(crate) struct NamedCapture {
    identity: ReplayIdentity,
    authority: CaptureAuthority,
}

impl NamedCapture {
    pub(crate) fn record_identity(&self) -> Result<RecordIdentity, String> {
        self.identity.record_identity()
    }
}

/// Start the ordinary safe Time-only capture used by run/dev/test.
pub(crate) fn begin_named_capture(
    file: &str,
    name: &str,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    json_mode: bool,
) -> Result<NamedCapture, i32> {
    let path = match replay_path_for_name(name) {
        Ok(path) => path,
        Err(message) => {
            emit_diag(
                "E2104",
                "invalid replay name",
                &message,
                "write `--record=NAME` with letters, digits, `-`, or `_`",
                json_mode,
            );
            return Err(ExitCodes::USAGE);
        }
    };
    let identity = match identity_for_file(file, profile, setting_overrides) {
        Ok(identity) => identity,
        Err(message) => {
            emit_diag(
                "E3629",
                "replay artifact could not be prepared",
                &message,
                "record a readable project-relative `.jet` file",
                json_mode,
            );
            return Err(ExitCodes::USER_ERROR);
        }
    };
    let authority = match prepare_safe_capture(
        &CaptureOpts {
            path: Some(path),
            sensitive: false,
        },
        json_mode,
    ) {
        Ok(authority) => authority,
        Err(status) => return Err(status),
    };
    Ok(NamedCapture {
        identity,
        authority,
    })
}

pub(crate) fn finish_named_capture(
    capture: &NamedCapture,
    exit_code: i32,
    json_mode: bool,
) -> Result<(), i32> {
    finalize_safe_capture(
        &capture.identity,
        &capture.authority,
        exit_code,
        json_mode,
        None,
    )
}

pub(crate) fn finish_named_capture_with_run(
    capture: &NamedCapture,
    exit_code: i32,
    json_mode: bool,
    run: &jet::Debug::RecordedRun,
) -> Result<(), i32> {
    finalize_safe_capture(
        &capture.identity,
        &capture.authority,
        exit_code,
        json_mode,
        Some(run),
    )
}
/// Index a finalized named replay through the canonical CmdProve producer.
/// Devtools callers reuse the identity established before execution.
pub(crate) fn index_named_replay_artifact(
    capture: &NamedCapture,
    path: &Path,
    capture_mode: jet::RecordIndex::RecordCapture,
) -> Result<jet::RecordIndex::RecordLink, String> {
    crate::CmdProve::index_replay_artifact(path, &capture.identity, capture_mode)
}

/// Open a named artifact for `jet debug`, install the shared Time adapter, and
/// consume its bounded record before the debugger starts.
pub(crate) fn open_named_replay(
    file: &str,
    value: &str,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
    json_mode: bool,
) -> Result<ReplayAuthority, i32> {
    let path = if value.ends_with(".jetproof-replay") || value.contains('/') {
        value.to_string()
    } else {
        match replay_path_for_name(value) {
            Ok(path) => path,
            Err(message) => {
                emit_diag(
                    "E2104",
                    "invalid replay name",
                    &message,
                    "write `--replay=NAME` with a valid replay name or artifact path",
                    json_mode,
                );
                return Err(ExitCodes::USAGE);
            }
        }
    };
    let identity = match identity_for_file(file, profile, setting_overrides) {
        Ok(identity) => identity,
        Err(message) => {
            emit_diag(
                "E3621",
                "replay semantic identity could not be established",
                &message,
                "replay the matching readable project file",
                json_mode,
            );
            return Err(ExitCodes::USER_ERROR);
        }
    };
    let mut authority = match prepare_replay(&identity, &path) {
        Ok(authority) => authority,
        Err((code, why)) => {
            let (what, fix) = match code {
                "E3621" => (
                    "replay semantic identity does not match",
                    "replay the exact source identity that produced the artifact",
                ),
                "E3628" => (
                    "replay capture exceeded its artifact limit",
                    "recapture a bounded artifact with a recorded Time authority",
                ),
                _ => (
                    "replay artifact is corrupt or unavailable",
                    "recapture the target and retry",
                ),
            };
            emit_diag(code, what, &why, fix, json_mode);
            return Err(ExitCodes::USER_ERROR);
        }
    };
    let time_ms = match authority.consume_time() {
        Ok(value) => value,
        Err(why) => {
            emit_diag(
                "E3623",
                "replay diverged from captured authority",
                &why,
                "recapture the target, then replay the matching artifact",
                json_mode,
            );
            return Err(ExitCodes::USER_ERROR);
        }
    };
    std::env::set_var("JET_PROVE_REPLAY_TIME_MS", time_ms.to_string());
    if !json_mode {
        eprintln!("ambient authority opened: Time; mir-v1");
    }
    Ok(authority)
}

fn identity_for_file(
    file: &str,
    profile: &str,
    setting_overrides: &BTreeMap<String, String>,
) -> Result<ReplayIdentity, String> {
    let source = fs::read(file).map_err(|error| format!("could not read `{file}`: {error}"))?;
    let entry = project_relative_entry(file)?;
    let source_digest = SHA256::sha256_hex(&source);
    let build_digest = SHA256::sha256_hex(format!("jet-build:{source_digest}").as_bytes());
    let lock_digest = SHA256::sha256_hex(b"jet-record-lock-v1");
    let mir_identity =
        crate::CmdProve::proof_mir_identity_for_file(file, profile, setting_overrides)?;
    let semantic_mir_hash = mir_identity.semantic_hash.clone();
    let optimized_mir_hash = mir_identity.optimized_hash.clone();
    let time_site_id = SHA256::sha256_hex(format!("jet-time:{entry}").as_bytes());
    Ok(ReplayIdentity {
        entry,
        source_digest,
        execution_adapter: "mir-v1".to_string(),
        target_triple: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        abi: std::env::consts::FAMILY.to_string(),
        build_digest,
        core_abi: "jet-core-abi-v1".to_string(),
        lock_digest,
        profile: profile.to_string(),
        semantic_mir_hash,
        optimized_mir_hash,
        mir_schema: jet_foundation::MIR::MIR_IDENTITY_SCHEMA.to_string(),
        mir_identity: mir_identity.canonical_json(),
        time_site_id,
    })
}

fn project_relative_entry(file: &str) -> Result<String, String> {
    let path = Path::new(file);
    let cwd = std::env::current_dir()
        .map_err(|error| format!("could not resolve the project root: {error}"))?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let relative = absolute
        .strip_prefix(&cwd)
        .map_err(|_| "entry identity must be inside the current project".to_string())?;
    let value = relative.to_string_lossy().replace('\\', "/");
    if value.is_empty()
        || value.starts_with('/')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("entry identity must be a clean project-relative path".to_string());
    }
    Ok(value)
}

pub(crate) fn prepare_safe_capture(
    opts: &CaptureOpts,
    json_mode: bool,
) -> Result<CaptureAuthority, i32> {
    if opts.sensitive {
        // Non-interactive expert path refuses until a TTY consent flow lands.
        emit_diag(
            "E3627",
            "replay capture refused sensitive data",
            "non-interactive `--capture-sensitive` cannot collect TTY consent for raw Rand/IO/Net values",
            "run from a TTY and type `capture sensitive`, or use safe `--capture` for Time-only roots",
            json_mode,
        );
        return Err(ExitCodes::USER_ERROR);
    }
    if !json_mode {
        eprintln!(
            "capture preflight: safe Time only; normal producer will run under this authority"
        );
    }
    let unix_ns = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => match u64::try_from(duration.as_nanos()) {
            Ok(value) => value,
            Err(_) => {
                emit_diag(
                    "E3629",
                    "replay artifact could not be finalized",
                    "wall-clock nanoseconds exceed the replay integer range",
                    "retry capture after the system clock is representable",
                    json_mode,
                );
                return Err(ExitCodes::USER_ERROR);
            }
        },
        Err(error) => {
            emit_diag(
                "E3629",
                "replay artifact could not be finalized",
                &format!("system clock is before Unix epoch: {error}"),
                "fix the system clock and retry capture",
                json_mode,
            );
            return Err(ExitCodes::USER_ERROR);
        }
    };
    let explicit_path = match opts.path.as_deref() {
        Some(path) => match validate_capture_path(path) {
            Ok(path) => {
                if fs::symlink_metadata(&path).is_ok() {
                    emit_diag(
                        "E3629",
                        "replay artifact could not be finalized",
                        "an explicit capture path must not already exist",
                        "choose a new `.jetproof-replay` path",
                        json_mode,
                    );
                    return Err(ExitCodes::USER_ERROR);
                }
                Some(path)
            }
            Err(message) => {
                emit_diag(
                    "E3629",
                    "replay artifact could not be finalized",
                    &message,
                    "choose a project-relative nonexistent `.jetproof-replay` path",
                    json_mode,
                );
                return Err(ExitCodes::USER_ERROR);
            }
        },
        None => None,
    };
    let time_ms = unix_ns / 1_000_000;
    let Ok(time_ms) = i64::try_from(time_ms) else {
        emit_diag(
            "E3629",
            "replay artifact could not be finalized",
            "captured Time exceeds the signed millisecond range",
            "retry capture after the system clock is representable",
            json_mode,
        );
        return Err(ExitCodes::USER_ERROR);
    };
    // CmdProve finalizes the artifact after all ordinary producers finish.
    // The environment variable is the narrow authority adapter consumed by
    // every execution tier during that producer run.
    std::env::set_var("JET_PROVE_REPLAY_TIME_MS", time_ms.to_string());
    if !json_mode {
        eprintln!("capture: Time authority prepared");
    }
    Ok(CaptureAuthority {
        unix_ns,
        explicit_path,
    })
}

pub(crate) fn finalize_safe_capture(
    identity: &ReplayIdentity,
    authority: &CaptureAuthority,
    exit_code: i32,
    json_mode: bool,
    recorded_run: Option<&jet::Debug::RecordedRun>,
) -> Result<(), i32> {
    if let Err(message) = validate_identity_for_capture(identity) {
        emit_diag(
            "E3629",
            "replay artifact could not be finalized",
            &message,
            "retry capture with a project-relative, supported target identity",
            json_mode,
        );
        return Err(ExitCodes::USER_ERROR);
    }
    let outcome = if exit_code == ExitCodes::RUNTIME_PANIC {
        "panic"
    } else {
        "exit"
    };
    let status = i64::from(exit_code);
    let bytes = match build_safe_time_artifact_with_run(
        identity,
        authority.unix_ns,
        outcome,
        status,
        recorded_run,
    ) {
        Ok(bytes) => bytes,
        Err(message) => {
            emit_diag(
                "E3629",
                "replay artifact could not be finalized",
                &message,
                "fix the destination path and retry capture",
                json_mode,
            );
            return Err(ExitCodes::USER_ERROR);
        }
    };
    let dest = match &authority.explicit_path {
        Some(path) => path.clone(),
        None => match resolve_capture_path(identity, None, &bytes) {
            Ok(path) => path,
            Err(message) => {
                emit_diag(
                    "E3629",
                    "replay artifact could not be finalized",
                    &message,
                    "fix the destination path and retry capture",
                    json_mode,
                );
                return Err(ExitCodes::USER_ERROR);
            }
        },
    };
    if let Err(message) = finalize_artifact(&dest, &bytes, json_mode) {
        emit_diag(
            "E3629",
            "replay artifact could not be finalized",
            &message,
            "fix the destination path and retry capture",
            json_mode,
        );
        return Err(ExitCodes::USER_ERROR);
    }
    let rel = dest.display().to_string();
    if !json_mode {
        eprintln!("capture: finalized outcome={outcome} status={status}");
        eprintln!("artifact: {rel}");
    }
    Ok(())
}

/// Check artifact presence without opening its contents. Missing artifacts
/// must still let target authority preflight report E3623 first.
pub(crate) fn replay_artifact_is_present(artifact_path: &str) -> bool {
    let Ok(path) = validate_replay_path(artifact_path) else {
        return false;
    };
    if ensure_read_parent(&path).is_err() {
        return false;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
        Err(_) => false,
    }
}

/// Validate an artifact and install its captured authorities for the normal
/// proof producer. Replay is execution of the same producer under this
/// adapter, not a successful early return that merely checks a file.
pub(crate) fn prepare_replay(
    identity: &ReplayIdentity,
    artifact_path: &str,
) -> Result<ReplayAuthority, (&'static str, String)> {
    let path = validate_replay_path(artifact_path).map_err(|message| ("E3622", message))?;
    ensure_read_parent(&path).map_err(|message| ("E3622", message))?;
    let metadata = fs::symlink_metadata(&path).map_err(|error| {
        (
            "E3622",
            format!("could not inspect `{artifact_path}`: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err((
            "E3622",
            format!("replay artifact is not a regular file: {artifact_path}"),
        ));
    }
    if metadata.len() > MAX_REPLAY_BYTES {
        return Err((
            "E3622",
            format!("replay artifact exceeds the {MAX_REPLAY_BYTES}-byte limit"),
        ));
    }
    let bytes = fs::read(path).map_err(|error| {
        (
            "E3622",
            format!("could not read `{artifact_path}`: {error}"),
        )
    })?;
    prepare_replay_bytes(identity, artifact_path, &bytes)
}

/// Open an indexed saved replay. Saved records intentionally have no
/// `.jetproof-replay` suffix; the closed path shape prevents this loader from
/// becoming an alternate ordinary replay-path parser.
pub(crate) fn prepare_saved_replay(
    identity: &ReplayIdentity,
    artifact_path: &Path,
) -> Result<ReplayAuthority, (&'static str, String)> {
    let path = validate_saved_replay_path(artifact_path).map_err(|message| ("E3622", message))?;
    ensure_read_parent(&path).map_err(|message| ("E3622", message))?;
    let display = path.display().to_string();
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| ("E3622", format!("could not inspect `{display}`: {error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err((
            "E3622",
            format!("saved replay is not a regular file: {display}"),
        ));
    }
    if metadata.len() > MAX_REPLAY_BYTES {
        return Err((
            "E3622",
            format!("saved replay exceeds the {MAX_REPLAY_BYTES}-byte limit"),
        ));
    }
    let bytes = fs::read(&path).map_err(|error| {
        (
            "E3622",
            format!("could not read saved replay `{display}`: {error}"),
        )
    })?;
    prepare_replay_bytes(identity, &display, &bytes)
}

fn prepare_replay_bytes(
    identity: &ReplayIdentity,
    artifact_path: &str,
    bytes: &[u8],
) -> Result<ReplayAuthority, (&'static str, String)> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REPLAY_BYTES {
        return Err((
            "E3622",
            format!("replay artifact exceeds the {MAX_REPLAY_BYTES}-byte limit"),
        ));
    }
    let header = parse_and_verify(bytes)?;
    identity_matches(&header, identity).map_err(|field| {
        (
            "E3621",
            format!("identity field `{field}` differs from the current target"),
        )
    })?;
    let time_values = extract_time_ms(bytes).map_err(|why| ("E3628", why))?;
    let recorded_run = extract_recorded_run(bytes).map_err(|why| ("E3622", why))?;
    let decision_ledger = recorded_run
        .decision_ledger
        .as_deref()
        .map(jet_foundation::MIR::MirDecisionLedger::from_json)
        .transpose()
        .map_err(|why| ("E3622", why))?;
    let time_ms = time_values.first().copied().ok_or((
        "E3628",
        "replay artifact contains no Time authority".to_string(),
    ))?;
    let expected_outcome = header
        .get("run_outcome")
        .cloned()
        .ok_or(("E3622", "replay header is missing run outcome".to_string()))?;
    let expected_status = header
        .get("run_status")
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|status| (0..=255).contains(status))
        .ok_or((
            "E3622",
            format!("replay header has an invalid run status for `{artifact_path}`"),
        ))?;
    Ok(ReplayAuthority {
        time_ms,
        expected_outcome,
        expected_status,
        time_values,
        next_time: 0,
        recorded_run,
        decision_ledger,
    })
}
fn resolve_capture_path(
    identity: &ReplayIdentity,
    explicit: Option<&str>,
    bytes: &[u8],
) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        if path.is_empty() {
            return Err("explicit capture path is empty".into());
        }
        return validate_capture_path(path);
    }
    let id = artifact_id_from_bytes(bytes)?;
    let stem = sanitize_entry(&identity.entry);
    let short_id = &id[..12.min(id.len())];
    Ok(PathBuf::from(format!(
        ".jet/replays/{stem}-{short_id}.jetproof-replay"
    )))
}

fn validate_capture_path(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("explicit capture path is empty".into());
    }
    if path.contains('\0') {
        return Err("capture path contains NUL".into());
    }
    if path.contains('\\') || path.split('/').any(|part| part.is_empty()) {
        return Err("capture path must use non-empty forward-slash components".into());
    }
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err("capture path must stay project-relative".into());
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("jetproof-replay") {
        return Err("capture path must end in `.jetproof-replay`".into());
    }
    Ok(path.to_path_buf())
}

fn validate_identity_for_capture(identity: &ReplayIdentity) -> Result<(), String> {
    let entry = Path::new(&identity.entry);
    if identity.entry.is_empty()
        || entry.is_absolute()
        || entry.components().any(|component| {
            matches!(
                component,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err("entry identity must be a project-relative path".into());
    }
    if identity.execution_adapter != "mir-v1" {
        return Err(format!(
            "unsupported execution adapter `{}`",
            identity.execution_adapter
        ));
    }
    for (name, value) in [
        ("source_digest", identity.source_digest.as_str()),
        ("build_digest", identity.build_digest.as_str()),
        ("lock_digest", identity.lock_digest.as_str()),
        ("semantic_mir_hash", identity.semantic_mir_hash.as_str()),
        ("optimized_mir_hash", identity.optimized_mir_hash.as_str()),
        ("time_site_id", identity.time_site_id.as_str()),
    ] {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(format!(
                "identity field `{name}` is not a lowercase SHA-256 digest"
            ));
        }
    }
    if identity.abi.is_empty()
        || identity.core_abi.is_empty()
        || identity.profile.is_empty()
        || identity.target_triple.is_empty()
        || identity.mir_schema.is_empty()
    {
        return Err("identity contains an empty required field".into());
    }
    validate_mir_identity(&identity.mir_identity)?;
    if identity.mir_schema != jet_foundation::MIR::MIR_IDENTITY_SCHEMA {
        return Err("identity uses an unsupported MIR schema".into());
    }
    Ok(())
}

fn validate_replay_path(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("replay artifact path is empty".into());
    }
    if path.contains('\0') || path.contains('\\') {
        return Err("replay artifact path must use forward-slash components".into());
    }
    let mut components = if let Some(relative) = path.strip_prefix('/') {
        relative.split('/')
    } else {
        path.split('/')
    };
    if components.any(|component| component.is_empty()) {
        return Err("replay artifact path must use non-empty components".into());
    }
    let path = Path::new(path);
    if path.components().any(|component| {
        matches!(
            component,
            Component::CurDir | Component::ParentDir | Component::Prefix(_)
        )
    }) {
        return Err("replay artifact path contains an unsafe component".into());
    }
    if path.extension().and_then(|extension| extension.to_str()) != Some("jetproof-replay") {
        return Err("replay artifact path must end in `.jetproof-replay`".into());
    }
    Ok(path.to_path_buf())
}
fn validate_saved_replay_path(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("saved replay path is empty".into());
    }
    if path.is_absolute() {
        return Err("saved replay path must stay project-relative".into());
    }
    let components = path.components().collect::<Vec<_>>();
    if components.len() != 4
        || components[0] != Component::Normal(std::ffi::OsStr::new(".jet"))
        || components[1] != Component::Normal(std::ffi::OsStr::new("records"))
        || components[2] != Component::Normal(std::ffi::OsStr::new("saved"))
    {
        return Err("saved replay path must be `.jet/records/saved/<artifact_id>`".into());
    }
    let Component::Normal(id) = components[3] else {
        return Err("saved replay path has an unsafe artifact id".into());
    };
    let id = id
        .to_str()
        .ok_or_else(|| "saved replay artifact id is not UTF-8".to_string())?;
    if id.len() != 24
        || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
        || id.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return Err("saved replay artifact id must be 24 lowercase hexadecimal bytes".into());
    }
    Ok(path.to_path_buf())
}

fn sanitize_entry(entry: &str) -> String {
    let mut out = String::new();
    for byte in entry.bytes() {
        if byte.is_ascii_alphanumeric() {
            out.push((byte as char).to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 80 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "program".to_string()
    } else {
        out
    }
}

fn finalize_artifact(path: &Path, bytes: &[u8], json_mode: bool) -> Result<(), String> {
    ensure_safe_parent(path)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() {
            return Err(format!(
                "final replay path is not a regular file: {}",
                path.display()
            ));
        }
        let existing = fs::read(path).map_err(|e| e.to_string())?;
        if existing == bytes {
            if !json_mode {
                eprintln!("already captured");
            }
            return Ok(());
        }
        return Err(format!(
            "refusing to overwrite differing artifact at {}",
            path.display()
        ));
    }
    let tmp = path.with_extension(format!(
        "jetproof-replay.tmp.{}.{}",
        std::process::id(),
        &SHA256::sha256_hex(bytes)[..8]
    ));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    // A hard-link commit is create-new on the final name: unlike rename it
    // cannot replace a concurrent file or symlink.  The temporary file stays
    // in the same directory and is removed after the commit.
    if let Err(error) = fs::hard_link(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error.to_string());
    }
    if let Err(error) = fs::remove_file(&tmp) {
        return Err(error.to_string());
    }
    #[cfg(unix)]
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    #[cfg(unix)]
    {
        let directory = fs::File::open(parent).map_err(|e| e.to_string())?;
        directory.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn ensure_safe_parent(path: &Path) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut current = PathBuf::from(".");
    for component in parent.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "capture parent is a symlink: {}",
                    current.display()
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!(
                    "capture parent is not a directory: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| error.to_string())?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&current, fs::Permissions::from_mode(0o700))
                        .map_err(|error| error.to_string())?;
                }
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn ensure_read_parent(path: &Path) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut current = if parent.is_absolute() {
        PathBuf::from(Path::new("/"))
    } else {
        PathBuf::from(".")
    };
    for component in parent.components() {
        match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => current.push(name),
            Component::ParentDir | Component::Prefix(_) => {
                return Err("replay artifact parent contains an unsafe component".into())
            }
        }
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            format!(
                "could not inspect replay artifact parent `{}`: {error}",
                current.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "replay artifact parent is a symlink: {}",
                current.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!(
                "replay artifact parent is not a directory: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn build_safe_time_artifact_with_run(
    identity: &ReplayIdentity,
    unix_ns: u64,
    outcome: &str,
    status: i64,
    recorded_run: Option<&jet::Debug::RecordedRun>,
) -> Result<Vec<u8>, String> {
    let decision_ledger = recorded_run
        .and_then(|run| run.decision_ledger.as_deref())
        .map(|payload| {
            jet_foundation::MIR::MirDecisionLedger::from_json(payload)
                .map(|ledger| ledger.canonical_json())
        })
        .transpose()?;
    let salt = privacy_salt()?;
    let time_payload = canonical_json(&[
        ("call_id", Json::Int(0)),
        ("site_id", Json::Str(identity.time_site_id.clone())),
        (
            "unix_ns",
            Json::Obj(vec![
                ("bits".into(), Json::Int(64)),
                ("t".into(), Json::Str("int".into())),
                ("v".into(), Json::Str(canonical_u64_string(unix_ns))),
            ]),
        ),
    ]);
    let mut frames = vec![encode_frame(1, KIND_TIME_WALL, 0, time_payload.as_bytes())];
    let mut payload_bytes = time_payload.len() as u64;
    if let Some(recorded_run) = recorded_run {
        for (index, act) in recorded_run.acts.iter().enumerate() {
            if index >= 100_000 - 1 {
                return Err("recorded run exceeds the 100000-frame limit".into());
            }
            let payload = recorded_act_payload(act)?;
            payload_bytes = payload_bytes
                .checked_add(payload.len() as u64)
                .ok_or_else(|| "recorded run payload length overflowed".to_string())?;
            if payload_bytes > 256 * 1024 * 1024 {
                return Err("recorded run exceeds the 256 MiB payload limit".into());
            }
            frames.push(encode_frame(
                1,
                KIND_RUN_ACT,
                u64::try_from(index + 1)
                    .map_err(|_| "recorded run sequence overflowed".to_string())?,
                payload.as_bytes(),
            ));
        }
    }
    let has_recorded_run = recorded_run.is_some_and(|run| !run.acts.is_empty());

    let zero_id = "000000000000000000000000";
    let header_zero = header_json(
        identity,
        zero_id,
        &salt,
        outcome,
        status,
        has_recorded_run,
        decision_ledger.as_deref(),
    );
    let mut body_zero = Vec::new();
    write_prefix(&mut body_zero, header_zero.as_bytes());
    for frame in &frames {
        body_zero.extend_from_slice(frame);
    }
    let artifact_id = content_id(&body_zero);

    let header = header_json(
        identity,
        &artifact_id,
        &salt,
        outcome,
        status,
        has_recorded_run,
        decision_ledger.as_deref(),
    );
    let mut out = Vec::new();
    write_prefix(&mut out, header.as_bytes());
    for frame in &frames {
        out.extend_from_slice(frame);
    }
    write_footer(
        &mut out,
        u64::try_from(frames.len())
            .map_err(|_| "recorded run frame count overflowed".to_string())?,
        payload_bytes,
    );
    Ok(out)
}

fn recorded_act_payload(act: &jet::Debug::RecordedAct) -> Result<String, String> {
    let line = i64::try_from(act.line).map_err(|_| "recorded act line is too large".to_string())?;
    let locals = act
        .locals
        .iter()
        .map(|local| {
            Json::Obj(vec![
                ("name".into(), Json::Str(local.name.clone())),
                ("type".into(), Json::Str(local.type_name.clone())),
                ("value".into(), Json::Str(local.value.clone())),
            ])
        })
        .collect();
    Ok(canonical_json(&[
        ("function", Json::Str(act.function.clone())),
        ("line", Json::Int(line)),
        ("locals", Json::Arr(locals)),
    ]))
}

fn header_json(
    identity: &ReplayIdentity,
    artifact_id: &str,
    salt: &str,
    outcome: &str,
    status: i64,
    has_recorded_run: bool,
    decision_ledger: Option<&str>,
) -> String {
    let mut extensions = Vec::new();
    if has_recorded_run {
        extensions.push((
            "recorded_run".into(),
            Json::Obj(vec![("version".into(), Json::Int(1))]),
        ));
    }
    if let Some(decision_ledger) = decision_ledger {
        extensions.push((
            "decision_ledger".into(),
            Json::Raw(decision_ledger.to_string()),
        ));
    }
    canonical_json(&[
        ("artifact_id", Json::Str(artifact_id.into())),
        (
            "capture",
            Json::Obj(vec![
                ("mode".into(), Json::Str("safe".into())),
                ("roots".into(), Json::Arr(vec![Json::Str("Time".into())])),
            ]),
        ),
        ("extensions", Json::Obj(extensions)),
        (
            "identity",
            Json::Obj(vec![
                ("abi".into(), Json::Str(identity.abi.clone())),
                (
                    "build_digest".into(),
                    Json::Str(identity.build_digest.clone()),
                ),
                ("core_abi".into(), Json::Str(identity.core_abi.clone())),
                ("entry".into(), Json::Str(identity.entry.clone())),
                (
                    "execution_adapter".into(),
                    Json::Str(identity.execution_adapter.clone()),
                ),
                (
                    "lock_digest".into(),
                    Json::Str(identity.lock_digest.clone()),
                ),
                ("profile".into(), Json::Str(identity.profile.clone())),
                (
                    "source_digest".into(),
                    Json::Str(identity.source_digest.clone()),
                ),
                (
                    "time_site_id".into(),
                    Json::Str(identity.time_site_id.clone()),
                ),
                (
                    "target_triple".into(),
                    Json::Str(identity.target_triple.clone()),
                ),
                (
                    "semantic_mir_hash".into(),
                    Json::Str(identity.semantic_mir_hash.clone()),
                ),
                (
                    "optimized_mir_hash".into(),
                    Json::Str(identity.optimized_mir_hash.clone()),
                ),
                ("mir_schema".into(), Json::Str(identity.mir_schema.clone())),
                (
                    "mir_identity".into(),
                    Json::Str(identity.mir_identity.clone()),
                ),
            ]),
        ),
        (
            "limits",
            Json::Obj(vec![
                ("frames".into(), Json::Int(100_000)),
                ("payload_bytes".into(), Json::Int(268_435_456)),
            ]),
        ),
        ("privacy_salt", Json::Str(salt.into())),
        ("producer", Json::Str("jet-prove".into())),
        (
            "run",
            Json::Obj(vec![
                ("outcome".into(), Json::Str(outcome.into())),
                ("status".into(), Json::Int(status)),
            ]),
        ),
        ("schema", Json::Str("jet.replay".into())),
        (
            "version",
            Json::Obj(vec![
                ("major".into(), Json::Int(1)),
                ("minor".into(), Json::Int(0)),
            ]),
        ),
    ])
}

fn write_prefix(out: &mut Vec<u8>, header: &[u8]) {
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(header);
}

fn encode_frame(flags: u8, kind: u16, seq: u64, payload: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(15 + payload.len() + 32);
    raw.push(flags);
    raw.extend_from_slice(&kind.to_le_bytes());
    raw.extend_from_slice(&seq.to_le_bytes());
    raw.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    raw.extend_from_slice(payload);
    let hash = SHA256::sha256(&raw);
    raw.extend_from_slice(&hash);
    raw
}

fn write_footer(out: &mut Vec<u8>, frames: u64, payload_bytes: u64) {
    let hash = SHA256::sha256(out);
    out.extend_from_slice(b"JEND");
    out.extend_from_slice(&frames.to_le_bytes());
    out.extend_from_slice(&payload_bytes.to_le_bytes());
    out.extend_from_slice(&hash);
}

fn content_id(body_with_zero_id: &[u8]) -> String {
    let hex = SHA256::sha256_hex(body_with_zero_id);
    hex[..24.min(hex.len())].to_string()
}

fn canonical_u64_string(value: u64) -> String {
    if value <= MAX_EXACT_JSON_INTEGER {
        value.to_string()
    } else {
        format!("{value:016x}")
    }
}

fn artifact_id_from_bytes(bytes: &[u8]) -> Result<String, String> {
    let header = parse_and_verify(bytes).map_err(|(_, why)| why)?;
    header
        .get("artifact_id")
        .cloned()
        .ok_or_else(|| "missing artifact_id".into())
}

fn parse_and_verify(
    bytes: &[u8],
) -> Result<std::collections::BTreeMap<String, String>, (&'static str, String)> {
    if bytes.len() < 16 + 52 || &bytes[..8] != MAGIC {
        return Err(("E3622", "missing JREPLAY magic".into()));
    }
    let major = u16::from_le_bytes([bytes[8], bytes[9]]);
    let minor = u16::from_le_bytes([bytes[10], bytes[11]]);
    if major != 1 || minor != 0 {
        return Err((
            "E3620",
            format!("unsupported replay schema {major}.{minor}"),
        ));
    }
    let hlen = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    if hlen > 4 * 1024 * 1024 {
        return Err(("E3622", "replay header exceeds the 4 MiB limit".into()));
    }
    let header_end = 16usize
        .checked_add(hlen)
        .ok_or(("E3622", "replay header length overflow".into()))?;
    let minimum_end = header_end
        .checked_add(52)
        .ok_or(("E3622", "replay footer length overflow".into()))?;
    if minimum_end > bytes.len() {
        return Err(("E3622", "truncated header".into()));
    }
    let header_bytes = &bytes[16..header_end];
    let header_text =
        std::str::from_utf8(header_bytes).map_err(|_| ("E3622", "header is not UTF-8".into()))?;
    let flat = flatten_identity_fields(header_text).map_err(|why| {
        let code = if header_schema_error(&why) {
            "E3620"
        } else {
            "E3622"
        };
        (code, why)
    })?;
    let jend = bytes
        .len()
        .checked_sub(52)
        .ok_or(("E3622", "missing JEND footer".into()))?;
    if &bytes[jend..jend + 4] != b"JEND" {
        return Err(("E3622", "missing JEND footer".into()));
    }
    let footer_end = jend
        .checked_add(52)
        .ok_or(("E3622", "replay footer length overflow".into()))?;
    if footer_end != bytes.len() {
        return Err(("E3622", "trailing bytes after footer".into()));
    }
    if jend < header_end {
        return Err(("E3622", "footer precedes replay payload".into()));
    }
    let expected = SHA256::sha256(&bytes[..jend]);
    let actual_start = jend
        .checked_add(20)
        .ok_or(("E3622", "replay footer length overflow".into()))?;
    let actual = &bytes[actual_start..footer_end];
    if actual != expected {
        return Err(("E3622", "footer hash mismatch".into()));
    }
    let mut off = header_end;
    let mut frame_count = 0u64;
    let mut payload_bytes = 0u64;
    let mut expected_sequence = 0u64;
    let mut time_frame_count = 0u64;
    while off < jend {
        let frame_header_end = off
            .checked_add(15)
            .ok_or(("E3622", "replay frame length overflow".into()))?;
        if frame_count >= 100_000 || frame_header_end > jend {
            return Err(("E3622", "replay frame limit or header is invalid".into()));
        }
        let flags = bytes[off];
        let kind = u16::from_le_bytes([bytes[off + 1], bytes[off + 2]]);
        let sequence = u64::from_le_bytes(
            bytes[off + 3..off + 11]
                .try_into()
                .map_err(|_| ("E3622", "invalid replay frame sequence".into()))?,
        );
        let plen = u32::from_le_bytes([
            bytes[off + 11],
            bytes[off + 12],
            bytes[off + 13],
            bytes[off + 14],
        ]) as usize;
        let frame_end = off
            .checked_add(15)
            .and_then(|end| end.checked_add(plen))
            .and_then(|end| end.checked_add(32))
            .ok_or(("E3622", "replay frame length overflow".into()))?;
        if frame_end > jend {
            return Err(("E3622", "truncated frame payload".into()));
        }
        if plen > 1024 * 1024 {
            return Err((
                "E3622",
                "replay frame payload exceeds the 1 MiB limit".into(),
            ));
        }
        if flags != 1
            || !matches!(kind, KIND_TIME_WALL | KIND_RUN_ACT)
            || sequence != expected_sequence
            || (frame_count == 0 && kind != KIND_TIME_WALL)
        {
            return Err(("E3622", "replay frame ordering or kind is invalid".into()));
        }
        let payload_end = off
            .checked_add(15)
            .and_then(|end| end.checked_add(plen))
            .ok_or(("E3622", "replay frame length overflow".into()))?;
        let hash = SHA256::sha256(&bytes[off..payload_end]);
        if hash.as_slice() != &bytes[payload_end..frame_end] {
            return Err(("E3622", "frame hash mismatch".into()));
        }
        frame_count += 1;
        expected_sequence += 1;
        payload_bytes = payload_bytes
            .checked_add(
                u64::try_from(plen)
                    .map_err(|_| ("E3622", "replay payload length overflow".into()))?,
            )
            .ok_or(("E3622", "replay payload length overflow".into()))?;
        if payload_bytes > 256 * 1024 * 1024 {
            return Err(("E3622", "replay payload exceeds the 256 MiB limit".into()));
        }
        let payload = std::str::from_utf8(&bytes[payload_end - plen..payload_end])
            .map_err(|_| ("E3622", "replay frame payload is not UTF-8".into()))?;
        if kind == KIND_TIME_WALL {
            time_frame_count += 1;
            validate_time_payload(payload).map_err(|why| ("E3622", why))?;
        } else {
            validate_recorded_act_payload(payload).map_err(|why| ("E3622", why))?;
        }
        off = frame_end;
    }
    if time_frame_count == 0 {
        return Err(("E3628", "replay artifact contains no Time authority".into()));
    }
    if time_frame_count > 1 {
        return Err((
            "E3622",
            "safe replay must contain exactly one consumed Time frame".into(),
        ));
    }
    let footer_frames = u64::from_le_bytes(
        bytes[jend + 4..jend + 12]
            .try_into()
            .map_err(|_| ("E3622", "invalid replay footer frame count".into()))?,
    );
    let footer_payload = u64::from_le_bytes(
        bytes[jend + 12..jend + 20]
            .try_into()
            .map_err(|_| ("E3622", "invalid replay footer payload count".into()))?,
    );
    if footer_frames != frame_count || footer_payload != payload_bytes {
        return Err(("E3622", "replay footer counts do not match frames".into()));
    }
    let schema = flat.get("schema").map(String::as_str).unwrap_or("");
    if schema != "jet.replay" {
        return Err(("E3622", format!("unexpected schema `{schema}`")));
    }
    let artifact_id = flat.get("artifact_id").map(String::as_str).unwrap_or("");
    if artifact_id.len() != 24 || !artifact_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err((
            "E3622",
            "replay artifact_id is not 24 lowercase hex bytes".into(),
        ));
    }
    if artifact_id.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err(("E3622", "replay artifact_id must use lowercase hex".into()));
    }
    let zero_header = zero_artifact_id_header(header_text).map_err(|why| ("E3622", why))?;
    if zero_header.len() != header_bytes.len() {
        return Err(("E3622", "replay artifact_id field is malformed".into()));
    }
    let mut body_zero = bytes[..16].to_vec();
    body_zero.extend_from_slice(&zero_header);
    body_zero.extend_from_slice(&bytes[header_end..jend]);
    if content_id(&body_zero) != artifact_id {
        return Err(("E3622", "replay artifact_id does not match content".into()));
    }
    Ok(flat)
}

fn header_schema_error(message: &str) -> bool {
    message.contains("unknown field")
        || message.contains("version is incompatible")
        || message.contains("version must be")
        || message.contains("missing version")
}

fn extract_time_ms(bytes: &[u8]) -> Result<Vec<i64>, String> {
    if bytes.len() < 16 {
        return Err("artifact too short for Time root".into());
    }
    let hlen = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let header_end = 16usize
        .checked_add(hlen)
        .ok_or_else(|| "replay header length overflow".to_string())?;
    if header_end > bytes.len() {
        return Err("replay header is truncated".into());
    }
    let jend = bytes
        .len()
        .checked_sub(52)
        .ok_or_else(|| "missing JEND while reading Time".to_string())?;
    if &bytes[jend..jend + 4] != b"JEND" {
        return Err("missing JEND while reading Time".to_string());
    }
    let mut values = Vec::new();
    let mut off = header_end;
    while off < jend {
        let frame_header_end = off
            .checked_add(15)
            .ok_or_else(|| "Time frame header length overflow".to_string())?;
        if frame_header_end > jend {
            return Err("Time frame header is truncated".into());
        }
        let kind = u16::from_le_bytes([bytes[off + 1], bytes[off + 2]]);
        if !matches!(kind, KIND_TIME_WALL | KIND_RUN_ACT) {
            return Err(format!("frame kind {kind:#06x} is not Time"));
        }
        let plen = u32::from_le_bytes([
            bytes[off + 11],
            bytes[off + 12],
            bytes[off + 13],
            bytes[off + 14],
        ]) as usize;
        let payload_end = frame_header_end
            .checked_add(plen)
            .ok_or_else(|| "Time payload length overflow".to_string())?;
        let frame_end = payload_end
            .checked_add(32)
            .ok_or_else(|| "Time frame length overflow".to_string())?;
        if frame_end > jend {
            return Err("Time frame is truncated".into());
        }
        if kind == KIND_TIME_WALL {
            let payload = std::str::from_utf8(&bytes[frame_header_end..payload_end])
                .map_err(|_| "Time payload is not UTF-8".to_string())?;
            values.push(time_ms_from_payload(payload)?);
        }
        off = frame_end;
    }
    if values.is_empty() {
        return Err("replay artifact contains no Time frames".into());
    }
    Ok(values)
}

fn extract_decision_ledger(bytes: &[u8]) -> Result<Option<String>, String> {
    if bytes.len() < 16 {
        return Err("artifact too short for decision ledger".into());
    }
    let hlen = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let header_end = 16usize
        .checked_add(hlen)
        .ok_or_else(|| "replay header length overflow".to_string())?;
    if header_end > bytes.len() {
        return Err("replay decision ledger header is truncated".into());
    }
    let header = std::str::from_utf8(&bytes[16..header_end])
        .map_err(|_| "replay decision ledger header is not UTF-8".to_string())?;
    let value = parse_json(header).map_err(|_| "replay header is not valid JSON".to_string())?;
    let DataTree::Object(root) = value else {
        return Err("replay header must be an object".into());
    };
    let Some(DataTree::Object(extensions)) = object_field(&root, "extensions") else {
        return Ok(None);
    };
    let Some(value) = object_field(extensions, "decision_ledger") else {
        return Ok(None);
    };
    let payload = canonical_json_value(value)?;
    if payload.len() > 512 * 1024 {
        return Err("replay decision ledger exceeds its transport limit".into());
    }
    let ledger = jet_foundation::MIR::MirDecisionLedger::from_json(&payload)?;
    Ok(Some(ledger.canonical_json()))
}

fn extract_recorded_run(bytes: &[u8]) -> Result<jet::Debug::RecordedRun, String> {
    if bytes.len() < 16 {
        return Err("artifact too short for recorded run".into());
    }
    let hlen = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let header_end = 16usize
        .checked_add(hlen)
        .ok_or_else(|| "replay header length overflow".to_string())?;
    let jend = bytes
        .len()
        .checked_sub(52)
        .ok_or_else(|| "missing JEND while reading recorded run".to_string())?;
    if header_end > jend || &bytes[jend..jend + 4] != b"JEND" {
        return Err("recorded run payload is truncated".into());
    }
    let mut run = jet::Debug::RecordedRun {
        acts: Vec::new(),
        decision_ledger: extract_decision_ledger(bytes)?,
    };
    let mut off = header_end;
    while off < jend {
        let frame_header_end = off
            .checked_add(15)
            .ok_or_else(|| "recorded act frame header length overflow".to_string())?;
        if frame_header_end > jend {
            return Err("recorded act frame header is truncated".into());
        }
        let sequence = u64::from_le_bytes(
            bytes[off + 3..off + 11]
                .try_into()
                .map_err(|_| "recorded act sequence is invalid".to_string())?,
        );
        let plen = u32::from_le_bytes([
            bytes[off + 11],
            bytes[off + 12],
            bytes[off + 13],
            bytes[off + 14],
        ]) as usize;
        let payload_end = frame_header_end
            .checked_add(plen)
            .ok_or_else(|| "recorded act payload length overflow".to_string())?;
        let frame_end = payload_end
            .checked_add(32)
            .ok_or_else(|| "recorded act frame length overflow".to_string())?;
        if frame_end > jend {
            return Err("recorded act frame is truncated".into());
        }
        let kind = u16::from_le_bytes([bytes[off + 1], bytes[off + 2]]);
        if kind == KIND_RUN_ACT {
            let payload = std::str::from_utf8(&bytes[frame_header_end..payload_end])
                .map_err(|_| "recorded act payload is not UTF-8".to_string())?;
            run.acts
                .push(parse_recorded_act_payload(payload, sequence)?);
        }
        off = frame_end;
    }
    Ok(run)
}

fn validate_recorded_act_payload(payload: &str) -> Result<(), String> {
    parse_recorded_act_payload(payload, 1).map(|_| ())
}

fn parse_recorded_act_payload(
    payload: &str,
    sequence: u64,
) -> Result<jet::Debug::RecordedAct, String> {
    let value =
        parse_json(payload).map_err(|_| "recorded act payload is not valid JSON".to_string())?;
    if canonical_json_value(&value)? != payload {
        return Err("recorded act payload is not canonical JSON".into());
    }
    let DataTree::Object(root) = value else {
        return Err("recorded act payload must be an object".into());
    };
    require_object_keys(
        &root,
        "recorded act payload",
        &["function", "line", "locals"],
        &["function", "line", "locals"],
    )?;
    let function = object_field(&root, "function")
        .and_then(|value| value.as_str().ok())
        .ok_or_else(|| "recorded act function must be a string".to_string())?;
    if function.is_empty() {
        return Err("recorded act function is empty".into());
    }
    let line = match object_field(&root, "line") {
        Some(DataTree::Int(line)) => {
            usize::try_from(*line).map_err(|_| "recorded act line is invalid".to_string())?
        }
        _ => return Err("recorded act line must be an integer".into()),
    };
    if line == 0 {
        return Err("recorded act line must be at least 1".into());
    }
    let DataTree::Array(locals) = object_field(&root, "locals")
        .ok_or_else(|| "recorded act locals are missing".to_string())?
    else {
        return Err("recorded act locals must be an array".into());
    };
    if locals.len() > 100_000 {
        return Err("recorded act has too many locals".into());
    }
    let mut snapshots = Vec::with_capacity(locals.len());
    for local in locals {
        let DataTree::Object(local) = local else {
            return Err("recorded act local must be an object".into());
        };
        require_object_keys(
            local,
            "recorded act local",
            &["name", "type", "value"],
            &["name", "type", "value"],
        )?;
        let name = object_field(local, "name")
            .and_then(|value| value.as_str().ok())
            .ok_or_else(|| "recorded act local name must be a string".to_string())?;
        let type_name = object_field(local, "type")
            .and_then(|value| value.as_str().ok())
            .ok_or_else(|| "recorded act local type must be a string".to_string())?;
        let value = object_field(local, "value")
            .and_then(|value| value.as_str().ok())
            .ok_or_else(|| "recorded act local value must be a string".to_string())?;
        if name.is_empty() || type_name.is_empty() {
            return Err("recorded act local name and type must be non-empty".into());
        }
        snapshots.push(jet::Debug::ValueSnapshot {
            name: name.to_string(),
            type_name: type_name.to_string(),
            value: value.to_string(),
        });
    }
    Ok(jet::Debug::RecordedAct {
        sequence,
        function: function.to_string(),
        line,
        locals: snapshots,
    })
}

fn time_ms_from_payload(payload: &str) -> Result<i64, String> {
    let value = parse_json(payload).map_err(|_| "Time payload is not valid JSON".to_string())?;
    let DataTree::Object(root) = value else {
        return Err("Time payload must be an object".into());
    };
    let DataTree::Object(unix_ns) = object_field(&root, "unix_ns")
        .ok_or_else(|| "Time payload is missing unix_ns".to_string())?
    else {
        return Err("Time payload unix_ns must be an object".into());
    };
    let value = object_field(unix_ns, "v")
        .and_then(|value| value.as_str().ok())
        .ok_or_else(|| "Time payload unix_ns value is invalid".to_string())?;
    let ns = parse_canonical_u64(value)?;
    i64::try_from(ns / 1_000_000)
        .map_err(|_| "Time value exceeds the signed millisecond range".into())
}

fn validate_time_payload(payload: &str) -> Result<(), String> {
    let value = parse_json(payload).map_err(|_| "Time payload is not valid JSON".to_string())?;
    if canonical_json_value(&value)? != payload {
        return Err("Time payload is not canonical JSON".to_string());
    }
    let DataTree::Object(root) = value else {
        return Err("Time payload must be an object".into());
    };
    for (key, _) in &root {
        if !["call_id", "site_id", "unix_ns"].contains(&key.as_str()) {
            return Err(format!("Time payload has unknown field `{key}`"));
        }
    }
    if !matches!(object_field(&root, "call_id"), Some(DataTree::Int(0))) {
        return Err("Time payload call_id is invalid".into());
    }
    if !matches!(
        object_field(&root, "site_id").and_then(|value| value.as_str().ok()),
        Some(site)
            if site.len() == 64
                && site.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                && site.bytes().any(|byte| byte != b'0')
    ) {
        return Err("Time payload site_id is invalid".into());
    }
    let DataTree::Object(unix_ns) = object_field(&root, "unix_ns")
        .ok_or_else(|| "Time payload is missing unix_ns".to_string())?
    else {
        return Err("Time payload unix_ns must be an object".into());
    };
    require_object_keys(
        unix_ns,
        "Time payload unix_ns",
        &["bits", "t", "v"],
        &["bits", "t", "v"],
    )?;
    if !matches!(object_field(unix_ns, "bits"), Some(DataTree::Int(64)))
        || !matches!(
            object_field(unix_ns, "t").and_then(|value| value.as_str().ok()),
            Some("int")
        )
    {
        return Err("Time payload unix_ns type is invalid".into());
    }
    let value = object_field(unix_ns, "v")
        .and_then(|value| value.as_str().ok())
        .ok_or_else(|| "Time payload unix_ns value is invalid".to_string())?;
    parse_canonical_u64(value).map_err(|_| "Time payload unix_ns value is invalid".to_string())?;
    Ok(())
}

fn parse_canonical_u64(value: &str) -> Result<u64, String> {
    if value.is_empty() {
        return Err("empty unsigned integer".into());
    }
    // An all-digit fixed-width value is ambiguous with decimal text. The
    // canonical emitter uses a leading zero for that case, so recognize it
    // before the decimal branch while keeping decimal-looking large values
    // rejected as non-canonical.
    if value.len() == 16
        && value.starts_with('0')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        let hexadecimal = u64::from_str_radix(value, 16)
            .map_err(|_| "unsigned integer is out of range".to_string())?;
        if hexadecimal <= MAX_EXACT_JSON_INTEGER {
            return Err("small unsigned integer must use minimal decimal".into());
        }
        return Ok(hexadecimal);
    }
    if value.bytes().all(|byte| byte.is_ascii_digit()) {
        if value.len() > 1 && value.starts_with('0') {
            return Err("unsigned integer has a leading zero".into());
        }
        let decimal = value
            .parse::<u64>()
            .map_err(|_| "unsigned integer is out of range".to_string())?;
        if decimal > MAX_EXACT_JSON_INTEGER {
            return Err("large unsigned integer must use fixed-width hexadecimal".into());
        }
        return Ok(decimal);
    }
    if value.len() != 16
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("unsigned integer is not canonical".into());
    }
    let hexadecimal = u64::from_str_radix(value, 16)
        .map_err(|_| "unsigned integer is out of range".to_string())?;
    if hexadecimal <= MAX_EXACT_JSON_INTEGER {
        return Err("small unsigned integer must use minimal decimal".into());
    }
    Ok(hexadecimal)
}

fn object_field<'a>(object: &'a [(String, DataTree)], key: &str) -> Option<&'a DataTree> {
    object
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn canonical_json_value(value: &DataTree) -> Result<String, String> {
    match value {
        DataTree::Null => Ok("null".to_string()),
        DataTree::Bool(value) => Ok(value.to_string()),
        DataTree::Int(value) => Ok(value.to_string()),
        DataTree::Float(value) if value.is_finite() => Ok(value.to_string()),
        DataTree::Float(_) => Err("JSON contains a non-finite number".into()),
        DataTree::Number(value) => Ok(value.clone()),
        DataTree::TypedText(value) | DataTree::Text(value) => Ok(json_str(value)),
        DataTree::Bytes(values) => Ok(format!(
            "[{}]",
            values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        )),
        DataTree::Array(values) => {
            let mut rendered = Vec::with_capacity(values.len());
            for value in values {
                rendered.push(canonical_json_value(value)?);
            }
            Ok(format!("[{}]", rendered.join(",")))
        }
        DataTree::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let mut rendered = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                rendered.push(format!(
                    "{}:{}",
                    json_str(key),
                    canonical_json_value(value)?
                ));
            }
            Ok(format!("{{{}}}", rendered.join(",")))
        }
    }
}

fn require_object_keys(
    object: &[(String, DataTree)],
    object_name: &str,
    required: &[&str],
    allowed: &[&str],
) -> Result<(), String> {
    for (key, _) in object {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{object_name} has unknown field `{key}`"));
        }
    }
    for key in required {
        if object_field(object, key).is_none() {
            return Err(format!("{object_name} is missing `{key}`"));
        }
    }
    Ok(())
}
fn validate_mir_identity(value: &str) -> Result<(), String> {
    let parsed = parse_json(value).map_err(|_| "mir_identity is not valid JSON".to_string())?;
    if canonical_json_value(&parsed)? != value {
        return Err("mir_identity is not canonical JSON".into());
    }
    let DataTree::Object(object) = parsed else {
        return Err("mir_identity must be a JSON object".into());
    };
    require_object_keys(
        &object,
        "mir_identity",
        &[
            "core_ids",
            "function_ids",
            "identity_digest",
            "optimized_hash",
            "schema",
            "semantic_hash",
            "source_map",
            "target_facts",
        ],
        &[
            "core_ids",
            "function_ids",
            "identity_digest",
            "optimized_hash",
            "schema",
            "semantic_hash",
            "source_map",
            "target_facts",
        ],
    )?;
    for key in ["identity_digest", "optimized_hash", "semantic_hash"] {
        let hash = object_field(&object, key)
            .and_then(|value| value.as_str().ok())
            .ok_or_else(|| format!("mir_identity `{key}` must be a string"))?;
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(format!("mir_identity `{key}` is not lowercase SHA-256 hex"));
        }
    }
    if !matches!(
        object_field(&object, "schema").and_then(|value| value.as_str().ok()),
        Some(schema) if schema == jet_foundation::MIR::MIR_IDENTITY_SCHEMA
    ) {
        return Err("mir_identity schema is unsupported".into());
    }
    for key in ["core_ids", "function_ids", "source_map", "target_facts"] {
        if !matches!(object_field(&object, key), Some(DataTree::Array(_))) {
            return Err(format!("mir_identity `{key}` must be an array"));
        }
    }
    Ok(())
}

fn flatten_identity_fields(
    header_text: &str,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let parsed = parse_json(header_text).map_err(|_| "header is not valid JSON".to_string())?;
    if canonical_json_value(&parsed)? != header_text {
        return Err("header is not canonical JSON".into());
    }
    let DataTree::Object(root) = parsed else {
        return Err("header must be a JSON object".into());
    };
    require_object_keys(
        &root,
        "header",
        &[
            "artifact_id",
            "capture",
            "extensions",
            "identity",
            "limits",
            "privacy_salt",
            "producer",
            "run",
            "schema",
            "version",
        ],
        &[
            "artifact_id",
            "capture",
            "extensions",
            "identity",
            "limits",
            "privacy_salt",
            "producer",
            "run",
            "schema",
            "version",
        ],
    )?;
    let mut out = std::collections::BTreeMap::new();
    let string_field = |object: &[(String, DataTree)], key: &str| {
        object_field(object, key)
            .and_then(|value| value.as_str().ok())
            .map(str::to_string)
            .ok_or_else(|| format!("header field `{key}` must be a string"))
    };
    for key in ["schema", "artifact_id"] {
        out.insert(key.to_string(), string_field(&root, key)?);
    }
    let DataTree::Object(identity) = object_field(&root, "identity")
        .ok_or_else(|| "header missing identity object".to_string())?
    else {
        return Err("header identity must be an object".into());
    };
    const IDENTITY_KEYS: &[&str] = &[
        "abi",
        "build_digest",
        "core_abi",
        "entry",
        "execution_adapter",
        "lock_digest",
        "mir_schema",
        "mir_identity",
        "optimized_mir_hash",
        "profile",
        "semantic_mir_hash",
        "source_digest",
        "time_site_id",
        "target_triple",
    ];
    require_object_keys(identity, "header identity", IDENTITY_KEYS, IDENTITY_KEYS)?;
    for key in IDENTITY_KEYS {
        let _ = string_field(identity, key)?;
    }
    for key in [
        "build_digest",
        "lock_digest",
        "optimized_mir_hash",
        "semantic_mir_hash",
        "source_digest",
        "time_site_id",
    ] {
        let value = string_field(identity, key)?;
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(format!(
                "header identity `{key}` is not lowercase SHA-256 hex"
            ));
        }
    }
    let entry = string_field(identity, "entry")?;
    let entry_path = Path::new(&entry);
    if entry.is_empty()
        || entry_path.is_absolute()
        || entry_path.components().any(|component| {
            matches!(
                component,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err("header identity entry is not project-relative".into());
    }
    let adapter = string_field(identity, "execution_adapter")?;
    if adapter != "mir-v1" {
        return Err(format!(
            "header execution adapter `{adapter}` is unsupported"
        ));
    }
    let mir_identity = string_field(identity, "mir_identity")?;
    validate_mir_identity(&mir_identity)?;
    for key in ["abi", "core_abi", "mir_schema", "profile", "target_triple"] {
        if string_field(identity, key)?.is_empty() {
            return Err(format!("header identity field `{key}` is empty"));
        }
    }
    if string_field(identity, "mir_schema")? != jet_foundation::MIR::MIR_IDENTITY_SCHEMA {
        return Err("header identity MIR schema is unsupported".into());
    }
    for key in [
        "entry",
        "source_digest",
        "execution_adapter",
        "target_triple",
    ] {
        out.insert(key.to_string(), string_field(identity, key)?);
    }
    for key in [
        "abi",
        "build_digest",
        "core_abi",
        "lock_digest",
        "mir_schema",
        "mir_identity",
        "optimized_mir_hash",
        "profile",
        "semantic_mir_hash",
        "time_site_id",
    ] {
        out.insert(key.to_string(), string_field(identity, key)?);
    }
    let producer = string_field(&root, "producer")?;
    if producer != "jet-prove" {
        return Err("header producer is not jet-prove".into());
    }
    if !matches!(
        object_field(&root, "privacy_salt").and_then(|value| value.as_str().ok()),
        Some(salt) if is_base64url_32(salt)
    ) {
        return Err("header privacy_salt is invalid".into());
    }
    if !matches!(object_field(&root, "extensions"), Some(DataTree::Object(_))) {
        return Err("header extensions must be an object".into());
    }
    let DataTree::Object(capture) = object_field(&root, "capture")
        .ok_or_else(|| "header missing capture object".to_string())?
    else {
        return Err("header capture must be an object".into());
    };
    require_object_keys(
        capture,
        "header capture",
        &["mode", "roots"],
        &["mode", "roots"],
    )?;
    if !matches!(
        object_field(capture, "mode").and_then(|value| value.as_str().ok()),
        Some("safe")
    ) {
        return Err("header capture mode is not safe".into());
    }
    if !matches!(
        object_field(capture, "roots"),
        Some(DataTree::Array(roots))
            if roots.len() == 1
                && matches!(
                    roots.first().and_then(|value| value.as_str().ok()),
                    Some("Time")
                )
    ) {
        return Err("header capture roots are not exactly [Time]".into());
    }
    let DataTree::Object(limits) =
        object_field(&root, "limits").ok_or_else(|| "header missing limits object".to_string())?
    else {
        return Err("header limits must be an object".into());
    };
    require_object_keys(
        limits,
        "header limits",
        &["frames", "payload_bytes"],
        &["frames", "payload_bytes"],
    )?;
    if !matches!(object_field(limits, "frames"), Some(DataTree::Int(100_000)))
        || !matches!(
            object_field(limits, "payload_bytes"),
            Some(DataTree::Int(268_435_456))
        )
    {
        return Err("header replay limits are invalid".into());
    }
    let DataTree::Object(run) =
        object_field(&root, "run").ok_or_else(|| "header missing run object".to_string())?
    else {
        return Err("header run must be an object".into());
    };
    require_object_keys(
        run,
        "header run",
        &["outcome", "status"],
        &["outcome", "status"],
    )?;
    let outcome = object_field(run, "outcome")
        .and_then(|value| value.as_str().ok())
        .ok_or_else(|| "header run outcome is invalid".to_string())?;
    let status = match object_field(run, "status") {
        Some(DataTree::Int(status)) if (0..=255).contains(status) => *status,
        _ => return Err("header run status is invalid".into()),
    };
    if !matches!(outcome, "exit" | "panic") {
        return Err("header run outcome is invalid".into());
    }
    if (outcome == "panic") != (status == 70) {
        return Err("header run outcome and status disagree".into());
    }
    out.insert("run_outcome".to_string(), outcome.to_string());
    out.insert("run_status".to_string(), status.to_string());
    let DataTree::Object(version) = object_field(&root, "version")
        .ok_or_else(|| "header missing version object".to_string())?
    else {
        return Err("header version must be an object".into());
    };
    require_object_keys(
        version,
        "header version",
        &["major", "minor"],
        &["major", "minor"],
    )?;
    if !matches!(object_field(version, "major"), Some(DataTree::Int(1)))
        || !matches!(object_field(version, "minor"), Some(DataTree::Int(0)))
    {
        return Err("header version is incompatible".into());
    }
    Ok(out)
}

fn zero_artifact_id_header(header_text: &str) -> Result<Vec<u8>, String> {
    let mut value = parse_json(header_text).map_err(|_| "header is not valid JSON".to_string())?;
    let DataTree::Object(root) = &mut value else {
        return Err("header must be a JSON object".into());
    };
    let valid_length = object_field(root, "artifact_id")
        .and_then(|value| value.as_str().ok())
        .is_some_and(|artifact_id| artifact_id.len() == 24);
    if !valid_length {
        return Err("header artifact_id must be a string with the right length".into());
    }
    let Some((_, artifact_id)) = root.iter_mut().find(|(key, _)| key == "artifact_id") else {
        return Err("header artifact_id must be a string".into());
    };
    *artifact_id = DataTree::Text("0".repeat(24));
    Ok(canonical_json_value(&value)?.into_bytes())
}

fn is_base64url_32(value: &str) -> bool {
    if value.len() != 43 {
        return false;
    }
    let mut buffer = 0u32;
    let mut bits = 0u8;
    let mut bytes = 0usize;
    for byte in value.bytes() {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return false,
        } as u32;
        buffer = (buffer << 6) | digit;
        bits += 6;
        while bits >= 8 {
            bits -= 8;
            bytes += 1;
            buffer &= if bits == 0 { 0 } else { (1u32 << bits) - 1 };
        }
    }
    bytes == 32 && bits == 2 && buffer == 0
}

fn identity_matches(
    header: &std::collections::BTreeMap<String, String>,
    identity: &ReplayIdentity,
) -> Result<(), String> {
    let check = |key: &str, expected: &str| match header.get(key) {
        Some(actual) if actual == expected => Ok(()),
        Some(_) => Err(key.to_string()),
        None => Err(key.to_string()),
    };
    for (key, expected) in [
        ("entry", identity.entry.as_str()),
        ("source_digest", identity.source_digest.as_str()),
        ("build_digest", identity.build_digest.as_str()),
        ("lock_digest", identity.lock_digest.as_str()),
        ("semantic_mir_hash", identity.semantic_mir_hash.as_str()),
        ("optimized_mir_hash", identity.optimized_mir_hash.as_str()),
        ("abi", identity.abi.as_str()),
        ("core_abi", identity.core_abi.as_str()),
        ("profile", identity.profile.as_str()),
        ("mir_schema", identity.mir_schema.as_str()),
        ("mir_identity", identity.mir_identity.as_str()),
        ("execution_adapter", identity.execution_adapter.as_str()),
        ("target_triple", identity.target_triple.as_str()),
        ("time_site_id", identity.time_site_id.as_str()),
    ] {
        check(key, expected)?;
    }
    Ok(())
}

fn privacy_salt() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom_fill(&mut bytes)?;
    Ok(base64url_unpadded(&bytes))
}

fn getrandom_fill(buf: &mut [u8]) -> Result<(), String> {
    // Std-only: read from /dev/urandom on Unix; fail closed otherwise.
    #[cfg(unix)]
    {
        use std::io::Read;
        let mut f = fs::File::open("/dev/urandom").map_err(|e| e.to_string())?;
        f.read_exact(buf).map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        let _ = buf;
        Err("privacy_salt requires OS CSPRNG".into())
    }
}

fn base64url_unpadded(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push(T[(n & 63) as usize] as char);
        i += 3;
    }
    if i < bytes.len() {
        let rem = bytes.len() - i;
        let mut n = (bytes[i] as u32) << 16;
        if rem == 2 {
            n |= (bytes[i + 1] as u32) << 8;
        }
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if rem == 2 {
            out.push(T[((n >> 6) & 63) as usize] as char);
        }
    }
    out
}

enum Json {
    Str(String),
    Int(i64),
    Raw(String),
    Obj(Vec<(String, Json)>),
    Arr(Vec<Json>),
}
fn canonical_json(fields: &[(&str, Json)]) -> String {
    let mut pairs: Vec<(String, Json)> = fields
        .iter()
        .map(|(k, v)| ((*k).to_string(), clone_json(v)))
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    render_obj(&pairs)
}

fn clone_json(v: &Json) -> Json {
    match v {
        Json::Str(s) => Json::Str(s.clone()),
        Json::Int(n) => Json::Int(*n),
        Json::Raw(raw) => Json::Raw(raw.clone()),
        Json::Obj(fields) => Json::Obj(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), clone_json(v)))
                .collect(),
        ),
        Json::Arr(items) => Json::Arr(items.iter().map(clone_json).collect()),
    }
}

fn render_obj(fields: &[(String, Json)]) -> String {
    let mut out = String::from("{");
    for (i, (k, v)) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&json_str(k));
        out.push(':');
        out.push_str(&render_value(v));
    }
    out.push('}');
    out
}

fn render_value(v: &Json) -> String {
    match v {
        Json::Str(s) => json_str(s),
        Json::Int(n) => n.to_string(),
        Json::Raw(raw) => raw.clone(),
        Json::Obj(fields) => {
            let mut sorted: Vec<(String, Json)> = fields
                .iter()
                .map(|(k, v)| (k.clone(), clone_json(v)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            render_obj(&sorted)
        }
        Json::Arr(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&render_value(item));
            }
            out.push(']');
            out
        }
    }
}

fn json_str(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn emit_diag(code: &str, what: &str, why: &str, fix: &str, json_mode: bool) {
    crate::emit_cli_report(
        code,
        what.to_string(),
        why.to_string(),
        fix.to_string(),
        json_mode,
    );
}
pub(crate) fn emit_prove_diag(code: &str, what: &str, why: &str, fix: &str, json_mode: bool) {
    crate::emit_cli_report_for_action(
        "prove",
        code,
        what.to_string(),
        why.to_string(),
        fix.to_string(),
        json_mode,
    );
}

#[allow(dead_code)]
pub(crate) fn fail_usage(message: &str) -> ! {
    crate::cli_error!("E2104", "{message}");
    exit(ExitCodes::USAGE);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ReplayIdentity {
        ReplayIdentity {
            entry: "examples/prove.jet".into(),
            source_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .into(),
            execution_adapter: "mir-v1".into(),
            target_triple: "x86_64-unknown-linux-gnu".into(),
            abi: "gnu".into(),
            build_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            core_abi: "1".into(),
            lock_digest: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            profile: "dev".into(),
            semantic_mir_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            optimized_mir_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            mir_schema: "mir-v1".into(),
            mir_identity: "{\"core_ids\":[],\"function_ids\":[],\"identity_digest\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"optimized_hash\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"schema\":\"mir-v1\",\"semantic_hash\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"source_map\":[],\"target_facts\":[]}".into(),
            time_site_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
        }
    }

    #[test]
    fn safe_time_artifact_round_trips_and_preserves_authority() {
        let bytes =
            build_safe_time_artifact_with_run(&identity(), 1_234_567_890, "exit", 0, None).unwrap();
        let header = parse_and_verify(&bytes).unwrap();
        assert_eq!(header.get("schema").map(String::as_str), Some("jet.replay"));
        assert_eq!(header.get("run_outcome").map(String::as_str), Some("exit"));
        assert_eq!(header.get("run_status").map(String::as_str), Some("0"));
        assert_eq!(extract_time_ms(&bytes).unwrap()[0], 1_234);
    }

    #[test]
    fn recorded_acts_stay_inside_the_replay_receipt() {
        let run = jet::Debug::RecordedRun {
            acts: vec![jet::Debug::RecordedAct {
                sequence: 1,
                function: "run".into(),
                line: 4,
                locals: vec![jet::Debug::ValueSnapshot {
                    name: "total".into(),
                    type_name: "Int".into(),
                    value: "0".into(),
                }],
            }],
            decision_ledger: None,
        };
        let bytes =
            build_safe_time_artifact_with_run(&identity(), 1_234_567_890, "exit", 0, Some(&run))
                .unwrap();
        let header = parse_and_verify(&bytes).unwrap();
        assert!(header.get("schema").is_some());
        assert!(String::from_utf8_lossy(&bytes).contains("recorded_run"));
        assert_eq!(extract_recorded_run(&bytes).unwrap(), run);
        assert_eq!(extract_time_ms(&bytes).unwrap(), vec![1_234]);
    }

    #[test]
    fn time_integer_encoding_is_minimal_until_fixed_width_is_required() {
        assert_eq!(canonical_u64_string(0), "0");
        assert_eq!(
            canonical_u64_string(MAX_EXACT_JSON_INTEGER),
            "9007199254740991"
        );
        assert_eq!(
            canonical_u64_string(MAX_EXACT_JSON_INTEGER + 1),
            "0020000000000000"
        );
        assert_eq!(parse_canonical_u64("0").unwrap(), 0);
        assert_eq!(
            parse_canonical_u64("0020000000000000").unwrap(),
            MAX_EXACT_JSON_INTEGER + 1
        );
        assert!(parse_canonical_u64("0000000000000000").is_err());
        assert!(parse_canonical_u64("9007199254740992").is_err());
    }

    #[test]
    fn privacy_salt_requires_a_valid_thirty_two_byte_base64url_value() {
        assert!(is_base64url_32(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ));
        assert!(!is_base64url_32(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA!"
        ));
        assert!(!is_base64url_32(
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA-"
        ));
    }

    #[test]
    fn safe_time_artifact_detects_payload_tampering() {
        let mut bytes =
            build_safe_time_artifact_with_run(&identity(), 1_234_567_890, "exit", 0, None).unwrap();
        let hlen = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let payload = 16 + hlen + 15;
        bytes[payload] ^= 1;
        assert!(parse_and_verify(&bytes).is_err());
    }

    #[test]
    fn replay_rejects_a_different_target_identity() {
        let path = std::env::temp_dir().join(format!(
            "jet-replay-identity-{}-{}.jetproof-replay",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bytes =
            build_safe_time_artifact_with_run(&identity(), 1_234_567_890, "exit", 0, None).unwrap();
        fs::write(&path, bytes).unwrap();
        let mut different = identity();
        different.source_digest = "different-source".into();
        let result = prepare_replay(&different, &path.to_string_lossy());
        fs::remove_file(&path).unwrap();
        assert!(matches!(result, Err(("E3621", _))));
    }

    #[test]
    fn replay_rejects_identity_changes_beyond_source_digest() {
        let bytes =
            build_safe_time_artifact_with_run(&identity(), 1_234_567_890, "exit", 0, None).unwrap();
        let header = parse_and_verify(&bytes).unwrap();
        let mut different = identity();
        different.core_abi = "2".into();
        assert_eq!(header.get("core_abi").map(String::as_str), Some("1"));
        assert!(identity_matches(&header, &different).is_err());
    }

    #[test]
    fn replay_rejects_time_site_identity_changes() {
        let original = identity();
        let bytes =
            build_safe_time_artifact_with_run(&original, 1_234_567_890, "exit", 0, None).unwrap();
        let header = parse_and_verify(&bytes).unwrap();
        let mut different = original.clone();
        different.time_site_id =
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into();
        assert_eq!(
            header.get("time_site_id").map(String::as_str),
            Some(original.time_site_id.as_str())
        );
        assert!(identity_matches(&header, &different).is_err());
    }

    #[test]
    fn capture_paths_reject_empty_or_hostile_components() {
        assert!(validate_capture_path("traces//run.jetproof-replay").is_err());
        assert!(validate_capture_path("traces\\run.jetproof-replay").is_err());
        assert!(validate_capture_path("traces/run.jetproof-replay").is_ok());
    }
}
