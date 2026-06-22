# Plan: Clarify the Rust-backed backend story

**Status:** planned. No language decision required for documentation; native-backend work
remains roadmap.

## Goal

Tell the truth: Jet currently compiles through generated Rust and `rustc`. Position that
as a pragmatic bootstrap while documenting what a future native backend would require.

## Implementation Steps

1. Update architecture docs to distinguish today's Rust backend from a future backend
   boundary.
2. Document rustc as a required toolchain component for native builds.
3. Identify which codegen assumptions tie Jet types directly to Rust/std.
4. Create a backend-readiness checklist: checked IR, runtime ABI, stdlib lowering,
   diagnostics boundary, source maps.
5. Avoid peer-to-Rust/C/C++ claims unless scoped to language goals rather than backend
   independence.

## Verification

- Docs grep for "native backend", "transpiler", "rustc", "backend swappable".
- `cargo check` unchanged; this is documentation/roadmap work first.
