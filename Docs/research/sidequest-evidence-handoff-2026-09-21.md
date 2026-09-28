# Wave A sidequest evidence handoff

**Status:** static preparation only. This artifact does not claim an implementation, a runtime result, a performance result, or a Tower card closeout.

**Cards:** #2414, #3000, #3001, #3002, #3003, #3011, #3016, #3407.

The handoff preserves the existing evidence homes and historical captures. It does not add a database bridge, graph store, public endpoint, MC/DC runtime claim, measurement schema, or Gauntlet policy. It does not change the frozen compiled-workload rows, historical playlist appendix, existing Gauntlet receipts, or protected Source/CmdDevTools, Core, sema Bundle, loader, and shared Gauntlet implementation paths.

## Receipt

| Card | Preparation state | Fixture/manifest action | Runtime or broad validation |
|---|---|---|---|
| #2414 | Handoff only; the frozen workload contract remains authoritative. | No manifest row or task fixture was changed. | Not run. RSS, peer-adapter, and target-toolchain blockers remain visible below. |
| #3000 | Handoff only; existing fault injection is the available seam, not a storage-crash proof. | No DB or VFS fixture was added because the inspected seam does not establish one. | Not run. No emitted DB bridge or crash/reopen receipt exists here. |
| #3001 | Handoff only; existing comparison parsing is the available rail, not an SQL adapter. | No SQL/database corpus was added because no source-matched execution home or adapter contract was established. | Not run. No semantic SQL execution or reduction receipt exists here. |
| #3002 | Source fixture added at `tests/fixtures/coverage_mcdc.jet`. | The fixture is a bounded compound-Boolean witness source only. It is not wired as an MC/DC collector and is not a runtime claim. | Not run. Default branch coverage policy is unchanged. |
| #3003 | Handoff only; the LSP tests are whole-document/current-revision evidence, not a mapped-region contract. | No host-format or public embedded endpoint fixture was added. | Not run. Mapped regions, cancellation, edits, and UTF-16 projection remain unproved. |
| #3011 | Handoff only; existing registries, test economics, comparison journal, and Gauntlet remain the owners. | No second catalog, measurement schema, or Gauntlet policy was added. | Not run. No cost inventory or performance claim is produced here. |
| #3016 | Handoff only; source emission and exact inspection remain implementation work on protected tooling paths. | No generated artifact, capture record, or command manifest was fabricated. | Not run. No assembly or disassembly receipt is claimed. |
| #3407 | Handoff only; the existing Gauntlet manifest/matrix/harness are retained unchanged. | No puzzle IDs, official inputs, expected answers, or comparison rows were invented. | Not run. No AoC correctness, tier, or performance cell is claimed. |

## #2414 — compiled workloads

### Retained authority and source refs

- Tower #2414 retains #1414, `tests/compiled_workloads/manifest.tsv`, and `tools/ci/compiled-workload-runner.mjs` as its direct refs.
- `docs/research/compiled-workload-source-boundary-2026-09.md:1-48` records the six frozen task rows, peer boundary, candidate-versus-selected distinction, structural inapplicability rule, and proof still required. Its row table points to the task definitions and peer ledger.
- `tests/compiled_workloads/manifest.tsv:1-8` is the existing manifest. Every row remains unchanged, including its input, expected output, authority, adapters, platforms, evidence anchor, #1414 owner, and #2414 loss owner.

### Handoff

The existing task and manifest homes are complete enough to hand the lane to the measurement owner; adding another task or synthetic peer would weaken the frozen contract. The next proof must consume the existing runner and emit separate artifact/output receipts for each applicable cell.

The current Tower record preserves these external blockers:

- The `service-json-http` RSS collector has produced no measurement, so no complete report exists from that producer chain.
- The pinned host lacks the `aarch64-unknown-linux-gnu` standard library/toolchain required by the cross-target rows.
- Required peer adapters and target toolchains are not all available. An unavailable cell must remain unavailable, not become zero, a selected-best substitute, or a Jet win.

Exact unknowns remain: no current per-peer/per-metric report, no current cross-target receipt, and no current gate result. The historical source-boundary note is not a performance result.

### Deferred checks

The named checks remain the owner-run work after the external blockers are repaired: repair the RSS collector; run the canonical compiled-workload runner; run the focused `agent_workloads` test; generate the complete native/cross-target report; and run the compiled-workload gate review/check. This handoff did not run them.

