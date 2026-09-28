# Fresh-agent 5/5 rerun verdict — 2393-r1

**Run date:** 2026-08-31 (timestamps are preserved in the raw receipts)  
**Verdict:** **FAIL.** This run does not earn Jet preference and does not close criteria 2, 3, or 5.

This is the execution verdict for the frozen protocol in [`fresh-agent-5-of-5-rerun-2026-08-30.md`](fresh-agent-5-of-5-rerun-2026-08-30.md), not a replacement protocol. The protocol requires ten participants, four matched tasks, both arms, project-green default and optimized/native evidence, seven score categories, and unanimous Jet preference.

## Executive result

| Gate input | Observed result | State |
| --- | ---: | --- |
| Expected participants | 10 | fixed denominator |
| Participants with both arms measured | 9 (A01, A02, A03, A05, A06, A07, A08, A09, A10) | incomplete |
| Unrecoverable participant | A04 Anthropic Jet: exit 137 after 600.021 s; Rust arm not started | not measured |
| Expected raw receipts | 80 | manifest contains all 80; 8 are explicitly unmeasured |
| Measured raw receipts | 72 | complete |
| Jet project-green task receipts | 27/36 measured tasks | fail |
| Rust project-green task receipts | 36/36 measured tasks | task evidence only; not the Jet gate |
| Blind comparisons | 9/10 | all nine measured choices were Rust |
| Full ten-participant campaign median | not measured | A04 is not omitted from the denominator |

The raw receipt manifest is [`raw/2393-r1/manifest.json`](raw/2393-r1/manifest.json). Every expected participant/task/arm path is present. Each measured receipt preserves the frozen task/source digests, prompt digest, final source, command argv, raw stdout/stderr, status, timer fields, correction count, diagnostics, and score; unmeasured receipts preserve the task digest and explicit null/stop state.

## Frozen controls and deviations

- Tool versions observed: `jet 1.0.0`, `rustc 1.97.1`, and `omp/18.0.11`.
- Fixtures, argv vectors, source roots, and environment identity are preserved in each receipt. Jet and Rust used separate participant directories.
- Rust task sources were standalone `.rs` files. The runner used a scratch `bin/rustc` wrapper to invoke `scripts/agent/jet-env rustc`; no Cargo project existed in the frozen task fixture. This is a protocol deviation from the wording “Rust uses cargo check,” and is not silently treated as equivalent evidence.
- Direct `omp --model gpt-5.6-luna` was rate-limited for A01. The permitted OMP task fallback ran the OpenAI-family A01 sessions. A01 was run in one session per arm: T1 selected Jet-first, while T2–T4 selected Rust-first. The per-task arm-order assignment was therefore not satisfied for T2–T4; the deviation is recorded in every A01 receipt.
- A04 Anthropic Jet timed out at the protocol 600 s limit and exited 137. The protocol stop rule was applied: no replacement, retry, or Rust arm was run.
- Anthropic rating retries for A08 Jet and A10 Rust corrected CLI/path mistakes before rating. The failed invocations are preserved beside the successful retry receipts.

## Project-green task outcomes

A task is green only when the receipt records a clean check or compile, exact default output, and exact optimized/native output. Expected seeded failures are separate from project-green success.

| Participant | Jet green tasks | Rust green tasks | Material observation |
| --- | ---: | ---: | --- |
| A01 | 0/4 | 4/4 | initial E2105 setup failure; PATH-corrected native retry hit Scheduler fact-registry ICE |
| A02 | 3/4 | 4/4 | Jet T1 optimized/build path hit an internal compiler failure |
| A03 | 3/4 | 4/4 | Jet T4 default run hit E0956 evaluator limitation |
| A04 | not measured | not measured | Jet tool failure stopped the participant |
| A05 | 4/4 | 4/4 | Jet fixed-task runs reached project-green |
| A06 | 3/4 | 4/4 | Jet T1 optimized/build path hit an internal compiler failure |
| A07 | 3/4 | 4/4 | Jet T1 optimized/build path hit an internal compiler failure |
| A08 | 3/4 | 4/4 | Jet T1 optimized/build path hit an internal compiler failure |
| A09 | 4/4 | 4/4 | Jet fixed-task runs reached project-green |
| A10 | 4/4 | 4/4 | Jet fixed-task runs reached project-green |

