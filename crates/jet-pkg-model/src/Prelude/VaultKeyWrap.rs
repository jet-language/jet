// D-CRYPTO-KEYWRAP1=A: canonical JVKW/v1 portable backup for typed vault keys.
// Emitted after Crypto.rs and SecretsCrypto.rs in the hidden bridge crate.

const JVKW_MAGIC: &[u8; 4] = b"JVKW";
const JVKW_VERSION: u8 = 1;
const JVKW_MAX: usize = 8_192;
const JVKW_PAYLOAD_LEN: usize = 64;
const JVKW_TAG_LEN: usize = 16;
const JVKW_RECIPIENT_STANZA_LEN: usize = 80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum JetVaultKeyWrapMode { Recipient = 1, Passphrase = 2 }

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JetVaultKeyWrapError {
    InvalidEncoding,
    UnsupportedVersion,
    UnsupportedMode,
    UnsupportedKeyType,
    InvalidLength,
    WeakPassphrase,
    OpenFailed,
    EntropyUnavailable,
    ResourceUnavailable,
    Vault(JetVaultError),
    Internal { incident_id: &'static str },
}

impl std::fmt::Display for JetVaultKeyWrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidEncoding => f.write_str("invalid wrapped vault key encoding"),
            Self::UnsupportedVersion => f.write_str("unsupported wrapped vault key version"),
            Self::UnsupportedMode => f.write_str("unsupported wrapped vault key mode"),
            Self::UnsupportedKeyType => f.write_str("unsupported wrapped vault key type"),
            Self::InvalidLength => f.write_str("invalid wrapped vault key length"),
            Self::WeakPassphrase => f.write_str("recovery passphrase must contain 16..=1048576 bytes"),
            Self::OpenFailed => f.write_str("open failed"),
            Self::EntropyUnavailable => f.write_str("the operating system could not provide cryptographic randomness"),
            Self::ResourceUnavailable => f.write_str("cryptographic resource unavailable"),
            Self::Vault(error) => write!(f, "{error}"),
            Self::Internal { incident_id } => write!(f, "internal key-wrap failure; incident {incident_id}"),
        }
    }
}

impl From<JetVaultError> for JetVaultKeyWrapError {
    fn from(error: JetVaultError) -> Self { Self::Vault(error) }
}

#[derive(Clone, Eq, PartialEq)]
pub struct JetWrappedVaultKey {
    bytes: Vec<u8>,
    mode: JetVaultKeyWrapMode,
    key_type: u8,
    origin_name: String,
    origin_generation: u64,
    header_len: usize,
}

impl JetWrappedVaultKey {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, JetVaultKeyWrapError> {
        let parsed = jvkw_parse(&bytes)?;
        Ok(Self {
            bytes,
            mode: parsed.mode,
            key_type: parsed.key_type,
            origin_name: parsed.origin.name,
            origin_generation: parsed.origin.generation,
            header_len: parsed.header_end - 16,
        })
    }
    pub fn bytes(&self) -> Vec<u8> { self.bytes.clone() }
    pub fn mode(&self) -> JetVaultKeyWrapMode { self.mode }
    pub fn key_type(&self) -> u8 { self.key_type }
    pub fn header_len(&self) -> usize { self.header_len }
}

impl std::fmt::Display for JetWrappedVaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mode = match self.mode { JetVaultKeyWrapMode::Recipient => "recipient", JetVaultKeyWrapMode::Passphrase => "passphrase" };
        let key_type = match self.key_type { 1 => "signing", 2 => "x25519", _ => "unsupported" };
        write!(f, "WrappedVaultKey(mode:{mode},type:{key_type},origin:{}@v{})", self.origin_name, self.origin_generation)
    }
}
impl std::fmt::Debug for JetWrappedVaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { std::fmt::Display::fmt(self, f) }
}

#[derive(Clone, Copy)]
pub enum JetVaultKeyUnlock<'a> {
    Recipient(&'a JetX25519SecretKey),
    Passphrase(&'a Secret),
}

#[derive(Clone, Copy)]
enum JvkwModeHeader {
    Recipient { count: usize, ephemeral: [u8; 32], salt: [u8; 16], stanzas_at: usize },
    Passphrase { salt: [u8; 16] },
}

struct JvkwParsed {
    mode: JetVaultKeyWrapMode,
    key_type: u8,
    origin: JetVaultOrigin,
    export_id: [u8; 16],
    payload_nonce: [u8; 24],
    header_end: usize,
    mode_header: JvkwModeHeader,
}

fn jvkw_take<'a>(bytes: &'a [u8], at: &mut usize, count: usize) -> Result<&'a [u8], JetVaultKeyWrapError> {
    let end = at.checked_add(count).ok_or(JetVaultKeyWrapError::InvalidLength)?;
    let value = bytes.get(*at..end).ok_or(JetVaultKeyWrapError::InvalidLength)?;
    *at = end;
    Ok(value)
}
fn jvkw_u16(bytes: &[u8], at: &mut usize) -> Result<usize, JetVaultKeyWrapError> { Ok(u16::from_le_bytes(jvkw_take(bytes, at, 2)?.try_into().unwrap()) as usize) }
fn jvkw_u32(bytes: &[u8], at: &mut usize) -> Result<usize, JetVaultKeyWrapError> { Ok(u32::from_le_bytes(jvkw_take(bytes, at, 4)?.try_into().unwrap()) as usize) }
fn jvkw_u64(bytes: &[u8], at: &mut usize) -> Result<u64, JetVaultKeyWrapError> { Ok(u64::from_le_bytes(jvkw_take(bytes, at, 8)?.try_into().unwrap())) }
fn jvkw_array<const N: usize>(bytes: &[u8], at: &mut usize) -> Result<[u8; N], JetVaultKeyWrapError> { jvkw_take(bytes, at, N)?.try_into().map_err(|_| JetVaultKeyWrapError::InvalidLength) }

