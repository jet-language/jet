//! Native JIT adapters for crypto, authentication, and vault calls.
//!
//! Algorithms come from the same source files emitted into AOT bridge crates.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use crate::Marshal::clone_string;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::Prelude::{jet_as_bytes, jet_e0956_unsupported};

pub(crate) mod runtime {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/SHA256Raw.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/SHAFamily.rs");
    pub(crate) use jet_crypto_entropy::jet_crypto_entropy_fill as jet_crypto_entropy_fill_for_host;
    use jet_crypto_entropy::{jet_crypto_entropy_fill, JetCryptoEntropyError};
    include!("../../jet-pkg-model/src/Prelude/Crypto.rs");
    include!("../../jet-pkg-model/src/Prelude/VaultNfc.rs");
    include!("../../jet-pkg-model/src/Prelude/SecretsCrypto.rs");
    include!("../../jet-pkg-model/src/Prelude/VaultKeyWrap.rs");

    pub fn digest256_hex_from_bytes(bytes: &[u8]) -> Option<String> {
        Some(jet_crypto_digest256_hex_impl(&JetDigest256(bytes.try_into().ok()?)))
    }

    pub fn digest512_hex_from_bytes(bytes: &[u8]) -> Option<String> {
        Some(jet_crypto_digest512_hex_impl(&JetDigest512(bytes.try_into().ok()?)))
    }

    use crate::Encoding::json_rt as jet_std;
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Auth.rs");
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs");

    pub fn auth_verify_jwt_defaulted(
        token: &String,
        key: &Vec<u8>,
        audience: &String,
        issuer: Option<&String>,
        clock_skew_ns: Option<i64>,
    ) -> Result<JetAuthClaims, JetAuthError> {
        jet_auth_verify_jwt_defaulted(token, key, audience, issuer, clock_skew_ns)
    }

    pub fn auth_verify_paseto_defaulted(
        token: &String,
        key: &Vec<u8>,
        audience: &String,
        issuer: Option<&String>,
        clock_skew_ns: Option<i64>,
        footer: Option<&Vec<u8>>,
        implicit: Option<&Vec<u8>>,
    ) -> Result<JetAuthClaims, JetAuthError> {
        jet_auth_verify_paseto_defaulted(
            token,
            key,
            audience,
            issuer,
            clock_skew_ns,
            footer,
            implicit,
            jet_crypto_expert_ed25519_verify_strict_impl,
        )
    }

    pub fn clone_secret(secret: &Secret) -> Secret {
        jet_crypto_secret_from_bytes_impl(jet_crypto_expert_secret_bytes_impl(secret))
    }

    pub fn clone_x25519_secret(key: &JetX25519SecretKey) -> JetX25519SecretKey {
        JetX25519SecretKey(jet_crypto_expert_x25519_secret_bytes_impl(key))
    }

    /// Interpreter ambient: rebuild a secret key from raw bytes.
    pub(crate) fn x25519_secret_from_bytes(bytes: Vec<u8>) -> Result<JetX25519SecretKey, String> {
        if bytes.len() != 32 {
            return Err("X25519SecretKey needs exactly 32 bytes".into());
        }
        Ok(JetX25519SecretKey(bytes))
    }

    /// Interpreter ambient: rebuild a password hash PHC string.
    pub(crate) fn password_hash_from_text(text: String) -> JetPasswordHash {
        JetPasswordHash(text)
    }
    /// Interpreter ambient: rebuild a signing key from its source-level bytes.
    pub(crate) fn signing_key_from_bytes(bytes: Vec<u8>) -> Result<JetSigningKey, String> {
        if bytes.len() != 32 {
            return Err("SigningKey needs exactly 32 bytes".into());
        }
        Ok(JetSigningKey(bytes))
    }

}

/// Interpreter ambient: rebuild X25519SecretKey without a Cranelift heap.
pub(crate) fn x25519_secret_from_vec(
    bytes: Vec<u8>,
) -> Result<runtime::JetX25519SecretKey, String> {
    runtime::x25519_secret_from_bytes(bytes)
}

pub(crate) enum CryptoValue {
    SigningKey(runtime::JetSigningKey),
    VerifyKey(runtime::JetVerifyKey),
    X25519SecretKey(runtime::JetX25519SecretKey),
    X25519PublicKey(runtime::JetX25519PublicKey),
    Signature(runtime::JetSignature),
    Digest256(runtime::JetDigest256),
    Digest512(runtime::JetDigest512),
    Hasher(runtime::JetCryptoHasher),
    Sealed(runtime::JetSealed),
    Secret(runtime::Secret),
    PasswordHash(runtime::JetPasswordHash),
    SharedSecret(runtime::JetSharedSecret),
    WrappedKey(runtime::JetWrappedKey),
    WrappedVaultKey(runtime::JetWrappedVaultKey),
    UnlockRecipient(i64),
    UnlockPassphrase(i64),
    KeyRefSigning(runtime::JetVaultKeyRef<runtime::JetSigningKey>),
    KeyRefX25519(runtime::JetVaultKeyRef<runtime::JetX25519SecretKey>),
    PlanSigning(runtime::JetVaultMutationPlan<runtime::JetSigningKey>),
    PlanX25519(runtime::JetVaultMutationPlan<runtime::JetX25519SecretKey>),
    WriteSigning(runtime::JetVaultWrite<runtime::JetSigningKey>),
    WriteX25519(runtime::JetVaultWrite<runtime::JetX25519SecretKey>),
    WrappedPlanSigning(runtime::JetVaultWrappedImportPlan<runtime::JetSigningKey>),
    WrappedPlanX25519(runtime::JetVaultWrappedImportPlan<runtime::JetX25519SecretKey>),
}

fn push(value: CryptoValue) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.crypto_values.push(Some(value));
        rt.crypto_values.len() as i64
    })
}

fn clone_bytes(handle: i64) -> Vec<u8> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(handle).unwrap_or(0);
        (0..len)
            .map(|index| rt.heap.list_get_int(handle, index).unwrap_or(0) as u8)
            .collect()
    })
}

fn path_string(handle: i64) -> String {
    Concurrency::with_runtime_mut(|rt| {
        let sid = rt.heap.record_get_string(handle, 0).unwrap_or(0);
        rt.heap.clone_string(sid).unwrap_or_default()
    })
}

fn alloc_bytes(bytes: &[u8]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for &byte in bytes {
            let _ = rt.heap.list_push_int(list, i64::from(byte));
        }
        list
    })
}

fn result(ok: bool, bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(super::JitResultValue { ok, bits });
        rt.results.len() as i64
    })
}

fn error(message: String) -> i64 {
    let handle = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(message));
    result(false, handle as u64)
}

fn err_debug(err: impl std::fmt::Debug) -> i64 {
    error(format!("{err:?}"))
}

fn claims_record(claims: runtime::JetAuthClaims) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(6);
        let subject = claims
            .subject
            .map(|value| rt.heap.alloc_string(value) + 1)
            .unwrap_or(0);
        let audience = rt.heap.alloc_string(claims.audience);
        let issuer = claims
            .issuer
            .map(|value| rt.heap.alloc_string(value) + 1)
            .unwrap_or(0);
        let (not_before_ok, not_before_bits) = match claims.not_before {
            Ok(value) => (true, value as u64),
            Err(_) => (false, 0),
        };
        let not_before = crate::runtime_host::alloc_jit_result(rt, not_before_ok, not_before_bits);
        let (issued_at_ok, issued_at_bits) = match claims.issued_at {
            Ok(value) => (true, value as u64),
            Err(_) => (false, 0),
        };
        // Optional NumericDate claims use the tagged result arena: `ok` is
        // presence and `bits` is the exact i64 payload. No scalar value is
        // reserved for absence.
        let issued_at = crate::runtime_host::alloc_jit_result(rt, issued_at_ok, issued_at_bits);
        let _ = rt.heap.record_set_int(record, 0, subject);
        let _ = rt.heap.record_set_string(record, 1, audience);
        let _ = rt.heap.record_set_int(record, 2, issuer);
        let _ = rt.heap.record_set_int(record, 3, claims.expires_at);
        let _ = rt.heap.record_set_int(record, 4, not_before);
        let _ = rt.heap.record_set_int(record, 5, issued_at);
        record
    })
}

// JIT ABI adapter only: encode the already-decided Prelude error into the
// tagged heap enum representation; verification policy remains in Auth.rs.
fn auth_error_bits(error: runtime::JetAuthError) -> u64 {
    const INVALID_SIGNATURE: u64 = 0;
    const WEAK_KEY: u64 = 1;
    const TOKEN_EXPIRED: u64 = 2;
    const MALFORMED_TOKEN: u64 = 3;
    const UNSUPPORTED_TOKEN: u64 = 4;
    const MISSING_CLAIM: u64 = 5;
    const DECODE_ERROR: u64 = 6;
    const WRONG_AUDIENCE: u64 = 7;
    const WRONG_ISSUER: u64 = 8;
    const TOKEN_NOT_YET_VALID: u64 = 9;
    enum AuthErrorField {
        Text(String),
        // Sema declares `AuthError.WrongIssuer.actual` an `Option<Text>`, so
        // the Prelude payload is the one carrier; this stays a marshalling shim.
        OptionalText(runtime::JetOutcome<String, runtime::JetAbsent>),
    }
    let named = |disc: u64, fields: Vec<AuthErrorField>| {
        Concurrency::with_runtime_mut(|rt| {
            let handle = rt.heap.alloc_record(fields.len() + 1);
            let _ = rt.heap.record_set_int(handle, 0, disc as i64);
            for (index, value) in fields.into_iter().enumerate() {
                let field = index as i64 + 1;
                match value {
                    AuthErrorField::Text(value) => {
                        let text = rt.heap.alloc_string(value);
                        let _ = rt.heap.record_set_string(handle, field, text);
                    }
                    AuthErrorField::OptionalText(value) => {
                        let bits = value
                            .map(|value| rt.heap.alloc_string(value) + 1)
                            .unwrap_or(0);
                        let _ = rt.heap.record_set_int(handle, field, bits);
                    }
                }
            }
            handle as u64
        })
    };
    match error {
        runtime::JetAuthError::InvalidSignature => named(INVALID_SIGNATURE, vec![]),
        runtime::JetAuthError::WeakKey => named(WEAK_KEY, vec![]),
        runtime::JetAuthError::TokenExpired => named(TOKEN_EXPIRED, vec![]),
        runtime::JetAuthError::MalformedToken(value) => {
            named(MALFORMED_TOKEN, vec![AuthErrorField::Text(value)])
        }
        runtime::JetAuthError::UnsupportedToken(value) => {
            named(UNSUPPORTED_TOKEN, vec![AuthErrorField::Text(value)])
        }
        runtime::JetAuthError::MissingClaim(value) => {
            named(MISSING_CLAIM, vec![AuthErrorField::Text(value)])
        }
        runtime::JetAuthError::DecodeError(value) => {
            named(DECODE_ERROR, vec![AuthErrorField::Text(value)])
        }
        runtime::JetAuthError::WrongAudience { expected, actual } => named(
            WRONG_AUDIENCE,
            vec![AuthErrorField::Text(expected), AuthErrorField::Text(actual)],
        ),
        runtime::JetAuthError::WrongIssuer { expected, actual } => named(
            WRONG_ISSUER,
            vec![
                AuthErrorField::Text(expected),
                AuthErrorField::OptionalText(actual),
            ],
        ),
        runtime::JetAuthError::TokenNotYetValid => named(TOKEN_NOT_YET_VALID, vec![]),
    }
}

fn take_crypto(handle: i64) -> Option<CryptoValue> {
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        rt.crypto_values.get_mut(index).and_then(Option::take)
    })
}

fn with_crypto<R>(handle: i64, f: impl FnOnce(&CryptoValue) -> Option<R>) -> Option<R> {
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        rt.crypto_values
            .get(index)
            .and_then(|slot| slot.as_ref())
            .and_then(f)
    })
}

fn with_crypto_mut<R>(handle: i64, f: impl FnOnce(&mut CryptoValue) -> Option<R>) -> Option<R> {
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        rt.crypto_values
            .get_mut(index)
            .and_then(|slot| slot.as_mut())
            .and_then(f)
    })
}

/// Snapshot closed-family secret material for `ExpiringSecret` zeroize-on-expiry.
/// Keeps the crypto handle live so `with` can loan it until expiry drops it.
pub(crate) fn claim_expiring_secret(handle: i64) -> Option<crate::Memory::SecretState> {
    let bytes = with_crypto(handle, |value| match value {
        CryptoValue::SigningKey(key) => {
            Some(runtime::jet_crypto_expert_signing_key_bytes_impl(key))
        }
        CryptoValue::X25519SecretKey(key) => {
            Some(runtime::jet_crypto_expert_x25519_secret_bytes_impl(key))
        }
        CryptoValue::Secret(secret) => Some(runtime::jet_crypto_expert_secret_bytes_impl(secret)),
        _ => None,
    })?;
    Some(crate::Memory::SecretState::from_material(handle, bytes))
}

pub(crate) fn drop_crypto_handle(handle: i64) {
    let _ = take_crypto(handle);
}

/// D-EMAIL-SMTP-CONFIG1=A: sole SMTP extraction boundary used by JIT email hosts.
pub(crate) fn secret_copy_for_smtp(handle: i64) -> Option<Vec<u8>> {
    with_crypto(handle, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::jet_crypto_secret_copy_for_smtp_impl(secret)),
        _ => None,
    })
}

fn public_keys(list: i64) -> Option<Vec<runtime::JetX25519PublicKey>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(list).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for index in 0..len {
            let handle = rt.heap.list_get_int(list, index)?;
            let key_index = handle.saturating_sub(1) as usize;
            match rt.crypto_values.get(key_index).and_then(Option::as_ref) {
                Some(CryptoValue::X25519PublicKey(key)) => out.push(key.clone()),
                _ => return None,
            }
        }
        Some(out)
    })
}

