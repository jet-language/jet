// D-DEP-GC1=A: the one collector source is assembled with one platform adapter.
//
// `__gc_core.rs` contains the alloc/core collector algorithms.  The adapter is
// selected only by the runtime crate; emitted programs inline the same core
// and their checked target adapter directly (see Codegen's GC assembly).

#[cfg(target_os = "none")]
mod __gc_platform {
    include!("__gc_portable.rs");
}

#[cfg(not(target_os = "none"))]
mod __gc_platform {
    include!("__gc_host.rs");
}

use __gc_platform::*;
include!("__gc_core.rs");
