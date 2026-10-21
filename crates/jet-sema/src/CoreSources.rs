//! Body text of the source-owned Core modules: the public rows of
//! `jet_foundation::CoreModuleExports::CORE_SOURCE_MODULES`, the private
//! parts in `jet_foundation::CoreSourceParts`, and its runtime units.
//!
//! The metadata rows stay in jet-foundation; the bodies live here, in the
//! first crate that reads them, so editing a Core body rebuilds jet-sema and
//! its dependents but never jet-foundation, the lexer, the parser or comptime.

use jet_foundation::CoreSourceParts::{CORE_TEXT_STRING_MODULE, CORE_ENCODING_STREAM_MODULE, CORE_ENCODING_CBOR_PARSE_MODULE, CORE_ENCODING_CBOR_STREAM_ALIAS, CORE_FILES_PATH_COMPONENTS_SOURCE_PART, CORE_ENCODING_XML_ENGINE_MODULE, CORE_ENCODING_XML_KERNEL_MODULE, CORE_CRYPTO_JETC_MODULE};

include!("CoreSourceTexts.rs");

const CORE_PRIVATE_SOURCE_TEXTS: &[(&str, &str)] = &[
    (CORE_TEXT_STRING_MODULE, include_str!("../../../Core/Text/String.jet")),
    ("__core_text_segmentation_tables", include_str!("../../../Core/Text/SegmentationTables.jet")),
    (CORE_ENCODING_STREAM_MODULE, include_str!("../../../Core/Encoding/Stream.jet")),
    (CORE_ENCODING_CBOR_PARSE_MODULE, include_str!("../../../Core/Encoding/CborParse.jet")),
    (CORE_FILES_PATH_COMPONENTS_SOURCE_PART.source.module, include_str!("../../../Core/Files/Runtime/KernelPathComponents.jet")),
    (CORE_ENCODING_CBOR_STREAM_ALIAS, include_str!("../../../Core/Encoding/CborStream.jet")),
    (CORE_ENCODING_XML_ENGINE_MODULE, include_str!("../../../Core/Encoding/XmlEngine.jet")),
    (CORE_ENCODING_XML_KERNEL_MODULE, include_str!("../../../Core/Encoding/XmlKernel.jet")),
    (CORE_CRYPTO_JETC_MODULE, include_str!("../../../Core/Crypto/Jetc.jet")),
    ("__core_crypto_ct", include_str!("../../../Core/Crypto/ExpertCt.jet")),
];

/// Source text of the Core source module (public, private part, or runtime
/// unit) whose `CoreSourceModule::module` is `module`.
pub fn core_source_text(module: &str) -> Option<&'static str> {
    CORE_SOURCE_TEXTS
        .iter()
        .chain(CORE_PRIVATE_SOURCE_TEXTS)
        .chain(CORE_RUNTIME_SOURCE_TEXTS)
        .find(|(name, _)| *name == module)
        .map(|(_, text)| *text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_private_core_source_part_has_body_text() {
        for part in jet_foundation::CoreSourceParts::CORE_PRIVATE_SOURCE_PARTS {
            assert!(
                super::core_source_text(part.source.module).is_some_and(|text| !text.is_empty()),
                "private Core source part {} has no body text ({})",
                part.source.module,
                part.source.path,
            );
        }
    }

    #[test]
    fn crypto_chunk_engine_is_private_and_embedded() {
        use jet_foundation::CoreSourceParts::core_private_source_part_by_module;
        let part = core_private_source_part_by_module("__core_crypto_jetc")
            .expect("crypto chunk engine private part");
        assert_eq!(part.owner, "core.crypto");
        assert_eq!(part.source.alias, "__core_crypto_jetc");
        assert_eq!(part.source.path, "Core/Crypto/Jetc.jet");
        assert!(part.source.owned_members.is_empty());
        assert_eq!(super::core_source_text(part.source.module),
            Some(include_str!("../../../Core/Crypto/Jetc.jet")));
        assert!(jet_foundation::CoreModuleExports::core_source_module(part.source.module).is_none());
        assert!(!jet_foundation::CoreModuleExports::CORE_MODULE_NAMES.contains(&part.source.module));
    }

    #[test]
    fn crypto_private_part_matches_generated_cli_projection() {
        let projection = include_str!("../../../Compiler/JetCli/Source/Cli/CoreSources.jet");
        assert!(projection.contains(
            "JetDriverCoreSource{module_name: \"__core_crypto_jetc\", alias: \"__core_crypto_jetc\", path: \"Core/Crypto/jetc.jet\", owner: \"core.crypto\", owned_members: [String]{}, source: \"\"}"
        ));
    }
}
