//! Package signing tier A — Ed25519 author signatures (card c146, D-PKGSIGN1).
//!
//! Tier B (SHA-256 content checksum) is the always-on integrity floor (c122).
//! Tier A adds an *author* signature: `jet registry publish` signs the package's
//! `content_hash` with the publisher's Ed25519 key and pins the public key into
//! the registry index on first publish (TOFU). Fetchers verify the signature
//! against the pinned key.
//!
//! The Ed25519 primitive is already implemented in the hidden FFI bridge
//! (`crates/jet-driver/src/Prelude/Crypto.rs`, `ed25519-dalek` per
//! D-DEP-CRYPTO1). `jet` itself is zero-dependency (I6), so — exactly as it
//! shells out to `cargo`/`rustc` to build that bridge — it shells out to the
//! bridge's `jet-crypto-helper` binary for keygen/sign/verify. No crypto crate
//! is ever added to `Source/` or `crates/jet-driver`'s own `Cargo.toml`.

use crate::Diagnostics::Diagnostic;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HELPER_ENTROPY_UNAVAILABLE_EXIT: i32 = 75;

/// Default registry name a key belongs to when none is given.
pub const DEFAULT_REGISTRY: &str = "jet";

/// Directory that holds signing keys: `$JET_KEYS_DIR` if set (tests/CI point it
/// at a scratch dir, same override idiom as `JET_STORE_DIR` /
/// `JET_REGISTRY_CACHE_DIR`), else `~/.jet/keys`.
pub fn keys_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("JET_KEYS_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".jet").join("keys")
}

/// `(seed_path, public_key_path)` for a registry:
/// `<keys>/<registry>.ed25519` (32-byte seed, 0600) and
/// `<keys>/<registry>.ed25519.pub` (hex public key, 0644).
pub fn key_paths(registry: &str) -> (PathBuf, PathBuf) {
    let dir = keys_dir();
    let seed = dir.join(format!("{registry}.ed25519"));
    let public = dir.join(format!("{registry}.ed25519.pub"));
    (seed, public)
}

/// Whether a signing key already exists for `registry`.
pub fn key_exists(registry: &str) -> bool {
    key_paths(registry).0.is_file()
}

/// Read the stored hex public key for `registry`, if a key exists.
pub fn read_public_key(registry: &str) -> Option<String> {
    let (_, pub_path) = key_paths(registry);
    std::fs::read_to_string(&pub_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Build (or reuse from cache) the `jet-crypto-helper` binary and return its
/// path. Reuses the exact hidden-cargo bridge the compiled-program crypto path
/// already builds; the first call is a cold `cargo build`, every later call is
/// an instant cache hit.
pub fn ensure_bridge_helper() -> Result<PathBuf, Diagnostic> {
    // needs_crypto = true, everything else off, no extern entries.
    let link = crate::FFI::build_bridge(&[], false, false, false, false, true, false, false, false)
        .map_err(|mut ds| {
            ds.drain(..)
                .next()
                .unwrap_or_else(|| bridge_error("the crypto bridge failed to build"))
        })?;
    link.helper_bin_path.ok_or_else(|| {
        bridge_error("the crypto helper binary was not produced by the bridge build")
    })
}

/// Generate a fresh Ed25519 keypair for `registry`, writing the seed (0600) and
/// hex public key (0644). Refuses to overwrite an existing key unless `force`
/// (E1248). Returns `(seed_path, public_key_path, public_key_hex)`.
pub fn keygen(registry: &str, force: bool) -> Result<(PathBuf, PathBuf, String), Diagnostic> {
    let (seed_path, pub_path) = key_paths(registry);
    if seed_path.is_file() && !force {
        return Err(e1248(&seed_path));
    }
    let helper = ensure_bridge_helper()?;
    let mut out = run_keygen_helper(&helper)?;
    let parsed = (|| {
        let mut it = out.split(|byte| byte.is_ascii_whitespace());
        let seed_hex = it
            .find(|field| !field.is_empty())
            .ok_or_else(|| bridge_error("keygen produced no seed"))?;
        let pub_hex = it
            .find(|field| !field.is_empty())
            .ok_or_else(|| bridge_error("keygen produced no public key"))?;
        let seed_hex = std::str::from_utf8(seed_hex)
            .map_err(|_| bridge_error("keygen produced a malformed seed"))?;
        let pub_hex = std::str::from_utf8(pub_hex)
            .map_err(|_| bridge_error("keygen produced a malformed public key"))?;
        let seed = hex_decode(seed_hex).map_err(|e| bridge_error(&e))?;
        Ok::<_, Diagnostic>((seed, pub_hex.to_string()))
    })();
    volatile_zeroize(&mut out);
    let (mut seed, pub_hex) = parsed?;

    let write_result = (|| {
        if let Some(parent) = seed_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io_error("keys directory", e))?;
        }
        std::fs::write(&seed_path, &seed).map_err(|e| io_error("signing key", e))?;
        set_mode(&seed_path, 0o600);
        std::fs::write(&pub_path, pub_hex.as_bytes())
            .map_err(|e| io_error("public key", e))?;
        set_mode(&pub_path, 0o644);
        Ok::<_, Diagnostic>(())
    })();
    volatile_zeroize(&mut seed);
    write_result?;

    Ok((seed_path, pub_path, pub_hex))
}