fn jvkw_parse(bytes: &[u8]) -> Result<JvkwParsed, JetVaultKeyWrapError> {
    if bytes.len() > JVKW_MAX || bytes.len() < 16 { return Err(JetVaultKeyWrapError::InvalidLength); }
    if &bytes[..4] != JVKW_MAGIC { return Err(JetVaultKeyWrapError::InvalidEncoding); }
    if bytes[4] != JVKW_VERSION { return Err(JetVaultKeyWrapError::UnsupportedVersion); }
    let mode = match bytes[5] { 1 => JetVaultKeyWrapMode::Recipient, 2 => JetVaultKeyWrapMode::Passphrase, _ => return Err(JetVaultKeyWrapError::UnsupportedMode) };
    let key_type = match bytes[6] { 1 | 2 => bytes[6], _ => return Err(JetVaultKeyWrapError::UnsupportedKeyType) };
    if bytes[7] != 0 { return Err(JetVaultKeyWrapError::InvalidEncoding); }
    let mut fixed_at = 8;
    let header_len = jvkw_u32(bytes, &mut fixed_at)?;
    let ciphertext_len = jvkw_u32(bytes, &mut fixed_at)?;
    if ciphertext_len != JVKW_PAYLOAD_LEN { return Err(JetVaultKeyWrapError::InvalidLength); }
    let header_end = 16usize.checked_add(header_len).ok_or(JetVaultKeyWrapError::InvalidLength)?;
    let expected = header_end.checked_add(JVKW_PAYLOAD_LEN + JVKW_TAG_LEN).ok_or(JetVaultKeyWrapError::InvalidLength)?;
    if expected != bytes.len() || expected > JVKW_MAX { return Err(JetVaultKeyWrapError::InvalidLength); }
    let mut at = 16;
    let repo_uuid = jvkw_array(bytes, &mut at)?;
    let name_len = jvkw_u16(bytes, &mut at)?;
    if !(1..=128).contains(&name_len) { return Err(JetVaultKeyWrapError::InvalidLength); }
    let name = String::from_utf8(jvkw_take(bytes, &mut at, name_len)?.to_vec()).map_err(|_| JetVaultKeyWrapError::InvalidEncoding)?;
    jet_vault_validate_name(&name).map_err(|_| JetVaultKeyWrapError::InvalidEncoding)?;
    let generation = jvkw_u64(bytes, &mut at)?;
    let opaque_id = jvkw_array(bytes, &mut at)?;
    let record_hash = jvkw_array(bytes, &mut at)?;
    let created_unix_ms = jvkw_u64(bytes, &mut at)?;
    let export_id = jvkw_array(bytes, &mut at)?;
    let payload_nonce = jvkw_array(bytes, &mut at)?;
    if repo_uuid == [0; 16] || generation == 0 || opaque_id == [0; 16] || record_hash == [0; 32] || created_unix_ms == 0 || export_id == [0; 16] {
        return Err(JetVaultKeyWrapError::InvalidEncoding);
    }
    let mode_header = match mode {
        JetVaultKeyWrapMode::Recipient => {
            let count = jvkw_u16(bytes, &mut at)?;
            if !(1..=16).contains(&count) { return Err(JetVaultKeyWrapError::InvalidLength); }
            let ephemeral = jvkw_array(bytes, &mut at)?;
            let salt = jvkw_array(bytes, &mut at)?;
            let stanzas_at = at;
            let mut previous: Option<[u8; 32]> = None;
            for _ in 0..count {
                let recipient: [u8; 32] = jvkw_array(bytes, &mut at)?;
                jvkw_take(bytes, &mut at, 48)?;
                if previous.is_some_and(|prior| prior >= recipient) { return Err(JetVaultKeyWrapError::InvalidEncoding); }
                previous = Some(recipient);
            }
            JvkwModeHeader::Recipient { count, ephemeral, salt, stanzas_at }
        }
        JetVaultKeyWrapMode::Passphrase => {
            let salt = jvkw_array(bytes, &mut at)?;
            if jvkw_u32(bytes, &mut at)? != 65_536 || jvkw_u32(bytes, &mut at)? != 3 || jvkw_take(bytes, &mut at, 1)?[0] != 1 || jvkw_take(bytes, &mut at, 3)? != [0, 0, 0] {
                return Err(JetVaultKeyWrapError::InvalidEncoding);
            }
            JvkwModeHeader::Passphrase { salt }
        }
    };
    if at != header_end { return Err(JetVaultKeyWrapError::InvalidLength); }
    Ok(JvkwParsed { mode, key_type, origin: JetVaultOrigin { repo_uuid, name, generation, opaque_id, record_hash }, export_id, payload_nonce, header_end, mode_header })
}

