# Make the environment controllable without plumbing it through the application

[Executive report](index.md) · [Language and Core](02-language-and-core.md) · [Domain walkthroughs](04-domain-walkthroughs.md)

Every API sample marked **Proposed** specifies intended behavior. It is not current-binary evidence. This chapter gives each system a real job, a small public boundary, internal obligations, rejected alternatives, and a way to tell whether the design worked.

## F05 · Test a timeout without waiting for a clock

### Why the usual test is unreliable

A background operation starts now and finishes later. A test that sleeps for a second does not know whether the operation has reached the state being checked. A longer sleep wastes time and still fails on a sufficiently busy machine.

A **controlled execution world** is a scope in which the test controls time, scheduling, and declared external inputs. The production function keeps its ordinary signature. Its existing effect calls go to the world's providers instead of the real clock or network.

Go's [testing/synctest](https://go.dev/blog/testing-time) supplies the important precedent: fake time and a way to wait until background activity is stable, without threading a new clock parameter through every application function. Jet can use its explicit effects and task lifetimes to make the controlled boundary more visible and reject an uncontrolled escape.

### Proposed test

`retry_invoice` is the unchanged production operation. It retries after five seconds through the ordinary time/task APIs. The assertion observes its externally meaningful attempt count, not an internal scheduler field.

```jet
// Proposed controlled test API:
testing.world(world -> {
    pending :: task.spawn(retry_invoice)
    world.wait_idle()
    assert(attempts.get() == 1)
    world.advance(5s)
    world.wait_idle()
    assert(attempts.get() == 2)
    pending.cancel()
    pending.join() ?? nil
})
```

The production function does not receive `world`. The test does, because the test needs controls. This is scoped execution, not a process-global monkey patch.

### The exact contract

| Question | Proposed rule |
|---|---|
| Which tasks belong to the world? | The callback and its structured descendants inherit the world through the existing execution context |
| What does `wait_idle()` mean? | Every controlled task is completed or blocked on a known controlled event; ordinary runnable work has drained |
| What does `advance(duration)` mean? | Advance the monotonic clock through due events up to the requested instant, draining runnable work at each event boundary |
| What happens to wall-clock dates? | A separately named fixed wall-clock origin supplies dates; advancing monotonic time does not silently change timezone or calendar rules |
| Two timers expire together? | A recorded stable tie-break order applies; an explicit exploration mode may vary legal orders |
| A task spins forever? | A step/deadline budget reports nontermination or budget exhaustion; the test does not hang indefinitely |
| A task calls the real network? | A missing controlled provider is a named failure; there is no quiet escape to the internet |
| A task needs randomness? | Use a recorded seeded source; security-grade randomness is not replaced by a weak generator in ordinary production execution |
| Can a task or borrowed world value escape? | The structured lifetime must reject the escape or finish/cancel it before scope exit |
| Can two tests run together? | Each world has its own context and state; there is no global clock shared by unrelated tests |
| Can a world be nested? | Reject an ambiguous nested scheduler by default; an explicit child scope shares the parent's clock and scheduler |

`wait_idle()` does not prove that nothing can ever happen again. It proves that nothing currently runnable remains before the next controlled input. That difference is essential for a timeout test.

### The internal design stays inside existing mechanisms

```text
ordinary application call
        |
checked effect operation + task context
        |
shared Prelude operation
        |
world-selected boundary provider
  real clock / controlled clock
  real transport / recorded transport
  real scheduler / controlled scheduler
```

The provider changes the environment input. It does not duplicate business logic, invent a new Result type, or run a second interpreter with different language rules. AOT, JIT, interpreter, and applicable web execution use the same controlled providers and observation contract.

The effect list and authority checks remain active. A testing world is not permission to call an effect that the function's boundary forbids. Foreign code that bypasses the controlled boundary is labeled uncontrolled and cannot earn a deterministic-execution claim.

### Alternatives

**A, recommended: scoped providers through existing effects.** This removes fake-clock plumbing and permits stable asynchronous assertions. The real cost is a controlled scheduler and explicit provider coverage. An external device cannot be made deterministic merely by wrapping its handle.

**B: explicit fake dependencies in every affected function.** This is a legitimate expert design when the dependency is part of the business interface. It is a poor default when dozens of functions acquire a clock parameter solely for tests.

