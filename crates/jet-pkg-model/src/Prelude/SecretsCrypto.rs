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
use std::io::{Read as _, Write as _};
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
    let (store, _, _, _) = vault_read_at(std::path::Path::new(".")).ok()?;
    store.strings.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone())
}

// ── Typed key vault (D-CRYPTO-VAULT1=A) ─────────────────────────────

const JVLT_MAGIC: &[u8; 4] = b"JVLT";
const JVLT_VERSION: u8 = 2;
const JVLT_MAX: usize = 16 * 1024 * 1024;
const JVLT_MAX_STRINGS: usize = 4096;
const JVLT_MAX_NAMES: usize = 1024;
const JVLT_MAX_VERSIONS: usize = 8192;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum JetVaultKeyStatus { Active = 1, Retired = 2, Revoked = 3 }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetVaultError {
    InvalidName,
    NotFound,
    WrongType,
    Revoked,
    Locked,
    AuthorityDenied,
    Conflict,
    UnsupportedProvider,
    InvalidEncoding,
    DurabilityUnknown,
    Crypto(JetCryptoError),
    Io { operation: &'static str, redacted_path: &'static str },
    Internal { incident_id: &'static str },
}
impl std::fmt::Display for JetVaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidName => f.write_str("invalid vault key name"),
            Self::NotFound => f.write_str("vault key not found"),
            Self::WrongType => f.write_str("vault key has a different type"),
            Self::Revoked => f.write_str("vault key is revoked"),
            Self::Locked => f.write_str("vault is locked"),
            Self::AuthorityDenied => f.write_str("vault write authority denied"),
            Self::Conflict => f.write_str("vault changed since this write was prepared"),
            Self::UnsupportedProvider => f.write_str("vault provider is unsupported on this platform"),
            Self::InvalidEncoding => f.write_str("vault store has invalid encoding"),
            Self::DurabilityUnknown => f.write_str("vault write visibility is known but durability is uncertain"),
            Self::Crypto(error) => write!(f, "vault cryptography failed: {error}"),
            Self::Io { operation, redacted_path } => write!(f, "vault {operation} failed at {redacted_path}"),
            Self::Internal { incident_id } => write!(f, "internal vault failure; incident {incident_id}"),
        }
    }
}

mod jet_vault_sealed { pub trait Sealed {} }
pub trait JetVaultKey: jet_vault_sealed::Sealed + Sized + 'static {
    const TAG: u8;
    const NAME: &'static str;
    fn generate() -> Result<Self, JetVaultError>;
    fn into_bytes(self) -> Vec<u8>;
    fn from_bytes(bytes: Vec<u8>) -> Result<Self, JetVaultError>;
    fn public_bytes(&self) -> Vec<u8>;
}
impl jet_vault_sealed::Sealed for JetSigningKey {}
impl JetVaultKey for JetSigningKey {
    const TAG: u8 = 1;
    const NAME: &'static str = "SigningKey";
    fn generate() -> Result<Self, JetVaultError> { jet_crypto_signing_generate_impl().map_err(JetVaultError::Crypto) }
    fn into_bytes(mut self) -> Vec<u8> { std::mem::take(&mut self.0) }
    fn from_bytes(bytes: Vec<u8>) -> Result<Self, JetVaultError> {
        if bytes.len() != 32 { return Err(JetVaultError::InvalidEncoding); }
        Ok(JetSigningKey(bytes))
    }
    fn public_bytes(&self) -> Vec<u8> { jet_crypto_signing_public_impl(self).0.to_vec() }
}
impl jet_vault_sealed::Sealed for JetX25519SecretKey {}
impl JetVaultKey for JetX25519SecretKey {
    const TAG: u8 = 2;
    const NAME: &'static str = "X25519SecretKey";
    fn generate() -> Result<Self, JetVaultError> { jet_crypto_x25519_generate_impl().map_err(JetVaultError::Crypto) }
    fn into_bytes(mut self) -> Vec<u8> { std::mem::take(&mut self.0) }
    fn from_bytes(bytes: Vec<u8>) -> Result<Self, JetVaultError> {
        if bytes.len() != 32 { return Err(JetVaultError::InvalidEncoding); }
        Ok(JetX25519SecretKey(bytes))
    }
    fn public_bytes(&self) -> Vec<u8> { jet_crypto_x25519_public_typed_impl(self).0.to_vec() }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum JetVaultProvider { Repo }

pub struct JetVaultKeyRef<T> {
    provider: JetVaultProvider,
    repo_uuid: [u8; 16],
    name: String,
    generation: u64,
    opaque_id: [u8; 16],
    record_hash: [u8; 32],
    marker: std::marker::PhantomData<fn() -> T>,
}
impl<T> Clone for JetVaultKeyRef<T> { fn clone(&self) -> Self { Self { provider: self.provider, repo_uuid: self.repo_uuid, name: self.name.clone(), generation: self.generation, opaque_id: self.opaque_id, record_hash: self.record_hash, marker: std::marker::PhantomData } } }
impl<T> PartialEq for JetVaultKeyRef<T> { fn eq(&self, other: &Self) -> bool { self.provider == other.provider && self.repo_uuid == other.repo_uuid && self.name == other.name && self.generation == other.generation && self.opaque_id == other.opaque_id && self.record_hash == other.record_hash } }
impl<T> Eq for JetVaultKeyRef<T> {}
impl<T> std::hash::Hash for JetVaultKeyRef<T> { fn hash<H: std::hash::Hasher>(&self, state: &mut H) { self.provider.hash(state); self.repo_uuid.hash(state); self.name.hash(state); self.generation.hash(state); self.opaque_id.hash(state); self.record_hash.hash(state); } }
impl<T> std::fmt::Display for JetVaultKeyRef<T> { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "repo:{}@v{}", self.name, self.generation) } }
impl<T> std::fmt::Debug for JetVaultKeyRef<T> { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "repo:{}@v{}#", self.name, self.generation)?; for byte in &self.opaque_id[..4] { write!(f, "{byte:02x}")?; } Ok(()) } }
impl<T> JetVaultKeyRef<T> { pub fn generation(&self) -> u64 { self.generation } }

pub struct JetVaultRotation<T> { pub previous: JetVaultKeyRef<T>, pub current: JetVaultKeyRef<T> }
impl<T> Clone for JetVaultRotation<T> { fn clone(&self) -> Self { Self { previous: self.previous.clone(), current: self.current.clone() } } }
impl<T> PartialEq for JetVaultRotation<T> { fn eq(&self, other:&Self)->bool { self.previous==other.previous && self.current==other.current } }
impl<T> Eq for JetVaultRotation<T> {}
impl<T> std::fmt::Debug for JetVaultRotation<T> { fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result { f.debug_struct("Rotation").field("previous",&self.previous).field("current",&self.current).finish() } }

#[derive(Eq, PartialEq)]
pub struct JetVaultStore {
    pub repo_uuid: [u8; 16],
    pub revision: u64,
    pub strings: Vec<(String, String)>,
    keys: Vec<JetVaultRecord>,
}
impl JetVaultStore { pub fn new(repo_uuid: [u8; 16]) -> Self { Self { repo_uuid, revision: 0, strings: Vec::new(), keys: Vec::new() } } }
impl std::fmt::Debug for JetVaultStore{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.debug_struct("VaultStore").field("repository_uuid",&self.repo_uuid).field("revision",&self.revision).field("string_count",&self.strings.len()).field("key_version_count",&self.keys.len()).finish()}}
impl Drop for JetVaultStore{fn drop(&mut self){for(_,value)in &mut self.strings{unsafe{zeroize(value.as_mut_vec())}}}}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum JetVaultProvenance { Generated = 1, Imported = 2 }

#[derive(Clone, Debug, Eq, PartialEq)]
struct JetVaultOrigin {
    repo_uuid: [u8; 16],
    name: String,
    generation: u64,
    opaque_id: [u8; 16],
    record_hash: [u8; 32],
}

#[derive(Eq, PartialEq)]
struct JetVaultRecord {
    name: String,
    key_type: u8,
    generation: u64,
    status: JetVaultKeyStatus,
    provenance: JetVaultProvenance,
    opaque_id: [u8; 16],
    created_unix_ms: u64,
    status_unix_ms: u64,
    record_hash: [u8; 32],
    public_key_hash: [u8; 32],
    reason_hash: [u8; 32],
    origin: Option<JetVaultOrigin>,
    key: Vec<u8>,
}
impl std::fmt::Debug for JetVaultRecord{fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.debug_struct("VaultRecord").field("name",&self.name).field("key_type",&self.key_type).field("generation",&self.generation).field("status",&self.status).field("provenance",&self.provenance).field("opaque_id",&self.opaque_id).field("created_unix_ms",&self.created_unix_ms).field("status_unix_ms",&self.status_unix_ms).field("record_hash",&self.record_hash).field("public_key_hash",&self.public_key_hash).field("reason_hash",&self.reason_hash).field("origin",&self.origin).field("key",&"<redacted>").finish()}}
impl Drop for JetVaultRecord{fn drop(&mut self){zeroize(&mut self.key)}}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum JetVaultAction { Generate, Store, Rotate, Retire, Revoke, RawImport }
impl JetVaultAction { fn byte(self) -> u8 { match self { Self::Generate => 1, Self::Store => 2, Self::Rotate => 3, Self::Retire => 4, Self::Revoke => 5, Self::RawImport => 6 } } fn text(self) -> &'static str { match self { Self::Generate => "generate", Self::Store => "store", Self::Rotate => "rotate", Self::Retire => "retire", Self::Revoke => "revoke", Self::RawImport => "raw import" } } }

pub struct JetVaultMutationPlan<T> {
    action: JetVaultAction,
    name: String,
    target: Option<JetVaultKeyRef<T>>,
    imported: Option<Zeroizing<Vec<u8>>>,
    reason: String,
    repo_uuid: [u8; 16],
    start_revision: u64,
    start_hash: [u8; 32],
    provider_hash: [u8; 32],
    public_key_hash: [u8; 32],
    key_digest: [u8; 32],
    transition_from: u64,
    transition_to: u64,
    start_kind: VaultStartKind,
    operation_hash: [u8; 32],
    issued: std::time::Instant,
    deadline: std::time::Instant,
    expires_unix_ms: u64,
}
pub struct JetVaultWrite<T> { operation_hash: [u8; 32], preview_hash: [u8; 32], user: u32, session: i32, deadline: std::time::Instant, marker: std::marker::PhantomData<fn() -> T> }
impl<T> std::fmt::Debug for JetVaultWrite<T> { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("VaultWrite(<redacted>)") } }

fn vault_hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in parts { hash.update(part); }
    hash.finalize().into()
}
fn vault_random<const N: usize>() -> Result<[u8; N], JetVaultError> {
    let mut out = [0u8; N];
    jet_crypto_entropy_fill(&mut out).map_err(|_| JetVaultError::Crypto(JetCryptoError::EntropyUnavailable))?;
    Ok(out)
}

