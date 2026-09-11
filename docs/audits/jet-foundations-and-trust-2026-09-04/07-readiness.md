# Stable release: qualify behavior, not a version string

[Master report](index.md) · [Assurance architecture](04-trust.md) · [Eight mission areas](02-frontier.md#q8)

Jet is prerelease under the owner's current instruction. Breaking changes remain available before 1.0. The project should use that freedom to improve foundations first, complete trust and comprehension next, and qualify stability only after the resulting system is coherent. A version number, an implementation-card closure, or an old green suite does not establish current readiness.

This report answers question 30, evaluates self-hosting, and states the current evidence boundary. It does not authorize a compiler port, a release, or current Jet-version migration machinery.

<a id="q30"></a>
## Q30. Use a dependency-ordered release plan with evidence at each boundary

**Direct answer.** Finish the semantic foundations and canonical capability map; complete the implementation and evidence loop; prove the selected compiler contract; demonstrate understandable end-to-end work in every mission area; then qualify one frozen candidate under the existing hardening, platform, performance, and owner-acceptance gates. Keep external contracts after 1.0. Do not stabilize an incoherent system merely to reach a date.

This is a dependency plan, not a promise of calendar duration. Work that does not depend on an unresolved choice can continue. The proof ballot blocks only the selected proof/release boundary, not factual collection, denominator work, diagnostics, or existing implementation obligations.

| Stage | Deliverable | Evidence required to leave the stage | Existing/new ownership | What cannot substitute |
|---|---|---|---|---|
| 1. Establish current truth | One authoritative distinction between ratified law, current implementation, and current proof | Capability and claim census, explicit prerelease wording, preserved history, exact candidate identity when one exists | #2927; #2285/#2286/#2898/#2902/#2903 | Version banner, packet row count, or old card state |
| 2. Complete semantic foundations | One behavior contract carried through checking, shared operations, runtime, and adapters | Source ownership and full-path implementation; each accepted form has a route and an observable obligation | #2888/#2898 and existing semantic owners; #2920–#2923 findings remain tracked | A common IR data type with four independent meanings |
| 3. Establish compiler assurance | Checked obligations across all Jet-controlled stages under D-COMPILER-PROOF1=A, plus the existing test portfolio | Replayable proofs/checks, explicit assumptions, counterexample sensitivity, no unknown promoted to proved | #2925; #2934–#2941/#2944; reuse #1127/#1131/#2644 | A toy model, bounded validator, trusted unchecked assertion, or only passing examples |
| 4. Complete comprehension and tools | The ordinary task is easy to explain, predict, modify, and derive | First-party census, deterministic task oracles, real command/editor exercise, separately reported beginner studies | #2926; #2945–#2948; #1918/#1034–#1037/#2389/#2505/#2506/#2900/#2906 | Generated docs alone, simulated RLI5 alone, or a mandatory studio |
| 5. Qualify real work | Complete workflows across all eight mission areas, including mixed-language and adverse cases | Matched correctness, latency, resource, portability, and failure evidence; owner-visible gaps | Domain cards; #822/#825/#1344/#2858/#2859 and other existing owners | One polished demo, aggregate average, or fastest cherry-picked cell |
| 6. Freeze and qualify a candidate | One reconstructible release candidate with no known disqualifying defect | Current build, conformance, property/differential/mutation evidence, supported platforms, install/recovery, performance, owner visual acceptance | #2942/#2943/#2944/#2952; #2919; #211/#806; #2334–#2343/#2420 | Evidence from a different commit/binary or implementation-only closure |
| 7. Complete professional handoff | A stable result under sustained broad pressure and independent domain challenge | The ratified clean window, breadth/volume floor, fresh-context quota, zero open P0 on the same candidate | D-HARDENING-GATE1 and #2420; dashboard #2339 | A signed FAILED receipt, elapsed idle time, hidden exclusions, or narrow easy-case volume |
| 8. Declare 1.0 and preserve external contracts | Owner-approved supported surface and compatibility promise | All selected gates pass, explicit support matrix and release artifacts, owner acceptance | Existing release laws applied to the real candidate; any new external policy change requires a ballot | Date pressure, a package version bump, or deletion of history |

Stages overlap where their inputs permit it. For example, learning census work and domain-oracle design can proceed while semantic implementation continues. Candidate-bound qualification cannot honestly finish before the candidate exists.

<a id="current-evidence"></a>
## Current result: no fresh candidate was produced

The authorized baseline command was:

```text
scripts/agent/jet-env cargo build --bin jet
```

It failed on the pre-existing dirty tree with exit code 101 and 161 errors reported in `jet-comptime`. A captured example is:

```text
error[E0433]: cannot find module or crate `jet_std`
crates/jet-codegen/src/Prelude/CoreLib/Top/WebTable.rs
```

The investigation did not repair that source, retry a broad build, run project gates, or substitute an older executable as current proof. Therefore it establishes no fresh Jet runtime result, current tier parity, current performance ranking, or release readiness. Historical observations retain their original identity and limits.

The separate JavaScript [research demonstrations](05-correctness.md#model-results) ran outside Jet. They clarify premises about stale facts, entity identity, numerical rewrites, and clock representation. They do not change this compiler result.

### Existing hardening law is stronger than “the suite passed”

D-HARDENING-GATE1=A requires 14 consecutive clean days, 10,000,000 valid differential cases, and at least 100 valid mutations for each eligible public callable. It also requires eight fresh-context lanes in four waves of two, with counted owner-ratified exclusions. A silent-data finding resets the clean window. A semantic change invalidates candidate-bound window and challenge evidence.

Those numbers are ratified requirements, not achieved results in this report. They are not a mathematical proof either. The new compiler-proof ballot complements them; it does not replace or weaken them.

The [hardening rig](../../../scripts/agent/hardening-rig.mjs) and #2339 dashboard are designed to keep missing, refused, stale, unsupported, and failed evidence visible. Historical Tower records also distinguish implementation of the rig from the final outcome on #2420. A protocol can correctly produce a FAILED receipt. “Verified receipt” must not be shortened to “verified Jet.”

<a id="release-truth"></a>
## Current-state documentation must not turn historical policy into achieved readiness

A source-only second read found one confirmed current-state contradiction: the versioning reference versus the owner's prerelease ruling. The comparison below also records consistent foreign-adoption bounds; those are not another defect.

| Authority or artifact | What it says | Correct disposition |
|---|---|---|
| Current owner alignment, 2026-09-04 | Jet is pre-1.0; version strings are not readiness evidence; breaking changes remain open; no current migration support is required | Governs this campaign and all readiness claims here. |
| [Versioning reference](../../spec/versioning.md), lines 3–22 in the captured read | v1.0.0 shipped, Jet is post-v1.0, and breaking changes require a major version plus formatter migration | Current-state wording needs correction under #2927. It is not evidence that a stable release happened. |
| [Ratified release policy](../../spec/release-policy.md) | Defines future/post-1.0 compatibility, feature-complete 1.0, edition policy, and a `Jet 1.0.0` banner | Preserve the historical law. Clarify applicability and current state; do not silently rewrite ratification. |
| [Migration tier map](../../spec/reference/migration-tier-map.md) and [import source](../../../Source/CmdImport.rs) | Consistent foreign-language subsets and binder-stub paths; Ada/Pascal command acceptance does not mean semantic conversion | Classification obligation, not an observed map/implementation contradiction. This is foreign adoption, not Jet-version migration. |

[#2927](index.md#card-2927) is the new deduplicated correction/census card. Its scope is truth labeling and capability classification, not a backdoor amendment to D-REL1–D-REL5 or the foreign adoption decisions. If implementation uncovers a genuine conflict in the future external contract, it needs a new explicit owner ballot with the old record preserved.

### Compatibility before and after 1.0

**Before 1.0:** make justified breaking changes cleanly, update the current corpus, and keep historical decisions intact as records. Do not build compatibility aliases, deprecated spellings, or present-version migration machinery merely to accommodate today's unreleased state.

**At 1.0:** publish the exact supported external contracts and support matrix that were qualified. Internal architecture may continue to improve while preserving them.

**After 1.0:** honor those external contracts. Any newly required change to their policy goes through the owner. This report does not discard already ratified release law, set a new compatibility duration, or choose a future migration tool.

<a id="self-hosting"></a>
## Self-hosting is a workload and a trust boundary, not a readiness certificate

**Recommendation.** Evaluate compiler-shaped Jet programs before deciding whether a full self-hosted compiler is required. Do not make self-hosting a new 1.0 gate now. Keep the existing bootstrap-readiness card and its owner sign-off boundary. The owner's current request reopens evaluation, not authorization to start a port.

| Possible benefit | Evidence that would demonstrate it | Why self-hosting alone is insufficient |
|---|---|---|
| Stress complex recursive data, ownership, and diagnostics | A real parser, formatter, interpreter, or compiler component with negative cases and cold-read review | A compiler can avoid difficult language features or carry private exceptions. |
| Improve day-to-day dogfooding | Developers complete ordinary compiler work with the public tools and no special path | Being written in Jet does not prove the tools are clear to newcomers. |
| Reduce implementation-language mismatch | Shared semantic data and generated contracts are simpler and remain correct | Porting duplicated semantics can preserve the duplication. |
| Support a verified bootstrap story | An explicit theorem/checker chain, stage identities, and trusted bootstrap boundary | Recompiling itself is a fixed point, not proof that compilation preserves meaning. |
| Test serious package and build scale | A multi-package compiler-shaped workload with incremental/cold measurements | One build on one host does not qualify all supported deployment paths. |

[CakeML](https://cakeml.org/) provides a relevant example of verified compilation and bootstrapping tied to formal semantics. [Zig's 0.10.0 release notes](https://ziglang.org/download/0.10.0/release-notes.html) show self-hosting as an implementation transition with concrete compiler capabilities. Neither establishes that an arbitrary self-hosted compiler is correct, nor that Jet must choose the same architecture.

Existing [#217](index.md#card-217) already demands a dogfood portfolio, memory-model soundness work, a ratified-surface sweep, and an owner sign-off before the port. Its historical log deferred bootstrap planning. This report does not mutate that record or restart the port. The bounded evaluation should supply the portfolio evidence and then let the existing gate determine whether a new decision is ready.

A useful evaluation set is already named there: formatter/interpreter work, arena/index-handle ASTs, long-lived concurrent services, comptime-heavy programs, FFI-heavy programs, and large multi-package workspaces. Those are valuable even if Jet never self-hosts.

<a id="owner-gates"></a>
## Owner gates: proof policy ratified, six bounded design choices published

| Gate | Current disposition | Work that can continue |
|---|---|---|
| Compiler assurance required before 1.0 | D-COMPILER-PROOF1=A is ratified on #2925. It requires implementation-bound preservation for all Jet stages, including compile-time and Core implementations, with explicit foreign-tool assumptions. [Current design snapshot](design-delivery.json). | #2934–#2941/#2944 define delivery. D-COMPILER-PROOF-TOOLS1 separately chooses the proof toolchain; no physical-machine theorem or silent I9 exception is approved. |
| Concrete tool, API, and presentation choices | Six complete short ballots cover proof tooling, derivation records, editor views, learning sequence, Shared revisions, and test comparison. [Full choices and dependencies](design-and-delivery-index.md). | Implement only ratified options; independent existing-law obligations remain actionable. No new keyword/sigil or parallel semantic mechanism is proposed. |
| Bootstrap/full compiler port | Existing #217 readiness and owner-sign-off boundary. Evaluation only in this campaign. | Compiler-shaped dogfood and explicit benefit/trust analysis. |
| Actual release readiness | Existing conformance, clean-window, challenge, P0, platform, performance, visual, and owner-acceptance gates | Implement and gather evidence under their owners. No release is announced here. |
| New language surface or external dependency from research | Not approved by reading a paper or writing this report | Bounded experiment using current mechanisms; a concrete later ballot only if a genuine product choice remains. |
| Visual HTML design | Owner authorized one Fable 5.1 pass on the rendered report set; Sol executes presentation changes | Astra retains the report's copy and substantive judgments. This review is not technical research verification. |

The finalized D-PLACE1, D-ACCEL1, and D-LOOPREAD1 records receive no further writes. D-TIER-ONEIR1, the fact laws, proof-tool laws, learning laws, and foreign-adoption laws are reused rather than reopened. Bugs #2920–#2923 remain carded; this report does not close them.

<a id="definition-of-ready"></a>
## What the owner should be able to inspect before 1.0

The final readiness view should be derived from evidence and answer these questions without repository archaeology:

1. Which exact candidate, source, toolchain, binary, target, and dependency identities were qualified?
2. What is the complete advertised capability denominator, including every counted unsupported or excluded cell?
3. Which facts are proved, independently checked, bounded, tested, historical, missing, refused, or stale?
4. Which negative controls show that the evidence would reject a plausible wrong implementation?
5. Do all applicable execution modes preserve the same allowed values, failures, effects, order, and lifecycle behavior?
6. Does each of the eight mission areas complete a real workflow with the required correctness, resource, and performance contract?
7. Can a newcomer explain, predict, modify, and derive the ordinary path without external rescue, while an expert can inspect or constrain it?
8. Can a supported clean machine install, build, run, debug, recover, and produce a reproducible report?
9. Are the required sustained qualification window and independent challenge results current and passing, with no open disqualifying defect?
10. Has the owner inspected the runnable visual surfaces through exact launch and interaction instructions and accepted the external contract?

A single green percentage cannot answer those questions. Nor should the owner have to read 2,717 evidence rows to discover a missing obligation. The [master](index.md), [coverage map](coverage.json), and [capability ledger](capability-ledger.json) are the research-stage version of that view, with current limits intact.

## Generalize every finding

| Finding | Defect shape | Predicted other instances | Structural correction | Card disposition |
|---|---|---|---|---|
| Version wording claims an achieved release | Historical/future policy projected as current state | Help banners, readmes, package metadata, generated docs | One current-state source plus preserved historical law and claim census | New #2927 |
| Implementation closure appears to mean qualification | Mechanism evidence widened to outcome | Hardening rig, performance harness, compiler adapters, visual features | Separate implementation, execution, candidate, and owner acceptance | Reuse #2919/#2420/#2339 |
| Old green evidence survives semantic change | Evidence identity/invalidation missing | Proof cache, CI artifact, docs example, dashboard | Candidate-bound receipts and invalidation | Reuse #2506/#2335/#2339 |
| One workload becomes a whole-language gate | Proxy mistaken for coverage | Self-hosting, demo app, benchmark, tutorial | Portfolio and denominator; direct obligation per claim | Reuse #217/#2858/#2900; new #2926 |
| Research mechanism is treated as adopted product | Feasibility evidence bypasses owner law | Solvers, proof tools, new type/effect syntax, studio | Explicit promotion gate and scoped ballot | New #2925/D-COMPILER-PROOF1; no dependency approval |
| Stability target suppresses foundational improvement | Date prioritized over qualified contract | Compatibility shims, premature syntax freeze, hidden exclusions | Use prerelease freedom; qualify the final combined design | Current owner ruling; no new migration machinery |
