# #3188 — Edit-local verdicts vs checked work

Closer09, 2026-09-29. Source head `e9c708fa7`; binary via
`~/.cache/jet-dev/safe-jet.sh` (jet-current snapshot14 → snapshot16).

## Question

Can the matched edit-verdict matrix (cold / no-change / body / public
signature / dependency delete-rename / effect-failure / package environment /
dirty overlay / recursive dependency) be measured on the self-hosted check
path, comparing warm and fresh verdicts and keeping checker work, semantic
impact and runtime/test reachability separate?

## Method

- Read the self-hosted driver for a reuse layer or receipt:
  `grep -i "reverif|items_checked|reuse|receipt" Compiler/JetDriver/Source/Driver`
  → one unrelated hit (`Pipeline.jet:826`, a diagnostic string). No reuse
  ledger, no `items_checked`, no phase timings, no impact list.
- Inspected the shipped CLI surface: `safe-jet.sh check --help` lists only
  `--json`, `--quiet`, `--color`, `-p`. There is no warm/daemon/watch check
  mode to compare against a fresh run.
- The card's harness (`cargo test --lib --features compiler-bootstrap-host
  bootstrap_tests::bootstrap_private_self_compile_harness`) needs the
  bootstrap host build and the new harness case; the contract forbids
  `Compiler/Bootstrap/check.sh` and broad cargo builds for closers.

## Evidence

| criterion | state on the current tree/binary |
|---|---|
| 1 warm vs fresh per cell | No warm path exists in the self-hosted check or the CLI; every `jet check` is a fresh process. A warm/fresh comparison has nothing to compare. |
| 2 edit-to-verdict time, phases, p50/p95, RSS | Not measured. Tonight's machine runs up to 6 concurrent capped jet processes from other workers (safe-jet queue waits of minutes were observed on this session's own runs), so wall-clock p50/p95 would not be honest. No per-phase timers exist to read. |
| 3 / 6 checked work vs semantic impact vs reachability | The archived Rust `ReverdictReceipt.reverified_items` has no self-hosted successor; there is no receipt to label. |
| 4 no-change / body edit vs uncached | Requires the cache under test; none exists. |
| 5 no linear/superiority claim | Met trivially: no claim is made here. |
| 7 harness JSON under `~/.cache/jet-dev/edit-verdicts/` | Not produced; harness case not implemented. |

## Verdict

BLOCKED/FAIL for this closer: every open criterion except 5 depends on the
implementation in the card's Changes 1–2 (check receipt in
`CompilerQueries.jet`, harness case in `Compiler/Bootstrap/Tests.rs`), which is
compiler work outside the evidence-closer contract, and the timing criteria
need a quiet machine.

## Follow-up

Implement Changes 1–2 as planned (batch-jetc-driver). When measuring, run the
matrix on an idle machine with repeated runs per the AGENTS.md performance
policy, and keep the receipt's `items_checked` separate from a
declaration-identity impact list and an explicit `reachability: Unknown`.
