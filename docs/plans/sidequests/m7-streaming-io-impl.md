# Sidequest: E2-M7 — Streaming I/O implementation

**Plan:** `docs/plans/epoch-2/m7-streaming-io.md`  
**Status:** all decisions ratified; ready to implement  
**Depends on:** E2-M6 (Fallible trait for I/O error `?` propagation)  
**Unblocks:** E2-M10 (HTTP over streaming I/O)

## Ratified decisions summary

| Decision | What to implement |
|---|---|
| D-IO1 | `std.path` helper module (join/parent/extension/normalize); NOT a first-class `Path` type |
| D-IO2 | RAII handles close on every exit path including `?`; S63 contract is user-facing |
| D-IO3 | Keep whole-file `fs.read`/`fs.write` as sugar over handles |
| D-FS2 | Ship game-loop example AND `poll_input` helper (both A&B ratified) |

## No amendments — plan is accurate as written

All four decisions landed as originally recommended. Implement directly from `m7-streaming-io.md`.

## Key implementation note

RAII cleanup (S63) must close on **every** exit path — including `?` early returns and panics.
The codegen must lower scope-exit cleanup to Rust `Drop` impls. Test with the `cleanup_on_error.txt`
fixture (both handles must close when `?` fires mid-loop).

## Diagnostics to register (E25xx)

E2501 (use-after-close/move), E2502 (runtime I/O error: names resource + operation), L2501 (large file advisory).
E2503 only if D-TXN1-3 are ratified (they are not; skip).

## Exit criteria

See `m7-streaming-io.md`. Key: large-file transform in bounded memory; both handles close on `?`.
`nix develop -c cargo test` green.
