//! D-ONCE: one table of which types each Core module exports, so a qualified
//! import (`alias.Leaf` where `alias` names a Core module) resolves through
//! one generic lookup instead of a hand-written match arm per module.
//!
//! Most modules canonicalize a qualified leaf straight to `Leaf`. `core.crypto`
//! additionally marks a narrow secret-bearing subset so the
//! caller can wrap those in the nominal-provenance type instead of the plain
//! one — everything else in the table is `Plain`.

/// How a resolved Core-module leaf should be represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreLeafKind {
    /// Canonicalize straight to `Type::Named(leaf)`.
    Plain,
    /// D-CRYPTO-API1=A: secret-bearing crypto values keep distinct nominal
    /// provenance from a same-named local type.
    CryptoNominal,
}

const CRYPTO_LEAVES: &[(&str, CoreLeafKind)] = &[
    ("Secret", CoreLeafKind::CryptoNominal),
    ("SigningKey", CoreLeafKind::CryptoNominal),
    ("X25519SecretKey", CoreLeafKind::CryptoNominal),
    ("SharedSecret", CoreLeafKind::CryptoNominal),
    ("VerifyKey", CoreLeafKind::Plain),
    ("X25519PublicKey", CoreLeafKind::Plain),
    ("Signature", CoreLeafKind::Plain),
    ("Sealed", CoreLeafKind::Plain),
    ("WrappedKey", CoreLeafKind::Plain),
    ("WrappedVaultKey", CoreLeafKind::Plain),
    ("KeyUnlock", CoreLeafKind::Plain),
    ("PasswordHash", CoreLeafKind::Plain),
    ("Digest256", CoreLeafKind::Plain),
    ("Digest512", CoreLeafKind::Plain),
    ("Hasher", CoreLeafKind::Plain),
    ("CryptoError", CoreLeafKind::Plain),
    ("FileCryptoError", CoreLeafKind::Plain),
    ("KeyWrapError", CoreLeafKind::Plain),
];

const ENCODING_LEAVES: &[(&str, CoreLeafKind)] = &[
    ("DataTree", CoreLeafKind::Plain),
    ("EncodingLimits", CoreLeafKind::Plain),
    ("EncodingError", CoreLeafKind::Plain),
    ("EncodingCause", CoreLeafKind::Plain),
    ("EncodingFormat", CoreLeafKind::Plain),
    ("EncodingErrorKind", CoreLeafKind::Plain),
    ("DataEvent", CoreLeafKind::Plain),
];

const EMAIL_LEAVES: &[(&str, CoreLeafKind)] = &[
    ("Address", CoreLeafKind::Plain),
    ("Message", CoreLeafKind::Plain),
    ("Attachment", CoreLeafKind::Plain),
    ("Envelope", CoreLeafKind::Plain),
    ("SMTPSecurity", CoreLeafKind::Plain),
    ("RecipientPolicy", CoreLeafKind::Plain),
    ("RecipientReport", CoreLeafKind::Plain),
    ("SendReport", CoreLeafKind::Plain),
    ("EmailError", CoreLeafKind::Plain),
    ("Limits", CoreLeafKind::Plain),
    ("SMTPAuth", CoreLeafKind::Plain),
    ("TLSTrust", CoreLeafKind::Plain),
    ("DkimConfig", CoreLeafKind::Plain),
    ("SMTPConfig", CoreLeafKind::Plain),
    ("Mailer", CoreLeafKind::Plain),
];

const ENV_LEAVES: &[(&str, CoreLeafKind)] = &[("EnvError", CoreLeafKind::Plain)];

const MEM_LEAVES: &[(&str, CoreLeafKind)] = &[("AllocError", CoreLeafKind::Plain)];

/// Canonical root Core types that are not qualified module leaves. Keep these
/// in the same Core-name authority used by generated declarations.
const CORE_ROOT_TYPES: &[&str] = &[
    crate::Syntax::TYPE_DECIMAL,
    crate::Syntax::DURATION_TYPE,
    "Date",
    "LocalDate",
    "LocalTime",
    crate::Syntax::TYPE_JSON_ERROR,
];