**C: sleep, poll, or replace process globals.** Sleeping retains timing uncertainty. Global replacement breaks isolation and can affect unrelated tests. Neither supplies the required boundary.

The design-away choice is to keep explicit dependency injection available without requiring it for ordinary time/task effects. No public dependency-injection framework is added.

## F06 · Derive resource hazards from the operations that use the resources

### The job

A game frame computes positions, draws a shadow map, renders a scene, and composites the result. A GPU cannot read an image before its writer finishes. Temporary images can reuse memory only when their lifetimes do not overlap.

A **resource schedule** is an ordering of operations that respects their reads, writes, lifetimes, and explicit external effects. It is not permission to change the program's observable order or to choose an undeclared device.

D-PLACE1 and D-ACCEL1 remain protected. They already own placement and acceleration policy. F06 adds the missing derivation and explanation of resource dependencies under those policies; it does not vote on a different automatic-device policy.

### Current repetition and proposed use

The inspected [game battery](../../../examples/features/game/battery/run.jet) constructs scene/input/component state and separately performs rendering-oriented calls. The [ML battery](../../../examples/features/math/ml_battery/run.jet) combines compute with a GPU receipt. Those sources do not establish a complete checked frame-resource scheduler.

The following proposed excerpt assumes ordinary checked functions `simulate`, `draw_scene`, and `composite`, and typed resources `positions`, `scene_image`, and `screen`.

```jet
// Proposed schedule scope; reads/writes come from checked calls.
render.frame(frame -> {
    simulate(&positions)
    draw_scene(positions, &scene_image)
    composite(scene_image, &screen)
})
```

The user did not list a second set of resource names in `reads:` and `writes:` declarations. The compiler already knows that `simulate` writes positions and `draw_scene` reads them. True foreign/driver boundaries need a complete checked resource declaration because their bodies are not visible; guessed access is forbidden.

### The opaque boundary must distinguish submission from completion

These are **proposed public API signatures**, not existing callable Jet APIs:

```jet
// Synchronous: all access finishes before the call returns.
fn draw_scene(positions: [Position], target: &Image) !RenderError

// Asynchronous: transfer the command bundle and its resource owners.
fn submit(frame: ^FrameCommands) FrameFlight !RenderError
fn finish(flight: ^FrameFlight) FrameResult !RenderError
```

The synchronous declaration means read access to `positions`, exclusive write access to `target`, and completion before return. It uses the existing call contract, not a new resource attribute. A driver that returns after submission cannot truthfully use that contract without waiting.

The asynchronous route transfers ownership. `FrameFlight` retains the submitted resource owners until the actual device completion event. `finish` returns the completed resources through `FrameResult`. Submission failure discharges every consumed owner through the normal failure/cleanup rules. Cancellation is not evidence of device completion: the owner remains retained until the device has stopped accessing it.

| Boundary fact | Required source of truth | Missing or false fact |
|---|---|---|
| Read/write access and possible aliasing | Checked signature/body; audited declaration for an opaque primitive | Serialize conservatively or reject an explicit parallel demand |
| Which resources outlive submission | Ordinary moved owners in `FrameFlight` | Reject an escaping call-local borrow |
| Completion, device error, and cancellation acknowledgement | Actual driver event, tied to this submitted operation | Do not reuse or release the resource |
| Whether the foreign implementation obeys its declaration | Existing audited FFI/provider boundary and provider conformance evidence | Name the foreign assumption; do not call it a compiler proof |

This design does not reinterpret `&` as an asynchronous loan. D-FFI-CAP1 remains exclusive-for-the-call. The frame implementation uses the existing task/completion machinery rather than adding another scheduler. Visible calls retain source semantics; scheduling is legal only where their checked contracts justify the transformation.

### What is derivable

| Fact | Legal consequence |
|---|---|
| Writer A produces a resource read by B | A must complete before B reads it |
| Two operations read the same immutable resource | Their reads do not conflict by themselves |
| Two writes may overlap | Order them or reject unsupported concurrent access |
| Resource regions are proven disjoint | Region-level concurrency may be legal; otherwise remain conservative |
| A temporary's last use precedes another's first use | Storage reuse may be legal if size, alignment, layout, and device constraints match |
| An operation has a visible external effect | Preserve its required ordering; do not treat it as a pure GPU node |
| A result is consumed asynchronously | Keep the owner alive until actual completion, not merely command submission |

