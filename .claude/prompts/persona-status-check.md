# Persona Status Check

Run this prompt to produce a live persona brief — a dated snapshot of Jet's current state
against the 9 canonical personas. Output is written to `docs/plans/persona-status/YYYY-MM-DD.md`
(today's date). Never overwrite a prior brief; runs stay diffable.

---

## Purpose

This is a verification-backed status check. The 9 personas, their projects, and the
Pull/Push table format are defined inline below — this prompt is self-contained. Every
claim in the output you write must reflect the current codebase state, verified this run.

---

## Step 1 — Orient

Read these files in full before doing anything else:

- `docs/spec/roadmap.md` — what milestones are verified and what is still open
- `docs/spec/philosophy.md` — ranked priorities; the constitution
- `docs/spec/diagnostics.md` — diagnostic voice and snapshot format
- `docs/reference/stdlib.md` — what std modules exist today
- `examples/features/` and `examples/showcase/` — list the files; note what exists

Also read the most recent prior brief in `docs/plans/persona-status/` if any exists — use it
for the trend comparison in the exec summary. If no prior brief exists, note "baseline run, no prior."

---

## Step 2 — Verify representative examples

For each persona below, identify 1–3 representative examples from `examples/` and run them.
Use `nix develop -c jet run <path>` — one at a time, never parallel (Nix serializes eval
and parallel runs produce noisy output). The dev-shell prints a banner to stdout on startup;
strip it before quoting output (the banner is not program output — quoting it as if it were
is a misread).

Quote the actual stdout or error output in your notes. Label each run with the file path and
the output you observed. This is your evidence base; every Pull/Push verdict must cite it.

Do not assert that something works or fails — verify it. If a claimed gap ("no std.regex,"
"no `extern c`," etc.) was listed in the 2026-06-16 baseline, grep `docs/reference/stdlib.md`
and `src/` to check whether it has shipped since. If it has shipped, remove it from the Push
column; if it still hasn't, carry it forward with a note.

---

## Step 3 — Write the brief

Produce a single markdown file at `docs/plans/persona-status/YYYY-MM-DD.md` (today's date).

### Brief structure

#### Header

```
# Jet Persona Status — YYYY-MM-DD
Run: <today's date>
Prior run: <date of most recent prior brief, or "none (baseline)">
```

#### Per-persona sections (all 9, in order)

For each persona:

```
### N. Name — role, project title

**Magic they need:** <one sentence: what would make this persona's project feel effortless>

| Pull (delivers magic today) | Push (friction today) |
|---|---|
| ... | ... |

**Verdict:** ship-ready / usable-with-friction / blocked — <one sentence, evidence-backed>
```

Use these verdict definitions:

- **ship-ready** — persona can complete their stated project today with no unresolved blockers
- **usable-with-friction** — persona can make meaningful progress but hits real pain points that cost time or require workarounds
- **blocked** — a hard prerequisite is missing; the persona cannot complete the project until it ships

The Pull/Push tables must reflect current state. Remove any entry that has shipped since the
baseline. Add new friction that has emerged. Every entry in the Push column should be
verifiable (a missing stdlib module, a missing example, a failed run, an open milestone).

The 9 personas are:

**Beginners (first compiled language)**
1. Maria — high school student, "Guess the Number" game
2. James — college student, CSV grade tracker
3. Priya — hobbyist, photo folder organizer

**Intermediate (comfortable with CLI tools, one other language)**
4. Carlos — DevOps engineer, log tail analyzer
5. Elena — data analyst, JSON report pipeline
6. Tom — indie dev, terminal roguelike

**Experts (Rust/Go/C/Zig experience)**
7. Marcus — graphics engineer, Raylib prototype game
8. Aisha — senior backend engineer, internal HTTP metrics service
9. Dr. Chen — embedded/tooling engineer, C firmware test harness

#### Recommendation table

After all 9 persona sections, write a deduplicated recommendation list sorted by how many
personas each item unblocks (highest count first). Use this format:

```
## Recommendations (ranked by personas unblocked)

| Priority | Gap | Unblocks | Count |
|----------|-----|----------|-------|
| 1 | ... | Maria, James, ... | N |
| 2 | ... | ... | N |
...
```

Tie-break by stack layer: core language > stdlib > tooling > ecosystem. Do not just repeat
the baseline's "broccoli order" — recount which personas each gap actually blocks given the
current state.

#### Executive summary

End with a section the owner can read in 30 seconds:

```
## Executive summary

**Strong:** <what Jet demonstrably delivers well today — 2–3 bullet points, evidence-cited>

**#1 gap:** <the single highest-priority unresolved blocker, one sentence>

**Trend:** <vs prior run: what shipped, what regressed, what is unchanged — or "baseline, no prior">
```

---

## Voice rules (non-negotiable)

- Terse. One sentence where one sentence will do.
- Plain language. Technical terms only when they are the precise word.
- Evidence-backed. Every claim tied to a run result, a milestone status, or a stdlib grep.
- No filler. Banned: "comprehensive," "robust," "seamless," "great," "powerful," hedging
  phrases ("it should be noted that," "it is worth mentioning"), restated headings,
  summary paragraphs that add no new information.
- Do not invent verdicts. If you cannot verify a claim, say what you checked and what
  you found, then note what you could not verify.

---

## What not to do

- Do not assert current state from memory or assumption. Verify it this run.
- Do not touch `docs/spec/` or any example file.
- Do not create any file other than `docs/plans/persona-status/YYYY-MM-DD.md`.
- Do not run `nix develop` invocations in parallel.
- Do not carry forward a Push entry without verifying it still applies.