pub fn jet_vault_validate_name(name: &str) -> Result<(), JetVaultError> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || jet_vault_nfc(name) != name || name.trim() != name || name == "." || name == ".."
        || name.contains('/') || name.contains('\\') || name.chars().any(char::is_control)
    { return Err(JetVaultError::InvalidName); }
    Ok(())
}
fn vault_validate_reason(reason:&str)->Result<(),JetVaultError>{let bytes=reason.as_bytes();if bytes.is_empty()||bytes.len()>256||jet_vault_nfc(reason)!=reason||reason.trim()!=reason||reason.chars().any(|c|c.is_control()||matches!(c,'\u{2028}'|'\u{2029}')){Err(JetVaultError::InvalidName)}else{Ok(())}}

fn push_u16(out: &mut Vec<u8>, value: usize) -> Result<(), JetVaultError> { out.extend_from_slice(&u16::try_from(value).map_err(|_| JetVaultError::InvalidEncoding)?.to_le_bytes()); Ok(()) }
fn push_u32(out: &mut Vec<u8>, value: usize) -> Result<(), JetVaultError> { out.extend_from_slice(&u32::try_from(value).map_err(|_| JetVaultError::InvalidEncoding)?.to_le_bytes()); Ok(()) }
fn take<'a>(bytes: &'a [u8], at: &mut usize, count: usize) -> Result<&'a [u8], JetVaultError> { let end = at.checked_add(count).ok_or(JetVaultError::InvalidEncoding)?; let value = bytes.get(*at..end).ok_or(JetVaultError::InvalidEncoding)?; *at = end; Ok(value) }
fn take_u8(bytes: &[u8], at: &mut usize) -> Result<u8, JetVaultError> { Ok(take(bytes, at, 1)?[0]) }
fn take_u16(bytes: &[u8], at: &mut usize) -> Result<usize, JetVaultError> { Ok(u16::from_le_bytes(take(bytes, at, 2)?.try_into().unwrap()) as usize) }
fn take_u32(bytes: &[u8], at: &mut usize) -> Result<usize, JetVaultError> { Ok(u32::from_le_bytes(take(bytes, at, 4)?.try_into().unwrap()) as usize) }
fn take_u64(bytes: &[u8], at: &mut usize) -> Result<u64, JetVaultError> { Ok(u64::from_le_bytes(take(bytes, at, 8)?.try_into().unwrap())) }
fn take_array<const N: usize>(bytes: &[u8], at: &mut usize) -> Result<[u8; N], JetVaultError> { take(bytes, at, N)?.try_into().map_err(|_| JetVaultError::InvalidEncoding) }
fn take_string_u16(bytes: &[u8], at: &mut usize, max: usize) -> Result<String, JetVaultError> { let len = take_u16(bytes, at)?; if len == 0 || len > max { return Err(JetVaultError::InvalidEncoding); } String::from_utf8(take(bytes, at, len)?.to_vec()).map_err(|_| JetVaultError::InvalidEncoding) }
fn take_string_u32(bytes: &[u8], at: &mut usize, max: usize) -> Result<String, JetVaultError> { let len = take_u32(bytes, at)?; if len > max { return Err(JetVaultError::InvalidEncoding); } String::from_utf8(take(bytes, at, len)?.to_vec()).map_err(|_| JetVaultError::InvalidEncoding) }

fn vault_public_hash(key_type: u8, key: &[u8]) -> Result<[u8; 32], JetVaultError> {
    let public = match key_type {
        1 => JetSigningKey::from_bytes(key.to_vec())?.public_bytes(),
        2 => JetX25519SecretKey::from_bytes(key.to_vec())?.public_bytes(),
        _ => return Err(JetVaultError::InvalidEncoding),
    };
    Ok(vault_hash(&[b"JVLT2 public key", &[key_type], &public]))
}
fn vault_key_digest(key_type: u8, key: &[u8]) -> [u8; 32] { vault_hash(&[b"JVLT2 key bytes", &[key_type], key]) }
fn vault_reason_hash(status: JetVaultKeyStatus, reason: &str) -> Result<[u8; 32], JetVaultError> {
    if status == JetVaultKeyStatus::Active { return if reason.is_empty() { Ok([0; 32]) } else { Err(JetVaultError::InvalidEncoding) }; }
    vault_validate_reason(reason)?;
    Ok(vault_hash(&[b"JVLT2 status reason", &[status as u8], &(reason.len() as u16).to_le_bytes(), reason.as_bytes()]))
}
fn vault_record_hash(record: &JetVaultRecord) -> [u8; 32] {
    let key_digest = vault_key_digest(record.key_type, &record.key);
    let (present, repo, origin_name, generation, opaque, hash) = match &record.origin {
        Some(origin) => (1u8, origin.repo_uuid, origin.name.as_bytes(), origin.generation, origin.opaque_id, origin.record_hash),
        None => (0u8, [0; 16], &[][..], 0, [0; 16], [0; 32]),
    };
    vault_hash(&[
        b"JVLT2 immutable record", &[record.key_type], &[record.provenance as u8],
        &(record.name.len() as u16).to_le_bytes(), record.name.as_bytes(), &record.generation.to_le_bytes(),
        &record.opaque_id, &record.created_unix_ms.to_le_bytes(), &record.public_key_hash, &key_digest,
        &[present], &repo, &(origin_name.len() as u16).to_le_bytes(), origin_name,
        &generation.to_le_bytes(), &opaque, &hash,
    ])
}

pub fn jet_vault_encode_v2(store: &JetVaultStore) -> Result<Vec<u8>, JetVaultError> {
    if store.revision == 0 || store.repo_uuid == [0; 16] || store.strings.len() > JVLT_MAX_STRINGS || store.keys.len() > JVLT_MAX_VERSIONS { return Err(JetVaultError::InvalidEncoding); }
    let mut names: Vec<(u8, String, u64, u64)> = Vec::new();
    let mut grouped: Vec<(u8, String)> = store.keys.iter().map(|r| (r.key_type, r.name.clone())).collect();
    grouped.sort(); grouped.dedup();
    for (tag, name) in grouped {
        let mut rows: Vec<_> = store.keys.iter().filter(|r| r.key_type == tag && r.name == name).collect();
        rows.sort_by_key(|record| record.generation);
        let latest = rows.iter().map(|r| r.generation).max().unwrap_or(0);
        let active: Vec<_> = rows.iter().filter(|r| r.status == JetVaultKeyStatus::Active).collect();
        let current = active.first().map(|r| r.generation).unwrap_or(0);
        if rows.len() != latest as usize || active.len() > 1 || rows.iter().enumerate().any(|(i,r)| r.generation != i as u64 + 1) || (current != 0 && current != latest) { return Err(JetVaultError::InvalidEncoding); }
        names.push((tag, name, current, latest));
    }
    if names.len() > JVLT_MAX_NAMES { return Err(JetVaultError::InvalidEncoding); }
    let mut out = Vec::new();
    out.extend_from_slice(JVLT_MAGIC); out.push(JVLT_VERSION); out.push(0); out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&store.repo_uuid); out.extend_from_slice(&store.revision.to_le_bytes());
    push_u32(&mut out, store.strings.len())?; push_u32(&mut out, names.len())?; push_u32(&mut out, store.keys.len())?;
    let mut strings:Vec<_> = store.strings.iter().collect(); strings.sort_by(|a,b| a.0.cmp(&b.0));
    let mut prior_string = None;
    for (name, value) in strings { jet_vault_validate_name(name)?; if prior_string.as_ref().is_some_and(|prior: &String| prior >= name) { return Err(JetVaultError::InvalidEncoding); } prior_string=Some(name.clone()); push_u16(&mut out, name.len())?; out.extend_from_slice(name.as_bytes()); if value.len() > 1024 * 1024 { return Err(JetVaultError::InvalidEncoding); } push_u32(&mut out, value.len())?; out.extend_from_slice(value.as_bytes()); }
    for (tag, name, current, latest) in &names { if !matches!(tag,1|2) { return Err(JetVaultError::InvalidEncoding); } jet_vault_validate_name(name)?; out.push(*tag); push_u16(&mut out, name.len())?; out.extend_from_slice(name.as_bytes()); out.extend_from_slice(&current.to_le_bytes()); out.extend_from_slice(&latest.to_le_bytes()); }
    let mut records:Vec<_> = store.keys.iter().collect();
    records.sort_by(|a,b| (a.key_type, a.name.as_bytes(), a.generation).cmp(&(b.key_type, b.name.as_bytes(), b.generation)));
    let mut opaque_ids = std::collections::HashSet::new();
    for record in records {
        jet_vault_validate_name(&record.name)?; if record.generation == 0 || record.key.len()!=32 || record.opaque_id==[0;16] || !opaque_ids.insert(record.opaque_id) || record.public_key_hash != vault_public_hash(record.key_type,&record.key)? || record.record_hash != vault_record_hash(&record) || (record.status==JetVaultKeyStatus::Active && (record.status_unix_ms!=record.created_unix_ms || record.reason_hash != [0;32])) { return Err(JetVaultError::InvalidEncoding); }
        out.push(record.key_type); out.push(record.status as u8); out.push(record.provenance as u8); out.push(0); push_u16(&mut out,record.name.len())?;out.extend_from_slice(record.name.as_bytes());out.extend_from_slice(&record.generation.to_le_bytes());out.extend_from_slice(&record.opaque_id);out.extend_from_slice(&record.created_unix_ms.to_le_bytes());out.extend_from_slice(&record.status_unix_ms.to_le_bytes());out.extend_from_slice(&record.reason_hash);out.extend_from_slice(&record.public_key_hash);
        match &record.origin { Some(origin)=>{if record.provenance!=JetVaultProvenance::Imported||origin.repo_uuid==[0;16]||origin.opaque_id==[0;16]||origin.record_hash==[0;32]||origin.generation==0{return Err(JetVaultError::InvalidEncoding)}jet_vault_validate_name(&origin.name)?;out.push(1);out.extend_from_slice(&origin.repo_uuid);push_u16(&mut out,origin.name.len())?;out.extend_from_slice(origin.name.as_bytes());out.extend_from_slice(&origin.generation.to_le_bytes());out.extend_from_slice(&origin.opaque_id);out.extend_from_slice(&origin.record_hash)},None=>{out.push(0);out.extend_from_slice(&[0;16]);out.extend_from_slice(&0u16.to_le_bytes());out.extend_from_slice(&0u64.to_le_bytes());out.extend_from_slice(&[0;16]);out.extend_from_slice(&[0;32]);} }
        push_u16(&mut out, record.key.len())?; out.extend_from_slice(&record.key);
    }
    let payload_hash = vault_hash(&[b"JVLT2 payload", &out]); out.extend_from_slice(&payload_hash);
    if out.len() > JVLT_MAX { return Err(JetVaultError::InvalidEncoding); }
    Ok(out)
}