The safest reference schedule is source order with no speculative reuse. Optimizations must preserve its defined observations. For unknown aliasing, the default is conservative ordering. An explicit demand for parallel execution may fail with the unresolved dependency, rather than manufacture a proof.

### The output should answer a concrete question

```text
Why did these operations run in order?
  simulate writes positions
  draw_scene reads positions
  dependency: simulate -> draw_scene

Why was this image buffer not reused?
  composite still reads scene_image
  its completion event has not occurred
```

The source-first workbench can show this directly at the calls. The data comes from the adopted explanation record, not a second graph built by the renderer for the editor.

### What this prevents in a game

It can prevent a read-before-write resource hazard, an early temporary-buffer reuse, or a draw that outlives its texture owner. It can make a hidden CPU/GPU transfer visible and let the user refuse it. It does not automatically prevent tunneling through a thin wall, poor camera design, or a renderer's incorrect lighting equation.

A physical-driver failure still requires driver/device evidence. A headless schedule model can test ordering and lifetime decisions, not prove every GPU obeys its driver contract.

### Alternatives

**A, recommended: derive the schedule from checked accesses and explicit primitive boundaries.** Keep source order as the reference and expose expert constraints through the existing placement/acceleration system. This removes duplicate dependency declarations.

**B: a manually declared render graph.** It can express everything, but the user must keep read/write declarations aligned with the called functions. A manual graph remains useful only where those declarations are the genuine external boundary, not a second description of visible Jet code.

**C: immediate submission only.** This has the smallest scheduler but cannot exploit whole-frame lifetime reuse and makes dependency management an application burden.

The unavoidable trade is retained schedule information and conservative decisions when facts are incomplete. Hiding unknown accesses or changing a protected acceleration default is not an acceptable way to improve the result.

## F08 · A model is a typed package, not a bag of unrelated files

### The job

Load an embedding model, turn text into vectors, and use those vectors in an application. The tokenizer, weights, tensor dimensions, numerical format, and model version must agree. A vector produced by a different model should not silently enter an incompatible index.

A **tensor** is an array with a shape, such as 32 rows of 768 numbers. A **tokenizer** converts text into the integer symbols expected by a model. A **model package** binds the graph, weights, tokenizer when needed, input/output types, and execution requirements into one checked dependency.

This is for people deliberately building AI/ML applications. It adds no AI requirement to Jet's compiler, editor, learning tools, package manager, or development workflow.

### The current battery is not an application model interface

The [ML battery](../../../examples/features/math/ml_battery/run.jet) demonstrates batching, collation, training-related compute, an exact embedding scan, top-k/cosine operations, streaming text/tool-loop mechanics, and model round-trip work. That is useful source coverage. It is not evidence of a production model artifact contract, complete tokenizer/weight pairing, or a general inference backend.

