# E3 reconciliation ledger — 2026-08-12

Board-only audit executed by fable-e3-audit under card #1869 (owner-ratified process: full mutation authority, change ledger required). One line per mutation class; card logs carry the detailed evidence. Authority: owner assignment 2026-08-12; ratified process decisions on #1869; AGENTS.md.

## Closed as shipped (evidence on card)
- #1627 script mode + fn run — implementation on master (68cb43643/fef006684), spec recorded (syntax-decisions.md:159-167), residuals re-homed to #1809.
- #1601 diagnostic_snapshots drift — b325c78ef + e892acc21; view_mut gap intentionally carded (#1616). [Reverted to building 17:59Z by the live bucket-1 orchestrator "fable"; criteria evidence stands on the card.]
- #1771 expand goldens — b5e85d2d7. [Same revert note.]
- #1776 shared_guards STM/deopt — e0f9fb913 + b312d1c5a; criterion 2 superseded by the stronger marshal fix. [Same revert note.]
- #1864 SharedGuard JIT marshal — b312d1c5a via merge 5ae24334f. [Same revert note.]
- #1919 performance session — jet perf family shipped (CLI.rs:288, CmdPerf.rs, tests/jet_perf_trace.rs).
- #1648 divergence-table triage — executed inside the audit: 8 rows re-probed, 3 cards minted (#1930 #1931 #1932), 1 rides #1929, 4 resolved identical; audit doc updated.

## Closed as complete capstones (recording work done inside the audit)
- #1620 compiler-facts capstone — recording verified at syntax-decisions.md:6490-6510; boilerplate rows do not apply to a design-only card.
- #1625 names capstone — one-tree slate section written (all 8 D-NAME outcomes, FILES1 folded in); per-outcome card map logged.
- #1651 choosing capstone — choosing slate section written (PAT1/TEST1/HEADS1/FNBODY1; FIND1 already at :397); duplicate ownership resolved.
- #1495 core-overhaul capstone — D-CORE slate recorded in stdlib-api-laws.md; impl map #1574-#1579 logged.

## Folded duplicates (newest ruling wins; deleted per minting law)
- #1812 → #1454: D-CHOOSE-FNBODY1=A and D-ONELINE-BODY1=B are one function-body law; #1454 rewritten to the reconciled slate (functions ::, heads ->, = leaves bodies).
- #1512 → #1514: condensation is simplify-catalog rule R1 under D-FMT-SIMPLIFY1=A; target spelling corrected to :: (the filed = form was retired by D-ONELINE-BODY1=B).

## Restructured (one card per root cause)
- #1754 takes map_surface (List/Map host-fn family); #1758 narrows to N-ary zip design; #1585 takes the set closure-method stem.
- #1757 adopts orphan compute stems (autodiff, ml, simd); #1761 adopts orphan standalone stems (errors/partial_and_notes, io/db_policy) and drops closed stems (auth_sessions, or_err).
- #1760 narrowed to serde_generic (encoding_breadth already compile-covered).

## Re-homed (strict charter)
- #1909 → e10/ci-release (CI measurement). #1918 → e10/learning (new milestone e10-learning; D-LEARN1 rides). #1848 stays e3 (owner-requested bloat removal protecting current agents — predecessor exception).
- #1893 → e6/ffi-core (D-FFI-CAP1 extern law). #1906 → e6/ffi-core (component ABI). #1915 → e6/polyglot-adapters (D-EMBED1=E; retitled from Decide). #1421 → e6/ffi-core (D-LIB-REUSE1=B dynamic-library half; #1422 keeps the sealed-objects half in e3, unblocked — its blockers were all ratified/done).
- #1914 stays e3 (wasip2 rides the HTTP/server pillar; D-WASISRV1=A ratified; judgment call). #1732 stays e3 (ratified D-ONCE-WORD1=A rides the coherence milestone). #1852, #1919 stayed e3 (concurrency/dev-loop pillars).

## New cards (bugs found; no fixes applied per scope)
- #1929 P0: master does not build — dd4bb9a22 merged out of order; prerequisite bb1ff495d lives on e3/bucket1-integration (161 commits ahead). Card upgraded to the integration duty.
- #1930 P1: types/dimensional_quantities dead on both tiers (E2201 + Items.rs:2175 ICE).
- #1931 P2: io/terminal_parity genuine tier divergence (secret read + stream ordering).
- #1932 P1: memory/returned_views release ICE (5x generated-Rust E0308).

## Spelling law applied (D-ONCE-AT1=D)
- 12 cards annotated to build in @ (never $): #1455 #1458 #1461 #1518(retitled) #1519 #1524 #1525 #1541 #1547 #1622 #1623(retitled) #1782.

## Hygiene
- Stale blockedBy cleared on 57 cards (done cards / ratified decisions / '#N'-format dangling refs normalized to ids); #1663 blockedBy rewritten to its 8 live constituents.
- #1814 correction: was closed today by the bucket-1 stream at 06f582a26; my accidental blockedBy addition removed.