fn jet_jit_crypto_x25519_generate() -> i64 {
    match runtime::jet_crypto_x25519_generate_impl() {
        Ok(key) => result(true, push(CryptoValue::X25519SecretKey(key)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_x25519_public(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::X25519SecretKey(key) => {
            Some(runtime::jet_crypto_x25519_public_typed_impl(key))
        }
        _ => None,
    }) {
        Some(public) => push(CryptoValue::X25519PublicKey(public)),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid X25519 secret key handle"));
            0
        }
    }
}
fn jet_jit_crypto_x25519_public_from_bytes_typed(bytes: i64) -> i64 {
    match runtime::jet_crypto_x25519_public_from_bytes_impl(clone_bytes(bytes)) {
        Ok(key) => result(true, push(CryptoValue::X25519PublicKey(key)) as u64),
        Err(err) => error(err.to_string()),
    }
}


fn jet_jit_crypto_x25519(secret_handle: i64, public_handle: i64) -> i64 {
    let secret_key = with_crypto(secret_handle, |value| match value {
        CryptoValue::X25519SecretKey(key) => Some(runtime::clone_x25519_secret(key)),
        _ => None,
    });
    let public_key = take_crypto(public_handle);
    match (secret_key, public_key) {
        (Some(secret_key), Some(CryptoValue::X25519PublicKey(public_key))) => {
            match runtime::jet_crypto_x25519_typed_impl(&secret_key, public_key) {
                Ok(shared) => result(true, push(CryptoValue::SharedSecret(shared)) as u64),
                Err(err) => error(err.to_string()),
            }
        }
        _ => error("invalid X25519 secret or public key handle".to_string()),
    }
}

fn jet_jit_crypto_signing_generate() -> i64 {
    match runtime::jet_crypto_signing_generate_impl() {
        Ok(key) => result(true, push(CryptoValue::SigningKey(key)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_signing_public(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::SigningKey(key) => Some(runtime::jet_crypto_signing_public_impl(key)),
        _ => None,
    }) {
        Some(public) => push(CryptoValue::VerifyKey(public)),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid signing key handle"));
            0
        }
    }
}

fn jet_jit_crypto_sign(key_handle: i64, message_handle: i64) -> i64 {
    let message = clone_bytes(message_handle);
    let signed = with_crypto(key_handle, |value| match value {
        CryptoValue::SigningKey(key) => {
            Some(runtime::jet_crypto_sign_typed_impl(key, &message).map_err(|err| err.to_string()))
        }
        _ => None,
    });
    match signed {
        Some(Ok(signature)) => result(true, push(CryptoValue::Signature(signature)) as u64),
        Some(Err(message)) => error(message),
        None => error("invalid signing key handle".to_string()),
    }
}

fn jet_jit_crypto_verify(key_handle: i64, message_handle: i64, signature_handle: i64) -> i64 {
    let message = clone_bytes(message_handle);
    let key = take_crypto(key_handle);
    let signature = take_crypto(signature_handle);
    match (key, signature) {
        (Some(CryptoValue::VerifyKey(key)), Some(CryptoValue::Signature(signature))) => {
            match runtime::jet_crypto_verify_typed_impl(key, &message, signature) {
                Ok(valid) => result(true, u64::from(valid)),
                Err(err) => error(err.to_string()),
            }
        }
        _ => error("invalid verification key or signature handle".to_string()),
    }
}

fn jet_jit_crypto_sha256(data_handle: i64) -> i64 {
    let digest = runtime::jet_crypto_sha256_typed_impl(&clone_bytes(data_handle));
    push(CryptoValue::Digest256(digest))
}

fn jet_jit_crypto_blake3(data_handle: i64) -> i64 {
    let digest = runtime::jet_crypto_blake3_typed_impl(&clone_bytes(data_handle));
    push(CryptoValue::Digest256(digest))
}

fn jet_jit_crypto_sha512(data_handle: i64) -> i64 {
    let digest = runtime::jet_crypto_sha512_typed_impl(&clone_bytes(data_handle));
    push(CryptoValue::Digest512(digest))
}

fn jet_jit_crypto_sha1(data_handle: i64) -> i64 {
    let text = runtime::jet_crypto_sha1_hex(&clone_bytes(data_handle));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

fn jet_jit_crypto_sha224(data_handle: i64) -> i64 {
    let text = runtime::jet_crypto_sha224_hex(&clone_bytes(data_handle));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

fn jet_jit_crypto_sha384(data_handle: i64) -> i64 {
    let text = runtime::jet_crypto_sha384_hex(&clone_bytes(data_handle));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

fn jet_jit_crypto_sha3_224(data_handle: i64) -> i64 {
    let text = runtime::jet_crypto_sha3_224_hex(&clone_bytes(data_handle));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

fn jet_jit_crypto_sha3_256(data_handle: i64) -> i64 {
    let text = runtime::jet_crypto_sha3_256_hex(&clone_bytes(data_handle));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

fn jet_jit_crypto_sha3_384(data_handle: i64) -> i64 {
    let text = runtime::jet_crypto_sha3_384_hex(&clone_bytes(data_handle));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

fn jet_jit_crypto_sha3_512(data_handle: i64) -> i64 {
    let text = runtime::jet_crypto_sha3_512_hex(&clone_bytes(data_handle));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

fn jet_jit_crypto_hmac_sha256(key_handle: i64, data_handle: i64) -> i64 {
    let out = runtime::jet_crypto_hmac_sha256(&clone_bytes(key_handle), &clone_bytes(data_handle));
    alloc_bytes(&out)
}

fn jet_jit_crypto_pbkdf2_hmac(password: i64, salt: i64, iterations: i64, key_len: i64) -> i64 {
    let out = runtime::jet_crypto_pbkdf2_hmac(
        &clone_bytes(password),
        &clone_bytes(salt),
        iterations,
        key_len,
    );
    alloc_bytes(&out)
}

fn jet_jit_crypto_hasher_new() -> i64 {
    push(CryptoValue::Hasher(runtime::jet_crypto_hasher_new()))
}

fn jet_jit_crypto_hasher_update(handle: i64, data: i64) -> i64 {
    let bytes = clone_bytes(data);
    if with_crypto_mut(handle, |value| match value {
        CryptoValue::Hasher(hasher) => {
            runtime::jet_crypto_hasher_update(hasher, &bytes);
            Some(())
        }
        _ => None,
    })
    .is_some()
    {
        0
    } else {
        Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid Hasher handle"));
        0
    }
}

fn jet_jit_crypto_hasher_digest(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::Hasher(hasher) => Some(runtime::jet_crypto_hasher_digest(hasher)),
        _ => None,
    }) {
        Some(text) => Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text)),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid Hasher handle"));
            0
        }
    }
}

fn jet_jit_crypto_digest256_hex(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::Digest256(digest) => Some(runtime::jet_crypto_digest256_hex_impl(digest)),
        _ => None,
    }) {
        Some(text) => Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text)),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid SHA-256 digest handle"));
            0
        }
    }
}

fn jet_jit_crypto_digest256_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::Digest256(digest) => Some(runtime::jet_crypto_digest256_bytes_impl(digest)),
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid SHA-256 digest handle"));
            0
        }
    }
}

fn jet_jit_crypto_digest512_hex(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::Digest512(digest) => Some(runtime::jet_crypto_digest512_hex_impl(digest)),
        _ => None,
    }) {
        Some(text) => Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text)),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid SHA-512 digest handle"));
            0
        }
    }
}

fn jet_jit_crypto_digest512_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::Digest512(digest) => Some(runtime::jet_crypto_digest512_bytes_impl(digest)),
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid SHA-512 digest handle"));
            0
        }
    }
}

fn jet_jit_crypto_signature_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::Signature(signature) => {
            Some(runtime::jet_crypto_signature_bytes_impl(signature))
        }
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid signature handle"));
            0
        }
    }
}
fn jet_jit_crypto_verify_key_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::VerifyKey(key) => Some(runtime::jet_crypto_verify_key_bytes_impl(key)),
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid verify key handle"));
            0
        }
    }
}

fn jet_jit_crypto_wrapped_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::WrappedKey(wrapped) => Some(runtime::jet_crypto_wrapped_bytes_impl(wrapped)),
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid wrapped key handle"));
            0
        }
    }
}


fn jet_jit_crypto_sealed_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::Sealed(sealed) => Some(runtime::jet_crypto_sealed_bytes_impl(sealed)),
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid sealed handle"));
            0
        }
    }
}

fn jet_jit_crypto_x25519_public_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::X25519PublicKey(key) => {
            Some(runtime::jet_crypto_x25519_public_bytes_impl(key))
        }
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid X25519 public key handle"));
            0
        }
    }
}

fn jet_jit_crypto_x25519_public_text(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::X25519PublicKey(key) => Some(runtime::jet_crypto_x25519_public_text_impl(key)),
        _ => None,
    }) {
        Some(text) => Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text)),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid X25519 public key handle"));
            0
        }
    }
}

