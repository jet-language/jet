# Plan: M12.2 registry + `jet publish` UX (D-PUBLISH1)

**Status: backlog — DEFERRED. D-PUBLISH1 is a real future owner decision but is NOT
decidable yet: it rides c50 (build-from-source) and c56 (registry upload) infra, both
unverified/soft-blocked on dep approvals. Promote D-PUBLISH1 to a full ballot card (with
worked `jet publish` shell examples) once M12.2 infra is verified — not before.**

Unblocks: **Saoirse** (publish a library), **Amara** (reproducible scripts pinned
to published versions).

---

## Goal

Both personas hit "M12.2 registry not yet verified — `jet publish` / semver
resolver status is Epoch-1 tail, open." The dependency *manifest* (S52) and a live
git-registry upload (c56) exist, and build-from-source infra is tracked (c50). The
*missing user-facing piece* is the publish + version-resolution **workflow**: how a
library author cuts a release and how a consumer pins/resolves a semver range.

This plan does **not** re-create the registry infra (c50/c56) — it covers the
`jet publish` command surface and the resolver's user-visible policy.

Verified: S52 manifest ratified (`syntax-decisions.md:761`, registry "in M12.2");
c56 = live git-registry upload (validates locally, explains push path); c50 =
build-from-source + M9 wave-2. No `jet publish` command decision found.

## Pipeline touch points

- **CLI** (`jet` driver): a `jet publish` verb (version check, manifest validate,
  tag/upload). Relates to the ratified `jet add/remove/fetch/update` family (S52).
- **resolver**: semver range resolution, lockfile policy (already partly in S52's
  "registry pins; moving selectors `@latest`").
- **stdlib/manifest**: `pkg.jet` version field semantics on publish.
- Mostly **tooling**, not language — but the command surface + error voice are
  product copy needing the owner's call.

## Invariants in play

- **I8** one publish path; don't fork the dependency-add story.
- **Beginner-experience**: `jet publish` should refuse footguns (publishing over an
  existing version, dirty working tree) with teaching errors.
- This rides **c50** (build-from-source) and **c56** (registry upload) — coordinate
  so the UX ships on top of the real infra, not a stub.

## Open questions (D-PUBLISH1 — DEFERRED; promote to a ballot card only after c50/c56 infra is verified)

1. **`jet publish` command shape** — `jet publish` (infers version from `pkg.jet`),
   `jet publish <version>`, or `jet release`? What does it validate before upload
   (clean tree, version bump, tests pass)?
2. **Versioning policy** — strict semver enforced (refuse a breaking change
   without a major bump — overlaps D-MIGRATE1/c73), or advisory? Immutable
   published versions (refuse re-publish) — yes/no.
3. **Resolver policy** — default to the highest compatible version, or exact pins
   + an explicit update? Lockfile committed by default? (S52 has moving selectors;
   pin down the default.)
4. **Auth / registry identity** — how does an author authenticate to push? (this
   may be infra-level c56, but the UX/error story is owner-facing.)

## Test plan

1. `jet publish` happy-path integration test against a local/test registry
   (validate → tag → upload), asserting the success output (product copy).
2. Refusal tests: dirty tree, re-publish existing version, missing version bump →
   each a golden error.
3. Resolver: a consumer pinning a semver range resolves to the expected version;
   `@latest` vs pinned behavior test.
