# Using Jet to build Jetpack: experience report

## Executive decision

This report's mechanical audit counted 3,064 physical lines in the integrated Jet canary. That is a retrospective scope count, not a matched language or phase-success result. The campaign ledger records Phase 1 as `blocked; not green`, and Phases 2 and 3 as `not reached; not measured` (`dogfood/jetpack/METRICS.md:28-32`).

The retrospective scorecard rated Jet reading highest among its activities. It did not compare equivalent Rust work. Jet was harder to create, modify, and debug than its final source size suggests. Compiler defects, Core coverage gaps, ownership edits, failure-domain rules, weak project-aware checking, and non-idiomatic branch-heavy code all added friction.

The original implementing-agent survey made no matched preference claim. The sealed `2393-r1` run recorded 9/9 measured blind choices for Rust; A04 had no answer. No campaign preference or success is claimed.

The final source contains the language ideas discussed below. Its final size does not measure development speed. The current product loop still requires too many correction passes.

The owner has paused this parity drive. The Rust Jetpack is now the sole active implementation until it is fully functional, reliable, and stable. The existing Jet canary remains evidence. It must not become a second maintenance stream. This pause does not reject eventual replacement.

## What this report measures

This report combines five evidence sets:

1. The integrated Jet source under `dogfood/jetpack/`.
2. The campaign ledger in `dogfood/jetpack/METRICS.md` and the comparison report in `docs/audits/dogfood-jetpack-2026-08-28.md`.
3. Tower findings `#1310`, `#2252`, `#2350`, `#2352`, `#2354`, `#2355`, `#2368`, `#2369`, `#2370`, and `#2371`.
4. Retrospectives from ten agents that wrote or corrected the model, plan, CLI, entry, and parity-test slices.
5. The sealed `2393-r1` verdict and raw receipt manifest for the 2026-08-31 execution.

The agent survey is retrospective. It records model preference and willingness to use Jet again. It does not claim human emotion. Some lanes were explicitly forbidden from running validation, so their diagnostics scores describe limited direct exposure.

No surveyed agent implemented the equivalent Rust slice. `METRICS.md` marks Rust matched source counts and the current binary, startup, latency, build, LSP, and error-cascade rows `not measured` (`dogfood/jetpack/METRICS.md:51-70,102-149`). The only matched Jet/Rust results added here are the sealed `2393-r1` task outcomes and blind choices, reported in the addendum below.

## Sealed `2393-r1` result (2026-08-31)

The frozen rerun produced a valid partial execution, not a passing 5/5 campaign. The sealed [verdict](fresh-agent-5-of-5-rerun-2026-08-31.md) and [receipt manifest](raw/2393-r1/manifest.json) report:

| Measure | Result | Meaning |
| --- | --- | --- |
| Jet project-green tasks | 27/36 measured tasks green | Jet task gate fails. |
| Rust project-green tasks | 36/36 measured tasks green | Rust task evidence only; it is not the Jet gate. |
| Blind preference | 9/9 measured choices were Rust; A04 had no measured choice | The preference gate fails. |

The fixed participant denominator remains 10. A04 is not treated as zero or omitted. These measurements do not establish Jet preference, performance, source-density, or campaign success (`fresh-agent-5-of-5-rerun-2026-08-31.md:10-20,126-138`).

## The short answer: did agents enjoy Jet more than Rust?

Not established.

The strongest common retrospective answer was: Jet was pleasant to read and promising for small typed tools, but not yet ready for load-bearing package tooling. Agents liked the compact data model, explicit error types, records, collection literals, `#Codable`, `#Test`, `??`, and visible copy boundaries. They disliked correction churn around `~`, error-domain propagation, package entry resolution, module checking, direct Result matching, and tier-specific compiler failures.

The ten-agent scorecard was:

| Activity | Mean | Median | Range | Reading |
| --- | ---: | ---: | ---: | --- |
| Reading Jet | 3.6/5 | 4.0 | 3–4 | Clear types and compact records were the strongest part. |
| Writing Jet | 2.9/5 | 3.0 | 2–3 | Concise surface, but ownership and failure rules caused retries. |
| Reasoning about Jet | 3.1/5 | 3.0 | 3–4 | Explicit effects helped; stringly parser state and nested branches hurt. |
| Creating new Jet code | 2.8/5 | 3.0 | 2–3 | Examples helped, but agents searched widely for canonical forms. |
| Modifying Jet code | 2.8/5 | 3.0 | 2–4 | Small edits often changed ownership or failure requirements upstream. |
| Diagnostics and debugging | 2.6/5 | 2.5 | 1–4 | Exact checks could be excellent; root-cause localization and tier coverage were uneven. |
| Tooling and docs | 2.7/5 | 2.5 | 2–4 | Examples were useful; project-aware checking and authoring guidance were incomplete. |

Reading was the only activity above the report's positive threshold. This scorecard measured Jet activity ratings only, not a matched Jet/Rust preference.

## What agents liked

### Typed failure is visible and compact

Agents repeatedly described signatures such as `String !ParseError` as compact within the Jet port. A local `#Error` record made the failure role visible beside the data model.

`??` also read well at Core boundaries. It kept the success path short and made fallback or error conversion local.

This strength weakened when helpers that looked pure also needed the caller's failure domain. `Bool !ParseError` plus `Ok(false)` surprised several agents. The concept remained understandable after discovery, but it was not self-teaching.

### Data declarations are dense

Jet records, list and map literals, interpolation, `#Codable`, and `#Test` made models and test fixtures short. The read-only report path showed the intended result: typed report records serialized with little visible plumbing.

The report's broader mechanical audit counted 3,064 Jet lines. The current `METRICS.md` capture covers nine implementation files and records 1,961 Jet lines and 6,379 whitespace tokens; its matched Rust LOC and tokens are `not measured` (`dogfood/jetpack/METRICS.md:51-70`). These scopes differ, so this report makes no Jet/Rust source-density claim.

### Ownership is auditable

Agents understood the value of `~`. It exposes a copy or retained ownership boundary instead of hiding it. The plan and transcript agents said this helped reasoning once code was stable.

The cost appeared during editing. `trim`, `split_once`, `.before`, `.after`, path operations, Core calls, record construction, and reused arrays all created sites where a missing `~` caused another correction pass.

### Diagnostics can be strong

Agents with a complete `jet check` loop scored diagnostics as high as 4/5. `E0209`, `E2404`, `E0109`, `E0121`, `E0602`, `E0358`, and `L0507` often named the exact site and a concrete fix.

The original report described a seeded error-cascade comparison, but the current `METRICS.md` error-cascade rows are `not measured` (`dogfood/jetpack/METRICS.md:144-149`). In `2393-r1`, seeded duplicate, unknown, and rename failures were named and resolved in the measured command set. This does not establish a cross-language diagnostic advantage (`fresh-agent-5-of-5-rerun-2026-08-31.md:109-115`).

This strength was inconsistent. Several root causes appeared as many downstream errors, or only after default-tier or AOT execution.

### Runtime and build measurements are unavailable

`METRICS.md` marks binary size, startup, verb latency, cold and warm build, first-result, and LSP rows `not measured` at the current integration point (`dogfood/jetpack/METRICS.md:102-149`). The earlier draft's binary, startup, latency, build, and peak-memory figures therefore do not count as measurements here. They are not used as results in this report.

The sealed rerun preserved per-command timers where supplied, but its published cross-language result is the 27/36 versus 36/36 project-green task count, not a binary or build-speed comparison (`fresh-agent-5-of-5-rerun-2026-08-31.md:10-22,33-50`).

## Where authoring was difficult

### Failure domains spread farther than expected

This was the most repeated structural complaint.

Helpers such as `safe_name`, `duplicate_package`, `valid_kind`, `has_space`, `slash`, and `is_under` needed explicit caller-compatible failure domains. Normal returns then needed `Ok(...)`. Public fallible functions also required public error carriers and public fields.

The rules are defensible. The experience was not. Agents often learned the rule only after `E2404` appeared at several call sites. The diagnostic did not always identify the helper whose inferred or default error domain caused the mismatch.

