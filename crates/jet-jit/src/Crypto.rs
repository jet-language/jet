//! Native JIT adapters for crypto, authentication, and vault calls.
//!
//! Algorithms come from the same source files emitted into AOT bridge crates.

use super::Concurrency;
use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_foundation::AST::Type;
use crate::Marshal::clone_string;

pub(crate) mod runtime {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs");
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/CryptoEntropy.rs");
    include!("../../jet-pkg-model/src/Prelude/Crypto.rs");
    include!("../../jet-pkg-model/src/Prelude/VaultNfc.rs");
    include!("../../jet-pkg-model/src/Prelude/SecretsCrypto.rs");
    include!("../../jet-pkg-model/src/Prelude/VaultKeyWrap.rs");

    use crate::Encoding::json_rt as jet_std;
    fn jet_sha256_raw(data: &[u8]) -> [u8; 32] {
        jet_crypto_email_sha256_impl(data)
    }
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/Auth.rs");

    pub fn auth_verify_jwt(
        token: &String,
        key: &Vec<u8>,
        audience: &String,
        issuer: Option<&String>,
        clock_skew_ms: i64,
    ) -> Result<JetAuthClaims, JetAuthError> {
        jet_auth_verify_jwt_impl(token, key, audience, issuer, clock_skew_ms)
    }

    pub fn auth_verify_paseto(
        token: &String,
        key: &Vec<u8>,
        audience: &String,
        issuer: Option<&String>,
        clock_skew_ms: i64,
        footer: &Vec<u8>,
        implicit: &Vec<u8>,
    ) -> Result<JetAuthClaims, JetAuthError> {
        jet_auth_verify_paseto_impl(
            token,
            key,
            audience,
            issuer,
            clock_skew_ms,
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
}

/// Interpreter ambient: rebuild X25519SecretKey without a Cranelift heap.
pub(crate) fn x25519_secret_from_vec(bytes: Vec<u8>) -> Result<runtime::JetX25519SecretKey, String> {
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
    Sealed(runtime::JetSealed),
    Secret(runtime::Secret),
    PasswordHash(runtime::JetPasswordHash),
    SharedSecret(runtime::JetSharedSecret),
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
        let record = rt.heap.alloc_record(5);
        let subject = claims
            .subject
            .map(|value| rt.heap.alloc_string(value) + 1)
            .unwrap_or(0);
        let audience = rt.heap.alloc_string(claims.audience);
        let issuer = claims
            .issuer
            .map(|value| rt.heap.alloc_string(value) + 1)
            .unwrap_or(0);
        let _ = rt.heap.record_set_int(record, 0, subject);
        let _ = rt.heap.record_set_string(record, 1, audience);
        let _ = rt.heap.record_set_int(record, 2, issuer);
        let _ = rt.heap.record_set_int(record, 3, claims.expires_at);
        let _ = rt
            .heap
            .record_set_int(record, 4, claims.issued_at.map(|value| value + 1).unwrap_or(0));
        record
    })
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
        rt.crypto_values.get(index).and_then(|slot| slot.as_ref()).and_then(f)
    })
}

/// Snapshot closed-family secret material for `ExpiringSecret` zeroize-on-expiry.
/// Keeps the crypto handle live so `with` can loan it until expiry drops it.
pub(crate) fn claim_expiring_secret(handle: i64) -> Option<crate::Memory::SecretState> {
    let bytes = with_crypto(handle, |value| match value {
        CryptoValue::SigningKey(key) => Some(runtime::jet_crypto_expert_signing_key_bytes_impl(key)),
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

pub(crate) fn vault_key_tag(ty: &Type) -> Option<i64> {
    match ty {
        Type::Named(name) if name == "SigningKey" => Some(1),
        Type::Named(name) if name == "X25519SecretKey" => Some(2),
        Type::Tagged { inner, .. } => vault_key_tag(inner),
        Type::Apply { args, .. } => args.iter().find_map(vault_key_tag),
        Type::Option(inner) | Type::List(inner) => vault_key_tag(inner),
        Type::Result { ok, .. } => vault_key_tag(ok),
        _ => None,
    }
}

extern "C" fn jet_jit_crypto_x25519_generate() -> i64 {
    match runtime::jet_crypto_x25519_generate_impl() {
        Ok(key) => result(true, push(CryptoValue::X25519SecretKey(key)) as u64),
        Err(err) => error(err.to_string()),
    }
}

extern "C" fn jet_jit_crypto_x25519_public(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::X25519SecretKey(key) => Some(runtime::jet_crypto_x25519_public_typed_impl(key)),
        _ => None,
    }) {
        Some(public) => push(CryptoValue::X25519PublicKey(public)),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid X25519 secret key handle"));
            0
        }
    }
}