pub fn jet_vault_wrapped_from_bytes_impl(bytes: Vec<u8>) -> Result<JetWrappedVaultKey, JetVaultKeyWrapError> { JetWrappedVaultKey::from_bytes(bytes) }
pub fn jet_vault_wrapped_bytes_impl(wrapped: &JetWrappedVaultKey) -> Vec<u8> { wrapped.bytes() }
pub fn jet_vault_unlock_recipient_impl(identity: &JetX25519SecretKey) -> JetVaultKeyUnlock<'_> { JetVaultKeyUnlock::Recipient(identity) }
pub fn jet_vault_unlock_passphrase_impl(passphrase: &Secret) -> JetVaultKeyUnlock<'_> { JetVaultKeyUnlock::Passphrase(passphrase) }

fn jvkw_push_u16(out: &mut Vec<u8>, value: usize) -> Result<(), JetVaultKeyWrapError> {
    out.extend_from_slice(&u16::try_from(value).map_err(|_| JetVaultKeyWrapError::InvalidLength)?.to_le_bytes());
    Ok(())
}
fn jvkw_push_u32(out: &mut Vec<u8>, value: usize) -> Result<(), JetVaultKeyWrapError> {
    out.extend_from_slice(&u32::try_from(value).map_err(|_| JetVaultKeyWrapError::InvalidLength)?.to_le_bytes());
    Ok(())
}
fn jvkw_random<const N: usize>() -> Result<Zeroizing<[u8; N]>, JetVaultKeyWrapError> {
    let mut bytes = Zeroizing([0; N]);
    jet_crypto_entropy_fill(&mut bytes.0).map_err(|_| JetVaultKeyWrapError::EntropyUnavailable)?;
    Ok(bytes)
}

#[cfg(test)]
thread_local! {
    static JVKW_TEST_TIME: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    static JVKW_RECIPIENT_OPEN_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)] pub fn jet_vault_keywrap_set_test_time(value: Option<u64>) { JVKW_TEST_TIME.with(|slot| slot.set(value)); }
#[cfg(test)] pub fn jet_vault_keywrap_test_recipient_open_count() -> usize { JVKW_RECIPIENT_OPEN_COUNT.with(|slot| slot.get()) }
fn jvkw_now_ms() -> Result<u64, JetVaultKeyWrapError> {
    #[cfg(test)] if let Some(value) = JVKW_TEST_TIME.with(|slot| slot.get()) { return Ok(value); }
    u64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-clock" })?.as_millis())
        .map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-clock" })
}

fn jvkw_fixed(mode: JetVaultKeyWrapMode, key_type: u8, header_len: usize) -> Result<Vec<u8>, JetVaultKeyWrapError> {
    let mut out = Vec::with_capacity(16 + header_len + JVKW_PAYLOAD_LEN + JVKW_TAG_LEN);
    out.extend_from_slice(JVKW_MAGIC);
    out.extend_from_slice(&[JVKW_VERSION, mode as u8, key_type, 0]);
    jvkw_push_u32(&mut out, header_len)?;
    jvkw_push_u32(&mut out, JVKW_PAYLOAD_LEN)?;
    Ok(out)
}

fn jvkw_common(
    out: &mut Vec<u8>,
    reference_repo: [u8; 16],
    name: &str,
    generation: u64,
    opaque_id: [u8; 16],
    record_hash: [u8; 32],
    created_unix_ms: u64,
    export_id: &[u8; 16],
    payload_nonce: &[u8; 24],
) -> Result<(), JetVaultKeyWrapError> {
    out.extend_from_slice(&reference_repo);
    jvkw_push_u16(out, name.len())?;
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(&generation.to_le_bytes());
    out.extend_from_slice(&opaque_id);
    out.extend_from_slice(&record_hash);
    out.extend_from_slice(&created_unix_ms.to_le_bytes());
    out.extend_from_slice(export_id);
    out.extend_from_slice(payload_nonce);
    Ok(())
}

fn jvkw_source<T: JetVaultKey>(root: &std::path::Path, reference: &JetVaultKeyRef<T>) -> Result<(JetVaultOrigin, Zeroizing<Vec<u8>>, [u8; 32]), JetVaultKeyWrapError> {
    let (store, _, _) = vault_read_contents_at(root)?;
    let record = vault_find(&store, reference)?;
    if record.status == JetVaultKeyStatus::Revoked { return Err(JetVaultKeyWrapError::Vault(JetVaultError::Revoked)); }
    let origin = JetVaultOrigin { repo_uuid: reference.repo_uuid, name: reference.name.clone(), generation: reference.generation, opaque_id: reference.opaque_id, record_hash: reference.record_hash };
    let key = Zeroizing(record.key.clone());
    let typed = T::from_bytes(key.0.clone()).map_err(JetVaultKeyWrapError::Vault)?;
    let public: [u8; 32] = typed.public_bytes().try_into().map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-public" })?;
    Ok((origin, key, public))
}

fn jvkw_seal_payload(mut header: Vec<u8>, backup_key: &[u8; 32], nonce: &[u8; 24], key: &[u8], public: &[u8; 32]) -> Result<JetWrappedVaultKey, JetVaultKeyWrapError> {
    let mut payload = Zeroizing(Vec::with_capacity(64));
    payload.0.extend_from_slice(key);
    payload.0.extend_from_slice(public);
    if payload.0.len() != JVKW_PAYLOAD_LEN { return Err(JetVaultKeyWrapError::Internal { incident_id: "jvkw-payload" }); }
    let mut aad = b"JVKW1 payload".to_vec();
    aad.extend_from_slice(&header);
    let ciphertext = XChaCha20Poly1305::new_from_slice(backup_key).map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-payload-key" })?
        .encrypt(XNonce::from_slice(nonce), Payload { msg: &payload.0, aad: &aad })
        .map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-payload-seal" })?;
    header.extend_from_slice(&ciphertext);
    JetWrappedVaultKey::from_bytes(header)
}

