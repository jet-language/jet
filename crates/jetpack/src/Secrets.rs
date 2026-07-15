//! U13 encrypted-repo-secrets engine (D-JPK-SECRETCRYPTO1=A, card c9jetpackgates).
//!
//! `secret("name")`-shaped repo secrets: `.jet/secrets.age` (project-relative,
//! committed — it's ciphertext) holds every declared secret, encrypted to
//! every recipient in `.jet/secrets-recipients` (project-relative, committed
//! plaintext, one `age1...` public key per line — the same convention the
//! real `age`/`rage` tools use for a recipients file). The matching private
//! identity (`AGE-SECRET-KEY-1...`) never leaves the machine that generated
//! it: `~/.jet/keys/secrets.identity` (or `$JET_KEYS_DIR/secrets.identity`,
//! same override `Source/Publish/Sign.rs` uses for signing keys), 0600.
//!
//! The actual X25519/ChaCha20-Poly1305 crypto lives in the hidden FFI bridge
//! (`Prelude/SecretsCrypto.rs`, the `age` crate, D-JPK-SECRETCRYPTO1) — this
//! module never touches it directly. `jetpack` itself stays I6-zero-dependency
//! by shelling out to the bridge's cached `jet-secrets-helper` binary, exactly
//! as `Source/Publish/Sign.rs` shells out to `jet-crypto-helper` for package
//! signing (card c146) — same pattern, mirrored function-for-function.
//!
//! Decrypted values are held in memory only for the span of one `set`/`get`
//! call; nothing plaintext is ever written to `.jet/`, the lock, the hangar,
//! or a temp file — the store on disk is always ciphertext, end to end
//! (encrypt/decrypt happen over the helper's stdin/stdout pipe, never a file).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// `.jet/secrets.age` — the encrypted store, project-relative. Committed:
/// it's ciphertext.
pub fn store_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(crate::Syntax::CONFIG_DEFAULT_DIR)
        .join("secrets.age")
}

/// `.jet/secrets-recipients` — plaintext, one `age1...` public key per line
/// (`#`-prefixed comment lines and blank lines ignored). Committed.
pub fn recipients_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(crate::Syntax::CONFIG_DEFAULT_DIR)
        .join("secrets-recipients")
}

/// The local decrypt identity: `$JET_KEYS_DIR/secrets.identity` if set, else
/// `~/.jet/keys/secrets.identity` — same directory `Source/Publish/Sign.rs`
/// uses for signing keys (`keys_dir`), just a different file name. Never
/// project-relative, never committed.
pub fn identity_path() -> PathBuf {
    let dir = if let Ok(dir) = std::env::var("JET_KEYS_DIR") {
        if !dir.is_empty() {
            PathBuf::from(dir)
        } else {
            default_keys_dir()
        }
    } else {
        default_keys_dir()
    };
    dir.join("secrets.identity")
}

fn default_keys_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".jet").join("keys")
}

/// Build (or reuse from cache) the `jet-secrets-helper` binary and return its
/// path. Reuses the exact hidden-cargo bridge the compiled-program `core.vault`
/// path already builds; the first call is a cold `cargo build`, every later
/// call is an instant cache hit — same shape as `Source/Publish/Sign.rs`'s
/// `ensure_bridge_helper`.
pub fn ensure_bridge_helper() -> Result<PathBuf, String> {
    // needs_secrets = true, everything else off, no extern entries.
    let link = crate::FFI::build_bridge(&[], false, false, false, false, false, false, false, true)
        .map_err(|mut ds| {
            ds.drain(..)
                .next()
                .map(|d| d.what)
                .unwrap_or_else(|| "the secrets bridge failed to build".to_string())
        })?;
    link.secrets_helper_bin_path
        .ok_or_else(|| "the secrets helper binary was not produced by the bridge build".to_string())
}

