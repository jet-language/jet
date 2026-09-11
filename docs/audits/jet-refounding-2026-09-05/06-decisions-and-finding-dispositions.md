# Choose the new boundaries; keep the mechanisms already adopted

[Executive report](index.md) · [Language and Core](02-language-and-core.md) · [Tools and systems](03-tools-and-systems.md) · [Research and release](05-research-correctness-and-release.md)

## The recommendation is a connected product, not twelve unrelated experiments

**Start with typed queries and a controlled execution world. Use them to finish a real application.** The query carries a calculation from ordinary values to files and changing data. The world makes its time, cancellation, and stale-result failures reproducible. Typed endpoint binding then exposes that application without another handwritten contract.

Columnar interchange and model packages extend the same idea across foreign boundaries. Space-safe geometry and resource scheduling extend it to games, GUI, compute, and embedded work. They do not justify a universal graph object, another scheduler, or a second language hidden in attributes.

The new choices below are **recommendations, not owner verdicts**. Their cards contain future implementation criteria. A published card or ballot does not establish that an API runs. No production implementation is part of this audit.

### The slate

| Feature | Owner choice | Recommendation and strongest alternative | Card |
|---|---|---|---|
| F01 · One endpoint contract | **D-ENDPOINT-SHAPE1** | A: derive transport binding, schema, and client from the checked handler. Alternatives: maintain separate descriptions, or make a separate typed protocol authoritative and bind the handler to it. | #2962 |
| F02 · One typed query | **D-QUERY-RETAIN1** | A: `Query<T>` plus ordinary list results; retain meaningful key/reducer types. Alternative: retain the public Table/Series/LazyFrame wrapper families with standardized operations. | #2963 |
| F03 · Live query maintenance | **D-QUERY-LIVE1** | A: maintain operations with checked update laws; disclose or refuse recomputation. Alternative: recompute the same query after each edit. | #2964 |
| F04 · Space-safe geometry | **D-SPACE-GEOMETRY1** | A: ordinary generic space and transform types with useful domain names. Alternative: application-specific records and conversions. | #2965 |
| F05 · Controlled execution world | **D-TEST-WORLD1** | A: virtual time and controlled providers at existing effect boundaries. Alternative: thread fake clocks and schedulers through production interfaces. | #2966 |
| F06 · Resource schedules | **D-RESOURCE-SCHEDULE1** | A: derive dependencies from checked accesses and actual completion. Alternative: require a separately declared resource graph. | #2967 |
| F07 · Shared columnar data | **D-COLUMNAR-BOUNDARY1** | A: a checked owner over compatible Arrow C buffers. Alternative: always materialize independently owned rows. The ABI itself adds no Arrow software dependency. | #2968 |
| F07 · Real Parquet input | **D-DATA-READER1** | A: pin the official `parquet` 59.3.0 reader behind the data provider. Alternative: a Jet-owned reader with the same complete format and target contract. | #2969 |
| F08 · Typed model package | **D-MODEL-PACKAGE1** | A: one package binds weights, preprocessing, typed signature, resources, and embedding identity. Alternative: application-owned raw-tensor wrappers. | #2970 |
| F08 · Model execution provider | **D-MODEL-BACKEND1** | A: pin ONNX Runtime 1.29.0 behind the existing foreign/package boundary. Alternative: a Jet-owned engine with equivalent final obligations. | #2971 |
| F09 · One ordinary run command | **D-RUN-PREPARE1** | A: `jet run` requests admitted environment preparation from Jetpack; `--no-prepare` refuses it. Alternative: keep `jetpack env --prep` separate. | #2972 |
| F10 · Hostile histories | **D-TEST-HISTORY1** | A: typed operation generation, independent observations, and validity-preserving shrinking. Alternative: selected handwritten histories only. | #2973 |

F11 and F12 do **not** need new votes for their underlying mechanisms. One complete Core declaration is already required by the shared-IR ruling. Explanation records, source views, learning feedback, shared revisions, and comparison have already been adopted.

The two backend choices deliberately show the same ordinary Jet API in both options. Their difference is the implementation, dependency closure, and verification boundary—not a cosmetic spelling. No intrinsic product disadvantage is invented for an equally complete Jet-owned implementation. Its compatibility, performance, and proof still need evidence.

## What changes, what does not, and what the owner actually authorizes

