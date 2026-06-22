# Plan: Checked IR boundary before codegen

**Status:** planned. No owner decision required for the first internal IR slice.

## Goal

Move codegen from "AST plus side registries" toward a checked representation that
contains only sema-approved facts.

## Implementation Steps

1. Define a minimal HIR/TIR module for functions, structs, enums, and resolved names.
2. Lower from AST to checked IR after sema registration/type checking.
3. Make codegen consume checked IR for one narrow feature slice first.
4. Move semantic-ish codegen checks back into sema as diagnostics.
5. Expand coverage module by module.

## First Slice

Start with simple functions and local expressions that do not involve ownership effects,
FFI, comptime, or package boundaries.

## Verification

- Golden examples compile identically.
- Add a generated-Rust diff test for the first slice if practical.
- `cargo check` and targeted golden suite.