## #3000 — storage crash/reopen

### Retained authority and source refs

- Tower #3000 requires a bounded internal host fixture over existing Fs/DB effects and the current test evidence path. It forbids a public VFS API, a new effect tree, and a bespoke report format.
- `docs/audits/mine-for-jet-2026-09-09.md:289-293` distinguishes SQLite regression, fault injection, crash/reopen simulation, fuzzing, semantic oracles, and coverage; it names #3000 as the storage crash/reopen owner.
- The retained claim ledger records the gap and stop line: `docs/audits/mine-for-jet-2026-09-09.md:16181-16277` (fault seam, transaction/crash model, deterministic snapshot/reorder/drop model), and `:17218-17253` (no evidenced one-command crash/reopen path and the requirement to measure whether existing fixtures can express the smallest scenario).
- `tests/effects.rs:78-116` is the existing deterministic `#Test(faults: [Fs.Write])` compile/effect harness. It proves deterministic selector propagation and fail-Nth enumeration for the existing effect/allocation rail; it does not model DB sync, crash snapshots, reopen, or power-loss ordering.
- `Core/testing.jet:79-90` retains bounded deterministic history enumeration. It is a reusable evidence primitive, not storage atomicity proof.
- Tower source refs remain the SQLite testing, VFS, and atomic-commit pages named on #3000. Their external semantics must not be silently converted into a Jet guarantee.

### Handoff and exact unknowns

No DB/VFS fixture was added. The current source-matched seam can carry deterministic fault selectors, but the missing storage contract is still exact and bounded:

1. Which existing Fs/DB effect operation boundaries are reachable by the actual emitted DB path?
2. Which write and sync boundaries are observable, and what snapshot/reorder/drop model is authorized?
3. What is the reopen oracle for a committed transaction versus its absence, including integrity and cleanup?
4. Which storage models are unsupported? Network filesystems must not inherit a local atomicity promise.
5. How are seed, schedule, reduced case, build identity, storage identity, target, and execution mode retained on the existing evidence rail?

A happy-path transaction, an allocation-only fault, or a source-only test must not fill any of these unknowns. The emitted DB bridge, if and when an owner supplies it, must be exercised directly before criterion 3 can be addressed.

### External blockers and deferred check

The blocker is the absent, source-identified storage crash/reopen model and the unproven emitted DB bridge, not permission to add a new public bridge. A future bounded fixture may be added only after those operation boundaries are established through the existing effect path. Required hostile outcomes remain timeout, invalid oracle, unsupported storage, incomplete schedule, partial transaction, and cleanup failure; none count as a pass.

## #3001 — semantic SQL/database comparison

### Retained authority and source refs

- Tower #3001 requires valid SQL/database generators, semantics-preserving transformations, corruption cases, and deterministic shrinkers through existing `core.testing.compare` and `ObservationRelation`; it forbids a second comparison engine and new public oracle API.
- `docs/audits/mine-for-jet-2026-09-09.md:289-293` names semantic SQL fuzzing as distinct from crash-only fuzzing. The retained ledger rows `:16669-16711` require DB/data and SQL transformations through the existing comparison/oracle path and preserve semantic first differences.
- `Core/testing.jet:33-41` is the existing Core comparison surface. It returns a `TestComparison` for typed values; it is not an SQL executor.
- `Source/CmdTest.rs:261-301` parses the existing comparison record and relation; `:412-484` requires each case's `case_id`, `input_id`, `reference`, and `candidate`, with optional replay observations, mutation, seed, and declared equality. `:399-409` maps mismatch, invalid oracle, contaminated, empty, timeout, unsupported, and unavailable outcomes without turning them into passes.
- `crates/jet-codegen/src/Prelude/CoreLib/Top/TestingComparison.rs:1-121` is the generated Prelude carrier for the same comparison machinery; it preserves typed failure/unavailable behavior rather than adding an SQL path.

### Handoff and exact unknowns

No SQL/database corpus was added. The existing comparison rail gives a bounded future fixture shape, but the following must be supplied by the implementation owner without inventing a bridge or schema:

1. The actual DB adapter and its applicable execution targets.
2. The generated data domain, typed null representation, collation/order contract, and whether rows are ordered or explicitly unordered.
3. Valid query transformations and their preconditions (projection/order/filter/join/null semantics).
4. Corruption inputs and their typed failure outcomes, separate from process crashes.
5. The independent reference/candidate implementation boundary, replay identity, and deterministic shrinker that preserves SQL/data preconditions.
6. The first-difference and reduced-case representation on the existing `ComparisonRecord`/history/evidence rail.

