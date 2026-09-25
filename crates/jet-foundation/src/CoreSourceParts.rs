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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorePrivateSourcePart {
    /// Public source module that schedules this part.
    pub owner: &'static str,
    /// Private source-module metadata. Its `module` is never a public Core
    /// module name and its `owned_members` set is intentionally empty.
    pub source: CoreSourceModule,
}

pub const CORE_TEXT_STRING_SOURCE_PART: CorePrivateSourcePart = CorePrivateSourcePart {
    owner: CORE_TEXT_STRING_OWNER,
    source: CoreSourceModule {
        module: CORE_TEXT_STRING_MODULE,
        alias: CORE_TEXT_STRING_ALIAS,
        path: "Core/text/string.jet",
        owned_members: &[],
        source: include_str!("../../../Core/text/string.jet"),
    },
};

/// All compiler-private source parts. Keep this separate from
/// `CoreModuleExports::CORE_SOURCE_MODULES`: adding a row there would make the
/// part publicly importable.
pub const CORE_PRIVATE_SOURCE_PARTS: &[CorePrivateSourcePart] =
    &[CORE_TEXT_STRING_SOURCE_PART];

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
