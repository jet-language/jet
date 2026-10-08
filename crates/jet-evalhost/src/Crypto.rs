//! Native JIT adapters for crypto, authentication, and vault calls.
//!
//! Algorithms come from the same source files emitted into AOT bridge crates.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use jet_foundation::AST::{CtValue, Type};
use jet_foundation::Diagnostics::{Diagnostic, Span};
use jet_foundation::Prelude::{jet_as_bytes, jet_e0956_unsupported};

pub(crate) mod runtime {
    #[allow(unused_imports)]
    pub use jet_unicode::*;
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

    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../jet-codegen/src/Prelude/CoreLib/Top/AuthSession.rs");

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
    WrappedPlanX25519(runtime::JetVaultWrappedImportPlan<runtime::JetX25519SecretKey>)
}

thread_local! {
    /// Opaque crypto and vault carriers of one interpreter run; handles are
    /// 1-based indices.
    static CRYPTO_VALUES: std::cell::RefCell<Vec<Option<CryptoValue>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Scope the opaque crypto carriers to one interpreter run, restoring the
/// enclosing run's carriers afterwards.
pub(crate) fn with_interpreter_crypto_state<R>(body: impl FnOnce() -> R) -> R {
    struct Restore(Vec<Option<CryptoValue>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = std::mem::take(&mut self.0);
            let current = CRYPTO_VALUES.with(|slot| slot.replace(previous));
            drop(current);
        }
    }
    let _restore = Restore(CRYPTO_VALUES.with(|slot| slot.take()));
    body()
}

fn push(value: CryptoValue) -> i64 {
    CRYPTO_VALUES.with(|slot| {
        let mut values = slot.borrow_mut();
        values.push(Some(value));
        values.len() as i64
    })
}

fn take_crypto(handle: i64) -> Option<CryptoValue> {
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    CRYPTO_VALUES.with(|slot| slot.borrow_mut().get_mut(index).and_then(Option::take))
}

fn with_crypto<R>(handle: i64, f: impl FnOnce(&CryptoValue) -> Option<R>) -> Option<R> {
    let index = usize::try_from(handle).ok()?.checked_sub(1)?;
    CRYPTO_VALUES.with(|slot| slot.borrow().get(index).and_then(Option::as_ref).and_then(f))
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
        _ => None
    }
}

/// The ONE `impl Display for JetVaultKeyRef` rendering, read through the same
/// handle registry `jet_jit_vault_key_ref_show` uses.
pub(crate) fn vault_key_ref_text(handle: i64) -> Option<String> {
    with_crypto(handle, |value| match value {
        CryptoValue::KeyRefSigning(key) => Some(key.to_string()),
        CryptoValue::KeyRefX25519(key) => Some(key.to_string()),
        _ => None
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
        _ => None
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
            _ => None
        })
        .map(|result| result.map(|write| push(CryptoValue::WriteSigning(write)))),
        2 => with_crypto(plan, |value| match value {
            CryptoValue::PlanX25519(plan) => {
                Some(runtime::jet_vault_authorize_write_impl(plan, reason))
            }
            _ => None
        })
        .map(|result| result.map(|write| push(CryptoValue::WriteX25519(write)))),
        _ => None
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
            _ => None
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
            _ => None
        },
        _ => None
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
        _ => None
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
        _ => None
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
            _ => None
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                Some(
                    runtime::jet_vault_commit_generate_impl(write, plan)
                        .map(|key| push(CryptoValue::KeyRefX25519(key))),
                )
            }
            _ => None
        },
        _ => None
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
            _ => None
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                Some(
                    runtime::jet_vault_commit_store_impl(write, plan)
                        .map(|key| push(CryptoValue::KeyRefX25519(key))),
                )
            }
            _ => None
        },
        _ => None
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
        _ => None
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
        _ => None
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
        _ => None
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
        fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))]
    }
}

fn crypto_password_carrier(text: String) -> CtValue {
    CtValue::Struct {
        type_name: "PasswordHash".to_string(),
        fields: vec![("text".to_string(), CtValue::Str(text))]
    }
}

