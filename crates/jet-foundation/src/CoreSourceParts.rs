//! Compiler-private Core source parts that are not public source modules.
//!
//! A private part is attached to a public owner for loading and source
//! identity, but it deliberately does not participate in `CORE_MODULE_NAMES`,
//! public source-module lookup, or the Core surface ledger.
//!
//! Runtime units are the second kind of private part: every
//! `Core/<area>/runtime/<unit>.jet` that is not a `// PORT-PENDING` packet,
//! listed by the generator straight from that directory. Identity
//! `core.<area>.runtime.<unit>`; only Core sources (public modules and other
//! runtime units) may import one, the loader joins a unit only when a loaded
//! Core source names it, and every item of a loaded unit is kept.
//!
//! Every source row uses the complete CoreSourceModule schema, including an
//! empty impl_types list when the part owns no implementation receivers.

use crate::CoreModuleExports::CoreSourceModule;
use crate::Diagnostics::{Diagnostic, Span};

/// Canonical public owner for the private String receiver source.
pub const CORE_TEXT_STRING_OWNER: &str = "core.text";
/// Internal source identity used only by loader/sema/codegen metadata.
pub const CORE_TEXT_STRING_MODULE: &str = "__core_text_string";
/// Reserved Rust module alias for the private String source part.
pub const CORE_TEXT_STRING_ALIAS: &str = "__core_text_string";

pub const CORE_ENCODING_STREAM_OWNER: &str = "core.encoding";
pub const CORE_ENCODING_STREAM_MODULE: &str = "__core_encoding_stream";
pub const CORE_ENCODING_STREAM_ALIAS: &str = "__core_encoding_stream";

pub const CORE_ENCODING_CBOR_PARSE_OWNER: &str = "core.encoding.cbor";
pub const CORE_ENCODING_CBOR_PARSE_MODULE: &str = "__core_encoding_cbor_parse";
pub const CORE_ENCODING_CBOR_PARSE_ALIAS: &str = "__core_encoding_cbor_parse";

pub const CORE_CRYPTO_JETC_OWNER: &str = "core.crypto";
pub const CORE_CRYPTO_JETC_MODULE: &str = "__core_crypto_jetc";
pub const CORE_CRYPTO_JETC_ALIAS: &str = "__core_crypto_jetc";

pub const CORE_ENCODING_CBOR_STREAM_ALIAS: &str = "__core_encoding_cbor_stream";

pub const CORE_ENCODING_XML_ENGINE_OWNER: &str = "core.encoding.xml";
pub const CORE_ENCODING_XML_ENGINE_MODULE: &str = "__core_encoding_xml_engine";
pub const CORE_ENCODING_XML_ENGINE_ALIAS: &str = "__core_encoding_xml_engine";
pub const CORE_ENCODING_XML_KERNEL_OWNER: &str = "core.encoding.xml";
pub const CORE_ENCODING_XML_KERNEL_MODULE: &str = "__core_encoding_xml_kernel";
pub const CORE_ENCODING_XML_KERNEL_ALIAS: &str = "__core_encoding_xml_kernel";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateSourcePart {
    /// Public source module that schedules this part.
    pub owner: &'static str,
    /// Private source-module metadata. Its `module` is never a public Core
    /// module name. `owned_members` selects source-local exported helpers only.
    pub source: CoreSourceModule,
}

pub const CORE_TEXT_STRING_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_TEXT_STRING_OWNER,
    source: CoreSourceModule {
        module: CORE_TEXT_STRING_MODULE,
        alias: CORE_TEXT_STRING_ALIAS,
        path: "Core/Text/String.jet",
        owned_members: &[],
        impl_types: &[],
    },
};

