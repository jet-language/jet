# Persona Status Check

Run this prompt to produce a live persona brief — a dated snapshot of Jet's current state
against a freshly generated persona set. Output is written to `docs/plans/persona-status/YYYY-MM-DD.md`
(today's date). Never overwrite a prior brief; runs stay diffable.

---

## Why personas are randomized each run

Fixed personas let the language quietly optimize for 9 specific recurring scenarios. Randomized
personas surface general-excellence gaps: if Jet is truly good, it serves any realistic user,
not just the 9 cases it has been reviewed against repeatedly. Each run is a new shotgun
spot-check, not a regression suite.

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

## Step 2 — Invent this run's personas

Generate exactly 9 personas fresh. Do not reuse names or projects from any prior brief.
Deliberately vary them to probe different corners of the language.

Maintain this stable structure so briefs remain comparable across runs:

**Tier spread (3 per tier)**
- Beginners — people writing their first compiled language
- Intermediate — comfortable with CLI tools, one other language
- Experts — Rust/Go/C/Zig or equivalent experience

**Domain spread (pick 9 distinct domains from this pool, no repeats within a run)**
- CLI tools / system utilities
- Data processing / ETL
- Web services / HTTP APIs
- Games / graphics / interactive
- Embedded / firmware / low-level systems
- Scripting / automation
- Libraries / reusable components
- Scientific / numerical computing
- Compilers / language tooling
- Networking / protocols
- DevOps / infra tooling
- Creative / generative / audio/visual

For each persona, invent:
- A name (any background, not the same as prior runs)
- One sentence of background (what they know, what they do)
- A concrete small-to-medium project they want to build in Jet

Record all 9 at the top of your working notes before proceeding to Step 3.

---

## Step 3 — Verify representative examples

For each persona, identify 1–3 representative examples from `examples/` and run them.
Use `nix develop -c jet run <path>` — one at a time, never parallel (Nix serializes eval
and parallel runs produce noisy output). The dev-shell prints a banner to stdout on startup;
strip it before quoting output (the banner is not program output — quoting it as if it were
is a misread).

Quote the actual stdout or error output in your notes. Label each run with the file path and
the output you observed. This is your evidence base; every Pull/Push verdict must cite it.

Do not assert that something works or fails — verify it. Grep `docs/reference/stdlib.md`
and `src/` to check whether any claimed gap has shipped. If it has shipped, drop it from the
Push column; if it still hasn't, carry it forward with a note.

---

## Step 4 — Write the brief

Produce a single markdown file at `docs/plans/persona-status/YYYY-MM-DD.md` (today's date).

### Brief structure

#### Header

```
# Jet Persona Status — YYYY-MM-DD
Run: <today's date>
Prior run: <date of most recent prior brief, or "none (baseline)">
Persona set: freshly generated this run (see §Why personas are randomized)
```

#### Per-persona sections (all 9, in order)

For each persona:

```
### N. Name — tier, domain, project title

**Background:** <one sentence>

**Magic they need:** <one sentence: what would make this project feel effortless in Jet>

| Pull (delivers magic today) | Push (friction today) |
|---|---|
| ... | ... |

**Verdict:** ship-ready / usable-with-friction / blocked — <one sentence, evidence-backed>
```

Use these verdict definitions:

- **ship-ready** — persona can complete their stated project today with no unresolved blockers
- **usable-with-friction** — persona can make meaningful progress but hits real pain points that cost time or require workarounds
- **blocked** — a hard prerequisite is missing; the persona cannot complete the project until it ships

Every Push entry must be verifiable (a missing stdlib module, a missing example, a failed run,
an open milestone). Remove any entry that has shipped since the last brief.

#### Recommendation table

After all 9 persona sections, write a deduplicated recommendation list sorted by how many
personas each item unblocks (highest count first):

```
## Recommendations (ranked by personas unblocked)

| Priority | Gap | Unblocks | Count |
|----------|-----|----------|-------|
| 1 | ... | Name, Name, ... | N |
| 2 | ... | ... | N |
...
```

Tie-break by stack layer: core language > stdlib > tooling > ecosystem.

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

- Do not reuse names, projects, or domains from a prior brief.
- Do not assert current state from memory or assumption. Verify it this run.
- Do not touch `docs/spec/` or any example file.
- Do not create any file other than `docs/plans/persona-status/YYYY-MM-DD.md`.
- Do not run `nix develop` invocations in parallel.
- Do not carry forward a Push entry without verifying it still applies.