/// Sign `content_hash` (its raw string bytes) with the seed at `seed_path`.
/// Returns the base64-encoded 64-byte Ed25519 signature.
pub fn sign(seed_path: &Path, content_hash: &str) -> Result<String, Diagnostic> {
    let helper = ensure_bridge_helper()?;
    let seed = std::fs::read(seed_path).map_err(|e| io_error("signing key", e))?;
    let cmd = format!(
        "sign {} {}",
        hex_encode(&seed),
        hex_encode(content_hash.as_bytes())
    );
    let sig_hex = run_helper(&helper, &cmd)?;
    let sig = hex_decode(&sig_hex).map_err(|e| bridge_error(&e))?;
    Ok(base64_encode(&sig))
}

/// Verify a base64 signature over `content_hash` against a hex public key.
/// `Ok(true)` = valid, `Ok(false)` = does not verify (tampered / wrong key).
/// `Err` only for an internal failure to run the helper at all.
pub fn verify(
    public_key_hex: &str,
    content_hash: &str,
    signature_b64: &str,
) -> Result<bool, Diagnostic> {
    let sig = match base64_decode(signature_b64) {
        Ok(s) => s,
        Err(_) => return Ok(false), // malformed signature never verifies
    };
    let helper = ensure_bridge_helper()?;
    let cmd = format!(
        "verify {} {} {}",
        public_key_hex.trim(),
        hex_encode(content_hash.as_bytes()),
        hex_encode(&sig)
    );
    // Helper exit: 0 = valid, anything else (2 invalid, 1 bad key/args) = not
    // verified. Any non-zero → false so we never silently accept (I1).
    Ok(run_helper_status(&helper, &cmd)? == 0)
}

// ──────────────────────────────────────────────
// Helper subprocess
// ──────────────────────────────────────────────