fn crypto_error_value(error: runtime::JetCryptoError) -> CtValue {
    use runtime::JetCryptoError;
    // Core owns this payload-free public projection; backend details never
    // become an unmatchable Struct { reason } in either direct or vault errors.
    let variant = match error {
        JetCryptoError::OpenFailed => "OpenFailed",
        JetCryptoError::NonContributoryKey
        | JetCryptoError::InvalidEncoding { .. }
        | JetCryptoError::UnsupportedVersion { .. }
        | JetCryptoError::UnsupportedAlgorithm { .. }
        | JetCryptoError::PasswordPolicy { .. } => "KeyRejected",
        JetCryptoError::InvalidLength { .. } | JetCryptoError::OutputLength { .. } => "Length",
        JetCryptoError::EntropyUnavailable
        | JetCryptoError::ResourceUnavailable { .. }
        | JetCryptoError::Internal { .. } => "Unavailable",
    };
    vault_enum("CryptoError", variant, Vec::new())
}

fn crypto_failed(error: runtime::JetCryptoError) -> Result<CtValue, Diagnostic> {
    Ok(CtValue::failed(Box::new(crypto_error_value(error))))
}

fn crypto_result<T>(
    value: Result<T, runtime::JetCryptoError>,
    encode: impl FnOnce(T) -> CtValue,
) -> Result<CtValue, Diagnostic> {
    match value {
        Ok(value) => Ok(CtValue::Present(Box::new(encode(value)))),
        Err(error) => crypto_failed(error)
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
        args
    }
}

fn vault_carrier(type_name: &str, handle: i64) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![(VAULT_HANDLE_FIELD.to_string(), CtValue::Int(handle))]
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
        ]
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
            _ => None
        })?
    })
}

fn vault_field<'a>(value: &'a CtValue, type_name: &str, name: &str) -> Option<&'a CtValue> {
    let CtValue::Struct {
        type_name: actual,
        fields
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
        _ => None
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
        _ => None
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
        _ => None
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
            vec![(None, crypto_error_value(error))],
        ),
        JetVaultError::IO {
            operation,
            redacted_path
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
        )
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
        )
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
        Err(error) => vault_failed(error)
    }
}

fn vault_wrap_result<T>(
    value: Result<T, runtime::JetVaultKeyWrapError>,
    encode: impl FnOnce(T) -> CtValue,
) -> Result<CtValue, Diagnostic> {
    match value {
        Ok(value) => vault_ok(encode(value)),
        Err(error) => vault_wrap_failed(error)
    }
}

fn vault_current_result(
    value: Result<Option<i64>, runtime::JetVaultError>,
) -> Result<CtValue, Diagnostic> {
    match value {
        Ok(Some(handle)) => vault_ok(CtValue::Present(Box::new(vault_key_ref_value(handle)))),
        Ok(None) => vault_ok(CtValue::absent(Type::Named("KeyRef".to_string()))),
        Err(error) => vault_failed(error)
    }
}

fn vault_status_value(status: runtime::JetVaultKeyStatus) -> CtValue {
    let variant = match status {
        runtime::JetVaultKeyStatus::Active => "Active",
        runtime::JetVaultKeyStatus::Retired => "Retired",
        runtime::JetVaultKeyStatus::Revoked => "Revoked"
    };
    vault_enum("KeyStatus", variant, Vec::new())
}

fn vault_rotation_value(previous: i64, current: i64) -> CtValue {
    CtValue::Struct {
        type_name: "Rotation".to_string(),
        fields: vec![
            ("previous".to_string(), vault_key_ref_value(previous)),
            ("current".to_string(), vault_key_ref_value(current)),
        ]
    }
}

fn vault_loaded_value(tag: i64, key: runtime::JetSigningKey) -> CtValue {
    CtValue::Struct {
        type_name: "SigningKey".to_string(),
        fields: vec![(
            "bytes".to_string(),
            CtValue::Bytes(runtime::jet_crypto_expert_signing_key_bytes_impl(&key)),
        )]
    }
}