| Choice | New public increment | Existing authority preserved | Owner-only part |
|---|---|---|---|
| Endpoint derivation | A handler-derived transport descriptor and generated client | Typed shapes, field validation, app graph, existing serve/dev default | Which declaration owns the public contract |
| Query normalization | `Query<T>`, generic grouped results, ordinary lists for materialized data | Checked SQL, eager list behavior, one-shot streams, numerical and failure rules | Clean replacement of the existing public data-container families |
| Incremental maintenance | `track`, `watch`, checked edit laws, visible recomputation/refusal | Existing LiveQuery lifecycle, reactive semantics, shared revision publication | Maintenance behavior and its memory/work policy |
| Space geometry | Typed point/delta/transform APIs and stock domain names | Carrier/knowledge, units, shapes, ordinary generics and provenance | Public geometry contract; no new keyword or unit notation |
| Controlled world | Test scope, virtual time, quiescence, explicit providers | Effects, tasks, cancellation, authority, replay and comparison | Public testing control; not a new production scheduler |
| Resource schedule | Frame scope over checked calls and resource completion | Ownership, effects, layouts, explicit placement and acceleration | New scheduling API, not a different placement/acceleration default |
| Columnar ABI | `ColumnBatch<Row>` with one owner and explicit sharing/copy results | Move/view rules, foreign trust, common data queries | Typed shared ownership boundary |
| Parquet reader | An actual file decoder behind the common typed source | Package admission, dependency pinning, query semantics and resource limits | Exact external reader scope and resolved closure |
| Model package | A typed artifact and session with semantic model/index identity | Jetpack, tensors, shapes, effects, placement, foreign boundary | Public package/session contract |
| Model provider | A real ONNX execution adapter | Same model signature, numerical policy, explicit provider controls | Exact external runtime scope and resolved closure |
| Run preparation | Explicit `jet run` delegates preparation; expert refusal flag | Jet executes source; Jetpack realizes needs; existing `--env`, trust, locked/offline rules | Ratified amendment of the existing preparation workflow and registered CLI flag |
| History generation | Operation strategies and validity-preserving shrinking | Existing comparison record, controlled effects and replay | Public test-generation contract |

**No invariant carve-out is requested.** No new grammar keyword or sigil is required by the recommended library designs. The ratified `--no-prepare` CLI flag follows the normal registry, help, documentation, and clean-cutover rules.

The report does not authorize a new compiler dependency. The Parquet and model-runtime votes are explicit provider-level dependency choices. An approved version must resolve to pinned artifacts, licenses, enabled features, target support, and provenance. A vague future “latest” package is not the chosen dependency.

### Protected and adopted decisions remain intact

- D-PLACE1, D-ACCEL1, and D-LOOPREAD1 receive no writes from this audit. The new designs consume their contracts rather than quietly changing them.
- D-COMPILER-PROOF1=A retains the whole Jet-controlled preservation obligation. Foreign assumptions remain named; the bounded model does not fill a proof cell.
- D-COMPILER-PROOF-TOOLS1, D-EXPLANATION-RECORD1, D-EXPLAIN-VIEW1, D-LEARN-FEEDBACK1, D-SHARED-REVISION1, and D-TEST-COMPARE1 are already ratified A. Their records remain historical law.
- D-TIER-ONEIR1 owns the one shared lowering and complete Core declaration route. F11 must finish that cutover, not propose another registry.
- D-LEDGER1=D deliberately separates the schema of facts, checked program facts, and run evidence. One typed query contract does not require one mutable store.
- D-CALLVALUE1=B already permits ordinary calls of named function values. The returned-function `.call(...)` rule remains unchanged.
- D-SQL-SURFACE1 already owns checked SQL. D-WEBAPP-SERVE1 already owns automatic app serving. Neither is sold as a new feature.
- D-VERDICT-2188-1 and D-VERDICT-2189-1 own the Jet/Jetpack boundary. F09 records the ratified narrow workflow amendment; it does not merge the tools or create a second resolver.
- Bugs #2920–#2923 are not refiled. Frozen #2420 remains untouched. The reported failed compiler construction is not retried.

## Every feature has a deletion obligation and an expert escape

A shorter example alone is not enough. Each proposal must remove a recurring source of duplicate work in the actual application, and it must preserve the necessary control behind that work.