fn jet_jit_crypto_x25519_public_from_text(text: i64) -> i64 {
    match runtime::jet_crypto_x25519_public_from_text_impl(clone_string(text)) {
        Ok(key) => result(true, push(CryptoValue::X25519PublicKey(key)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_secret_from_text(text: i64) -> i64 {
    push(CryptoValue::Secret(
        runtime::jet_crypto_secret_from_text_impl(clone_string(text)),
    ))
}

fn jet_jit_crypto_random_bytes(count: i64) -> i64 {
    jet_codegen::scheduler::jet_scheduler_world_reject_uncontrolled("entropy");
    alloc_bytes(&runtime::jet_std_crypto_random_bytes(count))
}

fn jet_jit_crypto_wrap(secret_handle: i64, recipient_handle: i64) -> i64 {
    let Some(secret) = with_crypto(secret_handle, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    }) else {
        return error("invalid wrap secret handle".to_string());
    };
    let Some(recipient) = with_crypto(recipient_handle, |value| match value {
        CryptoValue::X25519PublicKey(recipient) => Some(recipient.clone()),
        _ => None,
    }) else {
        return error("invalid wrap recipient handle".to_string());
    };
    match runtime::jet_crypto_wrap_typed_impl(&secret, recipient) {
        Ok(wrapped) => result(true, push(CryptoValue::WrappedKey(wrapped)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_unwrap(recipient: i64, wrapped: i64) -> i64 {
    let Some(recipient) = with_crypto(recipient, |value| match value {
        CryptoValue::X25519SecretKey(recipient) => Some(runtime::clone_x25519_secret(recipient)),
        _ => None,
    }) else {
        return error("invalid unwrap recipient handle".to_string());
    };
    let Some(CryptoValue::WrappedKey(wrapped)) = take_crypto(wrapped) else {
        return error("invalid wrapped key handle".to_string());
    };
    match runtime::jet_crypto_unwrap_typed_impl(&recipient, wrapped) {
        Ok(secret) => result(true, push(CryptoValue::Secret(secret)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_seal(recipients: i64, plaintext: i64, aad: i64) -> i64 {
    let Some(recipients) = public_keys(recipients) else {
        return error("invalid seal recipient list".to_string());
    };
    match runtime::jet_crypto_seal_typed_impl(
        recipients,
        &clone_bytes(plaintext),
        &clone_bytes(aad),
    ) {
        Ok(sealed) => result(true, push(CryptoValue::Sealed(sealed)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_open(recipient: i64, sealed: i64, aad: i64) -> i64 {
    let aad = clone_bytes(aad);
    let recipient_key = with_crypto(recipient, |value| match value {
        CryptoValue::X25519SecretKey(key) => Some(runtime::clone_x25519_secret(key)),
        _ => None,
    });
    let sealed_value = take_crypto(sealed);
    match (recipient_key, sealed_value) {
        (Some(recipient), Some(CryptoValue::Sealed(sealed))) => {
            match runtime::jet_crypto_open_typed_impl(&recipient, sealed, &aad) {
                Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
                Err(err) => error(err.to_string()),
            }
        }
        _ => error("invalid open recipient or sealed handle".to_string()),
    }
}

fn jet_jit_crypto_password_hash(password: i64) -> i64 {
    let Some(secret) = with_crypto(password, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    }) else {
        return error("invalid password secret handle".to_string());
    };
    match runtime::jet_crypto_password_hash_typed_impl(&secret) {
        Ok(hash) => result(true, push(CryptoValue::PasswordHash(hash)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_password_verify(password: i64, stored: i64) -> i64 {
    let password = with_crypto(password, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    });
    let stored = with_crypto(stored, |value| match value {
        CryptoValue::PasswordHash(hash) => Some(hash.clone()),
        _ => None,
    });
    match (password, stored) {
        (Some(password), Some(stored)) => {
            match runtime::jet_crypto_password_verify_typed_impl(&password, &stored) {
                Ok(valid) => result(true, u64::from(valid)),
                Err(err) => error(err.to_string()),
            }
        }
        _ => error("invalid password verify handles".to_string()),
    }
}

fn jet_jit_crypto_password_text(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::PasswordHash(hash) => Some(runtime::jet_crypto_password_text_impl(hash)),
        _ => None,
    }) {
        Some(text) => Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text)),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid PasswordHash handle"));
            0
        }
    }
}

fn jet_jit_crypto_file_open(recipient: i64, source: i64, dest: i64) -> i64 {
    let source = path_string(source);
    let dest = path_string(dest);
    let Some(recipient) = with_crypto(recipient, |value| match value {
        CryptoValue::X25519SecretKey(key) => Some(runtime::clone_x25519_secret(key)),
        _ => None,
    }) else {
        return error("invalid file_open recipient".to_string());
    };
    match runtime::jet_crypto_file_open_impl(&recipient, &source, &dest, || false) {
        Ok(()) => result(true, 0),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_secret_from_bytes(bytes: i64) -> i64 {
    push(CryptoValue::Secret(
        runtime::jet_crypto_secret_from_bytes_impl(clone_bytes(bytes)),
    ))
}

fn jet_jit_crypto_hkdf_sha256(ikm: i64, salt: i64, info: i64, length: i64) -> i64 {
    let Some(secret) = with_crypto(ikm, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    }) else {
        return error("invalid hkdf ikm secret".to_string());
    };
    match runtime::jet_crypto_hkdf_typed_impl(
        &secret,
        &clone_bytes(salt),
        &clone_bytes(info),
        length,
    ) {
        Ok(out) => result(true, push(CryptoValue::Secret(out)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_x25519_public_bytes_raw(secret: i64) -> i64 {
    match runtime::jet_crypto_x25519_public_impl(&clone_bytes(secret)) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err),
    }
}

fn jet_jit_crypto_x25519_shared(secret: i64, public: i64) -> i64 {
    match runtime::jet_crypto_x25519_shared_impl(&clone_bytes(secret), &clone_bytes(public)) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err),
    }
}

fn jet_jit_crypto_constant_time_equal(a: i64, b: i64) -> i64 {
    let left = with_crypto(a, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    });
    let right = with_crypto(b, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    });
    match (left, right) {
        (Some(a), Some(b)) => i64::from(runtime::jet_crypto_constant_time_secret_impl(&a, &b)),
        _ => 0,
    }
}

fn jet_jit_crypto_constant_time_equal_bytes(a: i64, b: i64) -> i64 {
    i64::from(runtime::jet_crypto_constant_time_equal_bytes_impl(
        &clone_bytes(a),
        &clone_bytes(b),
    ))
}

fn jet_jit_crypto_file_seal(recipients: i64, source: i64, dest: i64) -> i64 {
    let source = path_string(source);
    let dest = path_string(dest);
    let Some(keys) = public_keys(recipients) else {
        return error("invalid file_seal recipients".to_string());
    };
    match runtime::jet_crypto_file_seal_impl(keys, &source, &dest, || false) {
        Ok(()) => result(true, 0),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_aes256gcm_seal(key: i64, nonce: i64, plaintext: i64, aad: i64) -> i64 {
    match runtime::jet_crypto_expert_aes256gcm_seal_impl(
        &clone_bytes(key),
        &clone_bytes(nonce),
        &clone_bytes(plaintext),
        &clone_bytes(aad),
    ) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_aes256gcm_open(key: i64, nonce: i64, ciphertext: i64, aad: i64) -> i64 {
    match runtime::jet_crypto_expert_aes256gcm_open_impl(
        &clone_bytes(key),
        &clone_bytes(nonce),
        &clone_bytes(ciphertext),
        &clone_bytes(aad),
    ) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_xchacha20poly1305_seal(
    key: i64,
    nonce: i64,
    plaintext: i64,
    aad: i64,
) -> i64 {
    match runtime::jet_crypto_expert_xchacha20poly1305_seal_impl(
        &clone_bytes(key),
        &clone_bytes(nonce),
        &clone_bytes(plaintext),
        &clone_bytes(aad),
    ) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_xchacha20poly1305_open(
    key: i64,
    nonce: i64,
    ciphertext: i64,
    aad: i64,
) -> i64 {
    match runtime::jet_crypto_expert_xchacha20poly1305_open_impl(
        &clone_bytes(key),
        &clone_bytes(nonce),
        &clone_bytes(ciphertext),
        &clone_bytes(aad),
    ) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_ed25519_sign(key: i64, message: i64) -> i64 {
    match runtime::jet_crypto_expert_ed25519_sign_impl(&clone_bytes(key), &clone_bytes(message)) {
        Ok(signature) => result(true, push(CryptoValue::Signature(signature)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_ed25519_verify(
    key: i64,
    message: i64,
    signature: i64,
) -> i64 {
    match runtime::jet_crypto_expert_ed25519_verify_strict_impl(
        &clone_bytes(key),
        &clone_bytes(message),
        &clone_bytes(signature),
    ) {
        Ok(valid) => result(true, u64::from(valid)),
        Err(err) => error(err.to_string()),
    }
}


fn jet_jit_crypto_expert_argon2id(
    password: i64,
    salt: i64,
    memory_kib: i64,
    iterations: i64,
    lanes: i64,
    output_length: i64,
) -> i64 {
    let Some(password) = with_crypto(password, |value| match value {
        CryptoValue::Secret(password) => Some(runtime::clone_secret(password)),
        _ => None,
    }) else {
        return error("invalid Secret handle".to_string());
    };
    match runtime::jet_crypto_expert_argon2id_cancel_impl(
        &password,
        &clone_bytes(salt),
        memory_kib,
        iterations,
        lanes,
        output_length,
        jet_codegen::scheduler::jet_scheduler_wait_point_cancelled,
        jet_codegen::scheduler::jet_task_deliver_cancel,
        jet_codegen::scheduler::jet_scheduler_blocking_wait_enter,
        jet_codegen::scheduler::jet_scheduler_blocking_wait_leave,
    ) {
        Ok(secret) => result(true, push(CryptoValue::Secret(secret)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_signing_key_bytes(key: i64) -> i64 {
    match with_crypto(key, |value| match value {
        CryptoValue::SigningKey(key) => {
            Some(runtime::jet_crypto_expert_signing_key_bytes_impl(key))
        }
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid signing key handle"));
            0
        }
    }
}

fn jet_jit_crypto_expert_x25519_secret_bytes(key: i64) -> i64 {
    match with_crypto(key, |value| match value {
        CryptoValue::X25519SecretKey(key) => {
            Some(runtime::jet_crypto_expert_x25519_secret_bytes_impl(key))
        }
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid X25519 secret key handle"));
            0
        }
    }
}

fn jet_jit_crypto_expert_open_v1(key: i64, blob: i64) -> i64 {
    match runtime::jet_crypto_expert_open_v1_impl(&clone_bytes(key), &clone_bytes(blob)) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_migrate_v1(key: i64, source: i64, recipients: i64, dest: i64) -> i64 {
    let Some(recipients) = public_keys(recipients) else {
        return error("invalid migrate recipients".to_string());
    };
    match runtime::jet_crypto_expert_migrate_v1_impl(
        &clone_bytes(key),
        &path_string(source),
        recipients,
        &path_string(dest),
        || false,
    ) {
        Ok(()) => result(true, 0),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_x25519(secret: i64, public: i64, reject_all_zero: i64) -> i64 {
    match runtime::jet_crypto_expert_x25519_impl(
        &clone_bytes(secret),
        &clone_bytes(public),
        reject_all_zero != 0,
    ) {
        Ok(secret) => result(true, push(CryptoValue::Secret(secret)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_hkdf_sha256(ikm: i64, salt: i64, info: i64, length: i64) -> i64 {
    match runtime::jet_crypto_expert_hkdf_sha256_impl(
        &clone_bytes(ikm),
        &clone_bytes(salt),
        &clone_bytes(info),
        length,
    ) {
        Ok(secret) => result(true, push(CryptoValue::Secret(secret)) as u64),
        Err(err) => error(err.to_string()),
    }
}

fn jet_jit_crypto_expert_secret_bytes(secret: i64) -> i64 {
    match with_crypto(secret, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::jet_crypto_expert_secret_bytes_impl(secret)),
        CryptoValue::SharedSecret(secret) => {
            Some(runtime::jet_crypto_expert_shared_secret_bytes_impl(secret))
        }
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid secret handle"));
            0
        }
    }
}

fn jet_jit_auth_verify_jwt(
    token: i64,
    key: i64,
    audience: i64,
    issuer: i64,
    skew_ns: i64,
    skew_present: i64,
) -> i64 {
    let token = clone_string(token);
    let key = clone_bytes(key);
    let audience = clone_string(audience);
    let issuer = (issuer != 0).then(|| clone_string(issuer - 1));
    let clock_skew_ns = (skew_present != 0).then_some(skew_ns);
    match runtime::auth_verify_jwt_defaulted(
        &token,
        &key,
        &audience,
        issuer.as_ref(),
        clock_skew_ns,
    ) {
        Ok(claims) => result(true, claims_record(claims) as u64),
        Err(err) => result(false, auth_error_bits(err)),
    }
}

fn jet_jit_auth_verify_paseto(
    token: i64,
    key: i64,
    audience: i64,
    issuer: i64,
    skew_ns: i64,
    skew_present: i64,
    footer: i64,
    footer_present: i64,
    implicit: i64,
    implicit_present: i64,
) -> i64 {
    let token = clone_string(token);
    let key = clone_bytes(key);
    let audience = clone_string(audience);
    let issuer = (issuer != 0).then(|| clone_string(issuer - 1));
    let clock_skew_ns = (skew_present != 0).then_some(skew_ns);
    let footer = (footer_present != 0).then(|| clone_bytes(footer));
    let implicit = (implicit_present != 0).then(|| clone_bytes(implicit));
    match runtime::auth_verify_paseto_defaulted(
        &token,
        &key,
        &audience,
        issuer.as_ref(),
        clock_skew_ns,
        footer.as_ref(),
        implicit.as_ref(),
    ) {
        Ok(claims) => result(true, claims_record(claims) as u64),
        Err(err) => result(false, auth_error_bits(err)),
    }
}

// Session/app adapters only translate heap values to the shared AuthSession
// Prelude structs. State, validation, expiry, and error meaning stay there.
fn session_record(session: runtime::JetAuthSession) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(4);
        let id = rt.heap.alloc_string(session.id);
        let user_id = rt.heap.alloc_string(session.user_id);
        let cookie = rt.heap.alloc_string(session.cookie);
        let _ = rt.heap.record_set_string(record, 0, id);
        let _ = rt.heap.record_set_string(record, 1, user_id);
        let _ = rt.heap.record_set_int(record, 2, session.expires_at);
        let _ = rt.heap.record_set_string(record, 3, cookie);
        record
    })
}

fn session_from_record(handle: i64) -> Option<runtime::JetAuthSession> {
    Concurrency::with_runtime_mut(|rt| {
        let id = rt.heap.record_get_string(handle, 0)?;
        let user_id = rt.heap.record_get_string(handle, 1)?;
        let expires_at = rt.heap.record_get_int(handle, 2)?;
        let cookie = rt.heap.record_get_string(handle, 3)?;
        Some(runtime::JetAuthSession {
            id: rt.heap.clone_string(id)?,
            user_id: rt.heap.clone_string(user_id)?,
            expires_at,
            cookie: rt.heap.clone_string(cookie)?,
        })
    })
}

fn app_record(app: runtime::JetAuthApp) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let users_table = rt.heap.alloc_string(app.users_table);
        let providers = rt.heap.alloc_empty_list();
        for provider in app.providers {
            let value = rt.heap.alloc_string(provider);
            let _ = rt.heap.list_push_int(providers, value);
        }
        let _ = rt.heap.record_set_string(record, 0, users_table);
        let _ = rt.heap.record_set_int(record, 1, providers);
        record
    })
}

fn app_from_record(handle: i64) -> Option<runtime::JetAuthApp> {
    Concurrency::with_runtime_mut(|rt| {
        let users_table = rt.heap.record_get_string(handle, 0)?;
        let providers = rt.heap.record_get_int(handle, 1)?;
        let len = rt.heap.list_len(providers)?;
        Some(runtime::JetAuthApp {
            users_table: rt.heap.clone_string(users_table)?,
            providers: (0..len)
                .map(|index| rt.heap.list_get_string(providers, index))
                .collect::<Option<Vec<_>>>()?,
        })
    })
}

fn invalid_auth_handle(kind: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.set_trap(kind));
    0
}

fn result_text(value: String) -> i64 {
    let handle = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value));
    result(true, handle as u64)
}

fn jet_jit_auth_register_user(user_id: i64, password_hash: i64) -> i64 {
    match runtime::jet_auth_register_user(clone_string(user_id), clone_string(password_hash)) {
        Ok(()) => result(true, 0),
        Err(message) => error(message),
    }
}

fn jet_jit_auth_password_login(user_id: i64, password_hash: i64, now_ms: i64, ttl_ms: i64) -> i64 {
    match runtime::auth_password_login(
        clone_string(user_id),
        clone_string(password_hash),
        now_ms,
        ttl_ms,
    ) {
        Ok(session) => result(true, session_record(session) as u64),
        Err(message) => error(message),
    }
}

fn jet_jit_auth_session_validate(session_id: i64, now_ms: i64) -> i64 {
    let session_id = clone_string(session_id);
    match runtime::auth_session_validate(&session_id, now_ms) {
        Ok(session) => result(true, session_record(session) as u64),
        Err(message) => error(message),
    }
}

fn jet_jit_auth_magic_link_issue(user_id: i64, now_ms: i64, ttl_ms: i64) -> i64 {
    match runtime::auth_magic_link_issue(clone_string(user_id), now_ms, ttl_ms) {
        Ok(token) => result_text(token),
        Err(message) => error(message),
    }
}

fn jet_jit_auth_magic_link_consume(token: i64, now_ms: i64, ttl_ms: i64) -> i64 {
    match runtime::jet_auth_magic_link_consume(clone_string(token), now_ms, ttl_ms) {
        Ok(session) => result(true, session_record(session) as u64),
        Err(message) => error(message),
    }
}

fn jet_jit_auth_oauth_begin(provider: i64) -> i64 {
    match runtime::auth_oauth_begin(clone_string(provider)) {
        Ok(state) => result_text(state),
        Err(message) => error(message),
    }
}

fn jet_jit_auth_oauth_finish(state: i64, subject: i64, now_ms: i64, ttl_ms: i64) -> i64 {
    match runtime::auth_oauth_finish(clone_string(state), clone_string(subject), now_ms, ttl_ms) {
        Ok(session) => result(true, session_record(session) as u64),
        Err(message) => error(message),
    }
}

fn jet_jit_auth_session_show(session: i64) -> i64 {
    let Some(session) = session_from_record(session) else {
        return invalid_auth_handle("invalid Session handle");
    };
    let value = runtime::auth_session_show(&session);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value))
}

fn jet_jit_auth_session_user(session: i64) -> i64 {
    let Some(session) = session_from_record(session) else {
        return invalid_auth_handle("invalid Session handle");
    };
    let value = runtime::auth_session_user(&session);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value))
}

fn jet_jit_auth_session_cookie(session: i64) -> i64 {
    let Some(session) = session_from_record(session) else {
        return invalid_auth_handle("invalid Session handle");
    };
    let value = runtime::auth_session_cookie(&session);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value))
}

fn jet_jit_auth_session_id(session: i64) -> i64 {
    let Some(session) = session_from_record(session) else {
        return invalid_auth_handle("invalid Session handle");
    };
    let value = runtime::auth_session_id(&session);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value))
}

fn jet_jit_app_auth(users_table: i64) -> i64 {
    app_record(runtime::app_auth(clone_string(users_table)))
}

fn jet_jit_app_auth_oauth(auth: i64, providers: i64) -> i64 {
    let Some(auth) = app_from_record(auth) else {
        return invalid_auth_handle("invalid Auth handle");
    };
    app_record(runtime::app_auth_oauth(auth, clone_string(providers)))
}

fn jet_jit_app_auth_routes(auth: i64) -> i64 {
    let Some(auth) = app_from_record(auth) else {
        return invalid_auth_handle("invalid Auth handle");
    };
    let value = runtime::app_auth_routes(&auth);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value))
}

fn jet_jit_app_auth_show(auth: i64) -> i64 {
    let Some(auth) = app_from_record(auth) else {
        return invalid_auth_handle("invalid Auth handle");
    };
    let value = runtime::app_auth_show(&auth);
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value))
}

fn jet_jit_vault_get(name: i64) -> i64 {
    match runtime::jet_vault_get_impl(&clone_string(name)) {
        None => 0,
        Some(value) => Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value) + 1),
    }
}

fn jet_jit_vault_key_ref_show(handle: i64) -> i64 {
    let text = with_crypto(handle, |value| match value {
        CryptoValue::KeyRefSigning(key) => Some(key.to_string()),
        CryptoValue::KeyRefX25519(key) => Some(key.to_string()),
        _ => None,
    })
    .unwrap_or_else(|| "<invalid KeyRef>".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

/// D-CRYPTO-VAULT1=A / I9: the ambient (interpreter) tier reads the vault
/// through the same `jet_vault_current_impl` this file's Cranelift host calls
/// and AOT emits. Name validation, the store read and every `JetVaultError`
/// row stay in `Prelude/SecretsCrypto.rs`; the engines differ only in how they
/// carry the resulting handle. `None` here means the key tag is not one this
/// vault knows, which is the caller's refusal to render, not a store outcome.
pub(crate) fn vault_current_handle(
    name: &str,
    tag: i64,
) -> Option<Result<Option<i64>, runtime::JetVaultError>> {
    let name = name.to_string();
    match tag {
        1 => Some(
            runtime::jet_vault_current_impl::<runtime::JetSigningKey>(&name)
                .map(|key| key.map(|key| push(CryptoValue::KeyRefSigning(key)))),
        ),
        2 => Some(
            runtime::jet_vault_current_impl::<runtime::JetX25519SecretKey>(&name)
                .map(|key| key.map(|key| push(CryptoValue::KeyRefX25519(key)))),
        ),
        _ => None,
    }
}

/// The ONE `impl Display for JetVaultKeyRef` rendering, read through the same
/// handle registry `jet_jit_vault_key_ref_show` uses.
pub(crate) fn vault_key_ref_text(handle: i64) -> Option<String> {
    with_crypto(handle, |value| match value {
        CryptoValue::KeyRefSigning(key) => Some(key.to_string()),
        CryptoValue::KeyRefX25519(key) => Some(key.to_string()),
        _ => None,
    })
}
/// Marshal vault mutation plans and writes through the resident native handle
/// table for the ambient interpreter. The Prelude owns every mutation rule;
/// this adapter only selects the erased key carrier and preserves ownership.
pub(crate) fn vault_prepare_rotate_handle(
    name: &str,
    tag: i64,
) -> Option<Result<i64, runtime::JetVaultError>> {
    match tag {
        1 => Some(
            runtime::jet_vault_prepare_rotate_impl::<runtime::JetSigningKey>(&name.to_string())
                .map(|plan| push(CryptoValue::PlanSigning(plan))),
        ),
        2 => Some(
            runtime::jet_vault_prepare_rotate_impl::<runtime::JetX25519SecretKey>(&name.to_string())
                .map(|plan| push(CryptoValue::PlanX25519(plan))),
        ),
        _ => None,
    }
}

pub(crate) fn vault_authorize_write_handle(
    plan: i64,
    reason: &str,
    tag: i64,
) -> Option<Result<i64, runtime::JetVaultError>> {
    match tag {
        1 => with_crypto(plan, |value| match value {
            CryptoValue::PlanSigning(plan) => {
                Some(runtime::jet_vault_authorize_write_impl(plan, reason))
            }
            _ => None,
        })
        .map(|result| result.map(|write| push(CryptoValue::WriteSigning(write)))),
        2 => with_crypto(plan, |value| match value {
            CryptoValue::PlanX25519(plan) => {
                Some(runtime::jet_vault_authorize_write_impl(plan, reason))
            }
            _ => None,
        })
        .map(|result| result.map(|write| push(CryptoValue::WriteX25519(write)))),
        _ => None,
    }
}

pub(crate) fn vault_commit_rotate_handles(
    write: i64,
    plan: i64,
    tag: i64,
) -> Option<Result<(i64, i64), runtime::JetVaultError>> {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
                Some(
                    runtime::jet_vault_commit_rotate_impl(write, plan).map(|rotation| {
                        (
                            push(CryptoValue::KeyRefSigning(rotation.previous)),
                            push(CryptoValue::KeyRefSigning(rotation.current)),
                        )
                    }),
                )
            }
            _ => None,
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                Some(
                    runtime::jet_vault_commit_rotate_impl(write, plan).map(|rotation| {
                        (
                            push(CryptoValue::KeyRefX25519(rotation.previous)),
                            push(CryptoValue::KeyRefX25519(rotation.current)),
                        )
                    }),
                )
            }
            _ => None,
        },
        _ => None,
    }
}
pub(crate) fn vault_prepare_generate_handle(
    name: &str,
    tag: i64,
) -> Option<Result<i64, runtime::JetVaultError>> {
    match tag {
        1 => Some(
            runtime::jet_vault_prepare_generate_impl::<runtime::JetSigningKey>(&name.to_string())
                .map(|plan| push(CryptoValue::PlanSigning(plan))),
        ),
        2 => Some(
            runtime::jet_vault_prepare_generate_impl::<runtime::JetX25519SecretKey>(
                &name.to_string(),
            )
            .map(|plan| push(CryptoValue::PlanX25519(plan))),
        ),
        _ => None,
    }
}

pub(crate) fn vault_prepare_store_handle(
    name: &str,
    key_bytes: Vec<u8>,
    tag: i64,
) -> Option<Result<i64, runtime::JetVaultError>> {
    match tag {
        1 => Some(
            runtime::signing_key_from_bytes(key_bytes)
                .map_err(|_| runtime::JetVaultError::InvalidEncoding)
                .and_then(|key| {
                    runtime::jet_vault_prepare_store_impl(&name.to_string(), key)
                })
                .map(|plan| push(CryptoValue::PlanSigning(plan))),
        ),
        2 => Some(
            runtime::x25519_secret_from_bytes(key_bytes)
                .map_err(|_| runtime::JetVaultError::InvalidEncoding)
                .and_then(|key| {
                    runtime::jet_vault_prepare_store_impl(&name.to_string(), key)
                })
                .map(|plan| push(CryptoValue::PlanX25519(plan))),
        ),
        _ => None,
    }
}

pub(crate) fn vault_commit_generate_handles(
    write: i64,
    plan: i64,
    tag: i64,
) -> Option<Result<i64, runtime::JetVaultError>> {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
                Some(
                    runtime::jet_vault_commit_generate_impl(write, plan)
                        .map(|key| push(CryptoValue::KeyRefSigning(key))),
                )
            }
            _ => None,
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                Some(
                    runtime::jet_vault_commit_generate_impl(write, plan)
                        .map(|key| push(CryptoValue::KeyRefX25519(key))),
                )
            }
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn vault_commit_store_handles(
    write: i64,
    plan: i64,
    tag: i64,
) -> Option<Result<i64, runtime::JetVaultError>> {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
                Some(
                    runtime::jet_vault_commit_store_impl(write, plan)
                        .map(|key| push(CryptoValue::KeyRefSigning(key))),
                )
            }
            _ => None,
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                Some(
                    runtime::jet_vault_commit_store_impl(write, plan)
                        .map(|key| push(CryptoValue::KeyRefX25519(key))),
                )
            }
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn vault_versions_handles(
    name: &str,
    tag: i64,
) -> Option<Result<Vec<i64>, runtime::JetVaultError>> {
    match tag {
        1 => Some(
            runtime::jet_vault_versions_impl::<runtime::JetSigningKey>(&name.to_string()).map(
                |keys| {
                    keys.into_iter()
                        .map(|key| push(CryptoValue::KeyRefSigning(key)))
                        .collect()
                },
            ),
        ),
        2 => Some(
            runtime::jet_vault_versions_impl::<runtime::JetX25519SecretKey>(&name.to_string()).map(
                |keys| {
                    keys.into_iter()
                        .map(|key| push(CryptoValue::KeyRefX25519(key)))
                        .collect()
                },
            ),
        ),
        _ => None,
    }
}

pub(crate) fn vault_expert_prepare_import_signing_handle(
    name: &str,
    bytes: Vec<u8>,
) -> Result<i64, runtime::JetVaultError> {
    runtime::jet_vault_expert_prepare_import_signing_impl(&name.to_string(), bytes)
        .map(|plan| push(CryptoValue::PlanSigning(plan)))
}

pub(crate) fn vault_expert_prepare_import_x25519_handle(
    name: &str,
    bytes: Vec<u8>,
) -> Result<i64, runtime::JetVaultError> {
    runtime::jet_vault_expert_prepare_import_x25519_impl(&name.to_string(), bytes)
        .map(|plan| push(CryptoValue::PlanX25519(plan)))
}

pub(crate) fn vault_expert_commit_import_signing_handles(
    write: i64,
    plan: i64,
) -> Option<Result<i64, runtime::JetVaultError>> {
    match (take_crypto(write), take_crypto(plan)) {
        (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => Some(
            runtime::jet_vault_expert_commit_import_signing_impl(write, plan)
                .map(|key| push(CryptoValue::KeyRefSigning(key))),
        ),
        _ => None,
    }
}

pub(crate) fn vault_expert_commit_import_x25519_handles(
    write: i64,
    plan: i64,
) -> Option<Result<i64, runtime::JetVaultError>> {
    match (take_crypto(write), take_crypto(plan)) {
        (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => Some(
            runtime::jet_vault_expert_commit_import_x25519_impl(write, plan)
                .map(|key| push(CryptoValue::KeyRefX25519(key))),
        ),
        _ => None,
    }
}

const VAULT_HANDLE_FIELD: &str = jet_foundation::Syntax::VAULT_KEY_REF_HANDLE;
const VAULT_SHOWN_FIELD: &str = jet_foundation::Syntax::VAULT_KEY_REF_SHOWN;

fn vault_diag(message: impl Into<String>, span: Span) -> Diagnostic {
    let message = message.into();
    jet_e0956_unsupported(&message, span)
}

fn crypto_carrier(type_name: &str, bytes: Vec<u8>) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
    }
}

fn crypto_password_carrier(text: String) -> CtValue {
    CtValue::Struct {
        type_name: "PasswordHash".to_string(),
        fields: vec![("text".to_string(), CtValue::Str(text))],
    }
}

fn crypto_error_value(error: impl std::fmt::Display) -> CtValue {
    CtValue::Struct {
        type_name: "CryptoError".to_string(),
        fields: vec![("reason".to_string(), CtValue::Str(error.to_string()))],
    }
}

fn crypto_failed(error: impl std::fmt::Display) -> Result<CtValue, Diagnostic> {
    Ok(CtValue::failed(Box::new(crypto_error_value(error))))
}

fn crypto_result<T>(
    value: Result<T, runtime::JetCryptoError>,
    encode: impl FnOnce(T) -> CtValue,
) -> Result<CtValue, Diagnostic> {
    match value {
        Ok(value) => Ok(CtValue::Present(Box::new(encode(value)))),
        Err(error) => crypto_failed(error),
    }
}

fn vault_enum(
    type_name: &str,
    variant: &str,
    args: Vec<(Option<String>, CtValue)>,
) -> CtValue {
    CtValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args,
    }
}

fn vault_carrier(type_name: &str, handle: i64) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![(VAULT_HANDLE_FIELD.to_string(), CtValue::Int(handle))],
    }
}