/// One table row per Core module: its canonical name plus its exported leaf
/// types. Add a module here to give it resolve_type support — no new match
/// arm required.
const CORE_MODULE_EXPORTS: &[(&str, &[(&str, CoreLeafKind)])] = &[
    ("core.crypto", CRYPTO_LEAVES),
    ("core.encoding", ENCODING_LEAVES),
    ("core.encoding.json", &[
        ("JSONReader", CoreLeafKind::Plain),
        ("JSONWriter", CoreLeafKind::Plain),
    ]),
    ("core.encoding.jsonl", &[
        ("JSONLReader", CoreLeafKind::Plain),
        ("JSONLWriter", CoreLeafKind::Plain),
    ]),
    ("core.encoding.csv", &[
        ("CSVReader", CoreLeafKind::Plain),
        ("CSVWriter", CoreLeafKind::Plain),
    ]),
    ("core.encoding.xml", &[
        ("XMLReader", CoreLeafKind::Plain),
        ("XMLWriter", CoreLeafKind::Plain),
    ]),
    ("core.encoding.cbor", &[
        ("CBORReader", CoreLeafKind::Plain),
        ("CBORWriter", CoreLeafKind::Plain),
        ("CBOROptions", CoreLeafKind::Plain),
        ("CBORError", CoreLeafKind::Plain),
        ("CBORErrorKind", CoreLeafKind::Plain),
    ]),
    ("core.email", EMAIL_LEAVES),
    ("core.env", ENV_LEAVES),
    ("core.mem", MEM_LEAVES),
];

/// Look up how `module.leaf` should resolve, or `None` if that module/leaf
/// pair isn't a registered Core export (the caller falls through to other
/// resolution paths, e.g. the generic file-module registry or `core.lang`'s
/// dynamic rule check).
pub fn core_leaf_kind(module: &str, leaf: &str) -> Option<CoreLeafKind> {
    lookup(CORE_MODULE_EXPORTS, module, leaf)
}

/// Whether `name` is a canonical root or module-exported Core type.
///
/// Generated declarations use this same Core-name table when choosing a
/// user-visible type name. Keeping the query here prevents binders from
/// carrying a second, inevitably stale list of Core names.
pub fn is_core_type_name(name: &str) -> bool {
    CORE_ROOT_TYPES.contains(&name)
        || CORE_MODULE_EXPORTS
            .iter()
            .any(|(_, leaves)| leaves.iter().any(|(leaf, _)| *leaf == name))
}

/// The one generic lookup every module table (production or test) resolves
/// through — adding a module is a data row here, never a new match arm.
fn lookup(
    table: &[(&str, &[(&str, CoreLeafKind)])],
    module: &str,
    leaf: &str,
) -> Option<CoreLeafKind> {
    table
        .iter()
        .find(|(name, _)| *name == module)
        .and_then(|(_, leaves)| leaves.iter().find(|(l, _)| *l == leaf))
        .map(|(_, kind)| *kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_modules_resolve_without_a_bespoke_arm() {
        assert_eq!(core_leaf_kind("core.email", "Address"), Some(CoreLeafKind::Plain));
        assert_eq!(
            core_leaf_kind("core.crypto", "Secret"),
            Some(CoreLeafKind::CryptoNominal)
        );
        assert_eq!(core_leaf_kind("core.crypto", "VerifyKey"), Some(CoreLeafKind::Plain));
        assert_eq!(core_leaf_kind("core.env", "EnvError"), Some(CoreLeafKind::Plain));
        assert_eq!(core_leaf_kind("core.encoding.cbor", "CBORReader"), Some(CoreLeafKind::Plain));
        assert_eq!(core_leaf_kind("core.email", "NoSuchLeaf"), None);
        assert_eq!(core_leaf_kind("core.nonexistent", "Address"), None);
    }

    /// Criterion #2: a brand-new Core module needs one data row, not a new
    /// Rust match arm. `lookup` is the one function every module (production
    /// or, here, a module added purely for this test) resolves through.
    #[test]
    fn adding_a_module_is_one_data_row_no_new_match_arm() {
        const NEW_MODULE_LEAVES: &[(&str, CoreLeafKind)] = &[("Widget", CoreLeafKind::Plain)];
        const TABLE_WITH_NEW_MODULE: &[(&str, &[(&str, CoreLeafKind)])] =
            &[("core.widgets", NEW_MODULE_LEAVES)];

        assert_eq!(
            lookup(TABLE_WITH_NEW_MODULE, "core.widgets", "Widget"),
            Some(CoreLeafKind::Plain)
        );
    }
}