| Feature | Work that disappears | Control that remains | What would invalidate the simplification claim |
|---|---|---|---|
| F01 | Separate mechanical decoder/schema/client descriptions | Explicit exposure, transport binding, authentication, authorization, business-error mapping | A hidden default leaks fields or policy still needs a second schema |
| F02 | Table/Series/LazyFrame wrapper choreography and untyped grouped results | Explicit deferral, stream ownership, order, limits, spill and numerical rules | A list becomes secretly lazy, an error disappears, or the same job needs another query language |
| F03 | A second handwritten edit-maintenance algorithm | Explicit keys, transactions, retained memory, recomputation/refusal and publication | A join, removal, Float operation, error, or stale result silently changes meaning |
| F04 | Repeated application-specific coordinate wrappers and conversion checks | Dynamic frame identity, explicit transforms, fallible inverse, depth/ray choice | Static tags claim to identify runtime scenes or the default silently discards space |
| F05 | Real sleeps, global clock replacements, fake-clock parameters unrelated to the business interface | Provider declarations, event order, budgets, task completion and uncontrolled-effect refusal | Foreign work escapes the scope or tests need a second scheduler |
| F06 | Hand-maintained read/write lists for visible Jet code | Opaque-boundary access declarations, source-order reference, placement and completion | Unknown overlap is treated as disjoint or resources die at submission rather than completion |
| F07 | Compatible data's CSV/row-copy detour and repeated foreign release glue | Explicit conversion/copy policy, no-copy refusal, immutable views and trust | Sharing hides a copy, invalid view, unchecked narrowing, or double release |
| F08 | Per-application model metadata, preprocessing, shape and index-identity glue | Runtime/provider, device transfers, numerical policy, trust and session lifetime | Matching dimensions are mistaken for compatible embeddings or a backend fabricates success |
| F09 | Environment-entry ritual before ordinary execution | Exact environment, locked/offline admission, preparation refusal, normal source execution | `jet run` becomes a second resolver, edits locks, or activates from merely opening a folder |
| F10 | Manual sequence selection and manual reproducer minimization | Independent model, action strategies, bounds, distributions, replay identity | The model calls the buggy implementation or the shrinker destroys the failing history |
| F11 | Per-consumer Core-name and signature registration chores | One complete declaration, checked coverage, explicit adapter boundary | Adding an API still requires a handwritten row in each engine or checker |
| F12 | Reconstructing facts from prose, disconnected inspectors, and guess-the-error lessons | Ordinary source, exact assumptions, keyboard/text views, explicit experiments | The workbench becomes a required canvas, hides uncertainty, or merely narrates static text |

## Dependencies follow meaning, not an arbitrary phase count

```text
Adopted foundations
  one checked meaning + complete declarations + typed facts
  existing effects/tasks + ownership + shared revision publication
  implementation-bound compiler assurance
           |
           +--> typed query --> live maintenance --> live application
           |        |
           |        +--> typed columnar source --> real Parquet reader
           |
           +--> controlled world --> generated histories
           |
           +--> space geometry --> resource schedules
           |
           +--> typed model package --> pinned execution provider
           |
           +--> handler descriptor --> generated endpoint/client
           |
           +--> source-first explanation and learning

Existing Jetpack authority --> optional run-time preparation delegation
```

The Arrow C ownership boundary and the Parquet reader are separate choices. A reader can produce typed owned data before a foreign zero-copy boundary exists. Neither vote is permission to implement another query engine.

A model package and its backend are also separate. The signature, preprocessing, lifecycle, and index identity are application contracts. The provider implements supported operators under that contract. A provider name without real model execution does not close either card.

F03 can use the reference recomputation path while a particular optimization is unproved, but it cannot claim incremental execution for that operation. An explicit required-incremental request must reject instead. This is an exposed execution choice, not an execution-tier exception.

## Retain the adopted proof work; stop presenting it as the whole new design

The earlier report was too centered on proof and explanatory tooling. Deleting legitimate adopted obligations would not repair that mistake. This report replaces its product recommendation while preserving its raw research, decisions, and necessary implementation work.

| Existing owner | Disposition in this audit | Concrete relationship to the new report |
|---|---|---|
| #2925; #2934–#2941; #2944 | Retain the full compiler assurance contract | Chapter 05 gives the preservation boundary, binding requirements, falsifiers and release role; no subset substitutes for the ratified scope |
| #2942–#2944 | Retain candidate construction and qualification work | One candidate identity binds the changed source, compiler, proof, tests and claimed capabilities |
| #2926; #2947 | Retain and sharpen learning/census work | Use actual endpoint, query, timer, space, and ownership transitions; score explain, predict, modify, and transfer rather than completion alone |
| #2945–#2946; #2948 | Retain the adopted explanation and semantic-review mechanisms | F12 is the useful source-first product built from them, not another record or comparison format |
| #2949 | Retain shared revision publication | F03/F05/F06/F12 must distinguish revision from resource generation, clock time, and device completion |
| #2950 | Retain comparison | F10 consumes the same observation contract; generated histories do not create a second equality or trace schema |
| #2951 | Sharpen the optimization experiments | Add the exact incremental-query and resource-schedule candidates, failed Float law, and legality-before-cost rules from chapter 05 |
| #2952 | Sharpen test economics | Use named defect shapes, independent observations, mutation rejection, and measured whole-suite cost; never shrink the obligation denominator |
| #2953 | Retain foreign-boundary reasoning | F07/F08 provide concrete ownership, schema, model, provider, and numerical witnesses; foreign implementation assumptions remain explicit |
| #2954 | Replace abstract frontier framing with H1–H6 | Each experiment has a same-job baseline, declared observations, a falsifier, and an honest replication status |
| #2955 | Retain self-hosting as an evidence study, not a release prerequisite | Compiler-shaped workloads may reveal weaknesses; successful self-compilation is not correctness or a mandate to port now |
| #2898; #2507 | Reuse declaration/engine coverage authority | F11's remaining special-table routes must be classified and eliminated under the existing cutover, not a new registry project |
| #2517 | Reuse persisted module-summary/store work | F09/F12 and H6 must use correct existing identities and distinct artifact kinds, not another cache |
| #2919 | Retain the current construction/validation campaign | Nothing in this report converts unrun battery paths into passing implementation evidence |

