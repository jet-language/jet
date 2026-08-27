---
name: gauntlet
description: >-
  Run the gauntlet corpus and render the competitive scoreboard: measured
  win/parity/loss per matrix cell against Rust, Python, C, Zig, and domain
  incumbents, plus uncovered territory. Absorbs the retired field-audit's
  leave/stay and peer-gap role, now backed by numbers. On-demand, report-only,
  never gates.
---

# Gauntlet

Answer one question with numbers: **is Jet winning — or on track to win — on
everything, in every domain, against every language?** Runtime, compile speed,
tier latency, and read/write/reason ergonomics, measured on the standing
corpus in `gauntlet/`. If the corpus does not exist yet, stop and route to
`gauntlet-architect`.

## Day-zero frame

Judge every language, Jet included, as if all shipped tomorrow with no
history. Age, trust, adoption, community size, and package counts are givens,
never findings. Compare shipped artifacts only. Every loss must name work Jet
can do to close it.

## Run

1. Execute the harness (`gauntlet/harness/`) over every entry via
   `scripts/agent/jet-env`, one machine, one session. Verify each
   implementation still produces its expected output before timing; a wrong
   answer disqualifies the cell and is itself a finding.
2. Compute same-run ratios per metric. Raw times are machine-local; never
   compare against a previous run's numbers. Snapshot, not history, by owner
   decision.
3. Score every cell with the uniform strict bar, no per-entry softening:
   **win** = strictly better than the competitor, **parity** = within 5%
   (ratio ≤ 1.05), everything else **loss**.
4. Collect the advisory rows: readability proxies, RLI5 rubric scores, and
   Luna authoring cost per implementation. For the RLI5 rows: spawn a **Luna
   max subagent per persona** (true beginner default, plus switcher, domain
   expert, unattended agent), have it invoke the global universal `rli5`
   skill against the entry's Jet and port sources with the same modify/derive
   probes, then grade the returned friction tables here — weight each stumble
   by path commonality (`surface-frequency-audit` data where it exists,
   honest estimate otherwise; common-path stumbles are real findings,
   rare-path friction may be acceptable — say which). Advisory, never gating.
5. List every matrix cell with no corpus entry as **uncovered territory** —
   unmeasured is not winning.

## Report

One dark-mode, visual-first scoreboard: matrix cells × metrics, win/parity/
loss colored, losses first (a report with no losing row has not looked hard
enough). Include the beat table — where Jet categorically wins and what a peer
would have to break to match it — marking shipped versus ratified-but-unbuilt
on every row. Prose follows the `simple` skill rules.

Write it under `docs/audits/` via the Tower CLI (never hand-edit board JSON):

```
node plugins/tower/tower.mjs docs add --section audits --id gauntlet-YYYY-MM-DD --title "…" --file -
```

Never overwrite a different day's note.

## The ratchet

This skill never gates and keeps no history, so the cards are the only drift
protection — minting them is not optional:

- Every **loss** auto-mints a Tower card carrying the measured evidence
  (entry, metric, ratio, both sources), deduplicated against existing cards
  first — new evidence for a known cause goes on the existing card.
- Every **uncovered matrix cell** worth filling mints or updates a corpus
  card routed to `gauntlet-architect`.

Close with the per-finding disposition table from
`.agents/skills/_shared/audit-dispositions.md`; card rows satisfy it.

## The standing lens

Apply `.agents/skills/_shared/standing-lens.md` in full: the four questions,
the five agent-optimality quantities, the micro sweep, probe the running
binary, and the honesty rules. Where the lens and the day-zero frame appear to
disagree, the day-zero frame wins here.

Follow `AGENTS.md`. Pick this skill alone — do not chain other audit/research
skills unless the owner asks.
