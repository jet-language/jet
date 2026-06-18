# E2-M9 — First-party library ring

**Status:** all decisions ratified; ready to implement after M6 + M8.
**Depends on:** E2-M6 (clean API ergonomics), E2-M8 (versions + API diff enforce
the ring). sqlite in the ring depends on E2-M14 ✅ (C FFI done).
**Error codes:** E27xx block, shared across ring packages (claim in
docs/spec/diagnostics.md).
**Amendments:** D-LR4=B (jet.yaml in wave 1); D-DEP1 pattern for Rust-backed
libs; D-JSON1 coercion surfacing required. See §D-DEP1 and §D-JSON1 below.

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

## First wave (order per D-LR1 — ALL ship in Epoch 2)

Wave 1: `jet.csv`, `jet.toml`, `jet.yaml` (D-LR4=B), `jet.json`, `jet.log`, `jet.time`
Wave 2: `jet.regex`, `jet.crypto`, `jet.archive`, `jet.db` (sqlite via C FFI, D-LR2)

### D-DEP1: Rust-backed ring packages

Ring libs that need a Rust internal (e.g. fast YAML parser, regex engine) follow the
D-DEP1 FFI-wrapping pattern:

```jet
// In pkg.jet — pin the Rust crate:
[extern rust "some-crate@0.x.0"]

// In src/lib.jet — wrap the API surface:
@extern module some_crate { … }
```

This ships the Rust crate as a Jet package through the hangar store. No compiler crates (I6).
Document the pattern once in `docs/spec/packages.md` and reference it from each lib that uses it.

### D-JSON1: jet.json lenient coercion with surfacing

`jet.json.decode` coerces unambiguously (`"8080"` → `8080`, `"true"` → `true`).
The implementation MUST surface coercions — pick one mechanism:
- A: `jet.json.decode_verbose` returns `{ value: T, coercions: [Coercion] }` (preferred)
- C: sema advisory lint listing fields that will be coerced
Implement at least option A. Document and test it (golden test or unit test).

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
- Symmetric encryption / TLS in `jet.crypto` (TLS lives in E2-M10 as `jet.tls` package).
- Pulling any of these into core std (they stay `jet.*` packages).

## Exit criteria

- Real examples replace common Python/Node/Go scripts (the five above run).
- Package docs are generated and examples are tested.
- First-party versions and API diffs are enforced by E2-M8.
- Fallible APIs return `T ? E`; no hidden global state.
- `nix develop -c cargo test` green.