pub const CORE_TEXT_SEGMENTATION_TABLES_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_TEXT_STRING_OWNER,
    source: CoreSourceModule {
        module: "__core_text_segmentation_tables",
        alias: "__core_text_segmentation_tables",
        path: "Core/Text/SegmentationTables.jet",
        owned_members: &[],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_STREAM_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_STREAM_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_STREAM_MODULE,
        alias: CORE_ENCODING_STREAM_ALIAS,
        path: "Core/Encoding/Stream.jet",
        owned_members: &["json_reader", "json_writer", "jsonl_reader", "jsonl_writer", "object_entries", "object_from_entries", "bytes_payload", "typed_number", "typed_string", "text_from_bytes", "budget", "reserve", "__reserve_checked", "validate_limits", "byte_at", "int_is_binary64", "jcs_number", "utf16_units", "compare_units", "sort_by_units"],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_CBOR_PARSE_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_CBOR_PARSE_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_CBOR_PARSE_MODULE,
        alias: CORE_ENCODING_CBOR_PARSE_ALIAS,
        path: "Core/Encoding/CborParse.jet",
        owned_members: &["parse", "narrow_bits", "widen_float"],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_CBOR_STREAM_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_STREAM_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_CBOR_STREAM_ALIAS,
        alias: CORE_ENCODING_CBOR_STREAM_ALIAS,
        path: "Core/Encoding/CborStream.jet",
        owned_members: &["reader", "writer"],
        impl_types: &[],
    },
};
pub const CORE_FILES_PATH_COMPONENTS_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: "core.files",
    source: CoreSourceModule {
        module: "__core_files_path_components",
        alias: "__core_files_path_components",
        path: "Core/Files/Runtime/KernelPathComponents.jet",
        owned_members: &["from_text", "is_absolute", "components", "components_span", "next", "next_back", "parent_span", "strip_prefix_span", "owned_span", "push", "main_separator"],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_XML_ENGINE_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_XML_ENGINE_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_XML_ENGINE_MODULE,
        alias: CORE_ENCODING_XML_ENGINE_ALIAS,
        path: "Core/Encoding/XmlEngine.jet",
        owned_members: &["clean", "parse_document", "parse_document_with", "parse_document_bytes_with", "render_document", "render_document_bytes", "canonical_document", "document_root", "expanded_name_parts", "lookup_attribute", "element_content", "project_document_for_decode", "stream_event_value"],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_XML_KERNEL_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_XML_KERNEL_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_XML_KERNEL_MODULE,
        alias: CORE_ENCODING_XML_KERNEL_ALIAS,
        path: "Core/Encoding/XmlKernel.jet",
        owned_members: &["parse_document", "parse_document_with", "parse_document_bytes_with", "render_document", "render_document_bytes", "canonical_document", "document_root", "expanded_name_parts", "lookup_attribute", "element_content", "project_document_for_decode", "public_error", "parse_options", "render_encoding", "lexical_policy", "canonical_options", "field_error"],
        impl_types: &[],
    },
};
pub const CORE_CRYPTO_JETC_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_CRYPTO_JETC_OWNER,
    source: CoreSourceModule {
        module: CORE_CRYPTO_JETC_MODULE,
        alias: CORE_CRYPTO_JETC_ALIAS,
        path: "Core/Crypto/Jetc.jet",
        owned_members: &[],
        impl_types: &[],
    },
};

pub const CORE_CRYPTO_CT_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: "core.crypto",
    source: CoreSourceModule {
        module: "__core_crypto_ct",
        alias: "__core_crypto_ct",
        path: "Core/Crypto/ExpertCt.jet",
        owned_members: &[],
        impl_types: &[],
    },
};

/// All compiler-private source parts. Keep this separate from
/// `CoreModuleExports::CORE_SOURCE_MODULES`: adding a row there would make the
/// part publicly importable.
pub const CORE_PRIVATE_SOURCE_PARTS: &[CorePrivateSourcePart] =
    &[CORE_TEXT_STRING_SOURCE_PART, CORE_TEXT_SEGMENTATION_TABLES_SOURCE_PART, CORE_ENCODING_STREAM_SOURCE_PART, CORE_ENCODING_CBOR_PARSE_SOURCE_PART, CORE_FILES_PATH_COMPONENTS_SOURCE_PART, CORE_ENCODING_CBOR_STREAM_SOURCE_PART, CORE_ENCODING_XML_ENGINE_SOURCE_PART, CORE_ENCODING_XML_KERNEL_SOURCE_PART, CORE_CRYPTO_JETC_SOURCE_PART, CORE_CRYPTO_CT_SOURCE_PART];

pub fn core_private_source_part(owner: &str) -> Option<&'static CorePrivateSourcePart> {
    CORE_PRIVATE_SOURCE_PARTS
        .iter()
        .find(|part| part.owner == owner)
}

pub fn core_private_source_part_by_module(module: &str) -> Option<&'static CorePrivateSourcePart> {
    CORE_PRIVATE_SOURCE_PARTS
        .iter()
        .find(|part| part.source.module == module)
}

pub fn core_private_source_part_by_alias(alias: &str) -> Option<&'static CorePrivateSourcePart> {
    CORE_PRIVATE_SOURCE_PARTS
        .iter()
        .find(|part| part.source.alias == alias)
}