/// Generate a fresh identity/recipient pair, writing the identity to
/// [`identity_path`] (0600) and returning `(identity_path, recipient_string)`.
/// Refuses to overwrite an existing identity unless `force` (mirrors
/// `Sign::keygen`'s E1248 refusal — no secrets-specific code minted for it,
/// same "don't orphan existing ciphertext" rationale).
pub fn keygen(force: bool) -> Result<(PathBuf, String), String> {
    let path = identity_path();
    if path.is_file() && !force {
        return Err(format!(
            "a secrets identity already exists at `{}` — pass `--force` if you're sure (this orphans anything encrypted to the old key)",
            path.display()
        ));
    }
    let helper = ensure_bridge_helper()?;
    let out = run_helper(&helper, "keygen")?;
    let mut it = out.split_whitespace();
    let identity = it
        .next()
        .ok_or_else(|| "keygen produced no identity".to_string())?
        .to_string();
    let recipient = it
        .next()
        .ok_or_else(|| "keygen produced no recipient".to_string())?
        .to_string();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("couldn't create keys directory: {e}"))?;
    }
    std::fs::write(&path, &identity).map_err(|e| format!("couldn't write identity: {e}"))?;
    set_mode(&path, 0o600);
    Ok((path, recipient))
}

/// `jetpack secrets recipients add <age1...>` — appends to
/// [`recipients_path`] if not already present. `Ok(false)` means already
/// present; lock and filesystem failures remain errors.
pub fn add_recipient(project_dir: &Path, recipient: &str) -> Result<bool, String> {
    let _guard =
        super::RuntimePolicy::acquire_lock(&super::Store::managed_dir(project_dir), "secrets")
            .map_err(|e| format!("couldn't lock secrets recipients: {e}"))?;
    let path = recipients_path(project_dir);
    let recipient = recipient.trim();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("couldn't create `{}`: {e}", parent.display()))?;
    }
    let mut existing = match std::fs::read_to_string(&path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("couldn't read `{}`: {error}", path.display())),
    };
    if existing
        .lines()
        .map(str::trim)
        .any(|line| !line.starts_with('#') && line == recipient)
    {
        return Ok(false);
    }
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(recipient);
    existing.push('\n');
    atomic_write(&path, existing.as_bytes())?;
    Ok(true)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("`{}` has no parent directory", path.display()))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("secret");
    let temp = parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| format!("couldn't create `{}`: {e}", temp.display()))?;
        file.write_all(bytes)
            .map_err(|e| format!("couldn't write `{}`: {e}", temp.display()))?;
        file.sync_all()
            .map_err(|e| format!("couldn't sync `{}`: {e}", temp.display()))?;
        std::fs::rename(&temp, path)
            .map_err(|e| format!("couldn't finalize `{}`: {e}", path.display()))?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|e| format!("couldn't sync `{}`: {e}", parent.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

/// `jetpack secrets recipients list` — every recipient line (comments/blanks
/// stripped).
pub fn list_recipients(project_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(recipients_path(project_dir))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Read + decrypt the whole store. An absent store is an empty one (a fresh
/// project, not an error) — the *first* `set` creates it. A present store
/// with no local identity, or one that fails to decrypt, is an error.
pub fn read_store(project_dir: &Path) -> Result<Vec<(String, String)>, String> {
    let path = store_path(project_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let identity = std::fs::read_to_string(identity_path()).map_err(|_| {
        format!(
            "no local secrets identity at `{}` — run `jetpack secrets keygen`",
            identity_path().display()
        )
    })?;
    let ciphertext =
        std::fs::read(&path).map_err(|e| format!("couldn't read `{}`: {e}", path.display()))?;
    let helper = ensure_bridge_helper()?;
    let cmd = format!("decrypt {} {}", identity.trim(), hex_encode(&ciphertext));
    let plaintext_hex = run_helper(&helper, &cmd)?;
    let plaintext = hex_decode(&plaintext_hex)?;
    decode_pairs(&plaintext).ok_or_else(|| "the decrypted store is corrupt".to_string())
}

/// Encrypt + write the whole store to every recipient in
/// [`list_recipients`]. Ciphertext only ever touches disk here — the plaintext
/// wire bytes exist only in this process's memory and on the helper
/// subprocess's stdin/stdout pipe, never a file.
pub fn write_store(project_dir: &Path, pairs: &[(String, String)]) -> Result<(), String> {
    let _guard =
        super::RuntimePolicy::acquire_lock(&super::Store::managed_dir(project_dir), "secrets")
            .map_err(|e| e.to_string())?;
    let recipients = list_recipients(project_dir);
    if recipients.is_empty() {
        return Err(
            "no recipients declared — run `jetpack secrets recipients add <age1...>` first \
             (generate one with `jetpack secrets keygen`)"
                .to_string(),
        );
    }
    let helper = ensure_bridge_helper()?;
    let plaintext = encode_pairs(pairs);
    let cmd = format!(
        "encrypt {} {}",
        recipients.join(","),
        hex_encode(&plaintext)
    );
    let ciphertext_hex = run_helper(&helper, &cmd)?;
    let ciphertext = hex_decode(&ciphertext_hex)?;
    let path = store_path(project_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("couldn't create `.jet`: {e}"))?;
    }
    // Write via a temp file + rename so a killed process never leaves a
    // half-written store — the temp file holds ciphertext only, same as the
    // final path.
    atomic_write(&path, &ciphertext)
}

/// `jetpack secrets set <name> <value>` — upsert one entry and re-encrypt the
/// whole store.
pub fn set(project_dir: &Path, name: &str, value: &str) -> Result<(), String> {
    let mut pairs = read_store(project_dir)?;
    match pairs.iter_mut().find(|(k, _)| k == name) {
        Some((_, v)) => *v = value.to_string(),
        None => pairs.push((name.to_string(), value.to_string())),
    }
    write_store(project_dir, &pairs)
}

/// `jetpack secrets get <name>` — `Ok(None)` is "no such entry" (E1263 at the
/// CLI layer), distinct from `Err` (a bridge/crypto/identity failure).
pub fn get(project_dir: &Path, name: &str) -> Result<Option<String>, String> {
    let pairs = read_store(project_dir)?;
    Ok(pairs.into_iter().find(|(k, _)| k == name).map(|(_, v)| v))
}

// ──────────────────────────────────────────────
// Helper subprocess (mirrors Source/Publish/Sign.rs's spawn_helper/run_helper)
// ──────────────────────────────────────────────

fn run_helper(helper: &Path, command: &str) -> Result<String, String> {
    let out = spawn_helper(helper, command)?;
    if !out.status.success() {
        return Err(format!(
            "the secrets helper failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn spawn_helper(helper: &Path, command: &str) -> Result<std::process::Output, String> {
    let mut child = Command::new(helper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't run the secrets helper: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(command.as_bytes())
            .map_err(|e| format!("couldn't write to the secrets helper: {e}"))?;
    }
    child
        .wait_with_output()
        .map_err(|e| format!("the secrets helper didn't finish: {e}"))
}

// ──────────────────────────────────────────────
// Wire format + encodings (I6: std-only, no external crates). The pairs codec
// mirrors `Prelude/SecretsCrypto.rs`'s `jet_vault_encode_pairs`/
// `jet_vault_decode_pairs` byte-for-byte — the two are independently built
// crates (this one and the hidden bridge) that can't share Rust types, so the
// wire format is duplicated rather than imported, same reasoning as
// `jet.db`'s bind-param wire encoding.
// ──────────────────────────────────────────────

fn encode_pairs(pairs: &[(String, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
    for (k, v) in pairs {
        let kb = k.as_bytes();
        let vb = v.as_bytes();
        out.extend_from_slice(&(kb.len() as u32).to_le_bytes());
        out.extend_from_slice(kb);
        out.extend_from_slice(&(vb.len() as u32).to_le_bytes());
        out.extend_from_slice(vb);
    }
    out
}

fn decode_pairs(bytes: &[u8]) -> Option<Vec<(String, String)>> {
    fn take_u32(b: &[u8], at: &mut usize) -> Option<u32> {
        let end = at.checked_add(4)?;
        let slice: [u8; 4] = b.get(*at..end)?.try_into().ok()?;
        *at = end;
        Some(u32::from_le_bytes(slice))
    }
    fn take_str(b: &[u8], at: &mut usize, len: u32) -> Option<String> {
        let end = at.checked_add(len as usize)?;
        let slice = b.get(*at..end)?;
        *at = end;
        String::from_utf8(slice.to_vec()).ok()
    }
    let mut at = 0usize;
    let count = take_u32(bytes, &mut at)?;
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let klen = take_u32(bytes, &mut at)?;
        let k = take_str(bytes, &mut at, klen)?;
        let vlen = take_u32(bytes, &mut at)?;
        let v = take_str(bytes, &mut at, vlen)?;
        out.push((k, v));
    }
    if at != bytes.len() {
        return None;
    }
    Some(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err("odd-length hex string".to_string());
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16).ok_or("invalid hex digit")?;
        let lo = (bytes[i + 1] as char)
            .to_digit(16)
            .ok_or("invalid hex digit")?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Ok(out)
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn pairs_wire_round_trips() {
        let pairs = vec![
            ("db_password".to_string(), "hunter2".to_string()),
            ("api_key".to_string(), "abc123".to_string()),
        ];
        let bytes = encode_pairs(&pairs);
        assert_eq!(decode_pairs(&bytes), Some(pairs));
    }

    #[test]
    fn hex_roundtrips() {
        let data = [0u8, 1, 2, 250, 255, 16, 128];
        let h = hex_encode(&data);
        assert_eq!(hex_decode(&h).unwrap(), data);
    }

    #[test]
    fn identity_path_honors_env_override() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join("jet_secrets_test_keys_dir");
        std::env::set_var("JET_KEYS_DIR", &dir);
        assert_eq!(identity_path(), dir.join("secrets.identity"));
        std::env::remove_var("JET_KEYS_DIR");
    }

    #[test]
    fn recipients_add_is_idempotent_and_lists() {
        let dir = std::env::temp_dir().join(format!(
            "jet_secrets_unit_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        assert!(add_recipient(&dir, "age1exampleexampleexample").unwrap());
        assert!(
            !add_recipient(&dir, "age1exampleexampleexample").unwrap(),
            "idempotent"
        );
        assert_eq!(list_recipients(&dir), vec!["age1exampleexampleexample"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn recipient_lock_failure_is_not_already_present() {
        let dir = std::env::temp_dir().join(format!(
            "jet_secrets_lock_failure_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(dir.join(".jet")).unwrap();
        std::fs::write(dir.join(".jet/.locks"), "not a directory").unwrap();
        let error = add_recipient(&dir, "age1failure")
            .expect_err("lock failure must remain an error");
        assert!(error.contains("couldn't lock"), "{error}");
        assert!(!recipients_path(&dir).exists());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn recipient_write_failure_is_not_success() {
        let dir = std::env::temp_dir().join(format!(
            "jet_secrets_write_failure_{}_{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(recipients_path(&dir)).unwrap();
        let error = add_recipient(&dir, "age1failure")
            .expect_err("recipient path directory must make the write fail");
        assert!(error.contains("couldn't read"), "{error}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// Full round trip through the real age-style crypto FFI bridge: keygen,
    /// add a recipient, `set`, `get`, confirm the plaintext matches. Slow (a
    /// cold run triggers a real `cargo build` of the hidden bridge crate) —
    /// exercises the actual `age` crate integration, not just the pure-logic
    /// helpers above.
    #[test]
    fn crypto_round_trip_through_bridge() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!(
            "jet_secrets_bridge_rt_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let keys_dir = dir.join("keys");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::create_dir_all(&keys_dir);
        std::env::set_var("JET_KEYS_DIR", &keys_dir);

        let (_, recipient) = keygen(true).expect("keygen should succeed");
        assert!(add_recipient(&dir, &recipient).unwrap());

        set(&dir, "db_password", "hunter2").expect("set should succeed");
        let got = get(&dir, "db_password").expect("get should succeed");
        assert_eq!(got, Some("hunter2".to_string()));

        // Missing entry is `Ok(None)`, not an error.
        assert_eq!(get(&dir, "no_such_key").unwrap(), None);

        std::env::remove_var("JET_KEYS_DIR");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_store_on_absent_file_is_empty_not_error() {
        let dir = std::env::temp_dir().join(format!(
            "jet_secrets_unit_absent_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        assert_eq!(read_store(&dir), Ok(Vec::new()));
        std::fs::remove_dir_all(&dir).ok();
    }
}
