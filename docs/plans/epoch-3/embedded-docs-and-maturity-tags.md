# Embedded Docs And Maturity Tags

Card: #155 / c2l9f7y. Status: docs plan. Existing docs:
`docs/reference/embedded.md` and `docs/reference/maturity-tags.md`.

## Goal

Make embedded/freestanding work and API maturity tags discoverable without
leaking expert concepts into beginner onboarding.

## Beginner/Expert/Hybrid Pass

- Beginner: hosted single-file programs never mention target profiles, linker
  provenance, allocators, panic policy, or maturity policy.
- Expert: embedded docs expose target triples, freestanding Core availability,
  memory regions, linker provenance, allocator/panic facts, MMIO gates, audit
  JSON, and QEMU smoke paths.
- Hybrid: maturity tags are documentation metadata over the same APIs, not
  access control or release policy. Target profiles are typed facts over the
  same Jet language, not a dialect.

## Current Law

- D-MATURITY1: `@Experimental`, `@Tested`, `@Hardened` are doc-only markers
  before `fn`, parsed and erased.
- D-WD11 plus D-TARGET-SURFACE1/MEMORY1/LINKER1/ALLOC1/AUDIT1: embedded and
  freestanding work uses typed target profiles and dossier/audit output.
- E2-M15: `jet build --target`, `--freestanding`, and QEMU local harness are
  existing reference material.

## Implementation Slices

1. Verify `docs/reference/embedded.md` matches current D-TARGET law and E33xx
   diagnostics.
2. Add a beginner-facing "when you do not need this" intro to embedded docs:
   hosted Jet stays default.
3. Add an expert checklist: target profile facts, Core availability, unsafe
   gates, QEMU proof, audit artifact, dossier target lens.
4. Verify `docs/reference/maturity-tags.md` says tags are doc-only and do not
   alter sema, codegen, release policy, effects, or access.
5. Add examples that are documentation-only if behavior already exists:
   `@Experimental`, `@Tested`, `@Hardened` on public functions; embedded profile
   audit JSON excerpt once surface slice lands.
6. Cross-link from `docs/reference/core-library.md`, `docs/spec/roadmap.md`,
   and `docs/plans/epoch-3/typed-target-profiles.md`.
7. If docs generator lands, make maturity tags render as badges with no
   compiler behavior implied.

## Test Strategy

- Markdown link sanity with `rg`/`ls`.
- Future docs generator snapshots: maturity badges render; embedded page links
  E3301/E3302/E3303 and target-profile audit docs.
- Future implementation tests stay in `typed-target-profiles.md`: no hosted
  regression, target diagnostics, audit JSON, QEMU smoke where available.

## Ballots Needed

No ballots. Maturity tags and target-profile law are ratified. New ballots only
if docs propose a new marker, release gate, command, manifest field, or target
profile spelling.
