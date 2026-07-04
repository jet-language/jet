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
