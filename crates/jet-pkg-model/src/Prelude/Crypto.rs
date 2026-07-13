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
use sha2::{Digest, Sha512};
use subtle::ConstantTimeEq;

const JETV_MAGIC: &[u8; 4] = b"JETV";
const JETW_MAGIC: &[u8; 4] = b"JETW";
const JET_TYPED_VERSION: u8 = 1;
const JET_TYPED_SUITE: u8 = 1;

fn zeroize(bytes: &mut [u8]) {
    for byte in bytes { unsafe { std::ptr::write_volatile(byte, 0) } }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
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
pub fn jet_crypto_x25519_public_from_text_impl(text:String)->Result<JetX25519PublicKey,JetCryptoError>{if text.bytes().any(|b|b.is_ascii_uppercase())||!text.starts_with("jetx255191"){return Err(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m public key"})}let pos=text.rfind('1').ok_or(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m public key"})?;let encoded=text.as_bytes().get(pos+1..).filter(|s|s.len()>=6).ok_or(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m public key"})?;let mut values=Vec::with_capacity(encoded.len());for byte in encoded{values.push(BECH32_CHARSET.iter().position(|c|c==byte).ok_or(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m public key"})?as u8)}let mut check=bech32_hrp_expand(&text[..pos]);check.extend_from_slice(&values);if bech32_polymod(check)!=0x2bc830a3{return Err(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m checksum"})}let decoded=convert_bits(&values[..values.len()-6],5,8,false).ok_or(JetCryptoError::InvalidEncoding{operation:"X25519PublicKey.from_text",value_kind:"Bech32m public key"})?;Ok(JetX25519PublicKey(array32(&decoded,"X25519PublicKey.from_text","decoded key")?))}
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

const MAGIC: &[u8; 4] = b"JETC";
const VERSION: u8 = 1;
const ALGO_CHACHA20: u8 = 1;
const ALGO_AES256: u8 = 2;
const NONCE_LEN: usize = 12;

fn crypto_operation_error(_message: impl Into<String>) -> JetCryptoError {
    JetCryptoError::Internal { incident_id: "crypto-bridge" }
}

fn seal_with_algo(key: &[u8], plaintext: &[u8], algo: u8) -> Result<Vec<u8>, JetCryptoError> {
    if key.len() != 32 {
        return Err(crypto_operation_error(format!(
            "crypto.seal expects a 32-byte key, got {} bytes",
            key.len()
        )));
    }
    let mut nonce = [0u8; NONCE_LEN];
    jet_fill_random(&mut nonce)?;
    let ciphertext = match algo {
        ALGO_CHACHA20 => {
            let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|e| {
                crypto_operation_error(format!("invalid ChaCha20-Poly1305 key: {e}"))
            })?;
            cipher
                .encrypt(ChaNonce::from_slice(&nonce), plaintext)
                .map_err(|e| crypto_operation_error(format!("encryption failed: {e}")))?
        }
        ALGO_AES256 => {
            let cipher = Aes256Gcm::new_from_slice(key)
                .map_err(|e| crypto_operation_error(format!("invalid AES-256-GCM key: {e}")))?;
            cipher
                .encrypt(AesNonce::from_slice(&nonce), plaintext)
                .map_err(|e| crypto_operation_error(format!("encryption failed: {e}")))?
        }
        other => {
            return Err(crypto_operation_error(format!(
                "unknown seal algorithm id {other}"
            )))
        }
    };
    let mut out = Vec::with_capacity(4 + 2 + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(algo);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn open_envelope(key: &[u8], envelope: &[u8]) -> Result<Vec<u8>, String> {
    if key.len() != 32 {
        return Err(format!(
            "crypto.open expects a 32-byte key, got {} bytes",
            key.len()
        ));
    }
    if envelope.len() < 4 + 2 + NONCE_LEN + 16 {
        return Err("crypto.open: envelope too short".to_string());
    }
    if &envelope[..4] != MAGIC {
        return Err("crypto.open: not a Jet crypto envelope (bad magic)".to_string());
    }
    let version = envelope[4];
    if version != VERSION {
        return Err(format!(
            "crypto.open: unsupported envelope version {version} (only version {VERSION} is supported)"
        ));
    }
    let algo = envelope[5];
    let nonce = &envelope[6..6 + NONCE_LEN];
    let ciphertext = &envelope[6 + NONCE_LEN..];
    match algo {
        ALGO_CHACHA20 => {
            let cipher = ChaCha20Poly1305::new_from_slice(key)
                .map_err(|e| format!("invalid ChaCha20-Poly1305 key: {e}"))?;
            cipher
                .decrypt(ChaNonce::from_slice(nonce), ciphertext)
                .map_err(|_| {
                    "crypto.open: authentication failed (wrong key or corrupted envelope)"
                        .to_string()
                })
        }
        ALGO_AES256 => {
            let cipher = Aes256Gcm::new_from_slice(key)
                .map_err(|e| format!("invalid AES-256-GCM key: {e}"))?;
            cipher
                .decrypt(AesNonce::from_slice(nonce), ciphertext)
                .map_err(|_| {
                    "crypto.open: authentication failed (wrong key or corrupted envelope)"
                        .to_string()
                })
        }
        other => Err(format!("crypto.open: unknown algorithm id {other}")),
    }
}

fn jet_fill_random(out: &mut [u8]) -> Result<(), JetCryptoEntropyError> {
    jet_crypto_entropy_fill(out)
}

/// Default seal — ChaCha20-Poly1305 (D-CRYPTOENV1 misuse-resistant envelope).
pub fn jet_crypto_seal_impl(key: &Vec<u8>, plaintext: &Vec<u8>) -> Result<Vec<u8>, String> {
    seal_with_algo(key, plaintext, ALGO_CHACHA20).map_err(|error| error.to_string())
}

/// Open any supported envelope version/algorithm (algorithm agility).
pub fn jet_crypto_open_impl(key: &Vec<u8>, envelope: &Vec<u8>) -> Result<Vec<u8>, String> {
    open_envelope(key, envelope)
}

/// Expert-only: seal with a specific algorithm id (migration tests / expert tier).
pub fn jet_crypto_seal_algo_impl(
    key: &Vec<u8>,
    plaintext: &Vec<u8>,
    algo: i64,
) -> Result<Vec<u8>, String> {
    seal_with_algo(key, plaintext, algo as u8).map_err(|error| error.to_string())
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

pub fn jet_crypto_file_seal_impl(key: &Vec<u8>, plaintext: &Vec<u8>) -> Result<Vec<u8>, String> {
    seal_with_algo(key, plaintext, ALGO_CHACHA20).map_err(|error| error.to_string())
}

pub fn jet_crypto_file_open_impl(key: &Vec<u8>, envelope: &Vec<u8>) -> Result<Vec<u8>, String> {
    jet_crypto_open_impl(key, envelope)
}

fn bytes32(bytes: &[u8], label: &str) -> Result<[u8; 32], String> {
    bytes
        .try_into()
        .map_err(|_| format!("{label} expects 32 bytes, got {}", bytes.len()))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
