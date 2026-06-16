# E2-M9 — First-party library ring

**Status:** draft — **blocked on D-LR1…D-LR3** (Group M9).
**Depends on:** E2-M6 (clean API ergonomics), E2-M8 (versions + API diff enforce
the ring). sqlite in the ring depends on E2-M14 (C FFI) timing (D-LR2).
**Error codes:** E27xx block, shared across ring packages (claim in
docs/spec/diagnostics.md).

## Goal

Ship the batteries that make Jet feel complete without bloating core std. The
target: real examples replace common Python/Node/Go scripts — CSV cleanup, TOML
config rewrite, log processing, archive unpacking, hash verification.

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-LR1 | First-wave order | **A** — csv/toml/log/time first, then regex/archive/db | A | ✅ ratified 2026-06-16 — ship ALL ring libs in Epoch 2 |
| D-LR2 | sqlite | **A** — via E2-M14 C FFI when ready, else defer db ring | A | ✅ ratified 2026-06-16 — A: sqlite via C FFI now / pure-Jet later |
| D-LR3 | crypto surface | **A** — vetted hashes/HMAC/RNG only | A | ✅ ratified 2026-06-16 — crypto as broad as safely possible (vetted impls only) |
| D-LR4 | YAML library | — | — | ✅ ratified 2026-06-16 — B: add `jet.yaml` in wave 1 (owner chose B against prior rec A) |

## First wave (order per D-LR1)

- `jet.csv` — read/write with header handling and typed columns.
- `jet.toml` — parse/emit; round-trips a config rewrite.
- `jet.log` — structured logging (the observability minimum; pairs with E2-M12).
- `jet.time` — calendar/timezone package.
- `jet.regex` — matching/capture (no PCRE footguns by default).
- `jet.crypto` — vetted hashes/HMAC/random **primitives only** (D-LR3); never
  hand-rolled, never symmetric ciphers or TLS in this package.
- `jet.archive` — zip/tar/gzip.
- `jet.db` — base abstractions; sqlite first **if** E2-M14/runtime allow (D-LR2).

## Rules (the ring's quality bar)

- Same bar as the compiler: examples, docs, diagnostics, benchmarks where
  relevant.
- APIs prefer boring, unsurprising names.
- Fallible operations return `T ? E` (use E2-M6 conversion).
- No hidden global mutable state.
- "Pay for what you import/call" stays a design constraint (I8).
- Versions and public-API diffs are enforced by E2-M8.

## Diagnostics to register

- **E2701** malformed input with location (e.g. CSV row/column, TOML line) —
  what/why/fix, not a parser dump.
- **E2702** crypto misuse caught at the API boundary (e.g. reusing a nonce,
  truncated key) where statically detectable.
- **L2701** advisory: regex likely catastrophic-backtracking; suggest an anchor.

## Examples & tests

- `examples/features/39_csv.jet` — clean a messy CSV.
- `examples/features/40_toml.jet` — rewrite a config file.
- `examples/features/41_log.jet` — process a log stream.
- `examples/features/42_archive.jet` — unpack an archive.
- `examples/features/43_hash.jet` — verify a file hash.
- Each package: generated docs + tested doctests (depends on E2-M11 doctest
  surface; until then, example-backed golden tests).

## Out of scope

- A web framework, ORM, or async drivers (E2-V5/V7).
- Symmetric encryption / TLS in `jet.crypto` (TLS lives in E2-M10 via FFI).
- YAML/XML in the first wave (add only on evidence).
- Pulling any of these into core std (they stay `jet.*` packages).

## Exit criteria

- Real examples replace common Python/Node/Go scripts (the five above run).
- Package docs are generated and examples are tested.
- First-party versions and API diffs are enforced by E2-M8.
- Fallible APIs return `T ? E`; no hidden global state.
- `nix develop -c cargo test` green.