The direct-match bug made this worse. A documented `if call() == { .Ok(...) ... .Err(...) ... }` sometimes propagated the error instead of binding it. That forced `??` workarounds and deeper control flow until `#2354` was fixed.

### Copy boundaries were clear but noisy

The port made extensive use of strings, path views, split views, arrays, and maps. This is the exact workload that stresses explicit retention.

Agents did not object to visible copying. They objected to finding the correct boundary by repeated compiler passes. A better diagnostic would name the consuming callee, the later reuse, and the precise site that needs `~`.

The current experience makes safe code possible. It does not yet make safe code quick to write.

### Package and module behavior was surprising

The entry lane needed several attempts before a package output resolved. A root function in `entry.jet` and a transitive import did not behave as expected. A runner adapter was required until `#2352` fixed nested imported output resolution.

Relative `..` imports were rejected by design. The canonical project-root imports were correct, but isolated file checks could not resolve modules missing from a worker worktree. This made correct imports look broken during development.

The broader issue is feedback scope. `jet check` can report a file or package as clean while default-tier or AOT paths still fail. For package tooling, the normal check loop must expose module graph, output entry, Core reachability, and tier lowering problems before runtime.

### The standard library did not remove enough parsing work

The port hand-built parsers for `package.jet`, `.jet/lock`, profiles, selectors, journal records, and report JSON. `plan.jet` reached 970 lines before formatting and ended at 832 lines.

This was not only compiler friction. The dogfood port could not reuse Jetpack’s internal Rust package model as a public Jet library. Agents therefore wrote line scanners, quote scanners, brace depth trackers, string-key dispatch, canonical framing, and manual JSON.

A typed source-backed package/profile parser and deterministic JSON or framing builder would remove much of this work. Such an API should preserve spans, ordering, exact bytes, and error provenance.

### Warm rebuilds remain unmeasured

The native-cache identity bug is recorded as fixed in the bug table. That closure does not provide a warm timing result.

`METRICS.md` marks both cold and warm build rows `not measured` at the current integration point (`dogfood/jetpack/METRICS.md:123-128`). This report makes no Jet/Rust warm-speed claim.

Source inspection identifies package entry discovery, the programmable-build front end, semantic-index program-value construction, and native key or fingerprint derivation before the final cache lookup. This is a design observation, not timing evidence.

Demand-driven graph work remains a recommendation. It must not be presented as a measured performance result.

## The non-idiomatic Jet problem

The owner’s observation is correct. The integrated port contains too many classic guard branches and too few ordered arm tables.

A mechanical audit of the twelve implementation files found:

- 3,064 physical lines.
- 430 `if` tokens.
- 24 ordered arm tables, including 13 subject tables and 11 subjectless tables.
- 135 lines beginning with a simple string equality or inequality branch.
- 15 file-and-subject groups with at least three categorical string comparisons.

Not every guard should become a table. Early validation such as `if path == "" -> return Err(...)` is often clearer as a guard. The problem is categorical dispatch and mutually exclusive parser state expressed as adjacent guards.

### Worst examples

| File and symbol | Evidence | Better Jet shape |
| --- | --- | --- |
| `src/cli/main.jet::parse_command` | 64 `if` tokens in the 309-line file. `pending` is a string with five adjacent cases. Flags and verbs use repeated equality chains. | Use typed command and pending-option variants. Dispatch with `if pending == { ... }` and `if verb == { ... }`. |
| `src/cli/main.jet::execute` | Four adjacent `command.verb` branches sit inside nested Result tables. Result handling reaches five visible nesting levels. | Bind or handle each Result once, then use one exhaustive verb table. |
| `src/model/manifest.jet::kind_prefix` | Five package kinds, each with case and spacing variants, use repeated `if` statements. The file has 54 `if` tokens and no ordered table. | Normalize once, then use a subject table for the finite kind set. |
| `src/model/manifest.jet::parse_manifest` | Section, target-record, and key state use booleans plus nested `if` blocks. | Use typed parser state and tables over section and field keys. |
| `src/lock/lock.jet::parse_lock` | Field keys and record headers use adjacent string guards. The file has 48 `if` tokens and no ordered table. | Use record-state and field-key tables while keeping early validation guards. |
| `src/plan/plan.jet::parse_selector` | The subject `key` has nine categorical equality branches across profile and selector parsing. | Use one subject table with value alternatives such as `"revision" | "rev"`. |
| `src/store/journal.jet::parse_transaction` | Record kinds `delete`, `record`, `object`, `output`, and `reference` use sequential guards. | Dispatch once on the first field, then validate the selected record shape. |
| `src/realize/realize.jet::realize` | Providers use two classic branches and a final error. | Use one provider table with `"local" | "prefetched"`, `"recorded"`, and `else`. |