A record whose reference and candidate were authored from the same wrong query, a crash count without a semantic relation, an empty corpus, a timeout, an invalid oracle, or contaminated observations cannot pass.

### External blockers and deferred check

The missing DB adapter/fixture contract blocks an executable lane. Once an owner identifies it, the smallest source-matched input should use the existing comparison parser and preserve the current status vocabulary. No SQL command, public endpoint, graph store, or separate reducer is authorized by this handoff.

## #3002 — optional independent-condition evidence

### Retained authority and source refs

- Tower #3002 carries the ratified `D-TEST-MCDC1=A` decision. It adds an explicit optional mode while preserving ordinary branch coverage and the default release policy.
- `docs/audits/mine-for-jet-2026-09-09.md:34-35,107-113,185-200` keeps the branch-versus-independence distinction and the SQLite/LLVM source refs.
- The historical capture `docs/audits/mine-for-jet-2026-09-09.md:2259-2317` retains the exact logical contrast: branch-only cases `000,110` have both decision outcomes and no independent conditions; witness cases `010,110,100,101` establish all three independent effects. This is a logical/source capture, not a Jet runtime receipt.
- `tests/fixtures/coverage.jet:1-14` remains the existing function/branch fixture. The new `tests/fixtures/coverage_mcdc.jet` is deliberately only a source-level witness fixture for the retained predicate and cases.
- `crates/jet-foundation/src/MIR.rs:55-56,5852-5874` shows that coverage-point identity exists in MIR. That fact does not establish an MC/DC report or a source/object mapping.

### Fixture boundary

`tests/fixtures/coverage_mcdc.jet` contains one compound predicate, the branch-only contrast, and the four masking witness calls. It does not add a `--mcdc` command, annotate a condition, inspect a generated object, or claim that Jet collected independent-condition evidence. The default `--coverage` meaning remains untouched.

The future implementation must retain, per requested result, source/object scope, source/build identity, target, instrumentation, condition/decision records, witness pairs, optimized-away/unsupported/unavailable status, and the separation between diagnostic repair coverage and runtime condition evidence. Those fields are acceptance requirements, not a new schema proposed here.

### External blockers and deferred check

No current Jet runtime receipt was produced. A future focused proof must use the same source program and distinguish:

- both branch outcomes from independent condition effects;
- short-circuit masking from evaluating a skipped operand;
- a bitwise aggregate from independently tested Boolean conditions; and
- diagnostic repair coverage from runtime condition coverage.

No release-wide MC/DC gate follows from this fixture. The owner-run proof remains blocked until the optional collector and applicable build/toolchain are available.

## #3003 — embedded-language client contract

### Retained authority and source refs

- Tower #3003 carries ratified `D-EMBED-REGION1=A`: compiler-owned checked regions, one canonical checker, host-identified regions, shared source maps for diagnostics/navigation/edits, revision rejection, cancellation, UTF-8 boundaries, and UTF-16 editor conversion.
- `docs/audits/mine-for-jet-2026-09-09.md:259-264,200-204` records the TypeScript-port evidence, the VS Code/Volar refs, and the limit that whole-file overlays do not prove mapped-region cancellation or a stable embedded API.
- The retained claim ledger `:8152-8228` keeps the versioned compatibility-receipt requirement and the canonical-ballot correction; `:8579-8593` keeps the exact same-document/two-region acceptance boundary.
- `tests/lsp.rs:1181-1270` exercises document revisions and stale-result rejection through the existing LSP protocol. `tests/lsp.rs:3145-3164` exercises cancellation request behavior. These are existing whole-document/protocol tests, not a mapped-region fixture.
- The Tower decision's retained example uses the host string with an astral prefix and states host UTF-8 bytes `25..46` versus UTF-16 units `23..44`. The example is a contract witness, not a current public API.

### Handoff and exact unknowns

No host-format parser, public endpoint, or mapped-region fixture was added. The exact unknowns for the owner are:

1. The source-owned `SourceDocument`/region input and its validation of UTF-8 boundaries and unchanged mapped segments.
2. The identity and revision carried by every result, including a stale-result rejection rule after an incremental edit.
3. How diagnostics, navigation, completion, and edits share one map; edits into generated-only text must be rejected or explicitly labelled.
4. Cancellation behavior when a partial parse/check has already produced intermediate work.
5. Host UTF-16 conversion and byte-range preservation around astral characters.
6. The versioned compatibility receipt for one Svelte-like host document containing two Jet regions, without a second checker or generated-backend error surface.

The current LSP tests are useful anchors but cannot be relabeled as proof of any item above. No current compiler service API, endpoint spelling, or runtime parity is claimed here.

### External blocker and deferred check

The implementation owner must land the compiler-owned region contract through the existing checker path before a fixture can be made executable. The focused check then needs two regions, an edit before the second region, stale/cancelled results, Unicode offsets, partial syntax, and correct diagnostics/edits for the current revision. No alternate parser or host-owned duplicate mapping is authorized.

## #3011 — maintainer algorithm/API cost inventory

### Retained authority and source refs

- Tower #3011 owns the complete first-party denominator and requires source-derived discovery, private/generated/generic/delegated/target-specific coverage, correctness-oracle identity, optional maintainer measurements, and explicit unknowns.
- `docs/audits/mine-for-jet-2026-09-10.md:190-205` lists the existing source ledger, test economics, `.measure`, comparison journal, composition/runtime contracts, Gauntlet, and correctness helpers. It explicitly rejects a second subsystem, public annotations, an always-on timer, and a parallel catalog.
- The same report `:207-255` defines the source-derived scope, dimensions, cost-model facts, proof-versus-measurement distinction, invalidation identity, and hostile false-green controls.
- `:257-281` preserves one operation/identity/workload definition and the existing AOT measurement contract: `CmdCompile::run_test_target`, `CmdDevTools::collect_measure_evidence`, `JETTESTMEASURE1`, `JETALLOC1`, five warmups, twenty exact serial AOT samples, and mode-specific availability.
- `:265-271` requires independent Core declaration comparison and rejects replacing an oracle with a call to the implementation under test. `:326-342` retains the existing comparison/contract/golden/UI/property/fuzz rails and independent-oracle guidance.
- `crates/jet-foundation/src/CoreModuleExports.rs` and `Collections.rs` are source registries named by the retained research; `gauntlet/measurement-manifest.json:1-82` and `:84-123` are existing Gauntlet/report contracts, not an invitation to add another manifest.

### Handoff and exact unknowns

No inventory rows or measurement schema were added. The implementation owner must resolve, through existing identities and producers:

1. The executable source-derived denominator and reconciliation for additions, removals, delegated helpers, generated instances, and target variants.
2. The operation boundary that joins private/helper cost to an observable operation without counting a helper as an independent feature.
3. The existing record fields/producers needed for input dimensions, cost model, bound class, setup/output/callback cost, allocations, oracle, target/profile/toolchain/tier, and expiry.
4. The applicable-mode join for AOT, Cranelift, interpreter, and Web; AOT-only measurements must not be relabelled as another tier.
5. The representative pilot and its explicit uncovered remainder: compiler graph work, scalar API, quantile/selection, effectful I/O, and concurrency.
6. The stale/wrong/zero-valid/all-excluded/broken-oracle/changed-flags/noise controls and the single implementation owner for each unresolved cost finding.

The handoff intentionally does not name new JSON fields, a new dashboard, a graph store, or a public command. Unknown, unsupported, unmeasured, and unavailable rows remain visible.

### External blocker and deferred check

The lane is blocked by implementation and source-discovery work, not by permission to create a second evidence system. The eventual focused proof must be run by the existing maintainer/test/Gauntlet producers with a fresh candidate identity and independent output checks. No cost or speed claim is made here.

## #3016 — exact generated-code inspection

### Retained authority and source refs

- Tower #3016 carries `D-CODE-INSPECT1=A` and `D-CODE-INSPECT2=A`. The source-view amendment is `jet emit --asm`; the exact-build inspector and explicit bounded capture remain separate and immutable.
- `docs/audits/mine-for-jet-casey-2026-09-10.md:62-78` distinguishes algorithm-cost evidence from exact code inspection and records the pre-link versus final-linked/JIT stage boundary, explicit `--capture-code`, and no normal-run change.
- `:82-89` retains the existing inline-assembly source example. `:224-232` keeps the source/MIR/backend/pre-link/final-byte stage distinction and the requirement for exact availability labels.
- Tower refs identify `crates/jet-cli/src/CLI.rs`, `Source/CmdDevTools.rs`, `Source/CmdInspect.rs`, and `crates/jet-foundation/src/MIR.rs`, plus the external cargo-show-asm and rustc `--emit` references. These are source/design anchors only in this handoff.

