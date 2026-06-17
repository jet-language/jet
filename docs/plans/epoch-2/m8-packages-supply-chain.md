# E2-M8 — Packages and enterprise supply chain

**Status:** draft — **blocked on D-PKGS1…D-PKGS4** (Group M8). Builds on the
ratified M12 package design (D-PM1…8, docs/plans/epoch-1/m12-packages.md) and the
ratified jetpack track (D-JPK1…17).
**Depends on:** E2-M6 (clean public APIs to diff). Unblocks E2-M9 (ring versions
enforced here) and E2-M16 (layer-3 builds on this store/lockfile).
**Error codes:** E26xx block (claim in docs/spec/diagnostics.md).

## Goal

Finish the registry era and make dependency management acceptable to teams with
supply-chain requirements (E2-V8 = enterprise-class). The flagship feature is
elm-diff-style **enforced SemVer**: sema knows every public signature, so a
breaking change cannot publish under a non-breaking bump
(the flagship registry feature from the CLI-tooling survey). Single-file programs bypass all
of this (E2-V4, I).

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-PKGS1 | Registry hosting model | **A** — append-only git registry | A | ✅ ratified 2026-06-16 — A: git registry now (hosted later) |
| D-PKGS2 | `jet.*` namespace policy | **A** — owner-held reserved namespace | A | ✅ ratified 2026-06-16 — A: reserved `jet.*` namespace |
| D-PKGS3 | Signing | **A** — signed metadata optional v1; design signed cache | A | ✅ ratified 2026-06-16 — A: optional signed metadata v1 |
| D-PKGS4 | Yank / immutability rules | **A** — immutable releases; yank hides from new solves | A | ✅ ratified 2026-06-17 — A-amended: immutable releases + yank hides from new resolves; **publishing requires the package to compile and pass CI/tests first** (the registry rejects a publish that cannot be verified) |

## Scope

- **Resolver + publish (M12.2):** append-only git registry, semver ranges,
  PubGrub resolver, `jet publish`, `jet vendor`, `jet audit`, local
  compile-once cache.
- **Enforced SemVer:** sema-powered public API diff; `jet publish` refuses a
  breaking change under a non-breaking version bump (Elm-style).
- **Private/internal registry + mirror** configuration (Artifactory/Nexus-style
  proxying) without hard-coding public infrastructure.
- **Air-gapped builds** via `vendor/` and `--locked`; `jet fetch --locked` works
  offline.
- **SBOM** emission from `jet.lock` in SPDX and/or CycloneDX (nearly free given
  the lockfile).
- **Advisory database format + `jet audit`** command.
- **Namespace ownership** rules; immutable/yanked release policy (D-PKGS2/4).
- **Pre-publish gate (D-PKGS4-amended):** `jet publish` must:
  1. Run `jet build` + `jet test` locally first.
  2. Submit only if both pass (`--force` overrides with a warning).
  3. The registry re-verifies on receipt and rejects if verification fails.
- **Signed registry metadata** (optional v1) and a **design** for signed
  binary/source caches with generations/rollback (ship later, E2-M16).

## API-diff diagnostic (example)

```
$ jet publish
error[E2601]: this release is tagged 1.2.0 but removes public API
  --> src/lib.jet:40
   |
40 | pub fn parse(raw: String) -> Report ? ParseError   (removed)
   |
why: 1.2.0 is a minor bump, which promises no breaking changes. Callers
     pinned to ^1.0 would stop compiling.
fix: bump to 2.0.0, or restore `parse` (a deprecated shim counts).
```

## Diagnostics to register

- **E2601** publish would break SemVer (API diff; names the removed/changed item).
- **E2602** resolver conflict (readable PubGrub explanation, not a trace).
- **E2603** `jet audit` advisory match (CVE id + affected range + fixed version).
- **E2604** signature/integrity check failed for a fetched artifact.

## Examples & tests

- `examples/features/38_packages/` — a publishable package with a public API.
- `tests/pkg/semver_break.txt` — E2601 transcript.
- `tests/pkg/resolver_conflict.txt` — E2602 readable conflict.
- `tests/pkg/vendored_offline.txt` — `--locked` build with no network.
- An SBOM golden (SPDX + CycloneDX) generated from a fixture `jet.lock`.

## Out of scope

- Hosting the public registry as a running service (policy + format here, ops
  later).
- Binary distribution / prebuilt artifact CDN.
- Shipping (vs designing) the signed cache + rollback (E2-M16).
- JetOS-style system package management (jetpack-jetos track).

## Exit criteria

- Publish refuses breaking changes under a non-breaking version bump.
- Publish refuses packages that fail `jet build` or `jet test` (D-PKGS4 pre-publish gate).
- `jet fetch --locked` and vendored builds work offline.
- Resolver conflict diagnostics are readable.
- Private mirror flow works without hard-coding public infrastructure.
- SBOM emits in at least one of SPDX/CycloneDX from the lockfile.
- Single-file programs still bypass all package machinery.
- `nix develop -c cargo test` green.
