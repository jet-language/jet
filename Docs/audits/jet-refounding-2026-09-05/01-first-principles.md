# Keep the meaning; generate the repetitive work

[Executive report](index.md) · [Language and Core](02-language-and-core.md) · [Tools and systems](03-tools-and-systems.md)

## Start with the work, not the current feature names

A shop wants a live sales total. Its programmer needs to describe a sale, reject bad input, calculate the total, update it when a sale changes, show it in a page, and test corrections. These are not six unrelated problems. They share the same fields, calculation, and rules about which version of the data is current.

Yet an implementation can make the programmer write six partly overlapping descriptions: a database schema, a server record, a request validator, a query, an update handler, and a client record. Each description can be individually sensible. The defect is that the relationships between them are maintained by hand.

**The first-principles opportunity is to make those relationships executable.** Keep separate descriptions when they express genuinely different meanings. Generate the mechanically related ones from information the programmer already supplied.

### Five terms, with an example rather than a slogan

| Term | Plain meaning | Sales example |
|---|---|---|
| **Type** | A rule describing what kind of value something is | A sale has an identifier, a region, and an amount. An amount is not an arbitrary string. |
| **Dependency** | Something an answer depends on | The regional total depends on the region and amount of each included sale. |
| **State** | What is true about a thing now | A subscription is active; a sale has revision 18; a file is open. |
| **Effect** | An interaction beyond calculating a value from the given inputs | Read a file, ask the clock, send a response, or change shared state. |
| **Adapter** | Code that changes how a value is carried, not what it means | Turn a checked request into the function's arguments; turn the result into JSON. |

An adapter cannot legitimately invent a new error rule, silently round an amount, grant a permission, or decide that an old result is current. Those are semantic choices. Jet's existing invariants already put them in the checked program and shared Core implementation.

## The underlying idea: a calculation should remain useful after its first execution

A calculation is usually treated as a way to get an answer once. Jet can do more with the same checked calculation:

1. **Run it:** produce its result.
2. **Explain it:** show which inputs and rules produced that result.
3. **Update it:** when an input changes, recompute only what can safely be recomputed incrementally.
4. **Move it:** carry its typed inputs and outputs across a declared application or foreign boundary.
5. **Test it:** generate inputs or histories that challenge its stated rules.

These are not interchangeable operations. The proposal is that they consume one description instead of independently rediscovering it. An HTTP adapter does not become a query engine. A query engine does not become a proof assistant. They share the checked facts they actually need.

### Why this is more than the previous “behavior contract” conclusion

“Preserve the contract” is a constraint. It does not tell a programmer what to write. This report turns that constraint into operations with visible results:

| Previous abstraction | Concrete product consequence |
|---|---|
| Preserve shape | A typed endpoint generates its ordinary request binding and client contract; an aggregate retains its key and amount types. |
| Preserve dependencies | A watched query updates a result after a row edit without a separately maintained update function. |
| Preserve ownership and identity | An Arrow buffer has one checked release lifetime; a late callback cannot publish through an obsolete revision. |
| Preserve effects | A controlled test world supplies time and declared inputs through the same boundaries as ordinary execution. |
| Preserve operation meaning | A graphics schedule derives resource hazards from the checked operations rather than asking for a second manual read/write graph. |
| Preserve source relationships | A workbench points from a changed value to the exact calculation, source revision, and relevant operation. |

The benefit must be visible in code, not merely in a better internal report format.

## Simplicity does not mean deleting useful distinctions

The wrong refounding would put everything in one `Graph`, `Context`, or `Value` type and announce that Jet now has one mechanism. That would hide important differences and force users to recover them manually.

The useful distinction is between **one meaning with several representations** and **different meanings with similar-looking implementations**.