fn vault_key_ref_value(handle: i64) -> CtValue {
    CtValue::Struct {
        type_name: jet_foundation::Syntax::VAULT_KEY_REF_TYPE.to_string(),
        fields: vec![
            (VAULT_HANDLE_FIELD.to_string(), CtValue::Int(handle)),
            (
                VAULT_SHOWN_FIELD.to_string(),
                CtValue::Str(
                    vault_key_ref_text(handle)
                        .unwrap_or_else(|| "<invalid KeyRef>".to_string()),
                ),
            ),
        ],
    }
}

fn vault_handle(value: &CtValue, expected: &str) -> Option<i64> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != expected {
        return None;
    }
    fields.iter().find_map(|(name, value)| {
        (name == VAULT_HANDLE_FIELD).then(|| match value {
            CtValue::Int(handle) if *handle > 0 => Some(*handle),
            _ => None,
        })?
    })
}

fn vault_field<'a>(value: &'a CtValue, type_name: &str, name: &str) -> Option<&'a CtValue> {
    let CtValue::Struct {
        type_name: actual,
        fields,
    } = value
    else {
        return None;
    };
    (actual == type_name)
        .then(|| fields.iter().find(|(field, _)| field == name).map(|(_, value)| value))?
}

fn vault_nominal_bytes(
    value: &CtValue,
    type_name: &str,
    span: Span,
) -> Result<Vec<u8>, Diagnostic> {
    let field = vault_field(value, type_name, "bytes")
        .ok_or_else(|| vault_diag(format!("malformed {type_name} value"), span))?;
    jet_as_bytes(field, span)
}

fn vault_type_tag_name(name: &str) -> Option<i64> {
    match name.rsplit('.').next().unwrap_or(name) {
        "SigningKey" => Some(1),
        "X25519SecretKey" => Some(2),
        _ => None,
    }
}

fn vault_tag_from_type(ty: &Type) -> Option<i64> {
    match ty {
        Type::Named(name) => vault_type_tag_name(name),
        Type::Apply { name, args } => vault_type_tag_name(name)
            .or_else(|| args.iter().find_map(vault_tag_from_type)),
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            vault_tag_from_type(inner)
        }
        Type::Map { key, value, .. } => {
            vault_tag_from_type(key).or_else(|| vault_tag_from_type(value))
        }
        Type::Result { ok, err } => {
            vault_tag_from_type(ok).or_else(|| vault_tag_from_type(err))
        }
        Type::Tuple(fields) => fields.iter().find_map(|(_, ty)| vault_tag_from_type(ty)),
        Type::FixedList { elem, .. }
        | Type::InlineRange { base: elem, .. }
        | Type::Tagged { inner: elem, .. }
        | Type::Quantity { base: elem, .. } => vault_tag_from_type(elem),
        Type::TraitObject(names) => names.iter().find_map(|name| vault_type_tag_name(name)),
        Type::Union(members) => members.iter().find_map(vault_tag_from_type),
        _ => None,
    }
}

fn vault_tag_from_source_value(value: &CtValue) -> Option<i64> {
    let CtValue::Struct { type_name, .. } = value else {
        return None;
    };
    vault_type_tag_name(type_name)
}

fn crypto_value_tag(value: &CryptoValue) -> Option<i64> {
    match value {
        CryptoValue::KeyRefSigning(_)
        | CryptoValue::PlanSigning(_)
        | CryptoValue::WriteSigning(_)
        | CryptoValue::WrappedPlanSigning(_) => Some(1),
        CryptoValue::KeyRefX25519(_)
        | CryptoValue::PlanX25519(_)
        | CryptoValue::WriteX25519(_)
        | CryptoValue::WrappedPlanX25519(_) => Some(2),
        _ => None,
    }
}

fn vault_tag_from_carrier(value: &CtValue) -> Option<i64> {
    let handle = [
        jet_foundation::Syntax::VAULT_KEY_REF_TYPE,
        "MutationPlan",
        "VaultWrite",
        "WrappedImportPlan",
    ]
    .iter()
    .find_map(|type_name| vault_handle(value, type_name))?;
    with_crypto(handle, |value| crypto_value_tag(value))
}

fn vault_tag_for_args(
    resolved_ret: Option<&Type>,
    args: &[CtValue],
    fallback_indexes: &[usize],
    span: Span,
) -> Result<i64, Diagnostic> {
    resolved_ret
        .and_then(vault_tag_from_type)
        .or_else(|| {
            fallback_indexes.iter().find_map(|index| {
                args.get(*index).and_then(|value| {
                    vault_tag_from_carrier(value).or_else(|| vault_tag_from_source_value(value))
                })
            })
        })
        .ok_or_else(|| vault_diag("vault call has no checked key type", span))
}

fn vault_ok(value: CtValue) -> Result<CtValue, Diagnostic> {
    Ok(CtValue::Present(Box::new(value)))
}

fn vault_error_value(error: runtime::JetVaultError) -> CtValue {
    use runtime::JetVaultError;
    match error {
        JetVaultError::InvalidName => vault_enum("VaultError", "InvalidName", Vec::new()),
        JetVaultError::NotFound => vault_enum("VaultError", "NotFound", Vec::new()),
        JetVaultError::WrongType => vault_enum("VaultError", "WrongType", Vec::new()),
        JetVaultError::Revoked => vault_enum("VaultError", "Revoked", Vec::new()),
        JetVaultError::Locked => vault_enum("VaultError", "Locked", Vec::new()),
        JetVaultError::AuthorityDenied => {
            vault_enum("VaultError", "AuthorityDenied", Vec::new())
        }
        JetVaultError::Conflict => vault_enum("VaultError", "Conflict", Vec::new()),
        JetVaultError::UnsupportedProvider => {
            vault_enum("VaultError", "UnsupportedProvider", Vec::new())
        }
        JetVaultError::InvalidEncoding => {
            vault_enum("VaultError", "InvalidEncoding", Vec::new())
        }
        JetVaultError::DurabilityUnknown => {
            vault_enum("VaultError", "DurabilityUnknown", Vec::new())
        }
        JetVaultError::Crypto(error) => vault_enum(
            "VaultError",
            "Crypto",
            vec![(
                None,
                CtValue::Struct {
                    type_name: "CryptoError".to_string(),
                    fields: vec![("reason".to_string(), CtValue::Str(error.to_string()))],
                },
            )],
        ),
        JetVaultError::IO {
            operation,
            redacted_path,
        } => vault_enum(
            "VaultError",
            "IO",
            vec![
                (
                    Some("operation".to_string()),
                    CtValue::Str(operation.to_string()),
                ),
                (
                    Some("redacted_path".to_string()),
                    CtValue::Str(redacted_path.to_string()),
                ),
            ],
        ),
        JetVaultError::Internal { incident_id } => vault_enum(
            "VaultError",
            "Internal",
            vec![(
                Some("incident_id".to_string()),
                CtValue::Str(incident_id.to_string()),
            )],
        ),
    }
}

fn vault_wrap_error_value(error: runtime::JetVaultKeyWrapError) -> CtValue {
    use runtime::JetVaultKeyWrapError;
    match error {
        JetVaultKeyWrapError::InvalidEncoding => {
            vault_enum("KeyWrapError", "InvalidEncoding", Vec::new())
        }
        JetVaultKeyWrapError::UnsupportedVersion => {
            vault_enum("KeyWrapError", "UnsupportedVersion", Vec::new())
        }
        JetVaultKeyWrapError::UnsupportedMode => {
            vault_enum("KeyWrapError", "UnsupportedMode", Vec::new())
        }
        JetVaultKeyWrapError::UnsupportedKeyType => {
            vault_enum("KeyWrapError", "UnsupportedKeyType", Vec::new())
        }
        JetVaultKeyWrapError::InvalidLength => {
            vault_enum("KeyWrapError", "InvalidLength", Vec::new())
        }
        JetVaultKeyWrapError::WeakPassphrase => {
            vault_enum("KeyWrapError", "WeakPassphrase", Vec::new())
        }
        JetVaultKeyWrapError::OpenFailed => {
            vault_enum("KeyWrapError", "OpenFailed", Vec::new())
        }
        JetVaultKeyWrapError::EntropyUnavailable => {
            vault_enum("KeyWrapError", "EntropyUnavailable", Vec::new())
        }
        JetVaultKeyWrapError::ResourceUnavailable => {
            vault_enum("KeyWrapError", "ResourceUnavailable", Vec::new())
        }
        JetVaultKeyWrapError::Vault(error) => {
            vault_enum("KeyWrapError", "Vault", vec![(None, vault_error_value(error))])
        }
        JetVaultKeyWrapError::Internal { incident_id } => vault_enum(
            "KeyWrapError",
            "Internal",
            vec![(
                Some("incident_id".to_string()),
                CtValue::Str(incident_id.to_string()),
            )],
        ),
    }
}

fn vault_failed(error: runtime::JetVaultError) -> Result<CtValue, Diagnostic> {
    Ok(CtValue::failed(Box::new(vault_error_value(error))))
}

fn vault_wrap_failed(error: runtime::JetVaultKeyWrapError) -> Result<CtValue, Diagnostic> {
    Ok(CtValue::failed(Box::new(vault_wrap_error_value(error))))
}

fn vault_result<T>(
    value: Result<T, runtime::JetVaultError>,
    encode: impl FnOnce(T) -> CtValue,
) -> Result<CtValue, Diagnostic> {
    match value {
        Ok(value) => vault_ok(encode(value)),
        Err(error) => vault_failed(error),
    }
}

fn vault_wrap_result<T>(
    value: Result<T, runtime::JetVaultKeyWrapError>,
    encode: impl FnOnce(T) -> CtValue,
) -> Result<CtValue, Diagnostic> {
    match value {
        Ok(value) => vault_ok(encode(value)),
        Err(error) => vault_wrap_failed(error),
    }
}

fn vault_current_result(
    value: Result<Option<i64>, runtime::JetVaultError>,
) -> Result<CtValue, Diagnostic> {
    match value {
        Ok(Some(handle)) => vault_ok(CtValue::Present(Box::new(vault_key_ref_value(handle)))),
        Ok(None) => vault_ok(CtValue::absent(Type::Named("KeyRef".to_string()))),
        Err(error) => vault_failed(error),
    }
}

fn vault_status_value(status: runtime::JetVaultKeyStatus) -> CtValue {
    let variant = match status {
        runtime::JetVaultKeyStatus::Active => "Active",
        runtime::JetVaultKeyStatus::Retired => "Retired",
        runtime::JetVaultKeyStatus::Revoked => "Revoked",
    };
    vault_enum("KeyStatus", variant, Vec::new())
}

fn vault_rotation_value(previous: i64, current: i64) -> CtValue {
    CtValue::Struct {
        type_name: "Rotation".to_string(),
        fields: vec![
            ("previous".to_string(), vault_key_ref_value(previous)),
            ("current".to_string(), vault_key_ref_value(current)),
        ],
    }
}

fn vault_loaded_value(tag: i64, key: runtime::JetSigningKey) -> CtValue {
    CtValue::Struct {
        type_name: "SigningKey".to_string(),
        fields: vec![(
            "bytes".to_string(),
            CtValue::Bytes(runtime::jet_crypto_expert_signing_key_bytes_impl(&key)),
        )],
    }
}

fn vault_loaded_x25519_value(key: runtime::JetX25519SecretKey) -> CtValue {
    CtValue::Struct {
        type_name: "X25519SecretKey".to_string(),
        fields: vec![(
            "bytes".to_string(),
            CtValue::Bytes(runtime::jet_crypto_expert_x25519_secret_bytes_impl(&key)),
        )],
    }
}

