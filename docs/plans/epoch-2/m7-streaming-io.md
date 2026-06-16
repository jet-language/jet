# E2-M7 — Streaming I/O and resources

**Status:** draft — **blocked on D-IO1…D-IO3** (Group M7). The transactional
rollback ballot (Group 18, D-TXN1…3) is *adjacent* — RAII cleanup here is the
recommended answer to the owner-flagged "leave the world consistent on error"
need; decide D-TXN in this window or defer.
**Depends on:** E2-M6 (error conversion integrates with streaming errors), S63
(RAII cleanup, ratified). Unblocks E2-M10 (HTTP over streaming I/O).
**Error codes:** E25xx block (claim in docs/spec/diagnostics.md).

## Goal

Replace whole-file-only APIs with production I/O while keeping cleanup
automatic. The headline: a large-file transform that runs in bounded memory and
releases every resource on every exit path — including `?` early returns.

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-IO1 | Path handling | **A** — `std.path` helper module (not a first-class `Path` type yet) | A | ✅ ratified 2026-06-16 — A: `std.path` helper module (invest in ergonomics) |
| D-IO2 | Cleanup surface | **A** — RAII handle types (S63), drop on scope exit | A | OPEN — pending confirm of RAII-A |
| D-IO3 | Keep whole-file `fs.read`/`fs.write` | **A** — keep as sugar over handles | A | ✅ ratified 2026-06-16 — A: keep whole-file `fs.read`/`fs.write` helpers |
| D-FS2 | Game-loop / input polling | — | — | ✅ ratified 2026-06-16 — A&B: ship game-loop example AND `poll_input` helper |
| D-TXN1…3 (adjacent) | `transact` block | **A/A/A**, or defer | defer; RAII is the model | — |

## Surface

```jet
// RAII handles close on every exit path, including `?` (D-IO2 / S63):
fn copy(src: String, dst: String) -> Unit ? {
    val input  = files.open(src)?;
    val output = files.create(dst)?;
    for line in input.lines() {        // streaming, bounded memory
        output.write_line(line)?;      // if this fails, both handles close
    }
    ok(unit)
}

// Whole-file sugar stays (D-IO3):
val text = fs.read("config.toml")?;
```

## Scope

- **Handles + Reader/Writer.** `File` handle, `Reader`, `Writer`, buffered
  variants. Line iteration, byte chunks, `seek` where the platform allows.
- **Paths (D-IO1).** A `std.path` helper module (join/parent/extension/normalize)
  rather than a first-class `Path` type this epoch.
- **RAII cleanup (D-IO2 / S63).** Cleanup runs at scope end on *every* path; make
  the S63 contract user-facing with docs and tests. Runtime errors name the
  resource and operation ("could not write to `out.csv`: disk full").
- **Streaming std streams.** `stdin`/`stdout`/`stderr` as streaming
  readers/writers.
- **Error conversion.** I/O errors integrate with E2-M6's **`Fallible`** trait so `?`
  works in `-> T ?` functions.
- **`transact` (adjacent, D-TXN).** If ratified, a `transact { … }` block rolls
  back in-memory mutation when an inner `?` short-circuits; doing I/O inside is a
  compile error (E2-TXN). Recommendation is to satisfy the cleanup need with RAII
  first and treat `transact` as a separate, owner-gated decision.

## Diagnostics to register

- **E2501** use of a handle after it was closed/moved (reuses M2 ownership voice).
- **E2502** runtime I/O error report names resource + operation (what/why/fix).
- **E2503** (if D-TXN ratified) I/O performed inside a `transact` block.
- **L2501** advisory: whole-file read of a large file; consider streaming.

## Examples & tests

- `examples/features/37_stream.jet` — large-file transform in bounded memory.
- `tests/io/cleanup_on_error.txt` — proves both handles close when `?` fires.
- ui fixtures for E2501/E2502 (and E2503 if `transact` lands).

## Out of scope

- Async I/O (E2-V5 lock).
- A first-class `Path` type (revisit post-epoch if `std.path` proves limiting).
- Memory-mapped files, `io_uring`, platform-specific fast paths.
- Network sockets (E2-M10).

## Exit criteria

- Large-file transform runs with bounded memory.
- Resource cleanup happens on every exit path (tested with `?`-early-return).
- Runtime I/O errors name the resource and operation.
- Existing `fs.read`/`fs.write` APIs still work.
- `nix develop -c cargo test` green.
