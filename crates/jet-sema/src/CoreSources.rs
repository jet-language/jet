//! Body text of the source-owned Core modules: the public rows of
//! `jet_foundation::CoreModuleExports::CORE_SOURCE_MODULES` plus the private
//! parts in `jet_foundation::CoreSourceParts`.
//!
//! The metadata rows stay in jet-foundation; the bodies live here, in the
//! first crate that reads them, so editing a Core body rebuilds jet-sema and
//! its dependents but never jet-foundation, the lexer, the parser or comptime.

<<<<<<< HEAD
use jet_foundation::CoreSourceParts::{CORE_TEXT_STRING_MODULE, CORE_ENCODING_STREAM_MODULE};
||||||| d4aa52706
use jet_foundation::CoreSourceParts::CORE_TEXT_STRING_MODULE;
=======
use jet_foundation::CoreSourceParts::{CORE_TEXT_STRING_MODULE, CORE_ENCODING_STREAM_MODULE, CORE_ENCODING_CBOR_PARSE_MODULE};
>>>>>>> 3d6b63fe3

include!("CoreSourceTexts.rs");

<<<<<<< HEAD
const CORE_PRIVATE_SOURCE_TEXTS: &[(&str, &str)] = &[
    (CORE_TEXT_STRING_MODULE, include_str!("../../../Core/text/string.jet")),
    (CORE_ENCODING_STREAM_MODULE, include_str!("../../../Core/encoding/stream.jet")),
];
||||||| d4aa52706
const CORE_PRIVATE_SOURCE_TEXTS: &[(&str, &str)] =
    &[(CORE_TEXT_STRING_MODULE, include_str!("../../../Core/text/string.jet"))];
=======
const CORE_PRIVATE_SOURCE_TEXTS: &[(&str, &str)] = &[
    (CORE_TEXT_STRING_MODULE, include_str!("../../../Core/text/string.jet")),
    (CORE_ENCODING_STREAM_MODULE, include_str!("../../../Core/encoding/stream.jet")),
    (CORE_ENCODING_CBOR_PARSE_MODULE, include_str!("../../../Core/encoding/cbor_parse.jet")),
];
>>>>>>> 3d6b63fe3

/// Source text of the Core source module (public or private part) whose
/// `CoreSourceModule::module` is `module`.
pub fn core_source_text(module: &str) -> Option<&'static str> {
    CORE_SOURCE_TEXTS
        .iter()
        .chain(CORE_PRIVATE_SOURCE_TEXTS)
        .find(|(name, _)| *name == module)
        .map(|(_, text)| *text)
}
