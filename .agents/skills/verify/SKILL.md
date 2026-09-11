---
name: verify
description: >-
  Verify a Jet compiler/stdlib code change — criteria evidence, milestone closeout,
  fresh-binary smoke, snapshot/golden blessing, /tmp traps. Use before claiming
  code done, or when asked to verify. Not an audit skill.
---

# Verify a change in the Jet repo

Use this skill to **close a code change**. Do not run it after an audit or
research docs note “to be safe.”

Model and review policy follow `AGENTS.md` and the owner's current instruction.

## Environment sanity (before trusting ANY failure)

- `/tmp` is RAM-backed. Use `scripts/agent/tmp-guard.sh`; if it blocks, remove only the stale paths it names. Never put Cargo targets, broad logs, or test scratch there.
- Use `scripts/agent/jet-env`; it uses nix-direnv's cached environment when
  available. `full` selects browser/FFI/VM tooling.

## Scoped Rust formatting proof

Run this from the repository root after changing Rust files:

```sh
git diff --name-only --diff-filter=ACMRTUXB -- '*.rs' |
  xargs -r scripts/agent/jet-env rustfmt --check --edition 2021
```

This checks only changed tracked Rust files in the pinned shell. For the full
workspace proof, use `scripts/agent/jet-env cargo fmt --all -- --check`.

## Test strategy

- **Card closure:** run only the evidence named by the card's observable criteria
  against the integrated tree. Close immediately when it passes and no blocker
  contradicts it. No per-card reviewer, duplicate proof, suite batch, unfiltered
  census, or `verify-full.sh`.
- **Milestone closeout:** after every linked card is `done`, commit the frozen source
  and run `scripts/agent/closeout-gate.mjs open MILESTONE --by AGENT`. The token binds
  broad proof and review to that commit. Then run one composed targeted sweep and one
  fresh-context review, covering every applicable I9 tier.
- **Closeout findings:** reopen only the owning card and affected criteria. Fix,
  integrate, run the affected focused proof, and close it again. Freeze a new commit
  and open a new token before resuming broad proof.
- **Hard guards:** `proof-parallel.sh`, unfiltered Core conformance census,
  `verify-full.sh`, and `tower milestone verify` refuse without the token.
- Do not use global `-- --test-threads=1` for completion proof. Use it only for
  a targeted race reproduction after a parallel failure.

## Milestone review

The fresh-context reviewer receives the integrated milestone diff, acceptance
criteria, relevant authority and invariants, and implementation evidence. The review
checks concrete bugs, missed paths, false-green evidence, invariant breaks, stale
decisions, scope drift, duplicate mechanisms, orphaned work, and I9 drift. The
reviewer does not implement.

If the review finds a problem, the owning worker applies the fix. The orchestrator
integrates it, the reviewer reviews the delta, and the orchestrator verifies the
affected criteria. A review finding is not a reason to leave an unrelated card open.

## Owner acceptance boundary

Technical correctness belongs to agents. Workers return evidence; the orchestrator
records criteria evidence and sets `--phase done` after integration. Never park a
technical card in `verify` for the owner, and never set `needsAcceptance` for tests,
diagnostics, safety, compatibility, or other machine-verifiable claims.

Owner verification (`needsAcceptance` / Now “visual check”) is **only** for
look-and-feel with human eyes: UI/UX/DX taste, visual presentation, copy polish,
or a real environment the harness cannot replace. Give the owner a brief
observable checklist only — omit machine evidence.

## Blessing snapshots

Blessing accepts a reviewed behavior change; it is never a way to make red
tests disappear.

1. Run the focused test without an update variable and read the complete diff.
2. Build a fresh binary: `scripts/agent/jet-env cargo build`.
3. Preview with `scripts/agent/jet-env jet self devtools bless <target> --dry-run`,
   then bless only the named target. Diagnostic text comes from its executable
   registry; do not create a Markdown error-page or status-catalog mirror.
4. Inspect `git diff` immediately. Revert unrelated churn.
5. Re-run the focused test with no update variable.

### Run and update one fixture

Filters are repository-relative substring matches and fail when they match
nothing.

```sh
scripts/agent/jet-env env JET_UI_FILTER=tests/ui/arg_type_mismatch.jet \
  cargo test --test diagnostic_snapshots ui_snapshots -- --nocapture
scripts/agent/jet-env env JET_UI_FILTER=tests/ui/arg_type_mismatch.jet \
  UPDATE_EXPECT=tests/ui/arg_type_mismatch.jet \
  cargo test --test diagnostic_snapshots ui_snapshots -- --nocapture
```

## Fresh-binary smoke

Rebuild before `jet run` / smoke claims. The wrapper uses `target/debug/jet`.

## Syntax chores (when syntax changes)

Follow the verification checklist in this skill for Syntax.rs, grammars,
snapshots, and examples when the change touches them.

## Maintainer devtools (`jet self devtools`)

- `scripts/agent/jet-env jet self devtools grammars`
- `scripts/agent/jet-env jet self devtools bless [target...] [--dry-run]`
- Other `jet self devtools` verbs as needed for reduce / ice-report / scaffolds

## Traps

- Stale `target/debug/jet` while sources changed — rebuild before a compiler smoke.
- Broad proof without a review-ready milestone and frozen source commit — close cards
  first, then open the closeout token.
- Blessing without reading the full diff.
- Heavy scratch or logs in `/tmp` — use the disk-backed agent scratch path.
- Moving or renaming examples breaks path-embedding fixtures.
- Claiming done from an audit or research note without observable criterion evidence.