pub fn jet_vault_decode_v2(bytes: &[u8]) -> Result<JetVaultStore, JetVaultError> {
    if bytes.len() > JVLT_MAX || bytes.len() < 4 + 1 + 1 + 2 + 16 + 8 + 12 + 32 { return Err(JetVaultError::InvalidEncoding); }
    let (payload, encoded_hash) = bytes.split_at(bytes.len() - 32);
    if encoded_hash != vault_hash(&[b"JVLT2 payload", payload]) { return Err(JetVaultError::InvalidEncoding); }
    let mut at = 0;
    if take(payload, &mut at, 4)? != JVLT_MAGIC || take_u8(payload, &mut at)? != JVLT_VERSION || take_u8(payload, &mut at)? != 0 || take_u16(payload, &mut at)? != 0 { return Err(JetVaultError::InvalidEncoding); }
    let repo_uuid = take_array(payload, &mut at)?; let revision = take_u64(payload, &mut at)?; if repo_uuid==[0;16]||revision==0{return Err(JetVaultError::InvalidEncoding)}
    let string_count = take_u32(payload, &mut at)?; let name_count = take_u32(payload, &mut at)?; let version_count = take_u32(payload, &mut at)?;
    if string_count > JVLT_MAX_STRINGS || name_count > JVLT_MAX_NAMES || version_count > JVLT_MAX_VERSIONS { return Err(JetVaultError::InvalidEncoding); }
    let mut strings = Vec::with_capacity(string_count); let mut prior = None;
    for _ in 0..string_count { let name = take_string_u16(payload, &mut at, 128)?; jet_vault_validate_name(&name)?; if prior.as_ref().is_some_and(|p: &String| p >= &name) { return Err(JetVaultError::InvalidEncoding); } prior = Some(name.clone()); let value = take_string_u32(payload, &mut at, 1024 * 1024)?; strings.push((name, value)); }
    let mut names = Vec::with_capacity(name_count); let mut prior_name: Option<(u8,String)> = None;
    for _ in 0..name_count { let tag=take_u8(payload,&mut at)?;let name=take_string_u16(payload,&mut at,128)?;jet_vault_validate_name(&name)?;if !matches!(tag,1|2)||prior_name.as_ref().is_some_and(|p|p>=&(tag,name.clone())){return Err(JetVaultError::InvalidEncoding)}let current=take_u64(payload,&mut at)?;let latest=take_u64(payload,&mut at)?;if latest==0||(current!=0&&current!=latest){return Err(JetVaultError::InvalidEncoding)}prior_name=Some((tag,name.clone()));names.push((tag,name,current,latest));}
    let mut keys = Vec::with_capacity(version_count); let mut prior_key: Option<(u8,String,u64)> = None;let mut opaque_ids=std::collections::HashSet::new();
    for _ in 0..version_count {
        let key_type=take_u8(payload,&mut at)?;let status=match take_u8(payload,&mut at)?{1=>JetVaultKeyStatus::Active,2=>JetVaultKeyStatus::Retired,3=>JetVaultKeyStatus::Revoked,_=>return Err(JetVaultError::InvalidEncoding)};let provenance=match take_u8(payload,&mut at)?{1=>JetVaultProvenance::Generated,2=>JetVaultProvenance::Imported,_=>return Err(JetVaultError::InvalidEncoding)};if take_u8(payload,&mut at)?!=0{return Err(JetVaultError::InvalidEncoding)}let name=take_string_u16(payload,&mut at,128)?;jet_vault_validate_name(&name)?;let generation=take_u64(payload,&mut at)?;if generation==0||prior_key.as_ref().is_some_and(|p|p>=&(key_type,name.clone(),generation)){return Err(JetVaultError::InvalidEncoding)}prior_key=Some((key_type,name.clone(),generation));
        let opaque_id=take_array(payload,&mut at)?;let created_unix_ms=take_u64(payload,&mut at)?;let status_unix_ms=take_u64(payload,&mut at)?;let reason_hash=take_array(payload,&mut at)?;let public_key_hash=take_array(payload,&mut at)?;let origin_present=take_u8(payload,&mut at)?;let origin_repo=take_array(payload,&mut at)?;let origin_len=take_u16(payload,&mut at)?;let origin_name=String::from_utf8(take(payload,&mut at,origin_len)?.to_vec()).map_err(|_|JetVaultError::InvalidEncoding)?;let origin_generation=take_u64(payload,&mut at)?;let origin_opaque=take_array(payload,&mut at)?;let origin_hash=take_array(payload,&mut at)?;
        let origin=match origin_present{0 if origin_repo==[0;16]&&origin_len==0&&origin_generation==0&&origin_opaque==[0;16]&&origin_hash==[0;32]=>None,1 if provenance==JetVaultProvenance::Imported&&origin_repo!=[0;16]&&origin_generation>0&&origin_opaque!=[0;16]&&origin_hash!=[0;32]=>{jet_vault_validate_name(&origin_name)?;Some(JetVaultOrigin{repo_uuid:origin_repo,name:origin_name,generation:origin_generation,opaque_id:origin_opaque,record_hash:origin_hash})},_=>return Err(JetVaultError::InvalidEncoding)};
        let key_len=take_u16(payload,&mut at)?;if key_len!=32{return Err(JetVaultError::InvalidEncoding)}let key=take(payload,&mut at,key_len)?.to_vec();let mut record=JetVaultRecord{name,key_type,generation,status,provenance,opaque_id,created_unix_ms,status_unix_ms,record_hash:[0;32],public_key_hash,reason_hash,origin,key};if !matches!(key_type,1|2)||opaque_id==[0;16]||!opaque_ids.insert(opaque_id)||created_unix_ms==0||status_unix_ms<created_unix_ms||public_key_hash!=vault_public_hash(key_type,&record.key)?||(status==JetVaultKeyStatus::Active&&(reason_hash!=[0;32]||status_unix_ms!=created_unix_ms))||(status!=JetVaultKeyStatus::Active&&reason_hash==[0;32]){return Err(JetVaultError::InvalidEncoding)}record.record_hash=vault_record_hash(&record);keys.push(record);
    }
    if at != payload.len() { return Err(JetVaultError::InvalidEncoding); }
    for (tag,name,current,latest) in &names { let rows:Vec<_>=keys.iter().filter(|r|r.key_type==*tag&&r.name==*name).collect();if rows.len()!=*latest as usize||rows.iter().enumerate().any(|(i,r)|r.generation!=i as u64+1)||rows.iter().filter(|r|r.status==JetVaultKeyStatus::Active).count()!=usize::from(*current!=0)||(*current!=0&&!rows.iter().any(|r|r.generation==*current&&r.status==JetVaultKeyStatus::Active)){return Err(JetVaultError::InvalidEncoding)} }
    if keys.iter().any(|r|!names.iter().any(|(tag,name,_,_)|*tag==r.key_type&&*name==r.name)){return Err(JetVaultError::InvalidEncoding)}
    Ok(JetVaultStore { repo_uuid, revision, strings, keys })
}

