---
name: verify
description: >-
  Verify a Jet compiler/stdlib code change — scoped proof, major-push closeout,
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

- **Per card / change:** scoped targeted tests only —
  `scripts/agent/jet-env cargo test --test <name>`. One fresh independent
  reviewer inspects the diff and re-runs the relevant proof before close.
- **Batch / major-push closeout:** after 3–5 integrated card closures, or at a
  major-push boundary, the orchestrating session runs
  `scripts/agent/jet-env full scripts/agent/verify-full.sh`, once on the push's
  closeout card. Run it earlier only when targeted evidence identifies a
  repository-wide interaction. It uses a repo-local `TMPDIR` and normal test
  parallelism. CI also runs the full suite. An unrelated failure gets a new
  scoped card; it does not invalidate an already proved card closure.
- Do not use global `-- --test-threads=1` for completion proof. Use it only for
  a targeted race reproduction after a parallel failure.

## Adversarial review gate

Every completed change has one implementer and one fresh independent reviewer.
The reviewer receives only the diff, acceptance criteria, relevant authority and
invariants, and test evidence; assumes the patch is wrong; and seeks concrete
bugs, missed paths, false-green tests, invariant breaks, stale decisions, scope
drift, duplicate mechanisms, and orphaned work. They never implement.

The implementer fixes every material finding and the reviewer rechecks those
fixes. Record reviewer identity, model/effort, reviewed commit or diff,
findings, resolutions, and rerun evidence in Tower/PR handoff. Reviewer
approval alone is not completion evidence.

## Owner acceptance boundary

Technical correctness belongs to agents. Meet criteria, independently verify,
and `--phase done`. Never park a technical card in `verify` for the owner, and
never set `needsAcceptance` for tests, diagnostics, safety, compatibility, or
other machine-verifiable claims.

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
