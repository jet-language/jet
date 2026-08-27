---
name: gauntlet-architect
description: >-
  Define or evolve the gauntlet: Jet's standing competitive corpus of paired
  programs, the persona × domain × task-kind matrix, and the measurement
  harness. Use to seed the corpus, add or retire entries, or re-derive the
  corpus definition against current Jet. Not the audit — `gauntlet` runs and
  reports.
---

# Gauntlet Architect

The gauntlet measures the mission: Jet wins on everything, in every domain,
against every language — runtime speed, compile speed, tier latency, and
read/write/reason ergonomics — proven by real programs that actually run, not
by prose. This skill owns the corpus definition. The `gauntlet` skill owns
running and reporting.

## Mode check (run this first)

If `gauntlet/matrix.json` does not exist, run **seed mode**: this is the first
invocation; build everything below from scratch.

If it exists, run **evolve mode**: the owner re-invoked this skill to update
the corpus against the latest Jet and the latest goals. Re-derive the matrix
and entry set against the current tree, propose additions, retirements, and
definition changes as a diff for owner approval, and never blind re-seed or
overwrite existing entries that still measure something true.

## The matrix

`gauntlet/matrix.json` names the full territory: personas × domains ×
task-kinds. Task-kinds include at least: application, CLI, service, web,
embedded, data/numeric, scripting, notebook-style exploration. Personas span
true novice through domain expert plus the unattended agent (reuse
`persona-audit`'s ladder as the starting set). Domains come from a real sweep
of what people program, not from what Jet is currently good at.

The matrix is the owner's goal statement. Propose it (seed) or its diff
(evolve) and get **owner approval in chat** before landing it. Never narrow it
silently. Every corpus entry tags the matrix cells it fills; cells with no
entry are uncovered territory and appear in every gauntlet report.

## Entry shape

Each entry lives in `gauntlet/entries/<name>/`:

- `entry.json` — task spec (what the program must do, inputs, expected
  observable output), matrix cell tags, tier (`micro` | `program` | `script`),
  language list, and authoring provenance per implementation.
- `jet/` — idiomatic-beginner Jet, the headline row. Optional `jet-expert/`
  for the expert-facet variant on perf entries.
- One directory per reference language port.
- Expected output checked in; the harness verifies every implementation
  produces it before timing anything. A port that computes the wrong answer is
  a broken entry, not a fast one.

Language rails per entry: **Rust and Python always**; **C and Zig added where
the entry's claim is performance**; **the domain incumbent added where one
exists** (TypeScript for web, Go for services, C for embedded, and so on).

## Authorship law (level playing field)

- **Every headline implementation — Jet, every port, every fixture — is
  authored by a Luna max worker** (dispatch per
  `docs/agents/orchestration.md`). Same author, same reasoning budget, both
  sides: the corpus compares what an intermediate engineer produces in each
  language, and doubles as a continuous measurement of whether a non-frontier
  model can write correct Jet.
- Record authoring cost per implementation in `entry.json`: worker turns,
  retries, and diagnostics hit. This is a first-class metric.
- **Expert tier** (optional, perf entries only): when an established expert
  implementation exists (benchmarks-game, real OSS), check it in as a
  clearly-labeled sourced reference row, and pair it with a Jet expert variant
  authored by **Sol high** — expert-vs-expert, never Luna-vs-expert.
- The orchestrator never authors corpus code and workers never run the
  harness. Workers type-check with `scripts/agent/lane-check.sh` only.

## Harness contract

`gauntlet/harness/` scripts measure, per entry and implementation, on one
machine in one run:

- runtime: median-of-N wall time and peak RSS;
- compile: cold and warm build time against the competitor's own toolchain
  (`jet build` vs `cargo`/`zig`/`cc`/none);
- tier latency: `jet run` and `jet dev` first-result time (the
  scripting/notebook pull factor);
- binary size;
- readability proxies: LOC, token count, distinct-concept count, ceremony
  ratio;
- RLI5 rubric scores (advisory) — a Luna max subagent invokes the global
  universal `rli5` skill per persona; the `gauntlet` skill owns grading and
  commonality weighting;
- Luna authoring cost, copied from `entry.json`.

All comparisons are same-run ratios; raw times are machine-local and never
compared across runs. A run emits one `gauntlet/results/<date>.json`
(gitignored) that the `gauntlet` skill renders. Snapshot-only by owner
decision: no trend history is kept.

Run everything through `scripts/agent/jet-env`. Respect the target-dir laws in
`AGENTS.md`: no `/tmp` targets, shared main `target/`, cap-checked. Verify the
devshell provides every competitor toolchain the corpus needs; a missing
toolchain is an owner-visible flake change, not a silent skip.

## Seed scope

Seed mode builds ~12–15 entries across all three tiers: micro perf kernels
(where the C/Zig claim lives), real-world programs (CLI, service, parser —
where trust lives), and script/notebook tasks (where the ergonomics claim
lives). Fill the matrix breadth-first — one entry per distinct region beats
three entries in one cell. Evolve mode grows toward uncovered cells the same
way.

## Close

Seed or evolve closes when: matrix approved and landed, entries build and
produce their expected output in every language, harness runs end to end, and
one full `gauntlet` report has been produced from the result. Follow
`AGENTS.md`; corpus code goes through normal ownership and commit discipline.
