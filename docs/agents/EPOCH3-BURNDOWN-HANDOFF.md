# Epoch 3 Burndown — Orchestrator Handoff (2026-08-09, 05:00)

You are the epoch-3 burndown orchestrator. **Goal: every epoch-3 card `done`.**
Read this file, then `AGENTS.md`, then your memory index. Memory files `results-ledger`,
`role-boundary`, `worker-proof-boundary`, `shared-tree-safety`, `sol-sparingly`,
`no-closure-narration`, `scope-moves-need-explicit-approval`, and `caveman-always-on`
are LAW.

---

## 0. Owner's standing orders (learned the hard way tonight)

- **Never end a turn while work is in flight.** Your turn ends when you send a message,
  and nothing wakes you until a worker notification or the owner types. Block *inside*
  the turn (`for i in $(seq 1 19); do grep -q "tokens used" $LOG && break; sleep 30; done`)
  and keep merging/proving/closing. Reporting status and stopping is the #1 failure mode.
- **Caveman mode always on.** Terse. No prose bloat. Owner disables it, nobody else.
- **Do not narrate Tower closures back to him** — he watches the board. Chat carries only
  blockers, owner-gated decisions, regressions, and resource problems.
- **Luna at max is the default worker.** `codex exec -m gpt-5.6-luna -c model_reasoning_effort=max
  --sandbox workspace-write -C <worktree> - < brief.txt > log 2>&1 &` then `disown`.
  Sol (`-m gpt-5.6-sol -c model_reasoning_effort=high`) ONLY when Luna demonstrably failed
  the same task. Sol burn shocked the owner; never launch Sol workers together.
- **Scope moves need explicit approval.** "Could we move X to another epoch?" is a question.
  Deliver analysis + proposal, then wait.
- **No doc bloat.** Docs GC is sidequest card #1848, owner-scheduled. Don't touch it.
- Owner is watching burn rate; keep board writes immediate and avoid redundant polling.

## 1. What buckets 1–4 mean (his vocabulary — use it)

- **Bucket 1 — in-flight, closing now (~30 cards).** Everything whose code was written
  tonight and merged or ready to merge. Named below; nearly all now closed.
- **Bucket 2 — the bounded middle (~150 cards).** Well-defined cards with ratified law:
  the once-* remainder, the #1801–#1821 ratified-unbuilt family, the failure/meta/authority/
  build-config families, JIT-parity groups, and ~25 small test-red/doc-truth cards.
  Conveyor-belt work: Luna workers stream through milestone cards; the orchestrator
  integrates and closes each card on robust criteria evidence, then runs one composed
  closeout sweep and review for the milestone.
- **Bucket 3 — cross-cutting tail (~40 cards).** Types-v2 consumers (number grid, units,
  time), concurrency substrate #1557–#1565, script-mode and `::`-body corpus migrations,
  marker rebuild, Core namespace tree, #1158/#1161 (framework transplants — NOT paperwork,
  they need live-query transport + remote sync convergence). Substrates before consumers;
  corpus-wide migrations LAST (they conflict with everything).
