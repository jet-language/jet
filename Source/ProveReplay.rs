//! D-JREPLAY1=A / D-PROVE-REPLAY1=A: `.jetproof-replay` capture and `--replay`.
//!
//! Safe capture records Time only. Sensitive roots require `--capture-sensitive`
//! (consent path). Artifacts use the ratified binary envelope (magic `JREPLAY\0`,
//! canonical JSON header, framed records, `JEND` footer).

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};

use jet::ExitCodes;
use jet::SHA256;
use jet_foundation::JSON::{parse_json, JSONValue};

const MAGIC: &[u8; 8] = b"JREPLAY\0";
const KIND_TIME_WALL: u16 = 0x0001;
const MAX_REPLAY_BYTES: u64 = 256 * 1024 * 1024;

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
}

#[derive(Clone, Debug)]
pub(crate) struct CaptureAuthority {
    unix_ns: u64,
    explicit_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct ReplayAuthority {
    pub time_ms: i64,
    pub expected_outcome: String,
    pub expected_status: i32,
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
                "`--replay` needs a `.jetproof-replay` artifact path".to_string(),
            )),
        };
    }
    None
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
    eprintln!("capture preflight: safe Time only; normal producer will run under this authority");
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
    eprintln!("capture: Time authority prepared");
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
) -> Result<(), i32> {
    let outcome = if exit_code == ExitCodes::RUNTIME_PANIC {
        "panic"
    } else {
        "exit"
    };
    let status = i64::from(exit_code);
    let bytes = match build_safe_time_artifact(identity, authority.unix_ns, outcome, status) {
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
    if let Err(message) = finalize_artifact(&dest, &bytes) {
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
    eprintln!("capture: finalized outcome={outcome} status={status}");
    eprintln!("artifact: {rel}");
    Ok(())
}

/// Validate an artifact and install its captured authorities for the normal
/// proof producer. Replay is execution of the same producer under this
/// adapter, not a successful early return that merely checks a file.
pub(crate) fn prepare_replay(
    identity: &ReplayIdentity,
    artifact_path: &str,
) -> Result<ReplayAuthority, (&'static str, String)> {
    let path = Path::new(artifact_path);
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ("E3622", format!("could not inspect `{artifact_path}`: {error}"))
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
    let bytes = fs::read(path)
        .map_err(|error| ("E3622", format!("could not read `{artifact_path}`: {error}")))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_REPLAY_BYTES {
        return Err((
            "E3622",
            format!("replay artifact exceeds the {MAX_REPLAY_BYTES}-byte limit"),
        ));
    }
    let header = parse_and_verify(&bytes)?;
    identity_matches(&header, identity)
        .map_err(|field| ("E3621", format!("identity field `{field}` differs from the current target")))?;
    let time_ms = extract_first_time_ms(&bytes).map_err(|why| ("E3628", why))?;
    let expected_outcome = header
        .get("run_outcome")
        .cloned()
        .ok_or(("E3622", "replay header is missing run outcome".to_string()))?;
    let expected_status = header
        .get("run_status")
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|status| (0..=255).contains(status))
        .ok_or(("E3622", "replay header has an invalid run status".to_string()))?;
    Ok(ReplayAuthority {
        time_ms,
        expected_outcome,
        expected_status,
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

fn finalize_artifact(path: &Path, bytes: &[u8]) -> Result<(), String> {
    ensure_safe_parent(path)?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if !metadata.file_type().is_file() {
            return Err(format!("final replay path is not a regular file: {}", path.display()));
        }
        let existing = fs::read(path).map_err(|e| e.to_string())?;
        if existing == bytes {
            eprintln!("already captured");
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
    if let Some(parent) = path.parent() {
        let directory = fs::File::open(parent).map_err(|e| e.to_string())?;
        directory.sync_all().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn ensure_safe_parent(path: &Path) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut current = PathBuf::from(".");
    for component in parent.components() {
        let Component::Normal(name) = component else { continue };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("capture parent is a symlink: {}", current.display()));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!("capture parent is not a directory: {}", current.display()));
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

fn build_safe_time_artifact(
    identity: &ReplayIdentity,
    unix_ns: u64,
    outcome: &str,
    status: i64,
) -> Result<Vec<u8>, String> {
    let salt = privacy_salt()?;
    let payload = canonical_json(&[
        ("call_id", Json::Int(0)),
        (
            "site_id",
            Json::Str("0000000000000000000000000000000000000000000000000000000000000000".into()),
        ),
        (
            "unix_ns",
            Json::Obj(vec![
                ("bits".into(), Json::Int(64)),
                ("t".into(), Json::Str("int".into())),
                ("v".into(), Json::Str(format!("{unix_ns:016x}"))),
            ]),
        ),
    ]);
    let frame = encode_frame(1, KIND_TIME_WALL, 0, payload.as_bytes());

    let zero_id = "000000000000000000000000";
    let header_zero = header_json(identity, zero_id, &salt, outcome, status);
    let mut body_zero = Vec::new();
    write_prefix(&mut body_zero, header_zero.as_bytes());
    body_zero.extend_from_slice(&frame);
    let artifact_id = content_id(&body_zero);

    let header = header_json(identity, &artifact_id, &salt, outcome, status);
    let mut out = Vec::new();
    write_prefix(&mut out, header.as_bytes());
    out.extend_from_slice(&frame);
    write_footer(&mut out, 1, payload.len() as u64);
    Ok(out)
}

fn header_json(
    identity: &ReplayIdentity,
    artifact_id: &str,
    salt: &str,
    outcome: &str,
    status: i64,
) -> String {
    canonical_json(&[
        ("artifact_id", Json::Str(artifact_id.into())),
        (
            "capture",
            Json::Obj(vec![
                ("mode".into(), Json::Str("safe".into())),
                (
                    "roots".into(),
                    Json::Arr(vec![Json::Str("Time".into())]),
                ),
            ]),
        ),
        ("extensions", Json::Obj(vec![])),
        (
            "identity",
            Json::Obj(vec![
                ("abi".into(), Json::Str("gnu".into())),
                (
                    "build_digest".into(),
                    Json::Str(identity.source_digest.clone()),
                ),
                ("core_abi".into(), Json::Str("1".into())),
                ("entry".into(), Json::Str(identity.entry.clone())),
                (
                    "execution_adapter".into(),
                    Json::Str(identity.execution_adapter.clone()),
                ),
                (
                    "lock_digest".into(),
                    Json::Str(identity.source_digest.clone()),
                ),
                ("profile".into(), Json::Str("dev".into())),
                (
                    "source_digest".into(),
                    Json::Str(identity.source_digest.clone()),
                ),
                (
                    "target_triple".into(),
                    Json::Str(identity.target_triple.clone()),
                ),
                (
                    "tir_hash".into(),
                    Json::Str(identity.source_digest.clone()),
                ),
                ("tir_schema".into(), Json::Str("1".into())),
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
    let header_text = std::str::from_utf8(header_bytes)
        .map_err(|_| ("E3622", "header is not UTF-8".into()))?;
    let flat = flatten_identity_fields(header_text)
        .map_err(|why| ("E3622", why))?;
    let jend = bytes
        .windows(4)
        .rposition(|w| w == b"JEND")
        .ok_or(("E3622", "missing JEND footer".into()))?;
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
            return Err(("E3622", "replay frame payload exceeds the 1 MiB limit".into()));
        }
        if flags != 1 || kind != KIND_TIME_WALL || sequence != expected_sequence {
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
            .checked_add(u64::try_from(plen).map_err(|_| ("E3622", "replay payload length overflow".into()))?)
            .ok_or(("E3622", "replay payload length overflow".into()))?;
        if payload_bytes > 256 * 1024 * 1024 {
            return Err(("E3622", "replay payload exceeds the 256 MiB limit".into()));
        }
        let payload = std::str::from_utf8(&bytes[payload_end - plen..payload_end])
            .map_err(|_| ("E3622", "replay frame payload is not UTF-8".into()))?;
        validate_time_payload(payload).map_err(|why| ("E3622", why))?;
        off = frame_end;
    }
    if frame_count == 0 {
        return Err(("E3622", "replay artifact contains no Time frames".into()));
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
        return Err(("E3622", "replay artifact_id is not 24 lowercase hex bytes".into()));
    }
    if artifact_id.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return Err(("E3622", "replay artifact_id must use lowercase hex".into()));
    }
    let zero_id = "0".repeat(24);
    let marker = format!("\"artifact_id\":\"{artifact_id}\"");
    let replacement = format!("\"artifact_id\":\"{zero_id}\"");
    let zero_header = header_text
        .replace(&marker, &replacement)
        .into_bytes();
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

fn extract_first_time_ms(bytes: &[u8]) -> Result<i64, String> {
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
    let mut off = header_end;
    let jend = bytes
        .windows(4)
        .rposition(|w| w == b"JEND")
        .ok_or_else(|| "missing JEND while reading Time".to_string())?;
    let header_frame_end = off
        .checked_add(15)
        .ok_or_else(|| "Time frame header length overflow".to_string())?;
    if header_frame_end > jend {
        return Err("no Time frame present in replay artifact".into());
    }
    let kind = u16::from_le_bytes([bytes[off + 1], bytes[off + 2]]);
    if kind != KIND_TIME_WALL {
        return Err(format!("first frame kind {kind:#06x} is not Time"));
    }
    let plen = u32::from_le_bytes([
        bytes[off + 11],
        bytes[off + 12],
        bytes[off + 13],
        bytes[off + 14],
    ]) as usize;
    let frame_end = off
        .checked_add(15)
        .and_then(|end| end.checked_add(plen))
        .ok_or_else(|| "Time payload length overflow".to_string())?;
    if frame_end > jend {
        return Err("Time payload is truncated".into());
    }
    let payload_end = off
        .checked_add(15)
        .and_then(|end| end.checked_add(plen))
        .ok_or_else(|| "Time payload length overflow".to_string())?;
    let payload = std::str::from_utf8(&bytes[payload_end - plen..payload_end])
        .map_err(|_| "Time payload is not UTF-8".to_string())?;
    let hex = extract_nested_hex_v(payload).ok_or_else(|| {
        "Time payload missing unix_ns.v hex field".to_string()
    })?;
    let ns = u64::from_str_radix(&hex, 16)
        .map_err(|_| format!("invalid unix_ns hex `{hex}`"))?;
    let millis = ns / 1_000_000;
    i64::try_from(millis).map_err(|_| "Time value exceeds the signed millisecond range".into())
}

fn extract_nested_hex_v(payload: &str) -> Option<String> {
    // payload shape: {"call_id":0,...,"unix_ns":{"bits":64,"t":"int","v":"<hex>"}}
    let key = "\"v\":\"";
    let start = payload.find(key)? + key.len();
    let end = payload[start..].find('"')? + start;
    Some(payload[start..end].to_string())
}

fn validate_time_payload(payload: &str) -> Result<(), String> {
    let value = parse_json(payload).map_err(|_| "Time payload is not valid JSON".to_string())?;
    if canonical_json_value(&value)? != payload {
        return Err("Time payload is not canonical JSON".to_string());
    }
    let JSONValue::Object(root) = value else {
        return Err("Time payload must be an object".into());
    };
    for key in root.keys() {
        if !["call_id", "site_id", "unix_ns"].contains(&key.as_str()) {
            return Err(format!("Time payload has unknown field `{key}`"));
        }
    }
    if !matches!(root.get("call_id"), Some(JSONValue::Number(0))) {
        return Err("Time payload call_id is invalid".into());
    }
    if !matches!(root.get("site_id"), Some(JSONValue::String(site)) if site.len() == 64 && site.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())) {
        return Err("Time payload site_id is invalid".into());
    }
    let JSONValue::Object(unix_ns) = root
        .get("unix_ns")
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
    if !matches!(unix_ns.get("bits"), Some(JSONValue::Number(64)))
        || !matches!(unix_ns.get("t"), Some(JSONValue::String(kind)) if kind == "int")
    {
        return Err("Time payload unix_ns type is invalid".into());
    }
    if !matches!(unix_ns.get("v"), Some(JSONValue::String(value)) if value.len() == 16 && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())) {
        return Err("Time payload unix_ns value is invalid".into());
    }
    Ok(())
}

fn canonical_json_value(value: &JSONValue) -> Result<String, String> {
    match value {
        JSONValue::Null => Ok("null".to_string()),
        JSONValue::Bool(value) => Ok(value.to_string()),
        JSONValue::Number(value) => Ok(value.to_string()),
        JSONValue::Flt(value) if value.is_finite() => Ok(value.to_string()),
        JSONValue::Flt(_) => Err("JSON contains a non-finite number".into()),
        JSONValue::String(value) => Ok(json_str(value)),
        JSONValue::Array(values) => {
            let mut rendered = Vec::with_capacity(values.len());
            for value in values {
                rendered.push(canonical_json_value(value)?);
            }
            Ok(format!("[{}]", rendered.join(",")))
        }
        JSONValue::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            let mut rendered = Vec::with_capacity(keys.len());
            for key in keys {
                rendered.push(format!(
                    "{}:{}",
                    json_str(key),
                    canonical_json_value(&object[key])?
                ));
            }
            Ok(format!("{{{}}}", rendered.join(",")))
        }
    }
}

fn require_object_keys(
    object: &std::collections::HashMap<String, JSONValue>,
    object_name: &str,
    required: &[&str],
    allowed: &[&str],
) -> Result<(), String> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{object_name} has unknown field `{key}`"));
        }
    }
    for key in required {
        if !object.contains_key(*key) {
            return Err(format!("{object_name} is missing `{key}`"));
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
    let JSONValue::Object(root) = parsed else {
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
    let string_field = |object: &std::collections::HashMap<String, JSONValue>, key: &str| {
        match object.get(key) {
            Some(JSONValue::String(value)) => Ok(value.clone()),
            Some(_) => Err(format!("header field `{key}` must be a string")),
            None => Err(format!("header missing `{key}`")),
        }
    };
    for key in ["schema", "artifact_id"] {
        out.insert(key.to_string(), string_field(&root, key)?);
    }
    let JSONValue::Object(identity) = root
        .get("identity")
        .ok_or_else(|| "header missing identity object".to_string())?
    else {
        return Err("header identity must be an object".into());
    };
    require_object_keys(
        identity,
        "header identity",
        &[
            "abi",
            "build_digest",
            "core_abi",
            "entry",
            "execution_adapter",
            "lock_digest",
            "profile",
            "source_digest",
            "target_triple",
            "tir_hash",
            "tir_schema",
        ],
        &[
            "abi",
            "build_digest",
            "core_abi",
            "entry",
            "execution_adapter",
            "lock_digest",
            "profile",
            "source_digest",
            "target_triple",
            "tir_hash",
            "tir_schema",
        ],
    )?;
    for key in ["entry", "source_digest", "execution_adapter", "target_triple"] {
        out.insert(key.to_string(), string_field(identity, key)?);
    }
    let JSONValue::String(producer) = root
        .get("producer")
        .ok_or_else(|| "header missing producer".to_string())?
    else {
        return Err("header producer must be a string".into());
    };
    if producer != "jet-prove" {
        return Err("header producer is not jet-prove".into());
    }
    if !matches!(root.get("privacy_salt"), Some(JSONValue::String(salt)) if salt.len() == 43 && salt.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')) {
        return Err("header privacy_salt is invalid".into());
    }
    if !matches!(root.get("extensions"), Some(JSONValue::Object(_))) {
        return Err("header extensions must be an object".into());
    }
    let JSONValue::Object(capture) = root
        .get("capture")
        .ok_or_else(|| "header missing capture object".to_string())?
    else {
        return Err("header capture must be an object".into());
    };
    if !matches!(capture.get("mode"), Some(JSONValue::String(mode)) if mode == "safe") {
        return Err("header capture mode is not safe".into());
    }
    if !matches!(capture.get("roots"), Some(JSONValue::Array(roots)) if roots.len() == 1 && matches!(&roots[0], JSONValue::String(root) if root == "Time")) {
        return Err("header capture roots are not exactly [Time]".into());
    }
    require_object_keys(capture, "header capture", &["mode", "roots"], &["mode", "roots"])?;
    let JSONValue::Object(limits) = root
        .get("limits")
        .ok_or_else(|| "header missing limits object".to_string())?
    else {
        return Err("header limits must be an object".into());
    };
    require_object_keys(
        limits,
        "header limits",
        &["frames", "payload_bytes"],
        &["frames", "payload_bytes"],
    )?;
    if !matches!(limits.get("frames"), Some(JSONValue::Number(100_000)))
        || !matches!(limits.get("payload_bytes"), Some(JSONValue::Number(268_435_456)))
    {
        return Err("header replay limits are invalid".into());
    }
    let JSONValue::Object(run) = root
        .get("run")
        .ok_or_else(|| "header missing run object".to_string())?
    else {
        return Err("header run must be an object".into());
    };
    require_object_keys(run, "header run", &["outcome", "status"], &["outcome", "status"])?;
    if !matches!(run.get("outcome"), Some(JSONValue::String(outcome)) if matches!(outcome.as_str(), "exit" | "panic" | "timeout" | "unavailable"))
        || !matches!(run.get("status"), Some(JSONValue::Number(status)) if (0..=255).contains(status))
    {
        return Err("header run fields are invalid".into());
    }
    if let Some(JSONValue::String(outcome)) = run.get("outcome") {
        out.insert("run_outcome".to_string(), outcome.clone());
    }
    if let Some(JSONValue::Number(status)) = run.get("status") {
        out.insert("run_status".to_string(), status.to_string());
    }
    let JSONValue::Object(version) = root
        .get("version")
        .ok_or_else(|| "header missing version object".to_string())?
    else {
        return Err("header version must be an object".into());
    };
    if !matches!(version.get("major"), Some(JSONValue::Number(1)))
        || !matches!(version.get("minor"), Some(JSONValue::Number(0)))
    {
        return Err("header version is incompatible".into());
    }
    Ok(out)
}

fn identity_matches(
    header: &std::collections::BTreeMap<String, String>,
    identity: &ReplayIdentity,
) -> Result<(), String> {
    let check = |key: &str, expected: &str| {
        match header.get(key) {
            Some(actual) if actual == expected => Ok(()),
            Some(_) => Err(key.to_string()),
            None => Err(key.to_string()),
        }
    };
    check("entry", &identity.entry)?;
    check("source_digest", &identity.source_digest)?;
    check("execution_adapter", &identity.execution_adapter)?;
    check("target_triple", &identity.target_triple)?;
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
        Json::Obj(fields) => Json::Obj(fields.iter().map(|(k, v)| (k.clone(), clone_json(v))).collect()),
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
        Json::Obj(fields) => {
            let mut sorted: Vec<(String, Json)> =
                fields.iter().map(|(k, v)| (k.clone(), clone_json(v))).collect();
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

fn emit_diag(code: &str, what: &str, why: &str, fix: &str, json_mode: bool) {
    if json_mode {
        println!(
            "{{\"schema_version\":1,\"code\":{},\"severity\":\"error\",\"message\":{},\"why\":{},\"fix\":{},\"detail\":null,\"file\":null,\"line\":null,\"col\":null,\"span\":null,\"edit\":null}}",
            json_str(code),
            json_str(what),
            json_str(why),
            json_str(fix)
        );
    } else {
        eprintln!("Error [{code}]: {what}");
        eprintln!(" Why: {why}");
        eprintln!(" Fix: {fix}");
    }
}

#[allow(dead_code)]
pub(crate) fn fail_usage(message: &str) -> ! {
    eprintln!("error: {message}");
    exit(ExitCodes::USAGE);
}