Representative exact outputs are in the receipts, including T1 resolved inheritance, T2 three command vectors, T3 replay state, and T4 canonical JSON bytes. The receipt command arrays are task-isolated; for example [`A05/T4/jet.json`](raw/2393-r1/A05/T4/jet.json) preserves its check/build/default/native command results.

## Criterion 2 — scorecards and medians

The category order below is `reading/writing/reasoning/creating/modifying/diagnostics/tooling-docs`. Each row is the median of that participant-arm’s four task scores, followed by the lowest underlying raw score. Full campaign medians are **not measured** because A04 has no score row.

| Participant | Arm | Four-task median vector | Raw minimum |
| --- | --- | --- | ---: |
| A01 | Jet | 5/5/5/5/4/5/4 | 4 |
| A02 | Jet | 5/5/5/5/4.5/4/4 | 2 |
| A03 | Jet | 5/4/4/4/4/4/3 | 3 |
| A05 | Jet | 5/4/4/5/4/5/4 | 3 |
| A06 | Jet | 4/3.5/4/3.5/3/4/4.5 | 2 |
| A07 | Jet | 5/4/5/5/4/5/4 | 3 |
| A08 | Jet | 4/4/4/4/4/3/4 | 2 |
| A09 | Jet | 4/4/4/4/3.5/4/3 | 2 |
| A10 | Jet | 4/4/4/3.5/3.5/4/3 | 3 |
| A01 | Rust | 5/5/5/5/4/5/5 | 4 |
| A02 | Rust | 4/4/4/4/4/4/4 | 4 |
| A03 | Rust | 4/5/5/5/4/5/4 | 4 |
| A05 | Rust | 5/5/5/5/1/5/5 | 1 |
| A06 | Rust | 4/4/4/4/4/4/3 | 3 |
| A07 | Rust | 5/5/5/5/5/5/5 | 4 |
| A08 | Rust | 4/4/4/4/4/4/3 | 3 |
| A09 | Rust | 5/4/4/5/4/4/5 | 4 |
| A10 | Rust | 5/5/4.5/4/4/4.5/3 | 3 |

| Arm | Observed 9-participant partial median | Full 10-participant campaign median | Gate result |
| --- | --- | --- | --- |
| Jet | 5/4/4/4/4/4/4 | not measured (A04) | FAIL: Jet has raw scores 2/3 and participant medians below 5 |
| Rust | 5/5/4.5/5/4/4.5/4 | not measured (A04) | descriptive only |

Jet criterion 2 fails directly: A02, A06, and A08 each recorded a T1 `tooling_docs` score of 2; A09 T4 recorded `modifying=2`; several other Jet rows contain 3s. The fixed bar requires every underlying Jet score to be at least 4, every participant/category median to be 5, and a ten-participant campaign median of 5. None of those claims can be made here. Rust A05 also recorded `modifying=1`; that does not repair the Jet gate.

Raw category wording and each participant reason are preserved in `rating_response_raw`; the normalized `rating` object uses the receipt's stable `tooling_docs` field. The A02, A06, A08, and A10 Anthropic response text is preserved in the corresponding `rating_response_raw`; OpenAI follow-up response JSON is preserved in the receipt and scratch provenance.

## Criterion 3 — blind preference

Comparison labels were opaque and independently mapped from the persisted comparison digest. The table shows the exact answer, then its language mapping.

| Participant | Exact answer | Opaque labels (Jet/Rust) | Mapped choice |
| --- | --- | --- | --- |
| A01 | Arm A | Arm B/Arm A | Rust |
| A02 | Arm A | Arm B/Arm A | Rust |
| A03 | Arm B | Arm A/Arm B | Rust |
| A04 | not measured | not assigned after tool failure | not measured |
| A05 | Arm B | Arm A/Arm B | Rust |
| A06 | Arm A | Arm B/Arm A | Rust |
| A07 | Arm B | Arm A/Arm B | Rust |
| A08 | Arm A | Arm B/Arm A | Rust |
| A09 | Arm A | Arm B/Arm A | Rust |
| A10 | Arm A | Arm B/Arm A | Rust |

