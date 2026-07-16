# Persona Status Check

Run this prompt to produce a live persona brief: a dated snapshot of Jet's current
state against a freshly generated persona set. Write output to
`docs/plans/persona-status/YYYY-MM-DD.md` using today's date. Never overwrite a
prior brief; runs stay diffable.

## Why Personas Are Randomized

Fixed personas let the language optimize for recurring scenarios. Randomized
personas surface general-excellence gaps: if Jet is good, it serves any realistic
user, not just nine repeated cases.

## Step 1 - Orient

Read in full before doing anything else:

- `docs/spec/roadmap.md`
- `docs/spec/philosophy.md`
- `docs/spec/diagnostics.md`
- `docs/reference/stdlib.md` if present, otherwise inspect `docs/reference/` and
  `stdlibs/`
- `examples/features/` and `examples/showcase/` file lists

Also read the most recent prior brief in `docs/plans/persona-status/` if one
exists. If none exists, note "baseline run, no prior."

## Step 2 - Invent Personas

Generate exactly nine fresh personas. Do not reuse names or projects from any
prior brief. Vary them to probe different corners of the language.

Tier spread:

- Three beginners: first compiled language.
- Three intermediate users: comfortable with CLI tools and one other language.
- Three experts: Rust, Go, C, Zig, or equivalent experience.

Pick nine distinct domains, no repeats:

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

- a traditional American first name not used in prior runs;
- one sentence of background;
- one concrete small-to-medium Jet project.

Record all nine in working notes before verification.

## Step 3 - Verify Examples

For each persona, identify one to three representative examples from `examples/`
and run them one at a time:

```bash
scripts/agent/jet-env jet run <path>
```

Do not parallelize shell launches. Quote actual stdout or error output in notes,
labeled with the file path. Every Pull/Push verdict must cite this evidence.

Do not assert that something works or fails from memory. Verify it. Grep docs and
source to check whether claimed gaps have shipped.

## Step 4 - Write Brief

Create exactly one Markdown file:

```markdown
# Jet Persona Status - YYYY-MM-DD
Run: YYYY-MM-DD
Prior run: <date, or "none (baseline)">
Persona set: freshly generated this run
```

For each persona:

```markdown
### N. Name - tier, domain, project title

**Background:** <one sentence>

**Magic they need:** <one sentence>

| Pull (delivers magic today) | Push (friction today) |
|---|---|
| ... | ... |

**Verdict:** ship-ready / usable-with-friction / blocked - <evidence-backed sentence>
```

Verdicts:

- `ship-ready` - project can be completed today with no unresolved blockers.
- `usable-with-friction` - meaningful progress is possible but real pain remains.
- `blocked` - a hard prerequisite is missing.

After the personas, include:

```markdown
## Recommendations (ranked by personas unblocked)

| Priority | Gap | Unblocks | Count |
|----------|-----|----------|-------|
| 1 | ... | Name, Name, ... | N |
```

Tie-break by layer: core language, stdlib, tooling, ecosystem.

End with:

```markdown
## Executive summary

**Strong:** <2-3 bullets, evidence-cited>

**#1 gap:** <one sentence>

**Trend:** <vs prior run, or baseline>
```

## Voice Rules

Terse, plain, evidence-backed. Do not use filler. Banned terms include
"comprehensive," "robust," "seamless," "great," "powerful," and hedging phrases
such as "it should be noted that." If a claim cannot be verified, say what was
checked and what remains unknown.

## Do Not

- Reuse names, projects, or domains from a prior brief.
- Assert current state from memory.
- Touch `docs/spec/` or example files.
- Create any file other than the dated brief.
- Launch project commands outside `scripts/agent/jet-env` or run shell launches
  in parallel.
- Carry forward a Push entry without verifying it still applies.