| Things that look similar | Share | Do not collapse |
|---|---|---|
| A named function, a closure, and a callback | Callable types, argument contracts, effect checking | Their capture ownership and lifetime |
| A list, a file reader, and a watched query | Typed transformations where their laws agree | Eager evaluation, one-shot consumption, errors, retained state, and update lifetime |
| A plain value, a read view, and a shared handle | The represented value's type | Who owns it and who may change it |
| A UI signal and a database live query | Dependency publication and stale-result checks | Database transactions, network subscriptions, and local cell synchronization |
| A build graph and a frame schedule | A reusable graph data structure if useful | Build action trust, runtime effects, device barriers, and scheduling policy |
| A declaration, a checked fact, and a recorded observation | The typed query interface ratified in D-LEDGER1 | Static schema lifetime, program revision lifetime, and execution lifetime |

**A count of structs, tables, or caches is not a proof of over-engineering.** The deletion test asks whether two places can independently decide the same meaning. If they cannot, merging them may add coupling rather than remove it.

## What the Jet evidence actually shows

The following observations are from source, specifications, and the live decision store. They are not fresh runtime findings. “Burden” is a design inference from the cited interface unless a retained execution is explicitly named.

| ID | Source-observed fact | Concrete consequence | First-principles response |
|---|---|---|---|
| E01 | `core.data` exposes `Table<T>`, `LazyFrame<T>`, `DataStream<T>`, eager functions, and lazy-specific functions. [Reference](../../spec/reference/core-library.md), lines 2582–2655. | A user learns different operation entry points while moving between the same row transformations. | One typed query vocabulary and normalized plan, while preserving eager-list and one-shot-stream laws. |
| E02 | `DataGroup` fixes its key to `String` and its sum/mean to `Float`. [Reference](../../spec/reference/core-library.md), lines 2618–2620 and 2652–2653. | The API cannot retain a nominal key or amount type merely because the input had one. | Generic aggregate results that retain meaningful input types and derive only the type changes the operation requires. |
| E03 | Computed fields have dependency invalidation; reactive values have an observer graph; live queries have footprints, generations, dirty flags, and publication logic. [Computed fields](../../../examples/features/memory/computed_field.jet); `ReactiveEventWatch.rs`; `Top/LiveQuery.rs`. | New update features can grow their own invalidation and freshness rules. | Share authoritative dependency/publication rules; add query incrementalization rather than another hand-maintained update path. |
| E04 | Native Pool IDs and Web Pool IDs implement index/generation checks separately; live queries have a separate generation lifecycle. `MathTaskMem.rs:1683–1929`, `Core/Host.js:1–123`, `Top/LiveQuery.rs:601–772`. | A maintainer must establish that stale access and stale publication mean the same thing where the contracts overlap. | Audit the overlapping rule, not all identities indiscriminately. Reuse the adopted shared-revision work and I9 adapters. |
| E05 | The web battery declares an application graph, then explicitly constructs low-level HTTP machinery and request decoding. [Web battery](../../../examples/features/web/battery/run.jet), lines 24–37 and 63–153. | The example does not demonstrate the shortest high-level application path by itself. | Finish the already-ratified app-serving default; deepen typed endpoint binding and generated clients without inventing a second server. |
| E06 | The backend battery separately declares routes, checks generated OpenAPI strings, and composes listener/task/deadline/shutdown behavior. [Backend battery](../../../examples/features/net/backend_battery/run.jet), lines 71–147. | The relationship between a handler and its outside contract remains a place to examine for repeated description. | A checked endpoint descriptor must derive from the handler and transport mapping, not a parallel schema language. |
| E07 | The game battery combines a scene, low-level rendering, AABB calculations, fixed-step timing, and interpolation. [Game battery](../../../examples/features/game/battery/run.jet), lines 35–115. | Memory safety alone cannot catch a screen/world coordinate mix or a wrong frame schedule. | Space-safe geometry plus resource schedules built on existing ownership and effects. |
| E08 | Arrow and Parquet format names exist, but the inspected DataFlow paths return a bridge error for them. `Top/DataFlow.rs:1935–1939,2320–2324`. | Naming a format is not a native reader or a safe shared-buffer boundary. | Give actual columnar interchange a complete lifetime contract; treat native format decoding as a separate, explicit implementation and dependency choice. |
| E09 | Card #2035 is titled CSV/Parquet analytics, but its plan explicitly excludes Parquet and live re-query. Its example uses checked `SQL{...}` and `csv.query<T>`. [Example](../../../examples/features/serde/analytics_query.jet). | A title or broad aspiration is not evidence that Parquet or live query maintenance shipped. | Reuse the existing SQL engine and D-SQL-SURFACE1. Do not propose SQL as new, or treat the excluded pieces as delivered. |
| E10 | The ML battery manually batches training, scans embeddings, streams text, handles tool messages, and separately obtains device evidence. [ML battery](../../../examples/features/math/ml_battery/run.jet), lines 15–67. | Tensor operations do not by themselves provide an application-ready model package. | Typed model artifacts and bounded sessions, reusing compute and the existing package manager. |
| E11 | The GUI battery manually combines reactive state, focus, shortcuts, history, host capability checks, and platform results. [GUI battery](../../../examples/features/ui/battery/run.jet), lines 14–257. | A portable node tree is only part of a portable application. | Complete lifecycle and host operations; expose the same state and dependency relationships in the workbench. |
| E12 | The embedded battery exercises a shared register/DMA model and prints physical-target gates; the reference explicitly lacks a complete board HAL. [Battery](../../../examples/embedded/battery/run.jet); [reference](../../spec/reference/embedded.md), lines 200–210. | A host simulation cannot establish electrical, interrupt-latency, or physical deployment behavior. | Separate checked resource logic from device adapters and actual hardware evidence. |
| E13 | `RunCache` has its own root and key and explicitly excludes AOT `jet-store` artifacts; the architecture requires one machine-wide compiler/runtime store. [RunCache](../../../Source/RunCache.rs), lines 1–6 and 77–87; [architecture](../../spec/architecture.md), lines 122–138. | Store accounting can omit a durable cache path. Different cache lifetimes alone are not the defect. | Integrate accounting and durable storage under existing authority; retain distinct artifact kinds and correct lookup conditions. |
| E14 | Core checking has explicit type/member/signature tables; the MIR ruling requires one complete Core declaration and generated rows. `CheckerCoreLib/core_types.rs`, `module_items.rs`, `fixed_sigs.rs`; D-TIER-ONEIR1. | Adding a Core operation risks another per-consumer registration obligation. | Complete the existing generated-declaration cutover. No new “Core registry” project is justified. |
| E15 | `core_must_use_type` names `ScopeGuard`, `Iter`, and `Delivery`, while general checking also reads `#MustUse` declarations. `core_types.rs:8–21`; `CheckerOwnership.rs:4053–4170`. | The same ignored-value obligation has a Core-name route and an ordinary marker route. | Make the ordinary declared obligation authoritative; prove equivalent observed warnings before deleting the special route. |
| E16 | `EvidenceRecord` retains source, build, revision, completeness, diagnostics, attachments, and chains. `CmdReview` projects a much smaller state for comparison. `Evidence.rs:536–553`; `CmdReview.rs:246–397`. | A review can require opening the original record to understand what a changed claim actually establishes. | Reuse the now-ratified explanation record and semantic-review owners. Do not invent a replacement evidence format. |
| E17 | The current learn command declares three static exercises and a completion/problem/wrong-output loop. [CmdLearn](../../../Source/CmdLearn.rs), lines 1–50 and 214–367. | Fixing an error is not the same task as predicting, explaining, or transferring a concept. | Complete the adopted prediction/counterexample/change/transfer design, with real program states. |
| E18 | The language contract names one error-conversion rail; the Core reference still says “no automatic conversion.” [Language](../../spec/spec.md), lines 1097–1171; [Core reference](../../spec/reference/core-library.md), lines 176–194. | Two documentation routes teach different answers to a basic failure-handling question. | Correct the stale teaching surface under the existing decision; no new error mechanism. |

