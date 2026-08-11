---
name: burndown
description: Close Tower cards at volume — Claude orchestrates, Luna (gpt-5.6-luna via codex CLI, max reasoning) implements. Use for "burn down", "close bucket N", "burndown epoch N", "hammer cards", or any high-throughput card-closing session.
---

# Burndown — Claude orchestrates, Luna implements

Goal is **closed cards**, not activity. A session that dispatched twelve workers and
closed nothing failed. Lead every status with `Tower-closed since last checkpoint: [ids]`.

## Role split — absolute

**Claude never implements.** Not one file, not "while I'm here", not when a worker
flakes. Claude: picks and groups work, writes briefs, dispatches, verifies, reviews,
integrates, closes, reports. **Luna implements everything.**

Luna = `gpt-5.6-luna` via codex CLI at `model_reasoning_effort=max`. Never a Claude
subagent for implementation. Claude subagents are allowed only for **read-only** work
(inventory, audit, review) where their compressed output saves orchestrator context.

Board writes are orchestrator-only. Workers mark `criteria --meet` with evidence and
stop. Verifier ≠ builder; the orchestrator or a fresh reviewer sets `--verify` and
`--phase done`.

## The wave loop

1. **Group** the next 3–5 cards into work slices (see Grouping law).
2. **Checkpoint** — commit the tree before dispatch. A worker `git restore` has wiped
   uncommitted orchestrator work before.
3. **Dispatch** one Luna worker per slice, disjoint write paths, ≤5 live.
4. **Verify yourself** — build and run the proof. Never trust a worker's green.
5. **Review once** — one fresh reviewer for the whole batch's semantics cards.
6. **Integrate** sequentially, then **close** on met+verified criteria.
7. **Refill immediately** off the completion notification. Never poll; never idle a slot
   waiting for a slower sibling.

## Grouping law — the throughput multiplier

Closing 200+ cards is impossible one card per worker. Each dispatch costs a brief, a
build, a review, and a merge. Grouping is where the week is won or lost.

**One card-slice = one worker = one proof run = one review** when cards share:

- the same failing test, fixture, or example path;
- the same file neighborhood (one pass through `TIR/lower/**` fixes all of them);
- the same root cause (a table drift, a retired spelling, a missing marshal);
- the same mechanical shape (snapshot blesses, ledger row deletions, fixture migrations)
  — batch 10–20 of these into one worker, they are nearly free.

Measured clusters worth collapsing (2026-08-10 audit): corelib test-health reds (~20
cards, one pass), the JIT/AOT parity ledger (~41 cards, a handful of passes — `#1663`
retires the ledger itself), effect-root/table drifts. When a card's own criterion says
it closes sibling cards, fold them and delete the siblings.

Before dispatch ask: *would one worker fix these in one pass?* If yes, they are one
brief. Fold the cards first (log absorbed content on the survivor, delete duplicates),
so the count reflects the real work.

## Dispatch a Luna worker

Write the brief to a scratchpad file, then (Bash, `run_in_background: true`):

```sh
codex exec -m gpt-5.6-luna -c model_reasoning_effort=max \
  -c 'sandbox_workspace_write.writable_roots=["<repo>/.git","<home>/.cache/nix"]' \
  --sandbox workspace-write --skip-git-repo-check - < /path/to/brief.md
```

Both `writable_roots` entries are load-bearing and each cost a wasted round when
omitted: without `.git` the worker cannot commit (it does all the work, then dies at
`git commit`); without `~/.cache/nix` the Nix fetcher cache is read-only so **it cannot
build or run a single test**, and it will report success it never proved.

- Prompt via **stdin `-` always**. With `-i`, a positional prompt is silently ignored.
- Output is pipe-buffered — nothing appears until exit. Track live progress with
  `git status --short` and card logs, never by reading the worker's output file (it is a
  huge transcript that will blow up orchestrator context; read only its tail at the end).
- One worker writes the main tree; every concurrent sibling gets a worktree under
  `<repo>/.claude/worktrees/<name>`. Never a sibling folder beside the clone.
- Reuse the compiled worktree of a finished worker for the next one — a cold cargo
  target costs 15–20 minutes.

## Brief template

Zero ambiguity. Resolve every open question before dispatch.

- **Goal** — the card ids and a concrete definition of done. Paste the exit criteria
  verbatim; they are the real definition, not the card body.
- **State** — what already exists (commits, staged work, prior worker findings) so the
  worker audits instead of restarting.
- **Files** — **broad scope with exclusions, never an allowlist.** "You may write
  anywhere under `crates/**`, `Source/**`, `tests/**`, `examples/**`, `docs/**`,
  `editors/**` except `plugins/tower/**` and the guard internals of
  `tests/common/mod.rs`." Narrow allowlists cost two full rounds on card #1803 because
  the ratified law required files the brief had fenced off.
