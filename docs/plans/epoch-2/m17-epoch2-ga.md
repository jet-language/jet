# E2-M17 — Epoch 2 GA

**Status:** draft — **blocked on D-GA1…D-GA4** (Group M17) and the strategic
ballot (E2-V1…V12, E2-D1/D2). This milestone *consumes* every other plan; it
adds little new language surface.
**Depends on:** all of E2-M1…M16. Launch versioning ties to E2-D2/E2-V10.
**Error codes:** no new block; aggregates and verifies the rest.

## Goal

Prove the epoch with **real projects, not feature checklists** (E2-V2: credible
for internal services + CLIs). GA is the moment Jet stops being "a promising
small-tools language" and becomes a production platform — single-file `jet run`
still sacred throughout (E2-V4).

## Owner decisions — ratify before declaring GA

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-GA1 | Mandatory showcase set | **A** — 4 showcases + `jet dev` demo (B = all 6 as stretch) | A | ✅ ratified 2026-06-16 — B: all 6 showcases mandatory (owner chose B against prior rec A) |
| D-GA2 | Perf/size budgets | **A** — record per-showcase, no hard CI fail | A | ✅ ratified 2026-06-16 — B: hard CI perf/size gates (owner chose B against prior rec A) |
| D-GA3 | Beta period before GA tag | **A** — short public beta after audits | A | ✅ ratified 2026-06-16 — no beta before GA |
| D-GA4 = E2-D2 | Launch versioning | **C** — separate launch after GA hardening | C | ✅ ratified 2026-06-16 — normal SemVer (= E2-D2; see m2-release-policy) |

## Showcases (D-GA1)

1. A fast **CLI tool** — streaming I/O (M7), regex/data-format libs (M9), tests
   + docs (M11), and a package publish (M8).
2. A small **HTTP service** — tasks/channels (M1), logging + metrics (M12), TLS
   (M10), and a sqlite/durable store (M9/M10).
3. A **library package** — public API diffing + semver enforcement (M8), docs,
   doctests (M11).
4. A **`jet dev` demo** — instant feedback (M4).

Stretch (D-GA1 = B): 5. a **C interop** example (M14); 6. a **low-level /
freestanding** smoke project (M13/M15).

## Audit gates

- `nix develop -c cargo test` green across the whole workspace.
- **Soundness fuzzing** for ownership, references (M5), tasks (M1), low-level
  gates (M13), and FFI (M14).
- **Performance** target recorded per showcase (D-GA2 — record, don't hard-gate).
- **Binary size + compile-time** budgets recorded.
- **Docs** cover migration, compatibility (M2), packages (M8), services (M10),
  debugging (M12), and the low-level gates (M13).
- Every new diagnostic across Epoch 2 has a `jet explain` entry (M3).

## Release checklist

- Compatibility/release policy (M2) is published and `jet --version` honors it.
- Launch versioning decided (D-GA4/E2-D2); `jet --version` prints normal SemVer
  at GA unless the owner flips encoded Epoch SemVer.
- Showcase repos build from a clean checkout via `nix develop`.
- A short public beta runs after audits (D-GA3) before the GA tag.

## Examples & tests

- The showcase projects live under `examples/showcase/` (or named repos) and
  are built + smoke-tested in CI.
- A GA checklist test asserts: every Epoch 2 diagnostic has `jet explain`; every
  milestone marked done in docs/spec/roadmap.md; every showcase builds.

## Out of scope

- Net-new language features (GA hardens what exists; new surface needs a roadmap
  slot, I8).
- Marketing-launch versioning flip if E2-D2 = C (handled at the separate launch).
- JetOS as a shipped product (E2-V12 research-only).
- Async, exceptions, shared-state concurrency (deferred beyond Epoch 2).

## Exit criteria

- The mandatory showcases build, run, and are smoke-tested in CI.
- All audit gates pass; budgets are recorded.
- Docs cover the full adoption story.
- Every Epoch 2 diagnostic has `jet explain`.
- Single-file `jet run file.jet` still needs no manifest.
- `nix develop -c cargo test` green; roadmap marks Epoch 2 done with a date.