fn vault_paths(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) { let dir=root.join(".jet"); (dir.join("secrets.age"),dir.join("secrets-recipients"),dir) }
fn vault_identity_at(root: &std::path::Path) -> std::path::PathBuf { if root == std::path::Path::new(".") { identity_path() } else { root.join("keys/secrets.identity") } }
#[cfg(target_os="linux")]
#[repr(C)] struct VaultOpenHow{flags:u64,mode:u64,resolve:u64}
#[cfg(all(target_os="linux",target_arch="x86_64"))] const VAULT_SYS_RENAMEAT2:isize=316;
#[cfg(all(target_os="linux",target_arch="aarch64"))] const VAULT_SYS_RENAMEAT2:isize=276;
#[cfg(target_os="linux")] const VAULT_SYS_OPENAT2:isize=437;
#[cfg(target_os="linux")] const VAULT_SYS_PIDFD_OPEN:isize=434;
#[cfg(target_os="linux")] const VAULT_RESOLVE:u64=0x08|0x04;
#[cfg(target_os="linux")] const VAULT_O_CLOEXEC:u64=0x80000;
#[cfg(target_os="linux")] const VAULT_O_NOFOLLOW:u64=0x20000;
#[cfg(target_os="linux")] const VAULT_O_DIRECTORY:u64=0x10000;
#[cfg(target_os="linux")] const VAULT_O_CREAT:u64=0x40;
#[cfg(target_os="linux")] const VAULT_O_EXCL:u64=0x80;
#[cfg(target_os="linux")] const VAULT_O_RDWR:u64=2;
#[cfg(target_os="linux")] const VAULT_O_TMPFILE:u64=0x410000;
#[cfg(target_os="linux")] const VAULT_RENAME_NOREPLACE:u32=1;
#[cfg(target_os="linux")] const VAULT_RENAME_EXCHANGE:u32=2;
#[cfg(target_os="linux")] const VAULT_AT_FDCWD:i32=-100;
#[cfg(target_os="linux")] const VAULT_AT_SYMLINK_FOLLOW:i32=0x400;
#[cfg(target_os="linux")] unsafe extern "C"{
    fn syscall(number:isize,...)->isize;
    fn linkat(olddirfd:i32,oldpath:*const std::os::raw::c_char,newdirfd:i32,newpath:*const std::os::raw::c_char,flags:i32)->i32;
    fn unlinkat(dirfd:i32,path:*const std::os::raw::c_char,flags:i32)->i32;
}
#[cfg(target_os="linux")]
fn vault_linux_openat(dirfd:i32,name:&str,flags:u64,mode:u64)->Result<std::fs::File,std::io::Error>{use std::os::fd::FromRawFd;let name=std::ffi::CString::new(name).map_err(|_|std::io::Error::from_raw_os_error(22))?;let how=VaultOpenHow{flags:flags|VAULT_O_NOFOLLOW|VAULT_O_CLOEXEC,mode,resolve:VAULT_RESOLVE};let fd=unsafe{syscall(VAULT_SYS_OPENAT2,dirfd,name.as_ptr(),&how as *const VaultOpenHow,std::mem::size_of::<VaultOpenHow>())};if fd<0{Err(std::io::Error::last_os_error())}else{Ok(unsafe{std::fs::File::from_raw_fd(fd as i32)})}}
#[cfg(target_os="linux")]
fn vault_linux_dir(root:&std::path::Path)->Result<std::fs::File,JetVaultError>{use std::os::fd::AsRawFd;use std::os::unix::fs::{MetadataExt,PermissionsExt};let root_file=std::fs::File::open(root).map_err(|_|JetVaultError::Io{operation:"open",redacted_path:"<repository>"})?;let dir=vault_linux_openat(root_file.as_raw_fd(),".jet",VAULT_O_DIRECTORY,0).map_err(|error|if matches!(error.raw_os_error(),Some(18|20|22|38|40)){JetVaultError::UnsupportedProvider}else{JetVaultError::Io{operation:"open",redacted_path:"<vault-dir>"}})?;let metadata=dir.metadata().map_err(|_|JetVaultError::Io{operation:"stat",redacted_path:"<vault-dir>"})?;let(user,_)=vault_authority_identity()?;if !metadata.is_dir()||metadata.uid()!=user||metadata.permissions().mode()&0o022!=0{return Err(JetVaultError::UnsupportedProvider)}Ok(dir)}
#[cfg(target_os="linux")]
fn vault_linux_read(root:&std::path::Path,name:&str)->Result<Option<Vec<u8>>,JetVaultError>{use std::io::Read;use std::os::fd::AsRawFd;let dir=vault_linux_dir(root)?;let mut file=match vault_linux_openat(dir.as_raw_fd(),name,0,0){Ok(file)=>file,Err(error)=>return match error.raw_os_error(){Some(2)=>Ok(None),Some(38|22|40)=>Err(JetVaultError::UnsupportedProvider),_=>Err(JetVaultError::Locked)}};let mut bytes=Vec::new();file.read_to_end(&mut bytes).map_err(|_|JetVaultError::Locked)?;Ok(Some(bytes))}
#[derive(Clone,Copy,Debug,Eq,PartialEq)] enum VaultStartKind{Absent,Historical,V2}
fn vault_recipients_at(root:&std::path::Path)->Result<(Vec<String>,[u8;32]),JetVaultError>{
    #[cfg(not(target_os="linux"))]{let _=root;return Err(JetVaultError::UnsupportedProvider)}
    #[cfg(target_os="linux")]let text=String::from_utf8(vault_linux_read(root,"secrets-recipients")?.ok_or(JetVaultError::Locked)?).map_err(|_|JetVaultError::InvalidEncoding)?;let mut recipients=Vec::new();for line in text.lines(){let recipient=line.trim();if !recipient.is_empty(){let canonical=age::x25519::Recipient::from_str(recipient).map_err(|_|JetVaultError::InvalidEncoding)?.to_string();recipients.push(canonical)}}recipients.sort();if recipients.is_empty()||recipients.windows(2).any(|pair|pair[0]==pair[1])||recipients.len()>u16::MAX as usize{return Err(JetVaultError::InvalidEncoding)}let mut canonical=Vec::new();canonical.extend_from_slice(&(recipients.len()as u16).to_le_bytes());for recipient in &recipients{push_u16(&mut canonical,recipient.len())?;canonical.extend_from_slice(recipient.as_bytes())}Ok((recipients,vault_hash(&[b"JVLT2 provider recipients",&canonical])))
}
fn vault_canonical_pairs(mut pairs:Vec<(String,String)>)->Result<(Vec<(String,String)>,Vec<u8>),JetVaultError>{
    if pairs.len()>JVLT_MAX_STRINGS{return Err(JetVaultError::InvalidEncoding)}pairs.sort_by(|a,b|a.0.as_bytes().cmp(b.0.as_bytes()));if pairs.windows(2).any(|pair|pair[0].0==pair[1].0){return Err(JetVaultError::InvalidEncoding)}let mut canonical=Vec::new();canonical.extend_from_slice(&(pairs.len()as u32).to_le_bytes());for(name,value)in &pairs{jet_vault_validate_name(name)?;if value.len()>1024*1024{return Err(JetVaultError::InvalidEncoding)}push_u16(&mut canonical,name.len())?;canonical.extend_from_slice(name.as_bytes());push_u32(&mut canonical,value.len())?;canonical.extend_from_slice(value.as_bytes())}Ok((pairs,canonical))
}
fn vault_decode_ciphertext_at(root:&std::path::Path,ciphertext:&Vec<u8>)->Result<(JetVaultStore,[u8;32],VaultStartKind),JetVaultError>{
    let identity=std::fs::read_to_string(vault_identity_at(root)).map_err(|_|JetVaultError::Locked)?;
    let plaintext=Zeroizing(jet_vault_decrypt_impl(&identity,ciphertext).map_err(|_|JetVaultError::Locked)?);
    if plaintext.0.starts_with(JVLT_MAGIC){let store=jet_vault_decode_v2(&plaintext.0)?;if jet_vault_encode_v2(&store)?!=plaintext.0{return Err(JetVaultError::InvalidEncoding)}let hash=vault_hash(&[b"JVLT2 starting store",&plaintext.0]);Ok((store,hash,VaultStartKind::V2))}else{let pairs=jet_vault_decode_pairs(&plaintext.0).ok_or(JetVaultError::InvalidEncoding)?;let(pairs,canonical)=vault_canonical_pairs(pairs)?;let canonical=Zeroizing(canonical);Ok((JetVaultStore{repo_uuid:[0;16],revision:0,strings:pairs,keys:Vec::new()},vault_hash(&[b"JVLT1 starting store",&canonical.0]),VaultStartKind::Historical))}
}
#[cfg(target_os="linux")]
fn vault_recover_backup_at(root:&std::path::Path)->Result<(),JetVaultError>{use std::os::fd::AsRawFd;use std::os::unix::fs::MetadataExt;let dir=vault_linux_dir(root)?;let dirfd=dir.as_raw_fd();let backup=match vault_linux_openat(dirfd,".secrets.age.backup",0,0){Ok(file)=>file,Err(error)if error.raw_os_error()==Some(2)=>return Ok(()),Err(_)=>return Err(JetVaultError::DurabilityUnknown)};let backup_state=vault_decode_ciphertext_at(root,&vault_read_file(&backup)?).map_err(|_|JetVaultError::DurabilityUnknown)?;let final_file=match vault_linux_openat(dirfd,"secrets.age",0,0){Ok(file)=>Some(file),Err(error)if error.raw_os_error()==Some(2)=>None,Err(_)=>return Err(JetVaultError::DurabilityUnknown)};let final_state=final_file.as_ref().and_then(|file|vault_decode_ciphertext_at(root,&vault_read_file(file).ok()?).ok());let final_is_successor=final_state.as_ref().is_some_and(|final_state|final_state.0.revision>=backup_state.0.revision&&match(backup_state.2,final_state.2){(VaultStartKind::Historical,VaultStartKind::V2)=>true,(VaultStartKind::V2,VaultStartKind::V2)=>backup_state.0.repo_uuid==final_state.0.repo_uuid,_=>false});let quarantine=format!(".secrets.age.backup.done.{:x}",backup.metadata().map_err(|_|JetVaultError::DurabilityUnknown)?.ino());if final_is_successor{if !vault_remove_exact(&dir,".secrets.age.backup",&backup,&quarantine)?{return Err(JetVaultError::DurabilityUnknown)}dir.sync_all().map_err(|_|JetVaultError::DurabilityUnknown)?;return Ok(())}match final_file{Some(final_file)=>{vault_linux_rename(dirfd,".secrets.age.backup","secrets.age",VAULT_RENAME_EXCHANGE)?;let installed=vault_linux_openat(dirfd,"secrets.age",0,0).map_err(|_|JetVaultError::DurabilityUnknown)?;if !vault_same_inode(&installed,&backup){let _=vault_linux_rename(dirfd,".secrets.age.backup","secrets.age",VAULT_RENAME_EXCHANGE);return Err(JetVaultError::DurabilityUnknown)}let quarantine=format!(".secrets.age.rejected.{:x}",final_file.metadata().map_err(|_|JetVaultError::DurabilityUnknown)?.ino());if !vault_remove_exact(&dir,".secrets.age.backup",&final_file,&quarantine)?{return Err(JetVaultError::DurabilityUnknown)}},None=>{vault_linux_rename(dirfd,".secrets.age.backup","secrets.age",VAULT_RENAME_NOREPLACE)?;let installed=vault_linux_openat(dirfd,"secrets.age",0,0).map_err(|_|JetVaultError::DurabilityUnknown)?;if !vault_same_inode(&installed,&backup){return Err(JetVaultError::DurabilityUnknown)}}}dir.sync_all().map_err(|_|JetVaultError::DurabilityUnknown)?;Ok(())}
fn vault_read_contents_at(root: &std::path::Path) -> Result<(JetVaultStore,[u8;32],VaultStartKind),JetVaultError> {
    #[cfg(target_os="linux")]vault_recover_backup_at(root)?;
    #[cfg(target_os="linux")]let ciphertext=match vault_linux_read(root,"secrets.age")?{Some(bytes)=>bytes,None=>return Ok((JetVaultStore::new([0;16]),vault_hash(&[b"JVLT0 absent store"]),VaultStartKind::Absent))};
    vault_decode_ciphertext_at(root,&ciphertext)
}
fn vault_read_at(root: &std::path::Path) -> Result<(JetVaultStore,[u8;32],[u8;32],VaultStartKind),JetVaultError> {
    let(store,start_hash,start_kind)=vault_read_contents_at(root)?;let(_,provider_hash)=vault_recipients_at(root)?;Ok((store,start_hash,provider_hash,start_kind))
}
fn vault_ref<T:JetVaultKey>(store:&JetVaultStore,record:&JetVaultRecord)->JetVaultKeyRef<T>{JetVaultKeyRef{provider:JetVaultProvider::Repo,repo_uuid:store.repo_uuid,name:record.name.clone(),generation:record.generation,opaque_id:record.opaque_id,record_hash:record.record_hash,marker:std::marker::PhantomData}}
fn vault_find<'a,T:JetVaultKey>(store:&'a JetVaultStore,key_ref:&JetVaultKeyRef<T>)->Result<&'a JetVaultRecord,JetVaultError>{if key_ref.provider!=JetVaultProvider::Repo||store.repo_uuid!=key_ref.repo_uuid{return Err(JetVaultError::NotFound)} let record=store.keys.iter().find(|r|r.name==key_ref.name&&r.generation==key_ref.generation&&r.opaque_id==key_ref.opaque_id&&r.record_hash==key_ref.record_hash).ok_or(JetVaultError::NotFound)?;if record.key_type!=T::TAG{return Err(JetVaultError::WrongType)}if vault_record_hash(record)!=record.record_hash{return Err(JetVaultError::InvalidEncoding)}Ok(record)}

pub fn jet_vault_current_at<T:JetVaultKey>(root:&std::path::Path,name:&str)->Result<Option<JetVaultKeyRef<T>>,JetVaultError>{jet_vault_validate_name(name)?;let(store,_,_)=vault_read_contents_at(root)?;if store.keys.iter().any(|r|r.name==name&&r.key_type!=T::TAG){return Err(JetVaultError::WrongType)}Ok(store.keys.iter().find(|r|r.name==name&&r.key_type==T::TAG&&r.status==JetVaultKeyStatus::Active).map(|r|vault_ref(&store,r)))}
pub fn jet_vault_versions_at<T:JetVaultKey>(root:&std::path::Path,name:&str)->Result<Vec<JetVaultKeyRef<T>>,JetVaultError>{jet_vault_validate_name(name)?;let(store,_,_)=vault_read_contents_at(root)?;if store.keys.iter().any(|r|r.name==name&&r.key_type!=T::TAG){return Err(JetVaultError::WrongType)}let mut rows:Vec<_>=store.keys.iter().filter(|r|r.name==name&&r.key_type==T::TAG).collect();rows.sort_by_key(|r|std::cmp::Reverse(r.generation));Ok(rows.into_iter().map(|r|vault_ref(&store,r)).collect())}
pub fn jet_vault_load_at<T:JetVaultKey>(root:&std::path::Path,key_ref:&JetVaultKeyRef<T>)->Result<T,JetVaultError>{let(store,_,_)=vault_read_contents_at(root)?;let record=vault_find(&store,key_ref)?;if record.status==JetVaultKeyStatus::Revoked{return Err(JetVaultError::Revoked)}T::from_bytes(record.key.clone())}
pub fn jet_vault_status_at<T:JetVaultKey>(root:&std::path::Path,key_ref:&JetVaultKeyRef<T>)->Result<JetVaultKeyStatus,JetVaultError>{let(store,_,_)=vault_read_contents_at(root)?;Ok(vault_find(&store,key_ref)?.status)}