pub fn jet_vault_export_to_recipients_at<T: JetVaultKey>(
    root: &std::path::Path,
    reference: &JetVaultKeyRef<T>,
    recipients: &Vec<JetX25519PublicKey>,
) -> Result<JetWrappedVaultKey, JetVaultKeyWrapError> {
    if !(1..=16).contains(&recipients.len()) { return Err(JetVaultKeyWrapError::InvalidLength); }
    let (origin, key, public) = jvkw_source(root, reference)?;
    jvkw_export_to_recipients::<T>(origin, key, public, jvkw_now_ms()?, recipients)
}

fn jvkw_export_to_recipients<T: JetVaultKey>(
    origin: JetVaultOrigin,
    key: Zeroizing<Vec<u8>>,
    public: [u8; 32],
    created_unix_ms: u64,
    recipients: &Vec<JetX25519PublicKey>,
) -> Result<JetWrappedVaultKey, JetVaultKeyWrapError> {
    let mut recipients: Vec<[u8; 32]> = recipients.iter().map(|recipient| recipient.0).collect();
    recipients.sort();
    if recipients.windows(2).any(|pair| pair[0] == pair[1]) { return Err(JetVaultKeyWrapError::InvalidEncoding); }
    let mut backup_key = jvkw_random::<32>()?;
    let mut payload_nonce = jvkw_random::<24>()?;
    let mut export_id = jvkw_random::<16>()?;
    let mut ephemeral_secret = jvkw_random::<32>()?;
    let mut salt = jvkw_random::<16>()?;
    if backup_key.0 == [0; 32] || export_id.0 == [0; 16] { return Err(JetVaultKeyWrapError::Internal { incident_id: "jvkw-zero-random" }); }
    let ephemeral_public = x25519_dalek::x25519(ephemeral_secret.0, x25519_dalek::X25519_BASEPOINT_BYTES);
    let common_len = 122usize.checked_add(origin.name.len()).ok_or(JetVaultKeyWrapError::InvalidLength)?;
    let header_len = common_len.checked_add(50 + recipients.len() * JVKW_RECIPIENT_STANZA_LEN).ok_or(JetVaultKeyWrapError::InvalidLength)?;
    let mut header = jvkw_fixed(JetVaultKeyWrapMode::Recipient, T::TAG, header_len)?;
    jvkw_common(&mut header, origin.repo_uuid, &origin.name, origin.generation, origin.opaque_id, origin.record_hash, created_unix_ms, &export_id.0, &payload_nonce.0)?;
    jvkw_push_u16(&mut header, recipients.len())?;
    header.extend_from_slice(&ephemeral_public);
    header.extend_from_slice(&salt.0);
    let stanza_prefix = header.clone();
    for recipient in recipients {
        let mut shared = Zeroizing(x25519_checked(ephemeral_secret.0, recipient).map_err(|_| JetVaultKeyWrapError::OpenFailed)?);
        let mut key_info = b"JVKW1 recipient key".to_vec();
        key_info.extend_from_slice(&export_id.0); key_info.extend_from_slice(&ephemeral_public); key_info.extend_from_slice(&recipient);
        let mut nonce_info = b"JVKW1 recipient nonce".to_vec();
        nonce_info.extend_from_slice(&export_id.0); nonce_info.extend_from_slice(&ephemeral_public); nonce_info.extend_from_slice(&recipient);
        let mut kek = Zeroizing(hkdf32(&shared.0, &salt.0, &key_info).map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-recipient-kdf" })?);
        let mut nonce = Zeroizing(hkdf24(&shared.0, &salt.0, &nonce_info).map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-recipient-kdf" })?);
        let mut aad = b"JVKW1 recipient aad".to_vec(); aad.extend_from_slice(&stanza_prefix); aad.extend_from_slice(&recipient);
        let wrapped = XChaCha20Poly1305::new_from_slice(&kek.0).map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-recipient-key" })?
            .encrypt(XNonce::from_slice(&nonce.0), Payload { msg: &backup_key.0, aad: &aad })
            .map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-recipient-seal" })?;
        header.extend_from_slice(&recipient); header.extend_from_slice(&wrapped);
    }
    jvkw_seal_payload(header, &backup_key.0, &payload_nonce.0, &key.0, &public)
}

static JVKW_ARGON_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn jvkw_argon(password: &Secret, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, JetVaultKeyWrapError> {
    if !(16..=1_048_576).contains(&password.0.len()) { return Err(JetVaultKeyWrapError::WeakPassphrase); }
    let _admission = JVKW_ARGON_LOCK.lock().map_err(|_| JetVaultKeyWrapError::ResourceUnavailable)?;
    let params = argon2::Params::new(65_536, 3, 1, Some(32)).map_err(|_| JetVaultKeyWrapError::Internal { incident_id: "jvkw-argon-params" })?;
    let engine = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = Zeroizing([0; 32]);
    engine.hash_password_into(&password.0, salt, &mut key.0).map_err(|_| JetVaultKeyWrapError::ResourceUnavailable)?;
    Ok(key)
}

pub fn jet_vault_export_to_passphrase_at<T: JetVaultKey>(
    root: &std::path::Path,
    reference: &JetVaultKeyRef<T>,
    passphrase: &Secret,
) -> Result<JetWrappedVaultKey, JetVaultKeyWrapError> {
    if !(16..=1_048_576).contains(&passphrase.0.len()) { return Err(JetVaultKeyWrapError::WeakPassphrase); }
    let (origin, key, public) = jvkw_source(root, reference)?;
    jvkw_export_to_passphrase::<T>(origin, key, public, jvkw_now_ms()?, passphrase)
}

fn jvkw_export_to_passphrase<T: JetVaultKey>(
    origin: JetVaultOrigin,
    key: Zeroizing<Vec<u8>>,
    public: [u8; 32],
    created_unix_ms: u64,
    passphrase: &Secret,
) -> Result<JetWrappedVaultKey, JetVaultKeyWrapError> {
    let mut salt = jvkw_random::<16>()?;
    let mut payload_nonce = jvkw_random::<24>()?;
    let mut export_id = jvkw_random::<16>()?;
    if export_id.0 == [0; 16] { return Err(JetVaultKeyWrapError::Internal { incident_id: "jvkw-zero-random" }); }
    let mut backup_key = jvkw_argon(passphrase, &salt.0)?;
    let header_len = 122usize.checked_add(origin.name.len()).and_then(|value| value.checked_add(28)).ok_or(JetVaultKeyWrapError::InvalidLength)?;
    let mut header = jvkw_fixed(JetVaultKeyWrapMode::Passphrase, T::TAG, header_len)?;
    jvkw_common(&mut header, origin.repo_uuid, &origin.name, origin.generation, origin.opaque_id, origin.record_hash, created_unix_ms, &export_id.0, &payload_nonce.0)?;
    header.extend_from_slice(&salt.0); header.extend_from_slice(&65_536u32.to_le_bytes()); header.extend_from_slice(&3u32.to_le_bytes()); header.extend_from_slice(&[1, 0, 0, 0]);
    jvkw_seal_payload(header, &backup_key.0, &payload_nonce.0, &key.0, &public)
}

fn jvkw_open_recipient(parsed: &JvkwParsed, bytes: &[u8], identity: &JetX25519SecretKey) -> Result<Zeroizing<[u8; 32]>, JetVaultKeyWrapError> {
    #[cfg(test)] JVKW_RECIPIENT_OPEN_COUNT.with(|slot| slot.set(0));
    let JvkwModeHeader::Recipient { count, ephemeral, salt, stanzas_at } = parsed.mode_header else { return Err(JetVaultKeyWrapError::OpenFailed); };
    let mut private = Zeroizing([0; 32]); private.0.copy_from_slice(&identity.0);
    let own_public = x25519_dalek::x25519(private.0, x25519_dalek::X25519_BASEPOINT_BYTES);
    let match_index = (0..count).find(|index| {
        let start = stanzas_at + index * JVKW_RECIPIENT_STANZA_LEN;
        bool::from(bytes[start..start + 32].ct_eq(&own_public))
    });
    let index = match_index.unwrap_or(0);
    let stanza_at = stanzas_at + index * JVKW_RECIPIENT_STANZA_LEN;
    let recipient_public: [u8; 32] = bytes[stanza_at..stanza_at + 32].try_into().unwrap();
    #[cfg(test)] JVKW_RECIPIENT_OPEN_COUNT.with(|slot| slot.set(slot.get() + 1));
    let mut shared = Zeroizing(x25519_checked(private.0, ephemeral).map_err(|_| JetVaultKeyWrapError::OpenFailed)?);
    let mut key_info = b"JVKW1 recipient key".to_vec(); key_info.extend_from_slice(&parsed.export_id); key_info.extend_from_slice(&ephemeral); key_info.extend_from_slice(&recipient_public);
    let mut nonce_info = b"JVKW1 recipient nonce".to_vec(); nonce_info.extend_from_slice(&parsed.export_id); nonce_info.extend_from_slice(&ephemeral); nonce_info.extend_from_slice(&recipient_public);
    let mut kek = Zeroizing(hkdf32(&shared.0, &salt, &key_info).map_err(|_| JetVaultKeyWrapError::OpenFailed)?);
    let mut nonce = Zeroizing(hkdf24(&shared.0, &salt, &nonce_info).map_err(|_| JetVaultKeyWrapError::OpenFailed)?);
    let mut aad = b"JVKW1 recipient aad".to_vec(); aad.extend_from_slice(&bytes[..stanzas_at]); aad.extend_from_slice(&recipient_public);
    let opened = XChaCha20Poly1305::new_from_slice(&kek.0).map_err(|_| JetVaultKeyWrapError::OpenFailed)?
        .decrypt(XNonce::from_slice(&nonce.0), Payload { msg: &bytes[stanza_at + 32..stanza_at + 80], aad: &aad })
        .map_err(|_| JetVaultKeyWrapError::OpenFailed)?;
    if match_index.is_none() { return Err(JetVaultKeyWrapError::OpenFailed); }
    let backup: [u8; 32] = opened.as_slice().try_into().map_err(|_| JetVaultKeyWrapError::OpenFailed)?;
    let mut opened = Zeroizing(opened); zeroize(&mut opened.0);
    Ok(Zeroizing(backup))
}

fn jvkw_open<T: JetVaultKey>(wrapped: &JetWrappedVaultKey, unlock: JetVaultKeyUnlock<'_>) -> Result<(JvkwParsed, Zeroizing<Vec<u8>>, [u8; 32]), JetVaultKeyWrapError> {
    let parsed = jvkw_parse(&wrapped.bytes)?;
    let mut backup_key = match (parsed.mode, unlock) {
        (JetVaultKeyWrapMode::Recipient, JetVaultKeyUnlock::Recipient(identity)) => jvkw_open_recipient(&parsed, &wrapped.bytes, identity)?,
        (JetVaultKeyWrapMode::Passphrase, JetVaultKeyUnlock::Passphrase(passphrase)) => {
            let JvkwModeHeader::Passphrase { salt } = parsed.mode_header else { return Err(JetVaultKeyWrapError::OpenFailed); };
            jvkw_argon(passphrase, &salt).map_err(|error| if error == JetVaultKeyWrapError::WeakPassphrase { error } else if error == JetVaultKeyWrapError::ResourceUnavailable { error } else { JetVaultKeyWrapError::OpenFailed })?
        }
        _ => return Err(JetVaultKeyWrapError::OpenFailed),
    };
    let mut aad = b"JVKW1 payload".to_vec(); aad.extend_from_slice(&wrapped.bytes[..parsed.header_end]);
    let plaintext = XChaCha20Poly1305::new_from_slice(&backup_key.0).map_err(|_| JetVaultKeyWrapError::OpenFailed)?
        .decrypt(XNonce::from_slice(&parsed.payload_nonce), Payload { msg: &wrapped.bytes[parsed.header_end..], aad: &aad })
        .map_err(|_| JetVaultKeyWrapError::OpenFailed)?;
    let mut plaintext = Zeroizing(plaintext);
    if plaintext.0.len() != 64 || parsed.key_type != T::TAG { return Err(JetVaultKeyWrapError::OpenFailed); }
    let public: [u8; 32] = plaintext.0[32..].try_into().map_err(|_| JetVaultKeyWrapError::OpenFailed)?;
    let key = Zeroizing(plaintext.0[..32].to_vec());
    let typed = T::from_bytes(key.0.clone()).map_err(|_| JetVaultKeyWrapError::OpenFailed)?;
    let derived = typed.public_bytes();
    if derived.len() != 32 || !bool::from(derived.ct_eq(&public)) { return Err(JetVaultKeyWrapError::OpenFailed); }
    Ok((parsed, key, public))
}

pub struct JetVaultWrappedImportPlan<T> {
    name: String,
    key: Zeroizing<Vec<u8>>,
    origin: JetVaultOrigin,
    public_key_hash: [u8; 32],
    key_digest: [u8; 32],
    repo_uuid: [u8; 16],
    start_revision: u64,
    start_hash: [u8; 32],
    provider_hash: [u8; 32],
    start_kind: VaultStartKind,
    operation_hash: [u8; 32],
    existing: Option<JetVaultKeyRef<T>>,
    destination_opaque_id: [u8; 16],
    transition_from: u64,
    transition_to: u64,
    issued: std::time::Instant,
    deadline: std::time::Instant,
    expires_unix_ms: u64,
}
impl<T> std::fmt::Debug for JetVaultWrappedImportPlan<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("WrappedImportPlan(<redacted>)") }
}

pub fn jet_vault_prepare_import_wrapped_at<T: JetVaultKey>(
    root: &std::path::Path,
    name: &str,
    wrapped: JetWrappedVaultKey,
    unlock: JetVaultKeyUnlock<'_>,
) -> Result<JetVaultWrappedImportPlan<T>, JetVaultKeyWrapError> {
    jet_vault_validate_name(name)?;
    let wrapped_hash = vault_hash(&[b"JVKW1 wrapped bytes", &wrapped.bytes]);
    let (parsed, key, public) = jvkw_open::<T>(&wrapped, unlock)?;
    let public_key_hash = vault_hash(&[b"JVLT2 public key", &[T::TAG], &public]);
    let key_digest = vault_key_digest(T::TAG, &key.0);
    let (mut store, start_hash, provider_hash, start_kind) = vault_read_at(root)?;
    if start_kind != VaultStartKind::V2 {
        let provisional = jvkw_random::<16>()?;
        if provisional.0 == [0; 16] { return Err(JetVaultKeyWrapError::Internal { incident_id: "jvkw-zero-uuid" }); }
        store.repo_uuid = provisional.0;
    }
    if store.keys.iter().any(|record| record.name == name && record.key_type != T::TAG) { return Err(JetVaultKeyWrapError::Vault(JetVaultError::WrongType)); }
    let same_origin = (store.repo_uuid == parsed.origin.repo_uuid).then(|| store.keys.iter().find(|record| {
        record.name == parsed.origin.name && record.generation == parsed.origin.generation && record.opaque_id == parsed.origin.opaque_id && record.record_hash == parsed.origin.record_hash
    })).flatten();
    let existing = if let Some(record) = same_origin {
        if record.key_type != T::TAG || record.public_key_hash != public_key_hash || vault_key_digest(T::TAG, &record.key) != key_digest { return Err(JetVaultKeyWrapError::OpenFailed); }
        if record.status == JetVaultKeyStatus::Revoked { return Err(JetVaultKeyWrapError::Vault(JetVaultError::Revoked)); }
        Some(vault_ref(&store, record))
    } else { None };
    let transition_from = store.keys.iter().find(|record| record.name == name && record.key_type == T::TAG && record.status == JetVaultKeyStatus::Active).map(|record| record.generation).unwrap_or(0);
    let transition_to = if existing.is_some() { transition_from } else { store.keys.iter().filter(|record| record.name == name && record.key_type == T::TAG).map(|record| record.generation).max().unwrap_or(0).checked_add(1).ok_or(JetVaultKeyWrapError::Internal { incident_id: "jvkw-generation" })? };
    let destination_opaque_id = if existing.is_some() { [0; 16] } else { let value = jvkw_random::<16>()?; if value.0 == [0; 16] { return Err(JetVaultKeyWrapError::Internal { incident_id: "jvkw-zero-id" }); } value.0 };
    let mut operation = Vec::new();
    operation.extend_from_slice(b"JVKW1 import operation"); operation.extend_from_slice(&store.repo_uuid); operation.extend_from_slice(&store.revision.to_le_bytes()); operation.extend_from_slice(&start_hash); operation.extend_from_slice(&provider_hash);
    operation.push(T::TAG); jvkw_push_u16(&mut operation, name.len())?; operation.extend_from_slice(name.as_bytes()); operation.extend_from_slice(&parsed.origin.repo_uuid); jvkw_push_u16(&mut operation, parsed.origin.name.len())?; operation.extend_from_slice(parsed.origin.name.as_bytes()); operation.extend_from_slice(&parsed.origin.generation.to_le_bytes()); operation.extend_from_slice(&parsed.origin.opaque_id); operation.extend_from_slice(&parsed.origin.record_hash); operation.extend_from_slice(&public_key_hash); operation.extend_from_slice(&key_digest); operation.extend_from_slice(&wrapped_hash); operation.extend_from_slice(&destination_opaque_id); operation.extend_from_slice(&transition_from.to_le_bytes()); operation.extend_from_slice(&transition_to.to_le_bytes());
    let operation_hash = vault_hash(&[&operation]);
    let issued = std::time::Instant::now();
    let deadline = issued.checked_add(std::time::Duration::from_secs(300)).ok_or(JetVaultKeyWrapError::Internal { incident_id: "jvkw-deadline" })?;
    let expires_unix_ms = jvkw_now_ms()?.checked_add(300_000).ok_or(JetVaultKeyWrapError::Internal { incident_id: "jvkw-deadline" })?;
    Ok(JetVaultWrappedImportPlan { name: name.into(), key, origin: parsed.origin, public_key_hash, key_digest, repo_uuid: store.repo_uuid, start_revision: store.revision, start_hash, provider_hash, start_kind, operation_hash, existing, destination_opaque_id, transition_from, transition_to, issued, deadline, expires_unix_ms })
}

pub fn jet_vault_authorize_wrapped_import_impl<T: JetVaultKey>(plan: &JetVaultWrappedImportPlan<T>, reason: &str) -> Result<JetVaultWrite<T>, JetVaultKeyWrapError> {
    vault_validate_reason(reason)?;
    let now = std::time::Instant::now();
    if now > plan.deadline || now.duration_since(plan.issued) > std::time::Duration::from_secs(300) { return Err(JetVaultKeyWrapError::Vault(JetVaultError::Conflict)); }
    let mut bytes = Vec::new(); bytes.extend_from_slice(b"JVKW1 authority preview"); bytes.extend_from_slice(&plan.operation_hash); bytes.push(T::TAG); bytes.extend_from_slice(&plan.repo_uuid); bytes.extend_from_slice(&plan.start_revision.to_le_bytes()); jvkw_push_u16(&mut bytes, plan.name.len())?; bytes.extend_from_slice(plan.name.as_bytes()); bytes.extend_from_slice(&plan.origin.repo_uuid); jvkw_push_u16(&mut bytes, plan.origin.name.len())?; bytes.extend_from_slice(plan.origin.name.as_bytes()); bytes.extend_from_slice(&plan.origin.generation.to_le_bytes()); bytes.extend_from_slice(&plan.transition_from.to_le_bytes()); bytes.extend_from_slice(&plan.transition_to.to_le_bytes()); jvkw_push_u16(&mut bytes, reason.len())?; bytes.extend_from_slice(reason.as_bytes()); bytes.extend_from_slice(&plan.expires_unix_ms.to_le_bytes());
    let preview_hash = vault_hash(&[&bytes]);
    let preview = format!("import wrapped {} {} as {} from repository {} generation {} into repository {} at revision {}; bearer backup cannot be recalled by later source revocation; reason: {}; expires_unix_ms: {}", T::NAME, plan.origin.name, plan.name, vault_uuid_hex(&plan.origin.repo_uuid), plan.origin.generation, vault_uuid_hex(&plan.repo_uuid), plan.start_revision, reason, plan.expires_unix_ms);
    if !vault_native_authorize(&preview, &plan.repo_uuid) { return Err(JetVaultKeyWrapError::Vault(JetVaultError::AuthorityDenied)); }
    let (user, session) = vault_authority_identity()?;
    Ok(JetVaultWrite { operation_hash: plan.operation_hash, preview_hash, user, session, deadline: plan.deadline, marker: std::marker::PhantomData })
}

pub fn jet_vault_commit_import_wrapped_at<T: JetVaultKey>(
    root: &std::path::Path,
    write: JetVaultWrite<T>,
    mut plan: JetVaultWrappedImportPlan<T>,
) -> Result<JetVaultKeyRef<T>, JetVaultKeyWrapError> {
    let now = std::time::Instant::now();
    let (user, session) = vault_authority_identity()?;
    if now > plan.deadline || now.duration_since(plan.issued) > std::time::Duration::from_secs(300) || now > write.deadline || write.operation_hash != plan.operation_hash || write.preview_hash == [0; 32] || write.user != user || write.session != session {
        return Err(JetVaultKeyWrapError::Vault(JetVaultError::Conflict));
    }
    let lock = vault_acquire_lock(root, plan.repo_uuid, plan.start_revision, plan.start_hash)?;
    let (mut store, current_hash, provider_hash, start_kind) = vault_read_at(root)?;
    if current_hash != plan.start_hash || provider_hash != plan.provider_hash || start_kind != plan.start_kind || (start_kind == VaultStartKind::V2 && (store.repo_uuid != plan.repo_uuid || store.revision != plan.start_revision)) {
        return Err(JetVaultKeyWrapError::Vault(JetVaultError::Conflict));
    }
    if start_kind != VaultStartKind::V2 { store.repo_uuid = plan.repo_uuid; }
    if let Some(existing) = plan.existing.take() {
        let record = vault_find(&store, &existing)?;
        if record.status == JetVaultKeyStatus::Revoked { return Err(JetVaultKeyWrapError::Vault(JetVaultError::Revoked)); }
        if record.public_key_hash != plan.public_key_hash || vault_key_digest(T::TAG, &record.key) != plan.key_digest { return Err(JetVaultKeyWrapError::OpenFailed); }
        drop(lock);
        return Ok(existing);
    }
    if store.keys.iter().any(|record| record.name == plan.name && record.key_type != T::TAG) { return Err(JetVaultKeyWrapError::Vault(JetVaultError::WrongType)); }
    let generation = store.keys.iter().filter(|record| record.name == plan.name && record.key_type == T::TAG).map(|record| record.generation).max().unwrap_or(0).checked_add(1).ok_or(JetVaultKeyWrapError::Internal { incident_id: "jvkw-generation" })?;
    if generation != plan.transition_to { return Err(JetVaultKeyWrapError::Vault(JetVaultError::Conflict)); }
    let unix_ms = vault_lifecycle_unix_ms(&store)?;
    for record in &mut store.keys {
        if record.name == plan.name && record.key_type == T::TAG && record.status == JetVaultKeyStatus::Active {
            record.status = JetVaultKeyStatus::Retired; record.status_unix_ms = unix_ms; record.reason_hash = vault_reason_hash(JetVaultKeyStatus::Retired, "wrapped import")?;
        }
    }
    let key = T::from_bytes(std::mem::take(&mut plan.key.0)).map_err(|_| JetVaultKeyWrapError::OpenFailed)?;
    let key_bytes = key.into_bytes();
    if vault_key_digest(T::TAG, &key_bytes) != plan.key_digest { return Err(JetVaultKeyWrapError::OpenFailed); }
    let mut record = JetVaultRecord { name: plan.name.clone(), key_type: T::TAG, generation, status: JetVaultKeyStatus::Active, provenance: JetVaultProvenance::Imported, opaque_id: plan.destination_opaque_id, created_unix_ms: unix_ms, status_unix_ms: unix_ms, record_hash: [0; 32], public_key_hash: plan.public_key_hash, reason_hash: [0; 32], origin: Some(plan.origin.clone()), key: key_bytes };
    record.record_hash = vault_record_hash(&record);
    let reference = vault_ref(&store, &record);
    store.keys.push(record);
    store.revision = store.revision.checked_add(1).ok_or(JetVaultKeyWrapError::Internal { incident_id: "jvkw-revision" })?;
    vault_write_at(root, &store, plan.start_hash, plan.provider_hash, plan.start_kind, plan.start_revision, lock)?;
    Ok(reference)
}

pub fn jet_vault_export_to_recipients_impl<T: JetVaultKey>(reference: &JetVaultKeyRef<T>, recipients: &Vec<JetX25519PublicKey>) -> Result<JetWrappedVaultKey, JetVaultKeyWrapError> { jet_vault_export_to_recipients_at(&vault_cwd(), reference, recipients) }
pub fn jet_vault_export_to_passphrase_impl<T: JetVaultKey>(reference: &JetVaultKeyRef<T>, passphrase: &Secret) -> Result<JetWrappedVaultKey, JetVaultKeyWrapError> { jet_vault_export_to_passphrase_at(&vault_cwd(), reference, passphrase) }
pub fn jet_vault_prepare_import_wrapped_impl<T: JetVaultKey>(name: &String, wrapped: JetWrappedVaultKey, unlock: JetVaultKeyUnlock<'_>) -> Result<JetVaultWrappedImportPlan<T>, JetVaultKeyWrapError> { jet_vault_prepare_import_wrapped_at(&vault_cwd(), name, wrapped, unlock) }
pub fn jet_vault_commit_import_wrapped_impl<T: JetVaultKey>(write: JetVaultWrite<T>, plan: JetVaultWrappedImportPlan<T>) -> Result<JetVaultKeyRef<T>, JetVaultKeyWrapError> { jet_vault_commit_import_wrapped_at(&vault_cwd(), write, plan) }