- **Bucket 4 — XL umbrellas (~10 cards).** #444/#1150–#1153 services runtime,
  #1158/#1161 web transplants, #1413 interactive closeout, #1142/#1143 compute ML/SIMD.
  Owner ruled: **stay in e3, best effort.** A re-homing proposal exists and is UNAPPROVED:
  compute pair → e6 (`e6-gpu-backends`; only dependent #1144 is already e6), services chain
  → e7 (self-contained, zero e3 dependents). Do not execute without his word.

**His demand: buckets 1 and 2 fully closed.**

## 2. Live state at handoff

- **Board: 422 e3 done / 232 e3 open** (was 398/255 mid-session).
- **master = `c2bbd213b`**, `cargo check --workspace --tests` was GREEN two commits back;
  re-run it first — `sweep/prove4` merged after the last check.
- **Closed tonight (25):** #1724 #1539 #1710 #1766 #1697 #1694 #1681 #1800 #1683 #1684
  #1497 #1667 #1690 #1802 #1692 #1506 #1507 #1508 #1505 #1785 #1443 #1801 #1546 #1712
  #1686 #1688 #1528 #1141 #1673 #1674 #1698 #1700 #1704 #1711 #1715.
- **Last proof (`scratchpad/pf4.log`) was 4 reds out of 15 suites.** `sweep/prove4` (merged)
  fixes 1 of them. Remaining 3 are owned by worker **`sweep/final5`** — RUNNING at handoff,
  log `scratchpad/br-final5.log`, worktree `.claude/worktrees/bdf5`:
  1. `encoding_parity cbor_codable_bodies_match_aot_resident_and_forced_deopt`
  2. `encoding_parity cbor_migration_plan_matches_aot_and_forced_deopt`
     (both: #1719's unified derive path broke interpreter reachability of derived Codable
     bodies for generic owners — was fixed once at `Codegen/TIR/mod.rs:721`, regressed)
  3. `fmt fmt_lossless::fmt_is_lossless_on_supported_source_corpus` (pinned manifest drift)
- **#1719 is the only once-* card from the current milestone still open** — held because those
  cbor reds are its fallout. Close it when final5 lands green.
- **Building lane:** #1393 #1421 #1543 #1547 #1600 #1601 #1618 #1678 #1680 #1685 #1708
  #1719 #1754 #1758 #1810 #1822. Several are code-merged and need only criteria+close:
  #1678 (deny wall), #1680 (mem gate), #1810 (value-loop), #1547 (fact planes), #1708
  (core-call record, AOT slice), #1421 (c4/c9/c10 slice), #1543 (effect model, 8 owed).
  **#1685** has a 201-file WIP on branch `sweep/wt1b` from a worker that died — needs a
  continuation worker rebased on current master.
- **Owner-gated:** ballot **D-FAIL-ERRWIRE1** on card #1528 (web wire encoding for the
  default error; recommendation D). Only that criterion waits on it.

## 3. The loop that works

1. Select the active milestone's unblocked ready e3 cards (`tower state`, filter
   `phase==ready` and every `blockedBy` resolved). Claim, phase→building, one worktree
   per worker (`git worktree add .claude/worktrees/w<N> -b sweep/w<N> master`).
2. Brief template that worked (heredoc it per card): read the card JSON in full + every
   cited decision verbatim; meet EVERY criterion; I3/I4/I5/I8/I9 laws named explicitly;
   greenfield migration deletes the replaced form; unratified spelling → STOP and return
   ballot-needed; writable paths listed; hard prohibitions (`plugins/tower`, `jet-adjacent`,
   `AGENTS.md`, other cards' files); **NO board writes, NO cargo/jet, NO git**; skills
   ponytail + caveman + simple; return caveman with files+lines and proof commands.
3. Block on the logs. For each ready return, inspect the evidence, integrate the worktree
   into master, record the criteria evidence, and close the card when no known blocker
   contradicts it. Resolve conflicts yourself (or hand one conflict-resolution brief to
   a Luna in the main checkout).
4. At milestone end, run one composed targeted test sweep over the milestone's gates and
   one fresh-context review of the integrated milestone diff. Include all applicable I9
   tiers.
5. Every finding reopens its owning card and affected criteria. Apply and integrate the
   fix, review the delta, verify the affected criteria, and close the card again. Do not
   create a replacement card or revert to hide a finding.

## 4. Traps that cost hours tonight

- Big unproven slices merged together break the build in combination. `cargo check` per
  slice before it enters the queue.
- `git` lock contention: workers and your merges collide — `rm -f .git/index.lock` after
  confirming no live git process.
- Codex workers cannot run cargo/jet (Nix daemon blocked by the sandbox) and cannot write
  the board. You run every proof and every board write.
- Tower rejects `--epoch` moves while the card's milestone belongs to the old epoch; pass
  `--milestone` too. The orchestrator owns criteria evidence and closure.

## 5. Resources

- Scratchpad (briefs + all logs): `/tmp/claude-1000/-home-nate-Projects-Github-jet/8c2818ac-8850-4ba5-9f14-2f3973ffeddf/scratchpad/`
  — `DISPATCH-LEDGER.md` has the full night's ledger; `pf4.log` is the last proof.
- ~50 `sweep/*` branches are merged and prunable; worktrees under `.claude/worktrees/`
  (keep `builder`; `bdf5` and `wt1b` are live).