fn vault_prepare<T:JetVaultKey>(root:&std::path::Path,name:&str,action:JetVaultAction,target:Option<JetVaultKeyRef<T>>,imported:Option<Vec<u8>>,reason:String)->Result<JetVaultMutationPlan<T>,JetVaultError>{
    jet_vault_validate_name(name)?;if matches!(action,JetVaultAction::Retire|JetVaultAction::Revoke|JetVaultAction::Rotate){vault_validate_reason(&reason)?;}let(mut store,start_hash,provider_hash,start_kind)=vault_read_at(root)?;
    if start_kind!=VaultStartKind::V2{let provisional=vault_random()?;if provisional==[0;16]{return Err(JetVaultError::Internal{incident_id:"vault-zero-uuid"})}store.repo_uuid=provisional;}
    if store.keys.iter().any(|r|r.name==name&&r.key_type!=T::TAG){return Err(JetVaultError::WrongType)}
    match action {
        JetVaultAction::Generate if store.keys.iter().any(|r|r.name==name)=>return Err(JetVaultError::Conflict),
        JetVaultAction::Store|JetVaultAction::RawImport if store.keys.iter().any(|r|r.name==name&&r.status==JetVaultKeyStatus::Active)=>return Err(JetVaultError::Conflict),
        JetVaultAction::Rotate if !store.keys.iter().any(|r|r.name==name&&r.key_type==T::TAG&&r.status==JetVaultKeyStatus::Active)=>return Err(JetVaultError::NotFound),
        JetVaultAction::Retire|JetVaultAction::Revoke => {
            let reference=target.as_ref().ok_or(JetVaultError::NotFound)?;let record=vault_find(&store,reference)?;
            if action==JetVaultAction::Retire && record.status!=JetVaultKeyStatus::Active{return Err(if record.status==JetVaultKeyStatus::Revoked{JetVaultError::Revoked}else{JetVaultError::Conflict})}
            if action==JetVaultAction::Revoke && record.status==JetVaultKeyStatus::Revoked{return Err(JetVaultError::Revoked)}
        }
        _=>{}
    }
    let public_key_hash=match imported.as_ref(){Some(bytes)=>vault_public_hash(T::TAG,bytes)?,None=>[0;32]};let key_digest=match imported.as_ref(){Some(bytes)=>vault_key_digest(T::TAG,bytes),None=>[0;32]};
    let(reference_present,reference_repo,reference_generation,reference_opaque,reference_hash)=match target.as_ref(){Some(reference)=>(1u8,reference.repo_uuid,reference.generation,reference.opaque_id,reference.record_hash),None=>(0,[0;16],0,[0;16],[0;32])};
    let current=store.keys.iter().find(|r|r.name==name&&r.key_type==T::TAG&&r.status==JetVaultKeyStatus::Active).map(|r|r.generation).unwrap_or(0);let next=store.keys.iter().filter(|r|r.name==name&&r.key_type==T::TAG).map(|r|r.generation).max().unwrap_or(0).checked_add(1).ok_or(JetVaultError::Internal{incident_id:"vault-generation"})?;let(transition_from,transition_to)=match action{JetVaultAction::Rotate=>(current,next),JetVaultAction::Retire|JetVaultAction::Revoke=>(reference_generation,0),_=>(0,next)};
    let mut operation=Vec::new();operation.extend_from_slice(b"JVLT2 operation");operation.extend_from_slice(&store.repo_uuid);operation.extend_from_slice(&store.revision.to_le_bytes());operation.extend_from_slice(&start_hash);operation.extend_from_slice(&provider_hash);operation.push(action.byte());operation.push(T::TAG);push_u16(&mut operation,name.len())?;operation.extend_from_slice(name.as_bytes());operation.push(reference_present);operation.extend_from_slice(&reference_repo);operation.extend_from_slice(&reference_generation.to_le_bytes());operation.extend_from_slice(&reference_opaque);operation.extend_from_slice(&reference_hash);push_u16(&mut operation,reason.len())?;operation.extend_from_slice(reason.as_bytes());operation.extend_from_slice(&public_key_hash);operation.extend_from_slice(&key_digest);operation.extend_from_slice(&[0;32]);let operation_hash=vault_hash(&[&operation]);
    let issued=std::time::Instant::now();let deadline=issued.checked_add(std::time::Duration::from_secs(300)).ok_or(JetVaultError::UnsupportedProvider)?;let now_ms=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|JetVaultError::UnsupportedProvider)?.as_millis();let expires_unix_ms=u64::try_from(now_ms).ok().and_then(|value|value.checked_add(300_000)).ok_or(JetVaultError::UnsupportedProvider)?;
    Ok(JetVaultMutationPlan{action,name:name.into(),target,imported:imported.map(Zeroizing),reason,repo_uuid:store.repo_uuid,start_revision:store.revision,start_hash,provider_hash,public_key_hash,key_digest,transition_from,transition_to,start_kind,operation_hash,issued,deadline,expires_unix_ms})
}
pub fn jet_vault_prepare_generate_at<T:JetVaultKey>(root:&std::path::Path,name:&str)->Result<JetVaultMutationPlan<T>,JetVaultError>{vault_prepare(root,name,JetVaultAction::Generate,None,None,String::new())}
pub fn jet_vault_prepare_store_at<T:JetVaultKey>(root:&std::path::Path,name:&str,key:T)->Result<JetVaultMutationPlan<T>,JetVaultError>{vault_prepare(root,name,JetVaultAction::Store,None,Some(key.into_bytes()),String::new())}
fn jet_vault_prepare_raw_import_at<T:JetVaultKey>(root:&std::path::Path,name:&str,key:T)->Result<JetVaultMutationPlan<T>,JetVaultError>{vault_prepare(root,name,JetVaultAction::RawImport,None,Some(key.into_bytes()),String::new())}
pub fn jet_vault_prepare_rotate_at<T:JetVaultKey>(root:&std::path::Path,name:&str)->Result<JetVaultMutationPlan<T>,JetVaultError>{vault_prepare(root,name,JetVaultAction::Rotate,None,None,"rotated".into())}
pub fn jet_vault_prepare_retire_at<T:JetVaultKey>(root:&std::path::Path,key_ref:&JetVaultKeyRef<T>,reason:&str)->Result<JetVaultMutationPlan<T>,JetVaultError>{vault_prepare(root,&key_ref.name,JetVaultAction::Retire,Some(key_ref.clone()),None,reason.into())}
pub fn jet_vault_prepare_revoke_at<T:JetVaultKey>(root:&std::path::Path,key_ref:&JetVaultKeyRef<T>,reason:&str)->Result<JetVaultMutationPlan<T>,JetVaultError>{vault_prepare(root,&key_ref.name,JetVaultAction::Revoke,Some(key_ref.clone()),None,reason.into())}

