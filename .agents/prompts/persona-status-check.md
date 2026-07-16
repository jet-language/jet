# Persona status check

Produce an evidence-backed snapshot at
`docs/plans/persona-status/YYYY-MM-DD.md`. Never overwrite an existing run and
do not modify specs, examples, or other files.

Follow `AGENTS.md`. Inspect the latest prior brief for trend comparison. Search
the roadmap, philosophy, stdlib reference, diagnostics, examples, and source as
needed for each claim; do not read unrelated files in full.

## Personas

Generate exactly nine new personas: three beginners, three intermediate users,
and three experts. Use nine distinct domains selected from CLI/system tools,
data/ETL, web/API, games/graphics, embedded, automation, libraries, scientific,
compilers, networking, infrastructure, and creative audio/visual work. Do not
reuse a prior run's names or projects. Preserve the owner's preference for
traditional American names.

Each persona has a one-sentence background and one concrete small-to-medium Jet
project. Record all nine before testing.

## Evidence

For each persona, select one to three representative examples and run them
sequentially with `scripts/agent/jet-env jet run <path>`. Record path and actual
program output, excluding launcher noise. Search the stdlib and source before
claiming a missing feature. Recheck every carried-forward gap; delete it if it
shipped. Do not infer current behavior from memory or a prior brief.

## Output

Start with date, prior run, and the new persona set. For each persona include:

- tier, domain, project, background, and needed “magic”;
- a Pull/Push table backed by current evidence;
- `ship-ready`, `usable-with-friction`, or `blocked`, with one precise reason.

Use `ship-ready` only when the project has no unresolved blocker;
`usable-with-friction` when real progress is possible with costly workarounds;
`blocked` when a hard prerequisite is absent.

Add a deduplicated recommendation table ranked by personas unblocked, then by
core language → stdlib → tooling → ecosystem. End with a 30-second executive
summary: demonstrated strengths, single top gap, and trend from the prior run.

Use terse plain language. Every claim cites a run, current decision/card, file,
or source search. If proof is unavailable, name what was checked and do not
invent a verdict.
