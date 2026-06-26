# Doc cleanup proposal (Workstream F)

PROPOSE-only. Nothing destructive executed. The only edits made this pass are
the safe internal-link fixes listed under "Links fixed". Everything else is a
recommendation for owner approval.

Pre-release reality: no users exist. Anything written to alert/migrate/reassure
users is cruft, flagged below.

## Counting rule

- Non-Tower docs (root, `docs/`, `examples/**/README`, `editors/**/README`) are
  assessed individually.
- The epoch-3/4/5 forward-plans are assessed individually (the task singled them
  out for merge-verification).
- The rest of the `tools/Tower/` tree (sidequests, ballots, plans/README,
  proposals/README) is a retiring archive — folded into one coordination note,
  not a per-file delete spree. `Ideas.md` and `decision-ballots.md` are excluded
  from deletion per the task constraint.

Headline: **DELETE 6**, **MERGE 1 pair (2 files)**, **KEEP ~rest**, of which
**6 are KEEP-but-relocate** (epoch-4 + 5×epoch-5, out of the retiring tree).

---

## Links fixed (this pass — safe, additive)

All verified to resolve to existing targets:

- `docs/README.md` — `…/plans/jetpack-jetos/README.md` → `…/plans/epoch-5/README.md`
  (the `jetpack-jetos/` dir was renamed to `epoch-5/`).
- `docs/spec/roadmap.md` — four `…/plans/jetpack-jetos/…` links → `…/plans/epoch-5/…`
  (dir, `README.md`, `IMPLEMENTATION-STATUS.md`, `unified-ecosystem.md`); and the
  bare `[decision-ballots.md](decision-ballots.md)` → the real path
  `../../tools/Tower/docs/ballots/decision-ballots.md`.

## Links flagged, NOT fixed

- **`tests/gen_errors.rs` (generator bug — fix here, not in the 13 output pages).**
  The 13 `docs/reference/errors/E0*.md` pages + `errors/README.md` are generated;
  hand-editing them reverts on regen. The generator emits two wrong relative paths:
  - line 142: `../admin/04-diagnostics.md` → should be `../../spec/diagnostics.md`
  - lines 125 & 139: `../../{rel}` (to `tests/ui/*.jet`) → should be `../../../{rel}`
  Apply in the generator, then regenerate. (Left for owner: this is code in a
  doc-cleanup task.)
- **`docs/spec/syntax-decisions.md`** (owner-owned ratified record — not edited):
  two broken links — `…/epoch-3/user-derives-reflection.md` (plan file deleted in
  the merge) and bare `decision-ballots.md` (wrong relative path). Fix when the
  Workstream C reconcile touches this file.
- **`README.md` → `docs/research/`** — target dir does not exist; no redirect
  target. The repo-map row should be dropped (or `docs/research/` created). Owner
  call — left as a question below.
- **Retiring-tree internal breaks** (low value; resolve only if `tools/Tower/`
  is kept): `tools/Tower/docs/plans/README.md` (`sidequests/`, `jetpack-jetos/*`),
  `…/plans/epoch-3/README.md` (links 3 deleted plan files), `…/epoch-3/c-header-bindings.md`
  (`../../spec/spec.md`), `…/sidequests/memory-capability-model.md` (`docs/research/…`).

---

## DELETE (6)

| File | Reason | Unique content goes |
|---|---|---|
| `cleanup-overhaul-prompt.md` (root) | One-shot task prompt for this very cleanup pass; transient, audience-for-nobody. Once these proposals land it has no use. | Nothing to rescue — it's instructions, not content. |
| `tools/Tower/docs/plans/epoch-3/async-networking.md` | Merged into `Ideas.md` — verified, 4 `_(src: …async-networking…)_` bullets (runtime, no callback colors, FFI-bridged TLS/DB, open questions). | `Ideas.md` Level 4 networking entry. |
| `tools/Tower/docs/plans/epoch-3/c-header-bindings.md` | Merged — verified, src-tagged bullet on `jet bind`/`Source/CBind.rs` + remaining E3 work. | `Ideas.md` Level 3 `jet bind` entry. |
| `tools/Tower/docs/plans/epoch-3/plugin-api.md` | Merged — verified, 2 src-tagged bullets (PATH discovery D-DX5-A, formal plugin API D-DX5-B + open questions). | `Ideas.md` plugin-API entry. |
| `tools/Tower/docs/plans/epoch-3/testing-docs-ergonomics.md` | Merged — verified, 4 src-tagged bullets (property tests, doctests, coverage, `#Bench`), all `Implemented` c51. | `Ideas.md` Level 1 testing entries. |
| `tools/Tower/docs/plans/epoch-3/README.md` | Merged — verified, 2 src-tagged bullets (expression-body `fn` deferred; JIT runtime type server). | `Ideas.md`. |

Delete only after the owner confirms the Ideas.md merge (these are the
"superseded by Ideas.md" candidates per step 2). The epoch-3 merge is verified by
spot-check: every epoch-3 plan has a matching `_(src: plans/epoch-3/…)_` bullet.

## MERGE (1 pair)

| Files | Reason | Recommendation |
|---|---|---|
| `docs/spec/release-policy.md` ↔ `docs/reference/versioning.md` | Overlap: both define SemVer rules, editions, breaking-change → version-bump → migration. Single-source-of-truth violation (I8). | Pick one canonical home, cross-link the other to it. release-policy is the ratified record (D-REL1–5); versioning is the lighter reference page. Fold versioning's unique bump-table/`jet fmt`-migration detail into release-policy and reduce versioning to a pointer — OR vice-versa. **Owner question below.** Neither is a delete; both are ratified/linked. |

## KEEP

