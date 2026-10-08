//! Compiler-private Core source parts that are not public source modules.
//!
//! A private part is attached to a public owner for loading and source
//! identity, but it deliberately does not participate in `CORE_MODULE_NAMES`,
//! public source-module lookup, or the Core surface ledger.

use crate::CoreModuleExports::CoreSourceModule;

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
        path: "Core/text/string.jet",
        owned_members: &[],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_STREAM_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_STREAM_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_STREAM_MODULE,
        alias: CORE_ENCODING_STREAM_ALIAS,
        path: "Core/encoding/stream.jet",
        owned_members: &["json_reader", "json_writer", "jsonl_reader", "jsonl_writer", "object_entries", "object_from_entries", "bytes_payload", "typed_number", "typed_string", "text_from_bytes", "budget", "reserve", "__reserve_checked", "validate_limits", "byte_at", "int_is_binary64", "jcs_number", "utf16_units", "compare_units", "sort_by_units"],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_CBOR_PARSE_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_CBOR_PARSE_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_CBOR_PARSE_MODULE,
        alias: CORE_ENCODING_CBOR_PARSE_ALIAS,
        path: "Core/encoding/cbor_parse.jet",
        owned_members: &["parse", "narrow_bits", "widen_float"],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_CBOR_STREAM_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_STREAM_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_CBOR_STREAM_ALIAS,
        alias: CORE_ENCODING_CBOR_STREAM_ALIAS,
        path: "Core/encoding/cbor_stream.jet",
        owned_members: &["reader", "writer"],
        impl_types: &[],
    },
};
pub const CORE_FILES_PATH_COMPONENTS_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: "core.files",
    source: CoreSourceModule {
        module: "__core_files_path_components",
        alias: "__core_files_path_components",
        path: "Core/files/runtime/kernel_path_components.jet",
        owned_members: &["from_text", "is_absolute", "components", "components_span", "next", "next_back", "parent_span", "strip_prefix_span", "owned_span", "push", "main_separator"],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_XML_ENGINE_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_XML_ENGINE_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_XML_ENGINE_MODULE,
        alias: CORE_ENCODING_XML_ENGINE_ALIAS,
        path: "Core/encoding/xml_engine.jet",
        owned_members: &["clean", "parse_document", "parse_document_with", "parse_document_bytes_with", "render_document", "render_document_bytes", "canonical_document", "document_root", "expanded_name_parts", "lookup_attribute", "element_content", "project_document_for_decode", "stream_event_value"],
        impl_types: &[],
    },
};

pub const CORE_ENCODING_XML_KERNEL_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_ENCODING_XML_KERNEL_OWNER,
    source: CoreSourceModule {
        module: CORE_ENCODING_XML_KERNEL_MODULE,
        alias: CORE_ENCODING_XML_KERNEL_ALIAS,
        path: "Core/encoding/xml_kernel.jet",
        owned_members: &["parse_document", "parse_document_with", "parse_document_bytes_with", "render_document", "render_document_bytes", "canonical_document", "document_root", "expanded_name_parts", "lookup_attribute", "element_content", "project_document_for_decode", "public_error", "parse_options", "render_encoding", "lexical_policy", "canonical_options", "field_error"],
        impl_types: &[],
    },
};
/// All compiler-private source parts. Keep this separate from
/// `CoreModuleExports::CORE_SOURCE_MODULES`: adding a row there would make the
/// part publicly importable.
pub const CORE_PRIVATE_SOURCE_PARTS: &[CorePrivateSourcePart] =
    &[CORE_TEXT_STRING_SOURCE_PART, CORE_ENCODING_STREAM_SOURCE_PART, CORE_ENCODING_CBOR_PARSE_SOURCE_PART, CORE_FILES_PATH_COMPONENTS_SOURCE_PART, CORE_ENCODING_CBOR_STREAM_SOURCE_PART, CORE_ENCODING_XML_ENGINE_SOURCE_PART, CORE_ENCODING_XML_KERNEL_SOURCE_PART];

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
