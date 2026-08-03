#![allow(dead_code)]

// Keep one archive implementation across every execution tier. The package
// source is the authority; this module makes it available to the compiler
// seam without adding a dependency from Foundation to a codec crate.
include!("../../../corelib/core.archive/pkgs/archive/src/lib.rs");