The source already proves that agents can use ordered tables. Result handling and several plan branches use them correctly. The issue was habit and discoverability, not inability.

Several agents said they transferred Rust or C-style branch habits. Others avoided tables because direct Result matching was broken, module graphs were incomplete, or they were preserving behavior under a narrow correction brief.

The current lint does not cover the main failure. `L0507` catches chained `else if` forms and multiline branches with an `else`. It does not catch adjacent same-subject categorical guards. A file can therefore pass the style lint while still using ten independent `if key == "..."` statements as a dispatch table.

The right compiler improvement is narrow: detect adjacent exclusive branches over the same stable subject and suggest one ordered table. It must not warn on independent validation guards or state changes that make the conditions non-equivalent.

## Port design choices that made Jet harder to judge

These are dogfood implementation choices, not language defects.

### Stringly typed state

`ParsedCommand.verb`, `pending`, providers, manifest kinds, parser sections, selector keys, and journal record kinds are strings. Many also have parallel `seen_*` booleans.

Tags or enums would make the finite states explicit and let exhaustiveness help. They would also remove impossible combinations such as `pending == "digest"` with an unrelated verb.

### One large plan module

`plan.jet` contains parsing, inheritance, collision merging, selector parsing, source classification, hashing, provenance, and JSON rendering. At 832 lines and 75 LSP symbols, it is readable in pieces but not deep as a module.

A better human-authored structure would separate parsing, resolved profile facts, and rendering while keeping the declared Jet package boundary unchanged.

### Manual wire JSON

The CLI builds some reports with escaped string interpolation, even though other modules use `#Codable` records. This happened partly because Codable default-tier behavior exposed `#1310` and because the CLI needed exact wire output.

Typed report values with one deterministic encoder would be easier to read and safer to modify.

### Inconsistent error carriers

Model and CLI code use typed error records. The journal still uses `!Err` and raw string errors. The inconsistency weakens the otherwise strong failure model.

### Delayed validation in worker lanes

Several workers were told not to run checks. One large plan contribution then reached integration with 61 diagnostics. Correction agents fixed ownership, option, error, and syntax issues afterward.

This is a campaign-process confound. It made Jet feel worse, but it is not solely a Jet language result. The product lesson remains valid: a fast project-aware check must be cheap enough that every authoring lane can use it continuously.

## Structural friction versus bugs

### Structural friction

| Area | Structural issue | Product consequence | Direction |
| --- | --- | --- | --- |
| Failure typing | Caller-compatible error domains and `Ok` returns spread through pure-looking helpers. | Small edits create signature and call-site churn. | Improve inference boundaries or diagnostics without hiding effects. |
| Ownership | `~` is explicit but frequent around strings, views, paths, and collections. | Safe code is auditable but slow to converge. | Diagnose the consuming callee and later reuse; suggest the exact materialization site. |
| Branch idiom | Adjacent categorical guards bypass `L0507`. | Rust-shaped code survives in Jet and loses exhaustiveness. | Add a same-subject dispatch lint and stronger examples or formatter assists. |
| Parsing APIs | No public typed package/profile parser or deterministic framing builder served the port. | Hundreds of lines of scanners and string dispatch. | Expose one source-backed model with spans and deterministic encoding. |
| Project checks | File checks and package runtime exercise different reachability. | Clean checks can precede JIT or AOT failures. | Make the normal project check cover output resolution, module graph, Core closure, and tier lowering. |
| Module architecture | One package and one 832-line plan module carry many responsibilities. | Local reasoning is good; change isolation is weaker. | Deepen modules inside the user-declared package. Never auto-change package boundaries. |
| Incrementality | Final cache lookup follows substantial front-end work by source inspection. | Warm-build effect is not measured. | Use one demand-driven query and action graph with layered invalidation. |

