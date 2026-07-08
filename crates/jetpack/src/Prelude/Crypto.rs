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
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use sha2::{Digest, Sha512};
use subtle::ConstantTimeEq;

const MAGIC: &[u8; 4] = b"JETC";
const VERSION: u8 = 1;
const ALGO_CHACHA20: u8 = 1;
const ALGO_AES256: u8 = 2;
const NONCE_LEN: usize = 12;

fn seal_with_algo(key: &[u8], plaintext: &[u8], algo: u8) -> Result<Vec<u8>, String> {
    if key.len() != 32 {
        return Err(format!(
            "crypto.seal expects a 32-byte key, got {} bytes",
            key.len()
        ));
    }
    let mut nonce = [0u8; NONCE_LEN];
    jet_fill_random(&mut nonce);
    let ciphertext = match algo {
        ALGO_CHACHA20 => {
            let cipher = ChaCha20Poly1305::new_from_slice(key)
                .map_err(|e| format!("invalid ChaCha20-Poly1305 key: {e}"))?;
            cipher
                .encrypt(ChaNonce::from_slice(&nonce), plaintext)
                .map_err(|e| format!("encryption failed: {e}"))?
        }
        ALGO_AES256 => {
            let cipher = Aes256Gcm::new_from_slice(key)
                .map_err(|e| format!("invalid AES-256-GCM key: {e}"))?;
            cipher
                .encrypt(AesNonce::from_slice(&nonce), plaintext)
                .map_err(|e| format!("encryption failed: {e}"))?
        }
        other => return Err(format!("unknown seal algorithm id {other}")),
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

fn jet_fill_random(out: &mut [u8]) {
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        if f.read_exact(out).is_ok() {
            return;
        }
    }
    let mut state: u64 = 0x4d595df4d0f33173;
    for b in out.iter_mut() {
        state ^= state << 7;
        state ^= state >> 9;
        state = state.wrapping_mul(0x9e3779b97f4a7c15);
        *b = state as u8;
    }
}

/// Default seal — ChaCha20-Poly1305 (D-CRYPTOENV1 misuse-resistant envelope).
pub fn jet_crypto_seal_impl(key: &Vec<u8>, plaintext: &Vec<u8>) -> Result<Vec<u8>, String> {
    seal_with_algo(key, plaintext, ALGO_CHACHA20)
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
    seal_with_algo(key, plaintext, algo as u8)
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
/// Randomness comes from the same `/dev/urandom`-then-PRNG path the envelope
/// nonce uses (`jet_fill_random`). Used by the package-signing helper (c146).
pub fn jet_crypto_keygen_impl() -> (Vec<u8>, Vec<u8>) {
    let mut seed = [0u8; 32];
    jet_fill_random(&mut seed);
    let signing_key = SigningKey::from_bytes(&seed);
    let public = signing_key.verifying_key().to_bytes().to_vec();
    (seed.to_vec(), public)
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

pub fn jet_crypto_password_hash_impl(password: &String) -> Result<String, String> {
    let mut salt = [0u8; 16];
    jet_fill_random(&mut salt);
    jet_crypto_password_hash_with_salt_impl(password, &salt.to_vec())
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
    jet_crypto_seal_impl(key, plaintext)
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