// BEGIN GENERATED CORE RUNTIME PARTS
// Generated by Tools/agent/check-core-surface-ledger.mjs --write from Core/*/Runtime/**/*.jet; do not edit.
pub const CORE_RUNTIME_PARTS: &[CoreSourceModule] = &[
    CoreSourceModule { module: "core.files.runtime.kernel_path_components", alias: "__core_files_runtime_kernel_path_components", path: "Core/Files/Runtime/KernelPathComponents.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.prelude.runtime.kernel_fixed_arithmetic", alias: "__core_prelude_runtime_kernel_fixed_arithmetic", path: "Core/Prelude/Runtime/KernelFixedArithmetic.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.assets", alias: "__core_text_runtime_assets_assets", path: "Core/Text/Runtime/Assets/Assets.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_alphabetic", alias: "__core_text_runtime_assets_unicode_alphabetic", path: "Core/Text/Runtime/Assets/UnicodeAlphabetic.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_case_ignorable", alias: "__core_text_runtime_assets_unicode_case_ignorable", path: "Core/Text/Runtime/Assets/UnicodeCaseIgnorable.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_cased", alias: "__core_text_runtime_assets_unicode_cased", path: "Core/Text/Runtime/Assets/UnicodeCased.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_ccc", alias: "__core_text_runtime_assets_unicode_ccc", path: "Core/Text/Runtime/Assets/UnicodeCcc.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_compose_pairs", alias: "__core_text_runtime_assets_unicode_compose_pairs", path: "Core/Text/Runtime/Assets/UnicodeComposePairs.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_composition_exclusions", alias: "__core_text_runtime_assets_unicode_composition_exclusions", path: "Core/Text/Runtime/Assets/UnicodeCompositionExclusions.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_decomp_index", alias: "__core_text_runtime_assets_unicode_decomp_index", path: "Core/Text/Runtime/Assets/UnicodeDecompIndex.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_decomp_pool", alias: "__core_text_runtime_assets_unicode_decomp_pool", path: "Core/Text/Runtime/Assets/UnicodeDecompPool.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_default_ignorable", alias: "__core_text_runtime_assets_unicode_default_ignorable", path: "Core/Text/Runtime/Assets/UnicodeDefaultIgnorable.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_east_asian_width", alias: "__core_text_runtime_assets_unicode_east_asian_width", path: "Core/Text/Runtime/Assets/UnicodeEastAsianWidth.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_emoji", alias: "__core_text_runtime_assets_unicode_emoji", path: "Core/Text/Runtime/Assets/UnicodeEmoji.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_emoji_presentation", alias: "__core_text_runtime_assets_unicode_emoji_presentation", path: "Core/Text/Runtime/Assets/UnicodeEmojiPresentation.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_extended_pictographic", alias: "__core_text_runtime_assets_unicode_extended_pictographic", path: "Core/Text/Runtime/Assets/UnicodeExtendedPictographic.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_fold_index", alias: "__core_text_runtime_assets_unicode_fold_index", path: "Core/Text/Runtime/Assets/UnicodeFoldIndex.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_fold_pool", alias: "__core_text_runtime_assets_unicode_fold_pool", path: "Core/Text/Runtime/Assets/UnicodeFoldPool.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_general_category", alias: "__core_text_runtime_assets_unicode_general_category", path: "Core/Text/Runtime/Assets/UnicodeGeneralCategory.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_grapheme_break", alias: "__core_text_runtime_assets_unicode_grapheme_break", path: "Core/Text/Runtime/Assets/UnicodeGraphemeBreak.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_incb", alias: "__core_text_runtime_assets_unicode_incb", path: "Core/Text/Runtime/Assets/UnicodeIncb.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_letter", alias: "__core_text_runtime_assets_unicode_letter", path: "Core/Text/Runtime/Assets/UnicodeLetter.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_lower_index", alias: "__core_text_runtime_assets_unicode_lower_index", path: "Core/Text/Runtime/Assets/UnicodeLowerIndex.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_lower_pool", alias: "__core_text_runtime_assets_unicode_lower_pool", path: "Core/Text/Runtime/Assets/UnicodeLowerPool.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_lowercase", alias: "__core_text_runtime_assets_unicode_lowercase", path: "Core/Text/Runtime/Assets/UnicodeLowercase.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_name_index", alias: "__core_text_runtime_assets_unicode_name_index", path: "Core/Text/Runtime/Assets/UnicodeNameIndex.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_numeric", alias: "__core_text_runtime_assets_unicode_numeric", path: "Core/Text/Runtime/Assets/UnicodeNumeric.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_sentence_break", alias: "__core_text_runtime_assets_unicode_sentence_break", path: "Core/Text/Runtime/Assets/UnicodeSentenceBreak.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_simple_fold", alias: "__core_text_runtime_assets_unicode_simple_fold", path: "Core/Text/Runtime/Assets/UnicodeSimpleFold.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_title_index", alias: "__core_text_runtime_assets_unicode_title_index", path: "Core/Text/Runtime/Assets/UnicodeTitleIndex.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_title_pool", alias: "__core_text_runtime_assets_unicode_title_pool", path: "Core/Text/Runtime/Assets/UnicodeTitlePool.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_upper_index", alias: "__core_text_runtime_assets_unicode_upper_index", path: "Core/Text/Runtime/Assets/UnicodeUpperIndex.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_upper_pool", alias: "__core_text_runtime_assets_unicode_upper_pool", path: "Core/Text/Runtime/Assets/UnicodeUpperPool.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_uppercase", alias: "__core_text_runtime_assets_unicode_uppercase", path: "Core/Text/Runtime/Assets/UnicodeUppercase.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_white_space", alias: "__core_text_runtime_assets_unicode_white_space", path: "Core/Text/Runtime/Assets/UnicodeWhiteSpace.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.assets.unicode_word_break", alias: "__core_text_runtime_assets_unicode_word_break", path: "Core/Text/Runtime/Assets/UnicodeWordBreak.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.kernel_text_normal", alias: "__core_text_runtime_kernel_text_normal", path: "Core/Text/Runtime/KernelTextNormal.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.kernel_text_segments", alias: "__core_text_runtime_kernel_text_segments", path: "Core/Text/Runtime/KernelTextSegments.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.kernel_text_width", alias: "__core_text_runtime_kernel_text_width", path: "Core/Text/Runtime/KernelTextWidth.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.kernel_unicode_string", alias: "__core_text_runtime_kernel_unicode_string", path: "Core/Text/Runtime/KernelUnicodeString.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.top_unicode_tables", alias: "__core_text_runtime_top_unicode_tables", path: "Core/Text/Runtime/TopUnicodeTables.jet", owned_members: &[], impl_types: &[] },
    CoreSourceModule { module: "core.text.runtime.unicode_assets", alias: "__core_text_runtime_unicode_assets", path: "Core/Text/Runtime/UnicodeAssets.jet", owned_members: &[], impl_types: &[] },
];
// END GENERATED CORE RUNTIME PARTS