- **Do-not-touch** — carry forward known false positives explicitly, with the reason.
  Example: `use c.{abi} as abi` in `crates/jet-pkg-model/src/*Bind.rs` is `format!`
  interpolation, not syntax; rewriting it breaks every FFI bridge generator.
- **Constraints** — AGENTS.md invariants (I4 diagnostics, I7 syntax IDs, I9 tier
  parity); never invent user-facing syntax; never weaken, delete, or `#[ignore]` a test
  to reach green; commit is required.
- **Known-red** — name the failures that are already carded so the worker does not chase
  them.
- **Verify** — the exact targeted commands, and "targeted only; the orchestrator runs
  the full suite."
- **Output** — commit hash, file:line of key changes, real proof output tails, and an
  honest statement of anything still failing.

## Command hygiene (put in every brief)

- **Every** test/build command `timeout`-wrapped (300–900s).
- Harness guards are live: 10 GB allocation cap, 900s suite deadline. A guard trip is a
  defect to report — never a limit to raise. No suite may exceed 15 minutes.
- Build via `scripts/agent/jet-env cargo …` at repo root. Never a cargo target under
  `/tmp` (tmpfs → swap → kernel OOM, which kills the whole agent scope).
- `rm -rf ~/.cache/jet` before any post-codegen smoke run; the build cache is keyed on
  AST hash and serves stale binaries.
- Prelude is `include_str`-embedded: rebuild `jet` after editing it.
- Never `git add -A`; stage explicit paths. No `#` in commit messages (the githook
  rewrites `.tower` on card refs and dirties the tree).

## Proof — prove once, never cycle

Compile time dominates. Cyclical testing is the main throughput killer.

- Workers run **targeted** proof only. The orchestrator runs the union **once** per
  batch — one combined `cargo test -p jet --test <targets…>`.
- Rerun **only** the failing target. Never restart a broad suite "to be safe", never
  re-prove a target another agent already proved in this batch.
- Full suite runs **once at the end of the burndown scope**, not per batch, not per
  card. Unrelated failures become their own card and do not reopen a proved closure.
- Confirm `pgrep -fc "rustc|cargo"` is 0 before a big run; a second cargo blocks on the
  build lock.

## Review — one phase, blocking findings only

**Maximum one review phase per batch.** One fresh-context reviewer covers all semantics
cards in the batch (it may cover 2–3 composed batches). Mechanical cards — fixture
migrations, ledger row deletions, snapshot blesses, doc rewording — get **no** reviewer,
just an orchestrator spot-check of the diff.

The reviewer reports **blocking findings only**. Nits are logged, not fixed. Findings go
back to the implementer once; the reviewer rechecks only material fixes. That is the
whole loop — no third pass, no review-of-the-review.

Reviewer priorities: dropped logic in merge resolutions, I9 drift (new behavior only in
AOT, or policy re-encoded in a JIT/interpreter host instead of calling the Prelude
symbol), false-green tests, new `jit_gaps` parking, and scope creep.

## Never trust worker green

The orchestrator builds and runs the proof itself before closing. On 2026-08-10 a worker
committed code that did not compile while reporting "build passed, tests 3/3" — its
sandbox could not build at all. Verify:

1. `cargo build` succeeds;
2. the card's named tests actually pass;
3. `jit_gaps.txt` gained no entries (removals are good, additions are an I9 violation);
4. no test was weakened, deleted, or ignored to reach green.

## Close discipline

- Close on **met + independently verified** criteria, then `--phase done`.
- Before closing, re-read each criterion's evidence text. A criterion marked `met` whose
  evidence says "NOT SATISFIED" is a false close waiting to happen — this exact
  contradiction was caught on card #1526.
- A regression keeps its own card open and gets a fixer. Never mint a new card to fix an
  existing one.
- Retry cap 2. After two rejected rounds, escalate to the owner with the specific
  blocker — do not burn a third.

## Board honesty (the count must be real)

Full law: `docs/agents/orchestration.md` § Board hygiene and the tower skill's Minting
law. In one line: **probe before minting, retarget instead of close-and-remint, group by
work slice, and never quote a number you have not measured.** A stated remaining count
that grows is a failure; so is one that was secretly larger all along.

## Report

Every status: `Tower-closed since last checkpoint: [ids]` first, then blockers,
decisions needed, and regressions. No closure narration, no activity lists, no predicted
closures. The owner watches the Tower app — tell them what changed and what needs them.

## Reference

Board mechanics and campaign durability: `plugins/tower/skills/tower-burndown/SKILL.md`.
Repo law: `AGENTS.md`. Verification traps: `.agents/skills/verify/SKILL.md`.
