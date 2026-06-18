# E2-M2 — Release policy, editions, and the epoch contract

**Status:** implemented 2026-06-16 — all five decisions ratified; edition marker
in `pkg.jet`, E2001/E2002/L2001 registered, version banner enriched, policy
doc at docs/spec/release-policy.md. Mostly docs + manifest plumbing; little codegen.
**Depends on:** E2-M1 (verified). Gates every later public-breaking milestone.
**Error codes:** E20xx block (claim in docs/spec/diagnostics.md as implemented).

## Goal

Write the promises an enterprise adopts *before* it depends on Jet. This
milestone produces a ratified compatibility/release policy and the smallest
machinery that enforces it: a project compatibility marker, a version banner
that states what the toolchain supports, and a clear diagnostic when a manifest
asks for a future the toolchain can't provide.

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| E2-D1 / D-REL1 | External versioning policy | **A** — normal SemVer until launch | A | ✅ ratified 2026-06-16 — A: normal SemVer forever |
| E2-D2 / D-REL2 | When (if ever) encoded Epoch SemVer flips on | **C** — separate launch after GA | C | ✅ ratified 2026-06-16 — never encode epoch version; owner controls version bumps manually |
| D-REL3 | Project compatibility marker | **A** — `[package].edition = "2026"` | `edition` field | ✅ ratified 2026-06-16 — A: `edition` field |
| D-REL4 | LTS window length | **C** — no LTS pre-GA, set at GA | none yet | ✅ ratified 2026-06-16 — C: no LTS pre-GA |
| D-REL5 | Who may run migrations | **A** — owner-approved `jet fix` + edition upgrade only | A | ✅ ratified 2026-06-16 — A: only `jet fix` + edition upgrade may migrate |

Substitute the owner's ratified option everywhere if it differs from the Rec.

## Scope

- **Compatibility levels.** Define, in docs/spec, exactly what may change in a
  patch, minor, major, epoch, and edition. Patch = bug/diagnostic-text fixes;
  minor = additive; major = breaking with migration; epoch = era storytelling;
  edition = opt-in per-project syntax compatibility.
- **Backward-compatibility guarantee.** A written promise that post-1.0 code in
  edition N keeps compiling on later toolchains that still support edition N.
- **Deprecation policy + migration window.** How a feature is marked deprecated,
  how long it survives, and how `jet fix` assists the move.
- **Edition marker.** `edition:` in the `payload: { … }` block of `pkg.jet`
  (`jet.toml` was retired — the manifest is Jet syntax, U1/U10). A toolchain
  advertises the editions it supports; an unsupported future edition is **E2001**.
- **Generated-code license.** State explicitly: generated Rust carries no
  additional license obligation from the compiler. This is product-critical and
  pure docs.
- **`jet --version` contract.** Prints compiler SemVer, supported language
  epoch/edition range, and std/registry compatibility (see E2-D1 example).
- **Migration authority.** Which tools may rewrite user code, and only on
  explicit request (D-REL5).

## Manifest & diagnostics

```jet
payload: {
    name:    "wordstats",
    version: "0.1.0",
    edition: "2026",
}
```

The implemented E2001 (rendered in the project diagnostic voice; the
manifest-level diagnostic carries no source span, matching E1206–E1213):

```
Error [E2001]: this package needs a newer Jet
 Why: editions opt a project into a specific era of Jet syntax. A newer edition can use syntax this compiler does not understand. This toolchain supports editions up to 2026, but `pkg.jet` asks for `2099`.
 Fix: upgrade with `jet upgrade`, or set `edition: "2026"` in `pkg.jet`.
```

Single-file `jet run file.jet` has **no** edition marker and always uses the
toolchain's newest stable edition (E2-V4 / I: single-file stays sacred).

## Diagnostics to register

- **E2001** manifest requests an unsupported edition/epoch (what/why/fix above).
- **E2002** deprecated-since item used past its window (names the replacement).
- **L2001** lint: feature deprecated in this edition; suggests `jet fix`.

## Examples & tests

- `tests/release/version_banner.txt` — golden `jet --version` output.
- `tests/release/edition_too_new.txt` — E2001 transcript.
- `tests/release/deprecation.txt` — E2002 + L2001 with the migration hint.
- A docs test that the compatibility table in docs/spec is internally consistent
  (every "breaking" milestone names the edition gate it needs).

## Out of scope

- Actually shipping an LTS branch (set the *policy*, not the branch).
- Encoded Epoch SemVer on the binary (E2-D2 = C defers it).
- Registry-side SemVer enforcement (that is E2-M8's API-diff work; this
  milestone only sets the *policy* it will enforce).
- Automatic cross-edition rewrites beyond a documented `jet fix` scope.

## Exit criteria

- docs/spec has a ratified compatibility/release policy and license statement.
- `jet --version` prints compiler version + supported epoch/edition + registry
  compatibility, pinned by a golden test.
- A manifest with an unsupported edition fails with E2001.
- Every later breaking milestone in this folder names the edition/epoch gate it
  needs (cross-checked by the docs test).
- `nix develop -c cargo test` green; new diagnostics have snapshots and
  `jet explain` entries.