### Compiler, Core, and tooling bugs found by dogfood

| Card | Defect | User impact | State at closeout |
| --- | --- | --- | --- |
| `#1310` | Canonical Codable semantics did not survive whole-program TIR deopt. | Ordinary generated error/report types could make the default tier fail. | Reopened by dogfood; broader card remains ready. |
| `#2252` | Default-tier evaluator and Core coverage gaps reached `E0956`. | File operations, generated field receivers, and error branches blocked normal `jet run`. | Dogfood regressions fixed and card closed. |
| `#2350` | Imported modules emitted unqualified root helpers and wrong error types in AOT. | A checked package failed with internal exit 101. | Fixed and closed. |
| `#2352` | Package outputs could not resolve a two-directory imported entry. | A trivial package entry needed a runner adapter. | Fixed and closed. |
| `#2354` | Direct matching of struct-typed errors propagated instead of binding. | Documented Result tables failed and forced nested workarounds. | Fixed and closed. |
| `#2368` | Formatter rewrote return match arms into invalid Jet. | Formatting could break compiling source. | P0 fixed and closed. |
| `#2369` | Compiler `--version` handling intercepted application argv after `--`. | Valid Jetpack commands never reached the program. | P0 fixed and closed. |
| `#2370` | LSP returned no symbols for an imported package module. | The 832-line plan file had no symbol navigation. | Fixed and closed; 75 symbols now return. |
| `#2371` | Hidden Rust FFI bridge identity bypassed native cache keys. | The cache key omitted this identity; current cold and warm timings remain `not measured`. | Cache-key bug fixed and closed; no performance result claimed. |
| `#2355` | Shipped Rust Jetpack created a lock file during read-only list. | The oracle changed the store during inspection. | Rust bug fixed; dogfood normalization deleted. |

The companion ledger maps these ten rows as F29-F38 in table order. In particular, F34 owns `#2368`, and F38 owns `#2355`.

The bug count matters because it contaminated every subjective judgment. Agents were not only learning a new language. They were also crossing incorrect compiler behavior, incomplete tier support, broken formatter output, missing LSP symbols, and package-entry defects.

## Jet versus Rust: measured limits

### Sealed task and preference measurements

The only matched cross-language measurements in this report are from sealed `2393-r1` receipts:

- Jet reached 27/36 measured project-green task receipts.
- Rust reached 36/36 measured project-green task receipts.
- Nine of nine measured blind choices selected Rust. A04 had no measured choice.

The verdict is `FAIL — fixed 5/5 gate not met`. These results do not establish a Jet preference, a product-wide Rust advantage, or campaign success (`fresh-agent-5-of-5-rerun-2026-08-31.md:10-20,126-138`).

### Original retrospective evidence

The original ten-agent table records Jet activity ratings and qualitative testimony. It does not contain matched Rust authoring tasks. Those rows support claims about the reported Jet experience, not a Jet-over-Rust preference.

### Not measured

- Matched full-feature LOC and token counts.
- Binary size, startup, verb latency, cold build, warm build, first-result latency, peak memory, and LSP timing at the current integration point.
- Matched authoring time by equally experienced Jet and Rust developers.
- Long-term maintenance cost.
- A complete network-provider and package-universe parity run.
- Whether experts prefer Jet after compiler bugs and idiom problems are removed.

## What should change before another self-hosting port

1. **Make canonical Jet visible.** Teach and lint ordered same-subject dispatch. Keep early validation guards legal and quiet.
2. **Fix the authoring loop.** One project-aware check should expose module, output, Core, JIT, interpreter, and AOT reachability problems before execution.
3. **Improve ownership diagnostics.** Name the consuming call, later reuse, and exact `~` insertion point.
4. **Improve failure-domain diagnostics.** Name the helper that inferred the wrong carrier and show the required `!ErrorType` signature.
5. **Expose typed package and profile models.** Do not make every tool reparse Jet’s own package language with line scanners.
6. **Provide deterministic structured-output builders.** Exact JSON and framing should not require escaped interpolation.
7. **Complete demand-driven incrementality.** Skip unchanged item, file, module, package, Core, and action work before the final binary cache gate.
8. **Keep package boundaries sovereign.** Compiler automation may optimize inside declared packages and across declared dependency edges. It must never redefine user package or subpackage structure.
9. **Resume Jetpack parity only after owner approval.** The Rust implementation must stabilize first. Do not pay maintenance twice.

