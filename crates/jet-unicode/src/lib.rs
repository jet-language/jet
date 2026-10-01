//! The one compiled copy of Jet's generated Unicode tables.
//!
//! The AOT prelude embeds the same file as text; every in-binary host
//! (jet-foundation, the comptime evaluator, the JIT hosts) reads these
//! statics instead of compiling its own copy. The file never changes between
//! Unicode releases, so this leaf crate stays cached across rebuilds.

include!("../../jet-codegen/src/Prelude/CoreLib/Top/UnicodeTables.rs");
