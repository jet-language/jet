---
name: verify
description: >-
  Close an integrated Jet change with exact criterion evidence, relevant
  snapshots or goldens, syntax coverage, and milestone gates. Use before a
  completion claim; not for audits, research notes, or blanket reassurance.
---

# Verify

Read `AGENTS.md`, then choose only the branch that matches the changed
contract. Do not run a suite merely because this skill was loaded.

| Criterion or artifact | Conditional reference and canonical source |
|---|---|
| One card's observable criterion | [`references/criterion-evidence.md`](references/criterion-evidence.md); the card's named command and expected result |
| Diagnostic, UI snapshot, or executable golden | [`references/snapshots-goldens.md`](references/snapshots-goldens.md); `crates/jet-codegen/src/Prelude/Diagnostics.jet`, matching `tests/ui/` or `tests/ui_lint/`, `docs/spec/diagnostics.md`, and `examples/features/expected/` |
| Syntax, grammar, or example change | [`references/syntax.md`](references/syntax.md); `crates/jet-foundation/src/Syntax.rs`, `docs/spec/syntax-decisions.md`, `docs/spec/contributing/examples.md`, and `examples/README.md` |
| Linked-card milestone | [`../orchestration/references/closeout.md`](../orchestration/references/closeout.md) and `scripts/agent/closeout-gate.mjs` |
| Scratch, target, memory, or process safety | [`../orchestration/references/resources.md`](../orchestration/references/resources.md), `scripts/agent/tmp-guard.sh`, and `scripts/agent/jet-env` |

## Non-negotiable truth

- Verify the exact acceptance criteria against the integrated tree. Record the
  command and observable result; if evidence was not run, mark it unknown or
  pending, never green. A type-check or static receipt is not runtime, tier,
  diagnostic, snapshot, golden, or generated-artifact proof.
- Runtime claims require a fresh current-source binary before the actual CLI or
  UI path is exercised. Check every applicable I9 tier named by the criterion:
  AOT, Cranelift JIT (`jet run`/`jet dev`), interpreter/deopt, and web when
  applicable. Do not infer parity from another tier.
- The card and linked-card cadence, including the focused proof, `done` query,
  commit-bound token, one composed targeted sweep, and one fresh-context review,
  is defined only in `../orchestration/references/closeout.md`. Do not invent a
  second cadence here.
- `proof-parallel.sh`, an unfiltered conformance census, `verify-full.sh`, and
  `tower milestone verify` are tokened milestone operations, not card proof.
  No blanket suite substitutes for the named criterion command.
- Technical acceptance belongs to the agents and the orchestrator. Owner
  acceptance (`needsAcceptance` / “visual check”) is only for look-and-feel,
  copy or DX taste, visual presentation, or a real environment the harness
  cannot replace; it never replaces machine evidence.

If the owner explicitly orders implementation-before-validation, that order
supersedes the ordinary phase sequence for the requested implementation cards
only. Do not activate it without that explicit request, and never label an
unrun runtime or tier proof green.