/// Run the helper with `command` on stdin, returning trimmed stdout. Non-zero
/// exit is an error (used by keygen/sign, which have no "invalid" outcome).
fn run_helper(helper: &Path, command: &str) -> Result<String, Diagnostic> {
    let out = spawn_helper(helper, command)?;
    if !out.status.success() {
        return Err(bridge_error(&format!(
            "the crypto helper failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Key generation's entropy failure is a closed helper status. Provider and
/// helper text never cross this boundary; the command owns E1292's copy.
fn run_keygen_helper(helper: &Path) -> Result<Vec<u8>, Diagnostic> {
    let mut out = spawn_helper(helper, "keygen")?;
    if out.status.code() == Some(HELPER_ENTROPY_UNAVAILABLE_EXIT) {
        volatile_zeroize(&mut out.stdout);
        return Err(e1292());
    }
    if !out.status.success() {
        volatile_zeroize(&mut out.stdout);
        return Err(bridge_error("the crypto helper could not generate a signing key"));
    }
    Ok(out.stdout)
}

/// Run the helper and return its exit code (used by verify, which distinguishes
/// valid / invalid / error by exit status).
fn run_helper_status(helper: &Path, command: &str) -> Result<i32, Diagnostic> {
    let out = spawn_helper(helper, command)?;
    Ok(out.status.code().unwrap_or(-1))
}

fn spawn_helper(helper: &Path, command: &str) -> Result<std::process::Output, Diagnostic> {
    let mut child = Command::new(helper)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| bridge_error(&format!("couldn't run the crypto helper: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(command.as_bytes())
            .map_err(|e| bridge_error(&format!("couldn't write to the crypto helper: {e}")))?;
    }
    child
        .wait_with_output()
        .map_err(|e| bridge_error(&format!("the crypto helper didn't finish: {e}")))
}

// ──────────────────────────────────────────────
// Diagnostics
// ──────────────────────────────────────────────

/// E1248 — `jet registry keygen` refused because a key already exists.
pub fn e1248(path: &Path) -> Diagnostic {
    Diagnostic::error(
        "E1248",
        format!(
            "`jet registry keygen` refused: a signing key already exists at `{}`",
            path.display()
        ),
        "overwriting it would orphan every package you've published under the old key — consumers \
         who pinned it (TOFU) would see a key-rotation warning on your next publish."
            .to_string(),
        "use `jet registry keygen --force` if you're sure (e.g. the old key was compromised), or back it \
         up first with `jet registry key backup`."
            .to_string(),
        None,
    )
}

/// E1292 — package-signing key generation could not obtain OS entropy.
pub fn e1292() -> Diagnostic {
    Diagnostic::error(
        "E1292",
        "signing key generation needs cryptographic randomness".to_string(),
        "the operating system could not provide cryptographic randomness".to_string(),
        "retry as a new operation on a supported host; no key files were created".to_string(),
        None,
    )
}

/// D-CRYPTO-KEYGEN-DIAG1's command frame has a separate headline and What
/// line, so it is rendered here instead of through the source-span renderer.
pub fn render_e1292() -> &'static str {
    concat!(
        "Error [E1292]: signing key generation needs cryptographic randomness\n",
        " What: Jet could not create the package-signing key\n",
        " Why: the operating system could not provide cryptographic randomness\n",
        " Fix: retry as a new operation on a supported host; no key files were created\n",
    )
}

/// Internal helper/bridge failure. Reuses E0704 (foreign-crate bridge build
/// failure) — the crypto helper *is* a bridge target, so the code is apt and no
/// new diagnostic is minted.
fn bridge_error(msg: &str) -> Diagnostic {
    Diagnostic::error(
        "E0704",
        format!("couldn't run the Ed25519 signing helper: {msg}"),
        "package signing shells out to a hidden bridge binary built with `cargo`".to_string(),
        "check that `cargo` is installed (https://rustup.rs) and try again".to_string(),
        None,
    )
}

fn io_error(what: &str, e: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "E0704",
        format!("couldn't write the {what}: {e}"),
        "signing keys live under `~/.jet/keys` (or `$JET_KEYS_DIR`)".to_string(),
        "check disk permissions and try again".to_string(),
        None,
    )
}

fn volatile_zeroize(bytes: &mut [u8]) {
    for byte in bytes {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

// ──────────────────────────────────────────────
// Encodings (I6: std-only, no external crates)
// ──────────────────────────────────────────────

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

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | (b[2] as u32);
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some((c - b'A') as u32),
            b'a'..=b'z' => Some((c - b'a' + 26) as u32),
            b'0'..=b'9' => Some((c - b'0' + 52) as u32),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let s = s.as_bytes();
    if s.len() % 4 != 0 {
        return Err("base64 length not a multiple of 4".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    for chunk in s.chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            let v = if c == b'=' {
                0
            } else {
                val(c).ok_or("invalid base64 character")?
            };
            n |= v << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    if base64_encode(&out).as_bytes() != s {
        return Err("non-canonical base64 encoding".to_string());
    }
    Ok(out)
}

// ──────────────────────────────────────────────
// File permissions
// ──────────────────────────────────────────────

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

// ──────────────────────────────────────────────
// Tests (pure encoders — no bridge needed)
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrips() {
        let data = [0u8, 1, 2, 250, 255, 16, 128];
        let h = hex_encode(&data);
        assert_eq!(h, "000102faff1080");
        assert_eq!(hex_decode(&h).unwrap(), data);
    }

    #[test]
    fn base64_roundtrips_all_pad_lengths() {
        for input in [
            &b""[..],
            &b"f"[..],
            &b"fo"[..],
            &b"foo"[..],
            &b"foob"[..],
            &b"fooba"[..],
            &b"foobar"[..],
        ] {
            let enc = base64_encode(input);
            assert_eq!(
                base64_decode(&enc).unwrap(),
                input,
                "roundtrip for {input:?}"
            );
        }
        // Known vector.
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn keys_dir_honors_env_override() {
        std::env::set_var("JET_KEYS_DIR", "/tmp/jet_keys_test_dir");
        assert_eq!(keys_dir(), PathBuf::from("/tmp/jet_keys_test_dir"));
        std::env::remove_var("JET_KEYS_DIR");
    }

    #[test]
    fn e1292_command_frame_is_exact() {
        assert_eq!(
            render_e1292(),
            include_str!("../../tests/fixtures/jetpack-diagnostics/keygen_entropy_unavailable.stderr")
        );
        let diagnostic = e1292();
        assert_eq!(diagnostic.code, "E1292");
    }

    #[test]
    fn secret_temporary_zeroization_clears_every_byte() {
        let mut secret = vec![0xa5; 96];
        volatile_zeroize(&mut secret);
        assert_eq!(secret, vec![0; 96]);
    }
}
