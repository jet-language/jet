// core.crypto envelope runtime (D-CRYPTOENV1=A, D-DEP-CRYPTO1=A).
//
// Emitted into the hidden FFI bridge crate when a Jet program uses
// `core.crypto.seal` / `open` / `sign` / `verify`. The compiler crate
// stays zero-dependency (I6); RustCrypto lives only here.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce as AesNonce,
};
use chacha20poly1305::{ChaCha20Poly1305, Nonce as ChaNonce};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use chacha20poly1305::aead::Payload;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use sha2::{Digest, Sha256, Sha512};
use subtle::ConstantTimeEq;

const JETV_MAGIC: &[u8; 4] = b"JETV";
const JETW_MAGIC: &[u8; 4] = b"JETW";
const JET_TYPED_VERSION: u8 = 1;
const JET_TYPED_SUITE: u8 = 1;

fn zeroize(bytes: &mut [u8]) {
    for byte in &mut *bytes { unsafe { std::ptr::write_volatile(byte, 0) } }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    #[cfg(test)]
    CRYPTO_ZEROIZE_TEST_OBSERVER.with(|observer| {
        if let Some(callback) = observer.borrow_mut().as_mut() { callback(bytes); }
    });
}

struct Zeroizing<T: AsMut<[u8]>>(T);
impl<T: AsMut<[u8]>> std::ops::Deref for Zeroizing<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.0 }
}
impl<T: AsMut<[u8]>> std::ops::DerefMut for Zeroizing<T> {
    fn deref_mut(&mut self) -> &mut T { &mut self.0 }
}
impl<T: AsMut<[u8]>> Drop for Zeroizing<T> {
    fn drop(&mut self) { zeroize(self.0.as_mut()); }
}
impl<T: AsMut<[u8]> + AsRef<[u8]>> Zeroizing<T> {
    fn bytes(&self) -> &[u8] { self.0.as_ref() }
    fn bytes_mut(&mut self) -> &mut [u8] { self.0.as_mut() }
}

#[cfg(test)]
thread_local! {
    static CRYPTO_ZEROIZE_TEST_OBSERVER: std::cell::RefCell<Option<Box<dyn FnMut(&[u8])>>> =
        std::cell::RefCell::new(None);
}
#[cfg(test)]
pub fn jet_crypto_set_zeroize_test_observer(callback: impl FnMut(&[u8]) + 'static) {
    CRYPTO_ZEROIZE_TEST_OBSERVER.with(|observer| *observer.borrow_mut() = Some(Box::new(callback)));
}
#[cfg(test)]
pub fn jet_crypto_clear_zeroize_test_observer() {
    CRYPTO_ZEROIZE_TEST_OBSERVER.with(|observer| *observer.borrow_mut() = None);
}

macro_rules! secret_type {
    ($name:ident) => {
        pub struct $name(Vec<u8>);
        impl Drop for $name { fn drop(&mut self) { zeroize(&mut self.0); } }
    };
}
secret_type!(Secret);
secret_type!(JetSigningKey);
secret_type!(JetX25519SecretKey);
secret_type!(JetSharedSecret);

#[derive(Clone, PartialEq, Eq)] pub struct JetVerifyKey([u8; 32]);
#[derive(Clone, PartialEq, Eq)] pub struct JetX25519PublicKey([u8; 32]);
#[derive(Clone, PartialEq, Eq)] pub struct JetSignature([u8; 64]);
#[derive(Clone, PartialEq, Eq)] pub struct JetSealed(Vec<u8>);
#[derive(Clone, PartialEq, Eq)] pub struct JetWrappedKey(Vec<u8>);
#[derive(Clone, PartialEq, Eq)] pub struct JetPasswordHash(String);
#[derive(Clone, PartialEq, Eq)] pub struct JetDigest256([u8; 32]);
#[derive(Clone, PartialEq, Eq)] pub struct JetDigest512([u8; 64]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JetFileCryptoError {
    OpenFailed,
    SourceIo,
    DestinationIo,
    DestinationExists,
    SealFailed(JetCryptoError),
    Cancelled,
}

fn invalid_length(operation: &'static str, parameter: &'static str, expected: &'static str, actual: usize) -> JetCryptoError {
    JetCryptoError::InvalidLength { operation, parameter, expected, actual }
}
fn array32(bytes: &[u8], operation: &'static str, parameter: &'static str) -> Result<[u8; 32], JetCryptoError> {
    bytes.try_into().map_err(|_| invalid_length(operation, parameter, "exactly 32", bytes.len()))
}
fn checked_total(base: usize, count: usize, each: usize, tail: usize) -> Option<usize> {
    count.checked_mul(each)?.checked_add(base)?.checked_add(tail)
}

