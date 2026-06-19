# Workspace structure proposal — docs/ cleanup

**Status:** proposal for owner sign-off. No files moved. This doc only
describes what to move/merge/archive/delete.

**Goal:** one inbox for raw tasks, one visible pipeline from raw task →
agent-reviewed plan → decision ballot → ratified → implementation, one home per
fact (kill duplicates), and a *tiny* archive — the owner is not preserving
churn; the code and the spec speak for themselves.

---

## The pipeline (the spine everything hangs on)

A raw task flows left to right. Each stage is one place:

```
  INBOX            PLAN               BALLOT                RATIFIED            IMPLEMENTED
  one scratch  →   sidequest plan  →  open owner queue  →  decision log     →  milestone/spec
  file             (agent-reviewed)   (worked examples)    (the answer)        + examples

  inbox.md     →   plans/           →  spec/                spec/            →  plans/epoch-*/
                   sidequests/         decision-ballots.md  syntax-decisions    spec/spec.md
                                       (.html mirror)       .md                 examples/ (golden)
```

- **Inbox** (`docs/plans/inbox.md`): the owner dumps raw, unstructured tasks
  here. Bullets, half-thoughts, pasted designs. Nothing else writes here.
- **Plan** (`docs/plans/sidequests/<slug>.md`): an agent lifts one inbox item
  into a reviewed plan — current state, footprint, the decision points it
  needs. Removing the item from the inbox is how "promoted" is recorded.
- **Ballot** (`docs/spec/decision-ballots.md`): if the plan needs owner-facing
  syntax, its decision points land here with worked before/after examples. The
  `.html` is the presentation mirror of the same rows (kept in sync; not a dup).
- **Ratified** (`docs/spec/syntax-decisions.md`): the owner decides; the row
  leaves the ballot and the answer is recorded here. (Existing house rule.)
- **Implemented**: the plan is executed; behavior lands in `spec/spec.md`, the
  milestone plan, and golden `examples/`. The sidequest plan is then **deleted**
  (it was scaffolding), not kept as a trophy.

One glance answers "where does a new task go, and what happens next": it goes in
`inbox.md`, and it walks the five boxes above.

---

## Before → after

### Before (today)

```
docs/
  README.md                      # index; links guide/ research/ dev/ — none exist
  embedded.md                    # E2-M15 feature doc, stranded at docs/ root
  plans/
    README.md                    # second index; links post-epoch-2/ — does not exist
    owner-todo.md                # INBOX #1 (+ full design dumps inline)
    bug-fixes.md                 # INBOX #2 (duplicate inbox)
    modules.md                   # "implemented & verified 2026-06-18" (churn)
    module-split-refactor.md     # refactor plan, no code moved
    persona-examples.md          # 2026-06-16 snapshot brief (churn)
    capstone-logbook.md          # ready-to-implement plan
    sidequests/
      attr-at-to-hash.md         # @ → # (reviewed, blocked on owner)
      named-args-defaults.md
      constructor-no-space.md
      multiple-constructor-shapes.md
      stdlib-allocators-arena.md
    epoch-1/  m03..m14 (14 files) # all done
    epoch-2/  README, EPOCH2-STATUS, m1..m18 (20 files)
    epoch-3/  README + 7 files
    jetpack-jetos/  README, IMPLEMENTATION-STATUS, unified-ecosystem,
                    jetos-design, payload-env-separation, unified... (5 files)
  spec/
    philosophy, spec, syntax-decisions, architecture, diagnostics,
    roadmap, release-policy, decision-ballots.md, decision-ballots.html
  reference/
    stdlib.md, versioning.md, errors/ (E0101..E0120 + README)
```

### After (proposed)