#[cfg(test)] thread_local!{static VAULT_AUTHORIZER:std::cell::RefCell<Option<Box<dyn FnMut(&str)->bool>>>=std::cell::RefCell::new(None);}
#[cfg(test)] pub fn jet_vault_set_test_authorizer(callback:impl FnMut(&str)->bool+'static){VAULT_AUTHORIZER.with(|slot|*slot.borrow_mut()=Some(Box::new(callback)));}
#[cfg(test)] pub fn jet_vault_clear_test_authorizer(){VAULT_AUTHORIZER.with(|slot|*slot.borrow_mut()=None);}
#[cfg(target_os="linux")] fn vault_authority_identity()->Result<(u32,i32),JetVaultError>{unsafe extern "C"{fn geteuid()->u32;fn getsid(pid:i32)->i32;}let user=unsafe{geteuid()};let session=unsafe{getsid(0)};if session<0{Err(JetVaultError::UnsupportedProvider)}else{Ok((user,session))}}
#[cfg(not(target_os="linux"))] fn vault_authority_identity()->Result<(u32,i32),JetVaultError>{Err(JetVaultError::UnsupportedProvider)}
fn vault_uuid_hex(bytes:&[u8;16])->String{bytes.iter().map(|byte|format!("{byte:02x}")).collect()}
#[cfg(test)] thread_local!{static VAULT_TEST_TRUST_PATH:std::cell::RefCell<Option<std::path::PathBuf>>=const{std::cell::RefCell::new(None)};}
#[cfg(test)] pub fn jet_vault_set_test_trust_path(path:Option<std::path::PathBuf>){VAULT_TEST_TRUST_PATH.with(|slot|*slot.borrow_mut()=path);}
fn vault_headless_granted(repo_uuid:&[u8;16])->bool{#[cfg(test)]let test_path=VAULT_TEST_TRUST_PATH.with(|slot|slot.borrow().clone());#[cfg(test)]if let Some(path)=test_path{let Ok(text)=std::fs::read_to_string(path)else{return false};let subject=vault_uuid_hex(repo_uuid);return text.lines().map(str::trim).any(|line|line==format!("grant:user:vault.write:{subject}")||line==format!("grant:repo:vault.write:{subject}"));}let Some(home)=std::env::var_os("HOME")else{return false};let Ok(text)=std::fs::read_to_string(std::path::PathBuf::from(home).join(".jet/trust"))else{return false};let subject=vault_uuid_hex(repo_uuid);text.lines().map(str::trim).any(|line|line==format!("grant:user:vault.write:{subject}")||line==format!("grant:repo:vault.write:{subject}"))}
fn vault_native_authorize(preview:&str,repo_uuid:&[u8;16])->bool{
    #[cfg(test)] { if let Some(answer)=VAULT_AUTHORIZER.with(|slot|slot.borrow_mut().as_mut().map(|callback|callback(preview))){return answer;} }
    #[cfg(target_os="linux")] { use std::io::{BufRead,Write}; let Ok(mut tty)=std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty") else{return vault_headless_granted(repo_uuid)};if writeln!(tty,"Vault write request: {preview}\nType `authorize` to continue:").is_err(){return false}let Ok(reader_file)=tty.try_clone()else{return false};let mut answer=String::new();std::io::BufReader::new(reader_file).read_line(&mut answer).is_ok()&&answer.trim()=="authorize" }
    #[cfg(not(target_os="linux"))] { let _=preview;false }
}
pub fn jet_vault_authorize_write_impl<T:JetVaultKey>(plan:&JetVaultMutationPlan<T>,reason:&str)->Result<JetVaultWrite<T>,JetVaultError>{vault_validate_reason(reason)?;let now=std::time::Instant::now();if now>plan.deadline||now.duration_since(plan.issued)>std::time::Duration::from_secs(300){return Err(JetVaultError::Conflict)}let mut bytes=Vec::new();bytes.extend_from_slice(b"JVLT2 authority preview");bytes.extend_from_slice(&plan.operation_hash);bytes.push(plan.action.byte());bytes.push(T::TAG);bytes.extend_from_slice(&plan.repo_uuid);bytes.extend_from_slice(&plan.start_revision.to_le_bytes());push_u16(&mut bytes,plan.name.len())?;bytes.extend_from_slice(plan.name.as_bytes());bytes.extend_from_slice(&plan.transition_from.to_le_bytes());bytes.extend_from_slice(&plan.transition_to.to_le_bytes());push_u16(&mut bytes,plan.reason.len())?;bytes.extend_from_slice(plan.reason.as_bytes());push_u16(&mut bytes,reason.len())?;bytes.extend_from_slice(reason.as_bytes());bytes.extend_from_slice(&plan.expires_unix_ms.to_le_bytes());let preview_hash=vault_hash(&[&bytes]);let preview=format!("{} {} {} {} -> {} in repository {} at revision {}; reason: {}; authority reason: {}; expires_unix_ms: {}",plan.action.text(),T::NAME,plan.name,plan.transition_from,plan.transition_to,vault_uuid_hex(&plan.repo_uuid),plan.start_revision,if plan.reason.is_empty(){"<none>"}else{&plan.reason},reason,plan.expires_unix_ms);if !vault_native_authorize(&preview,&plan.repo_uuid){return Err(JetVaultError::AuthorityDenied)}let(user,session)=vault_authority_identity()?;Ok(JetVaultWrite{operation_hash:plan.operation_hash,preview_hash,user,session,deadline:plan.deadline,marker:std::marker::PhantomData})}

#[cfg(target_os="linux")]
struct VaultLockGuard{dir:std::fs::File,file:std::fs::File,nonce:String}
#[cfg(not(target_os="linux"))]
struct VaultLockGuard;
#[cfg(target_os="linux")]
fn vault_linux_rename(dirfd:i32,old:&str,new:&str,flags:u32)->Result<(),JetVaultError>{let old=std::ffi::CString::new(old).map_err(|_|JetVaultError::Internal{incident_id:"vault-temp-name"})?;let new=std::ffi::CString::new(new).map_err(|_|JetVaultError::Internal{incident_id:"vault-temp-name"})?;let result=unsafe{syscall(VAULT_SYS_RENAMEAT2,dirfd,old.as_ptr(),dirfd,new.as_ptr(),flags)};if result<0{let error=std::io::Error::last_os_error();if matches!(error.raw_os_error(),Some(38|22)){Err(JetVaultError::UnsupportedProvider)}else{Err(JetVaultError::Io{operation:"install",redacted_path:"<vault-store>"})}}else{Ok(())}}
#[cfg(target_os="linux")]
fn vault_linux_unlink(dirfd:i32,name:&str)->Result<(),JetVaultError>{let name=std::ffi::CString::new(name).map_err(|_|JetVaultError::Internal{incident_id:"vault-entry-name"})?;if unsafe{unlinkat(dirfd,name.as_ptr(),0)}==0{Ok(())}else{Err(JetVaultError::Io{operation:"unlink",redacted_path:"<vault-entry>"})}}
#[cfg(target_os="linux")]
fn vault_linux_link_file(file:&std::fs::File,dirfd:i32,name:&str)->Result<(),JetVaultError>{use std::os::fd::AsRawFd;let source=std::ffi::CString::new(format!("/proc/self/fd/{}",file.as_raw_fd())).map_err(|_|JetVaultError::Internal{incident_id:"vault-fd-path"})?;let name=std::ffi::CString::new(name).map_err(|_|JetVaultError::Internal{incident_id:"vault-entry-name"})?;if unsafe{linkat(VAULT_AT_FDCWD,source.as_ptr(),dirfd,name.as_ptr(),VAULT_AT_SYMLINK_FOLLOW)}==0{Ok(())}else{let error=std::io::Error::last_os_error();if matches!(error.raw_os_error(),Some(38|22)){Err(JetVaultError::UnsupportedProvider)}else{Err(JetVaultError::Conflict)}}}
#[cfg(target_os="linux")]
fn vault_same_inode(left:&std::fs::File,right:&std::fs::File)->bool{use std::os::unix::fs::MetadataExt;match(left.metadata(),right.metadata()){(Ok(left),Ok(right))=>left.dev()==right.dev()&&left.ino()==right.ino(),_=>false}}
#[cfg(target_os="linux")]
fn vault_read_file(file:&std::fs::File)->Result<Vec<u8>,JetVaultError>{use std::io::{Read,Seek};let mut file=file.try_clone().map_err(|_|JetVaultError::Locked)?;file.seek(std::io::SeekFrom::Start(0)).map_err(|_|JetVaultError::Locked)?;let mut bytes=Vec::new();file.read_to_end(&mut bytes).map_err(|_|JetVaultError::Locked)?;Ok(bytes)}
#[cfg(target_os="linux")]
fn vault_quarantine_exact(dir:&std::fs::File,name:&str,held:&std::fs::File,quarantine:&str)->Result<bool,JetVaultError>{use std::os::fd::AsRawFd;let dirfd=dir.as_raw_fd();let visible=match vault_linux_openat(dirfd,name,0,0){Ok(file)=>file,Err(error)if error.raw_os_error()==Some(2)=>return Ok(false),Err(_)=>return Err(JetVaultError::Conflict)};if !vault_same_inode(&visible,held){return Ok(false)}#[cfg(test)]if name==".secrets.age.lock"&&vault_test_fault("substitute-lock-after-check"){use std::io::Write;vault_linux_rename(dirfd,name,".secrets.age.lock.acquired",VAULT_RENAME_NOREPLACE)?;let mut replacement=vault_linux_openat(dirfd,name,1|VAULT_O_CREAT|VAULT_O_EXCL,0o600).map_err(|_|JetVaultError::Conflict)?;replacement.write_all(b"replacement").map_err(|_|JetVaultError::Conflict)?;replacement.sync_all().map_err(|_|JetVaultError::Conflict)?;}vault_linux_rename(dirfd,name,quarantine,VAULT_RENAME_NOREPLACE)?;let moved=vault_linux_openat(dirfd,quarantine,0,0).map_err(|_|JetVaultError::Conflict)?;if vault_same_inode(&moved,held){Ok(true)}else{let _=vault_linux_rename(dirfd,quarantine,name,VAULT_RENAME_NOREPLACE);Ok(false)}}
#[cfg(target_os="linux")]
fn vault_remove_exact(dir:&std::fs::File,name:&str,held:&std::fs::File,quarantine:&str)->Result<bool,JetVaultError>{use std::os::fd::AsRawFd;if !vault_quarantine_exact(dir,name,held,quarantine)?{return Ok(false)}vault_linux_unlink(dir.as_raw_fd(),quarantine)?;Ok(true)}
#[cfg(target_os="linux")]
impl Drop for VaultLockGuard{fn drop(&mut self){let quarantine=format!(".secrets.age.lock.done.{}",self.nonce);let _=vault_remove_exact(&self.dir,".secrets.age.lock",&self.file,&quarantine);}}
#[cfg(target_os="linux")]
fn vault_process_start(pid:u32)->Option<u64>{let text=std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;let rest=text.rsplit_once(')')?.1;rest.split_whitespace().nth(19)?.parse().ok()}
#[cfg(target_os="linux")]
fn vault_valid_hex(text:&str,count:usize)->bool{text.len()==count&&text.bytes().all(|byte|byte.is_ascii_hexdigit())&&text.bytes().any(|byte|byte!=b'0')}
#[cfg(target_os="linux")]
fn vault_recover_stale_lock(root:&std::path::Path,dir:&std::fs::File,expected_hash:[u8;32],expected_revision:u64,expected_repo:[u8;16])->Result<(),JetVaultError>{use std::os::fd::{AsRawFd,FromRawFd};use std::os::unix::fs::MetadataExt;let dirfd=dir.as_raw_fd();let file=vault_linux_openat(dirfd,".secrets.age.lock",0,0).map_err(|_|JetVaultError::Conflict)?;let metadata=file.metadata().map_err(|_|JetVaultError::Conflict)?;let(user,_)=vault_authority_identity()?;if !metadata.file_type().is_file()||metadata.uid()!=user||metadata.modified().ok().and_then(|time|time.elapsed().ok()).is_none_or(|age|age<=std::time::Duration::from_secs(600)){return Err(JetVaultError::Conflict)}let text=String::from_utf8(vault_read_file(&file)?).map_err(|_|JetVaultError::Conflict)?;let mut repo=None;let mut revision=None;let mut pid=None;let mut start=None;let mut nonce=None;let mut hash=None;for field in text.split_whitespace(){if let Some((key,value))=field.split_once('='){match key{"repo"=>repo=Some(value),"revision"=>revision=value.parse().ok(),"pid"=>pid=value.parse().ok(),"process-start"=>start=value.parse().ok(),"nonce"=>nonce=Some(value),"hash"=>hash=Some(value),_=>{}}}}let(pid,start,nonce,hash)=(pid.ok_or(JetVaultError::Conflict)?,start.ok_or(JetVaultError::Conflict)?,nonce.ok_or(JetVaultError::Conflict)?,hash.ok_or(JetVaultError::Conflict)?);let expected_repo_text=vault_uuid_hex(&expected_repo);if repo!=Some(expected_repo_text.as_str())||revision!=Some(expected_revision)||!vault_valid_hex(nonce,32)||hash!=hex_bytes(&expected_hash){return Err(JetVaultError::Conflict)}let current_start=vault_process_start(pid);let pidfd=unsafe{syscall(VAULT_SYS_PIDFD_OPEN,pid as i32,0u32)};let dead=if pidfd>=0{drop(unsafe{std::fs::File::from_raw_fd(pidfd as i32)});current_start!=Some(start)}else{match std::io::Error::last_os_error().raw_os_error(){Some(3)=>true,Some(38|22)=>return Err(JetVaultError::UnsupportedProvider),_=>false}};if !dead{return Err(JetVaultError::Conflict)}let(current,current_hash,_,_)=vault_read_at(root)?;if current.revision!=expected_revision||current_hash!=expected_hash{return Err(JetVaultError::Conflict)}let quarantine=format!(".secrets.age.lock.stale.{nonce}");if !vault_remove_exact(dir,".secrets.age.lock",&file,&quarantine)?{return Err(JetVaultError::Conflict)}Ok(())}
#[cfg(target_os="linux")]
fn vault_acquire_lock(root:&std::path::Path,repo_uuid:[u8;16],revision:u64,start_hash:[u8;32])->Result<VaultLockGuard,JetVaultError>{use std::io::Write;use std::os::fd::AsRawFd;let dir=vault_linux_dir(root)?;let dirfd=dir.as_raw_fd();let mut file=match vault_linux_openat(dirfd,".secrets.age.lock",1|VAULT_O_CREAT|VAULT_O_EXCL,0o600){Ok(file)=>file,Err(error)if error.raw_os_error()==Some(17)=>{vault_recover_stale_lock(root,&dir,start_hash,revision,repo_uuid)?;vault_linux_openat(dirfd,".secrets.age.lock",1|VAULT_O_CREAT|VAULT_O_EXCL,0o600).map_err(|_|JetVaultError::Conflict)?},Err(error)if matches!(error.raw_os_error(),Some(38|22))=>return Err(JetVaultError::UnsupportedProvider),Err(_)=>return Err(JetVaultError::Conflict)};let nonce=vault_random::<16>()?;if nonce==[0;16]{return Err(JetVaultError::Internal{incident_id:"vault-zero-lock-nonce"})}let nonce=hex_bytes(&nonce);let process_start=vault_process_start(std::process::id()).ok_or(JetVaultError::UnsupportedProvider)?;writeln!(&mut file,"repo={} revision={} pid={} process-start={} nonce={} hash={}",vault_uuid_hex(&repo_uuid),revision,std::process::id(),process_start,nonce,hex_bytes(&start_hash)).map_err(|_|JetVaultError::Io{operation:"lock",redacted_path:"<vault-lock>"})?;file.sync_all().map_err(|_|JetVaultError::Io{operation:"lock",redacted_path:"<vault-lock>"})?;Ok(VaultLockGuard{dir,file,nonce})}
#[cfg(not(target_os="linux"))]
fn vault_acquire_lock(_root:&std::path::Path,_repo_uuid:[u8;16],_revision:u64,_start_hash:[u8;32])->Result<VaultLockGuard,JetVaultError>{Err(JetVaultError::UnsupportedProvider)}

fn vault_lifecycle_unix_ms(store:&JetVaultStore)->Result<u64,JetVaultError>{let wall=if vault_test_fault("clock-rollback"){1}else{u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|JetVaultError::UnsupportedProvider)?.as_millis()).map_err(|_|JetVaultError::UnsupportedProvider)?};Ok(store.keys.iter().fold(wall,|latest,record|latest.max(record.created_unix_ms).max(record.status_unix_ms)))}