## Owner direction recorded

The pause is recorded in two durable places:

- Tower card `#2327` contains the owner directive.
- `docs/agents/prompts/dogfood-jetpack.md` now starts with a stop notice and clarifies that side-by-side isolation was a campaign rule, not a permanent rejection of replacement.

The current Jet source remains a useful canary, benchmark fixture, and compiler regression corpus. It is not an active second Jetpack product.

## Implementer testimony by slice

These are original retrospective answers. They are not matched measurements and do not override the sealed `2393-r1` result above.

| Agent | Slice | Would use Jet again? | Strongest positive | Strongest friction | Highest-value improvement |
| --- | --- | --- | --- | --- | --- |
| `JetpackModelMax` | Manifest, reference, lock models | Yes for small deterministic parsers; no production preference | Compact structs, arrays, and fallible signatures | Hand-written state machines and brace or interpolation confusion | Canonical table-driven parsing guidance |
| `JetpackPlanMax` | Plan and profile rendering | Cautious yes for small read-only tools | Typed contracts, interpolation, collections, useful diagnostics | 970-line scanner and renderer; repeated ownership edits | Typed profile parser and deterministic JSON or framing builder |
| `JetpackReadOnlyMax` | Read-only store reports and CLI | Cautious yes for bounded utilities | `#Codable` report records | Project-root imports and incomplete module checks | Project-aware checking from any source file |
| `JetpackCLIFixMax` | CLI correction | Conditional yes | Compact `!CLIError`, `Ok`, `Err`, and `??` | Direct fallible-call matching propagated instead of binding | Reliable Result carrier matching |
| `JetpackEntryFixMax` | Package entry and runner | Conditional yes after Core and entry stability | Small records and typed errors | Entry resolution required several adapters and checks | Make package entry resolution follow the checked source graph |
| `JetpackTranscriptMax` | Transcript and parity harness | Conditional yes for small tests | Concise records and process chains | Reusing values across consuming Core APIs required many `~` copies | Better move diagnostics with callee and reuse sites |
| `JetpackPhase1ParityMax` | Integrated parity correction | Yes for small single-module tests | `#Codable`, `#Test`, explicit errors | Cross-module AOT and test reachability | Reliable modular AOT test codegen and discovery |
| `FixManifestErrorsMax` | Manifest and reference failure repair | Guarded yes for small typed parsers | Local typed error carriers | Error-domain mismatches surfaced at call sites | Failure-domain fix-its naming the source helper |
| `FixModelSyntaxMax` | Manifest and lock syntax repair | Conditional; not package tooling yet | Terse errors and records | View materialization and literal-brace syntax | Ownership-boundary diagnostics |
| `FixLockErrorsMax` | Lock error repair | Conditional yes | `String !ParseError` is compact | Helper error domains and public error visibility | First-class failure-domain and visibility guidance |

## Final assessment

Jet can express the canary workload, and the retrospective report records specific Jet activity ratings and source observations. `METRICS.md` does not provide matched Rust source or current runtime measurements.

The sealed `2393-r1` run measured 27/36 Jet project-green tasks, 36/36 Rust project-green tasks, and 9/9 measured blind choices for Rust. Its verdict is `FAIL`, not campaign success.

The experience still falls short of “simple, enjoyable, and frictionless.” The final source hides how much correction work occurred. The most important gap is not syntax volume. It is the distance between the first plausible Jet implementation and an idiomatic, cross-tier-correct, project-green implementation.

That distance must shrink before Jet can honestly claim a user preference over Rust. The route is concrete: canonical branch tables, typed public package models, better ownership and failure diagnostics, project-wide tier-aware checks, and earlier demand-driven cache gates.