fn vault_prepare_lifecycle_handle(
    key_ref: i64,
    reason: &str,
    tag: i64,
    revoke: bool,
) -> Option<Result<i64, runtime::JetVaultError>> {
    let reason = reason.to_string();
    match tag {
        1 => with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefSigning(key) => Some(if revoke {
                runtime::jet_vault_prepare_revoke_impl(key, &reason)
            } else {
                runtime::jet_vault_prepare_retire_impl(key, &reason)
            }),
            _ => None,
        })
        .map(|value| value.map(|plan| push(CryptoValue::PlanSigning(plan)))),
        2 => with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => Some(if revoke {
                runtime::jet_vault_prepare_revoke_impl(key, &reason)
            } else {
                runtime::jet_vault_prepare_retire_impl(key, &reason)
            }),
            _ => None,
        })
        .map(|value| value.map(|plan| push(CryptoValue::PlanX25519(plan)))),
        _ => None,
    }
}

fn vault_commit_void_handles(
    write: i64,
    plan: i64,
    tag: i64,
    revoke: bool,
) -> Option<Result<(), runtime::JetVaultError>> {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
                Some(if revoke {
                    runtime::jet_vault_commit_revoke_impl(write, plan)
                } else {
                    runtime::jet_vault_commit_retire_impl(write, plan)
                })
            }
            _ => None,
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                Some(if revoke {
                    runtime::jet_vault_commit_revoke_impl(write, plan)
                } else {
                    runtime::jet_vault_commit_retire_impl(write, plan)
                })
            }
            _ => None,
        },
        _ => None,
    }
}

fn vault_authorize_wrapped_handle(
    plan: i64,
    reason: &str,
    tag: i64,
) -> Option<Result<i64, runtime::JetVaultKeyWrapError>> {
    let reason = reason.to_string();
    match tag {
        1 => with_crypto(plan, |value| match value {
            CryptoValue::WrappedPlanSigning(plan) => Some(
                runtime::jet_vault_authorize_wrapped_import_impl(plan, &reason),
            ),
            _ => None,
        })
        .map(|value| value.map(|write| push(CryptoValue::WriteSigning(write)))),
        2 => with_crypto(plan, |value| match value {
            CryptoValue::WrappedPlanX25519(plan) => Some(
                runtime::jet_vault_authorize_wrapped_import_impl(plan, &reason),
            ),
            _ => None,
        })
        .map(|value| value.map(|write| push(CryptoValue::WriteX25519(write)))),
        _ => None,
    }
}

fn vault_commit_wrapped_handles(
    write: i64,
    plan: i64,
    tag: i64,
) -> Option<Result<i64, runtime::JetVaultKeyWrapError>> {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::WrappedPlanSigning(plan))) => {
                Some(
                    runtime::jet_vault_commit_import_wrapped_impl(write, plan)
                        .map(|key| push(CryptoValue::KeyRefSigning(key))),
                )
            }
            _ => None,
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::WrappedPlanX25519(plan))) => {
                Some(
                    runtime::jet_vault_commit_import_wrapped_impl(write, plan)
                        .map(|key| push(CryptoValue::KeyRefX25519(key))),
                )
            }
            _ => None,
        },
        _ => None,
    }
}

enum AmbientVaultUnlock {
    Recipient(runtime::JetX25519SecretKey),
    Passphrase(runtime::Secret),
}

fn vault_unlock_from_handle(handle: i64) -> Option<AmbientVaultUnlock> {
    match take_crypto(handle) {
        Some(CryptoValue::UnlockRecipient(identity)) => with_crypto(identity, |value| match value {
            CryptoValue::X25519SecretKey(key) => {
                Some(AmbientVaultUnlock::Recipient(runtime::clone_x25519_secret(key)))
            }
            _ => None,
        }),
        Some(CryptoValue::UnlockPassphrase(passphrase)) => {
            with_crypto(passphrase, |value| match value {
                CryptoValue::Secret(secret) => {
                    Some(AmbientVaultUnlock::Passphrase(runtime::clone_secret(secret)))
                }
                _ => None,
            })
        }
        _ => None,
    }
}

fn vault_public_keys(
    value: &CtValue,
    span: Span,
) -> Result<Vec<runtime::JetX25519PublicKey>, Diagnostic> {
    let CtValue::List(items) = value else {
        return Err(vault_diag(
            "vault export recipients must be a list of X25519PublicKey",
            span,
        ));
    };
    items
        .iter()
        .map(|item| {
            let bytes = vault_nominal_bytes(item, "X25519PublicKey", span)?;
            runtime::jet_crypto_x25519_public_from_bytes_impl(bytes).map_err(|_| {
                vault_diag("vault export received an invalid X25519PublicKey", span)
            })
        })
        .collect()
}