fn vault_loaded_x25519_value(key: runtime::JetX25519SecretKey) -> CtValue {
    CtValue::Struct {
        type_name: "X25519SecretKey".to_string(),
        fields: vec![(
            "bytes".to_string(),
            CtValue::Bytes(runtime::jet_crypto_expert_x25519_secret_bytes_impl(&key)),
        )]
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
            _ => None
        })
        .map(|value| value.map(|plan| push(CryptoValue::PlanSigning(plan)))),
        2 => with_crypto(key_ref, |value| match value {
            CryptoValue::KeyRefX25519(key) => Some(if revoke {
                runtime::jet_vault_prepare_revoke_impl(key, &reason)
            } else {
                runtime::jet_vault_prepare_retire_impl(key, &reason)
            }),
            _ => None
        })
        .map(|value| value.map(|plan| push(CryptoValue::PlanX25519(plan)))),
        _ => None
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
            _ => None
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::PlanX25519(plan))) => {
                Some(if revoke {
                    runtime::jet_vault_commit_revoke_impl(write, plan)
                } else {
                    runtime::jet_vault_commit_retire_impl(write, plan)
                })
            }
            _ => None
        },
        _ => None
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
            _ => None
        })
        .map(|value| value.map(|write| push(CryptoValue::WriteSigning(write)))),
        2 => with_crypto(plan, |value| match value {
            CryptoValue::WrappedPlanX25519(plan) => Some(
                runtime::jet_vault_authorize_wrapped_import_impl(plan, &reason),
            ),
            _ => None
        })
        .map(|value| value.map(|write| push(CryptoValue::WriteX25519(write)))),
        _ => None
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
            _ => None
        },
        2 => match (take_crypto(write), take_crypto(plan)) {
            (Some(CryptoValue::WriteX25519(write)), Some(CryptoValue::WrappedPlanX25519(plan))) => {
                Some(
                    runtime::jet_vault_commit_import_wrapped_impl(write, plan)
                        .map(|key| push(CryptoValue::KeyRefX25519(key))),
                )
            }
            _ => None
        },
        _ => None
    }
}

enum AmbientVaultUnlock {
    Recipient(runtime::JetX25519SecretKey),
    Passphrase(runtime::Secret)
}

