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

- `df -h /tmp` — if near full, `rm -rf /tmp/nix-shell.*` and re-run; a full
  tmpfs causes phantom ENOSPC failures unrelated to your change.
- Use `scripts/agent/jet-env`; it uses nix-direnv's cached environment when
  available. `full` selects browser/FFI/VM tooling.

## Test strategy

- **Card closure:** use the evidence named by the card's robust observable exit
  criteria. The orchestrator checks the evidence after integration and closes the
  card when no known blocker contradicts it. No per-card reviewer or duplicate proof
  is required.
- **Milestone closeout:** after the milestone patches are integrated and cards are
  closed, the orchestrator runs one composed targeted test sweep over the milestone's
  gates and one fresh-context review of the integrated milestone diff. Include every
  applicable I9 execution tier. Add broader targets only when the criteria or a known
  interaction requires them.
- **Closeout findings:** every finding reopens its owning card and affected criteria.
  Apply and integrate the fix, review the delta, and verify the affected criteria
  before the card and milestone close again.
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

## Blessing snapshots and generated docs

Blessing accepts a reviewed behavior change; it is never a way to make red
tests disappear.

1. Run the focused test without an update variable and read the complete diff.
2. Build a fresh binary: `scripts/agent/jet-env cargo build`.
3. Preview with `scripts/agent/jet-env jet self devtools bless <target> --dry-run`,
   then bless only the named target. For generated error pages:
   `scripts/agent/jet-env env UPDATE_DOCS=1 cargo test --test gen_errors gen_error_pages -- --nocapture`.
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

- Stale `target/debug/jet` while sources changed — rebuild first.
- Blessing without reading the full diff.
- `/tmp` full → phantom ENOSPC.
- Moving/renaming examples breaks path-embedding fixtures.
- Claiming done from an audit/research note without this skill's proof gate.