pub fn jet_crypto_secret_from_text_impl(mut text: String) -> Secret {
    let bytes = std::mem::take(&mut text).into_bytes();
    Secret(bytes)
}
pub fn jet_crypto_secret_from_bytes_impl(mut bytes: Vec<u8>) -> Secret {
    Secret(std::mem::take(&mut bytes))
}
/// D-EMAIL-SMTP-CONFIG1=A: sole SMTP extraction boundary. Returned bytes are
/// owned by Mailer and zeroized on every construction failure and Drop path.
pub fn jet_crypto_secret_copy_for_smtp_impl(secret: &Secret) -> Vec<u8> {
    secret.0.clone()
}
pub fn jet_crypto_zeroize_email_impl(bytes: &mut Vec<u8>) {
    zeroize(bytes);
    bytes.clear();
}
pub fn jet_crypto_email_sha256_impl(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
pub fn jet_crypto_email_ed25519_sign_impl(key: &Vec<u8>, message: &[u8]) -> Result<Vec<u8>, String> {
    let mut seed: [u8; 32] = key.as_slice().try_into()
        .map_err(|_| "DKIM private key must contain exactly 32 bytes".to_string())?;
    let signature = SigningKey::from_bytes(&seed).sign(message).to_bytes().to_vec();
    zeroize(&mut seed);
    Ok(signature)
}
pub fn jet_crypto_x25519_generate_impl() -> Result<JetX25519SecretKey, JetCryptoError> {
    let mut bytes = vec![0; 32];
    jet_crypto_entropy_fill(&mut bytes).map_err(|_| JetCryptoError::EntropyUnavailable)?;
    Ok(JetX25519SecretKey(bytes))
}
pub fn jet_crypto_x25519_public_typed_impl(secret: &JetX25519SecretKey) -> JetX25519PublicKey {
    let mut raw = [0u8; 32]; raw.copy_from_slice(&secret.0);
    JetX25519PublicKey(x25519_dalek::x25519(raw, x25519_dalek::X25519_BASEPOINT_BYTES))
}
pub fn jet_crypto_x25519_public_from_bytes_impl(bytes: Vec<u8>) -> Result<JetX25519PublicKey, JetCryptoError> {
    Ok(JetX25519PublicKey(array32(&bytes, "X25519PublicKey.from_bytes", "bytes")?))
}
pub fn jet_crypto_x25519_public_bytes_impl(key: &JetX25519PublicKey) -> Vec<u8> { key.0.to_vec() }
const BECH32_CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
fn bech32_polymod(values: impl IntoIterator<Item = u8>) -> u32 {
    let mut chk = 1u32;
    for value in values {
        let top = chk >> 25; chk = (chk & 0x1ff_ffff) << 5 ^ value as u32;
        for (i, generator) in [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3].iter().enumerate() { if (top >> i) & 1 != 0 { chk ^= generator; } }
    }
    chk
}
fn bech32_hrp_expand(hrp: &str) -> Vec<u8> { hrp.bytes().map(|b| b >> 5).chain(std::iter::once(0)).chain(hrp.bytes().map(|b| b & 31)).collect() }
fn convert_bits(data: &[u8], from: u32, to: u32, pad: bool) -> Option<Vec<u8>> {
    let mut acc=0u32;let mut bits=0u32;let maxv=(1u32<<to)-1;let mut out=Vec::new();for value in data{if(*value as u32)>>from!=0{return None}acc=(acc<<from)|*value as u32;bits+=from;while bits>=to{bits-=to;out.push(((acc>>bits)&maxv)as u8)}}if pad{if bits!=0{out.push(((acc<<(to-bits))&maxv)as u8)}}else if bits>=from||((acc<<(to-bits))&maxv)!=0{return None}Some(out)
}
pub fn jet_crypto_x25519_public_text_impl(key:&JetX25519PublicKey)->String{let hrp="jetx25519";let data=convert_bits(&key.0,8,5,true).expect("fixed conversion");let mut input=bech32_hrp_expand(hrp);input.extend_from_slice(&data);input.extend_from_slice(&[0;6]);let polymod=bech32_polymod(input)^0x2bc830a3;let mut out=format!("{hrp}1");for value in data{out.push(BECH32_CHARSET[value as usize]as char)}for shift in(0..6).rev(){out.push(BECH32_CHARSET[((polymod>>(shift*5))&31)as usize]as char)}out}
pub fn jet_crypto_x25519_public_from_text_impl(text:String)->Result<JetX25519PublicKey,JetCryptoError>{
    // HRP (9) + separator + 32-byte 5-bit payload (52) + checksum (6).
    // Exact size rejects extension/truncation and keeps parsing allocation-bounded.
    if text.len()!=68||!text.is_ascii()||text.bytes().any(|b|b.is_ascii_uppercase())||text.as_bytes().get(9)!=Some(&b'1')||!text.starts_with("jetx255191"){
        return Err(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m public key"})
    }
    let encoded=&text.as_bytes()[10..];
    let mut values=Vec::with_capacity(58);
    for byte in encoded{values.push(BECH32_CHARSET.iter().position(|c|c==byte).ok_or(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m public key"})?as u8)}
    let mut check=bech32_hrp_expand("jetx25519");check.extend_from_slice(&values);
    if bech32_polymod(check)!=0x2bc830a3{return Err(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m checksum"})}
    let decoded=convert_bits(&values[..52],5,8,false).ok_or(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m public key"})?;
    Ok(JetX25519PublicKey(array32(&decoded,"X25519PublicKey.from_text","decoded key")?))
}
pub fn jet_crypto_signing_generate_impl() -> Result<JetSigningKey, JetCryptoError> {
    let mut bytes = vec![0; 32];
    jet_crypto_entropy_fill(&mut bytes).map_err(|_| JetCryptoError::EntropyUnavailable)?;
    Ok(JetSigningKey(bytes))
}
pub fn jet_crypto_signing_public_impl(key: &JetSigningKey) -> JetVerifyKey {
    let mut seed = [0; 32]; seed.copy_from_slice(&key.0);
    JetVerifyKey(SigningKey::from_bytes(&seed).verifying_key().to_bytes())
}
pub fn jet_crypto_verify_key_from_bytes_impl(bytes: Vec<u8>) -> Result<JetVerifyKey, JetCryptoError> {
    let raw = array32(&bytes, "VerifyKey.from_bytes", "bytes")?;
    VerifyingKey::from_bytes(&raw).map_err(|_| JetCryptoError::InvalidEncoding { operation: "VerifyKey.from_bytes", value_kind: "Ed25519 public key" })?;
    Ok(JetVerifyKey(raw))
}
pub fn jet_crypto_verify_key_bytes_impl(key: &JetVerifyKey) -> Vec<u8> { key.0.to_vec() }
pub fn jet_crypto_signature_from_bytes_impl(bytes: Vec<u8>) -> Result<JetSignature, JetCryptoError> {
    let raw: [u8; 64] = bytes.as_slice().try_into().map_err(|_| invalid_length("Signature.from_bytes", "bytes", "exactly 64", bytes.len()))?;
    Ok(JetSignature(raw))
}
pub fn jet_crypto_signature_bytes_impl(signature: &JetSignature) -> Vec<u8> { signature.0.to_vec() }
pub fn jet_crypto_sign_typed_impl(key: &JetSigningKey, message: &Vec<u8>) -> Result<JetSignature, JetCryptoError> {
    if message.len() > 1_073_741_824 { return Err(invalid_length("sign", "message", "at most 1073741824", message.len())); }
    let mut seed = [0; 32]; seed.copy_from_slice(&key.0);
    let signature = SigningKey::from_bytes(&seed).sign(message).to_bytes();
    zeroize(&mut seed);
    Ok(JetSignature(signature))
}
pub fn jet_crypto_verify_typed_impl(key: JetVerifyKey, message: &Vec<u8>, signature: JetSignature) -> Result<bool, JetCryptoError> {
    if message.len() > 1_073_741_824 { return Err(invalid_length("verify", "message", "at most 1073741824", message.len())); }
    let key = VerifyingKey::from_bytes(&key.0).map_err(|_| JetCryptoError::InvalidEncoding { operation: "verify", value_kind: "Ed25519 public key" })?;
    Ok(key.verify_strict(message, &ed25519_dalek::Signature::from_bytes(&signature.0)).is_ok())
}
pub fn jet_crypto_x25519_typed_impl(secret: &JetX25519SecretKey, public: JetX25519PublicKey) -> Result<JetSharedSecret, JetCryptoError> {
    let mut raw = [0; 32]; raw.copy_from_slice(&secret.0);
    let shared = x25519_dalek::x25519(raw, public.0);
    zeroize(&mut raw);
    if bool::from(shared.ct_eq(&[0; 32])) { return Err(JetCryptoError::NonContributoryKey); }
    Ok(JetSharedSecret(shared.to_vec()))
}

fn hkdf32(shared: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; 32], JetCryptoError> {
    let hk = Hkdf::<sha2::Sha256>::new(Some(salt), shared);
    let mut out = [0; 32];
    hk.expand(info, &mut out).map_err(|_| JetCryptoError::Internal { incident_id: "hkdf-expand" })?;
    Ok(out)
}
fn hkdf24(shared: &[u8], salt: &[u8], info: &[u8]) -> Result<[u8; 24], JetCryptoError> {
    let hk = Hkdf::<sha2::Sha256>::new(Some(salt), shared);
    let mut out = [0; 24];
    hk.expand(info, &mut out).map_err(|_| JetCryptoError::Internal { incident_id: "hkdf-expand" })?;
    Ok(out)
}
fn x25519_checked(mut secret: [u8; 32], public: [u8; 32]) -> Result<[u8; 32], JetCryptoError> {
    let shared = x25519_dalek::x25519(secret, public);
    zeroize(&mut secret);
    if bool::from(shared.ct_eq(&[0; 32])) { Err(JetCryptoError::NonContributoryKey) } else { Ok(shared) }
}
fn typed_header(magic: &[u8; 4], recipient_count: u16, ephemeral: &[u8; 32], salt: &[u8; 16], nonce: Option<&[u8; 24]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(if nonce.is_some() { 82 } else { 58 });
    out.extend_from_slice(magic); out.push(JET_TYPED_VERSION); out.push(JET_TYPED_SUITE);
    out.extend_from_slice(&0u16.to_be_bytes()); out.extend_from_slice(&recipient_count.to_be_bytes());
    out.extend_from_slice(ephemeral); out.extend_from_slice(salt);
    if let Some(nonce) = nonce { out.extend_from_slice(nonce); }
    out
}

pub fn jet_crypto_seal_typed_impl(mut recipients: Vec<JetX25519PublicKey>, plaintext: &Vec<u8>, aad: &Vec<u8>) -> Result<JetSealed, JetCryptoError> {
    if recipients.is_empty() || recipients.len() > 256 { return Err(invalid_length("seal", "recipients", "1..=256", recipients.len())); }
    if plaintext.len() > 16_777_216 { return Err(invalid_length("seal", "plaintext", "at most 16777216", plaintext.len())); }
    if aad.len() > 16_777_216 { return Err(invalid_length("seal", "aad", "at most 16777216", aad.len())); }
    recipients.sort_by_key(|k| k.0);
    if recipients.windows(2).any(|pair| pair[0].0 == pair[1].0) { return Err(JetCryptoError::InvalidEncoding { operation: "seal", value_kind: "duplicate recipient" }); }
    let mut ephemeral_secret = [0; 32]; let mut salt = [0; 16]; let mut payload_nonce = [0; 24]; let mut file_key = [0; 32];
    let fill = (|| { jet_crypto_entropy_fill(&mut ephemeral_secret)?; jet_crypto_entropy_fill(&mut salt)?; jet_crypto_entropy_fill(&mut payload_nonce)?; jet_crypto_entropy_fill(&mut file_key) })();
    if fill.is_err() { zeroize(&mut ephemeral_secret); zeroize(&mut salt); zeroize(&mut payload_nonce); zeroize(&mut file_key); return Err(JetCryptoError::EntropyUnavailable); }
    let ephemeral_public = x25519_dalek::x25519(ephemeral_secret, x25519_dalek::X25519_BASEPOINT_BYTES);
    let header = typed_header(JETV_MAGIC, recipients.len() as u16, &ephemeral_public, &salt, Some(&payload_nonce));
    let mut stanzas = Vec::with_capacity(recipients.len() * 80);
    for recipient in &recipients {
        let mut shared = x25519_checked(ephemeral_secret, recipient.0)?;
        let mut key_info = b"JETV1 wrap".to_vec(); key_info.extend_from_slice(&ephemeral_public); key_info.extend_from_slice(&recipient.0);
        let mut nonce_info = b"JETV1 nonce".to_vec(); nonce_info.extend_from_slice(&ephemeral_public); nonce_info.extend_from_slice(&recipient.0);
        let mut kek = hkdf32(&shared, &salt, &key_info)?; let nonce = hkdf24(&shared, &salt, &nonce_info)?;
        let mut stanza_aad = b"JETV1 stanza".to_vec(); stanza_aad.extend_from_slice(&header); stanza_aad.extend_from_slice(&recipient.0);
        let wrapped = XChaCha20Poly1305::new_from_slice(&kek).map_err(|_| JetCryptoError::Internal { incident_id: "jetv-key" })?
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: &file_key, aad: &stanza_aad }).map_err(|_| JetCryptoError::Internal { incident_id: "jetv-wrap" })?;
        stanzas.extend_from_slice(&recipient.0); stanzas.extend_from_slice(&wrapped);
        zeroize(&mut shared); zeroize(&mut kek);
    }
    let mut payload_aad = b"JETV1 payload".to_vec(); payload_aad.extend_from_slice(&header); payload_aad.extend_from_slice(&stanzas); payload_aad.extend_from_slice(aad);
    let ciphertext = XChaCha20Poly1305::new_from_slice(&file_key).map_err(|_| JetCryptoError::Internal { incident_id: "jetv-key" })?
        .encrypt(XNonce::from_slice(&payload_nonce), Payload { msg: plaintext, aad: &payload_aad }).map_err(|_| JetCryptoError::Internal { incident_id: "jetv-payload" })?;
    let mut out = header; out.extend_from_slice(&stanzas); out.extend_from_slice(&(plaintext.len() as u32).to_be_bytes()); out.extend_from_slice(&ciphertext);
    zeroize(&mut ephemeral_secret); zeroize(&mut file_key);
    Ok(JetSealed(out))
}

pub fn jet_crypto_sealed_from_bytes_impl(bytes: Vec<u8>) -> Result<JetSealed, JetCryptoError> {
    if bytes.len() < 182 || bytes.len() > 16_797_798 { return Err(invalid_length("Sealed.from_bytes", "bytes", "182..=16797798", bytes.len())); }
    if &bytes[..4] != JETV_MAGIC { return Err(JetCryptoError::InvalidEncoding { operation: "Sealed.from_bytes", value_kind: "JETV magic" }); }
    if bytes[4] != 1 { return Err(JetCryptoError::UnsupportedVersion { operation: "Sealed.from_bytes", version: bytes[4] }); }
    if bytes[5] != 1 { return Err(JetCryptoError::UnsupportedAlgorithm { operation: "Sealed.from_bytes", algorithm: bytes[5] }); }
    if bytes[6] != 0 || bytes[7] != 0 { return Err(JetCryptoError::InvalidEncoding { operation: "Sealed.from_bytes", value_kind: "flags" }); }
    let count = u16::from_be_bytes([bytes[8], bytes[9]]) as usize;
    if count == 0 || count > 256 { return Err(invalid_length("Sealed.from_bytes", "recipient_count", "1..=256", count)); }
    let len_offset = checked_total(82, count, 80, 0).ok_or(JetCryptoError::InvalidEncoding { operation: "Sealed.from_bytes", value_kind: "length" })?;
    if bytes.len() < len_offset + 20 { return Err(JetCryptoError::InvalidEncoding { operation: "Sealed.from_bytes", value_kind: "truncated envelope" }); }
    let payload_len = u32::from_be_bytes(bytes[len_offset..len_offset+4].try_into().unwrap()) as usize;
    if payload_len > 16_777_216 { return Err(invalid_length("Sealed.from_bytes", "payload_len", "at most 16777216", payload_len)); }
    let expected = len_offset.checked_add(4).and_then(|n| n.checked_add(payload_len)).and_then(|n| n.checked_add(16)).ok_or(JetCryptoError::InvalidEncoding { operation: "Sealed.from_bytes", value_kind: "length" })?;
    if bytes.len() != expected { return Err(JetCryptoError::InvalidEncoding { operation: "Sealed.from_bytes", value_kind: "trailing or truncated bytes" }); }
    let mut previous: Option<&[u8]> = None;
    for i in 0..count { let start = 82 + i * 80; let current = &bytes[start..start+32]; if previous.is_some_and(|p| p >= current) { return Err(JetCryptoError::InvalidEncoding { operation: "Sealed.from_bytes", value_kind: "recipient order" }); } previous = Some(current); }
    Ok(JetSealed(bytes))
}
pub fn jet_crypto_sealed_bytes_impl(sealed: &JetSealed) -> Vec<u8> { sealed.0.clone() }

pub fn jet_crypto_open_typed_impl(recipient: &JetX25519SecretKey, envelope: JetSealed, aad: &Vec<u8>) -> Result<Vec<u8>, JetCryptoError> {
    let bytes = &envelope.0; let count = u16::from_be_bytes([bytes[8], bytes[9]]) as usize;
    let mut recipient_secret = [0; 32]; recipient_secret.copy_from_slice(&recipient.0);
    let own_public = x25519_dalek::x25519(recipient_secret, x25519_dalek::X25519_BASEPOINT_BYTES);
    let ephemeral: [u8; 32] = bytes[10..42].try_into().unwrap(); let salt = &bytes[42..58]; let nonce = &bytes[58..82];
    let Some(index) = (0..count).find(|i| bytes[82+i*80..114+i*80] == own_public) else { zeroize(&mut recipient_secret); return Err(JetCryptoError::OpenFailed); };
    let mut shared = x25519_checked(recipient_secret, ephemeral).map_err(|_| JetCryptoError::OpenFailed)?;
    let mut key_info = b"JETV1 wrap".to_vec(); key_info.extend_from_slice(&ephemeral); key_info.extend_from_slice(&own_public);
    let mut nonce_info = b"JETV1 nonce".to_vec(); nonce_info.extend_from_slice(&ephemeral); nonce_info.extend_from_slice(&own_public);
    let mut kek = hkdf32(&shared, salt, &key_info).map_err(|_| JetCryptoError::OpenFailed)?; let wrap_nonce = hkdf24(&shared, salt, &nonce_info).map_err(|_| JetCryptoError::OpenFailed)?;
    let header = &bytes[..82]; let mut stanza_aad = b"JETV1 stanza".to_vec(); stanza_aad.extend_from_slice(header); stanza_aad.extend_from_slice(&own_public);
    let stanza_start = 82 + index * 80;
    let mut file_key = XChaCha20Poly1305::new_from_slice(&kek).map_err(|_| JetCryptoError::OpenFailed)?
        .decrypt(XNonce::from_slice(&wrap_nonce), Payload { msg: &bytes[stanza_start+32..stanza_start+80], aad: &stanza_aad }).map_err(|_| JetCryptoError::OpenFailed)?;
    let len_offset = 82 + count * 80; let payload_len = u32::from_be_bytes(bytes[len_offset..len_offset+4].try_into().unwrap()) as usize;
    let mut payload_aad = b"JETV1 payload".to_vec(); payload_aad.extend_from_slice(header); payload_aad.extend_from_slice(&bytes[82..len_offset]); payload_aad.extend_from_slice(aad);
    let result = XChaCha20Poly1305::new_from_slice(&file_key).map_err(|_| JetCryptoError::OpenFailed)?
        .decrypt(XNonce::from_slice(nonce), Payload { msg: &bytes[len_offset+4..len_offset+4+payload_len+16], aad: &payload_aad }).map_err(|_| JetCryptoError::OpenFailed);
    zeroize(&mut shared); zeroize(&mut kek); zeroize(&mut file_key); result
}

pub fn jet_crypto_wrap_typed_impl(secret: &Secret, recipient: JetX25519PublicKey) -> Result<JetWrappedKey, JetCryptoError> {
    if secret.0.len() > 1_048_576 { return Err(invalid_length("wrap", "secret", "at most 1048576", secret.0.len())); }
    let mut ephemeral_secret = [0; 32]; let mut salt = [0; 16]; jet_crypto_entropy_fill(&mut ephemeral_secret).map_err(|_| JetCryptoError::EntropyUnavailable)?; jet_crypto_entropy_fill(&mut salt).map_err(|_| JetCryptoError::EntropyUnavailable)?;
    let ephemeral = x25519_dalek::x25519(ephemeral_secret, x25519_dalek::X25519_BASEPOINT_BYTES); let mut shared = x25519_checked(ephemeral_secret, recipient.0)?;
    let mut key_info = b"JETW1 key".to_vec(); key_info.extend_from_slice(&ephemeral); key_info.extend_from_slice(&recipient.0); let mut nonce_info = b"JETW1 nonce".to_vec(); nonce_info.extend_from_slice(&ephemeral); nonce_info.extend_from_slice(&recipient.0);
    let mut kek = hkdf32(&shared, &salt, &key_info)?; let nonce = hkdf24(&shared, &salt, &nonce_info)?;
    let mut header = typed_header(JETW_MAGIC, 0, &ephemeral, &salt, None); header.truncate(8); header.extend_from_slice(&recipient.0); header.extend_from_slice(&ephemeral); header.extend_from_slice(&salt); header.extend_from_slice(&(secret.0.len() as u32).to_be_bytes());
    let mut wrap_aad = b"JETW1 wrap".to_vec(); wrap_aad.extend_from_slice(&header);
    let ciphertext = XChaCha20Poly1305::new_from_slice(&kek).map_err(|_| JetCryptoError::Internal { incident_id: "jetw-key" })?.encrypt(XNonce::from_slice(&nonce), Payload { msg: &secret.0, aad: &wrap_aad }).map_err(|_| JetCryptoError::Internal { incident_id: "jetw-wrap" })?;
    let mut out = header; out.extend_from_slice(&ciphertext); zeroize(&mut shared); zeroize(&mut kek); Ok(JetWrappedKey(out))
}
pub fn jet_crypto_wrapped_from_bytes_impl(bytes: Vec<u8>) -> Result<JetWrappedKey, JetCryptoError> {
    if bytes.len() < 108 || bytes.len() > 1_048_684 { return Err(invalid_length("WrappedKey.from_bytes", "bytes", "108..=1048684", bytes.len())); }
    if &bytes[..4] != JETW_MAGIC { return Err(JetCryptoError::InvalidEncoding { operation: "WrappedKey.from_bytes", value_kind: "JETW magic" }); }
    if bytes[4] != 1 { return Err(JetCryptoError::UnsupportedVersion { operation: "WrappedKey.from_bytes", version: bytes[4] }); } if bytes[5] != 1 { return Err(JetCryptoError::UnsupportedAlgorithm { operation: "WrappedKey.from_bytes", algorithm: bytes[5] }); } if bytes[6] != 0 || bytes[7] != 0 { return Err(JetCryptoError::InvalidEncoding { operation: "WrappedKey.from_bytes", value_kind: "flags" }); }
    let len = u32::from_be_bytes(bytes[88..92].try_into().unwrap()) as usize; if len > 1_048_576 || bytes.len() != 92 + len + 16 { return Err(JetCryptoError::InvalidEncoding { operation: "WrappedKey.from_bytes", value_kind: "length" }); } Ok(JetWrappedKey(bytes))
}
pub fn jet_crypto_wrapped_bytes_impl(wrapped: &JetWrappedKey) -> Vec<u8> { wrapped.0.clone() }
pub fn jet_crypto_unwrap_typed_impl(recipient: &JetX25519SecretKey, wrapped: JetWrappedKey) -> Result<Secret, JetCryptoError> {
    let b=&wrapped.0; let recipient_public:[u8;32]=b[8..40].try_into().unwrap(); let mut secret=[0;32]; secret.copy_from_slice(&recipient.0); let own=x25519_dalek::x25519(secret,x25519_dalek::X25519_BASEPOINT_BYTES); if !bool::from(own.ct_eq(&recipient_public)){zeroize(&mut secret);return Err(JetCryptoError::OpenFailed)} let ephemeral:[u8;32]=b[40..72].try_into().unwrap(); let salt=&b[72..88]; let mut shared=x25519_checked(secret,ephemeral).map_err(|_|JetCryptoError::OpenFailed)?;
    let mut ki=b"JETW1 key".to_vec();ki.extend_from_slice(&ephemeral);ki.extend_from_slice(&recipient_public);let mut ni=b"JETW1 nonce".to_vec();ni.extend_from_slice(&ephemeral);ni.extend_from_slice(&recipient_public);let mut kek=hkdf32(&shared,salt,&ki).map_err(|_|JetCryptoError::OpenFailed)?;let nonce=hkdf24(&shared,salt,&ni).map_err(|_|JetCryptoError::OpenFailed)?;let mut aad=b"JETW1 wrap".to_vec();aad.extend_from_slice(&b[..92]);let plain=XChaCha20Poly1305::new_from_slice(&kek).map_err(|_|JetCryptoError::OpenFailed)?.decrypt(XNonce::from_slice(&nonce),Payload{msg:&b[92..],aad:&aad}).map_err(|_|JetCryptoError::OpenFailed)?;zeroize(&mut shared);zeroize(&mut kek);Ok(Secret(plain))
}

pub fn jet_crypto_sha256_typed_impl(data:&Vec<u8>)->JetDigest256 { use sha2::Sha256; JetDigest256(Sha256::digest(data).into()) }
pub fn jet_crypto_blake3_typed_impl(data:&Vec<u8>)->JetDigest256 { JetDigest256(*blake3::hash(data).as_bytes()) }
pub fn jet_crypto_sha512_typed_impl(data:&Vec<u8>)->JetDigest512 { JetDigest512(Sha512::digest(data).into()) }
pub fn jet_crypto_digest256_bytes_impl(d:&JetDigest256)->Vec<u8>{d.0.to_vec()} pub fn jet_crypto_digest512_bytes_impl(d:&JetDigest512)->Vec<u8>{d.0.to_vec()}
pub fn jet_crypto_digest256_hex_impl(d:&JetDigest256)->String{hex_encode(&d.0)} pub fn jet_crypto_digest512_hex_impl(d:&JetDigest512)->String{hex_encode(&d.0)}
pub fn jet_crypto_hkdf_typed_impl(ikm:&Secret,salt:&Vec<u8>,info:&Vec<u8>,length:i64)->Result<Secret,JetCryptoError>{if !(0..=8160).contains(&length){return Err(JetCryptoError::OutputLength{operation:"hkdf_sha256",minimum:0,maximum:8160,actual:length.unsigned_abs() as usize})}let mut out=vec![0;length as usize];Hkdf::<sha2::Sha256>::new(Some(salt),&ikm.0).expand(info,&mut out).map_err(|_|JetCryptoError::Internal{incident_id:"hkdf-expand"})?;Ok(Secret(out))}
pub fn jet_crypto_constant_time_secret_impl(a:&Secret,b:&Secret)->bool{let max=a.0.len().max(b.0.len());let mut diff=a.0.len()^b.0.len();for i in 0..max{diff|=(a.0.get(i).copied().unwrap_or(0)^b.0.get(i).copied().unwrap_or(0))as usize;}diff==0}
pub fn jet_crypto_password_hash_typed_impl(password:&Secret)->Result<JetPasswordHash,JetCryptoError>{let mut salt=[0;16];jet_crypto_entropy_fill(&mut salt).map_err(|_|JetCryptoError::EntropyUnavailable)?;let encoded=argon2::password_hash::SaltString::encode_b64(&salt).map_err(|_|JetCryptoError::Internal{incident_id:"password-salt"})?;let hash=argon2::PasswordHasher::hash_password(&argon2::Argon2::default(),&password.0,&encoded).map_err(|_|JetCryptoError::ResourceUnavailable{resource:"password hashing"})?.to_string();zeroize(&mut salt);Ok(JetPasswordHash(hash))}
pub fn jet_crypto_password_parse_impl(text:String)->Result<JetPasswordHash,JetCryptoError>{argon2::PasswordHash::new(&text).map_err(|_|JetCryptoError::InvalidEncoding{operation:"PasswordHash.parse",value_kind:"PHC string"})?;Ok(JetPasswordHash(text))}
pub fn jet_crypto_password_text_impl(hash:&JetPasswordHash)->String{hash.0.clone()}
pub fn jet_crypto_password_verify_typed_impl(password:&Secret,stored:&JetPasswordHash)->Result<bool,JetCryptoError>{let parsed=argon2::PasswordHash::new(&stored.0).map_err(|_|JetCryptoError::InvalidEncoding{operation:"password_verify",value_kind:"PHC string"})?;Ok(argon2::PasswordVerifier::verify_password(&argon2::Argon2::default(),&password.0,&parsed).is_ok())}

fn expert_aead_lengths(
    operation: &'static str,
    key: &[u8],
    nonce: &[u8],
    nonce_length: usize,
    input: &[u8],
    aad: &[u8],
    opening: bool,
) -> Result<(), JetCryptoError> {
    if key.len() != 32 { return Err(invalid_length(operation, "key", "exactly 32", key.len())); }
    if nonce.len() != nonce_length { return Err(invalid_length(operation, "nonce", if nonce_length == 24 { "exactly 24" } else { "exactly 12" }, nonce.len())); }
    let maximum = if opening { 1_073_741_840 } else { 1_073_741_824 };
    let minimum = if opening { 16 } else { 0 };
    if input.len() < minimum || input.len() > maximum {
        return Err(invalid_length(operation, if opening { "ciphertext" } else { "plaintext" }, if opening { "16..=1073741840" } else { "at most 1073741824" }, input.len()));
    }
    if aad.len() > 16_777_216 { return Err(invalid_length(operation, "aad", "at most 16777216", aad.len())); }
    Ok(())
}

pub fn jet_crypto_expert_xchacha20poly1305_seal_impl(key:&Vec<u8>,nonce:&Vec<u8>,plaintext:&Vec<u8>,aad:&Vec<u8>)->Result<Vec<u8>,JetCryptoError>{
    expert_aead_lengths("expert.xchacha20poly1305_seal",key,nonce,24,plaintext,aad,false)?;
    XChaCha20Poly1305::new_from_slice(key).map_err(|_|JetCryptoError::Internal{incident_id:"expert-xchacha-key"})?.encrypt(XNonce::from_slice(nonce),Payload{msg:plaintext,aad}).map_err(|_|JetCryptoError::Internal{incident_id:"expert-xchacha-seal"})
}
pub fn jet_crypto_expert_xchacha20poly1305_open_impl(key:&Vec<u8>,nonce:&Vec<u8>,ciphertext:&Vec<u8>,aad:&Vec<u8>)->Result<Vec<u8>,JetCryptoError>{
    expert_aead_lengths("expert.xchacha20poly1305_open",key,nonce,24,ciphertext,aad,true)?;
    XChaCha20Poly1305::new_from_slice(key).map_err(|_|JetCryptoError::Internal{incident_id:"expert-xchacha-key"})?.decrypt(XNonce::from_slice(nonce),Payload{msg:ciphertext,aad}).map_err(|_|JetCryptoError::OpenFailed)
}
pub fn jet_crypto_expert_aes256gcm_seal_impl(key:&Vec<u8>,nonce:&Vec<u8>,plaintext:&Vec<u8>,aad:&Vec<u8>)->Result<Vec<u8>,JetCryptoError>{
    expert_aead_lengths("expert.aes256gcm_seal",key,nonce,12,plaintext,aad,false)?;
    Aes256Gcm::new_from_slice(key).map_err(|_|JetCryptoError::Internal{incident_id:"expert-aes-key"})?.encrypt(AesNonce::from_slice(nonce),Payload{msg:plaintext,aad}).map_err(|_|JetCryptoError::Internal{incident_id:"expert-aes-seal"})
}
pub fn jet_crypto_expert_aes256gcm_open_impl(key:&Vec<u8>,nonce:&Vec<u8>,ciphertext:&Vec<u8>,aad:&Vec<u8>)->Result<Vec<u8>,JetCryptoError>{
    expert_aead_lengths("expert.aes256gcm_open",key,nonce,12,ciphertext,aad,true)?;
    Aes256Gcm::new_from_slice(key).map_err(|_|JetCryptoError::Internal{incident_id:"expert-aes-key"})?.decrypt(AesNonce::from_slice(nonce),Payload{msg:ciphertext,aad}).map_err(|_|JetCryptoError::OpenFailed)
}
pub fn jet_crypto_expert_ed25519_sign_impl(seed:&Vec<u8>,message:&Vec<u8>)->Result<JetSignature,JetCryptoError>{
    if seed.len()!=32{return Err(invalid_length("expert.ed25519_sign","seed","exactly 32",seed.len()))}if message.len()>1_073_741_824{return Err(invalid_length("expert.ed25519_sign","message","at most 1073741824",message.len()))}let mut raw=[0;32];raw.copy_from_slice(seed);let signature=SigningKey::from_bytes(&raw).sign(message).to_bytes();zeroize(&mut raw);Ok(JetSignature(signature))
}
pub fn jet_crypto_expert_ed25519_verify_strict_impl(public:&Vec<u8>,message:&Vec<u8>,signature:&Vec<u8>)->Result<bool,JetCryptoError>{
    if signature.len()!=64{return Err(invalid_length("expert.ed25519_verify_strict","signature","exactly 64",signature.len()))}if message.len()>1_073_741_824{return Err(invalid_length("expert.ed25519_verify_strict","message","at most 1073741824",message.len()))}let public=array32(public,"expert.ed25519_verify_strict","public")?;let mut signature_bytes=[0;64];signature_bytes.copy_from_slice(signature);let key=VerifyingKey::from_bytes(&public).map_err(|_|JetCryptoError::InvalidEncoding{operation:"expert.ed25519_verify_strict",value_kind:"Ed25519 public key"})?;Ok(key.verify_strict(message,&ed25519_dalek::Signature::from_bytes(&signature_bytes)).is_ok())
}
pub fn jet_crypto_expert_x25519_impl(secret:&Vec<u8>,public:&Vec<u8>,reject_all_zero:bool)->Result<Secret,JetCryptoError>{
    let mut secret=array32(secret,"expert.x25519","secret")?;let public=array32(public,"expert.x25519","public")?;let shared=x25519_dalek::x25519(secret,public);zeroize(&mut secret);if reject_all_zero&&bool::from(shared.ct_eq(&[0;32])){return Err(JetCryptoError::NonContributoryKey)}Ok(Secret(shared.to_vec()))
}
pub fn jet_crypto_expert_hkdf_sha256_impl(ikm:&Vec<u8>,salt:&Vec<u8>,info:&Vec<u8>,length:i64)->Result<Secret,JetCryptoError>{
    if !(0..=8160).contains(&length){return Err(JetCryptoError::OutputLength{operation:"expert.hkdf_sha256",minimum:0,maximum:8160,actual:length.unsigned_abs() as usize})}let mut out=vec![0;length as usize];Hkdf::<sha2::Sha256>::new(Some(salt),ikm).expand(info,&mut out).map_err(|_|JetCryptoError::Internal{incident_id:"expert-hkdf-expand"})?;Ok(Secret(out))
}
pub fn jet_crypto_expert_argon2id_impl(password:&Secret,salt:&Vec<u8>,memory_kib:i64,iterations:i64,lanes:i64,output_length:i64)->Result<Secret,JetCryptoError>{
    if password.0.len()>1_048_576{return Err(JetCryptoError::PasswordPolicy{reason:"password exceeds 1048576 bytes"})}if !(8..=64).contains(&salt.len()){return Err(invalid_length("expert.argon2id","salt","8..=64",salt.len()))}if !(8_192..=262_144).contains(&memory_kib)||!(1..=10).contains(&iterations)||!(1..=8).contains(&lanes)||memory_kib<8*lanes||memory_kib.checked_mul(iterations).is_none_or(|v|v>1_048_576){return Err(JetCryptoError::PasswordPolicy{reason:"Argon2id parameters exceed policy"})}if !(16..=64).contains(&output_length){return Err(JetCryptoError::OutputLength{operation:"expert.argon2id",minimum:16,maximum:64,actual:output_length.unsigned_abs() as usize})}let params=argon2::Params::new(memory_kib as u32,iterations as u32,lanes as u32,Some(output_length as usize)).map_err(|_|JetCryptoError::PasswordPolicy{reason:"invalid Argon2id parameters"})?;let engine=argon2::Argon2::new(argon2::Algorithm::Argon2id,argon2::Version::V0x13,params);let mut out=vec![0;output_length as usize];engine.hash_password_into(&password.0,salt,&mut out).map_err(|_|JetCryptoError::ResourceUnavailable{resource:"password hashing"})?;Ok(Secret(out))
}
pub fn jet_crypto_expert_secret_bytes_impl(secret:&Secret)->Vec<u8>{secret.0.clone()}
pub fn jet_crypto_expert_signing_key_bytes_impl(key:&JetSigningKey)->Vec<u8>{key.0.clone()}
pub fn jet_crypto_expert_x25519_secret_bytes_impl(key:&JetX25519SecretKey)->Vec<u8>{key.0.clone()}
pub fn jet_crypto_expert_shared_secret_bytes_impl(secret:&JetSharedSecret)->Vec<u8>{secret.0.clone()}

const MAGIC: &[u8; 4] = b"JETC";
const VERSION: u8 = 1;
const ALGO_CHACHA20: u8 = 1;
const ALGO_AES256: u8 = 2;
const NONCE_LEN: usize = 12;
const JETC_V1_MIN_LEN: usize = 4 + 2 + NONCE_LEN + 16;
const JETC_V1_MAX_PLAINTEXT: usize = 1_073_741_824;
const JETC_V1_MAX_LEN: usize = JETC_V1_MIN_LEN + JETC_V1_MAX_PLAINTEXT;
const JETC_V2_VERSION: u8 = 2;
const JETC_V2_CHUNK: usize = 1_048_576;
const JETC_V2_MAX_PLAINTEXT: u64 = 1_099_511_627_776;
const JETC_V2_MAX_RECORDS: u64 = 1_048_577;
const JETC_V2_MAX_BODY_LEN: u64 = JETC_V2_MAX_PLAINTEXT + 21 * JETC_V2_MAX_RECORDS;
const JETC_V2_STANZA: usize = 96;
const JETC_V2_HEADER_BASE: usize = 74;
const JETC_V2_HEADER_TAG: usize = 16;

fn crypto_operation_error(_message: impl Into<String>) -> JetCryptoError {
    JetCryptoError::Internal { incident_id: "crypto-bridge" }
}

/// D-CRYPTO-ENVELOPE2=A: the only historical JETC v1 reader. Every grammar,
/// key, and authentication failure is deliberately collapsed to OpenFailed.
pub fn jet_crypto_expert_open_v1_impl(
    key: &Vec<u8>,
    envelope: &Vec<u8>,
) -> Result<Vec<u8>, JetCryptoError> {
    if key.len() != 32
        || !(JETC_V1_MIN_LEN..=JETC_V1_MAX_LEN).contains(&envelope.len())
        || &envelope[..4] != MAGIC
        || envelope[4] != VERSION
    {
        return Err(JetCryptoError::OpenFailed);
    }
    let nonce = &envelope[6..6 + NONCE_LEN];
    let ciphertext = &envelope[6 + NONCE_LEN..];
    match envelope[5] {
        ALGO_CHACHA20 => ChaCha20Poly1305::new_from_slice(key)
            .map_err(|_| JetCryptoError::OpenFailed)?
            .decrypt(ChaNonce::from_slice(nonce), ciphertext)
            .map_err(|_| JetCryptoError::OpenFailed),
        ALGO_AES256 => Aes256Gcm::new_from_slice(key)
            .map_err(|_| JetCryptoError::OpenFailed)?
            .decrypt(AesNonce::from_slice(nonce), ciphertext)
            .map_err(|_| JetCryptoError::OpenFailed),
        _ => Err(JetCryptoError::OpenFailed),
    }
}

fn jet_fill_random(out: &mut [u8]) -> Result<(), JetCryptoEntropyError> {
    jet_crypto_entropy_fill(out)
}

/// Sign `message` with a 32-byte Ed25519 seed key.
pub fn jet_crypto_sign_impl(
    secret_key: &Vec<u8>,
    message: &Vec<u8>,
) -> Result<Vec<u8>, String> {
    if secret_key.len() != 32 {
        return Err(format!(
            "crypto.sign expects a 32-byte secret key, got {} bytes",
            secret_key.len()
        ));
    }
    let seed: [u8; 32] = secret_key
        .as_slice()
        .try_into()
        .map_err(|_| "crypto.sign: bad key length".to_string())?;
    let signing_key = SigningKey::from_bytes(&seed);
    Ok(signing_key.sign(message).to_bytes().to_vec())
}

/// Generate a fresh Ed25519 keypair. Returns `(seed, public_key)` where `seed`
/// is the 32-byte secret seed and `public_key` is the 32-byte verifying key.
/// Randomness comes from the shared D-CRYPTO-RNG1 OS provider. Used by the
/// package-signing helper (c146); failure returns before any key artifact.
fn crypto_keygen() -> Result<(Vec<u8>, Vec<u8>), JetCryptoError> {
    let mut seed = [0u8; 32];
    jet_fill_random(&mut seed)?;
    let returned_seed = seed.to_vec();
    let signing_key = SigningKey::from_bytes(&seed);
    let public = signing_key.verifying_key().to_bytes().to_vec();
    jet_crypto_entropy_zeroize(&mut seed);
    Ok((returned_seed, public))
}

pub fn jet_crypto_keygen_impl() -> Result<(Vec<u8>, Vec<u8>), String> {
    crypto_keygen().map_err(|error| error.to_string())
}

/// Verify an Ed25519 signature (32-byte public key, 64-byte signature).
pub fn jet_crypto_verify_impl(
    public_key: &Vec<u8>,
    message: &Vec<u8>,
    signature: &Vec<u8>,
) -> Result<(), String> {
    if public_key.len() != 32 {
        return Err(format!(
            "crypto.verify expects a 32-byte public key, got {} bytes",
            public_key.len()
        ));
    }
    if signature.len() != 64 {
        return Err(format!(
            "crypto.verify expects a 64-byte signature, got {} bytes",
            signature.len()
        ));
    }
    let pk: [u8; 32] = public_key
        .as_slice()
        .try_into()
        .map_err(|_| "crypto.verify: bad public key length".to_string())?;
    let sig_bytes: [u8; 64] = signature
        .as_slice()
        .try_into()
        .map_err(|_| "crypto.verify: bad signature length".to_string())?;
    let verifying_key = VerifyingKey::from_bytes(&pk)
        .map_err(|e| format!("crypto.verify: invalid public key: {e}"))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
    verifying_key
        .verify(message, &sig)
        .map_err(|_| "crypto.verify: signature invalid".to_string())
}

pub fn jet_crypto_sha512_impl(data: &Vec<u8>) -> String {
    let digest = Sha512::digest(data);
    hex_encode(&digest)
}

pub fn jet_crypto_blake3_impl(data: &Vec<u8>) -> String {
    let digest = blake3::hash(data);
    digest.to_hex().to_string()
}

pub fn jet_crypto_constant_time_eq_impl(a: &Vec<u8>, b: &Vec<u8>) -> bool {
    a.as_slice().ct_eq(b.as_slice()).into()
}

pub fn jet_crypto_hkdf_sha256_impl(
    ikm: &Vec<u8>,
    salt: &Vec<u8>,
    info: &Vec<u8>,
    len: i64,
) -> Result<Vec<u8>, String> {
    if len < 0 {
        return Err("crypto.hkdf_sha256 length must be non-negative".to_string());
    }
    let mut out = vec![0u8; len as usize];
    let hk = Hkdf::<sha2::Sha256>::new(Some(salt.as_slice()), ikm);
    hk.expand(info, &mut out)
        .map_err(|_| "crypto.hkdf_sha256 length is too large".to_string())?;
    Ok(out)
}

pub fn jet_crypto_x25519_public_impl(secret: &Vec<u8>) -> Result<Vec<u8>, String> {
    let sk = bytes32(secret, "crypto.x25519_public secret")?;
    let public = x25519_dalek::x25519(sk, x25519_dalek::X25519_BASEPOINT_BYTES);
    Ok(public.to_vec())
}

pub fn jet_crypto_x25519_shared_impl(
    secret: &Vec<u8>,
    public: &Vec<u8>,
) -> Result<Vec<u8>, String> {
    let sk = bytes32(secret, "crypto.x25519_shared secret")?;
    let pk = bytes32(public, "crypto.x25519_shared public key")?;
    Ok(x25519_dalek::x25519(sk, pk).to_vec())
}

fn crypto_password_hash(password: &String) -> Result<String, JetCryptoError> {
    let mut salt = [0u8; 16];
    jet_fill_random(&mut salt)?;
    jet_crypto_password_hash_with_salt_impl(password, &salt.to_vec())
        .map_err(crypto_operation_error)
}

pub fn jet_crypto_password_hash_impl(password: &String) -> Result<String, String> {
    crypto_password_hash(password).map_err(|error| error.to_string())
}

pub fn jet_crypto_password_hash_with_salt_impl(
    password: &String,
    salt: &Vec<u8>,
) -> Result<String, String> {
    if salt.len() < 8 {
        return Err("crypto.password_hash salt must be at least 8 bytes".to_string());
    }
    let salt = argon2::password_hash::SaltString::encode_b64(salt)
        .map_err(|e| format!("crypto.password_hash salt failed: {e}"))?;
    let argon2 = argon2::Argon2::default();
    argon2::PasswordHasher::hash_password(&argon2, password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("crypto.password_hash failed: {e}"))
}

pub fn jet_crypto_password_verify_impl(password: &String, stored: &String) -> bool {
    let Ok(hash) = argon2::PasswordHash::new(stored) else {
        return false;
    };
    argon2::PasswordVerifier::verify_password(
        &argon2::Argon2::default(),
        password.as_bytes(),
        &hash,
    )
    .is_ok()
}

#[cfg(not(target_os = "linux"))]
struct JetcTemp {
    path: std::path::PathBuf,
    file: Option<std::fs::File>,
}

struct JetcParent {
    path: std::path::PathBuf,
    #[cfg(target_os = "linux")]
    file: std::fs::File,
    #[cfg(target_os = "linux")]
    destination: std::ffi::CString,
    #[cfg(not(target_os = "linux"))]
    metadata: std::fs::Metadata,
}

struct JetcPublishTemp {
    file: Option<std::fs::File>,
    #[cfg(not(target_os = "linux"))]
    path: std::path::PathBuf,
}

struct JetcSnapshot {
    file: std::fs::File,
    length: u64,
    hash: [u8; 32],
    metadata: std::fs::Metadata,
}

#[cfg(not(target_os = "linux"))]
static JETC_STAGE_COUNTER: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
thread_local! {
    static JETC_BOUNDARY_TEST_OBSERVER: std::cell::RefCell<Option<fn(&'static str)>> = const { std::cell::RefCell::new(None) };
    static JETC_IO_TEST_FAULT: std::cell::RefCell<Option<(&'static str, JetcIoTestFault)>> = const { std::cell::RefCell::new(None) };
    static JETC_IO_TEST_HITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[derive(Clone, Copy)]
enum JetcIoTestFault { Short(usize), EofAfter(usize), Error(i32) }

#[cfg(test)]
fn jet_crypto_set_file_boundary_test_observer(observer: fn(&'static str)) {
    JETC_BOUNDARY_TEST_OBSERVER.with(|slot| *slot.borrow_mut() = Some(observer));
}

#[cfg(test)]
fn jet_crypto_clear_file_boundary_test_observer() {
    JETC_BOUNDARY_TEST_OBSERVER.with(|slot| *slot.borrow_mut() = None);
}

#[cfg(test)]
fn observe_jetc_boundary(boundary: &'static str) {
    JETC_BOUNDARY_TEST_OBSERVER.with(|slot| {
        if let Some(observer) = *slot.borrow() { observer(boundary); }
    });
}

#[cfg(test)]
fn jet_crypto_set_file_io_test_fault(boundary: &'static str, fault: JetcIoTestFault) {
    JETC_IO_TEST_FAULT.with(|slot| *slot.borrow_mut() = Some((boundary, fault)));
    JETC_IO_TEST_HITS.with(|hits| hits.set(0));
}

#[cfg(test)]
fn jet_crypto_clear_file_io_test_fault() {
    JETC_IO_TEST_FAULT.with(|slot| *slot.borrow_mut() = None);
}

#[cfg(test)]
fn jet_crypto_file_io_test_hits() -> usize {
    JETC_IO_TEST_HITS.with(|hits| hits.get())
}

fn jetc_io_error(_boundary: &'static str) -> Option<std::io::Error> {
    #[cfg(test)]
    return JETC_IO_TEST_FAULT.with(|slot| {
        let mut slot = slot.borrow_mut();
        match *slot {
            Some((boundary, JetcIoTestFault::Error(code))) if boundary == _boundary => {
                slot.take();
                JETC_IO_TEST_HITS.with(|hits| hits.set(hits.get() + 1));
                Some(std::io::Error::from_raw_os_error(code))
            }
            _ => None,
        }
    });
    #[cfg(not(test))]
    None
}

fn jetc_io_limit(_boundary: &'static str, requested: usize) -> usize {
    #[cfg(test)]
    return JETC_IO_TEST_FAULT.with(|slot| {
        let mut slot = slot.borrow_mut();
        match slot.as_mut() {
            Some((boundary, JetcIoTestFault::Short(limit))) if *boundary == _boundary => {
                JETC_IO_TEST_HITS.with(|hits| hits.set(hits.get() + 1));
                requested.min(*limit)
            }
            Some((boundary, JetcIoTestFault::EofAfter(remaining))) if *boundary == _boundary => {
                JETC_IO_TEST_HITS.with(|hits| hits.set(hits.get() + 1));
                let limit = requested.min(*remaining);
                *remaining -= limit;
                limit
            }
            _ => requested,
        }
    });
    #[cfg(not(test))]
    requested
}

fn jetc_read(file: &mut std::fs::File, buffer: &mut [u8], boundary: &'static str) -> std::io::Result<usize> {
    use std::io::Read;
    loop {
        if let Some(error) = jetc_io_error(boundary) {
            if error.kind() == std::io::ErrorKind::Interrupted { continue; }
            return Err(error);
        }
        let limit = jetc_io_limit(boundary, buffer.len());
        match file.read(&mut buffer[..limit]) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

fn jetc_read_exact(file: &mut std::fs::File, buffer: &mut [u8], boundary: &'static str) -> std::io::Result<()> {
    let mut offset = 0;
    while offset < buffer.len() {
        let read = match jetc_read(file, &mut buffer[offset..], boundary) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if read == 0 { return Err(std::io::ErrorKind::UnexpectedEof.into()); }
        offset += read;
    }
    Ok(())
}

fn jetc_write_all<W: std::io::Write + ?Sized>(file: &mut W, bytes: &[u8], boundary: &'static str) -> std::io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        if let Some(error) = jetc_io_error(boundary) {
            if error.kind() == std::io::ErrorKind::Interrupted { continue; }
            return Err(error);
        }
        let limit = jetc_io_limit(boundary, bytes.len() - offset);
        let written = match file.write(&bytes[offset..offset + limit]) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if written == 0 { return Err(std::io::ErrorKind::WriteZero.into()); }
        offset += written;
    }
    Ok(())
}

fn jetc_sync_all(file: &std::fs::File, boundary: &'static str) -> std::io::Result<()> {
    if let Some(error) = jetc_io_error(boundary) { return Err(error); }
    file.sync_all()
}

fn jetc_fail_point(boundary: &'static str) -> std::io::Result<()> {
    match jetc_io_error(boundary) { Some(error) => Err(error), None => Ok(()) }
}

#[cfg(not(target_os = "linux"))]
impl Drop for JetcTemp {
    fn drop(&mut self) {
        self.file.take();
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for JetcPublishTemp {
    fn drop(&mut self) {
        self.file.take();
        #[cfg(not(target_os = "linux"))]
        let _ = std::fs::remove_file(&self.path);
    }
}

fn destination_parent_path(destination: &std::path::Path) -> Result<std::path::PathBuf, JetFileCryptoError> {
    let parent = destination.parent().ok_or(JetFileCryptoError::DestinationIo)?;
    Ok(if parent.as_os_str().is_empty() { std::path::PathBuf::from(".") } else { parent.to_path_buf() })
}

#[cfg(target_os = "linux")]
fn open_linux_directory_nofollow(path: &std::path::Path) -> Result<std::fs::File, JetFileCryptoError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    unsafe extern "C" { fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32; }
    const FLAGS: i32 = 0o200000 | 0o400000 | 0o2000000;
    let start = if path.is_absolute() { std::path::Path::new("/") } else { std::path::Path::new(".") };
    let mut directory = std::fs::OpenOptions::new().read(true)
        .custom_flags(0o200000 | 0o400000 | 0o2000000)
        .open(start).map_err(|_| JetFileCryptoError::DestinationIo)?;
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            if matches!(component, std::path::Component::RootDir | std::path::Component::CurDir) { continue; }
            return Err(JetFileCryptoError::DestinationIo);
        };
        let name = std::ffi::CString::new(name.as_bytes()).map_err(|_| JetFileCryptoError::DestinationIo)?;
        let fd = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), FLAGS, 0) };
        if fd < 0 { return Err(JetFileCryptoError::DestinationIo); }
        directory = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    if !directory.metadata().map_err(|_| JetFileCryptoError::DestinationIo)?.is_dir() {
        return Err(JetFileCryptoError::DestinationIo);
    }
    Ok(directory)
}

#[cfg(target_os = "linux")]
fn hold_destination_parent(destination: &std::path::Path) -> Result<JetcParent, JetFileCryptoError> {
    use std::os::unix::ffi::OsStrExt;
    let path = destination_parent_path(destination)?;
    let file = open_linux_directory_nofollow(&path)?;
    let name = destination.file_name().ok_or(JetFileCryptoError::DestinationIo)?;
    let destination = std::ffi::CString::new(name.as_bytes()).map_err(|_| JetFileCryptoError::DestinationIo)?;
    Ok(JetcParent { path, file, destination })
}

#[cfg(not(target_os = "linux"))]
fn hold_destination_parent(destination: &std::path::Path) -> Result<JetcParent, JetFileCryptoError> {
    let path = destination_parent_path(destination)?;
    let metadata = std::fs::metadata(&path).map_err(|_| JetFileCryptoError::DestinationIo)?;
    Ok(JetcParent { path, metadata })
}

#[cfg(target_os = "linux")]
fn revalidate_destination_parent(parent: &JetcParent) -> Result<(), JetFileCryptoError> {
    let current = open_linux_directory_nofollow(&parent.path)?;
    if !same_parent_identity(
        &parent.file.metadata().map_err(|_| JetFileCryptoError::DestinationIo)?,
        &current.metadata().map_err(|_| JetFileCryptoError::DestinationIo)?,
    ) { return Err(JetFileCryptoError::DestinationIo); }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn revalidate_destination_parent(parent: &JetcParent) -> Result<(), JetFileCryptoError> {
    let current = std::fs::metadata(&parent.path).map_err(|_| JetFileCryptoError::DestinationIo)?;
    if !same_parent_identity(&parent.metadata, &current) { return Err(JetFileCryptoError::DestinationIo); }
    Ok(())
}

#[cfg(target_os = "linux")]
fn create_linux_anonymous_file(parent: &JetcParent) -> Result<std::fs::File, JetFileCryptoError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::MetadataExt;
    unsafe extern "C" { fn openat(dirfd: i32, pathname: *const i8, flags: i32, mode: u32) -> i32; }
    const O_TMPFILE: i32 = 0o20200000;
    const O_CLOEXEC: i32 = 0o2000000;
    let fd = unsafe { openat(parent.file.as_raw_fd(), c".".as_ptr(), 0o2 | O_TMPFILE | O_CLOEXEC, 0o600) };
    if fd < 0 { return Err(JetFileCryptoError::DestinationIo); }
    let file = unsafe { std::fs::File::from_raw_fd(fd) };
    if file.metadata().map_err(|_| JetFileCryptoError::DestinationIo)?.nlink() != 0 {
        return Err(JetFileCryptoError::DestinationIo);
    }
    Ok(file)
}

#[cfg(target_os = "linux")]
fn create_publish_temp(parent: &JetcParent, _prefix: &str) -> Result<JetcPublishTemp, JetFileCryptoError> {
    Ok(JetcPublishTemp { file: Some(create_linux_anonymous_file(parent)?) })
}

#[cfg(target_os = "linux")]
fn create_unlinked_stage(parent: &JetcParent) -> Result<std::fs::File, JetFileCryptoError> {
    create_linux_anonymous_file(parent)
}

#[cfg(not(target_os = "linux"))]
fn create_unlinked_stage(parent: &JetcParent) -> Result<std::fs::File, JetFileCryptoError> {
    let mut stage = create_stage_in(&parent.path)?;
    std::fs::remove_file(&stage.path).map_err(|_| JetFileCryptoError::DestinationIo)?;
    stage.path = std::path::PathBuf::new();
    stage.file.take().ok_or(JetFileCryptoError::DestinationIo)
}

#[cfg(not(target_os = "linux"))]
fn create_publish_temp(parent: &JetcParent, prefix: &str) -> Result<JetcPublishTemp, JetFileCryptoError> {
    let mut temp = create_temp_in(&parent.path, prefix)?;
    Ok(JetcPublishTemp {
        file: temp.file.take(),
        path: std::mem::take(&mut temp.path),
    })
}

#[cfg(not(target_os = "linux"))]
fn open_private_new(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(not(target_os = "linux"))]
fn create_temp_in(dir: &std::path::Path, prefix: &str) -> Result<JetcTemp, JetFileCryptoError> {
    for _ in 0..128 {
        let sequence = JETC_STAGE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!(".{prefix}-{}-{sequence}", std::process::id()));
        match open_private_new(&path) {
            Ok(file) => return Ok(JetcTemp { path, file: Some(file) }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(JetFileCryptoError::DestinationIo),
        }
    }
    Err(JetFileCryptoError::DestinationIo)
}

#[cfg(not(target_os = "linux"))]
fn create_stage_in(dir: &std::path::Path) -> Result<JetcTemp, JetFileCryptoError> {
    for _ in 0..128 {
        let sequence = JETC_STAGE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!(".jetc-stage-{}-{sequence}", std::process::id()));
        match open_private_new(&path) {
            Ok(file) => return Ok(JetcTemp { path, file: Some(file) }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(JetFileCryptoError::DestinationIo),
        }
    }
    Err(JetFileCryptoError::DestinationIo)
}

#[cfg(target_os = "linux")]
fn open_source_nofollow(path: &std::path::Path) -> Result<std::fs::File, JetFileCryptoError> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new().read(true).custom_flags(0x20000).open(path)
        .map_err(|_| JetFileCryptoError::SourceIo)?;
    if !file.metadata().map_err(|_| JetFileCryptoError::SourceIo)?.is_file() {
        return Err(JetFileCryptoError::SourceIo);
    }
    Ok(file)
}

#[cfg(not(target_os = "linux"))]
fn open_source_nofollow(path: &std::path::Path) -> Result<std::fs::File, JetFileCryptoError> {
    if std::fs::symlink_metadata(path).map_err(|_| JetFileCryptoError::SourceIo)?.file_type().is_symlink() {
        return Err(JetFileCryptoError::SourceIo);
    }
    let file = std::fs::File::open(path).map_err(|_| JetFileCryptoError::SourceIo)?;
    if !file.metadata().map_err(|_| JetFileCryptoError::SourceIo)?.is_file() {
        return Err(JetFileCryptoError::SourceIo);
    }
    Ok(file)
}

fn same_source_identity(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino() && a.len() == b.len()
            && a.mtime() == b.mtime() && a.mtime_nsec() == b.mtime_nsec()
    }
    #[cfg(not(unix))]
    { a.len() == b.len() && a.modified().ok() == b.modified().ok() }
}

fn same_parent_identity(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    { a.permissions().readonly() == b.permissions().readonly() }
}

fn hash_stream(
    file: &mut std::fs::File,
    cancelled: fn() -> bool,
    boundary: &'static str,
    expected_length: u64,
) -> Result<([u8; 32], u64), JetFileCryptoError> {
    let mut hash = Sha256::new();
    let mut buffer = Zeroizing(vec![0u8; JETC_V2_CHUNK]);
    let mut length = 0u64;
    loop {
        if cancelled() { return Err(JetFileCryptoError::Cancelled); }
        let read = jetc_read(file, &mut buffer, boundary).map_err(|_| JetFileCryptoError::SourceIo)?;
        if read == 0 {
            if length != expected_length { return Err(JetFileCryptoError::SourceIo); }
            break;
        }
        length = length.checked_add(read as u64).ok_or(JetFileCryptoError::SourceIo)?;
        if length > expected_length || length > JETC_V2_MAX_PLAINTEXT {
            return Err(JetFileCryptoError::SourceIo);
        }
        hash.update(&buffer[..read]);
    }
    Ok((hash.finalize().into(), length))
}

#[cfg(target_os = "linux")]
fn snapshot_source(
    source: &std::path::Path,
    parent: &JetcParent,
    cancelled: fn() -> bool,
) -> Result<JetcSnapshot, JetFileCryptoError> {
    use std::io::{Seek, SeekFrom};
    let mut source_file = open_source_nofollow(source)?;
    let source_meta = source_file.metadata().map_err(|_| JetFileCryptoError::SourceIo)?;
    if source_meta.len() > JETC_V2_MAX_PLAINTEXT { return Err(JetFileCryptoError::SourceIo); }
    let mut stage_file = create_unlinked_stage(parent)?;
    #[cfg(test)]
    observe_jetc_boundary("seal-stage");
    if cancelled() { return Err(JetFileCryptoError::Cancelled); }
    let mut buffer = Zeroizing(vec![0u8; JETC_V2_CHUNK]);
    let mut source_hash = Sha256::new();
    let mut length = 0u64;
    loop {
        if cancelled() { return Err(JetFileCryptoError::Cancelled); }
        let read = jetc_read(&mut source_file, &mut buffer, "seal-source-read").map_err(|_| JetFileCryptoError::SourceIo)?;
        if read == 0 { break; }
        length = length.checked_add(read as u64).ok_or(JetFileCryptoError::SourceIo)?;
        if length > JETC_V2_MAX_PLAINTEXT { return Err(JetFileCryptoError::SourceIo); }
        source_hash.update(&buffer[..read]);
        jetc_write_all(&mut stage_file, &buffer[..read], "seal-stage-write").map_err(|_| JetFileCryptoError::DestinationIo)?;
    }
    if length != source_meta.len() { return Err(JetFileCryptoError::SourceIo); }
    jetc_sync_all(&stage_file, "seal-stage-fsync").map_err(|_| JetFileCryptoError::DestinationIo)?;
    stage_file.seek(SeekFrom::Start(0)).map_err(|_| JetFileCryptoError::DestinationIo)?;
    let (stage_hash, stage_len) = hash_stream(&mut stage_file, cancelled, "seal-stage-read", length)?;
    let first_hash: [u8; 32] = source_hash.finalize().into();
    if stage_len != length || !bool::from(stage_hash.ct_eq(&first_hash)) { return Err(JetFileCryptoError::SourceIo); }
    let mut second = open_source_nofollow(source)?;
    let second_meta = second.metadata().map_err(|_| JetFileCryptoError::SourceIo)?;
    if !same_source_identity(&source_meta, &second_meta) { return Err(JetFileCryptoError::SourceIo); }
    let (second_hash, second_len) = hash_stream(
        &mut second,
        cancelled,
        "seal-source-recheck-read",
        source_meta.len(),
    )?;
    if second_len != length || !bool::from(second_hash.ct_eq(&first_hash)) { return Err(JetFileCryptoError::SourceIo); }
    stage_file.seek(SeekFrom::Start(0)).map_err(|_| JetFileCryptoError::DestinationIo)?;
    let metadata = stage_file.metadata().map_err(|_| JetFileCryptoError::SourceIo)?;
    Ok(JetcSnapshot { file: stage_file, length, hash: stage_hash, metadata })
}

#[cfg(not(target_os = "linux"))]
fn snapshot_source(
    _source: &std::path::Path,
    _parent: &JetcParent,
    _cancelled: fn() -> bool,
) -> Result<JetcSnapshot, JetFileCryptoError> {
    Err(JetFileCryptoError::SourceIo)
}

fn jetc_v2_body_len(plain_len: u64) -> Option<u64> {
    if plain_len > JETC_V2_MAX_PLAINTEXT { return None; }
    let full = plain_len / JETC_V2_CHUNK as u64;
    let tail = plain_len % JETC_V2_CHUNK as u64;
    let non_final = full.checked_mul(JETC_V2_CHUNK as u64 + 21)?;
    non_final.checked_add(tail + 21)
}

fn jetc_v2_plain_len(body_len: u64) -> Option<u64> {
    let record_len = JETC_V2_CHUNK as u64 + 21;
    let full = body_len / record_len;
    let final_record_len = body_len % record_len;
    if !(21..record_len).contains(&final_record_len) { return None; }
    full.checked_mul(JETC_V2_CHUNK as u64)?
        .checked_add(final_record_len - 21)
        .filter(|length| *length <= JETC_V2_MAX_PLAINTEXT)
}

fn jetc_v2_container_len_matches(total: u64, header_len: usize, body_len: u64) -> bool {
    header_len <= 32 * 1024 * 1024
        && body_len <= JETC_V2_MAX_BODY_LEN
        && jetc_v2_plain_len(body_len).is_some()
        && 20u64.checked_add(header_len as u64).and_then(|n| n.checked_add(body_len)) == Some(total)
}

fn jetc_v2_record_shape_valid(length: usize, flags: u8) -> bool {
    match flags {
        0 => length == JETC_V2_CHUNK,
        1 => length < JETC_V2_CHUNK,
        _ => false,
    }
}

fn recipient_id(public: &[u8; 32]) -> [u8; 16] {
    let mut hash = Sha256::new();
    hash.update(b"JETC2 recipient id");
    hash.update(public);
    let digest: [u8; 32] = hash.finalize().into();
    digest[..16].try_into().expect("fixed digest slice")
}

fn nonce24(prefix: &[u8; 16], index: u64) -> [u8; 24] {
    let mut nonce = [0u8; 24];
    nonce[..16].copy_from_slice(prefix);
    nonce[16..].copy_from_slice(&index.to_le_bytes());
    nonce
}

#[cfg(target_os = "linux")]
fn publish_temp(mut temp: JetcPublishTemp, parent: &JetcParent, _destination: &std::path::Path, operation: &'static str) -> Result<(), JetFileCryptoError> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" { fn linkat(olddirfd: i32, oldpath: *const i8, newdirfd: i32, newpath: *const i8, flags: i32) -> i32; }
    const AT_EMPTY_PATH: i32 = 0x1000;
    let output_sync = match operation { "open" => "open-output-fsync", "migrate" => "migrate-output-fsync", _ => "seal-output-fsync" };
    let directory_sync = match operation { "open" => "open-directory-fsync", "migrate" => "migrate-directory-fsync", _ => "seal-directory-fsync" };
    jetc_sync_all(temp.file.as_ref().ok_or(JetFileCryptoError::DestinationIo)?, output_sync)
        .map_err(|_| JetFileCryptoError::DestinationIo)?;
    revalidate_destination_parent(parent)?;
    let file = temp.file.as_ref().ok_or(JetFileCryptoError::DestinationIo)?;
    if unsafe { linkat(file.as_raw_fd(), c"".as_ptr(), parent.file.as_raw_fd(), parent.destination.as_ptr(), AT_EMPTY_PATH) } != 0 {
        return if std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists {
            Err(JetFileCryptoError::DestinationExists)
        } else {
            Err(JetFileCryptoError::DestinationIo)
        };
    }
    jetc_sync_all(&parent.file, directory_sync).map_err(|_| JetFileCryptoError::DestinationIo)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn publish_temp(mut temp: JetcPublishTemp, parent: &JetcParent, destination: &std::path::Path, _operation: &'static str) -> Result<(), JetFileCryptoError> {
    temp.file.as_mut().ok_or(JetFileCryptoError::DestinationIo)?.sync_all()
        .map_err(|_| JetFileCryptoError::DestinationIo)?;
    temp.file.take();
    revalidate_destination_parent(parent)?;
    match std::fs::hard_link(&temp.path, destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Err(JetFileCryptoError::DestinationExists),
        Err(_) => return Err(JetFileCryptoError::DestinationIo),
    }
    std::fs::remove_file(&temp.path).map_err(|_| JetFileCryptoError::DestinationIo)?;
    std::fs::File::open(&parent.path).and_then(|file| file.sync_all()).map_err(|_| JetFileCryptoError::DestinationIo)?;
    temp.path = std::path::PathBuf::new();
    Ok(())
}

fn canonical_jetc_v2_recipients(
    mut recipients: Vec<JetX25519PublicKey>,
) -> Result<Vec<JetX25519PublicKey>, JetFileCryptoError> {
    if recipients.is_empty() || recipients.len() > 256 { return Err(JetFileCryptoError::SealFailed(JetCryptoError::InvalidLength { operation: "file_seal", parameter: "recipients", expected: "1..=256", actual: recipients.len() })); }
    recipients.sort_by_key(|key| (recipient_id(&key.0), key.0));
    if recipients.windows(2).any(|pair| pair[0].0 == pair[1].0) { return Err(JetFileCryptoError::SealFailed(JetCryptoError::InvalidEncoding { operation: "file_seal", value_kind: "duplicate recipient" })); }
    Ok(recipients)
}

fn seal_jetc_v2_from_snapshot(
    recipients: Vec<JetX25519PublicKey>,
    snapshot: JetcSnapshot,
    parent: &JetcParent,
    destination: &String,
    cancelled: fn() -> bool,
    verify_output: bool,
) -> Result<(), JetFileCryptoError> {
    use std::io::{Seek, SeekFrom};
    let JetcSnapshot { file: mut stage, length: plain_len, hash: expected_hash, metadata: expected_metadata } = snapshot;
    let destination = std::path::Path::new(destination);
    let body_len = jetc_v2_body_len(plain_len).ok_or(JetFileCryptoError::SourceIo)?;
    let header_len = JETC_V2_HEADER_BASE.checked_add(recipients.len() * JETC_V2_STANZA).and_then(|n| n.checked_add(JETC_V2_HEADER_TAG)).ok_or(JetFileCryptoError::DestinationIo)?;
    let mut fixed = Vec::with_capacity(20);
    fixed.extend_from_slice(b"JETC"); fixed.extend_from_slice(&[JETC_V2_VERSION, 1, 1, 0]);
    fixed.extend_from_slice(&(header_len as u32).to_le_bytes()); fixed.extend_from_slice(&body_len.to_le_bytes());
    let mut file_id = Zeroizing([0u8; 16]);
    let mut file_key = Zeroizing([0u8; 32]);
    let mut ephemeral_secret = Zeroizing([0u8; 32]);
    let mut nonce_prefix = Zeroizing([0u8; 16]);
    macro_rules! fill_envelope_random {
        ($target:expr) => {
            if let Err(error) = jet_crypto_entropy_fill($target) {
                return Err(JetFileCryptoError::SealFailed(error));
            }
        };
    }
    fill_envelope_random!(file_id.bytes_mut());
    fill_envelope_random!(file_key.bytes_mut());
    fill_envelope_random!(ephemeral_secret.bytes_mut());
    fill_envelope_random!(nonce_prefix.bytes_mut());
    let ephemeral_public = x25519_dalek::x25519(*ephemeral_secret, x25519_dalek::X25519_BASEPOINT_BYTES);
    let mut header = Vec::with_capacity(header_len);
    header.extend_from_slice(file_id.bytes()); header.extend_from_slice(&ephemeral_public); header.extend_from_slice(nonce_prefix.bytes());
    header.extend_from_slice(&(JETC_V2_CHUNK as u32).to_le_bytes()); header.extend_from_slice(&(recipients.len() as u16).to_le_bytes()); header.extend_from_slice(&0u32.to_le_bytes());
    let header_base = header.clone();
    for recipient in &recipients {
        if bool::from(recipient.0.ct_eq(&[0; 32])) { return Err(JetFileCryptoError::SealFailed(JetCryptoError::NonContributoryKey)); }
        let id = recipient_id(&recipient.0);
        let mut shared = Zeroizing(x25519_checked(*ephemeral_secret, recipient.0).map_err(JetFileCryptoError::SealFailed)?);
        let mut key_info = b"JETC2 wrap key".to_vec(); key_info.extend_from_slice(&ephemeral_public); key_info.extend_from_slice(&recipient.0);
        let mut nonce_info = b"JETC2 wrap nonce".to_vec(); nonce_info.extend_from_slice(&ephemeral_public); nonce_info.extend_from_slice(&recipient.0);
        let kek = Zeroizing(hkdf32(shared.bytes(), file_id.bytes(), &key_info).map_err(JetFileCryptoError::SealFailed)?);
        let wrap_nonce = Zeroizing(hkdf24(shared.bytes(), file_id.bytes(), &nonce_info).map_err(JetFileCryptoError::SealFailed)?);
        let mut aad = b"JETC2 wrap aad".to_vec(); aad.extend_from_slice(&fixed); aad.extend_from_slice(&header_base); aad.extend_from_slice(&id); aad.extend_from_slice(&recipient.0);
        let wrapped = XChaCha20Poly1305::new_from_slice(kek.bytes()).map_err(|_| JetFileCryptoError::SealFailed(JetCryptoError::Internal { incident_id: "jetc2-wrap-key" }))?
            .encrypt(XNonce::from_slice(wrap_nonce.bytes()), Payload { msg: file_key.bytes(), aad: &aad }).map_err(|_| JetFileCryptoError::SealFailed(JetCryptoError::Internal { incident_id: "jetc2-wrap" }))?;
        header.extend_from_slice(&id); header.extend_from_slice(&recipient.0); header.extend_from_slice(&wrapped);
    }
    let mut header_aad = b"JETC2 header".to_vec(); header_aad.extend_from_slice(&fixed); header_aad.extend_from_slice(&header);
    let header_tag = XChaCha20Poly1305::new_from_slice(file_key.bytes()).map_err(|_| JetFileCryptoError::SealFailed(JetCryptoError::Internal { incident_id: "jetc2-header-key" }))?
        .encrypt(XNonce::from_slice(&nonce24(&*nonce_prefix, 0)), Payload { msg: &[], aad: &header_aad }).map_err(|_| JetFileCryptoError::SealFailed(JetCryptoError::Internal { incident_id: "jetc2-header" }))?;
    header.extend_from_slice(&header_tag);
    let mut header_hash_input = fixed.clone(); header_hash_input.extend_from_slice(&header);
    let header_hash: [u8; 32] = Sha256::digest(&header_hash_input).into();
    let mut output = create_publish_temp(parent, "jetc-output")?;
    #[cfg(test)]
    observe_jetc_boundary(if verify_output { "migrate-output" } else { "seal-output" });
    if cancelled() { return Err(JetFileCryptoError::Cancelled); }
    let out = output.file.as_mut().ok_or(JetFileCryptoError::DestinationIo)?;
    let output_write = if verify_output { "migrate-output-write" } else { "seal-output-write" };
    jetc_write_all(out, &fixed, output_write).and_then(|_| jetc_write_all(out, &header, output_write))
        .map_err(|_| JetFileCryptoError::DestinationIo)?;
    let mut buffer = Zeroizing(vec![0u8; JETC_V2_CHUNK]); let mut index = 1u64; let mut remaining = plain_len;
    let mut staged_hash = Sha256::new();
    stage.seek(SeekFrom::Start(0)).map_err(|_| JetFileCryptoError::SourceIo)?;
    while remaining >= JETC_V2_CHUNK as u64 {
        #[cfg(test)]
        observe_jetc_boundary(if verify_output { "migrate-record" } else { "seal-record" });
        if cancelled() { return Err(JetFileCryptoError::Cancelled); }
        jetc_read_exact(&mut stage, &mut buffer, if verify_output { "migrate-stage-read" } else { "seal-stage-read" })
            .map_err(|_| JetFileCryptoError::SourceIo)?;
        staged_hash.update(buffer.bytes());
        let flags = 0u8; let length = JETC_V2_CHUNK as u32;
        let mut aad = b"JETC2 chunk".to_vec(); aad.extend_from_slice(&header_hash); aad.extend_from_slice(&index.to_le_bytes()); aad.extend_from_slice(&length.to_le_bytes()); aad.push(flags);
        let encrypted = XChaCha20Poly1305::new_from_slice(file_key.bytes()).map_err(|_| JetFileCryptoError::SealFailed(JetCryptoError::Internal { incident_id: "jetc2-body-key" }))?
            .encrypt(XNonce::from_slice(&nonce24(&*nonce_prefix, index)), Payload { msg: buffer.bytes(), aad: &aad }).map_err(|_| JetFileCryptoError::SealFailed(JetCryptoError::Internal { incident_id: "jetc2-body" }))?;
        jetc_write_all(out, &length.to_le_bytes(), output_write)
            .and_then(|_| jetc_write_all(out, &[flags], output_write))
            .and_then(|_| jetc_write_all(out, &encrypted, output_write))
            .map_err(|_| JetFileCryptoError::DestinationIo)?;
        remaining -= JETC_V2_CHUNK as u64; index += 1;
    }
    if index > JETC_V2_MAX_RECORDS { return Err(JetFileCryptoError::SourceIo); }
    #[cfg(test)]
    observe_jetc_boundary(if verify_output { "migrate-final" } else { "seal-final" });
    if cancelled() { return Err(JetFileCryptoError::Cancelled); }
    jetc_read_exact(&mut stage, &mut buffer[..remaining as usize], if verify_output { "migrate-stage-read" } else { "seal-stage-read" })
        .map_err(|_| JetFileCryptoError::SourceIo)?;
    staged_hash.update(&buffer[..remaining as usize]);
    let stage_read = if verify_output { "migrate-stage-read" } else { "seal-stage-read" };
    let mut trailing = [0u8; 1];
    if jetc_read(&mut stage, &mut trailing, stage_read).map_err(|_| JetFileCryptoError::SourceIo)? != 0
        || !same_source_identity(
            &expected_metadata,
            &stage.metadata().map_err(|_| JetFileCryptoError::SourceIo)?,
        )
        || !bool::from(<[u8; 32]>::from(staged_hash.finalize()).ct_eq(&expected_hash))
    {
        return Err(JetFileCryptoError::SourceIo);
    }
    let flags = 1u8; let length = remaining as u32;
    let mut aad = b"JETC2 chunk".to_vec(); aad.extend_from_slice(&header_hash); aad.extend_from_slice(&index.to_le_bytes()); aad.extend_from_slice(&length.to_le_bytes()); aad.push(flags);
    let encrypted = XChaCha20Poly1305::new_from_slice(file_key.bytes()).map_err(|_| JetFileCryptoError::SealFailed(JetCryptoError::Internal { incident_id: "jetc2-final-key" }))?
        .encrypt(XNonce::from_slice(&nonce24(&*nonce_prefix, index)), Payload { msg: &buffer[..remaining as usize], aad: &aad }).map_err(|_| JetFileCryptoError::SealFailed(JetCryptoError::Internal { incident_id: "jetc2-final" }))?;
    jetc_write_all(out, &length.to_le_bytes(), output_write)
        .and_then(|_| jetc_write_all(out, &[flags], output_write))
        .and_then(|_| jetc_write_all(out, &encrypted, output_write))
        .map_err(|_| JetFileCryptoError::DestinationIo)?;
    if verify_output {
        jetc_sync_all(out, "migrate-verify-fsync").map_err(|_| JetFileCryptoError::DestinationIo)?;
        jetc_fail_point("migrate-reopen-verify").map_err(|_| JetFileCryptoError::DestinationIo)?;
        let staged_output = output.file.as_ref().ok_or(JetFileCryptoError::DestinationIo)?
            .try_clone().map_err(|_| JetFileCryptoError::DestinationIo)?;
        open_jetc_v2_file(None, Some((file_key.bytes(), &*ephemeral_secret)), staged_output, None, cancelled)
            .map_err(|error| match error {
                JetFileCryptoError::Cancelled => JetFileCryptoError::Cancelled,
                _ => JetFileCryptoError::DestinationIo,
            })?;
    }
    #[cfg(test)]
    observe_jetc_boundary(if verify_output { "migrate-before-publish" } else { "seal-before-publish" });
    if cancelled() { return Err(JetFileCryptoError::Cancelled); }
    publish_temp(output, parent, destination, if verify_output { "migrate" } else { "seal" })
}

fn seal_jetc_v2(
    recipients: Vec<JetX25519PublicKey>,
    source: &String,
    destination: &String,
    cancelled: fn() -> bool,
    verify_output: bool,
) -> Result<(), JetFileCryptoError> {
    let recipients = canonical_jetc_v2_recipients(recipients)?;
    let destination_path = std::path::Path::new(destination);
    let parent = hold_destination_parent(destination_path)?;
    let snapshot = snapshot_source(std::path::Path::new(source), &parent, cancelled)?;
    seal_jetc_v2_from_snapshot(recipients, snapshot, &parent, destination, cancelled, verify_output)
}

pub fn jet_crypto_file_seal_impl(
    recipients: Vec<JetX25519PublicKey>,
    source: &String,
    destination: &String,
    cancelled: fn() -> bool,
) -> Result<(), JetFileCryptoError> {
    seal_jetc_v2(recipients, source, destination, cancelled, false)
}

fn unwrap_jetc_v2_stanza(
    private: [u8; 32],
    peer: [u8; 32],
    file_id: &[u8; 16],
    ephemeral_public: &[u8; 32],
    recipient_public: &[u8; 32],
    fixed: &[u8],
    header_base: &[u8],
    stanza: &[u8],
) -> Result<Zeroizing<Vec<u8>>, JetFileCryptoError> {
    let shared = Zeroizing(x25519_checked(private, peer).map_err(|_| JetFileCryptoError::OpenFailed)?);
    let mut key_info = b"JETC2 wrap key".to_vec();
    key_info.extend_from_slice(ephemeral_public);
    key_info.extend_from_slice(recipient_public);
    let mut nonce_info = b"JETC2 wrap nonce".to_vec();
    nonce_info.extend_from_slice(ephemeral_public);
    nonce_info.extend_from_slice(recipient_public);
    let kek = Zeroizing(hkdf32(shared.bytes(), file_id, &key_info).map_err(|_| JetFileCryptoError::OpenFailed)?);
    let wrap_nonce = Zeroizing(hkdf24(shared.bytes(), file_id, &nonce_info).map_err(|_| JetFileCryptoError::OpenFailed)?);
    let mut aad = b"JETC2 wrap aad".to_vec();
    aad.extend_from_slice(fixed);
    aad.extend_from_slice(header_base);
    aad.extend_from_slice(&stanza[..48]);
    Ok(Zeroizing(
        XChaCha20Poly1305::new_from_slice(kek.bytes())
            .map_err(|_| JetFileCryptoError::OpenFailed)?
            .decrypt(XNonce::from_slice(wrap_nonce.bytes()), Payload { msg: &stanza[48..96], aad: &aad })
            .map_err(|_| JetFileCryptoError::OpenFailed)?,
    ))
}

fn open_jetc_v2_file(
    recipient: Option<&JetX25519SecretKey>,
    known_writer_secrets: Option<(&[u8], &[u8; 32])>,
    mut input: std::fs::File,
    destination: Option<&String>,
    cancelled: fn() -> bool,
) -> Result<(), JetFileCryptoError> {
    use std::io::{Seek, SeekFrom, Write};
    input.seek(SeekFrom::Start(0)).map_err(|_| JetFileCryptoError::OpenFailed)?;
    let input_read = if known_writer_secrets.is_some() { "migrate-verify-read" } else { "open-input-read" };
    let total = input.metadata().map_err(|_| JetFileCryptoError::OpenFailed)?.len();
    let mut fixed = [0u8; 20]; jetc_read_exact(&mut input, &mut fixed, input_read).map_err(|_| JetFileCryptoError::OpenFailed)?;
    if &fixed[..4] != b"JETC" || fixed[4] != 2 || fixed[5] != 1 || fixed[6] != 1 || fixed[7] != 0 { return Err(JetFileCryptoError::OpenFailed); }
    let header_len = u32::from_le_bytes(fixed[8..12].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?) as usize;
    let body_len = u64::from_le_bytes(fixed[12..20].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?);
    if !jetc_v2_container_len_matches(total, header_len, body_len) { return Err(JetFileCryptoError::OpenFailed); }
    let mut header = vec![0u8; header_len]; jetc_read_exact(&mut input, &mut header, input_read).map_err(|_| JetFileCryptoError::OpenFailed)?;
    if header.len() < JETC_V2_HEADER_BASE + JETC_V2_HEADER_TAG { return Err(JetFileCryptoError::OpenFailed); }
    let file_id: [u8; 16] = header[0..16].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?;
    let ephemeral: [u8; 32] = header[16..48].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?;
    let nonce_prefix: [u8; 16] = header[48..64].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?;
    let chunk_size = u32::from_le_bytes(header[64..68].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?) as usize;
    let count = u16::from_le_bytes(header[68..70].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?) as usize;
    let metadata_len = u32::from_le_bytes(header[70..74].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?) as usize;
    let expected_header = JETC_V2_HEADER_BASE.checked_add(count.checked_mul(JETC_V2_STANZA).ok_or(JetFileCryptoError::OpenFailed)?).and_then(|n| n.checked_add(metadata_len)).and_then(|n| n.checked_add(16)).ok_or(JetFileCryptoError::OpenFailed)?;
    if chunk_size != JETC_V2_CHUNK || count == 0 || count > 256 || metadata_len != 0 || header_len != expected_header { return Err(JetFileCryptoError::OpenFailed); }
    let mut previous: Option<([u8; 16], [u8; 32])> = None;
    for index in 0..count {
        let start = JETC_V2_HEADER_BASE + index * JETC_V2_STANZA;
        let id: [u8; 16] = header[start..start+16].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?;
        let public: [u8; 32] = header[start+16..start+48].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?;
        if id != recipient_id(&public) || bool::from(public.ct_eq(&[0; 32])) || previous.is_some_and(|p| p >= (id, public)) { return Err(JetFileCryptoError::OpenFailed); }
        previous = Some((id, public));
    }
    let file_key = if let Some((known, ephemeral_secret)) = known_writer_secrets {
        if known.len() != 32 { return Err(JetFileCryptoError::OpenFailed); }
        if !bool::from(x25519_dalek::x25519(*ephemeral_secret, x25519_dalek::X25519_BASEPOINT_BYTES).ct_eq(&ephemeral)) {
            return Err(JetFileCryptoError::OpenFailed);
        }
        for index in 0..count {
            let start = JETC_V2_HEADER_BASE + index * JETC_V2_STANZA;
            let public: [u8; 32] = header[start+16..start+48].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?;
            let unwrapped = unwrap_jetc_v2_stanza(
                *ephemeral_secret,
                public,
                &file_id,
                &ephemeral,
                &public,
                &fixed,
                &header[..JETC_V2_HEADER_BASE],
                &header[start..start+JETC_V2_STANZA],
            )?;
            if !bool::from(unwrapped.bytes().ct_eq(known)) { return Err(JetFileCryptoError::OpenFailed); }
        }
        Zeroizing(known.to_vec())
    } else {
        let recipient = recipient.ok_or(JetFileCryptoError::OpenFailed)?;
        let mut secret = Zeroizing([0u8; 32]); secret.copy_from_slice(&recipient.0);
        let own_public = x25519_dalek::x25519(*secret, x25519_dalek::X25519_BASEPOINT_BYTES);
        let selected = (0..count).find(|index| {
            let start = JETC_V2_HEADER_BASE + index * JETC_V2_STANZA;
            bool::from(header[start+16..start+48].ct_eq(&own_public))
        }).unwrap_or(0);
        let start = JETC_V2_HEADER_BASE + selected * JETC_V2_STANZA;
        let stanza_public: [u8; 32] = header[start+16..start+48].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?;
        let matched = bool::from(stanza_public.ct_eq(&own_public));
        let unwrapped = unwrap_jetc_v2_stanza(
            *secret,
            ephemeral,
            &file_id,
            &ephemeral,
            &stanza_public,
            &fixed,
            &header[..JETC_V2_HEADER_BASE],
            &header[start..start+JETC_V2_STANZA],
        )?;
        if !matched { return Err(JetFileCryptoError::OpenFailed); }
        unwrapped
    };
    let tag_start = header_len - 16;
    let mut header_aad = b"JETC2 header".to_vec(); header_aad.extend_from_slice(&fixed); header_aad.extend_from_slice(&header[..tag_start]);
    XChaCha20Poly1305::new_from_slice(file_key.bytes()).map_err(|_| JetFileCryptoError::OpenFailed)?
        .decrypt(XNonce::from_slice(&nonce24(&nonce_prefix, 0)), Payload { msg: &header[tag_start..], aad: &header_aad }).map_err(|_| JetFileCryptoError::OpenFailed)?;
    let mut header_hash_input = fixed.to_vec(); header_hash_input.extend_from_slice(&header);
    let header_hash: [u8; 32] = Sha256::digest(&header_hash_input).into();
    let destination = destination.map(std::path::Path::new);
    let parent = destination.map(hold_destination_parent).transpose()?;
    let mut output = parent.as_ref().map(|parent| create_publish_temp(parent, "jetc-open")).transpose()?;
    #[cfg(test)]
    if output.is_some() { observe_jetc_boundary("open-output"); }
    if output.is_some() && cancelled() { return Err(JetFileCryptoError::Cancelled); }
    let mut discard = std::io::sink();
    let out: &mut dyn Write = if let Some(temp) = output.as_mut() {
        temp.file.as_mut().ok_or(JetFileCryptoError::DestinationIo)?
    } else {
        &mut discard
    };
    let mut consumed = 0u64; let mut index = 1u64; let mut saw_final = false; let mut plaintext = Zeroizing(vec![0u8; JETC_V2_CHUNK]);
    while consumed < body_len {
        #[cfg(test)]
        observe_jetc_boundary(if known_writer_secrets.is_some() { "migrate-verify-record" } else { "open-record" });
        if cancelled() { return Err(JetFileCryptoError::Cancelled); }
        if index > JETC_V2_MAX_RECORDS || body_len - consumed < 21 { return Err(JetFileCryptoError::OpenFailed); }
        let mut record = [0u8; 5]; jetc_read_exact(&mut input, &mut record, input_read).map_err(|_| JetFileCryptoError::OpenFailed)?;
        let length = u32::from_le_bytes(record[..4].try_into().map_err(|_| JetFileCryptoError::OpenFailed)?) as usize; let flags = record[4];
        if !jetc_v2_record_shape_valid(length, flags) || saw_final { return Err(JetFileCryptoError::OpenFailed); }
        let encrypted_len = length.checked_add(16).ok_or(JetFileCryptoError::OpenFailed)?;
        consumed = consumed.checked_add(5 + encrypted_len as u64).ok_or(JetFileCryptoError::OpenFailed)?;
        if consumed > body_len { return Err(JetFileCryptoError::OpenFailed); }
        let mut encrypted = vec![0u8; encrypted_len]; jetc_read_exact(&mut input, &mut encrypted, input_read).map_err(|_| JetFileCryptoError::OpenFailed)?;
        let mut chunk_aad = b"JETC2 chunk".to_vec(); chunk_aad.extend_from_slice(&header_hash); chunk_aad.extend_from_slice(&index.to_le_bytes()); chunk_aad.extend_from_slice(&(length as u32).to_le_bytes()); chunk_aad.push(flags);
        let clear = Zeroizing(XChaCha20Poly1305::new_from_slice(file_key.bytes()).map_err(|_| JetFileCryptoError::OpenFailed)?
            .decrypt(XNonce::from_slice(&nonce24(&nonce_prefix, index)), Payload { msg: &encrypted, aad: &chunk_aad }).map_err(|_| JetFileCryptoError::OpenFailed)?);
        plaintext[..length].copy_from_slice(clear.bytes());
        jetc_write_all(out, &plaintext[..length], "open-output-write").map_err(|_| JetFileCryptoError::DestinationIo)?;
        zeroize(&mut plaintext[..length]);
        saw_final = flags == 1; index += 1;
    }
    if !saw_final || consumed != body_len { return Err(JetFileCryptoError::OpenFailed); }
    #[cfg(test)]
    if destination.is_some() { observe_jetc_boundary("open-before-publish"); }
    if cancelled() { return Err(JetFileCryptoError::Cancelled); }
    if let Some(destination) = destination {
        publish_temp(
            output.ok_or(JetFileCryptoError::DestinationIo)?,
            parent.as_ref().ok_or(JetFileCryptoError::DestinationIo)?,
            destination,
            "open",
        )
    } else {
        Ok(())
    }
}

fn open_jetc_v2(
    recipient: Option<&JetX25519SecretKey>,
    known_writer_secrets: Option<(&[u8], &[u8; 32])>,
    source: &String,
    destination: Option<&String>,
    cancelled: fn() -> bool,
) -> Result<(), JetFileCryptoError> {
    let input = open_source_nofollow(std::path::Path::new(source))
        .map_err(|_| JetFileCryptoError::OpenFailed)?;
    open_jetc_v2_file(recipient, known_writer_secrets, input, destination, cancelled)
}

pub fn jet_crypto_file_open_impl(
    recipient: &JetX25519SecretKey,
    source: &String,
    destination: &String,
    cancelled: fn() -> bool,
) -> Result<(), JetFileCryptoError> {
    open_jetc_v2(Some(recipient), None, source, Some(destination), cancelled)
}

/// D-CRYPTO-ENVELOPE2=A: authenticate the sole canonical v1 grammar, reseal
/// through the canonical v2 writer, and reopen-verify before atomic publish.
pub fn jet_crypto_expert_migrate_v1_impl(
    key: &Vec<u8>,
    source: &String,
    recipients: Vec<JetX25519PublicKey>,
    destination: &String,
    cancelled: fn() -> bool,
) -> Result<(), JetFileCryptoError> {
    use std::io::{Seek, SeekFrom};
    let source_path = std::path::Path::new(source);
    let destination_path = std::path::Path::new(destination);
    if source_path == destination_path { return Err(JetFileCryptoError::DestinationExists); }
    let mut input = open_source_nofollow(source_path)?;
    let total = input.metadata().map_err(|_| JetFileCryptoError::SourceIo)?.len();
    if total > JETC_V1_MAX_LEN as u64 { return Err(JetFileCryptoError::OpenFailed); }
    let mut envelope = Zeroizing(Vec::with_capacity(total as usize));
    let mut buffer = Zeroizing(vec![0u8; JETC_V2_CHUNK]);
    loop {
        #[cfg(test)]
        observe_jetc_boundary("migrate-v1-read");
        if cancelled() { return Err(JetFileCryptoError::Cancelled); }
        let read = jetc_read(&mut input, &mut buffer, "migrate-v1-read-io").map_err(|_| JetFileCryptoError::SourceIo)?;
        if read == 0 { break; }
        if envelope.len().checked_add(read).is_none_or(|length| length > JETC_V1_MAX_LEN) {
            return Err(JetFileCryptoError::OpenFailed);
        }
        envelope.extend_from_slice(&buffer[..read]);
    }
    if envelope.len() as u64 != total { return Err(JetFileCryptoError::SourceIo); }
    let plaintext = Zeroizing(
        jet_crypto_expert_open_v1_impl(key, &envelope)
            .map_err(|_| JetFileCryptoError::OpenFailed)?,
    );
    let recipients = canonical_jetc_v2_recipients(recipients)?;
    let parent = hold_destination_parent(destination_path)?;
    let mut clear_file = create_unlinked_stage(&parent)?;
    #[cfg(test)]
    observe_jetc_boundary("migrate-stage");
    if cancelled() { return Err(JetFileCryptoError::Cancelled); }
    jetc_write_all(&mut clear_file, plaintext.bytes(), "migrate-stage-write").map_err(|_| JetFileCryptoError::DestinationIo)?;
    jetc_sync_all(&clear_file, "migrate-stage-fsync").map_err(|_| JetFileCryptoError::DestinationIo)?;
    let metadata = clear_file.metadata().map_err(|_| JetFileCryptoError::DestinationIo)?;
    clear_file.seek(SeekFrom::Start(0)).map_err(|_| JetFileCryptoError::DestinationIo)?;
    seal_jetc_v2_from_snapshot(
        recipients,
        JetcSnapshot {
            file: clear_file,
            length: plaintext.len() as u64,
            hash: Sha256::digest(plaintext.bytes()).into(),
            metadata,
        },
        &parent,
        destination,
        cancelled,
        true,
    )
}

fn bytes32(bytes: &[u8], label: &str) -> Result<[u8; 32], String> {
    bytes
        .try_into()
        .map_err(|_| format!("{label} expects 32 bytes, got {}", bytes.len()))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