fn vault_unlock_from_handle(handle: i64) -> Option<AmbientVaultUnlock> {
    match take_crypto(handle) {
        Some(CryptoValue::UnlockRecipient(identity)) => with_crypto(identity, |value| match value {
            CryptoValue::X25519SecretKey(key) => {
                Some(AmbientVaultUnlock::Recipient(runtime::clone_x25519_secret(key)))
            }
            _ => None
        }),
        Some(CryptoValue::UnlockPassphrase(passphrase)) => {
            with_crypto(passphrase, |value| match value {
                CryptoValue::Secret(secret) => {
                    Some(AmbientVaultUnlock::Passphrase(runtime::clone_secret(secret)))
                }
                _ => None
            })
        }
        _ => None
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
                    Err(error) => return Some(Err(error))
                };
                let (type_name, bytes) = match method {
                    "sha256" => ("Digest256", runtime::jet_crypto_digest256_bytes_impl(
                        &runtime::jet_crypto_sha256_typed_impl(&input))),
                    "blake3" => ("Digest256", runtime::jet_crypto_digest256_bytes_impl(
                        &runtime::jet_crypto_blake3_typed_impl(&input))),
                    _ => ("Digest512", runtime::jet_crypto_digest512_bytes_impl(
                        &runtime::jet_crypto_sha512_typed_impl(&input)))
                };
                Some(Ok(CtValue::Struct {
                    type_name: type_name.to_string(),
                    fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))]
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
                    Err(error) => return Some(Err(error))
                };
                let secret = match runtime::x25519_secret_from_bytes(bytes) {
                    Ok(secret) => secret,
                    Err(error) => return Some(Err(vault_diag(error, span)))
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
                    Err(error) => return Some(Err(error))
                };
                let public = match runtime::jet_crypto_x25519_public_from_bytes_impl(bytes) {
                    Ok(public) => public,
                    Err(error) => return Some(crypto_failed(error))
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
                    Err(error) => return Some(Err(error))
                };
                let signing = match runtime::signing_key_from_bytes(bytes) {
                    Ok(signing) => signing,
                    Err(error) => return Some(Err(vault_diag(error, span)))
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
                    Err(error) => return Some(Err(error))
                };
                let plaintext = match jet_as_bytes(plaintext, span) {
                    Ok(plaintext) => plaintext,
                    Err(error) => return Some(Err(error))
                };
                let aad = match jet_as_bytes(aad, span) {
                    Ok(aad) => aad,
                    Err(error) => return Some(Err(error))
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
                        Err(error) => return Some(Err(vault_diag(error, span)))
                    },
                    Err(error) => return Some(Err(error))
                };
                let sealed = match vault_nominal_bytes(sealed, "Sealed", span) {
                    Ok(bytes) => match runtime::jet_crypto_sealed_from_bytes_impl(bytes) {
                        Ok(sealed) => sealed,
                        Err(error) => return Some(crypto_failed(error))
                    },
                    Err(error) => return Some(Err(error))
                };
                let aad = match jet_as_bytes(aad, span) {
                    Ok(aad) => aad,
                    Err(error) => return Some(Err(error))
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
                        Err(error) => return Some(Err(vault_diag(error, span)))
                    },
                    Err(error) => return Some(Err(error))
                };
                let message = match jet_as_bytes(message, span) {
                    Ok(message) => message,
                    Err(error) => return Some(Err(error))
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
                        Err(error) => return Some(crypto_failed(error))
                    },
                    Err(error) => return Some(Err(error))
                };
                let message = match jet_as_bytes(message, span) {
                    Ok(message) => message,
                    Err(error) => return Some(Err(error))
                };
                let signature = match vault_nominal_bytes(signature, "Signature", span) {
                    Ok(bytes) => match runtime::jet_crypto_signature_from_bytes_impl(bytes) {
                        Ok(signature) => signature,
                        Err(error) => return Some(crypto_failed(error))
                    },
                    Err(error) => return Some(Err(error))
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
                    Err(error) => return Some(Err(error))
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
                    Err(error) => return Some(Err(error))
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
            "__vault_wrapped_from_bytes" => {
                let [bytes] = args.as_slice() else {
                    return Some(Err(vault_diag(
                        "core.crypto.__vault_wrapped_from_bytes received malformed arguments",
                        span,
                    )));
                };
                let bytes = match jet_as_bytes(bytes, span) {
                    Ok(bytes) => bytes,
                    Err(error) => return Some(Err(error))
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
                    _ => None
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
                    Err(error) => return Some(Err(error))
                };
                let identity = match runtime::x25519_secret_from_bytes(bytes) {
                    Ok(identity) => identity,
                    Err(error) => return Some(Err(vault_diag(error, span)))
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
                    Err(error) => return Some(Err(error))
                };
                let passphrase = runtime::jet_crypto_secret_from_bytes_impl(bytes);
                let passphrase = push(CryptoValue::Secret(passphrase));
                let unlock = push(CryptoValue::UnlockPassphrase(passphrase));
                Some(Ok(vault_carrier("KeyUnlock", unlock)))
            }
            _ => None
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
            };
            let value = match tag {
                1 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefSigning(key) => {
                        Some(runtime::jet_vault_load_impl(key))
                    }
                    _ => None
                })
                .map(|value| value.map(|key| vault_loaded_value(1, key))),
                2 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefX25519(key) => {
                        Some(runtime::jet_vault_load_impl(key))
                    }
                    _ => None
                })
                .map(|value| value.map(vault_loaded_x25519_value)),
                _ => None
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
                Err(error) => return Some(Err(error))
            };
            let value = match tag {
                1 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefSigning(key) => {
                        Some(runtime::jet_vault_status_impl(key))
                    }
                    _ => None
                }),
                2 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefX25519(key) => {
                        Some(runtime::jet_vault_status_impl(key))
                    }
                    _ => None
                }),
                _ => None
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
                Err(error) => return Some(Err(error))
            };
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error))
            };
            let value = match tag {
                1 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefSigning(key) => Some(
                        runtime::jet_vault_export_to_recipients_impl(key, &recipients),
                    ),
                    _ => None
                }),
                2 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefX25519(key) => Some(
                        runtime::jet_vault_export_to_recipients_impl(key, &recipients),
                    ),
                    _ => None
                }),
                _ => None
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
                Err(error) => return Some(Err(error))
            };
            let passphrase = runtime::jet_crypto_secret_from_bytes_impl(bytes);
            let tag = match vault_tag_for_args(resolved_ret_ref, &args, &[0], span) {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error))
            };
            let value = match tag {
                1 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefSigning(key) => Some(
                        runtime::jet_vault_export_to_passphrase_impl(key, &passphrase),
                    ),
                    _ => None
                }),
                2 => with_crypto(key_ref, |value| match value {
                    CryptoValue::KeyRefX25519(key) => Some(
                        runtime::jet_vault_export_to_passphrase_impl(key, &passphrase),
                    ),
                    _ => None
                }),
                _ => None
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
                _ => None
            });
            let tag = match resolved_ret_ref
                .and_then(vault_tag_from_type)
                .or(wrapped_tag)
                .ok_or_else(|| vault_diag("vault wrapped plan has no checked key type", span))
            {
                Ok(tag) => tag,
                Err(error) => return Some(Err(error))
            };
            let wrapped = match take_crypto(wrapped_handle) {
                Some(CryptoValue::WrappedVaultKey(wrapped)) => wrapped,
                _ => return Some(Err(vault_diag("invalid wrapped vault key", span)))
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
                _ => return Some(Err(vault_diag("invalid vault key type", span)))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
                Err(error) => return Some(Err(error))
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
        _ => None
    }
}