The source paths under `Prelude` in this table are rooted at `crates/jet-codegen/src/Prelude/`; the checker paths are rooted at `crates/jet-sema/src/Sema/`. Exact scope and the distinction between a fact and its predicted neighbors remain in the final finding table.

## Three attractive proposals that did not survive the authority check

### “Unify function-value calls” was based on an over-broad reading

D-CALLVALUE1=B already permits `f(x)` for a stored function value and ordinary calls for stored fields. Only calling a just-returned function requires `.call(...)`; the owner explicitly rejected the adjacent `)(` shape.

```jet
// Existing decision, not a proposed change:
dloss :: compute.grad(loss)
gradient :: dloss(weights, input, target)

// The explicit chain remains available:
gradient :: compute.grad(loss).call(weights, input, target)
```

There is no reason to sell direct calls of named function values as new. This audit does not reopen that choice.

### “Merge all fact tables” would contradict a deliberate design

D-LEDGER1=D keeps three stores by lifetime: the schema of fact kinds, the checked facts of a program, and evidence from a run. It requires one typed query contract over them. `Registry.rs`, `Facts.rs`, and `Evidence.rs` coexisting is therefore not itself a defect.

The correct audit question is: **does a consumer read the authoritative fact through that contract, or independently reconstruct it?** This distinction prevents a fashionable “single source of truth” rewrite from becoming a giant, poorly separated mutable store.

