---
name: gauntlet
description: >-
  Jet's standing competitive corpus: paired real programs measured against
  Rust, Python, C, Zig, and domain incumbents. One skill, two modes chosen on
  activation — run (harness + win/parity/loss scoreboard, absorbs the retired
  field-audit's peer-gap role) or build/update (define or evolve the matrix,
  entries, and harness). Report-only, never gates.
---

# Gauntlet

The gauntlet measures the mission: Jet wins on everything, in every domain,
against every language — runtime speed, compile speed, tier latency, and
read/write/reason ergonomics — proven by real programs that actually run, not
by prose.

## Mode (pick on activation)

Ask the owner which mode this invocation is, unless the request already says:

- **Run** — execute the corpus, render the scoreboard.
- **Build/update** — define or evolve the corpus itself.

If `gauntlet/matrix.json` does not exist, only build/update is possible: say
so and seed. In build/update with an existing corpus, never blind re-seed —
re-derive against the current tree and propose additions, retirements, and
definition changes as a diff for owner approval.

## Shared laws (both modes)

**Day-zero frame.** Judge every language, Jet included, as if all shipped
tomorrow with no history. Age, trust, adoption, community size, and package
counts are givens, never findings. Compare shipped artifacts only. Every loss
must name work Jet can do to close it.

**Authorship (level playing field).** Every headline implementation — Jet,
every port, every fixture — is authored by a Luna max worker (dispatch per
`docs/agents/orchestration.md`). Same author, same reasoning budget, both
sides. Record authoring cost per implementation in `entry.json`: worker
turns, retries, diagnostics hit — a first-class metric. Expert tier (optional,
perf entries only): when an established expert implementation exists
(benchmarks-game, real OSS), check it in as a labeled sourced reference row,
paired with a Jet expert variant authored by Sol high — expert-vs-expert,
never Luna-vs-expert. The orchestrator never authors corpus code; workers
never run the harness and type-check with `scripts/agent/lane-check.sh` only.

**Environment.** Everything runs through `scripts/agent/jet-env`. Respect the
target-dir laws in `AGENTS.md`: no `/tmp` targets, shared main `target/`,
cap-checked. A missing competitor toolchain is an owner-visible flake change,
not a silent skip.

## Build/update mode

**The matrix.** `gauntlet/matrix.json` names the full territory: personas ×
domains × task-kinds. Task-kinds include at least: application, CLI, service,
web, embedded, data/numeric, scripting, notebook-style exploration. Personas
span true novice through domain expert plus the unattended agent (start from
`persona-audit`'s ladder). Domains come from a real sweep of what people
program, not from what Jet is currently good at. The matrix is the owner's
goal statement: propose it (or its diff) and get owner approval in chat
before landing it. Never narrow it silently. Every entry tags the cells it
fills; cells with no entry are uncovered territory in every run report.

**Entry shape.** `gauntlet/entries/<name>/`:

- `entry.json` — task spec (behavior, inputs, expected observable output),
  matrix cell tags, tier (`micro` | `program` | `script`), language list,
  authoring provenance and cost per implementation.
- `jet/` — idiomatic-beginner Jet, the headline row. Optional `jet-expert/`
  on perf entries.
- One directory per reference-language port.
- Expected output checked in; harness verifies every implementation produces
  it before timing. A wrong answer is a broken entry, not a fast one.

**Language rails.** Rust and Python always; C and Zig where the entry's claim
is performance; the domain incumbent where one exists (TypeScript web, Go
services, C embedded, and so on).

**Harness contract.** `gauntlet/harness/` measures, per entry and
implementation, one machine, one run: median-of-N wall time and peak RSS;
cold and warm compile against the competitor's own toolchain; `jet run` and
`jet dev` first-result latency; binary size; readability proxies (LOC,
tokens, distinct concepts, ceremony ratio); RLI5 rubric scores (advisory);
Luna authoring cost. All comparisons are same-run ratios; raw times are
machine-local. A run emits one `gauntlet/results/<date>.json` (gitignored).
Snapshot-only by owner decision: no trend history.

**Seed scope.** First build lands ~12–15 entries across all three tiers:
micro perf kernels (the C/Zig claim), real-world programs (CLI, service,
parser — where trust lives), script/notebook tasks (the ergonomics claim).
Fill the matrix breadth-first; growth targets uncovered cells.

**Build close.** Matrix approved and landed, entries build and produce
expected output in every language, harness runs end to end, one run-mode
report produced from the result.

## Run mode

1. Execute the harness over every entry. Verify expected output before
   timing; a wrong answer disqualifies the cell and is itself a finding.
2. Compute same-run ratios per metric. Never compare against a previous
   run's numbers.
3. Score every cell, uniform strict bar, no per-entry softening: **win** =
   strictly better, **parity** = ratio ≤ 1.05, everything else **loss**.
4. Collect advisory rows: readability proxies, RLI5 rubric, authoring cost.
   For RLI5: spawn a Luna max subagent per persona (true beginner default,
   plus switcher, domain expert, unattended agent); it invokes the global
   universal `rli5` skill against the entry's Jet and port sources with the
   same modify/derive probes; grade the returned friction tables here,
   weighting each stumble by path commonality (`surface-frequency-audit`
   data where it exists, honest estimate otherwise; common-path stumbles are
   real findings, rare-path friction may be acceptable — say which).
   Advisory, never gating.
5. List every matrix cell with no entry as **uncovered territory** —
   unmeasured is not winning.

**Report.** One dark-mode, visual-first scoreboard: matrix cells × metrics,
win/parity/loss colored, losses first (a report with no losing row has not
looked hard enough). Include the beat table — where Jet categorically wins
and what a peer must break to match it — marking shipped versus
ratified-but-unbuilt per row. Prose follows the `simple` skill rules. Write
under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id gauntlet-YYYY-MM-DD --title "…" --file -
```

Never overwrite a different day's note.

**The ratchet.** This skill never gates and keeps no history, so cards are
the only drift protection — minting them is not optional. Every loss
auto-mints a Tower card carrying the measured evidence (entry, metric, ratio,
both sources), deduplicated against existing cards first — new evidence for a
known cause goes on the existing card. Every uncovered cell worth filling
mints or updates a build-mode corpus card. Close with the per-finding
disposition table from `.agents/skills/_shared/audit-dispositions.md`; card
rows satisfy it.

## The standing lens

Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions,
the five agent-optimality quantities, the micro sweep, probe the running
binary, and the honesty rules. Where the lens and the day-zero frame appear
to disagree, the day-zero frame wins here.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