```
docs/
  README.md                      # THE index — single, links only to real dirs
  plans/
    README.md                    # implementation protocol only (not a 2nd index)
    inbox.md                     # THE one scratch-pad (owner-todo + bug-fixes merged)
    capstone-logbook.md          # ready-to-implement plan (keep)
    module-split-refactor.md     # EXECUTED (all 11 phases committed) — archive/delete; git holds history
    sidequests/                  # agent-reviewed plans, one per task; deleted once shipped
      attr-at-to-hash.md
      named-args-defaults.md
      constructor-no-space.md
      multiple-constructor-shapes.md
      stdlib-allocators-arena.md
      memory-capabilities.md     # NEW: promoted out of the inbox dump
    epoch-2/  README, EPOCH2-STATUS, m1..m18   # active
    epoch-3/  README + 7 files                 # future
    jetpack-jetos/  README, IMPLEMENTATION-STATUS, unified-ecosystem,
                    jetos-design, payload-env-separation
    archive/                     # TINY; rationale-only; see policy below
  spec/
    philosophy, spec, syntax-decisions, architecture, diagnostics,
    roadmap, release-policy, decision-ballots.md, decision-ballots.html
  reference/
    stdlib.md, versioning.md, embedded.md, errors/ (E01xx + README)
```

What changed, in one breath: two inboxes became one; the second index and all
dangling links are gone; the stranded `embedded.md` joined `reference/`;
finished churn (`modules.md`, `persona-examples.md`, the entire done `epoch-1/`)
is deleted; the inbox's inline design dumps were promoted to sidequest plans.

---

## Single-source-of-truth: duplicates found and canonical home

| Overlap | Files | Canonical home | Action |
|---|---|---|---|
| Two inboxes | `plans/owner-todo.md`, `plans/bug-fixes.md` | `plans/inbox.md` | merge both → inbox.md (this also closes the "multiple sources of truth" bug *listed inside bug-fixes.md*) |
| Two "where to look" indexes | `docs/README.md`, `plans/README.md` | `docs/README.md` | `docs/README.md` is the only index; `plans/README.md` keeps **only** the implementing-agent protocol + dependency graph, drops its index role |
| Dangling index links | `docs/README.md` → `guide/ research/ dev/`; `plans/README.md` → `post-epoch-2/` | reality | fix links to dirs that exist; see Decision A for the missing ones |
| Inline design dumps in the inbox | `owner-todo.md` (Memory Capability Model, Odin ideas, persona content, package-targets essay) | sidequest plans | promote each to `sidequests/<slug>.md`; leave a one-line inbox stub or delete the stub |
| Feature doc at wrong level | `docs/embedded.md` (E2-M15 cross/freestanding) | `reference/` | move to `reference/embedded.md` |
| Snapshot brief vs live status | `plans/persona-examples.md` (2026-06-16 snapshot) | — | delete; it is a dated snapshot, superseded by current `roadmap.md` + `EPOCH2-STATUS.md` |
| Done milestone plans | `plans/epoch-1/*` (14 files), `plans/modules.md` | git history | delete; all marked done/verified, behavior now lives in `spec/spec.md` + golden examples |

**Declared-SSOT files that look like dups but are NOT — leave them:**
- `roadmap.md` (active/not-verified), `epoch-2/EPOCH2-STATUS.md` (E2 status),
  `jetpack-jetos/IMPLEMENTATION-STATUS.md` (ecosystem status) each own a
  *different scope* and already declare it. Overlap is by-scope, not duplication.
- `decision-ballots.md` + `decision-ballots.html` are a record/presentation
  **sync pair**, not a duplicate. Keep both.
- `unified-ecosystem.md` is the jetpack/jetos design-of-record and already
  declares what it supersedes. Keep.

---

## Decisions for the owner

### Decision A — the dangling `guide/ research/ dev/` links

`docs/README.md` advertises three dirs that were never created.

- **Option 1 (delete the links):** drop the rows from the index. *After:* the
  index lists only what exists. Zero new dirs. The learner's-guide / idea-bank /
  nix-setup content simply isn't promised.
- **Option 2 (create the dirs):** make `guide/`, `research/`, `dev/` as real
  homes and move content in (e.g. a nix doc into `dev/`). *After:* index links
  resolve, but you now carry empty-ish scaffolding you must fill.

*Recommendation:* **Option 1.** Don't promise homes you haven't built; SSOT and
edit-existing both say don't spawn empty dirs. Add them back the day real
content needs a home.

### Decision B — inbox shape

- **Option 1 (one flat file, light sections):** `inbox.md` with `## Next`,
  `## Considerations`, `## Bugs`, `## Far horizon`. *After:* the owner pastes
  anywhere under a heading; agents skim top-down.