The six decision records published with the previous delivery slate are ratified A in the live board. The other cards are implementation work or studies, not unratified ballots. No remaining unratified ballot in that slate needs withdrawal. The studies are retained with the sharper contracts below; their open acceptance obligations remain open.

## Mixed-language adoption remains a first-class path

**A foreign call is a promise across a boundary, not a proof of the foreign code.** D-FFI-UNIFY1, D-FFI-CAP1, and D-FFI-CPP1 remain the authority. The [existing interop chapter](../jet-foundations-and-trust-2026-09-04/06-interop.md), [mixed-repository guide](../../spec/reference/mixed-repo.md), and [migration tier map](../../spec/reference/migration-tier-map.md) provide the detailed current contract. F07/F08 must reuse it.

| Job | Concrete path | What the explanation and evidence must retain |
|---|---|---|
| Add Jet to a C repository | Bind the existing header; import it from Jet; build and call the actual archive | Source/archive identity, declared ABI, ownership, errors, native dependency and unsupported cases |
| Add Jet to a C++ repository | Use `jet inspect bind cpp` with explicit target and selected clang/archive tools; retain the generated checked shim | Layout/exception/ownership assumptions, callback constraints, exact source identity and target-specific limits |
| Replace a foreign module gradually | Keep the original source authoritative until a proposed replacement passes independent differential fixtures and review | Which behaviors were compared, inputs, unsupported remainder and observations—not a blanket “converted” label |
| Explain or debug across the boundary | Show the Jet source call and its foreign contract; attach actual foreign observations when available | Unknown, stripped, optimized-away and unavailable detail stays labeled; names are not evidence of a native location |
| Consume columnar buffers or model execution | Admit the actual provider under the same package/foreign contract | Owner/release lifetime, copies, layout, device completion, failure and target availability |

The current C/C++ adoption classification is **binder-only and overlay-first**, not automatic source conversion. The report does not invent `jet cc`, a universal transpiler, or a new FFI annotation system. #2953 owns the unified reasoning account. The original guide's command and test claims are source evidence reused here; this audit did not rerun that foreign matrix against the blocked fresh compiler.

## Whole-Jet teaching and friction have an explicit denominator