[ONNX Runtime's API](https://onnxruntime.ai/docs/api/python/api_summary.html) illustrates the real underlying job: load a graph, supply named typed tensors, choose execution providers, and manage device inputs/outputs. Its documentation also exposes a cost Jet should not hide: inputs and outputs default to CPU, and explicit I/O binding is needed to avoid unnecessary device copies.

### Proposed ordinary use

`search_encoder` is a pinned model dependency. Its package declares a text input and an embedding output with a checked dimension and model identity.

```jet
// Proposed package-backed model use:
encoder :: models.open(search_encoder) ?? panic("model unavailable")
query :: encoder.embed("quiet laptop") ?? panic("cannot encode query")
hits :: index.nearest(query, count: 5) ?? panic("incompatible index")
loop hit in hits -> print(hit.title)
```

The application does not write a dictionary of tensor names or separately load a tokenizer that happens to have a similar filename. The expert tensor-call interface remains available on the same package for models that do not expose text operations.

### The package contract

| Part | What is fixed or checked |
|---|---|
| Identity | Content identities for graph, weights, tokenizer, adapter, and declared preprocessing |
| Interface | Named input/output types, tensor element formats, shape relations, and bounds for dynamic dimensions |
| Semantic relation | Which tokenizer and preprocessing belong to which model; no inference from filenames |
| Execution | Supported operators and provider requirements under the existing target/acceleration policy |
| State | Ownership of session state and caches; reset, cancellation, and destruction rules |
| Limits | Maximum context, batch, generated output, retained cache, and temporary allocation |
| Errors | Invalid shape, incompatible artifact, missing provider, exhausted limit, cancellation, and backend failure remain distinct |
| Trust | Custom operators and executable extensions require declared authority; a model file is not assumed harmless because it contains weights |
| Reproducibility | Record relevant model, provider, numerical-mode, seed, and input identities without recording secrets by default |

A dynamic model load cannot magically produce a compile-time constant shape. It returns a checked runtime witness or fails the requested interface. A model with an incompatible dynamic axis is not cast into the expected type.

### Streaming is a resource lifetime

A generation stream owns its active session/cache relationship until completion or cancellation. Stopping consumption must stop or account for the producer's retained work. Already emitted text and tool effects are not rolled back. A cancellation cannot promise that a remote request or a foreign kernel never ran.

The ordinary stream uses Jet's existing bounded buffering and task cancellation rules. It does not invent an unbounded token queue, an independent cancellation flag, or a separate “AI async” mechanism. Tools invoked by a model remain ordinary capability-checked application operations; generated text does not become executable authority.

### Search correctness remains explicit

An embedding index records the encoder identity, dimension, metric, and any normalization contract. A cosine query and an unnormalized dot-product query are not silently interchangeable. Exact search is the correctness baseline. Approximate search is an explicit algorithm with measured recall and memory/latency tradeoffs; it must not replace exact search because a table grew large.

### Backend choice is separate from the public type

The proposed public contract is engine-independent, but an implementation must contain a real supported backend. The preferred first concrete backend is **ONNX Runtime through the existing foreign/package boundary**, with its package and provider closure explicitly owner-approved and pinned. It is not a new external dependency of a compiler seam crate.

This audit does not claim that all ONNX operators, providers, or model families work everywhere. The implementation card must name and execute the supported model/operator/target matrix. Required native and web behavior cannot be parked as “later.” The model package must reject an unsupported operator or target before presenting a usable session.

Alternatives are a separate public API for every engine, or a new Jet-owned general inference engine. The former duplicates tensor/lifetime policy. The latter greatly expands maintenance and proof obligations before establishing a same-job advantage. Reuse an existing engine behind one typed boundary first; a future Jet kernel replaces it only after equivalent behavior and better measured whole-job cost are established.

### What disappears

The typed package replaces separate asset pins, tensor-name dictionaries, unchecked tokenizer selection, index/model shape glue, and private cancellation rules. It does not replace model evaluation, data quality work, domain safety review, or the need to test the application's actual task.

## F09 · `jet run` should realize the declared environment through Jetpack

### The current rule was explicit; D-RUN-PREPARE1 ratifies the amendment

The current spec says an unrealized library produces E0983; E1355 also preserves the no-on-demand realization boundary. D-VERDICT-2188-1 separates responsibilities: Jet acts on source; Jetpack acquires, pins, realizes, and exposes dependencies. D-VERDICT-2189-1 defines the corresponding environment/preparation surface. Ratified D-RUN-PREPARE1 narrowly amends the ordinary run behavior; it does not merge the tools.

The current manual workflow is therefore understandable:

```sh
jetpack env --prep
jetpack env -- jet run app.jet
```

The ratified behavior is not to merge the binaries or create `jetpack run` again. Jet delegates environment realization to the existing Jetpack owner when running a project, then executes source under the returned environment.

### Ratified ordinary workflow

```sh
jet run app.jet
```

On a trusted project with an exact lock, Jet asks Jetpack for the declared closure, obtains its checked environment, and runs the program. A warm invocation reuses the same closure. The shell's unrelated PATH must not decide which declared tool is used.

A **closure** is the complete set of dependencies needed for the selected job, including dependencies of dependencies. **Realizing** it means obtaining and preparing those exact inputs, not choosing new versions.

### The safe default is not silent execution of a new checkout

| Situation | Result |
|---|---|
| Trusted project, exact lock, all inputs present | Run without another shell step or redundant prompt |
| Trusted project, exact lock, missing downloadable inputs | Show the required preparation and delegate acquisition under the declared network/trust policy |
| Untrusted project | Ask for the existing trust decision before any project-controlled build action; noninteractive denial remains denial |
| Missing or stale lock under `--locked` | Fail before acquisition or execution; do not repair it silently |
| `--offline` and a missing object | Name the exact missing object and stop without network access |
| Explicit `--env ci` with no such environment | Reject the name; do not fall back to a different environment |
| Pure one-file program with no environment needs | Run directly; do not make Jetpack work a mandatory tax |
| Already inside the matching realized environment | Reuse its verified identity; do not recursively re-enter |
| User refuses automatic preparation | `--no-prepare` requires the declared environment already be usable |

`--no-prepare` is an explicit command surface. It refuses Jetpack preparation before the `--` separator; the user can run `jetpack env --prep` to warm dependencies without running source.

### Why this is better than documenting the shell ritual

The repository already has the information needed to run the job. Requiring a human to manually arrange that information in the shell adds a failure path unrelated to the program. It also makes editor, terminal, CI, and task-runner launches behave differently.

The compiler should still not implement package resolution. It asks Jetpack for a typed, content-identified execution environment through the established boundary. One owner keeps resolution, acquisition, trust, and closure logic. The source command coordinates the complete user job.

### Alternatives

**A, recommended: delegated preparation by default, with locked/offline/trust controls and explicit refusal.** This is the Go-like one-command path. It amends the no-on-demand rule while preserving the binary responsibility boundary.

**B: keep mandatory explicit preparation and environment entry.** It makes acquisition impossible during a source command, but retains the shell-dependent beginner failure and editor/CI setup burden.

**C: enable only shell auto-activation.** The adopted hook remains useful, but it cannot cover every editor, CI process, or task runner. It also makes behavior depend on how the directory was entered.

The unavoidable cost in A is that a cold run may perform declared preparation and take longer. Hiding that time is not a fix. Show the stages, allow refusal, keep the warm path short, and never resolve a different version to avoid a failure.

## F10 · Generate the bad order of events, then shrink it

### The job

A connection opens, receives work, starts closing, and gets a late callback. A UI document is closed while a search result is still in flight. A file watcher sends duplicate changes. These are not single-input bugs. They depend on history.

A **stateful test** generates actions as well as values. A **shrinker** removes unnecessary actions and simplifies values until a small failing history remains. [Hypothesis](https://hypothesis.readthedocs.io/en/latest/stateful.html) already does this with rule-based state machines and reusable generated objects. Jet should borrow that concrete mechanism, not claim it invented model-based testing.

### Proposed test shape

`SessionModel` is a small independent reference model. `open_session` creates the real implementation. `session_actions` contains ordinary typed actions and valid-input generators; they are not parser magic.

```jet
// Proposed library API inside an ordinary test:
testing.histories(
    model: SessionModel.new,
    system: open_session,
    actions: session_actions,
    invariant: session_matches_model
) ?? panic("session history failed")
```

The runner may produce this kind of minimal counterexample:

```text
open
start request 1
close
deliver response for request 1

Expected: response rejected because the session is closed
Observed: response published into the next session
```

This is an illustrative output contract, not a claimed newly found Jet defect.

### What the language can derive, and what the user still specifies

| From checked declarations | From the test author or a domain contract |
|---|---|
| Parameter types and numeric bounds | What the application is supposed to accomplish |
| Nominal owner/state identities | A simple independent model when equivalence is required |
| Which operation consumes a handle | Meaningful input distributions and domain constraints |
| Declared errors and effects | Which external events to generate, including invalid ones |
| Registered state transitions where available | The observation that distinguishes correct from incorrect behavior |
| Existing generation/provenance checks | Whether a legal but surprising behavior is acceptable |

The runner must not derive both expected and actual results from the same implementation. That would compare a function to itself and call agreement correctness. Reuse type and transition facts for legal generation; keep the independent oracle independent.

### Invalid histories need their own channel

A typed API may make “use a consumed handle” impossible in ordinary Jet source. The test harness must not bypass I1 just to call it. Rejected source examples prove static rejection. A foreign-boundary or runtime event harness can inject a late external event through its explicit adversarial input boundary and check the safe rejection.

Legal histories and invalid boundary events are both valuable, but they are not the same test space. The report and runner must say which was explored.

### Shrinking must preserve the reason the history is meaningful

Deleting `open` while keeping `read` can make a history invalid before it reaches the original bug. A useful shrinker tracks created objects, consumed handles, and required prerequisites. It can remove an entire object lifetime or replace an action with a simpler valid one. It cannot celebrate a shorter trace that now fails for a different setup error.

The final artifact includes the action sequence, values, schedule choices, input identities, and candidate identity. It replays through the existing test/evidence commands and D-TEST-COMPARE1 observation model. A separate history CLI and new evidence database are unnecessary.

### Alternatives

**A, recommended: a typed history runner integrated with ordinary tests, the controlled world, and adopted comparison records.** This catches ordering defects and produces a small replay. Generation still needs a domain oracle and finite budgets.

**B: hand-write named event sequences.** Keep these for a known regression. They do not explore combinations the author did not anticipate.

**C: fuzz raw bytes only.** Keep byte fuzzing for parsers and wire boundaries. It cannot efficiently discover deep valid lifetimes by itself.

The complete design keeps B and C where they answer different questions. It removes one-off copies of history generation, shrinking, and replay mechanics, not useful test methods.

## F12 · Make program relationships visible at the source

### The problem is reconstruction, not a lack of dashboards

A programmer wants to know why a value changed, what a call can do, whether a view is still valid, or why an edit restarts the program. Raw fact dumps and unrelated trace files force them to reconstruct the answer.

A **source-first workbench** attaches a checked relationship or observed event to the code that caused it. It is not a new visual programming language and does not make a graph editor the source of truth.

D-EXPLANATION-RECORD1, D-EXPLAIN-VIEW1, D-LEARN-FEEDBACK1, D-SHARED-REVISION1, and D-TEST-COMPARE1 are already ratified A. The work below is their concrete product composition. Those decisions are not reopened or counted as new audit discoveries.

### Five useful views, one source selection

| User question | Default view | Expert expansion |
|---|---|---|
| What is this value? | Type, current observed value if captured, and plain meaning | Carrier, refinements, units, shape, provenance, revision, and unknown facts |
| Why did it change? | Changed input and the shortest dependency chain | Query operations, invalidation, incremental/recomputed work, and publication decision |
| Can this call change or escape something? | Read/write/move obligations and named effects | Returned-view provenance, foreign boundary, authority, retained callback lifetime |
| Why is this slow or allocating? | Measured cost at the source and the responsible operation | Copies, materialization, transfers, schedule, cache hit/miss, refused optimization |
| What did my edit change? | Changed observable behavior or a clearly stated unproved comparison | First differing observation, affected facts, candidate identity, and complete replay |

Static facts and observed values use distinct labels. “This branch is reachable” is not “this branch ran.” “These executions agreed” is not “all executions are equivalent.” A stale capture remains visibly tied to its old source revision.

### A concrete sales-total screen

```text
selected: .sum(s -> s.cents)

Type
  one total per Region, measured in integer cents

Cause
  sale 1 changed: 125 -> 175
  North total changed: 200 -> 250

Work
  updated one exact contribution
  did not reread the source file

Limits
  observed run at revision 19
  not a proof about every possible query
```

This example contains an actual teaching sequence: a changed input, a relationship, and the consequence. The UI need not display the words “temporal substrate” or “semantic projection” to explain it.

### Learning should require an action, not another paragraph

The adopted learning interaction can use the same ordinary program:

1. **Predict:** which total changes if sale 1 moves from North to South?
2. **Observe:** run the edit and inspect both totals.
3. **Explain:** show the removal from North and insertion into South.
4. **Modify:** add a filter and predict a sale crossing its boundary.
5. **Transfer:** apply the same dependency idea to a UI list or build input.

A wrong prediction gets the smallest counterexample, not praise followed by a lecture. The interface teaches programming concepts such as ownership, state, dependency, order, and abstraction. It does not require learning a proprietary diagram vocabulary first.

### The command and editor must consume the same answer

The existing `jet inspect` family, LSP, debugger/canvas, and adopted explanation views should consume the one typed explanation contract. A command prints it; an editor presents it beside source; a browser can show it as a relationship view. None should independently infer a different story from names or logs.

D-LEDGER1 keeps registry definitions, checked facts, and execution evidence in their appropriate stores. The workbench queries across them through their shared contract. It does not merge their lifetimes, mutable state, or storage formats into one giant database.

### One exact source-to-answer interaction

The following is a **proposed projection under D-EXPLAIN-VIEW1**, not a captured response from today's LSP. It uses ordinary LSP hover rather than creating a second checking API:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "textDocument/hover",
  "params": {
    "textDocument": { "uri": "file:///project/sales.jet" },
    "position": { "line": 19, "character": 12 }
  }
}
```

For the selected aggregate, a response can project the adopted explanation record into standard `Hover.contents`:

```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": {
    "contents": {
      "kind": "markdown",
      "value": "**Type:** Group<Region, Int>\n\n**Cause:** sale 1 changed by +50 cents; North changed from 200 to 250.\n\n**Evidence:** observed run at source revision 19. No universal proof claimed."
    }
  }
}
```

The editor obtains static types from checked facts and the changed-value claim from an explicitly selected run. With no run, it omits that claim and says “No captured run selected.” A hover never silently launches the program. A changed document invalidates the old hover; a pinned historical view keeps its revision label instead of pretending to describe current source.

**Keyboard path:** select the aggregate → invoke the editor's Explain action → read the summary → Tab through Type, Cause, Work, and Limits → Enter to expand one relationship → Escape to return focus to the selected source. This is an editor action, not new Jet syntax. The same path must work without a mouse or color.

**Text path:** the existing `jet explain --cost sales.jet` command is the home for static work explanations. Its proposed human-readable projection of the same selected operation is:

```text
sales.jet:20  sum(s -> s.cents)
type: Group<Region, Int>
work: one full source traversal
reason: no maintenance law selected for this plan
evidence: checked plan; not a runtime timing
```

That output is an acceptance example, not output observed in this audit. A JSON projection must retain the same location, revision, evidence class, and limitation. A richer pinned relationship panel may use the adopted explanation query, but must not introduce an independent semantic analyzer.

### Accessibility and trust are completion requirements

The workbench must work without color, with keyboard navigation, and at narrow and wide viewports. Focus must return to the selected source location. Long paths, missing captures, stale revisions, unsupported target observations, and redacted secrets are ordinary states with useful explanations.

No tool runs a program, acquires a dependency, calls a model, or exposes a secret merely because the user hovered a symbol. Static inspection stays static. Live capture requires the existing execution/trust boundary.

## Simplify the compiler and tool path around a checked module summary

The source observations in the first chapter show multiple cache kinds, a batch/editor query service, build-action records, and resident execution. These are not all duplicates. Their objects have different lifetimes and validity conditions.

The useful simplification is a **complete checked module summary** consumed by all downstream jobs. It carries the public type/effect contract, source/import identities, diagnostic completion, executable representation identity, and generated boundary facts. A downstream tool must not parse source again because one field was omitted from the summary.

| Keep separate | Share |
|---|---|
| Process-memory query memoization | Dependency identity and validity rules |
| Immutable build/run artifacts | Machine-store accounting and corruption policy |
| Whole-invocation observations | Candidate/input identity and declared observation scope |
| Editor overlays | The same checker and checked summary contract |
| Native/JIT/web machine outputs | The same executable meaning and source mapping |

The adopted MIR/Core and store decisions own the implementation. Delete only a duplicate durable cache owner or semantic route after its consumers migrate. Do not make `jet run` recheck everything just to force it through an inappropriate build-cache API. Equally, do not let a warm cache skip authority or reuse a verdict whose relevant inputs changed.

## What the user should not have to learn

The complete design does not add a universal graph language, a dependency-injection framework, a second reactive binding, a model-specific task system, a history-test command family, or a new package executor. It adds useful operations at existing boundaries and makes their relationships inspectable.

The test is practical: a new user runs a project, calls a model, watches a query, or tests a timeout before needing to understand the machinery described in this chapter. An expert can inspect and control that machinery when the job requires it.