Measured comparison count is 9/10: Rust 9, Jet 0, no preference 0. The fixed criterion requires every preference receipt to choose Jet. Criterion 3 fails. Each exact participant reason is in [`raw/2393-r1/<participant>/comparison.json`](raw/2393-r1/), with preserved direct/task response provenance in `.tmp/2393-r1/runs/`.

## Criterion 5 — dated report, evidence, and residual closure

This report and the updated ledger provide the dated artifacts, and the manifest provides all expected paths and SHA-256 digests. That is necessary but not sufficient: eight receipts are explicitly unmeasured, the full median is unavailable, and criterion 5 requires measured scores, preference, and residual testimony closure.

| Evidence dimension | Observed | State |
| --- | --- | --- |
| Receipt completeness | 80 paths present; 72 measured; 8 A04 rows not measured | fail |
| Source metrics | physical lines and Unicode-whitespace token counts in every measured receipt | recorded |
| Timers | wall time, first stdout, peak RSS where supplied, and exit status preserved per command | recorded; missing values remain null |
| Seeded diagnostics | 27 Jet and 54 Rust seeded-failure records; all are marked as named and resolved in the measured command set | recorded |
| Unseeded/tool diagnostics | Jet includes E2105 setup misses, native/AOT internal compiler failures, and A03 T4 E0956; these remain residual findings | open |
| Testimony ledger | updated with this run; implementation and experience owners remain open | open |

### Residual findings

1. A04 is a hard coverage hole, not a zero. The exact OMP failure receipt is sealed in [`A04/`](raw/2393-r1/A04/) and records exit 137, timeout, empty stdout, and `Working...` stderr.
2. Jet optimized/native reliability is not general across this sample. A01’s PATH-corrected retry hit the fact-registry law violation for `Scheduler`; A02, A06, A07, and A08 hit Jet native/AOT failures on T1. These are compiler/tool failures, not user diagnostics.
3. A03 T4 default execution returned E0956 (`JSON lenient decode coercion audit effects` unsupported by the current evaluator), so the source did not reach project-green under the fixed default command.
4. Jet style/cost diagnostics L0507, L0514, L0520, L2510, and L0503 are preserved as encounters. The seeded failure cases were named and resolved; advisory diagnostics are not relabeled as user failures.
5. The testimony owners from the ledger remain open. A fresh score does not close #2391, #2387, #2388, #2389, #2390, or #1310; F46 remains declined for this bounded campaign and F47 remains gated by #2327.

## Gate assessment

| Criterion | State | Evidence |
| ---: | --- | --- |
| 1 | done | Frozen protocol was executed and deviations were recorded. |
| 2 | open | 72 measured scorecards expose Jet raw scores below 4 and medians below 5; A04 prevents the ten-participant campaign median. |
| 3 | open | 9 comparisons are measured and all nine choose Rust; A04 has no comparison. |
| 4 | open | Ledger rows remain mapped but unresolved owners and testimony obligations remain. |
| 5 | open | This dated report and ledger are present, but the fixed evidence set is incomplete and residual closure is open. |

## Conclusion

The 2393-r1 rerun is a valid partial execution with sealed raw evidence, not a passing five-of-five campaign. Do not close criteria 2, 3, or 5. The next run must keep the ten-participant denominator, use an arm-order mechanism that can honor each task assignment, preserve the Rust check-mode contract or explicitly ratify a standalone replacement, and reopen each owning card for the residual compiler, experience, and testimony findings.

Raw receipts: [`raw/2393-r1/`](raw/2393-r1/). Updated ledger: [`../proposals/dogfood-jet-experience-5-of-5.md`](../proposals/dogfood-jet-experience-5-of-5.md).