The retained [explain/predict/modify/derive census](../jet-foundations-and-trust-2026-09-04/03-comprehension.md#explain-predict-modify-derive-census) covers 24 first-party families: installation through release records, including the language, Core, eight application domains, commands, LSP, live tools, build, and FFI. The [323-identity ledger](../jet-foundations-and-trust-2026-09-04/capability-ledger.json) preserves its collected source inventory. Neither count means complete runtime or human-learning qualification.

This replacement adds concrete programs, domain walkthroughs, and an independent beginner read of all twelve full ballots. It does **not** claim that the beginner reviewer used the actual whole Jet product or that this is a representative human study. #2926 still owns whole-product teaching coverage; the adopted learning/view cards own the runtime interaction.

| Friction shape | Repetition evidence | Recovery consequence | Disposition |
|---|---|---|---|
| Fresh candidate or first-hour path fails | Existing exact failures; ordinary tasks necessarily cross this path | User cannot begin or trust the candidate | Keep #2919 and #2920–#2923; do not recard or rerun the reported construction failure |
| One contract is manually restated at a boundary | Source comparisons in E01/E02/E05/E14 | A field or rule changes in one copy only | #2974 census; F01/F02/F07/F11 |
| A stale owner/result appears current | Lifecycle source comparison; runtime neighbors remain predictions | Incorrect result, resource reuse or contradictory tooling evidence | #2975 census; F03/F05/F06/F12 |
| Setup requires hidden preparation | Explicit environment law and source workflow | A useful source program cannot reach its first run | F09 delegates to Jetpack through the ratified run boundary; no silent workaround |
| Recovery or teaching contradicts current law | E18's concrete implicit-error documentation conflict | Reader learns and repeats the wrong rule | #2976; wider teaching inventory under #2926 |
| Foreign capability or release wording overstates its evidence | Existing source-classification and release-state findings | User chooses an unavailable deployment or trusts an unqualified candidate | #2927; #2953 |

This is a task-and-consequence ordering, **not measured usage frequency**. The [retained friction ledger](../jet-foundations-and-trust-2026-09-04/03-comprehension.md#q18) separates beginner, expert, and enterprise impact. A frequency claim would need a declared task corpus or user study; this audit has neither. No invented popularity score chooses the design.

## The release plan preserves the owner's compatibility boundary

| Stage | Applicable rule | What this audit does not authorize |
|---|---|---|
| Before 1.0 | Make justified breaking changes, migrate the current corpus cleanly, preserve decision history | Current-version migration infrastructure, compatibility aliases or claims that a version string proves readiness |
| At 1.0 | Publish the exact externally supported contracts and qualified support matrix | A narrower denominator disguised as stable release |
| After 1.0 | Preserve those external contracts; take a required policy change to the owner | Silent amendment of ratified release law or an invented future compatibility duration |

The [retained release-policy disposition](../jet-foundations-and-trust-2026-09-04/07-readiness.md#compatibility-before-and-after-10) records this interview requirement and #2927's current-state correction. R0–R5 in chapter 05 is the qualification path. Foreign source adoption is a separate problem from migrating between Jet versions.

## What the independent readers changed

| Review | Concrete finding | Revision |
|---|---|---|
| Fresh true-beginner read of every full ballot | Terms and failure outcomes were not self-teaching; provider alternatives looked identical without a boundary explanation | Worked contracts, explicit evidence labels, comparison of maintenance/provider obligations, and preserved real alternatives |
| Rival-family adversarial read | Protocol-first ownership was not fairly represented | Full third endpoint option; recommendation scoped to a Jet-owned handler |
| Rival-family adversarial read | Geometry's ergonomic claim contradicted generic notation in its example | A stock `ScreenPoint` name plus the precise generic meaning |
| Rival-family adversarial read | Backend pin lacked an exact release artifact; reader detail had an undefined label | Exact ONNX Runtime release/tag citation; named D-COLUMNAR-BOUNDARY1 |
| Rival-family adversarial read | Aggregate example did not demonstrate exact integer retention | Worked sum beyond the Float exact-integer boundary |
| Rival-family adversarial read | Independent-history oracle was only a caution, not an exit criterion | Reject production-dependent models as qualifying independent evidence |
| Separate report challenge | Data cutover omitted named ratified surfaces; environment authority was misidentified | Explicit data-surface amendment scope; corrected D-VERDICT-2189-1 |
| Separate report challenge | FFI, first-party teaching coverage, frequency limits and release interview requirements were hard to trace | Direct retained-contract sections above and the machine-readable interview map |
| Separate report challenge | Resource completion and LSP presentation lacked a concrete interface | Synchronous/asynchronous ownership signatures, ordinary LSP request/response, keyboard path and text output in chapter 03 |

The [beginner review](beginner-review.json), [rival review](adversarial-review.json), and [published slate](publication.json) are retained as evidence. These are review-and-revision receipts, not a claim that the reviewers subsequently ran unimplemented APIs.

## The original thirty questions remain traceable

This is a design/research response, not a claim that every file, platform, user, or runtime path has been empirically tested. The original [coverage record](../jet-foundations-and-trust-2026-09-04/coverage.json) and [capability ledger](../jet-foundations-and-trust-2026-09-04/capability-ledger.json) remain provenance. The latter contains 323 mapped inventory identities, not a verified complete functionality denominator.

| Original question | Direct answer in this report | Evidence boundary or remaining implementation obligation |
|---|---|---|
| 1 · Turing Award lessons | Chapter 01's Dijkstra/Iverson/Codd/Lamport treatment; chapter 05's linked primary sources | Borrow precise abstractions, executable notation and explicit observations; do not claim their awards validate a Jet proposal |
| 2 · Language postmortems | Chapter 01's Go engineering and language-regret comparisons; retained foundational research | Dependency/control lessons become F02/F05/F09/F11; this is source research, not a popularity survey |
| 3 · Cherry-picks and necessary surroundings | Every feature's complete contract and the table above | Include ownership, failures, lifecycle, target behavior and expert refusal; syntax alone is not the feature |
| 4 · Silhouettes and gaps | E01–E18 in chapter 01; eight actual workload paths in chapter 04 | Source observations and predicted neighbors are labeled separately |
| 5 · Missing first-principles shapes | Typed changing queries, space transforms, execution worlds, resource schedules, model packages | These are proposals with owner gates, not newly discovered universal primitives |
| 6 · Research that could advance Jet | DBSP, Differential Dataflow, Adapton, effect work, Arrow, Euclid, model testing and proof sources | Linked sources define specific borrowed mechanisms and limitations |
| 7 · A possible research breakthrough | Chapter 05's maintained-query theorem and H1–H6 | One bounded model ran; novelty, unbounded proof, implementation binding and independent replication are not claimed |
| 8 · Fundamentals and advanced capability together | One query and typed boundary across lists, files, changing data and model applications | No measured peer win is claimed; final matched-cell gates remain strict |
| 9 · Aggressive non-cyber red team | Failure tables in chapters 02–05 and the hostile-history proposal | Stale publication, hidden errors, Float reassociation, copies and resource completion are named rejecting cases |
| 10 · Explicit assumptions | Chapter 01's rejected proposals; each feature's premises and alternatives | Current code is not authority; adopted decisions survive attractive redesigns |
| 11 · Implicit assumptions | Clock versus progress, type versus runtime identity, source presence versus capability, allocation versus sharing | Each is tied to a concrete counterexample or future falsifier |
| 12 · RLI5 across Jet | Common vocabulary, same-workload examples, eight-domain walkthroughs, #2926 and the fresh ballot reader | A model reader is not a human usability study; whole-corpus teaching completion is not claimed |
| 13 · Reasoning at every level and across FFI | F07/F08/F11/F12, the mixed-language section above, retained interop chapter and #2953 | D-FFI-UNIFY1/D-FFI-CAP1/D-FFI-CPP1 remain law; static guarantees, observed events and foreign assumptions remain distinct |
| 14 · Development experience beyond a language | F09/F12: ordinary run, source-first workbench, relationships and exact states | The editor remains ordinary source; no mandatory visual language or AI service |
| 15 · Creative teachability | Predict before reveal, edit one cause, observe the effect, transfer to a new context | Real learning must be measured; attractive prose or a completed tutorial is insufficient |
| 16 · Teach programming principles | Timers, ownership, query changes, coordinate spaces and independent reference models | Explain why an operation is valid, not merely which Jet spelling passes |
| 17 · Visuals and LSP relationships | F12's source/effect/dependency/history views and the HTML integer illustration | The HTML interaction runs JavaScript; it is not a Jet workbench or compiler demonstration |
| 18 · Friction and rough edges | Lost aggregate types, repeated boundaries, real reader gaps, setup ritual and contradictory error teaching | E18 has a concrete documentation owner; inferred neighbors get census treatment |
| 19 · Full functionality and intended behavior | Chapter 05's whole-candidate qualification and complete vertical criteria | The failed fresh construction remains blocking evidence; no reduced mode or mock adapter closes a card |
| 20 · Verifiably correct behavior and trust | Adopted compiler preservation scope plus exact application/boundary claims | No guarantee about unwritten intent, untrusted foreign code or physical hardware follows from source alone |
| 21 · Aggressive compiler optimization research | Chapter 05's legal-rewrite table, candidate theorem, Float counterexample and cost separation | Invalid fast code is ineligible; bounded agreement is not proof |
| 22 · Efficient, effective tests | Mutation/obligation ledger, independent observations, selected focused evidence and #2952 | Remove redundant cost only while retaining every required obligation and distinct defect detector |
| 23 · How other languages establish correctness | CompCert, Alive2, Go synctest, Hypothesis and Rust testing comparisons | Their evidence classes differ; a test harness is not a preservation theorem |
| 24 · Test harness design | Typed histories, deterministic worlds, conformance, mutation controls and existing composed runners | No second scheduler, comparison schema or ceremonial test for plumbing |
| 25 · CI/CD | R0–R5, exact candidate identity, fail-closed proof and target qualification | CI labels cannot widen measured evidence or make a failing build green |
| 26 · Prevent recurring software bugs | Typed boundaries, stale-owner refusal, complete error handling, controlled cancellation and valid transformations | Bugs prevented by a checked rule are distinguished from bugs merely made easier to reproduce |
| 27 · Game glitches Jet should prevent | Chapter 04's coordinate, stale entity, frame, device-completion and interpolation cases | Memory safety does not establish gameplay intent, stable frame times or correct drivers |
| 28 · Trade space and pushed bounds | Every proposal's gains/losses, deletion obligation and explicit expert control | Internal migration difficulty is not used to reject the better final design |
| 29 · Creative language/code-generation solutions | Handler-derived clients, generic aggregation, incremental updates, typed spaces and derived schedules | They reuse established checking and code generation instead of inventing another language |
| 30 · A clear stable-release plan | R0–R5 in chapter 05; the pre/at/post-1.0 compatibility table above | Stable release needs complete candidate, correctness, real workloads and matched performance evidence—not a card count or new current-version migration machinery |

The machine-readable [coverage map](coverage.json) preserves each original question's wording and points to its new answer. It does not count a design answer as implemented functionality.

## Finding dispositions

The table names the home of every source observation and each new public proposal. `card` means the work has an owner and still needs its criteria. `decision` means the cited ruling is already ratified. `no-action` records a rejected inference, not a silently dropped defect.

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
|---|---|---|
| E01 · Separate public data operation/container families | card | #2963; F02 deliberately amends the public design rather than pretending the wrappers are a compiler bug. |
| E02 · Aggregate key/value type erosion | card | #2963; #2974 enumerates the same shape at other public boundaries. |
| E03 · Overlapping dependency and invalidation mechanisms | card | #2964; #2949; #2975 classifies shared rules versus intentional lifecycle differences. |
| E04 · Generation/revision rule overlap across adapters | card | #2898; #2949; #2975; source duplication is not a fresh stale-access runtime failure. |
| E05 · High-level app plus manual transport path | card | #2962; retain the already-adopted app-serving default and prove the short path. |
| E06 · Handler/schema/lifecycle description boundaries | card | #2962; #2967; #2974; #2975. |
| E07 · Game coordinates and frame completion | card | #2965; #2967; no claim that every raw-vector use is defective. |
| E08 · Named Arrow/Parquet paths without a real bridge | card | #2968; #2969; shared-buffer ABI and file decoding remain separate complete obligations. |
| E09 · Analytics title exceeds its explicit implemented scope | no-action | Keep #2035 history intact; its plan excludes Parquet and live maintenance, so neither is counted as shipped here. |
| E10 · Compute primitives without a complete model package | card | #2970; #2971; #2974. |
| E11 · Portable UI tree versus full host/lifecycle application | card | #2919; #2946; #2949; #2975; retain the current GUI/host owners rather than inventing a second UI engine. |
| E12 · Host embedded model versus physical board proof | card | #2919; #2953; physical adapter and hardware gates remain explicit. |
| E13 · Durable cache/storage accounting and identities | card | #2517; #2972; #2975; distinct artifact kinds remain valid. |
| E14 · Complete declaration versus handwritten consumer rows | card | #2898; #2507; finish the existing generated-declaration obligation. |
| E15 · Named Core must-use route versus ordinary declaration | card | #2898; #2974; classify and migrate under the complete-declaration rule, not another warning table. |
| E16 · Small review projection versus complete evidence | card | #2945; #2946; #2948. |
| E17 · Fix-the-error loop versus transferable learning | card | #2926; #2947. |
| E18 · Contradictory automatic error-conversion teaching | card | #2976; correct the reference under the existing error-conversion law. |
| F01 · Derived endpoint contract | card | #2962; owner choice D-ENDPOINT-SHAPE1. |
| F02 · Typed query and list results | card | #2963; owner choice D-QUERY-RETAIN1. |
| F03 · Checked live query maintenance | card | #2964; owner choice D-QUERY-LIVE1. |
| F04 · Space-safe geometry | card | #2965; owner choice D-SPACE-GEOMETRY1. |
| F05 · Controlled execution world | card | #2966; owner choice D-TEST-WORLD1. |
| F06 · Derived resource schedule | card | #2967; owner choice D-RESOURCE-SCHEDULE1. |
| F07 · Columnar ownership and real reader | card | #2968; #2969; separate owner choices for ABI and external decoder. |
| F08 · Model package and real provider | card | #2970; #2971; separate owner choices for contract and external runtime. |
| F09 · Delegated project preparation | card | #2972; explicit amendment choice D-RUN-PREPARE1. |
| F10 · Generated operation histories | card | #2973; owner choice D-TEST-HISTORY1. |
| F11 · One complete Core declaration | decision | D-TIER-ONEIR1; D-CORE-PRELUDE1; implementation coverage remains with existing owners. |
| F12 · Source-first workbench | decision | D-EXPLANATION-RECORD1; D-EXPLAIN-VIEW1; D-LEARN-FEEDBACK1; D-SHARED-REVISION1; D-TEST-COMPARE1. |
| H1–H6 · Research hypotheses | card | #2951; #2954; the bounded model is an initial result, not all hypothesis evidence. |
| C01 · Type-loss shape census | card | #2974; retain intentional conversions and clean rows in the denominator. |
| C02 · Lifecycle/dependency shape census | card | #2975; retain meaningful identity distinctions. |
| Interview · Mixed-language reasoning and adoption | card | #2953; retain D-FFI-UNIFY1/D-FFI-CAP1/D-FFI-CPP1 and the actual binder/conversion capability classes. |
| Interview · Whole-product teaching and friction | card | #2926; retained 24-family task census and 323-identity source ledger; no human-learning or frequency result is claimed. |
| Interview · Current state and future compatibility | card | #2927; preserve ratified release law while correcting current-state claims; no pre-1.0 migration requirement is added. |
| Rejected unification of all fact stores | no-action | D-LEDGER1 deliberately separates stores by lifetime; only independently reconstructed meaning is a defect candidate. |
| Rejected function-call rediscovery | no-action | Named function values already use ordinary calls; D-CALLVALUE1 retains the deliberate returned-call rule. |
| Rejected automatic breakthrough or release claim | no-action | No universal theorem, fresh Jet runtime success, peer win, human-learning improvement, or stable-release readiness was established by this audit. |
<!-- /audit-dispositions -->

## Generalize every finding

A census must enumerate the predicted neighbors mechanically, including clean and intentionally different cases. It must not label every similarly named field a duplicate or every conversion a bug. An actual runtime defect needs an actual matching execution once the compiler-construction gate is available.

| Findings | Defect or design-gap shape | Predicted unprobed instances | Structural fix | Census and implementation owners |
|---|---|---|---|---|
| E01–E02; F02 | Public containers and adapters discard information the program already knows | Join keys, chart/group outputs, serialization, numeric/unit aggregation, nullable foreign fields | Keep generic semantic result types and one typed operation plan; require explicit lossy conversion | #2974 → #2963; reuse #2898 for declaration coverage |
| E03–E04; F03/F12 | A dependency changes but an old owner/result still acts as current | Reactive observers, computed fields, network queries, editor checks, caches, Pool/web adapters | One authoritative rule per lifecycle; owner/revision publication checks; distinct identities stay distinct | #2975 → #2949/#2964; #2898 for engine readers |
| E05–E06; F01 | One boundary is independently described by several producers | Request decoding, schema, clients, forms, URL parameters, error/status mapping | Derive mechanical projections from the chosen typed owner; retain explicit transport and authority policy | #2974 → #2962 |
| E06–E07; F05/F06 | Dependency and completion facts are repeated or inferred from the wrong event | CPU tasks, render passes, GPU submissions, DMA, callbacks, cancellation cleanup | Derive visible accesses; declare opaque accesses; retain ownership through actual completion | #2975 → #2966/#2967; #2953 for foreign boundaries |
| E07; F04 | Equal numeric representations hide different meanings | Screen/world/camera, GUI/device coordinates, scene instances, stale viewport transforms | Ordinary typed spaces and checked transforms, with runtime identity where static types cannot identify an instance | #2974/#2975 → #2965 |
| E08–E10; F07/F08 | A format or capability name is mistaken for a finished typed boundary | Columnar imports, file readers, model loaders, device providers, semantic index reuse | Complete real adapter, ownership, error, resource, target, and identity contracts; no unavailable-success facade | #2974; #2926/#2919 workload coverage → #2968–#2971 |
| E11–E12 | A host model or portable tree is mistaken for a complete deployed application | Native window services, mobile hosts, board HALs, interrupts, physical timing | Retain portable meaning while naming every required host operation and real-environment gate | #2926/#2919/#2953; no second GUI or hardware semantic engine |
| E13; F09 | An ordinary command depends on hidden preparation or incomplete durable identity | Toolchains, foreign compilers, selected environments, model artifacts, runtime caches | Delegate to existing package authority; use complete existing artifact identities and accounting | #2975 → #2517/#2972 |
| E14–E15; F11 | A Core function or obligation is registered separately by each consumer | Methods, signatures, effects, must-use rules, reflection, interpreter/JIT/web entry rows | One complete declared operation/obligation generates consumers; incomplete coverage rejects | #2898/#2974; adopted complete-declaration cutover under #2507 |
| E16–E17; F12 | A tool exposes a verdict but not the relationship needed to understand or change it | Diagnostics, semantic review, learning, inspector views, generated-code explanations | One fact/evidence query route; source-linked reasons; predict/change/transfer interaction | #2926 → #2945–#2948 |
| E18 | Two documentation entrypoints teach contradictory language law | Error propagation, ownership defaults, retired surface summaries, foreign conversion guidance | Correct the lower-authority teaching surface and census the same retired claim; no new semantic mechanism | #2976; #2926 for wider teaching coverage |
| H1–H6; compiler assurance | Evidence is promoted beyond what it checked | Bounded model to theorem, source to runtime, replay to universal correctness, host rig to hardware, one metric to peer win | Bind claim, inputs, implementation, observation, assumptions, and exact evidence class; fail closed on missing proof | #2925/#2927; #2934–#2944; #2951–#2954 |

The design succeeds only if these structural fixes remove real repeated work while preserving useful distinctions. If a proposal merely moves that work into a hidden adapter, forces a second description, or weakens an observable contract, reject it rather than polishing its name.