### Handoff and exact unknowns

No command registration, artifact, capture record, or generated assembly was fabricated. The implementation owner still must establish:

1. Exact source/build/input-closure identity for pre-link emission and exact artifact selectors.
2. Cache reuse and invalidation boundaries, including linker-only changes versus compiler-output changes.
3. Ordinary authority and preparation behavior, including denial before application entry and no new network authority.
4. One status/exit/recovery vocabulary for unavailable, stale, optimized-away, ambiguous, missing, redacted, exhausted-budget, and tool-side capture failure.
5. Producer-backed mappings for inline, merged, generic, stripped, and optimized-away symbols without guessed lines or weakened optimization.
6. Atomic complete exports, bounded retention, eviction/tombstones, and explicit separation of artifact bytes from executed bytes.

The source-view command must never start the application; exact inspection must never build, replay, or start capture. Inline assembly implementation remains #3017's separate contract.

### External blocker and deferred check

The protected Source/CmdDevTools and related tooling paths are outside this static-preparation lane. No fresh compiler/toolchain or live AOT/JIT receipt was run here. The eventual focused check must exercise cold emission, exact cache reuse/invalidation, denial, no app entry, AOT/live/retained-JIT selection, ordinary and inline-assembly functions, an optimized-away symbol, and text/editor/JSON equivalence.

## #3407 — frozen thirty-puzzle AoC corpus

### Retained authority and source refs

- Tower #3407 requires exactly thirty preselected puzzle IDs, both parts, a declared diverse rationale, independent expected answers, owned fixtures, natural implementations for every applicable comparison language, applicable Jet-tier agreement, and discovery of all sixty cells without changing historical receipts.
- Its direct refs are the official Advent of Code about page, sibling #3404, `gauntlet/measurement-manifest.json`, `gauntlet/matrix.json`, and `gauntlet/harness/run.mjs`.
- `gauntlet/measurement-manifest.json:1-82` fixes the current 24-entry/27-cell Gauntlet report contract, required/optional Jet tiers, output verification, ratio verdicts, and missing=`unmeasured` policy. `:84-123` names its existing integrated-gate inputs and freshness contract.
- Existing `gauntlet/entries/*` fixtures and historical receipts remain untouched. They are not AoC puzzle answers and must not be relabelled as such.

### Handoff and exact unknowns

No AoC ID, puzzle prose, personal input, expected answer, language source, or Gauntlet row was invented. Before a bounded corpus fixture can be added, the owner must provide:

1. The exact thirty IDs and both-part selection rationale, frozen before Jet measurement.
2. Official per-puzzle citations without redistributing official puzzle prose or private personal inputs.
3. Owned public sample/stress fixtures and an independently authored oracle for both parts.
4. Complete natural implementations and algorithm disclosures for each applicable comparison language.
5. The selector and manifest join that exposes sixty part cells to existing consumers without replacing the real-workload gate.
6. Applicable Jet tiers, target/toolchain identity, output hashes, and explicit unsupported/unmeasured cells.

The official FAQ's input/privacy boundary remains in force: no account cookies and no answer submission. No corpus can be called frozen until the ID list and fixture/oracle identities are recorded.

### External blocker and deferred check

The missing owner-selected ID set and independent oracle block a source-matched corpus manifest. The future check is the card's bounded selector through the canonical harness on an identified source commit, with both-part comparison across applicable tiers/peers and no mutation of historical Gauntlet receipts. This handoff did not run it.

## Checks performed for this handoff

- Read the current Tower card bodies, plans, criteria, refs, and ratified decisions for all eight cards through the non-serve Tower CLI. No Tower write was made.
- Read the retained 2026-09-09 playlist audit, compiled-workload source boundary, 2026-09-10 maintainer research, and 2026-09-10 exact-code research at the cited sections.
- Inspected only the existing source/fixture seams cited above. No protected implementation path was edited.
- Added one bounded source fixture for #3002. It was not compiled or executed; it cannot serve as an MC/DC receipt.
- No broad test, build, formatter, linter, generator, Tower serve, or performance run was executed.

**Receipt:** `DOCS ONLY`
