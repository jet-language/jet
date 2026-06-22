# Plan: Build-time I/O completion (`embed_bytes`)

**Status:** planned. D-CTIO1 is ratified. Blocked in practice by implementing D-SG9 sized
integers, especially `U8`.

## Goal

Finish D-CTIO1 by adding `embed_bytes(path) -> [U8]` and keeping path safety aligned with
the already-hardened `embed_file`.

## Implementation Steps

1. Wait for or implement the D-SG9 `U8` type support required by the return type.
2. Reuse the existing literal-path and no-escape validation from `embed_file`.
3. Add `embed_bytes` to parser/sema/comptime evaluation.
4. Lower or interpret bytes as `[U8]` without widening to `[Int]`.
5. Add diagnostics for non-literal paths and project-root escapes.

## Verification

- UI snapshots for bad path forms.
- Golden example embedding a small binary fixture.
- Comptime tests proving exact byte values.