/// The runtime unit whose identity is exactly `module`.
pub fn core_runtime_part(module: &str) -> Option<&'static CoreSourceModule> {
    CORE_RUNTIME_PARTS.iter().find(|part| part.module == module)
}

/// The runtime unit with the reserved module alias `alias`.
pub fn core_runtime_part_by_alias(alias: &str) -> Option<&'static CoreSourceModule> {
    CORE_RUNTIME_PARTS.iter().find(|part| part.alias == alias)
}

/// The runtime unit a `core.*` import path names, with the item path after
/// the unit when the path selects one member (`core.a.runtime.u.Item`). The
/// longest unit wins, so a nested unit (`core.text.runtime.assets.x`) is never
/// read as member `x` of a sibling unit `core.text.runtime.assets`.
pub fn core_runtime_import(path: &str) -> Option<(&'static CoreSourceModule, Option<&str>)> {
    CORE_RUNTIME_PARTS
        .iter()
        .filter_map(|part| {
            let rest = path.strip_prefix(part.module)?;
            if rest.is_empty() {
                return Some((part, None));
            }
            let item = rest.strip_prefix('.')?;
            (!item.is_empty()).then_some((part, Some(item)))
        })
        .max_by_key(|(part, _)| part.module.len())
}

/// E1001 for a program outside Core that names the Core-private
/// `core.<area>.runtime` namespace. Loader and Sema both call this, so the
/// module path and the selective-import path report the same refusal.
pub fn core_private_unit_diagnostic(path: &str, span: Span) -> Option<Diagnostic> {
    let mut segments = path.split('.');
    let (Some("core"), Some(area), Some("runtime")) =
        (segments.next(), segments.next(), segments.next())
    else {
        return None;
    };
    let public = format!("core.{area}");
    let fix = if crate::Syntax::is_known_core_module(&public) {
        format!("use the public module `{public}` instead")
    } else {
        format!("use one of: {}", crate::Syntax::core_modules_list())
    };
    Some(Diagnostic::error(
        "E1001",
        format!("`{path}` is private to Core"),
        "`core.<area>.runtime` units implement the Core library; only Core sources import them"
            .to_string(),
        fix,
        Some(span),
    ))
}