fn ambient_core_call(
    module: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    resolved_ret: Option<Type>,
    _sink: Option<&mut jet_codegen::Comptime::DevSink>,
) -> Option<Result<CtValue, Diagnostic>> {
    if module == "core.crypto" {
        return match method {
            "sha256" | "sha512" | "blake3" => {
                let [input] = args.as_slice() else {
                    return Some(Err(vault_diag("malformed typed digest arguments", span)));
                };
                let input = match jet_as_bytes(input, span) {
                    Ok(input) => input,
                    Err(error) => return Some(Err(error)),
                };
                let (type_name, bytes) = match method {
                    "sha256" => ("Digest256", runtime::jet_crypto_digest256_bytes_impl(
                        &runtime::jet_crypto_sha256_typed_impl(&input))),
                    "blake3" => ("Digest256", runtime::jet_crypto_digest256_bytes_impl(
                        &runtime::jet_crypto_blake3_typed_impl(&input))),
                    _ => ("Digest512", runtime::jet_crypto_digest512_bytes_impl(
                        &runtime::jet_crypto_sha512_typed_impl(&input))),
                };
                Some(Ok(CtValue::Struct {
                    type_name: type_name.to_string(),
                    fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
                }))
            }
            "__x25519_generate" => Some(crypto_result(
                runtime::jet_crypto_x25519_generate_impl(),
                |key| {
                    crypto_carrier(
                        "X25519SecretKey",
                        runtime::jet_crypto_expert_x25519_secret_bytes_impl(&key),
                    )
                },
            )),
            "__x25519_public" => {
                let [secret] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__x25519_public received malformed arguments",
                        span,
                    )));
                };
                let bytes = match vault_nominal_bytes(secret, "X25519SecretKey", span) {
                    Ok(bytes) => bytes,
                    Err(error) => return Some(Err(error)),
                };
                let secret = match runtime::x25519_secret_from_bytes(bytes) {
                    Ok(secret) => secret,
                    Err(error) => return Some(Err(vault_diag(error, span))),
                };
                let public = runtime::jet_crypto_x25519_public_typed_impl(&secret);
                Some(Ok(crypto_carrier(
                    "X25519PublicKey",
                    runtime::jet_crypto_x25519_public_bytes_impl(&public),
                )))
            }
            "__x25519_public_text" => {
                let [public] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__x25519_public_text received malformed arguments",
                        span,
                    )));
                };
                let bytes = match vault_nominal_bytes(public, "X25519PublicKey", span) {
                    Ok(bytes) => bytes,
                    Err(error) => return Some(Err(error)),
                };
                let public = match runtime::jet_crypto_x25519_public_from_bytes_impl(bytes) {
                    Ok(public) => public,
                    Err(error) => return Some(crypto_failed(error)),
                };
                Some(Ok(CtValue::Str(
                    runtime::jet_crypto_x25519_public_text_impl(&public),
                )))
            }
            "__x25519_public_from_text" => {
                let [CtValue::Str(text)] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__x25519_public_from_text received malformed arguments",
                        span,
                    )));
                };
                Some(crypto_result(
                    runtime::jet_crypto_x25519_public_from_text_impl(text.clone()),
                    |public| {
                        crypto_carrier(
                            "X25519PublicKey",
                            runtime::jet_crypto_x25519_public_bytes_impl(&public),
                        )
                    },
                ))
            }
            "__signing_generate" => Some(crypto_result(
                runtime::jet_crypto_signing_generate_impl(),
                |key| {
                    crypto_carrier(
                        "SigningKey",
                        runtime::jet_crypto_expert_signing_key_bytes_impl(&key),
                    )
                },
            )),
            "__signing_public" => {
                let [signing] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__signing_public received malformed arguments",
                        span,
                    )));
                };
                let bytes = match vault_nominal_bytes(signing, "SigningKey", span) {
                    Ok(bytes) => bytes,
                    Err(error) => return Some(Err(error)),
                };
                let signing = match runtime::signing_key_from_bytes(bytes) {
                    Ok(signing) => signing,
                    Err(error) => return Some(Err(vault_diag(error, span))),
                };
                let public = runtime::jet_crypto_signing_public_impl(&signing);
                Some(Ok(crypto_carrier(
                    "VerifyKey",
                    runtime::jet_crypto_verify_key_bytes_impl(&public),
                )))
            }
            "__secret_from_text" => {
                let [CtValue::Str(text)] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__secret_from_text received malformed arguments",
                        span,
                    )));
                };
                let secret = runtime::jet_crypto_secret_from_text_impl(text.clone());
                Some(Ok(crypto_carrier(
                    "Secret",
                    runtime::jet_crypto_expert_secret_bytes_impl(&secret),
                )))
            }
            "seal" => {
                let [recipients, plaintext, aad] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.seal received malformed arguments",
                        span,
                    )));
                };
                let recipients = match vault_public_keys(recipients, span) {
                    Ok(recipients) => recipients,
                    Err(error) => return Some(Err(error)),
                };
                let plaintext = match jet_as_bytes(plaintext, span) {
                    Ok(plaintext) => plaintext,
                    Err(error) => return Some(Err(error)),
                };
                let aad = match jet_as_bytes(aad, span) {
                    Ok(aad) => aad,
                    Err(error) => return Some(Err(error)),
                };
                Some(crypto_result(
                    runtime::jet_crypto_seal_typed_impl(recipients, &plaintext, &aad),
                    |sealed| {
                        crypto_carrier(
                            "Sealed",
                            runtime::jet_crypto_sealed_bytes_impl(&sealed),
                        )
                    },
                ))
            }
            "open" => {
                let [recipient, sealed, aad] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.open received malformed arguments",
                        span,
                    )));
                };
                let recipient = match vault_nominal_bytes(recipient, "X25519SecretKey", span) {
                    Ok(bytes) => match runtime::x25519_secret_from_bytes(bytes) {
                        Ok(recipient) => recipient,
                        Err(error) => return Some(Err(vault_diag(error, span))),
                    },
                    Err(error) => return Some(Err(error)),
                };
                let sealed = match vault_nominal_bytes(sealed, "Sealed", span) {
                    Ok(bytes) => match runtime::jet_crypto_sealed_from_bytes_impl(bytes) {
                        Ok(sealed) => sealed,
                        Err(error) => return Some(crypto_failed(error)),
                    },
                    Err(error) => return Some(Err(error)),
                };
                let aad = match jet_as_bytes(aad, span) {
                    Ok(aad) => aad,
                    Err(error) => return Some(Err(error)),
                };
                Some(crypto_result(
                    runtime::jet_crypto_open_typed_impl(&recipient, sealed, &aad),
                    CtValue::Bytes,
                ))
            }
            "sign" => {
                let [signing, message] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.sign received malformed arguments",
                        span,
                    )));
                };
                let signing = match vault_nominal_bytes(signing, "SigningKey", span) {
                    Ok(bytes) => match runtime::signing_key_from_bytes(bytes) {
                        Ok(signing) => signing,
                        Err(error) => return Some(Err(vault_diag(error, span))),
                    },
                    Err(error) => return Some(Err(error)),
                };
                let message = match jet_as_bytes(message, span) {
                    Ok(message) => message,
                    Err(error) => return Some(Err(error)),
                };
                Some(crypto_result(
                    runtime::jet_crypto_sign_typed_impl(&signing, &message),
                    |signature| {
                        crypto_carrier(
                            "Signature",
                            runtime::jet_crypto_signature_bytes_impl(&signature),
                        )
                    },
                ))
            }
            "verify" => {
                let [verify_key, message, signature] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.verify received malformed arguments",
                        span,
                    )));
                };
                let verify_key = match vault_nominal_bytes(verify_key, "VerifyKey", span) {
                    Ok(bytes) => match runtime::jet_crypto_verify_key_from_bytes_impl(bytes) {
                        Ok(verify_key) => verify_key,
                        Err(error) => return Some(crypto_failed(error)),
                    },
                    Err(error) => return Some(Err(error)),
                };
                let message = match jet_as_bytes(message, span) {
                    Ok(message) => message,
                    Err(error) => return Some(Err(error)),
                };
                let signature = match vault_nominal_bytes(signature, "Signature", span) {
                    Ok(bytes) => match runtime::jet_crypto_signature_from_bytes_impl(bytes) {
                        Ok(signature) => signature,
                        Err(error) => return Some(crypto_failed(error)),
                    },
                    Err(error) => return Some(Err(error)),
                };
                Some(crypto_result(
                    runtime::jet_crypto_verify_typed_impl(verify_key, &message, signature),
                    CtValue::Bool,
                ))
            }
            "password_hash" => {
                let [password] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.password_hash received malformed arguments",
                        span,
                    )));
                };
                let password = match vault_nominal_bytes(password, "Secret", span) {
                    Ok(bytes) => runtime::jet_crypto_secret_from_bytes_impl(bytes),
                    Err(error) => return Some(Err(error)),
                };
                Some(crypto_result(
                    runtime::jet_crypto_password_hash_typed_impl(&password),
                    |hash| crypto_password_carrier(runtime::jet_crypto_password_text_impl(&hash)),
                ))
            }
            "password_verify" => {
                let [password, stored] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.password_verify received malformed arguments",
                        span,
                    )));
                };
                let password = match vault_nominal_bytes(password, "Secret", span) {
                    Ok(bytes) => runtime::jet_crypto_secret_from_bytes_impl(bytes),
                    Err(error) => return Some(Err(error)),
                };
                let text = match vault_field(stored, "PasswordHash", "text") {
                    Some(CtValue::Str(text)) => text.clone(),
                    _ => {
                        return Some(Err(vault_diag(
                            "malformed PasswordHash value",
                            span,
                        )))
                    }
                };
                let stored = runtime::password_hash_from_text(text);
                Some(crypto_result(
                    runtime::jet_crypto_password_verify_typed_impl(&password, &stored),
                    CtValue::Bool,
                ))
            }
            "__digest256_hex" | "__digest512_hex" => {
                let [digest] = args.as_slice() else {
                    return Some(Err(vault_diag("malformed digest hexadecimal arguments", span)));
                };
                let type_name = if method == "__digest256_hex" { "Digest256" } else { "Digest512" };
                let bytes = match vault_nominal_bytes(digest, type_name, span) {
                    Ok(bytes) => bytes,
                    Err(error) => return Some(Err(error)),
                };
                let text = if method == "__digest256_hex" {
                    runtime::digest256_hex_from_bytes(&bytes)
                } else {
                    runtime::digest512_hex_from_bytes(&bytes)
                };
                Some(text.map(CtValue::Str).ok_or_else(|| {
                    vault_diag(format!("malformed {type_name} byte length"), span)
                }))
            }
            "__vault_wrapped_from_bytes" => {
                let [bytes] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__vault_wrapped_from_bytes received malformed arguments",
                        span,
                    )));
                };
                let bytes = match jet_as_bytes(bytes, span) {
                    Ok(bytes) => bytes,
                    Err(error) => return Some(Err(error)),
                };
                Some(vault_wrap_result(
                    runtime::jet_vault_wrapped_from_bytes_impl(bytes),
                    |wrapped| {
                        vault_carrier(
                            "WrappedVaultKey",
                            push(CryptoValue::WrappedVaultKey(wrapped)),
                        )
                    },
                ))
            }
            "__vault_wrapped_bytes" => {
                let [wrapped] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__vault_wrapped_bytes received malformed arguments",
                        span,
                    )));
                };
                let Some(handle) = vault_handle(wrapped, "WrappedVaultKey") else {
                    return Some(Err(vault_diag(
                        "core.crypto.__vault_wrapped_bytes received an invalid WrappedVaultKey",
                        span,
                    )));
                };
                let Some(bytes) = with_crypto(handle, |value| match value {
                    CryptoValue::WrappedVaultKey(wrapped) => {
                        Some(runtime::jet_vault_wrapped_bytes_impl(wrapped))
                    }
                    _ => None,
                }) else {
                    return Some(Err(vault_diag(
                        "core.crypto.__vault_wrapped_bytes received an invalid WrappedVaultKey",
                        span,
                    )));
                };
                Some(Ok(CtValue::Bytes(bytes)))
            }
            "__vault_unlock_recipient" => {
                let [identity] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__vault_unlock_recipient received malformed arguments",
                        span,
                    )));
                };
                let bytes = match vault_nominal_bytes(identity, "X25519SecretKey", span) {
                    Ok(bytes) => bytes,
                    Err(error) => return Some(Err(error)),
                };
                let identity = match runtime::x25519_secret_from_bytes(bytes) {
                    Ok(identity) => identity,
                    Err(error) => return Some(Err(vault_diag(error, span))),
                };
                let identity = push(CryptoValue::X25519SecretKey(identity));
                let unlock = push(CryptoValue::UnlockRecipient(identity));
                Some(Ok(vault_carrier("KeyUnlock", unlock)))
            }
            "__vault_unlock_passphrase" => {
                let [passphrase] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__vault_unlock_passphrase received malformed arguments",
                        span,
                    )));
                };
                let bytes = match vault_nominal_bytes(passphrase, "Secret", span) {
                    Ok(bytes) => bytes,
                    Err(error) => return Some(Err(error)),
                };
                let passphrase = runtime::jet_crypto_secret_from_bytes_impl(bytes);
                let passphrase = push(CryptoValue::Secret(passphrase));
                let unlock = push(CryptoValue::UnlockPassphrase(passphrase));
                Some(Ok(vault_carrier("KeyUnlock", unlock)))
            }
            _ => None,
        };
    }
    if module != "core.crypto.vault" {
        return None;
    }
    if method == "get" {
        let [CtValue::Str(name)] = args.as_slice() else {
            return Some(Err(vault_diag(
                "core.crypto.vault.get received malformed arguments",
                span,
            )));
        };
        return Some(Ok(
            runtime::jet_vault_get_impl(name)
                .map(|value| CtValue::Present(Box::new(CtValue::Str(value))))
                .unwrap_or_else(|| CtValue::absent(Type::String)),
        ));
    }
    let resolved_ret_ref = resolved_ret.as_ref();
    match method {
        "current" => {
            let [CtValue::Str(name)] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.current received malformed arguments",
                    span,
                )));
            };
            let tag = resolved_ret_ref
                .and_then(vault_tag_from_type)
                .unwrap_or(1);
            let Some(value) = vault_current_handle(name, tag) else {
                return Some(Err(vault_diag("invalid vault key type", span)));
            };
            Some(vault_current_result(value))
        }
        "versions" => {
            let [CtValue::Str(name)] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.versions received malformed arguments",
                    span,
                )));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let Some(value) = vault_versions_handles(name, tag) else {
                return Some(Err(vault_diag("invalid vault key type", span)));
            };
            Some(vault_result(value, |handles| {
                CtValue::List(
                    handles
                        .into_iter()
                        .map(vault_key_ref_value)
                        .collect(),
                )
            }))
        }
        "prepare_generate" | "prepare_rotate" => {
            let [CtValue::Str(name)] = args.as_slice() else {
                return Some(Err(vault_diag(
                    format!("core.crypto.vault.{method} received malformed arguments"),
                    span,
                )));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let value = if method == "prepare_generate" {
                vault_prepare_generate_handle(name, tag)
            } else {
                vault_prepare_rotate_handle(name, tag)
            };
            let Some(value) = value else {
                return Some(Err(vault_diag("invalid vault key type", span)));
            };
            Some(vault_result(value, |handle| {
                vault_carrier("MutationPlan", handle)
            }))
        }
        "prepare_store" => {
            let [CtValue::Str(name), key] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.prepare_store received malformed arguments",
                    span,
                )));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[1], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let bytes = match vault_nominal_bytes(
                key,
                if tag == 1 {
                    "SigningKey"
                } else {
                    "X25519SecretKey"
                },
                span,
            ) {
                Ok(bytes) => bytes,
                Err(error) => return Some(Err(error)),
            };
            let Some(value) = vault_prepare_store_handle(name, bytes, tag) else {
                return Some(Err(vault_diag("invalid vault key type", span)));
            };
            Some(vault_result(value, |handle| {
                vault_carrier("MutationPlan", handle)
            }))
        }
        "prepare_retire" | "prepare_revoke" => {
            let [key_ref, CtValue::Str(reason)] = args.as_slice() else {
                return Some(Err(vault_diag(
                    format!("core.crypto.vault.{method} received malformed arguments"),
                    span,
                )));
            };
            let Some(key_ref) = vault_handle(key_ref, "KeyRef") else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let Some(value) = vault_prepare_lifecycle_handle(
                key_ref,
                reason,
                tag,
                method == "prepare_revoke",
            ) else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            Some(vault_result(value, |handle| {
                vault_carrier("MutationPlan", handle)
            }))
        }
        "authorize_write" => {
            let [plan, CtValue::Str(reason)] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.authorize_write received malformed arguments",
                    span,
                )));
            };
            let Some(plan) = vault_handle(plan, "MutationPlan") else {
                return Some(Err(vault_diag("invalid mutation plan", span)));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let Some(value) = vault_authorize_write_handle(plan, reason, tag) else {
                return Some(Err(vault_diag("invalid mutation plan", span)));
            };
            Some(vault_result(value, |handle| {
                vault_carrier("VaultWrite", handle)
            }))
        }
        "commit_generate" | "commit_store" => {
            let [write, plan] = args.as_slice() else {
                return Some(Err(vault_diag(
                    format!("core.crypto.vault.{method} received malformed arguments"),
                    span,
                )));
            };
            let Some(write) = vault_handle(write, "VaultWrite") else {
                return Some(Err(vault_diag("invalid vault write", span)));
            };
            let Some(plan) = vault_handle(plan, "MutationPlan") else {
                return Some(Err(vault_diag("invalid mutation plan", span)));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0, 1], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let value = if method == "commit_generate" {
                vault_commit_generate_handles(write, plan, tag)
            } else {
                vault_commit_store_handles(write, plan, tag)
            };
            let Some(value) = value else {
                return Some(Err(vault_diag("invalid vault commit handles", span)));
            };
            Some(vault_result(value, |handle| vault_key_ref_value(handle)))
        }
        "commit_rotate" => {
            let [write, plan] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.commit_rotate received malformed arguments",
                    span,
                )));
            };
            let Some(write) = vault_handle(write, "VaultWrite") else {
                return Some(Err(vault_diag("invalid vault write", span)));
            };
            let Some(plan) = vault_handle(plan, "MutationPlan") else {
                return Some(Err(vault_diag("invalid mutation plan", span)));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0, 1], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let Some(value) = vault_commit_rotate_handles(write, plan, tag) else {
                return Some(Err(vault_diag("invalid vault commit handles", span)));
            };
            Some(vault_result(value, |(previous, current)| {
                vault_rotation_value(previous, current)
            }))
        }
        "commit_retire" | "commit_revoke" => {
            let [write, plan] = args.as_slice() else {
                return Some(Err(vault_diag(
                    format!("core.crypto.vault.{method} received malformed arguments"),
                    span,
                )));
            };
            let Some(write) = vault_handle(write, "VaultWrite") else {
                return Some(Err(vault_diag("invalid vault write", span)));
            };
            let Some(plan) = vault_handle(plan, "MutationPlan") else {
                return Some(Err(vault_diag("invalid mutation plan", span)));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0, 1], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let Some(value) = vault_commit_void_handles(
                write,
                plan,
                tag,
                method == "commit_revoke",
            ) else {
                return Some(Err(vault_diag("invalid vault commit handles", span)));
            };
            Some(vault_result(value, |_| CtValue::Unit))
        }
        "load" => {
            let [key_ref] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.load received malformed arguments",
                    span,
                )));
            };
            let Some(key_ref) = vault_handle(key_ref, "KeyRef") else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let value = match tag {
                1 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefSigning(key) => {
                        Some(runtime::jet_vault_load_impl(key))
                    }
                    _ => None,
                })
                .map(|value| value.map(|key| vault_loaded_value(1, key))),
                2 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefX25519(key) => {
                        Some(runtime::jet_vault_load_impl(key))
                    }
                    _ => None,
                })
                .map(|value| value.map(vault_loaded_x25519_value)),
                _ => None,
            };
            let Some(value) = value else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            Some(vault_result(value, |value| value))
        }
        "status" => {
            let [key_ref] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.status received malformed arguments",
                    span,
                )));
            };
            let Some(key_ref) = vault_handle(key_ref, "KeyRef") else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let value = match tag {
                1 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefSigning(key) => {
                        Some(runtime::jet_vault_status_impl(key))
                    }
                    _ => None,
                }),
                2 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefX25519(key) => {
                        Some(runtime::jet_vault_status_impl(key))
                    }
                    _ => None,
                }),
                _ => None,
            };
            let Some(value) = value else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            Some(vault_result(value, vault_status_value))
        }
        "export_to_recipients" => {
            let [key_ref, recipients] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.export_to_recipients received malformed arguments",
                    span,
                )));
            };
            let Some(key_ref) = vault_handle(key_ref, "KeyRef") else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            let recipients = match vault_public_keys(recipients, span) {
                Ok(recipients) => recipients,
                Err(error) => return Some(Err(error)),
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let value = match tag {
                1 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefSigning(key) => Some(
                        runtime::jet_vault_export_to_recipients_impl(key, &recipients),
                    ),
                    _ => None,
                }),
                2 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefX25519(key) => Some(
                        runtime::jet_vault_export_to_recipients_impl(key, &recipients),
                    ),
                    _ => None,
                }),
                _ => None,
            };
            let Some(value) = value else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            Some(vault_wrap_result(value, |wrapped| {
                vault_carrier(
                    "WrappedVaultKey",
                    push(CryptoValue::WrappedVaultKey(wrapped)),
                )
            }))
        }
        "export_to_passphrase" => {
            let [key_ref, passphrase] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.export_to_passphrase received malformed arguments",
                    span,
                )));
            };
            let Some(key_ref) = vault_handle(key_ref, "KeyRef") else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            let bytes = match vault_nominal_bytes(passphrase, "Secret", span) {
                Ok(bytes) => bytes,
                Err(error) => return Some(Err(error)),
            };
            let passphrase = runtime::jet_crypto_secret_from_bytes_impl(bytes);
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let value = match tag {
                1 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefSigning(key) => Some(
                        runtime::jet_vault_export_to_passphrase_impl(key, &passphrase),
                    ),
                    _ => None,
                }),
                2 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefX25519(key) => Some(
                        runtime::jet_vault_export_to_passphrase_impl(key, &passphrase),
                    ),
                    _ => None,
                }),
                _ => None,
            };
            let Some(value) = value else {
                return Some(Err(vault_diag("invalid vault key reference", span)));
            };
            Some(vault_wrap_result(value, |wrapped| {
                vault_carrier(
                    "WrappedVaultKey",
                    push(CryptoValue::WrappedVaultKey(wrapped)),
                )
            }))
        }
        "prepare_import_wrapped" => {
            let [CtValue::Str(name), wrapped, unlock] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.prepare_import_wrapped received malformed arguments",
                    span,
                )));
            };
            let Some(wrapped_handle) = vault_handle(wrapped, "WrappedVaultKey") else {
                return Some(Err(vault_diag("invalid wrapped vault key", span)));
            };
            let wrapped_tag = with_crypto(wrapped_handle, |value| match value {
                CryptoValue::WrappedVaultKey(wrapped) => Some(i64::from(wrapped.key_type())),
                _ => None,
            });
            let tag = match resolved_ret_ref
                .and_then(vault_tag_from_type)
                .or(wrapped_tag)
                .ok_or_else(|| vault_diag("vault wrapped plan has no checked key type", span))
            {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let wrapped = match take_crypto(wrapped_handle) {
                Some(CryptoValue::WrappedVaultKey(wrapped)) => wrapped,
                _ => return Some(Err(vault_diag("invalid wrapped vault key", span))),
            };
            let Some(unlock_handle) = vault_handle(unlock, "KeyUnlock") else {
                return Some(Err(vault_diag("invalid wrapped import unlock", span)));
            };
            let Some(unlock) = vault_unlock_from_handle(unlock_handle) else {
                return Some(Err(vault_diag("invalid wrapped import unlock", span)));
            };
            let value = match (tag, unlock) {
                (1, AmbientVaultUnlock::Recipient(identity)) => {
                    let unlock = runtime::JetVaultKeyUnlock::Recipient(&identity);
                    runtime::jet_vault_prepare_import_wrapped_impl(name, wrapped, unlock)
                        .map(|plan| push(CryptoValue::WrappedPlanSigning(plan)))
                }
                (1, AmbientVaultUnlock::Passphrase(passphrase)) => {
                    let unlock = runtime::JetVaultKeyUnlock::Passphrase(&passphrase);
                    runtime::jet_vault_prepare_import_wrapped_impl(name, wrapped, unlock)
                        .map(|plan| push(CryptoValue::WrappedPlanSigning(plan)))
                }
                (2, AmbientVaultUnlock::Recipient(identity)) => {
                    let unlock = runtime::JetVaultKeyUnlock::Recipient(&identity);
                    runtime::jet_vault_prepare_import_wrapped_impl(name, wrapped, unlock)
                        .map(|plan| push(CryptoValue::WrappedPlanX25519(plan)))
                }
                (2, AmbientVaultUnlock::Passphrase(passphrase)) => {
                    let unlock = runtime::JetVaultKeyUnlock::Passphrase(&passphrase);
                    runtime::jet_vault_prepare_import_wrapped_impl(name, wrapped, unlock)
                        .map(|plan| push(CryptoValue::WrappedPlanX25519(plan)))
                }
                _ => return Some(Err(vault_diag("invalid vault key type", span))),
            };
            Some(vault_wrap_result(value, |handle| {
                vault_carrier("WrappedImportPlan", handle)
            }))
        }
        "authorize_wrapped_import" => {
            let [plan, CtValue::Str(reason)] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.authorize_wrapped_import received malformed arguments",
                    span,
                )));
            };
            let Some(plan) = vault_handle(plan, "WrappedImportPlan") else {
                return Some(Err(vault_diag("invalid wrapped import plan", span)));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let Some(value) = vault_authorize_wrapped_handle(plan, reason, tag) else {
                return Some(Err(vault_diag("invalid wrapped import plan", span)));
            };
            Some(vault_wrap_result(value, |handle| {
                vault_carrier("VaultWrite", handle)
            }))
        }
        "commit_import_wrapped" => {
            let [write, plan] = args.as_slice() else {
                return Some(Err(vault_diag(
                    "core.crypto.vault.commit_import_wrapped received malformed arguments",
                    span,
                )));
            };
            let Some(write) = vault_handle(write, "VaultWrite") else {
                return Some(Err(vault_diag("invalid vault write", span)));
            };
            let Some(plan) = vault_handle(plan, "WrappedImportPlan") else {
                return Some(Err(vault_diag("invalid wrapped import plan", span)));
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0, 1], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error)),
            };
            let Some(value) = vault_commit_wrapped_handles(write, plan, tag) else {
                return Some(Err(vault_diag("invalid wrapped commit handles", span)));
            };
            Some(vault_wrap_result(value, |handle| vault_key_ref_value(handle)))
        }
        "prepare_import_signing" | "prepare_import_x25519" => {
            let [CtValue::Str(name), bytes] = args.as_slice() else {
                return Some(Err(vault_diag(
                    format!("core.crypto.vault.{method} received malformed arguments"),
                    span,
                )));
            };
            let bytes = match jet_as_bytes(bytes, span) {
                Ok(bytes) => bytes,
                Err(error) => return Some(Err(error)),
            };
            let value = if method == "prepare_import_signing" {
                vault_expert_prepare_import_signing_handle(name, bytes)
            } else {
                vault_expert_prepare_import_x25519_handle(name, bytes)
            };
            Some(vault_result(value, |handle| {
                vault_carrier("MutationPlan", handle)
            }))
        }
        "commit_import_signing" | "commit_import_x25519" => {
            let [write, plan] = args.as_slice() else {
                return Some(Err(vault_diag(
                    format!("core.crypto.vault.{method} received malformed arguments"),
                    span,
                )));
            };
            let Some(write) = vault_handle(write, "VaultWrite") else {
                return Some(Err(vault_diag("invalid vault write", span)));
            };
            let Some(plan) = vault_handle(plan, "MutationPlan") else {
                return Some(Err(vault_diag("invalid mutation plan", span)));
            };
            let value = if method == "commit_import_signing" {
                vault_expert_commit_import_signing_handles(write, plan)
            } else {
                vault_expert_commit_import_x25519_handles(write, plan)
            };
            let Some(value) = value else {
                return Some(Err(vault_diag("invalid expert commit handles", span)));
            };
            Some(vault_result(value, |handle| vault_key_ref_value(handle)))
        }
        _ => None,
    }
}

pub(crate) fn register_interpreter_ambient(
    context: &mut crate::InterpreterAmbientContext,
) {
    context.register_core_call(ambient_core_call);
}