### “Add typed shapes, live serving, or SQL” would resell adopted work

D-TYPE2-MEASURE1 already unifies lengths, matrix shapes, lanes, and exponents as measures. D-WEBAPP-SERVE1 already makes the app graph a running server under `jet run` and a live-reloading server under `jet dev`. D-SQL-SURFACE1 already owns the checked SQL and typed-query relationship.

The new proposals must deepen those contracts: retain meaningful types through aggregation; maintain a query as data changes; derive a client's contract from the same endpoint; finish the path rather than add a second spelling. Existing-but-unbuilt work is an implementation obligation, not a research discovery.

## What Go actually teaches Jet

Go's small syntax is the visible result of a wider engineering choice. In [Go at Google: Language Design in the Service of Software Engineering](https://go.dev/talks/2012/splash.article), the authors name slow builds, uncontrolled dependencies, incompatible language subsets, update cost, and automation difficulty. Their dependency and export-data design attacks work around a program, not just punctuation inside it.

**Take the engineering move, not a costume.** Jet should not acquire Go's error tuples, garbage collector, or interface rules merely to look simpler. It should make ordinary projects easy to build, run, inspect, and change while retaining Jet's stronger ownership, effect, and type facts.

| Go lesson | Jet-specific decision | What would demonstrate success |
|---|---|---|
| A standard command should perform the ordinary workflow | F09 delegates exact environment realization instead of requiring an environment-only shell detour | Same checked-out project, clean user environment, one command to first useful output; locked/offline refusals remain clear |
| Dependencies should be explicit and cheap to inspect | Preserve explicit import/package contracts and use complete export/query identities | A private-body edit does not invalidate unrelated importers; a public type/default/effect change does invalidate affected consumers |
| Concurrency testing should not require redesigning every API | F05 supplies a scoped clock and scheduler at existing effect boundaries | The real timeout function runs unchanged in production and controlled-world tests |
| Fewer special cases aid everyday maintenance | F11 removes compiler-only Core registration chores | One new API declaration reaches every generated consumer, or the compiler build rejects incomplete coverage |
| Small common calls can coexist with explicit policy | Keep ratified argument labels, safe defaults, and expert options | A beginner call is short; an expert can state the timeout, memory ceiling, target, and refusal policy without a different API family |