pub(crate) fn register_interpreter_ambient(
    context: &mut crate::InterpreterAmbientContext,
) {
    context.register_core_call(ambient_core_call);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_crypto_case(value: CtValue, expected: &str) {
        let CtValue::Enum { type_name, variant, args } = value else {
            panic!("CryptoError must be a matchable enum, not backend text");
        };
        assert_eq!(type_name, "CryptoError");
        assert_eq!(variant, expected);
        assert!(args.is_empty(), "the public CryptoError cases are payload-free");
    }

    #[test]
    fn crypto_error_projection_covers_every_backend_variant() {
        use runtime::JetCryptoError;
        let cases = [
            (JetCryptoError::OpenFailed, "OpenFailed"),
            (JetCryptoError::NonContributoryKey, "KeyRejected"),
            (JetCryptoError::InvalidEncoding { operation: "decode", value_kind: "key" }, "KeyRejected"),
            (JetCryptoError::UnsupportedVersion { operation: "decode", version: 0 }, "KeyRejected"),
            (JetCryptoError::UnsupportedAlgorithm { operation: "decode", algorithm: "unknown".into() }, "KeyRejected"),
            (JetCryptoError::PasswordPolicy { reason: "policy" }, "KeyRejected"),
            (JetCryptoError::InvalidLength { operation: "import", parameter: "bytes", expected: "32", actual: 0 }, "Length"),
            (JetCryptoError::OutputLength { operation: "derive", minimum: 1, maximum: 32, actual: 0 }, "Length"),
            (JetCryptoError::EntropyUnavailable, "Unavailable"),
            (JetCryptoError::ResourceUnavailable { resource: "memory" }, "Unavailable"),
            (JetCryptoError::Internal { incident_id: "crypto-4835" }, "Unavailable"),
        ];
        for (error, expected) in cases {
            assert_crypto_case(crypto_error_value(error), expected);
        }
    }

    #[test]
    fn vault_crypto_entropy_failure_is_a_nested_public_enum() {
        struct ResetEntropy;
        impl Drop for ResetEntropy {
            fn drop(&mut self) {
                runtime::jet_crypto_entropy_clear_test_provider();
            }
        }
        runtime::jet_crypto_entropy_set_test_provider(|_| runtime::JetCryptoEntropyStep::Failed);
        let _reset = ResetEntropy;
        let error = match <runtime::JetSigningKey as runtime::JetVaultKey>::generate() {
            Err(error) => error,
            Ok(_) => panic!("a rejected entropy provider must not generate a key"),
        };
        assert!(matches!(error, runtime::JetVaultError::Crypto(runtime::JetCryptoError::EntropyUnavailable)));
        let CtValue::Enum { type_name, variant, mut args } = vault_error_value(error) else {
            panic!("VaultError must remain an enum");
        };
        assert_eq!(type_name, "VaultError");
        assert_eq!(variant, "Crypto");
        assert_eq!(args.len(), 1);
        let (label, cause) = args.pop().unwrap();
        assert_eq!(label, None, "Crypto has one positional CryptoError payload");
        assert_crypto_case(cause, "Unavailable");
    }
}
