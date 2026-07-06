// core.vault runtime (U13, D-JPK-SECRETCRYPTO1=A) — age-style crypto:
// X25519 recipients, ChaCha20-Poly1305 payload, via the `age` crate.
//
// This file is emitted verbatim into the hidden FFI bridge crate (see
// Source/FFI.rs) when a Jet program uses `core.vault`, and also backs the
// `jet-secrets-helper` binary that `jetpack secrets set/get/recipients/keygen`
// shells out to (jetpack itself stays I6-zero-dependency). The compiler crate
// (`Source/`, `crates/jet-driver`) never depends on `age`; only this text does.
//
// Wire format for the decrypted store payload: a flat, hand-rolled
// tagged-length encoding of `[(String, String)]` name/value pairs — no JSON
// crate, no escaping edge cases (mirrors `jet.db`'s own tagged-length wire
// convention for the same "two independently-built crates can't share Rust
// types" reason).

use age::secrecy::ExposeSecret;
use std::io::{Read, Write};
use std::str::FromStr;

/// Generate a fresh X25519 identity/recipient pair. Returns
/// `(identity_string, recipient_string)` — the `AGE-SECRET-KEY-1...` private
/// form (kept out of the repo, `~/.jet/keys/secrets.identity`) and the
/// `age1...` public form (committed to the repo's recipients file).
pub fn jet_vault_keygen_impl() -> (String, String) {
    let identity = age::x25519::Identity::generate();
    let recipient = identity.to_public();
    (identity.to_string().expose_secret().to_string(), recipient.to_string())
}

/// Encrypt `plaintext` to every recipient in `recipients` (each an `age1...`
/// string). Returns the binary age ciphertext.
pub fn jet_vault_encrypt_impl(recipients: &Vec<String>, plaintext: &Vec<u8>) -> Result<Vec<u8>, String> {
    if recipients.is_empty() {
        return Err("no recipients — add at least one with `jetpack secrets recipients add`".to_string());
    }
    let mut parsed: Vec<Box<dyn age::Recipient + Send>> = Vec::with_capacity(recipients.len());
    for r in recipients {
        let rec = age::x25519::Recipient::from_str(r.trim())
            .map_err(|e| format!("bad recipient `{r}`: {e}"))?;
        parsed.push(Box::new(rec));
    }
    let encryptor = age::Encryptor::with_recipients(parsed)
        .ok_or_else(|| "couldn't build encryptor: no valid recipients".to_string())?;
    let mut out = Vec::new();
    let mut writer = encryptor
        .wrap_output(&mut out)
        .map_err(|e| format!("couldn't start encryption: {e}"))?;
    writer
        .write_all(plaintext)
        .map_err(|e| format!("encryption write failed: {e}"))?;
    writer.finish().map_err(|e| format!("encryption finalize failed: {e}"))?;
    Ok(out)
}

/// Decrypt `ciphertext` with `identity` (an `AGE-SECRET-KEY-1...` string).
/// Returns the plaintext bytes, or an error string (wrong key, corrupt data).
pub fn jet_vault_decrypt_impl(identity: &String, ciphertext: &Vec<u8>) -> Result<Vec<u8>, String> {
    let identity = age::x25519::Identity::from_str(identity.trim())
        .map_err(|e| format!("bad identity: {e}"))?;
    let decryptor = age::Decryptor::new(&ciphertext[..])
        .map_err(|e| format!("not a valid age file: {e}"))?;
    let age::Decryptor::Recipients(decryptor) = decryptor else {
        return Err("not a recipients-based age file (passphrase-encrypted?)".to_string());
    };
    let mut out = Vec::new();
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|e| format!("decryption failed (wrong key or corrupted store): {e}"))?;
    reader
        .read_to_end(&mut out)
        .map_err(|e| format!("decryption read failed: {e}"))?;
    Ok(out)
}

// ── Tagged-length wire format for the decrypted `[(name, value)]` store ────

/// Encode `pairs` as `count:u32-le, (name_len:u32-le, name-bytes, value_len:u32-le, value-bytes)*`.
pub fn jet_vault_encode_pairs(pairs: &[(String, String)]) -> Vec<u8> {
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

/// Decode the wire format `jet_vault_encode_pairs` produces. `None` on any
/// malformed input (truncated length prefix, bad UTF-8, trailing garbage).
pub fn jet_vault_decode_pairs(bytes: &[u8]) -> Option<Vec<(String, String)>> {
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
        return None; // trailing garbage
    }
    Some(out)
}

/// The `.jet/secrets.age` path, relative to the current working directory —
/// the same "project-relative" convention `core.files` uses.
fn store_path() -> std::path::PathBuf {
    std::path::PathBuf::from(".jet").join("secrets.age")
}

/// The local decrypt identity: `$JET_KEYS_DIR/secrets.identity` if set, else
/// `~/.jet/keys/secrets.identity` — same directory `Source/Publish/Sign.rs`
/// uses for signing keys, just a different file name.
fn identity_path() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("JET_KEYS_DIR") {
        if !dir.is_empty() {
            return std::path::PathBuf::from(dir).join("secrets.identity");
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home)
        .join(".jet")
        .join("keys")
        .join("secrets.identity")
}

/// `core.vault.get(name)` — the whole read path a compiled Jet program uses:
/// read the local identity, read the project's encrypted store, decrypt,
/// look up `name`. `None` on any failure (no identity, no store, wrong key,
/// missing entry) — the same "may be missing" shape as `core.env.get`, never
/// a panic (I2: no path here may crash the program in place of a value).
pub fn jet_vault_get_impl(name: &str) -> Option<String> {
    let identity = std::fs::read_to_string(identity_path()).ok()?;
    let ciphertext = std::fs::read(store_path()).ok()?;
    let plaintext = jet_vault_decrypt_impl(&identity, &ciphertext).ok()?;
    let pairs = jet_vault_decode_pairs(&plaintext)?;
    pairs.into_iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keygen_encrypt_decrypt_round_trips() {
        let (identity, recipient) = jet_vault_keygen_impl();
        let plaintext = b"hunter2".to_vec();
        let ciphertext = jet_vault_encrypt_impl(&vec![recipient], &plaintext).unwrap();
        assert_ne!(ciphertext, plaintext, "ciphertext must not equal plaintext");
        let decrypted = jet_vault_decrypt_impl(&identity, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_identity_fails_to_decrypt() {
        let (_, recipient) = jet_vault_keygen_impl();
        let (other_identity, _) = jet_vault_keygen_impl();
        let ciphertext = jet_vault_encrypt_impl(&vec![recipient], &b"secret".to_vec()).unwrap();
        assert!(jet_vault_decrypt_impl(&other_identity, &ciphertext).is_err());
    }

    #[test]
    fn pairs_wire_round_trips() {
        let pairs = vec![
            ("db_password".to_string(), "hunter2".to_string()),
            ("api_key".to_string(), "abc123".to_string()),
        ];
        let bytes = jet_vault_encode_pairs(&pairs);
        assert_eq!(jet_vault_decode_pairs(&bytes), Some(pairs));
    }

    #[test]
    fn pairs_wire_rejects_truncated_input() {
        let pairs = vec![("k".to_string(), "v".to_string())];
        let mut bytes = jet_vault_encode_pairs(&pairs);
        bytes.truncate(bytes.len() - 1);
        assert_eq!(jet_vault_decode_pairs(&bytes), None);
    }
}