Individually assessed, all current and single-home:

- `README.md` — keep. (One cruft flag: see owner question on the production-
  readiness posture.)
- `CLAUDE.md` — keep (agent operating manual).
- `docs/README.md` — keep, but **update the dashboard pointer**: it tells agents
  to run `tools/Tower/Tower.mjs serve` and calls `decision-ballots.md` the "open
  decision queue". Tower **v2** (`tools/Tower-v2/`) is now canonical and
  `decision-ballots.md` is a read-only archive. Stale guidance, not a broken link.
- `docs/spec/philosophy.md`, `architecture.md`, `diagnostics.md`, `spec.md` — keep
  (authoritative surface).
- `docs/spec/roadmap.md` — keep (links fixed above).
- `docs/spec/syntax-decisions.md` — keep; reconcile is Workstream C, broken links
  flagged above. Do not delete/rewrite (owner-owned).
- `docs/reference/core-library.md`, `embedded.md` — keep.
- `docs/reference/errors/E0*.md` (13) + `errors/README.md` — keep; generated,
  generator link bug flagged above.
- `examples/capstone/logbook/README.md`, `examples/graphics/raylib_hello/README.md`,
  `examples/jetpack/README.md` — keep; each documents its program, current syntax
  (`pkg.jet`, `env.jet`, `#Unsafe`, D-CBIND3, D-JPK17), no user-alert cruft.
- `editors/vscode/README.md`, `editors/zed/README.md` — keep; install/setup docs,
  current. (`editors/**/node_modules/*.md` are vendored deps — ignore.)

## KEEP but RELOCATE out of the retiring tree (6)

These are **not** Ideas-level scratch — they are design-of-record and must
survive `tools/Tower/` retirement. Recommend a durable home (e.g. `docs/spec/` or
a new `docs/planning/`). **Owner question on the target path below.**

- `tools/Tower/docs/plans/epoch-5/README.md`, `unified-ecosystem.md`,
  `jetos-design.md`, `payload-env-separation.md`, `IMPLEMENTATION-STATUS.md`
  — the jetpack/jetos design-of-record (owner-ratified U1–U10, D-JPK*, D-OS1).
  **MERGE GAP (step 2):** none of the five appears in `Ideas.md` (zero `_(src:
  plans/epoch-5/…)_` tags; only superficial jetpack/registry mentions exist).
  Correctly absent — they're ratified spec, not ideas — but that means they are
  **not** "superseded by Ideas.md" and must not be deleted. `roadmap.md` and
  `docs/README.md` link these as authoritative.
- `tools/Tower/docs/plans/epoch-4/jai-secure-metaprogramming.md` — **partial
  merge.** The 13 `_(src: …epoch-4…)_` Ideas.md bullets capture the decision seam
  (D-BUILDENTRY1/ACTION1/TARGET1/TOOLCHAIN1/PROBE1/POLICY1/CACHE1/LOCK1, tiers,
  lifecycle verbs, legacy interop). The plan's deeper rationale — threat model,
  the 5-layer security architecture, build-system parity tables, enterprise modes,
  devil's-advocate pass (1138 lines → 13 bullets) — is **not** in Ideas.md and
  would be lost on deletion. Treat like epoch-5: keep/relocate, owner confirms
  whether the depth should persist or the 13 bullets suffice.

---

## tools/Tower/ retirement — one coordination note

The whole `tools/Tower/` tree is a retiring v1 archive. Do **not** restructure it
here. Once its content is rescued (Ideas merge confirmed; epoch-4/5 relocated;
deferred ballots surfaced into v2), retire the tree in one coordinated move.
Carve-outs that must be preserved/relocated, not swept away:

- `proposals/Ideas.md` — owner's sorting surface; relocate to a neutral durable
  path (e.g. `docs/planning/Ideas.md`).
- `ballots/decision-ballots.md` — read-only archive being consolidated; keep until
  consolidation completes.
- `sidequests/*` — one in-flight-task plan each; several are already shipped
  (`units-tag.md` implemented 2026-06-25, `dap-debugger.md` step 1 shipped,
  `doc-consistency-sweep.md` marked done) and are delete-on-ship per the plans
  protocol; others are live (`jit-cranelift.md`, `publish-registry-ux.md`,
  `qualifier-system-implementation.md`, etc.). Reconcile against card status at
  retirement, not now.
- `epoch-4`/`epoch-5` plans — relocate (see above) before the tree goes.

---

## Owner questions

1. **Production-readiness posture vs "pre-release, no users."** `README.md`
   ("Can I use this in production?", "post-v1.0"), `versioning.md` ("v1.0
   shipped"), and `release-policy.md` ("the promise an enterprise adopts") all
   address an enterprise/user audience that does not exist yet. Is this a
   deliberate framing to keep, or audience-for-nobody cruft to trim? (It sits in
   ratified records — your call, not a unilateral cut.)

2. **release-policy.md vs versioning.md — which is canonical?** Both restate the
   SemVer/edition/migration rules. Recommend collapsing to one + a cross-link.
   Which is the home: the ratified `spec/release-policy.md`, or the
   `reference/versioning.md` user page?

3. **Relocation target for epoch-4/epoch-5 design-of-record.** `docs/spec/`,
   `docs/planning/`, or somewhere else? They must leave `tools/Tower/` before it
   retires.

4. **`README.md` repo-map row `docs/research/`** points at a non-existent dir.
   Drop the row, or create `docs/research/`? (One sidequest also links into it.)

5. **epoch-4 jai depth.** Do the 13 Ideas.md bullets suffice, or relocate the full
   1138-line security/threat-model design? (Default recommendation: relocate — the
   rationale is not reconstructable from the bullets.)
