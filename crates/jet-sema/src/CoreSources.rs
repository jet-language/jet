//! Body text of the source-owned Core modules: the public rows of
//! `jet_foundation::CoreModuleExports::CORE_SOURCE_MODULES` plus the private
//! parts in `jet_foundation::CoreSourceParts`.
//!
//! The metadata rows stay in jet-foundation; the bodies live here, in the
//! first crate that reads them, so editing a Core body rebuilds jet-sema and
//! its dependents but never jet-foundation, the lexer, the parser or comptime.

use jet_foundation::CoreSourceParts::CORE_TEXT_STRING_MODULE;

include!("CoreSourceTexts.rs");

const CORE_PRIVATE_SOURCE_TEXTS: &[(&str, &str)] =
    &[(CORE_TEXT_STRING_MODULE, include_str!("../../../Core/text/string.jet"))];

/// Source text of the Core source module (public or private part) whose
/// `CoreSourceModule::module` is `module`.
pub fn core_source_text(module: &str) -> Option<&'static str> {
    CORE_SOURCE_TEXTS
        .iter()
        .chain(CORE_PRIVATE_SOURCE_TEXTS)
        .find(|(name, _)| *name == module)
        .map(|(_, text)| *text)
}