extern "C" fn jet_jit_crypto_signing_generate() -> i64 {
    match runtime::jet_crypto_signing_generate_impl() {
        Ok(key) => result(true, push(CryptoValue::SigningKey(key)) as u64),
        Err(err) => error(err.to_string()),
    }
}

extern "C" fn jet_jit_crypto_signing_public(handle: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_sign(key_handle: i64, message_handle: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_verify(
    key_handle: i64,
    message_handle: i64,
    signature_handle: i64,
) -> i64 {
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

extern "C" fn jet_jit_crypto_sha256(data_handle: i64) -> i64 {
    let digest = runtime::jet_crypto_sha256_typed_impl(&clone_bytes(data_handle));
    push(CryptoValue::Digest256(digest))
}

extern "C" fn jet_jit_crypto_sha512_bytes(data_handle: i64) -> i64 {
    let text = runtime::jet_crypto_sha512_impl(&clone_bytes(data_handle));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

extern "C" fn jet_jit_crypto_blake3_bytes(data_handle: i64) -> i64 {
    let text = runtime::jet_crypto_blake3_impl(&clone_bytes(data_handle));
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

extern "C" fn jet_jit_crypto_digest256_hex(handle: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_digest256_bytes(handle: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_signature_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::Signature(signature) => Some(runtime::jet_crypto_signature_bytes_impl(signature)),
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid signature handle"));
            0
        }
    }
}

extern "C" fn jet_jit_crypto_sealed_bytes(handle: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_x25519_public_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::X25519PublicKey(key) => Some(runtime::jet_crypto_x25519_public_bytes_impl(key)),
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid X25519 public key handle"));
            0
        }
    }
}

extern "C" fn jet_jit_crypto_x25519_public_text(handle: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_x25519_public_from_text(text: i64) -> i64 {
    match runtime::jet_crypto_x25519_public_from_text_impl(clone_string(text)) {
        Ok(key) => result(true, push(CryptoValue::X25519PublicKey(key)) as u64),
        Err(err) => error(err.to_string()),
    }
}

extern "C" fn jet_jit_crypto_secret_from_text(text: i64) -> i64 {
    push(CryptoValue::Secret(runtime::jet_crypto_secret_from_text_impl(clone_string(text))))
}

extern "C" fn jet_jit_crypto_random_bytes(count: i64) -> i64 {
    match runtime::jet_crypto_entropy_bytes(count) {
        Ok(bytes) => alloc_bytes(&bytes),
        Err(err) => {
            eprintln!("Error [E3001]: panic: core.crypto.random.bytes: {err}");
            std::process::exit(70);
        }
    }
}

extern "C" fn jet_jit_crypto_seal(recipients: i64, plaintext: i64, aad: i64) -> i64 {
    let Some(recipients) = public_keys(recipients) else {
        return error("invalid seal recipient list".to_string());
    };
    match runtime::jet_crypto_seal_typed_impl(recipients, &clone_bytes(plaintext), &clone_bytes(aad)) {
        Ok(sealed) => result(true, push(CryptoValue::Sealed(sealed)) as u64),
        Err(err) => error(err.to_string()),
    }
}

extern "C" fn jet_jit_crypto_open(recipient: i64, sealed: i64, aad: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_password_hash(password: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_password_verify(password: i64, stored: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_password_text(handle: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_file_open(recipient: i64, source: i64, dest: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_secret_from_bytes(bytes: i64) -> i64 {
    push(CryptoValue::Secret(runtime::jet_crypto_secret_from_bytes_impl(
        clone_bytes(bytes),
    )))
}

extern "C" fn jet_jit_crypto_hkdf_sha256(ikm: i64, salt: i64, info: i64, length: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_x25519_public_bytes_raw(secret: i64) -> i64 {
    match runtime::jet_crypto_x25519_public_impl(&clone_bytes(secret)) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err),
    }
}

extern "C" fn jet_jit_crypto_x25519_shared(secret: i64, public: i64) -> i64 {
    match runtime::jet_crypto_x25519_shared_impl(&clone_bytes(secret), &clone_bytes(public)) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err),
    }
}

extern "C" fn jet_jit_crypto_constant_time_equal(a: i64, b: i64) -> i64 {
    let left = with_crypto(a, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    });
    let right = with_crypto(b, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    });
    match (left, right) {
        (Some(a), Some(b)) => {
            i64::from(runtime::jet_crypto_constant_time_secret_impl(&a, &b))
        }
        _ => 0,
    }
}

extern "C" fn jet_jit_crypto_constant_time_equal_bytes(a: i64, b: i64) -> i64 {
    i64::from(runtime::jet_crypto_constant_time_equal_bytes_impl(
        &clone_bytes(a),
        &clone_bytes(b),
    ))
}

extern "C" fn jet_jit_crypto_file_seal(recipients: i64, source: i64, dest: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_expert_aes256gcm_seal(key: i64, nonce: i64, plaintext: i64, aad: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_expert_aes256gcm_open(key: i64, nonce: i64, ciphertext: i64, aad: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_expert_open_v1(key: i64, blob: i64) -> i64 {
    match runtime::jet_crypto_expert_open_v1_impl(&clone_bytes(key), &clone_bytes(blob)) {
        Ok(bytes) => result(true, alloc_bytes(&bytes) as u64),
        Err(err) => error(err.to_string()),
    }
}

extern "C" fn jet_jit_crypto_expert_migrate_v1(key: i64, source: i64, recipients: i64, dest: i64) -> i64 {
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

extern "C" fn jet_jit_crypto_expert_x25519(secret: i64, public: i64, reject_all_zero: i64) -> i64 {
    match runtime::jet_crypto_expert_x25519_impl(
        &clone_bytes(secret),
        &clone_bytes(public),
        reject_all_zero != 0,
    ) {
        Ok(secret) => result(true, push(CryptoValue::Secret(secret)) as u64),
        Err(err) => error(err.to_string()),
    }
}

extern "C" fn jet_jit_crypto_expert_secret_bytes(secret: i64) -> i64 {
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

extern "C" fn jet_jit_auth_verify_jwt(
    token: i64,
    key: i64,
    audience: i64,
    issuer: i64,
    skew_ms: i64,
) -> i64 {
    let token = clone_string(token);
    let key = clone_bytes(key);
    let audience = clone_string(audience);
    let issuer = (issuer != 0).then(|| clone_string(issuer - 1));
    match runtime::auth_verify_jwt(&token, &key, &audience, issuer.as_ref(), skew_ms) {
        Ok(claims) => result(true, claims_record(claims) as u64),
        Err(err) => err_debug(err),
    }
}

extern "C" fn jet_jit_auth_verify_paseto(
    token: i64,
    key: i64,
    audience: i64,
    issuer: i64,
    skew_ms: i64,
    footer: i64,
    implicit: i64,
) -> i64 {
    let token = clone_string(token);
    let key = clone_bytes(key);
    let audience = clone_string(audience);
    let issuer = (issuer != 0).then(|| clone_string(issuer - 1));
    match runtime::auth_verify_paseto(
        &token,
        &key,
        &audience,
        issuer.as_ref(),
        skew_ms,
        &clone_bytes(footer),
        &clone_bytes(implicit),
    ) {
        Ok(claims) => result(true, claims_record(claims) as u64),
        Err(err) => err_debug(err),
    }
}

extern "C" fn jet_jit_vault_get(name: i64) -> i64 {
    match runtime::jet_vault_get_impl(&clone_string(name)) {
        None => 0,
        Some(value) => Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(value) + 1),
    }
}

extern "C" fn jet_jit_vault_key_ref_show(handle: i64) -> i64 {
    let text = with_crypto(handle, |value| match value {
        CryptoValue::KeyRefSigning(key) => Some(key.to_string()),
        CryptoValue::KeyRefX25519(key) => Some(key.to_string()),
        _ => None,
    })
    .unwrap_or_else(|| "<invalid KeyRef>".to_string());
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(text))
}

extern "C" fn jet_jit_vault_current(name: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_prepare_generate(name: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_authorize_write(plan: i64, reason: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_commit_generate(write: i64, plan: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_prepare_rotate(name: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_prepare_store(name: i64, key: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_prepare_retire(key_ref: i64, reason: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_prepare_revoke(key_ref: i64, reason: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_commit_store(write: i64, plan: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_commit_rotate(write: i64, plan: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_commit_retire(write: i64, plan: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_commit_revoke(write: i64, plan: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_load(key_ref: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_status(key_ref: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_versions(name: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_export_to_recipients(key_ref: i64, recipients: i64, tag: i64) -> i64 {
    let Some(recipients) = public_keys(recipients) else {
        return error("invalid export recipients".to_string());
    };
    match tag {
        1 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefSigning(key) => {
                Some(runtime::jet_vault_export_to_recipients_impl(key, &recipients))
            }
            _ => None,
        }) {
            Some(Ok(wrapped)) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        2 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => {
                Some(runtime::jet_vault_export_to_recipients_impl(key, &recipients))
            }
            _ => None,
        }) {
            Some(Ok(wrapped)) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

extern "C" fn jet_jit_vault_export_to_passphrase(key_ref: i64, passphrase: i64, tag: i64) -> i64 {
    let passphrase = with_crypto(passphrase, |value| match value {
        CryptoValue::Secret(secret) => Some(runtime::clone_secret(secret)),
        _ => None,
    });
    let Some(passphrase) = passphrase else {
        return error("invalid passphrase secret".to_string());
    };
    match tag {
        1 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefSigning(key) => {
                Some(runtime::jet_vault_export_to_passphrase_impl(key, &passphrase))
            }
            _ => None,
        }) {
            Some(Ok(wrapped)) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        2 => match with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => {
                Some(runtime::jet_vault_export_to_passphrase_impl(key, &passphrase))
            }
            _ => None,
        }) {
            Some(Ok(wrapped)) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid key ref handle".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

extern "C" fn jet_jit_vault_wrapped_from_bytes(bytes: i64) -> i64 {
    match runtime::jet_vault_wrapped_from_bytes_impl(clone_bytes(bytes)) {
        Ok(wrapped) => result(true, push(CryptoValue::WrappedVaultKey(wrapped)) as u64),
        Err(err) => err_debug(err),
    }
}

extern "C" fn jet_jit_vault_wrapped_bytes(handle: i64) -> i64 {
    match with_crypto(handle, |value| match value {
        CryptoValue::WrappedVaultKey(wrapped) => Some(runtime::jet_vault_wrapped_bytes_impl(wrapped)),
        _ => None,
    }) {
        Some(bytes) => alloc_bytes(&bytes),
        None => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap("invalid wrapped vault key handle"));
            0
        }
    }
}

extern "C" fn jet_jit_vault_unlock_recipient(identity: i64) -> i64 {
    push(CryptoValue::UnlockRecipient(identity))
}

extern "C" fn jet_jit_vault_unlock_passphrase(passphrase: i64) -> i64 {
    push(CryptoValue::UnlockPassphrase(passphrase))
}

extern "C" fn jet_jit_vault_prepare_import_wrapped(name: i64, wrapped: i64, unlock: i64, tag: i64) -> i64 {
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

extern "C" fn jet_jit_vault_authorize_wrapped_import(plan: i64, reason: i64, tag: i64) -> i64 {
    let reason = clone_string(reason);
    match tag {
        1 => match with_crypto(plan, |value| match value {
            CryptoValue::WrappedPlanSigning(plan) => {
                Some(runtime::jet_vault_authorize_wrapped_import_impl(plan, &reason))
            }
            _ => None,
        }) {
            Some(Ok(write)) => result(true, push(CryptoValue::WriteSigning(write)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid wrapped import plan".to_string()),
        },
        2 => match with_crypto(plan, |value| match value {
            CryptoValue::WrappedPlanX25519(plan) => {
                Some(runtime::jet_vault_authorize_wrapped_import_impl(plan, &reason))
            }
            _ => None,
        }) {
            Some(Ok(write)) => result(true, push(CryptoValue::WriteX25519(write)) as u64),
            Some(Err(err)) => err_debug(err),
            None => error("invalid wrapped import plan".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

extern "C" fn jet_jit_vault_commit_import_wrapped(write: i64, plan: i64, tag: i64) -> i64 {
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
            (
                Some(CryptoValue::WriteX25519(write)),
                Some(CryptoValue::WrappedPlanX25519(plan)),
            ) => match runtime::jet_vault_commit_import_wrapped_impl(write, plan) {
                Ok(key) => result(true, push(CryptoValue::KeyRefX25519(key)) as u64),
                Err(err) => err_debug(err),
            },
            _ => error("invalid commit_import_wrapped handles".to_string()),
        },
        _ => error("invalid vault key tag".to_string()),
    }
}

extern "C" fn jet_jit_vault_expert_prepare_import_signing(name: i64, bytes: i64) -> i64 {
    match runtime::jet_vault_expert_prepare_import_signing_impl(&clone_string(name), clone_bytes(bytes))
    {
        Ok(plan) => result(true, push(CryptoValue::PlanSigning(plan)) as u64),
        Err(err) => err_debug(err),
    }
}

extern "C" fn jet_jit_vault_expert_commit_import_signing(write: i64, plan: i64) -> i64 {
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

pub(crate) struct CryptoHostFns {
    pub x25519_generate: FuncId,
    pub x25519_public: FuncId,
    pub signing_generate: FuncId,
    pub signing_public: FuncId,
    pub sign: FuncId,
    pub verify: FuncId,
    pub sha256: FuncId,
    pub sha512_bytes: FuncId,
    pub blake3_bytes: FuncId,
    pub digest256_hex: FuncId,
    pub digest256_bytes: FuncId,
    pub signature_bytes: FuncId,
    pub sealed_bytes: FuncId,
    pub x25519_public_bytes: FuncId,
    pub x25519_public_text: FuncId,
    pub x25519_public_from_text: FuncId,
    pub secret_from_text: FuncId,
    pub random_bytes: FuncId,
    pub seal: FuncId,
    pub open: FuncId,
    pub password_hash: FuncId,
    pub password_verify: FuncId,
    pub password_text: FuncId,
    pub file_open: FuncId,
    pub secret_from_bytes: FuncId,
    pub hkdf_sha256: FuncId,
    pub x25519_public_from_bytes: FuncId,
    pub x25519_shared: FuncId,
    pub constant_time_equal: FuncId,
    pub constant_time_equal_bytes: FuncId,
    pub file_seal: FuncId,
    pub expert_aes256gcm_seal: FuncId,
    pub expert_aes256gcm_open: FuncId,
    pub expert_open_v1: FuncId,
    pub expert_migrate_v1: FuncId,
    pub expert_x25519: FuncId,
    pub expert_secret_bytes: FuncId,
    pub verify_jwt: FuncId,
    pub verify_paseto: FuncId,
    pub vault_get: FuncId,
    pub vault_key_ref_show: FuncId,
    pub vault_current: FuncId,
    pub vault_prepare_generate: FuncId,
    pub vault_prepare_rotate: FuncId,
    pub vault_prepare_store: FuncId,
    pub vault_prepare_retire: FuncId,
    pub vault_prepare_revoke: FuncId,
    pub vault_authorize_write: FuncId,
    pub vault_commit_generate: FuncId,
    pub vault_commit_store: FuncId,
    pub vault_commit_rotate: FuncId,
    pub vault_commit_retire: FuncId,
    pub vault_commit_revoke: FuncId,
    pub vault_load: FuncId,
    pub vault_status: FuncId,
    pub vault_versions: FuncId,
    pub vault_export_to_recipients: FuncId,
    pub vault_export_to_passphrase: FuncId,
    pub vault_wrapped_from_bytes: FuncId,
    pub vault_wrapped_bytes: FuncId,
    pub vault_unlock_recipient: FuncId,
    pub vault_unlock_passphrase: FuncId,
    pub vault_prepare_import_wrapped: FuncId,
    pub vault_authorize_wrapped_import: FuncId,
    pub vault_commit_import_wrapped: FuncId,
    pub vault_expert_prepare_import_signing: FuncId,
    pub vault_expert_commit_import_signing: FuncId,
}

pub(crate) fn register_crypto_symbols(builder: &mut JITBuilder) {
    for (name, pointer) in [
        ("jet_jit_crypto_x25519_generate", jet_jit_crypto_x25519_generate as *const u8),
        ("jet_jit_crypto_x25519_public", jet_jit_crypto_x25519_public as *const u8),
        ("jet_jit_crypto_signing_generate", jet_jit_crypto_signing_generate as *const u8),
        ("jet_jit_crypto_signing_public", jet_jit_crypto_signing_public as *const u8),
        ("jet_jit_crypto_sign", jet_jit_crypto_sign as *const u8),
        ("jet_jit_crypto_verify", jet_jit_crypto_verify as *const u8),
        ("jet_jit_crypto_sha256", jet_jit_crypto_sha256 as *const u8),
        ("jet_jit_crypto_sha512_bytes", jet_jit_crypto_sha512_bytes as *const u8),
        ("jet_jit_crypto_blake3_bytes", jet_jit_crypto_blake3_bytes as *const u8),
        ("jet_jit_crypto_digest256_hex", jet_jit_crypto_digest256_hex as *const u8),
        ("jet_jit_crypto_digest256_bytes", jet_jit_crypto_digest256_bytes as *const u8),
        ("jet_jit_crypto_signature_bytes", jet_jit_crypto_signature_bytes as *const u8),
        ("jet_jit_crypto_sealed_bytes", jet_jit_crypto_sealed_bytes as *const u8),
        ("jet_jit_crypto_x25519_public_bytes", jet_jit_crypto_x25519_public_bytes as *const u8),
        ("jet_jit_crypto_x25519_public_text", jet_jit_crypto_x25519_public_text as *const u8),
        ("jet_jit_crypto_x25519_public_from_text", jet_jit_crypto_x25519_public_from_text as *const u8),
        ("jet_jit_crypto_secret_from_text", jet_jit_crypto_secret_from_text as *const u8),
        ("jet_jit_crypto_random_bytes", jet_jit_crypto_random_bytes as *const u8),
        ("jet_jit_crypto_seal", jet_jit_crypto_seal as *const u8),
        ("jet_jit_crypto_open", jet_jit_crypto_open as *const u8),
        ("jet_jit_crypto_password_hash", jet_jit_crypto_password_hash as *const u8),
        ("jet_jit_crypto_password_verify", jet_jit_crypto_password_verify as *const u8),
        ("jet_jit_crypto_password_text", jet_jit_crypto_password_text as *const u8),
        ("jet_jit_crypto_file_open", jet_jit_crypto_file_open as *const u8),
        ("jet_jit_crypto_secret_from_bytes", jet_jit_crypto_secret_from_bytes as *const u8),
        ("jet_jit_crypto_hkdf_sha256", jet_jit_crypto_hkdf_sha256 as *const u8),
        ("jet_jit_crypto_x25519_public_from_bytes", jet_jit_crypto_x25519_public_bytes_raw as *const u8),
        ("jet_jit_crypto_x25519_shared", jet_jit_crypto_x25519_shared as *const u8),
        ("jet_jit_crypto_constant_time_equal", jet_jit_crypto_constant_time_equal as *const u8),
        ("jet_jit_crypto_constant_time_equal_bytes", jet_jit_crypto_constant_time_equal_bytes as *const u8),
        ("jet_jit_crypto_file_seal", jet_jit_crypto_file_seal as *const u8),
        ("jet_jit_crypto_expert_aes256gcm_seal", jet_jit_crypto_expert_aes256gcm_seal as *const u8),
        ("jet_jit_crypto_expert_aes256gcm_open", jet_jit_crypto_expert_aes256gcm_open as *const u8),
        ("jet_jit_crypto_expert_open_v1", jet_jit_crypto_expert_open_v1 as *const u8),
        ("jet_jit_crypto_expert_migrate_v1", jet_jit_crypto_expert_migrate_v1 as *const u8),
        ("jet_jit_crypto_expert_x25519", jet_jit_crypto_expert_x25519 as *const u8),
        ("jet_jit_crypto_expert_secret_bytes", jet_jit_crypto_expert_secret_bytes as *const u8),
        ("jet_jit_auth_verify_jwt", jet_jit_auth_verify_jwt as *const u8),
        ("jet_jit_auth_verify_paseto", jet_jit_auth_verify_paseto as *const u8),
        ("jet_jit_vault_get", jet_jit_vault_get as *const u8),
        ("jet_jit_vault_key_ref_show", jet_jit_vault_key_ref_show as *const u8),
        ("jet_jit_vault_current", jet_jit_vault_current as *const u8),
        ("jet_jit_vault_prepare_generate", jet_jit_vault_prepare_generate as *const u8),
        ("jet_jit_vault_prepare_rotate", jet_jit_vault_prepare_rotate as *const u8),
        ("jet_jit_vault_prepare_store", jet_jit_vault_prepare_store as *const u8),
        ("jet_jit_vault_prepare_retire", jet_jit_vault_prepare_retire as *const u8),
        ("jet_jit_vault_prepare_revoke", jet_jit_vault_prepare_revoke as *const u8),
        ("jet_jit_vault_authorize_write", jet_jit_vault_authorize_write as *const u8),
        ("jet_jit_vault_commit_generate", jet_jit_vault_commit_generate as *const u8),
        ("jet_jit_vault_commit_store", jet_jit_vault_commit_store as *const u8),
        ("jet_jit_vault_commit_rotate", jet_jit_vault_commit_rotate as *const u8),
        ("jet_jit_vault_commit_retire", jet_jit_vault_commit_retire as *const u8),
        ("jet_jit_vault_commit_revoke", jet_jit_vault_commit_revoke as *const u8),
        ("jet_jit_vault_load", jet_jit_vault_load as *const u8),
        ("jet_jit_vault_status", jet_jit_vault_status as *const u8),
        ("jet_jit_vault_versions", jet_jit_vault_versions as *const u8),
        ("jet_jit_vault_export_to_recipients", jet_jit_vault_export_to_recipients as *const u8),
        ("jet_jit_vault_export_to_passphrase", jet_jit_vault_export_to_passphrase as *const u8),
        ("jet_jit_vault_wrapped_from_bytes", jet_jit_vault_wrapped_from_bytes as *const u8),
        ("jet_jit_vault_wrapped_bytes", jet_jit_vault_wrapped_bytes as *const u8),
        ("jet_jit_vault_unlock_recipient", jet_jit_vault_unlock_recipient as *const u8),
        ("jet_jit_vault_unlock_passphrase", jet_jit_vault_unlock_passphrase as *const u8),
        ("jet_jit_vault_prepare_import_wrapped", jet_jit_vault_prepare_import_wrapped as *const u8),
        ("jet_jit_vault_authorize_wrapped_import", jet_jit_vault_authorize_wrapped_import as *const u8),
        ("jet_jit_vault_commit_import_wrapped", jet_jit_vault_commit_import_wrapped as *const u8),
        ("jet_jit_vault_expert_prepare_import_signing", jet_jit_vault_expert_prepare_import_signing as *const u8),
        ("jet_jit_vault_expert_commit_import_signing", jet_jit_vault_expert_commit_import_signing as *const u8),
    ] {
        builder.symbol(name, pointer);
    }
}

pub(crate) fn declare_crypto_host_fns(module: &mut JITModule) -> Result<CryptoHostFns, String> {
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
    let mut septenary = quinary.clone();
    septenary.params.push(AbiParam::new(types::I64));
    septenary.params.push(AbiParam::new(types::I64));
    let mut import = |name: &str, signature: &Signature| {
        module
            .declare_function(name, Linkage::Import, signature)
            .map_err(|error| error.to_string())
    };
    Ok(CryptoHostFns {
        x25519_generate: import("jet_jit_crypto_x25519_generate", &nullary)?,
        x25519_public: import("jet_jit_crypto_x25519_public", &unary)?,
        signing_generate: import("jet_jit_crypto_signing_generate", &nullary)?,
        signing_public: import("jet_jit_crypto_signing_public", &unary)?,
        sign: import("jet_jit_crypto_sign", &binary)?,
        verify: import("jet_jit_crypto_verify", &ternary)?,
        sha256: import("jet_jit_crypto_sha256", &unary)?,
        sha512_bytes: import("jet_jit_crypto_sha512_bytes", &unary)?,
        blake3_bytes: import("jet_jit_crypto_blake3_bytes", &unary)?,
        digest256_hex: import("jet_jit_crypto_digest256_hex", &unary)?,
        digest256_bytes: import("jet_jit_crypto_digest256_bytes", &unary)?,
        signature_bytes: import("jet_jit_crypto_signature_bytes", &unary)?,
        sealed_bytes: import("jet_jit_crypto_sealed_bytes", &unary)?,
        x25519_public_bytes: import("jet_jit_crypto_x25519_public_bytes", &unary)?,
        x25519_public_text: import("jet_jit_crypto_x25519_public_text", &unary)?,
        x25519_public_from_text: import("jet_jit_crypto_x25519_public_from_text", &unary)?,
        secret_from_text: import("jet_jit_crypto_secret_from_text", &unary)?,
        random_bytes: import("jet_jit_crypto_random_bytes", &unary)?,
        seal: import("jet_jit_crypto_seal", &ternary)?,
        open: import("jet_jit_crypto_open", &ternary)?,
        password_hash: import("jet_jit_crypto_password_hash", &unary)?,
        password_verify: import("jet_jit_crypto_password_verify", &binary)?,
        password_text: import("jet_jit_crypto_password_text", &unary)?,
        file_open: import("jet_jit_crypto_file_open", &ternary)?,
        secret_from_bytes: import("jet_jit_crypto_secret_from_bytes", &unary)?,
        hkdf_sha256: import("jet_jit_crypto_hkdf_sha256", &quaternary)?,
        x25519_public_from_bytes: import("jet_jit_crypto_x25519_public_from_bytes", &unary)?,
        x25519_shared: import("jet_jit_crypto_x25519_shared", &binary)?,
        constant_time_equal: import("jet_jit_crypto_constant_time_equal", &binary)?,
        constant_time_equal_bytes: import("jet_jit_crypto_constant_time_equal_bytes", &binary)?,
        file_seal: import("jet_jit_crypto_file_seal", &ternary)?,
        expert_aes256gcm_seal: import("jet_jit_crypto_expert_aes256gcm_seal", &quaternary)?,
        expert_aes256gcm_open: import("jet_jit_crypto_expert_aes256gcm_open", &quaternary)?,
        expert_open_v1: import("jet_jit_crypto_expert_open_v1", &binary)?,
        expert_migrate_v1: import("jet_jit_crypto_expert_migrate_v1", &quaternary)?,
        expert_x25519: import("jet_jit_crypto_expert_x25519", &ternary)?,
        expert_secret_bytes: import("jet_jit_crypto_expert_secret_bytes", &unary)?,
        verify_jwt: import("jet_jit_auth_verify_jwt", &quinary)?,
        verify_paseto: import("jet_jit_auth_verify_paseto", &septenary)?,
        vault_get: import("jet_jit_vault_get", &unary)?,
        vault_key_ref_show: import("jet_jit_vault_key_ref_show", &unary)?,
        vault_current: import("jet_jit_vault_current", &binary)?,
        vault_prepare_generate: import("jet_jit_vault_prepare_generate", &binary)?,
        vault_prepare_rotate: import("jet_jit_vault_prepare_rotate", &binary)?,
        vault_prepare_store: import("jet_jit_vault_prepare_store", &ternary)?,
        vault_prepare_retire: import("jet_jit_vault_prepare_retire", &ternary)?,
        vault_prepare_revoke: import("jet_jit_vault_prepare_revoke", &ternary)?,
        vault_authorize_write: import("jet_jit_vault_authorize_write", &ternary)?,
        vault_commit_generate: import("jet_jit_vault_commit_generate", &ternary)?,
        vault_commit_store: import("jet_jit_vault_commit_store", &ternary)?,
        vault_commit_rotate: import("jet_jit_vault_commit_rotate", &ternary)?,
        vault_commit_retire: import("jet_jit_vault_commit_retire", &ternary)?,
        vault_commit_revoke: import("jet_jit_vault_commit_revoke", &ternary)?,
        vault_load: import("jet_jit_vault_load", &binary)?,
        vault_status: import("jet_jit_vault_status", &binary)?,
        vault_versions: import("jet_jit_vault_versions", &binary)?,
        vault_export_to_recipients: import("jet_jit_vault_export_to_recipients", &ternary)?,
        vault_export_to_passphrase: import("jet_jit_vault_export_to_passphrase", &ternary)?,
        vault_wrapped_from_bytes: import("jet_jit_vault_wrapped_from_bytes", &unary)?,
        vault_wrapped_bytes: import("jet_jit_vault_wrapped_bytes", &unary)?,
        vault_unlock_recipient: import("jet_jit_vault_unlock_recipient", &unary)?,
        vault_unlock_passphrase: import("jet_jit_vault_unlock_passphrase", &unary)?,
        vault_prepare_import_wrapped: import("jet_jit_vault_prepare_import_wrapped", &quaternary)?,
        vault_authorize_wrapped_import: import("jet_jit_vault_authorize_wrapped_import", &ternary)?,
        vault_commit_import_wrapped: import("jet_jit_vault_commit_import_wrapped", &ternary)?,
        vault_expert_prepare_import_signing: import("jet_jit_vault_expert_prepare_import_signing", &binary)?,
        vault_expert_commit_import_signing: import("jet_jit_vault_expert_commit_import_signing", &binary)?,
    })
}
