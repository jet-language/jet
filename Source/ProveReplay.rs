//! D-JREPLAY1=A / D-PROVE-REPLAY1=A: `.jetproof-replay` capture and `--replay`.
//!
//! Safe capture records Time only. Sensitive roots require `--capture-sensitive`
//! (consent path). Artifacts use the ratified binary envelope (magic `JREPLAY\0`,
//! canonical JSON header, framed records, `JEND` footer).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};

use jet::ExitCodes;
use jet::SHA256;

const MAGIC: &[u8; 8] = b"JREPLAY\0";
const KIND_TIME_WALL: u16 = 0x0001;

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

pub(crate) fn run_safe_capture(
    identity: &ReplayIdentity,
    opts: &CaptureOpts,
    json_mode: bool,
) -> i32 {
    if opts.sensitive {
        // Non-interactive expert path refuses until a TTY consent flow lands.
        emit_diag(
            "E3627",
            "replay capture refused sensitive data",
            "non-interactive `--capture-sensitive` cannot collect TTY consent for raw Rand/IO/Net values",
            "run from a TTY and type `capture sensitive`, or use safe `--capture` for Time-only roots",
            json_mode,
        );
        return ExitCodes::USER_ERROR;
    }
    eprintln!("capture preflight: safe Time only; authority unchanged");
    let unix_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let bytes = match build_safe_time_artifact(identity, unix_ns) {
        Ok(bytes) => bytes,
        Err(message) => {
            emit_diag(
                "E3629",
                "replay artifact could not be finalized",
                &message,
                "fix the destination path and retry capture",
                json_mode,
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let dest = match resolve_capture_path(identity, opts.path.as_deref(), &bytes) {
        Ok(path) => path,
        Err(message) => {
            emit_diag(
                "E3629",
                "replay artifact could not be finalized",
                &message,
                "choose a project-relative nonexistent `.jetproof-replay` path",
                json_mode,
            );
            return ExitCodes::USER_ERROR;
        }
    };
    if let Err(message) = finalize_artifact(&dest, &bytes) {
        emit_diag(
            "E3629",
            "replay artifact could not be finalized",
            &message,
            "fix the destination path and retry capture",
            json_mode,
        );
        return ExitCodes::USER_ERROR;
    }
    let rel = dest.display().to_string();
    eprintln!("capture: exit 0");
    eprintln!("artifact: {rel}");
    if json_mode {
        println!(
            "{{\"schema\":\"jet.replay\",\"version\":{{\"major\":1,\"minor\":0}},\"capture\":{{\"mode\":\"safe\",\"roots\":[\"Time\"]}},\"artifact\":{}}}",
            json_str(&rel)
        );
    }
    ExitCodes::OK
}

pub(crate) fn run_replay(
    identity: &ReplayIdentity,
    artifact_path: &str,
    json_mode: bool,
) -> i32 {
    let bytes = match fs::read(artifact_path) {
        Ok(b) => b,
        Err(e) => {
            emit_diag(
                "E3622",
                "replay artifact is corrupt",
                &format!("could not read `{artifact_path}`: {e}"),
                "pass an intact `.jetproof-replay` path",
                json_mode,
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let header = match parse_and_verify(&bytes) {
        Ok(h) => h,
        Err(code_why) => {
            let (code, why) = code_why;
            emit_diag(
                code,
                match code {
                    "E3620" => "replay schema version is incompatible",
                    "E3621" => "replay semantic identity does not match",
                    _ => "replay artifact is corrupt",
                },
                &why,
                "recapture with the current toolchain, or repair the artifact",
                json_mode,
            );
            return ExitCodes::USER_ERROR;
        }
    };
    if let Err(field) = identity_matches(&header, identity) {
        emit_diag(
            "E3621",
            "replay semantic identity does not match",
            &format!("identity field `{field}` differs from the current target"),
            "recapture against this exact source/toolchain revision",
            json_mode,
        );
        return ExitCodes::USER_ERROR;
    }
    eprintln!("ambient authority opened: none; {} exact", identity.execution_adapter);
    eprintln!("replay: exact match");
    if json_mode {
        println!(
            "{{\"schema\":\"jet.replay\",\"status\":\"exact\",\"artifact\":{}}}",
            json_str(artifact_path)
        );
    }
    ExitCodes::OK
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
        if Path::new(path).is_absolute() {
            return Ok(PathBuf::from(path));
        }
        return Ok(PathBuf::from(path));
    }
    let id = artifact_id_from_bytes(bytes)?;
    let stem = Path::new(&identity.entry)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("program")
        .replace('.', "_");
    Ok(PathBuf::from(format!(
        ".jet/replays/{stem}-{id}.jetproof-replay"
    )))
}

fn finalize_artifact(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if path.exists() {
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
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

fn build_safe_time_artifact(identity: &ReplayIdentity, unix_ns: u64) -> Result<Vec<u8>, String> {
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
    let header_zero = header_json(identity, zero_id, &salt, "exit", 0);
    let mut body_zero = Vec::new();
    write_prefix(&mut body_zero, header_zero.as_bytes());
    body_zero.extend_from_slice(&frame);
    let artifact_id = content_id(&body_zero);

    let header = header_json(identity, &artifact_id, &salt, "exit", 0);
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

fn parse_and_verify(bytes: &[u8]) -> Result<std::collections::BTreeMap<String, String>, ( &'static str, String)> {
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
    if 16 + hlen + 52 > bytes.len() {
        return Err(("E3622", "truncated header".into()));
    }
    let header_bytes = &bytes[16..16 + hlen];
    let header_text = std::str::from_utf8(header_bytes)
        .map_err(|_| ("E3622", "header is not UTF-8".into()))?;
    let flat = flatten_identity_fields(header_text)
        .map_err(|why| ("E3622", why))?;
    let jend = bytes
        .windows(4)
        .rposition(|w| w == b"JEND")
        .ok_or(("E3622", "missing JEND footer".into()))?;
    if jend + 52 != bytes.len() {
        return Err(("E3622", "trailing bytes after footer".into()));
    }
    let expected = SHA256::sha256(&bytes[..jend]);
    let actual = &bytes[jend + 20..jend + 52];
    if actual != expected {
        return Err(("E3622", "footer hash mismatch".into()));
    }
    // Verify first frame hash when present.
    let mut off = 16 + hlen;
    if off < jend {
        if off + 15 > jend {
            return Err(("E3622", "truncated frame".into()));
        }
        let plen = u32::from_le_bytes([
            bytes[off + 11],
            bytes[off + 12],
            bytes[off + 13],
            bytes[off + 14],
        ]) as usize;
        if off + 15 + plen + 32 > jend {
            return Err(("E3622", "truncated frame payload".into()));
        }
        let hash = SHA256::sha256(&bytes[off..off + 15 + plen]);
        if hash.as_slice() != &bytes[off + 15 + plen..off + 15 + plen + 32] {
            return Err(("E3622", "frame hash mismatch".into()));
        }
    }
    let schema = flat.get("schema").map(String::as_str).unwrap_or("");
    if schema != "jet.replay" {
        return Err(("E3622", format!("unexpected schema `{schema}`")));
    }
    Ok(flat)
}

fn flatten_identity_fields(
    header_text: &str,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    // Minimal extractor for the identity strings we compare; rejects non-object.
    let mut out = std::collections::BTreeMap::new();
    for key in [
        "schema",
        "artifact_id",
        "entry",
        "source_digest",
        "execution_adapter",
        "target_triple",
    ] {
        if let Some(v) = extract_string_field(header_text, key) {
            out.insert(key.to_string(), v);
        }
    }
    if !out.contains_key("schema") {
        return Err("header missing schema".into());
    }
    Ok(out)
}

fn extract_string_field(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = json.find(&needle)? + needle.len();
    let mut out = String::new();
    let bytes = json.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            return Some(out);
        }
        if b == b'\\' {
            i += 1;
            if i >= bytes.len() {
                return None;
            }
            out.push(bytes[i] as char);
        } else {
            out.push(b as char);
        }
        i += 1;
    }
    None
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