## Plans authored (planning → ready), evidence-packet-backed
#677 #1462 #1756 #1757 #1760 #1761 #1778 #1782 #1848 #1852 #1857 #1858 #1871 #1872 #1875 #1883 #1884 #1885 #1886 #1887 #1890 #1891 #1892 #1894 #1895 #1914(retitled) #1917 #1923 #1925 #1927 #1928 #1929. Gated remainder: #1853 and #1916 carry full ballots (D-ALLOC-PROGRAM1, D-TESTFAULT1); #1516 carries D-FENCE-GLYPH1 (ratified-law conflict: fence digraph vs D-ONCE-DOLLAR1=B).

## Spec recordings landed (docs commits fbf1f0a31 + follow-ups)
- One-tree name slate (8 D-NAME outcomes), choosing slate (4 D-CHOOSE outcomes), D-CORE slate in stdlib-api-laws.md, FILES1 fold, divergence-table triage in the jit-parity audit doc.

## Milestones (ordered, theme-based, 100% coverage of open e3)
M01 e3-bucket-1 Integration and build truth (17, incl. #1869) → M02 failure-model (13) → M03 markers-facts (12) → M04 derives (10) → M05 build-facts (10) → M06 names-core (14) → M07 entry-cli (18) → M08 surface-coherence (13) → M09 types-numbers (12) → M10 memory (9) → M11 concurrency (12) → M12 authority-boundary (20) → M13 jit-parity (13) → M14 services-http (5 XL umbrellas) → M15 web-ui (4) → M16 compute (2 XL) → M17 agent-diagnostics (22) → M18 devloop-testing (8). Dependencies recorded in each milestone goal. Old buckets 2/3/4 emptied of open cards (history intact); bucket-1 renamed to M01 to respect the live integration stream.

## Criteria hardening
- Quality-bar additions applied across the weak-criteria set (batches A/B; per-card log lines); every planning card also received hardened criteria with its plan.

## Flags for the owner
- A concurrent orchestrator ("fable", bucket-1 stream) is live: it reverted four of my closures to building at 17:59Z with integration still in flight, and closed #1814 against the integration branch. No edit war; evidence stands on the cards.
- e11 holds core-syntax cards (#1416 #1419 #1420 comprehension guards) that look mis-homed; out of this audit's scope, flagged only.
- Master is broken (#1929) — no cargo test could run during this audit; all shipped-reality verdicts are static+git evidence plus stale-binary probes.

## Final counts (2026-08-12 close)
- Open e3 cards: 228 (assignment) → 214. Closed by this audit: 11 (7 shipped/complete + 4 capstones); 2 folded-and-deleted; 7 re-homed out of e3; 4 new bug cards minted; 1 audit card (#1869) closes with this ledger. Four closures were reverted to building by the live bucket-1 orchestrator and are counted as open above.
- Plans authored: 36 cards planning → ready/deciding. Ballots filed: D-FENCE-GLYPH1 (#1516), D-TESTFAULT1 (#1916) — both open for the owner. A drafted D-ALLOC-PROGRAM1 duplicate was discarded (already ratified =A).
- Criteria hardened: 133 machine-checkable criteria added across 69 cards + hardened rows on every planned card.
- Milestones: 18 ordered theme milestones, 100% coverage, dependencies in goals.
- Lint: remaining findings are explained here — duplicate-suspect rows are deliberate program structure (jit_gaps program #1663 + constituents; #1754/#1758 root-cause split) or benign shared-file mentions; 16 criteria-evidence-conflict rows are historical evidence-prose heuristics on already-closed cards, not live defects; #86/#676 blocker-unpopulated live in e10/e11 (out of e3 scope); #1408/#1828 done-without-evidence received evidence log lines.

## Part 2 — clean-slate handoff (owner-directed, 2026-08-12 later)
- Landed e3/bucket1-integration into master as merge 31b0c79ec (161 commits; 3 conflict resolutions logged in the commit). Master builds green; jet smoke-tested; run_entry 21/21.
- #1929 closed (build restored). Integration fallout measured and carded: #1934 bind tier0 deopt (6 tests), #1935 lint-snapshot mismatches (6 fixtures), #1936 jit_coverage_audit stack overflow, #1933 auth_sessions example red, E1112 missing typed row logged+criterioned on #1806, shared_guards concurrent case logged on #1776/#1864 (the other orchestrator's reverts were correct — evidence updated).
- Deleted 57 fully-merged branches; all worktrees already pruned except builder (now detached at master). 28 salvage branches inventoried (docs/audits/salvage-branch-inventory-2026-08-12.md) with triage card #1937.
- Recovered the stranded matrix-surface proposal to master (69852c18f, card #1437).
- Released stale leases on #1435 #1629 and building handoffs on #1804 #1805 #1806 #1867 (implementations landed with the merge; verification deltas recorded per card).
- D-TESTFAULT1 reworked twice per owner feedback: rec is now B — keep #Test fn; per-effect question-mark suffix in the signature row, test-only; keyword change parked as option A. #1916 plan synced.
- Stray pre-merge WIP checkpointed as 732437403 (nothing lost).
