// The compiler/runtime seam owns this source so every in-process adapter calls
// one epoch. The AOT emitter embeds the same source once in its flat prelude.
include!("../../jet-codegen/src/Prelude/Core/TimeMonotonic.rs");