fn jet_jit_vault_current(name: i64, tag: i64) -> i64 {
    let name = clone_string(name);
    match tag {
        1 => match runtime::jet_vault_current_impl::<runtime::JetSigningKey>(&name) {
            Ok(None) => result(true, 0),
            Ok(Some(key)) => result(true, (push(CryptoValue::KeyRefSigning(key)) + 1) as u64),
            Err(err) => err_debug(err),
        },
        2 => match runtime::jet_vault_current_impl::<runtime::JetX25519SecretKey>(&name) {
            Ok(None) => result(true, 0),
            Ok(Some(key)) => result(true, (push(CryptoValue::KeyRefX25519(key)) + 1) as u64),
            Err(err) => err_debug(err),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_prepare_generate(name: i64, tag: i64) -> i64 {
    let name = clone_string(name);
    match tag {
        1 => match runtime::jet_vault_prepare_generate_impl::<runtime::JetSigningKey>(&name) {
            Ok(plan) => result(true, push(CryptoValue::PlanSigning(plan)) as u64),
            Err(err) => err_debug(err),
        },
        2 => match runtime::jet_vault_prepare_generate_impl::<runtime::JetX25519SecretKey>(&name) {
            Ok(plan) => result(true, push(CryptoValue::PlanX25519(plan)) as u64),
            Err(err) => err_debug(err),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_authorize_write(plan: i64, reason: i64, tag: i64) -> i64 {
    let reason = clone_string(reason);
    match tag {
        1 => match with_crypto(plan, |value| match value {
            CryptoValue::PlanSigning(plan) => {
                Some(runtime::jet_vault_authorize_write_impl(plan, &reason))
            }
            _ => None,
        }) {
            Some(Ok(write)) => result(true, push(CryptoValue::WriteSigning(write)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid mutation plan handle".to_string()),
        },
        2 => match with_crypto(plan, |value| match value {
            CryptoValue::PlanX25519(plan) => {
                Some(runtime::jet_vault_authorize_write_impl(plan, &reason))
            }
            _ => None,
        }) {
            Some(Ok(write)) => result(true, push(CryptoValue::WriteX25519(write)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid mutation plan handle".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_commit_generate(write: i64, plan: i64, tag: i64) -> i64 {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
                match runtime::jet_vault_commit_generate_impl(write, plan) {
                    Ok(key) => result(true, push(CryptoValue::KeyRefSigning(key)) as u64),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid commit_generate handles".to_string()),
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                match runtime::jet_vault_commit_generate_impl(write, plan) {
                    Ok(key) => result(true, push(CryptoValue::KeyRefX25519(key)) as u64),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid commit_generate handles".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_prepare_rotate(name: i64, tag: i64) -> i64 {
    let name = clone_string(name);
    match tag {
        1 => match runtime::jet_vault_prepare_rotate_impl::<runtime::JetSigningKey>(&name) {
            Ok(plan) => result(true, push(CryptoValue::PlanSigning(plan)) as u64),
            Err(err) => err_debug(err),
        },
        2 => match runtime::jet_vault_prepare_rotate_impl::<runtime::JetX25519SecretKey>(&name) {
            Ok(plan) => result(true, push(CryptoValue::PlanX25519(plan)) as u64),
            Err(err) => err_debug(err),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_prepare_store(name: i64, key: i64, tag: i64) -> i64 {
    let name = clone_string(name);
    match tag {
        1 => match take_crypto(key) {
            Some(CryptoValue::SigningKey(key)) => {
                match runtime::jet_vault_prepare_store_impl(&name, key) {
                    Ok(plan) => result(true, push(CryptoValue::PlanSigning(plan)) as u64),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid store key handle".to_string()),
        },
        2 => match take_crypto(key) {
            Some(CryptoValue::X25519SecretKey(key)) => {
                match runtime::jet_vault_prepare_store_impl(&name, key) {
                    Ok(plan) => result(true, push(CryptoValue::PlanX25519(plan)) as u64),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid store key handle".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_prepare_retire(key_ref: i64, reason: i64, tag: i64) -> i64 {
    let reason = clone_string(reason);
    match tag {
        1 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefSigning(key) => {
                Some(runtime::jet_vault_prepare_retire_impl(key, &reason))
            }
            _ => None,
        }) {
            Some(Ok(plan)) => result(true, push(CryptoValue::PlanSigning(plan)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        2 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => {
                Some(runtime::jet_vault_prepare_retire_impl(key, &reason))
            }
            _ => None,
        }) {
            Some(Ok(plan)) => result(true, push(CryptoValue::PlanX25519(plan)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_prepare_revoke(key_ref: i64, reason: i64, tag: i64) -> i64 {
    let reason = clone_string(reason);
    match tag {
        1 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefSigning(key) => {
                Some(runtime::jet_vault_prepare_revoke_impl(key, &reason))
            }
            _ => None,
        }) {
            Some(Ok(plan)) => result(true, push(CryptoValue::PlanSigning(plan)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        2 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => {
                Some(runtime::jet_vault_prepare_revoke_impl(key, &reason))
            }
            _ => None,
        }) {
            Some(Ok(plan)) => result(true, push(CryptoValue::PlanX25519(plan)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_commit_store(write: i64, plan: i64, tag: i64) -> i64 {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
                match runtime::jet_vault_commit_store_impl(write, plan) {
                    Ok(key) => result(true, push(CryptoValue::KeyRefSigning(key)) as u64),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid vault commit handles".to_string()),
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                match runtime::jet_vault_commit_store_impl(write, plan) {
                    Ok(key) => result(true, push(CryptoValue::KeyRefX25519(key)) as u64),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid vault commit handles".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn rotation_record_signing(rotation: runtime::JetVaultRotation<runtime::JetSigningKey>) -> i64 {
    let previous = push(CryptoValue::KeyRefSigning(rotation.previous));
    let current = push(CryptoValue::KeyRefSigning(rotation.current));
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(record, 0, previous);
        let _ = rt.heap.record_set_int(record, 1, current);
        record
    })
}

fn rotation_record_x25519(rotation: runtime::JetVaultRotation<runtime::JetX25519SecretKey>) -> i64 {
    let previous = push(CryptoValue::KeyRefX25519(rotation.previous));
    let current = push(CryptoValue::KeyRefX25519(rotation.current));
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_int(record, 0, previous);
        let _ = rt.heap.record_set_int(record, 1, current);
        record
    })
}

fn jet_jit_vault_commit_rotate(write: i64, plan: i64, tag: i64) -> i64 {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
                match runtime::jet_vault_commit_rotate_impl(write, plan) {
                    Ok(rotation) => result(true, rotation_record_signing(rotation) as u64),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid vault commit handles".to_string()),
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                match runtime::jet_vault_commit_rotate_impl(write, plan) {
                    Ok(rotation) => result(true, rotation_record_x25519(rotation) as u64),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid vault commit handles".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_commit_retire(write: i64, plan: i64, tag: i64) -> i64 {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
                match runtime::jet_vault_commit_retire_impl(write, plan) {
                    Ok(()) => result(true, 0),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid vault commit handles".to_string()),
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                match runtime::jet_vault_commit_retire_impl(write, plan) {
                    Ok(()) => result(true, 0),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid vault commit handles".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_commit_revoke(write: i64, plan: i64, tag: i64) -> i64 {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
                match runtime::jet_vault_commit_revoke_impl(write, plan) {
                    Ok(()) => result(true, 0),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid vault commit handles".to_string()),
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                match runtime::jet_vault_commit_revoke_impl(write, plan) {
                    Ok(()) => result(true, 0),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid vault commit handles".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_load(key_ref: i64, tag: i64) -> i64 {
    match tag {
        1 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefSigning(key) => Some(runtime::jet_vault_load_impl(key)),
            _ => None,
        }) {
            Some(Ok(key)) => result(true, push(CryptoValue::SigningKey(key)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        2 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => Some(runtime::jet_vault_load_impl(key)),
            _ => None,
        }) {
            Some(Ok(key)) => result(true, push(CryptoValue::X25519SecretKey(key)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_status(key_ref: i64, tag: i64) -> i64 {
    let status = match tag {
        1 => with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefSigning(key) => Some(runtime::jet_vault_status_impl(key)),
            _ => None,
        }),
        2 => with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => Some(runtime::jet_vault_status_impl(key)),
            _ => None,
        }),
        _ => return error("invalid vault key tag".to_string()),
    };
    match status {
        Some(Ok(status)) => result(true, status as u8 as u64),
        Some(Err(err)) => err_debug(err),
        None => error("invalid key ref handle".to_string()),
    }
}

fn jet_jit_vault_versions(name: i64, tag: i64) -> i64 {
    let name = clone_string(name);
    match tag {
        1 => match runtime::jet_vault_versions_impl::<runtime::JetSigningKey>(&name) {
            Ok(versions) => {
                let list = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
                for key in versions {
                    let handle = push(CryptoValue::KeyRefSigning(key));
                    let _ = Concurrency::with_runtime_mut(|rt| rt.heap.list_push_int(list, handle));
                }
                result(true, list as u64)
            }
            Err(err) => err_debug(err),
        },
        2 => match runtime::jet_vault_versions_impl::<runtime::JetX25519SecretKey>(&name) {
            Ok(versions) => {
                let list = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
                for key in versions {
                    let handle = push(CryptoValue::KeyRefX25519(key));
                    let _ = Concurrency::with_runtime_mut(|rt| rt.heap.list_push_int(list, handle));
                }
                result(true, list as u64)
            }
            Err(err) => err_debug(err),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_export_to_recipients(key_ref: i64, recipients: i64, tag: i64) -> i64 {
    let Some(recipients) = public_keys(recipients) else {
        return error("invalid export recipients".to_string());
    };
    match tag {
        1 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefSigning(key) => Some(runtime::jet_vault_export_to_recipients_impl(
                key,
                &recipients,
            )),
            _ => None,
        }) {
            Some(Ok(wrapped)) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        2 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => Some(runtime::jet_vault_export_to_recipients_impl(
                key,
                &recipients,
            )),
            _ => None,
        }) {
            Some(Ok(wrapped)) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_export_to_passphrase(key_ref: i64, passphrase: i64, tag: i64) -> i64 {
    let passphrase = with_crypto(passphrase, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    });
    let Some(passphrase) = passphrase else {
        return error("invalid passphrase secret".to_string());
    };
    match tag {
        1 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefSigning(key) => Some(runtime::jet_vault_export_to_passphrase_impl(
                key,
                &passphrase,
            )),
            _ => None,
        }) {
            Some(Ok(wrapped)) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        2 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => Some(runtime::jet_vault_export_to_passphrase_impl(
                key,
                &passphrase,
            )),
            _ => None,
        }) {
            Some(Ok(wrapped)) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_wrapped_from_bytes(bytes: i64) -> i64 {
    match runtime::jet_vault_wrapped_from_bytes_impl(clone_bytes(bytes)) {
        Ok(wrapped) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
        Err(err) => err_debug(err),
    }
}

fn jet_jit_vault_wrapped_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::WrappedVaultKey(wrapped) => {
            Some(runtime::jet_vault_wrapped_bytes_impl(wrapped))
        }
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid wrapped vault key handle"));
            0
        }
    }
}

fn jet_jit_vault_unlock_recipient(identity: i64) -> i64 {
    push(CryptoValue::UnlockRecipient(identity))
}

fn jet_jit_vault_unlock_passphrase(passphrase: i64) -> i64 {
    push(CryptoValue::UnlockPassphrase(passphrase))
}

fn jet_jit_vault_prepare_import_wrapped(name: i64, wrapped: i64, unlock: i64, tag: i64) -> i64 {
    let name = clone_string(name);
    let wrapped = match take_crypto(wrapped) {
        Some(CryptoValue::WrappedVaultKey(wrapped)) => wrapped,
        _ => return error("invalid wrapped vault key handle".to_string()),
    };
    let unlock_kind = take_crypto(unlock);
    match (tag, unlock_kind) {
        (1, Some(CryptoValue::UnlockRecipient(identity))) => {
            let Some(identity) = with_crypto(identity, |value| match value {
                CryptoValue::X25519SecretKey(key) => Some(runtime::clone_x25519_secret(key)),
                _ => None,
            }) else {
                return error("invalid unlock identity".to_string());
            };
            let unlock = runtime::JetVaultKeyUnlock::Recipient(&identity);
            match runtime::jet_vault_prepare_import_wrapped_impl(&name, wrapped, unlock) {
                Ok(plan) => result(true, push(CryptoValue::WrappedPlanSigning(plan)) as u64),
                Err(err) => err_debug(err),
            }
        }
        (1, Some(CryptoValue::UnlockPassphrase(passphrase))) => {
            let Some(passphrase) = with_crypto(passphrase, |value| match value {
                CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
                _ => None,
            }) else {
                return error("invalid unlock passphrase".to_string());
            };
            let unlock = runtime::JetVaultKeyUnlock::Passphrase(&passphrase);
            match runtime::jet_vault_prepare_import_wrapped_impl(&name, wrapped, unlock) {
                Ok(plan) => result(true, push(CryptoValue::WrappedPlanSigning(plan)) as u64),
                Err(err) => err_debug(err),
            }
        }
        (2, Some(CryptoValue::UnlockRecipient(identity))) => {
            let Some(identity) = with_crypto(identity, |value| match value {
                CryptoValue::X25519SecretKey(key) => Some(runtime::clone_x25519_secret(key)),
                _ => None,
            }) else {
                return error("invalid unlock identity".to_string());
            };
            let unlock = runtime::JetVaultKeyUnlock::Recipient(&identity);
            match runtime::jet_vault_prepare_import_wrapped_impl(&name, wrapped, unlock) {
                Ok(plan) => result(true, push(CryptoValue::WrappedPlanX25519(plan)) as u64),
                Err(err) => err_debug(err),
            }
        }
        (2, Some(CryptoValue::UnlockPassphrase(passphrase))) => {
            let Some(passphrase) = with_crypto(passphrase, |value| match value {
                CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
                _ => None,
            }) else {
                return error("invalid unlock passphrase".to_string());
            };
            let unlock = runtime::JetVaultKeyUnlock::Passphrase(&passphrase);
            match runtime::jet_vault_prepare_import_wrapped_impl(&name, wrapped, unlock) {
                Ok(plan) => result(true, push(CryptoValue::WrappedPlanX25519(plan)) as u64),
                Err(err) => err_debug(err),
            }
        }
        _ => error("invalid wrapped import unlock".to_string()),
    }
}

fn jet_jit_vault_authorize_wrapped_import(plan: i64, reason: i64, tag: i64) -> i64 {
    let reason = clone_string(reason);
    match tag {
        1 => match with_crypto(plan, |value| match value {
            CryptoValue::WrappedPlanSigning(plan) => Some(
                runtime::jet_vault_authorize_wrapped_import_impl(plan, &reason),
            ),
            _ => None,
        }) {
            Some(Ok(write)) => result(true, push(CryptoValue::WriteSigning(write)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid wrapped import plan".to_string()),
        },
        2 => match with_crypto(plan, |value| match value {
            CryptoValue::WrappedPlanX25519(plan) => Some(
                runtime::jet_vault_authorize_wrapped_import_impl(plan, &reason),
            ),
            _ => None,
        }) {
            Some(Ok(write)) => result(true, push(CryptoValue::WriteX25519(write)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid wrapped import plan".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_commit_import_wrapped(write: i64, plan: i64, tag: i64) -> i64 {
    match tag {
        1 => match (take_crypto(write), take_crypto(plan)) {
            (
                Some(CryptoValue::WriteSigning(write)),
                Some(CryptoValue::WrappedPlanSigning(plan)),
            ) => match runtime::jet_vault_commit_import_wrapped_impl(write, plan) {
                Ok(key) => result(true, push(CryptoValue::KeyRefSigning(key)) as u64),
                Err(err) => err_debug(err),
            },
            _ => error("invalid commit_import_wrapped handles".to_string()),
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::WrappedPlanX25519(plan))) => {
                match runtime::jet_vault_commit_import_wrapped_impl(write, plan) {
                    Ok(key) => result(true, push(CryptoValue::KeyRefX25519(key)) as u64),
                    Err(err) => err_debug(err),
                }
            }
            _ => error("invalid commit_import_wrapped handles".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

fn jet_jit_vault_expert_prepare_import_signing(name: i64, bytes: i64) -> i64 {
    match runtime::jet_vault_expert_prepare_import_signing_impl(
        &clone_string(name),
        clone_bytes(bytes),
    ) {
        Ok(plan) => result(true, push(CryptoValue::PlanSigning(plan)) as u64),
        Err(err) => err_debug(err),
    }
}

fn jet_jit_vault_expert_commit_import_signing(write: i64, plan: i64) -> i64 {
    match (take_crypto(write), take_crypto(plan)) {
        (Some(CryptoValue::WriteSigning(write)), Some(CryptoValue::PlanSigning(plan))) => {
            match runtime::jet_vault_expert_commit_import_signing_impl(write, plan) {
                Ok(key) => result(true, push(CryptoValue::KeyRefSigning(key)) as u64),
                Err(err) => err_debug(err),
            }
        }
        _ => error("invalid expert import signing handles".to_string()),
    }
}
fn jet_jit_vault_expert_prepare_import_x25519(name: i64, bytes: i64) -> i64 {
    match runtime::jet_vault_expert_prepare_import_x25519_impl(
        &clone_string(name),
        clone_bytes(bytes),
    ) {
        Ok(plan) => result(true, push(CryptoValue::PlanX25519(plan)) as u64),
        Err(err) => err_debug(err),
    }
}

fn jet_jit_vault_expert_commit_import_x25519(write: i64, plan: i64) -> i64 {
    match (take_crypto(write), take_crypto(plan)) {
        (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
            match runtime::jet_vault_expert_commit_import_x25519_impl(write, plan) {
                Ok(key) => result(true, push(CryptoValue::KeyRefX25519(key)) as u64),
                Err(err) => err_debug(err),
            }
        }
        _ => error("invalid expert import X25519 handles".to_string()),
    }
}


host_fns! {
    struct CryptoHostFns;
    register: register_crypto_symbols;
    declare: declare_crypto_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut nullary = Signature::new(cc);
        nullary.returns.push(AbiParam::new(types::I64));
        let mut unary = nullary.clone();
        unary.params.push(AbiParam::new(types::I64));
        let mut binary = unary.clone();
        binary.params.push(AbiParam::new(types::I64));
        let mut ternary = binary.clone();
        ternary.params.push(AbiParam::new(types::I64));
        let mut quaternary = ternary.clone();
        quaternary.params.push(AbiParam::new(types::I64));
        let mut quinary = quaternary.clone();
        quinary.params.push(AbiParam::new(types::I64));
        let mut senary = quinary.clone();
        senary.params.push(AbiParam::new(types::I64));
        let mut septenary = senary.clone();
        septenary.params.push(AbiParam::new(types::I64));
        let mut octonary = septenary.clone();
        octonary.params.push(AbiParam::new(types::I64));
        let mut nonary = octonary.clone();
        nonary.params.push(AbiParam::new(types::I64));
        let mut denary = nonary.clone();
        denary.params.push(AbiParam::new(types::I64));


    }
    x25519_generate: "jet_jit_crypto_x25519_generate" => jet_jit_crypto_x25519_generate: nullary;
    x25519_public: "jet_jit_crypto_x25519_public" => jet_jit_crypto_x25519_public: unary;
    x25519: "jet_jit_crypto_x25519" => jet_jit_crypto_x25519: binary;
    signing_generate: "jet_jit_crypto_signing_generate" => jet_jit_crypto_signing_generate: nullary;
    signing_public: "jet_jit_crypto_signing_public" => jet_jit_crypto_signing_public: unary;
    sign: "jet_jit_crypto_sign" => jet_jit_crypto_sign: binary;
    verify: "jet_jit_crypto_verify" => jet_jit_crypto_verify: ternary;
    sha256: "jet_jit_crypto_sha256" => jet_jit_crypto_sha256: unary;
    blake3: "jet_jit_crypto_blake3" => jet_jit_crypto_blake3: unary;
    sha512: "jet_jit_crypto_sha512" => jet_jit_crypto_sha512: unary;
    sha1: "jet_jit_crypto_sha1" => jet_jit_crypto_sha1: unary;
    sha224: "jet_jit_crypto_sha224" => jet_jit_crypto_sha224: unary;
    sha384: "jet_jit_crypto_sha384" => jet_jit_crypto_sha384: unary;
    sha3_224: "jet_jit_crypto_sha3_224" => jet_jit_crypto_sha3_224: unary;
    sha3_256: "jet_jit_crypto_sha3_256" => jet_jit_crypto_sha3_256: unary;
    sha3_384: "jet_jit_crypto_sha3_384" => jet_jit_crypto_sha3_384: unary;
    sha3_512: "jet_jit_crypto_sha3_512" => jet_jit_crypto_sha3_512: unary;
    hmac_sha256: "jet_jit_crypto_hmac_sha256" => jet_jit_crypto_hmac_sha256: binary;
    pbkdf2_hmac: "jet_jit_crypto_pbkdf2_hmac" => jet_jit_crypto_pbkdf2_hmac: quaternary;
    hasher_new: "jet_jit_crypto_hasher_new" => jet_jit_crypto_hasher_new: nullary;
    hasher_update: "jet_jit_crypto_hasher_update" => jet_jit_crypto_hasher_update: binary;
    hasher_digest: "jet_jit_crypto_hasher_digest" => jet_jit_crypto_hasher_digest: unary;
    digest256_hex: "jet_jit_crypto_digest256_hex" => jet_jit_crypto_digest256_hex: unary;
    digest256_bytes: "jet_jit_crypto_digest256_bytes" => jet_jit_crypto_digest256_bytes: unary;
    digest512_hex: "jet_jit_crypto_digest512_hex" => jet_jit_crypto_digest512_hex: unary;
    digest512_bytes: "jet_jit_crypto_digest512_bytes" => jet_jit_crypto_digest512_bytes: unary;
    signature_bytes: "jet_jit_crypto_signature_bytes" => jet_jit_crypto_signature_bytes: unary;
    verify_key_bytes: "jet_jit_crypto_verify_key_bytes" => jet_jit_crypto_verify_key_bytes: unary;
    wrapped_bytes: "jet_jit_crypto_wrapped_bytes" => jet_jit_crypto_wrapped_bytes: unary;
    sealed_bytes: "jet_jit_crypto_sealed_bytes" => jet_jit_crypto_sealed_bytes: unary;
    x25519_public_bytes: "jet_jit_crypto_x25519_public_bytes" => jet_jit_crypto_x25519_public_bytes: unary;
    x25519_public_text: "jet_jit_crypto_x25519_public_text" => jet_jit_crypto_x25519_public_text: unary;
    x25519_public_from_text: "jet_jit_crypto_x25519_public_from_text" => jet_jit_crypto_x25519_public_from_text: unary;
    secret_from_text: "jet_jit_crypto_secret_from_text" => jet_jit_crypto_secret_from_text: unary;
    random_bytes: "jet_jit_crypto_random_bytes" => jet_jit_crypto_random_bytes: unary;
    seal: "jet_jit_crypto_seal" => jet_jit_crypto_seal: ternary;
    wrap: "jet_jit_crypto_wrap" => jet_jit_crypto_wrap: binary;
    unwrap: "jet_jit_crypto_unwrap" => jet_jit_crypto_unwrap: binary;
    open: "jet_jit_crypto_open" => jet_jit_crypto_open: ternary;
    password_hash: "jet_jit_crypto_password_hash" => jet_jit_crypto_password_hash: unary;
    password_verify: "jet_jit_crypto_password_verify" => jet_jit_crypto_password_verify: binary;
    password_text: "jet_jit_crypto_password_text" => jet_jit_crypto_password_text: unary;
    file_open: "jet_jit_crypto_file_open" => jet_jit_crypto_file_open: ternary;
    secret_from_bytes: "jet_jit_crypto_secret_from_bytes" => jet_jit_crypto_secret_from_bytes: unary;
    hkdf_sha256: "jet_jit_crypto_hkdf_sha256" => jet_jit_crypto_hkdf_sha256: quaternary;
    x25519_public_from_bytes: "jet_jit_crypto_x25519_public_from_bytes" => jet_jit_crypto_x25519_public_bytes_raw: unary;
    x25519_public_typed_from_bytes: "jet_jit_crypto_x25519_public_from_bytes_typed" => jet_jit_crypto_x25519_public_from_bytes_typed: unary;
    x25519_shared: "jet_jit_crypto_x25519_shared" => jet_jit_crypto_x25519_shared: binary;
    constant_time_equal: "jet_jit_crypto_constant_time_equal" => jet_jit_crypto_constant_time_equal: binary;
    constant_time_equal_bytes: "jet_jit_crypto_constant_time_equal_bytes" => jet_jit_crypto_constant_time_equal_bytes: binary;
    file_seal: "jet_jit_crypto_file_seal" => jet_jit_crypto_file_seal: ternary;
    expert_aes256gcm_seal: "jet_jit_crypto_expert_aes256gcm_seal" => jet_jit_crypto_expert_aes256gcm_seal: quaternary;
    expert_aes256gcm_open: "jet_jit_crypto_expert_aes256gcm_open" => jet_jit_crypto_expert_aes256gcm_open: quaternary;
    expert_xchacha20poly1305_seal: "jet_jit_crypto_expert_xchacha20poly1305_seal" => jet_jit_crypto_expert_xchacha20poly1305_seal: quaternary;
    expert_xchacha20poly1305_open: "jet_jit_crypto_expert_xchacha20poly1305_open" => jet_jit_crypto_expert_xchacha20poly1305_open: quaternary;
    expert_ed25519_sign: "jet_jit_crypto_expert_ed25519_sign" => jet_jit_crypto_expert_ed25519_sign: binary;
    expert_ed25519_verify: "jet_jit_crypto_expert_ed25519_verify" => jet_jit_crypto_expert_ed25519_verify: ternary;
    expert_argon2id: "jet_jit_crypto_expert_argon2id" => jet_jit_crypto_expert_argon2id: senary;
    expert_signing_key_bytes: "jet_jit_crypto_expert_signing_key_bytes" => jet_jit_crypto_expert_signing_key_bytes: unary;
    expert_x25519_secret_bytes: "jet_jit_crypto_expert_x25519_secret_bytes" => jet_jit_crypto_expert_x25519_secret_bytes: unary;
    expert_open_v1: "jet_jit_crypto_expert_open_v1" => jet_jit_crypto_expert_open_v1: binary;
    expert_migrate_v1: "jet_jit_crypto_expert_migrate_v1" => jet_jit_crypto_expert_migrate_v1: quaternary;
    expert_x25519: "jet_jit_crypto_expert_x25519" => jet_jit_crypto_expert_x25519: ternary;
    expert_hkdf_sha256: "jet_jit_crypto_expert_hkdf_sha256" => jet_jit_crypto_expert_hkdf_sha256: quaternary;
    expert_secret_bytes: "jet_jit_crypto_expert_secret_bytes" => jet_jit_crypto_expert_secret_bytes: unary;
    verify_jwt: "jet_jit_auth_verify_jwt" => jet_jit_auth_verify_jwt: senary;
    verify_paseto: "jet_jit_auth_verify_paseto" => jet_jit_auth_verify_paseto: denary;
    auth_register_user: "jet_jit_auth_register_user" => jet_jit_auth_register_user: binary;
    auth_password_login: "jet_jit_auth_password_login" => jet_jit_auth_password_login: quaternary;
    auth_session_validate: "jet_jit_auth_session_validate" => jet_jit_auth_session_validate: binary;
    auth_magic_link_issue: "jet_jit_auth_magic_link_issue" => jet_jit_auth_magic_link_issue: ternary;
    auth_magic_link_consume: "jet_jit_auth_magic_link_consume" => jet_jit_auth_magic_link_consume: ternary;
    auth_oauth_begin: "jet_jit_auth_oauth_begin" => jet_jit_auth_oauth_begin: unary;
    auth_oauth_finish: "jet_jit_auth_oauth_finish" => jet_jit_auth_oauth_finish: quaternary;
    auth_session_show: "jet_jit_auth_session_show" => jet_jit_auth_session_show: unary;
    auth_session_user: "jet_jit_auth_session_user" => jet_jit_auth_session_user: unary;
    auth_session_cookie: "jet_jit_auth_session_cookie" => jet_jit_auth_session_cookie: unary;
    auth_session_id: "jet_jit_auth_session_id" => jet_jit_auth_session_id: unary;
    app_auth: "jet_jit_app_auth" => jet_jit_app_auth: unary;
    app_auth_oauth: "jet_jit_app_auth_oauth" => jet_jit_app_auth_oauth: binary;
    app_auth_routes: "jet_jit_app_auth_routes" => jet_jit_app_auth_routes: unary;
    app_auth_show: "jet_jit_app_auth_show" => jet_jit_app_auth_show: unary;
    // Canonical `core.web`/`app` `auth_routes`/`auth_show` rows resolve by the
    // exact symbol the row declares; the `jet_jit_*` spellings above stay for
    // Core rows projected through `CoreCallRecord::jit_symbol_candidates`.
    row_app_auth_routes: "jet_app_auth_routes" => jet_jit_app_auth_routes: unary;
    row_app_auth_show: "jet_app_auth_show" => jet_jit_app_auth_show: unary;
    vault_get: "jet_jit_vault_get" => jet_jit_vault_get: unary;
    vault_key_ref_show: "jet_jit_vault_key_ref_show" => jet_jit_vault_key_ref_show: unary;
    vault_current: "jet_jit_vault_current" => jet_jit_vault_current: binary;
    vault_prepare_generate: "jet_jit_vault_prepare_generate" => jet_jit_vault_prepare_generate: binary;
    vault_prepare_rotate: "jet_jit_vault_prepare_rotate" => jet_jit_vault_prepare_rotate: binary;
    vault_prepare_store: "jet_jit_vault_prepare_store" => jet_jit_vault_prepare_store: ternary;
    vault_prepare_retire: "jet_jit_vault_prepare_retire" => jet_jit_vault_prepare_retire: ternary;
    vault_prepare_revoke: "jet_jit_vault_prepare_revoke" => jet_jit_vault_prepare_revoke: ternary;
    vault_authorize_write: "jet_jit_vault_authorize_write" => jet_jit_vault_authorize_write: ternary;
    vault_commit_generate: "jet_jit_vault_commit_generate" => jet_jit_vault_commit_generate: ternary;
    vault_commit_store: "jet_jit_vault_commit_store" => jet_jit_vault_commit_store: ternary;
    vault_commit_rotate: "jet_jit_vault_commit_rotate" => jet_jit_vault_commit_rotate: ternary;
    vault_commit_retire: "jet_jit_vault_commit_retire" => jet_jit_vault_commit_retire: ternary;
    vault_commit_revoke: "jet_jit_vault_commit_revoke" => jet_jit_vault_commit_revoke: ternary;
    vault_load: "jet_jit_vault_load" => jet_jit_vault_load: binary;
    vault_status: "jet_jit_vault_status" => jet_jit_vault_status: binary;
    vault_versions: "jet_jit_vault_versions" => jet_jit_vault_versions: binary;
    vault_export_to_recipients: "jet_jit_vault_export_to_recipients" => jet_jit_vault_export_to_recipients: ternary;
    vault_export_to_passphrase: "jet_jit_vault_export_to_passphrase" => jet_jit_vault_export_to_passphrase: ternary;
    vault_wrapped_from_bytes: "jet_jit_vault_wrapped_from_bytes" => jet_jit_vault_wrapped_from_bytes: unary;
    vault_wrapped_bytes: "jet_jit_vault_wrapped_bytes" => jet_jit_vault_wrapped_bytes: unary;
    vault_unlock_recipient: "jet_jit_vault_unlock_recipient" => jet_jit_vault_unlock_recipient: unary;
    vault_unlock_passphrase: "jet_jit_vault_unlock_passphrase" => jet_jit_vault_unlock_passphrase: unary;
    vault_prepare_import_wrapped: "jet_jit_vault_prepare_import_wrapped" => jet_jit_vault_prepare_import_wrapped: quaternary;
    vault_authorize_wrapped_import: "jet_jit_vault_authorize_wrapped_import" => jet_jit_vault_authorize_wrapped_import: ternary;
    vault_commit_import_wrapped: "jet_jit_vault_commit_import_wrapped" => jet_jit_vault_commit_import_wrapped: ternary;
    vault_expert_prepare_import_signing: "jet_jit_vault_expert_prepare_import_signing" => jet_jit_vault_expert_prepare_import_signing: binary;
    vault_expert_commit_import_signing: "jet_jit_vault_expert_commit_import_signing" => jet_jit_vault_expert_commit_import_signing: binary;
    vault_expert_prepare_import_x25519: "jet_jit_vault_expert_prepare_import_x25519" => jet_jit_vault_expert_prepare_import_x25519: binary;
    vault_expert_commit_import_x25519: "jet_jit_vault_expert_commit_import_x25519" => jet_jit_vault_expert_commit_import_x25519: binary;
}
