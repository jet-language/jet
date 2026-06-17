# Sidequest: E2-M8 — Packages and supply chain implementation

**Plan:** `docs/plans/epoch-2/m8-packages-supply-chain.md`  
**Status:** all decisions ratified; ready to implement  
**Depends on:** E2-M6 (clean public APIs to diff)  
**Unblocks:** E2-M9 (ring versions enforced here), E2-M16

## Ratified decisions summary

| Decision | What to implement |
|---|---|
| D-PKGS1 | Append-only git registry now (hosted CDN/service later) |
| D-PKGS2 | `jet.*` namespace reserved; owner-held; reject publish attempts from others (E2601) |
| D-PKGS3 | Signed metadata optional in v1; design the signed binary/source cache |
| D-PKGS4 **AMENDED** | Immutable releases + yank hides from new resolves + **registry rejects a publish that cannot compile and pass CI/tests** |

## Critical amendment: D-PKGS4

The owner added a pre-publish gate: **packages must compile and pass their tests before the registry accepts a publish.** This means `jet publish` must:

1. Run `jet build` + `jet test` locally first
2. Submit only if both pass (or `--force` with a warning)
3. The registry itself must re-verify on receipt (reject if verification fails)

This is stricter than what the original plan described. Wire it into the E2601 diagnostic family.

## Version pin syntax

Version pins use `#` not `@`: `pkg#1.2.0`. Register `VERSION-#` syntax in `src/syntax.rs` (see `syntax-register-batch.md`).

## Diagnostics to register (E26xx)

E2601 (SemVer break), E2602 (resolver conflict), E2603 (advisory match), E2604 (integrity check failed).

## Exit criteria

See `m8-packages-supply-chain.md`. Key: publish refuses breaking changes AND refuses if tests fail; offline vendored builds work; SBOM emits. `nix develop -c cargo test` green.