fn vault_commit_at<T:JetVaultKey>(root:&std::path::Path,mut plan:JetVaultMutationPlan<T>,write:JetVaultWrite<T>,expected:JetVaultAction)->Result<(Option<JetVaultKeyRef<T>>,Option<JetVaultKeyRef<T>>),JetVaultError>{
    let now=std::time::Instant::now();let(user,session)=vault_authority_identity()?;if plan.action!=expected||now>plan.deadline||now.duration_since(plan.issued)>std::time::Duration::from_secs(300)||now>write.deadline||write.operation_hash!=plan.operation_hash||write.preview_hash==[0;32]||write.user!=user||write.session!=session{return Err(JetVaultError::Conflict)}
    let lock=vault_acquire_lock(root,plan.repo_uuid,plan.start_revision,plan.start_hash)?;
    let(mut store,current_hash,provider_hash,start_kind)=vault_read_at(root)?;if start_kind!=plan.start_kind||current_hash!=plan.start_hash||provider_hash!=plan.provider_hash{return Err(JetVaultError::Conflict)}if start_kind==VaultStartKind::V2{if store.repo_uuid!=plan.repo_uuid||store.revision!=plan.start_revision{return Err(JetVaultError::Conflict)}}else{if store.repo_uuid!=[0;16]||store.revision!=0||plan.start_revision!=0{return Err(JetVaultError::Conflict)}store.repo_uuid=plan.repo_uuid;}
    let previous=store.keys.iter().find(|r|r.name==plan.name&&r.key_type==T::TAG&&r.status==JetVaultKeyStatus::Active).map(|r|vault_ref(&store,r));
    let mut current=None;
    match plan.action {
        JetVaultAction::Generate|JetVaultAction::Store|JetVaultAction::Rotate|JetVaultAction::RawImport=>{
            let unix_ms=vault_lifecycle_unix_ms(&store)?;if plan.action==JetVaultAction::Rotate{for record in &mut store.keys{if record.name==plan.name&&record.key_type==T::TAG&&record.status==JetVaultKeyStatus::Active{record.status=JetVaultKeyStatus::Retired;record.status_unix_ms=unix_ms;record.reason_hash=vault_reason_hash(JetVaultKeyStatus::Retired,"rotated")?;}}}
            let key=match plan.imported.take(){Some(mut bytes)=>T::from_bytes(std::mem::take(&mut bytes.0))?,None=>T::generate()?};let public_key_hash=vault_hash(&[b"JVLT2 public key",&[T::TAG],&key.public_bytes()]);let key_bytes=key.into_bytes();let key_digest=vault_key_digest(T::TAG,&key_bytes);if plan.action!=JetVaultAction::Generate&&plan.action!=JetVaultAction::Rotate&&(public_key_hash!=plan.public_key_hash||key_digest!=plan.key_digest){return Err(JetVaultError::Conflict)}let generation=store.keys.iter().filter(|r|r.name==plan.name&&r.key_type==T::TAG).map(|r|r.generation).max().unwrap_or(0).checked_add(1).ok_or(JetVaultError::Internal{incident_id:"vault-generation"})?;if generation!=plan.transition_to{return Err(JetVaultError::Conflict)}let opaque_id=vault_random()?;if opaque_id==[0;16]{return Err(JetVaultError::Internal{incident_id:"vault-zero-id"})}let provenance=if matches!(plan.action,JetVaultAction::Generate|JetVaultAction::Rotate){JetVaultProvenance::Generated}else{JetVaultProvenance::Imported};let mut record=JetVaultRecord{name:plan.name.clone(),key_type:T::TAG,generation,status:JetVaultKeyStatus::Active,provenance,opaque_id,created_unix_ms:unix_ms,status_unix_ms:unix_ms,record_hash:[0;32],public_key_hash,reason_hash:[0;32],origin:None,key:key_bytes};record.record_hash=vault_record_hash(&record);current=Some(vault_ref(&store,&record));store.keys.push(record);
        }
        JetVaultAction::Retire|JetVaultAction::Revoke=>{let unix_ms=vault_lifecycle_unix_ms(&store)?;let target=plan.target.as_ref().ok_or(JetVaultError::NotFound)?;let record=store.keys.iter_mut().find(|r|r.name==target.name&&r.generation==target.generation&&r.opaque_id==target.opaque_id&&r.record_hash==target.record_hash).ok_or(JetVaultError::NotFound)?;if record.key_type!=T::TAG{return Err(JetVaultError::WrongType)}if record.status==JetVaultKeyStatus::Revoked{return Err(JetVaultError::Revoked)}record.status=if plan.action==JetVaultAction::Retire{JetVaultKeyStatus::Retired}else{JetVaultKeyStatus::Revoked};record.status_unix_ms=unix_ms;record.reason_hash=vault_reason_hash(record.status,&plan.reason)?;}
    }
    store.revision=store.revision.checked_add(1).ok_or(JetVaultError::Internal{incident_id:"vault-revision"})?;vault_write_at(root,&store,plan.start_hash,plan.provider_hash,plan.start_kind,plan.start_revision,lock)?;Ok((previous,current))
}

