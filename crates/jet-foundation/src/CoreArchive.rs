#![allow(dead_code)]

// Keep one audited archive ABI kernel across every execution tier. The ordinary
// Jet package is the public authority; this include makes its internal kernel
// available to the compiler seam without adding a codec-crate dependency.
include!("../../../corelib/core.archive/pkgs/archive/src/lib.rs");