Go 1.25's [testing/synctest](https://go.dev/blog/testing-time) is an especially useful contemporary comparison. It runs concurrent code in a scoped “bubble” with fake time and a way to wait until work is stably blocked. It avoids fake-clock parameters in production APIs. It also distinguishes passage of time from synchronization. Jet should copy that complete lesson, not merely add a clock mock.

No benchmark here establishes that Jet currently beats Go on build latency, task overhead, or test execution. The comparison defines the job and the acceptance criterion.

## Classic ideas become concrete product decisions

### Dijkstra: make the abstraction precise, not vague

[The Humble Programmer](https://www.cs.utexas.edu/~EWD/transcriptions/EWD03xx/EWD340.html) argues that the tools and notation shape what programmers can reason about. It also distinguishes testing for the presence of bugs from establishing their absence, and argues for constructing a program and its correctness argument together.

For Jet, that means a query's error order, an endpoint's validation, and a resource's lifetime belong in the design before code generation. It does **not** mean every user must write a proof to total some rows. The compiler and library carry the repetitive proof obligations where the rule is known. The programmer states the domain rule that the compiler cannot guess.

### Iverson: operations should suggest useful combinations

[Notation as a Tool of Thought](https://www.jsoftware.com/papers/tot1.htm) values executable notation, composability, suggestivity, and the ability to hide detail without losing precision. The useful transfer is not APL's character set. It is that understanding one operation should help a user predict its composition with another.

For Jet, `filter`, `join`, and `sum` should remain recognizable when the data source changes. A user who understands a batch query should be able to understand its watched form. The compiler must retain the laws that make this safe; effectful callbacks and order-sensitive floating-point reductions are not magically algebraic.

### Codd: separate the question from the storage arrangement

The [original relational-model publication abstract](https://research.ibm.com/publications/a-relational-model-of-data-for-large-shared-data-banks) argues for protecting programs from changes in internal representation. That is directly relevant to a Jet query that should not need rewriting because rows now arrive in a columnar batch or a file.

The limit matters. SQL bags, nulls, and unordered relations are not identical to ordered Jet lists and `?T`. The design must state ordering, multiplicity, absence, and validation rules. “Use relational algebra” is not permission to change the answer or error order.

### Lamport and model checking: a final value does not describe a history

The retained [concurrency lecture](https://lamport.azurewebsites.net/pubs/turing.pdf) and [model-checking lecture](https://www-verimag.imag.fr/~sifakis/TuringAwardPaper-Apr14.pdf) motivate state-machine reasoning and counterexample histories. A request that returns the correct text can still publish it after its owner was closed. A game entity can have the correct coordinates and still be the wrong generation of entity.

F10 turns that lesson into generated histories over declared state and protocol information. F05 supplies controlled time and scheduling. The workbench displays the smallest failing sequence. This is stronger than checking the last output, but bounded exploration remains bounded exploration, not a proof of all possible executions.

### Modern incremental computation: maintain an answer, not a second program

[Differential Dataflow](https://timelydataflow.github.io/differential-dataflow/introduction.html) explicitly supports changing the input of a standing computation and observing corresponding output changes. The [2025 DBSP journal paper](https://link.springer.com/article/10.1007/s00778-025-00922-y) describes incrementalizing a composed query by incrementalizing its primitive operations.

F03 adopts the useful shape: an ordinary query plus changes to its input. It does not claim DBSP proves arbitrary Jet programs incrementally equivalent. Mutable aliasing, effects, validation errors, ordered outputs, and floating-point rules are Jet obligations that the transfer must address. The later research chapter states the exact candidate theorem and the counterexamples that can defeat it.

## The language regrets become prevention rules

The [earlier source-by-source retrospective](../jet-foundations-and-trust-2026-09-04/01-foundations.md#q2) remains the historical reading ledger. This replacement changes its consequence from general advice into design constraints.

| Historical problem | Concrete rule for this design | Feature that must obey it |
|---|---|---|
| A pleasant asynchronous API hides completion and failure ownership | Every standing operation has an owner, a terminal result, and a cancellation rule | Live queries, worlds, resource schedules, model sessions |
| A lazy abstraction hides space retention | Retained state, materialization, and ceilings are inspectable; one-shot collection remains obvious | Typed and live queries |
| A foreign pointer loses bounds and lifetime | Import a checked owned/view handle with shape and release behavior; reject unknown lifetime rather than assume it | Arrow and all foreign boundaries |
| A generated schema exposes an internal representation | Public output types are selected explicitly; private fields and secret tags are not automatically exported | Endpoints and model packages |
| An environment tool becomes a second language runtime | Jetpack realizes declared dependencies; Jet executes source; no duplicate resolver or shell evaluation | One-command project execution |
| A compact language lacks the libraries needed for real work | Supply complete application paths, including shutdown, deployment, and failures | All eight workload walkthroughs |
| An attractive optimizer loses information needed for correctness | Preserve effects, ownership, failure order, numeric laws, and source identity through the shared executable path | Incrementalization, fusion, scheduling, and generated adapters |
| A formal model is mistaken for the implementation | Bind proof to the production compiler and keep an independent counterexample route | The ratified compiler-proof work |

This is not a historical consensus or a ranking of languages. Each retained source has its context and limitations. Incomplete lecture captures remain incomplete; adding Iverson's body and Codd's publication abstract does not complete a census of all Turing lectures.

## The beginner path and the expert path must meet at the same operation

A deep module hides useful complexity behind a small interface. It does not hide a different program. The expert form should expose a choice the beginner form already made, rather than switch to a separate implementation with different answers.

| Feature | Beginner | Show the choice | Make it explicit | Refuse it |
|---|---|---|---|---|
| Query | Ask for rows and collect a result | Show operations, ordering, validation, and materialization | Set memory, spill, and execution policy | Require an in-memory plan or reject unsupported pushdown |
| Live query | Watch an explicitly changing source | Show dependencies, retained indexes, revision, and update work | Set retention and update policy | Require incremental maintenance or use a one-shot query |
| Endpoint | Bind a typed handler | Show path/body mapping, public fields, errors, and rights | Write transport and authorization rules | Use a plain function and explicit low-level HTTP code |
| World | Run a timeout test with controlled time | Show providers, pending jobs, and next event | Select clock, scheduler, and declared input source | Reject uncontrolled host effects and unsupported foreign calls |
| Geometry | Use stock world/screen point types | Show source and destination spaces | Supply the transform and units | Reject a raw cross-space operation |
| Resource schedule | Submit typed operations | Show hazards, ordering, reuse, and target | Pin order, memory, or device choices under existing policy | Reject a schedule that cannot meet the declared constraint |
| Project run | Run the project | Print the exact realized environment and trust class | Use Jetpack's explicit environment command | Locked/offline/no-realization policy, without a hidden fallback |

## What would make this refounding fail?

The proposal fails if it merely moves handwritten duplication into a larger framework. It also fails if ordinary work now requires learning “plans,” “worlds,” or “facts” before doing anything useful.

Its specific rejection conditions are concrete:

- A typed query needs string field names where the field was already statically known.
- A watched result requires the user to maintain both the query and its update algorithm for an operation the compiler claims to support.
- A generated endpoint exposes a private field or guesses authorization.
- A controlled world calls the real clock or network without declaring that escape.
- A resource schedule changes observable effect order under the name of parallelism.
- A coordinate conversion removes a space distinction before checking it.
- An Arrow import calls a release callback twice, accepts an unknown lifetime, or calls a required copy “zero-copy.”
- A model package treats a matching tensor shape as proof that the model is accurate or harmless.
- One-command execution bypasses the adopted sandbox/trust rules.
- A tool shows an old fact as current or an unmeasured speedup as established.

The positive claim is narrower and stronger: **one useful description can support more correct operations, with less repeated user work, when the compiler preserves the information those operations need.** The next chapters specify those operations in enough detail to accept, amend, or reject them.