#[cfg(target_os="linux")]
fn vault_write_at(root:&std::path::Path,store:&JetVaultStore,start_hash:[u8;32],provider_hash:[u8;32],start_kind:VaultStartKind,start_revision:u64,_lock:VaultLockGuard)->Result<(),JetVaultError>{
    use std::io::Write;use std::os::fd::AsRawFd;
    let dir_file=vault_linux_dir(root)?;let dirfd=dir_file.as_raw_fd();
    let(current,current_hash,current_provider,current_kind)=vault_read_at(root)?;
    if current_hash!=start_hash||current_provider!=provider_hash||current_kind!=start_kind||current.revision!=start_revision||(start_kind==VaultStartKind::V2&&current.repo_uuid!=store.repo_uuid){return Err(JetVaultError::Conflict)}
    let(recipients,_)=vault_recipients_at(root)?;
    let plaintext=Zeroizing(jet_vault_encode_v2(store)?);
    let ciphertext=jet_vault_encrypt_impl(&recipients,&plaintext.0).map_err(|_|JetVaultError::Locked)?;
    let mut temp=vault_linux_openat(dirfd,".",VAULT_O_RDWR|VAULT_O_TMPFILE,0o600).map_err(|error|if matches!(error.raw_os_error(),Some(38|22|95)){JetVaultError::UnsupportedProvider}else{JetVaultError::Io{operation:"create",redacted_path:"<vault-temp>"}})?;
    temp.write_all(&ciphertext).map_err(|_|JetVaultError::Io{operation:"write",redacted_path:"<vault-store>"})?;
    temp.sync_all().map_err(|_|JetVaultError::Io{operation:"fsync",redacted_path:"<vault-store>"})?;
    let staged_ciphertext=vault_read_file(&temp)?;
    let(staged,_,staged_kind)=vault_decode_ciphertext_at(root,&staged_ciphertext)?;
    if staged_kind!=VaultStartKind::V2||staged.repo_uuid!=store.repo_uuid||staged.revision!=store.revision||jet_vault_encode_v2(&staged)?!=plaintext.0{return Err(JetVaultError::InvalidEncoding)}
    if vault_test_fault("cancel-before-install"){return Err(JetVaultError::Conflict)}
    let(recheck,recheck_hash,recheck_provider,recheck_kind)=vault_read_at(root)?;
    if recheck_hash!=start_hash||recheck_provider!=provider_hash||recheck_kind!=start_kind||recheck.revision!=start_revision{return Err(JetVaultError::Conflict)}
    let current_file=if start_kind==VaultStartKind::Absent{None}else{let file=vault_linux_openat(dirfd,"secrets.age",0,0).map_err(|_|JetVaultError::Conflict)?;let(state,hash,kind)=vault_decode_ciphertext_at(root,&vault_read_file(&file)?)?;if hash!=start_hash||kind!=start_kind||state.revision!=start_revision{return Err(JetVaultError::Conflict)}Some(file)};
    if let Some(file)=&current_file{vault_linux_link_file(file,dirfd,".secrets.age.backup")?;}
    let nonce=vault_random::<16>()?;if nonce==[0;16]{return Err(JetVaultError::Internal{incident_id:"vault-zero-temp-nonce"})}
    let next_name=format!(".secrets.age.next.{}",hex_bytes(&nonce));
    vault_linux_link_file(&temp,dirfd,&next_name)?;
    #[cfg(test)]if vault_test_fault("substitute-temp-before-install"){let(_,_,dir_path)=vault_paths(root);let stolen=dir_path.join(format!("{next_name}.held"));std::fs::rename(dir_path.join(&next_name),stolen).map_err(|_|JetVaultError::Conflict)?;std::fs::write(dir_path.join(&next_name),b"substituted").map_err(|_|JetVaultError::Conflict)?;}
    let visible=vault_linux_openat(dirfd,&next_name,0,0).map_err(|_|JetVaultError::Conflict)?;
    if !vault_same_inode(&visible,&temp){return Err(JetVaultError::Conflict)}
    if start_kind==VaultStartKind::Absent{vault_linux_rename(dirfd,&next_name,"secrets.age",VAULT_RENAME_NOREPLACE)?;}else{vault_linux_rename(dirfd,&next_name,"secrets.age",VAULT_RENAME_EXCHANGE)?;}
    let installed=vault_linux_openat(dirfd,"secrets.age",0,0).map_err(|_|JetVaultError::DurabilityUnknown)?;
    if !vault_same_inode(&installed,&temp){if start_kind!=VaultStartKind::Absent{let _=vault_linux_rename(dirfd,&next_name,"secrets.age",VAULT_RENAME_EXCHANGE);}return Err(JetVaultError::DurabilityUnknown)}
    let(installed_store,_,installed_kind)=vault_decode_ciphertext_at(root,&vault_read_file(&installed)?).map_err(|_|JetVaultError::DurabilityUnknown)?;
    if installed_kind!=VaultStartKind::V2||installed_store.repo_uuid!=store.repo_uuid||installed_store.revision!=store.revision||jet_vault_encode_v2(&installed_store).map_err(|_|JetVaultError::DurabilityUnknown)?!=plaintext.0{return Err(JetVaultError::DurabilityUnknown)}
    if let Some(file)=&current_file{let quarantine=format!("{next_name}.old");if !vault_remove_exact(&dir_file,&next_name,file,&quarantine)?{return Err(JetVaultError::DurabilityUnknown)}}
    if vault_test_fault("durability-after-install"){return Err(JetVaultError::DurabilityUnknown)}
    dir_file.sync_all().map_err(|_|JetVaultError::DurabilityUnknown)?;
    if let Some(file)=&current_file{let quarantine=format!(".secrets.age.backup.done.{}",hex_bytes(&nonce));if !vault_remove_exact(&dir_file,".secrets.age.backup",file,&quarantine)?{return Err(JetVaultError::DurabilityUnknown)}dir_file.sync_all().map_err(|_|JetVaultError::DurabilityUnknown)?;}
    let _=vault_test_fault("cancel-after-install");Ok(())
}
#[cfg(test)] thread_local!{static VAULT_TEST_FAULT:std::cell::RefCell<Option<&'static str>>=const{std::cell::RefCell::new(None)};}
#[cfg(test)] pub fn jet_vault_set_test_fault(fault:Option<&'static str>){VAULT_TEST_FAULT.with(|slot|*slot.borrow_mut()=fault);}
fn vault_test_fault(name:&str)->bool{#[cfg(test)]{return VAULT_TEST_FAULT.with(|slot|slot.borrow().as_ref().is_some_and(|fault|*fault==name))}#[cfg(not(test))]{let _=name;false}}
fn hex_bytes(bytes:&[u8])->String{bytes.iter().map(|byte|format!("{byte:02x}")).collect()}
#[cfg(not(target_os="linux"))] fn vault_write_at(_root:&std::path::Path,_store:&JetVaultStore,_start_hash:[u8;32],_provider_hash:[u8;32],_start_kind:VaultStartKind,_start_revision:u64,_lock:VaultLockGuard)->Result<(),JetVaultError>{Err(JetVaultError::UnsupportedProvider)}

pub fn jet_vault_strings_from_plaintext(plaintext:&Vec<u8>)->Result<Vec<(String,String)>,JetVaultError>{if plaintext.starts_with(JVLT_MAGIC){let mut store=jet_vault_decode_v2(plaintext)?;Ok(std::mem::take(&mut store.strings))}else{let pairs=jet_vault_decode_pairs(plaintext).ok_or(JetVaultError::InvalidEncoding)?;Ok(vault_canonical_pairs(pairs)?.0)}}
pub fn jet_vault_replace_strings_at(root:&std::path::Path,pairs:Vec<(String,String)>)->Result<(),JetVaultError>{let(pairs,_)=vault_canonical_pairs(pairs)?;let(mut store,start_hash,provider_hash,start_kind)=vault_read_at(root)?;if start_kind!=VaultStartKind::V2{return Err(JetVaultError::InvalidEncoding)}let lock=vault_acquire_lock(root,store.repo_uuid,store.revision,start_hash)?;let(current,current_hash,current_provider,current_kind)=vault_read_at(root)?;if current_hash!=start_hash||current_provider!=provider_hash||current_kind!=VaultStartKind::V2||current.repo_uuid!=store.repo_uuid||current.revision!=store.revision{return Err(JetVaultError::Conflict)}for(_,value)in &mut store.strings{unsafe{zeroize(value.as_mut_vec())}}store.strings=pairs;let start_revision=store.revision;store.revision=store.revision.checked_add(1).ok_or(JetVaultError::Internal{incident_id:"vault-revision"})?;vault_write_at(root,&store,start_hash,provider_hash,start_kind,start_revision,lock)}
pub fn jet_vault_replace_strings_impl(pairs:Vec<(String,String)>)->Result<(),JetVaultError>{jet_vault_replace_strings_at(std::path::Path::new("."),pairs)}

pub fn jet_vault_commit_generate_at<T:JetVaultKey>(root:&std::path::Path,plan:JetVaultMutationPlan<T>,write:JetVaultWrite<T>)->Result<JetVaultKeyRef<T>,JetVaultError>{vault_commit_at(root,plan,write,JetVaultAction::Generate)?.1.ok_or(JetVaultError::Internal{incident_id:"vault-generate"})}
pub fn jet_vault_commit_store_at<T:JetVaultKey>(root:&std::path::Path,plan:JetVaultMutationPlan<T>,write:JetVaultWrite<T>)->Result<JetVaultKeyRef<T>,JetVaultError>{vault_commit_at(root,plan,write,JetVaultAction::Store)?.1.ok_or(JetVaultError::Internal{incident_id:"vault-store"})}
fn jet_vault_commit_raw_import_at<T:JetVaultKey>(root:&std::path::Path,plan:JetVaultMutationPlan<T>,write:JetVaultWrite<T>)->Result<JetVaultKeyRef<T>,JetVaultError>{vault_commit_at(root,plan,write,JetVaultAction::RawImport)?.1.ok_or(JetVaultError::Internal{incident_id:"vault-raw-import"})}
pub fn jet_vault_commit_rotate_at<T:JetVaultKey>(root:&std::path::Path,plan:JetVaultMutationPlan<T>,write:JetVaultWrite<T>)->Result<JetVaultRotation<T>,JetVaultError>{let(previous,current)=vault_commit_at(root,plan,write,JetVaultAction::Rotate)?;Ok(JetVaultRotation{previous:previous.ok_or(JetVaultError::NotFound)?,current:current.ok_or(JetVaultError::Internal{incident_id:"vault-rotate"})?})}
pub fn jet_vault_commit_retire_at<T:JetVaultKey>(root:&std::path::Path,plan:JetVaultMutationPlan<T>,write:JetVaultWrite<T>)->Result<(),JetVaultError>{vault_commit_at(root,plan,write,JetVaultAction::Retire).map(|_|())}
pub fn jet_vault_commit_revoke_at<T:JetVaultKey>(root:&std::path::Path,plan:JetVaultMutationPlan<T>,write:JetVaultWrite<T>)->Result<(),JetVaultError>{vault_commit_at(root,plan,write,JetVaultAction::Revoke).map(|_|())}

fn vault_cwd() -> std::path::PathBuf { std::path::PathBuf::from(".") }
pub fn jet_vault_current_impl<T:JetVaultKey>(name:&String)->Result<Option<JetVaultKeyRef<T>>,JetVaultError>{jet_vault_current_at(&vault_cwd(),name)}
pub fn jet_vault_versions_impl<T:JetVaultKey>(name:&String)->Result<Vec<JetVaultKeyRef<T>>,JetVaultError>{jet_vault_versions_at(&vault_cwd(),name)}
pub fn jet_vault_load_impl<T:JetVaultKey>(key_ref:&JetVaultKeyRef<T>)->Result<T,JetVaultError>{jet_vault_load_at(&vault_cwd(),key_ref)}
pub fn jet_vault_status_impl<T:JetVaultKey>(key_ref:&JetVaultKeyRef<T>)->Result<JetVaultKeyStatus,JetVaultError>{jet_vault_status_at(&vault_cwd(),key_ref)}
pub fn jet_vault_prepare_generate_impl<T:JetVaultKey>(name:&String)->Result<JetVaultMutationPlan<T>,JetVaultError>{jet_vault_prepare_generate_at(&vault_cwd(),name)}
pub fn jet_vault_prepare_store_impl<T:JetVaultKey>(name:&String,key:T)->Result<JetVaultMutationPlan<T>,JetVaultError>{jet_vault_prepare_store_at(&vault_cwd(),name,key)}
pub fn jet_vault_prepare_rotate_impl<T:JetVaultKey>(name:&String)->Result<JetVaultMutationPlan<T>,JetVaultError>{jet_vault_prepare_rotate_at(&vault_cwd(),name)}
pub fn jet_vault_prepare_retire_impl<T:JetVaultKey>(key_ref:&JetVaultKeyRef<T>,reason:&String)->Result<JetVaultMutationPlan<T>,JetVaultError>{jet_vault_prepare_retire_at(&vault_cwd(),key_ref,reason)}
pub fn jet_vault_prepare_revoke_impl<T:JetVaultKey>(key_ref:&JetVaultKeyRef<T>,reason:&String)->Result<JetVaultMutationPlan<T>,JetVaultError>{jet_vault_prepare_revoke_at(&vault_cwd(),key_ref,reason)}
pub fn jet_vault_commit_generate_impl<T:JetVaultKey>(write:JetVaultWrite<T>,plan:JetVaultMutationPlan<T>)->Result<JetVaultKeyRef<T>,JetVaultError>{jet_vault_commit_generate_at(&vault_cwd(),plan,write)}
pub fn jet_vault_commit_store_impl<T:JetVaultKey>(write:JetVaultWrite<T>,plan:JetVaultMutationPlan<T>)->Result<JetVaultKeyRef<T>,JetVaultError>{jet_vault_commit_store_at(&vault_cwd(),plan,write)}
pub fn jet_vault_commit_rotate_impl<T:JetVaultKey>(write:JetVaultWrite<T>,plan:JetVaultMutationPlan<T>)->Result<JetVaultRotation<T>,JetVaultError>{jet_vault_commit_rotate_at(&vault_cwd(),plan,write)}
pub fn jet_vault_commit_retire_impl<T:JetVaultKey>(write:JetVaultWrite<T>,plan:JetVaultMutationPlan<T>)->Result<(),JetVaultError>{jet_vault_commit_retire_at(&vault_cwd(),plan,write)}
pub fn jet_vault_commit_revoke_impl<T:JetVaultKey>(write:JetVaultWrite<T>,plan:JetVaultMutationPlan<T>)->Result<(),JetVaultError>{jet_vault_commit_revoke_at(&vault_cwd(),plan,write)}
pub fn jet_vault_expert_prepare_import_signing_impl(name:&String,bytes:Vec<u8>)->Result<JetVaultMutationPlan<JetSigningKey>,JetVaultError>{let mut bytes=Zeroizing(bytes);if bytes.len()!=32{return Err(JetVaultError::InvalidEncoding)}let key=JetSigningKey::from_bytes(std::mem::take(&mut bytes.0))?;jet_vault_prepare_raw_import_at(&vault_cwd(),name,key)}
pub fn jet_vault_expert_prepare_import_x25519_impl(name:&String,bytes:Vec<u8>)->Result<JetVaultMutationPlan<JetX25519SecretKey>,JetVaultError>{let mut bytes=Zeroizing(bytes);if bytes.len()!=32{return Err(JetVaultError::InvalidEncoding)}let key=JetX25519SecretKey::from_bytes(std::mem::take(&mut bytes.0))?;jet_vault_prepare_raw_import_at(&vault_cwd(),name,key)}
pub fn jet_vault_expert_commit_import_signing_impl(write:JetVaultWrite<JetSigningKey>,plan:JetVaultMutationPlan<JetSigningKey>)->Result<JetVaultKeyRef<JetSigningKey>,JetVaultError>{jet_vault_commit_raw_import_at(&vault_cwd(),plan,write)}
pub fn jet_vault_expert_commit_import_x25519_impl(write:JetVaultWrite<JetX25519SecretKey>,plan:JetVaultMutationPlan<JetX25519SecretKey>)->Result<JetVaultKeyRef<JetX25519SecretKey>,JetVaultError>{jet_vault_commit_raw_import_at(&vault_cwd(),plan,write)}

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