- **Option 2 (one file, strictly flat bullets):** no sections, pure dump.
  *After:* lowest friction to write, harder to scan once it grows.

*Recommendation:* **Option 1.** Mirrors how `owner-todo.md` is already used
(it has Next / Considerations / Far Horizon), just merging `bug-fixes.md` in as
a `## Bugs` section — minimal change, keeps the owner's existing muscle memory.

### Decision C — archive granularity

- **Option 1 (delete almost everything, no archive):** finished plans and dated
  snapshots are deleted outright; git history is the archive. *After:* `plans/`
  shows only live work. `archive/` may not exist at all.
- **Option 2 (tiny `plans/archive/` for rationale-bearing docs only):** keep a
  doc *only* when it records **why** a still-live design is shaped the way it is
  and that rationale lives nowhere else. *After:* e.g.
  `jetpack-jetos/payload-env-separation.md` (the pkg.jet-vs-Env.jet rationale)
  could move to `archive/` rather than be deleted.

*Recommendation:* **Option 2, kept brutally small** — but default to delete.
Archive a doc only if removing it would lose load-bearing rationale with no
other home. Everything marked "done/verified/implemented" is delete, not
archive: the owner said code speaks for itself.

---

## Migration checklist (propose only — do not execute)

Every current file has a disposition below; nothing is orphaned.

**Inbox**
- [ ] Create `plans/inbox.md`. Move all task content from `owner-todo.md` and
      `bug-fixes.md` into it (Decision B shape). Promote inline design dumps out
      first (see "Promote"). Then delete `owner-todo.md` and `bug-fixes.md`.

**Promote (inbox dump → sidequest plan)**
- [ ] `owner-todo.md` "Memory Capability Model" (view/edit/take/share) →
      `sidequests/memory-capabilities.md` (it is a full design, not a task).
- [ ] `owner-todo.md` "lib vs exe / targets" essay → fold into existing
      `sidequests/` package work or a new `sidequests/package-targets.md`.
- [ ] `owner-todo.md` Odin `when`/range-`switch` notes, pattern-matching list,
      `defer`, typed-error-families, distinct-types → keep as terse inbox
      bullets (they are still raw ideas, not yet plan-ready).

**Index / SSOT**
- [ ] `docs/README.md`: make it the sole index; fix Decision-A links; remove the
      "second index" overlap with `plans/README.md`.
- [ ] `plans/README.md`: strip its index role; keep only the implementing-agent
      protocol, dependency graph, example-numbering table. Remove the dangling
      `post-epoch-2/` link.

**Move**
- [ ] `docs/embedded.md` → `reference/embedded.md`.

**Delete (done / dated churn — git history is the record)**
- [ ] `plans/epoch-1/` (all 14 milestone files — every one marked done).
- [ ] `plans/modules.md` (implemented & verified 2026-06-18).
- [ ] `plans/persona-examples.md` (2026-06-16 snapshot, superseded by roadmap +
      EPOCH2-STATUS).

**Keep in place (live work)**
- [ ] `plans/capstone-logbook.md` (delete once executed).

**Archive/delete (executed)**
- [ ] `plans/module-split-refactor.md` — all 11 phases committed; the split
      itself is the record. Archive or delete; git holds the history.
- [ ] `plans/sidequests/*` (the five existing plans; delete each once shipped).
- [ ] `plans/epoch-2/*`, `plans/epoch-3/*`, `plans/jetpack-jetos/*`.
- [ ] all of `spec/*` and `reference/*` (incl. `errors/E01xx`).

**Archive (only per Decision C, only if rationale has no other home)**
- [ ] Create `plans/archive/` *only if* at least one doc qualifies. Candidate:
      `jetpack-jetos/payload-env-separation.md`. Otherwise skip the dir.

**Hygiene rule going forward**
- [ ] A sidequest plan is deleted the moment its feature ships (behavior lives
      in `spec/spec.md` + examples). Plans are scaffolding, not trophies.
- [ ] Promoted plans cite code by **symbol** (`src/fmt/exprs.rs — fmt_call_args`),
      never by `file.rs:NNNN`. The module-split refactor just invalidated every
      line-number ref; symbol refs survive both line drift and file splits, which
      keeps a plan a single source of truth instead of a stale map.
