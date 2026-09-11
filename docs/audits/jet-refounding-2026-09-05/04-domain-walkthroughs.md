# The design must help finish an application

[Executive report](index.md) · [Tools and systems](03-tools-and-systems.md) · [Correctness and release](05-research-correctness-and-release.md)

These are complete **workload walkthroughs**, from input to deployment and failure handling. They are not eight runnable applications produced by this audit. Proposed code is a design excerpt, with its surrounding application responsibilities stated. The current batteries supply source evidence; their current-tree execution remains unproved here.

The same twelve changes should help several areas without flattening their differences. A GPU frame, an HTTP request, and an interrupt have different scheduling and failure contracts. Sharing ownership and dependency facts does not make their drivers interchangeable.

## 1 · Web: a support desk with a live queue

### What the application must do

A customer submits a ticket. Invalid fields appear beside the form. An authorized agent opens the ticket, changes its status, and sees the queue count update. Another agent must not overwrite a newer edit unknowingly. Reloading the browser must not lose committed state. Deployment must carry the actual server/client contract, assets, and database requirements.

### Current source and repeated work

The [web battery](../../../examples/features/web/battery/run.jet) declares an app graph with routes, actions, and forms, then uses a separate low-level mux/serve path and manual parameter, form, and session handling. Automatic graph serving is already ratified. A proposal that merely adds `app.serve()` would miss both the existing decision and the deeper duplication.

The missing relationship is between the ordinary typed operation, its HTTP binding, its public shape, and its client. F01 completes that relationship. F03 keeps the queue calculation current; the adopted shared revision rule prevents an older result from replacing a newer one.

### The proposed application structure

```text
Ticket input type + validation
              |
ordinary create/update/lookup functions
              |
checked endpoint descriptors
        /             \
server adapters    generated client
        \             /
        one public contract
              |
committed ticket source -> watched queue query -> UI
```

The public input and output types are explicit. A persistence record may contain internal audit fields that the browser does not receive. The endpoint layer cannot infer authorization from a function being public.

```jet
// Proposed endpoint declarations over existing application functions:
lookup :: web.get("/tickets/{id}", lookup_ticket)
create :: web.post("/tickets", create_ticket)
update :: web.post("/tickets/{id}/status", update_ticket_status)
```

The queue uses the same typed query for its first render and later changes. It does not maintain a separate `open_count` variable by hand. The database's committed changes feed the existing live-query boundary; uncommitted edits do not leak into a published queue.

### What happens when things go wrong

| Event | Required behavior |
|---|---|
| Ticket ID is not a positive integer | Decode/validation error before the handler runs |
| Two sources supply the same field | Reject the ambiguous request; do not choose a hidden winner |
| User lacks access | Explicit authorization failure; a generated client does not bypass it |
| Two agents edit the same revision | Existing revision/transaction contract reports conflict or applies its declared merge; no silent last-writer assumption |
| Old search request returns after a newer one | Reject stale publication under the shared revision rule |
| Database disconnects | Show unavailable/pending/error state, not an empty queue presented as truth |
| Browser closes a subscription | Detach it and release retained work through the existing lifecycle |
| Generated client is stale | Detect descriptor mismatch rather than decode a new response using old assumptions |

### Beginner and expert paths

The beginner writes records, validation, handlers, and an app graph, then runs the project. The expert inspects request binding, public fields, effects, rights, query work, retained state, and deployment inputs. They can replace transport mapping or use raw HTTP without abandoning the same domain functions.

### What still has to ship

Real graph serving, generated client calls, browser validation/focus states, session and CSRF behavior, transaction/revision integration, asset delivery, and deployment must be exercised together. F01 is not complete when OpenAPI text is generated. F03 is not complete when a counter changes in a headless example. The accepted web battery and its existing implementation owners remain the delivery vehicle.

**Matched peers:** a comparable FastAPI/Pydantic application and a Go HTTP application, with the same validation, auth, storage, response shape, and deployment assumptions. Measure startup, build, request latency, memory, and edit feedback separately.

## 2 · Games: a small action game that survives real frame histories

### What the application must do

Read input, simulate a fixed-time step, move entities, resolve collisions, render a camera view, play audio, and record enough input to reproduce a defect. Assets may load or unload while work is in flight. Resize, pause, slow frames, and scene transitions are normal states.

### Current source and the real gaps

The [game battery](../../../examples/features/game/battery/run.jet) has scene/assets/input/components, atlas and sprite work, gamepad/audio calls, replay, fixed-step/interpolation logic, and a hand-written axis-aligned bounding-box test. Earlier old-binary reports lacked some APIs now present in source. They are not evidence that those exports remain absent.

What the source does not establish is a complete typed-space path, a complete frame-resource scheduler, or a physically verified renderer/gamepad/audio release. F04, F05, F06, and F10 address distinct parts of that job.

### A player's click should not depend on a naming convention

```jet
// Proposed typed-space picking excerpt:
mouse :: window.pointer_position()
ray :: camera.ray_from_screen(mouse) ?? panic("stale camera frame")
hit :: scene.pick(ray)
```

The screen position carries its space and relevant frame identity. A perspective camera returns a ray because one pixel does not identify a unique point in three-dimensional space. Picking uses the ray against the scene. There is no unchecked addition of camera offsets to raw mouse numbers.

### The frame uses checked resources

Simulation writes positions. Rendering reads positions and assets. Compositing reads the rendered image. F06 derives those dependencies from checked accesses. A texture owner remains alive until the GPU's completion, not only until command submission returns.

The expert can inspect the schedule, barriers, transfers, and buffer reuse. Requiring a device or forbidding a transfer uses the protected placement/acceleration policy. No new “game fast mode” overrides it.

### Which C++-style glitches become impossible, less likely, or unchanged?

| Defect class | Proposed prevention | Remaining limit |
|---|---|---|
| Dangling entity/asset pointer | Existing ownership/generation checks | A semantically wrong but still-live entity remains possible |
| Screen point treated as world point | F04 type mismatch | Choosing the wrong valid camera is still an application error |
| Temporary GPU image reused too early | F06 lifetime and completion dependency | Driver correctness remains a trusted boundary |
| Late asset callback updates a new scene | Shared revision/owner identity and F10 histories | Every external callback must enter through the checked boundary |
| Milliseconds treated as seconds | Existing units and dimensional rules | An intentionally wrong timestep can still be type-correct |
| Frame-rate-dependent motion | Fixed-step contract plus controlled-time/replay proof | A variable-step game can still be a deliberate choice |
| Object tunnels through a thin wall | A swept/continuous collision algorithm, not memory safety | Physics approximation and geometry remain domain work |
| Jitter from inconsistent interpolation | One declared simulation/render time relation and tests | Correct interpolation can still look bad by artistic judgment |
| Nondeterministic replay | Controlled inputs, explicit numerical/scheduling contract, honest unsupported boundaries | Floating-point/device differences cannot be waved away with a seed |

The important distinction is that a language can rule out invalid operations, but not infer the intended game design. The audit should promise the former and help test the latter.

### What still has to ship

A usable game needs real graphics pipelines, shader/resource tooling, audio decoding/streaming, device input, asset packaging, debugging, and a frame profiler. The current source and incumbent gap reports do not establish all of those. The language proposals reduce error-prone glue; they do not turn a headless replay transcript into a finished game engine.

**Matched peers:** the same game loop and assets in Bevy/Rust and a C++ engine/library path. Compare correct output or controlled state, frame-time distribution, startup/build time, memory, and device transfers. Do not compare a headless Jet update loop against a peer that also renders and decodes assets.

## 3 · CLI and scripts: a repository search-and-rewrite tool

### What the application must do

Read command-line options, stream stdin or walk files, obey ignore rules, report useful errors, optionally preview edits, and perform an explicit write. It must handle broken pipes, non-UTF-8 paths where applicable, permission errors, cancellation, and deterministic output order.

### Current source

The [CLI battery](../../../examples/features/cli/battery/run.jet) has typed subcommands, helpers, process/record paths, and `#Job`. It also names outstanding stdin/ignore/release gates. A printed future command is not evidence that the path works.

The strongest simplification is to finish the ratified typed argument/shape mechanism and make F09 eliminate environment setup unrelated to the script. No new argument parser, script language, or command hierarchy is proposed.

### A useful default should stay a plain function

```jet
// Proposed application excerpt; the argument contract is ordinary Jet.
fn rewrite(pattern: String, replacement: String, * write: Bool = false) {
    plan :: build_rewrite_plan(pattern, replacement)
        ?? panic("cannot inspect inputs")
    print(plan.preview())
    if write -> plan.apply() ?? panic("write failed")
}
```

The excerpt names application helpers, not new Core APIs. The relevant design is the boundary: parsing and validation come from the ordinary parameter contract; the application has a preview value; applying it is an explicit effectful action.

The exact current parameter-zone spelling remains governed by its ratified decision. This proposed example does not add a second keyword-only parameter mechanism.

### The environment is not the user's first debugging task

Under F09, `jet run rewrite.jet` obtains the declared execution environment through Jetpack. `--locked` and `--offline` keep their strict meanings. A missing declared tool is not replaced by a similarly named host executable. The same launch should work from the terminal, editor, or CI process.

### Failure and expert control

A preview is tied to input identities. If a file changes before apply, the operation reports the conflict instead of overwriting newer data. Stdin is one-shot and bounded according to the operation. Broken-pipe handling must match CLI conventions without disguising another I/O error as success.

The expert controls traversal, concurrency, ordering, limits, target environment, and output format. `NO_COLOR` changes presentation, not the information available. The default help should explain the task and next action before listing every implementation option.

### What still has to ship

The full stdin/ignore/path/error/write matrix, completion/help integration, packaging, and an actual install-and-run path remain required. A typed command declaration alone does not establish a usable CLI ecosystem.

**Matched peers:** ripgrep-like search where that is the job, and equivalent Go/Rust/Python scripts for rewrite behavior. Compare the same file set, ignore rules, encodings, output order, and write safety. Build/install/startup time matters as much as an inner loop.

## 4 · Data analysis: the same report from CSV, Parquet, and live edits

### What the application must do

Load typed sales, validate them, join reference data, group totals, draw a report, and explain a discrepancy. A small dataset should require little ceremony. A larger one should use streaming/columnar execution without rewriting business meaning.

### Current source

The [analysis example](../../../examples/features/tooling/data_analysis.jet) already parses typed CSV and performs filtering, sorting, joining, grouping, statistics, and text/SVG plotting. The [SQL example](../../../examples/features/serde/analytics_query.jet) already uses checked SQL. The current reference's fixed string/Float aggregate and separate lazy family are the concrete F02 targets.

### One calculation, three source lifetimes

```jet
// Proposed query; Sale retains its own field types.
plan :: data.query(source)
    .filter(s -> s.cents > 0)
    .group_by(s -> s.region)
    .sum(s -> s.cents)
```

| Source | What the user asks for | What must remain the same |
|---|---|---|
| Ordinary `[Sale]` | `plan.collect()` | Typed keys, amounts, order, and errors |
| Typed Parquet/Arrow source | `plan.collect()` | The same query meaning, with visible copy/read/spill choices |
| Explicit changing source | `plan.watch()` | The batch answer at each committed revision |

The file reader and changing-source adapter do not own a second aggregation algorithm. A plot consumes the typed result rather than requiring a conversion back into a fixed `DataGroup` shape.

### A discrepancy should be inspectable

Select a total and see the contributing rows, applied filter, key type, exact or approximate numerical operation, and source revision. If a file is invalid, the error names the field and source location. If a rewrite skipped work, the explanation names the validated fact that allowed it.

The workbench should distinguish “not read,” “read and rejected,” “filtered out,” and “contributed zero.” Those states can produce the same-looking empty chart but mean very different things.

### What still has to ship

Real Parquet/Arrow reading, encoding coverage, corruption limits, bounded out-of-core execution, correct joins and order, typed plotting adapters, and live-query integration are required. Distributed analytics is not implied by a local query plan; it is a separate placement, failure, and consistency problem. The report does not add a distributed execution mechanism merely to say “scales.”

**Matched peers:** Polars, DuckDB, and the relevant Rust/Python baseline. Match schema, validation, nullability, exactness, grouping order, scan projection, cold/warm state, and output materialization. A loss on any required metric stays a loss.

## 5 · Backend services: a bounded API that shuts down correctly

### What the application must do

Accept requests, authenticate them, use a database pool, enforce deadlines and load limits, record useful observations, and stop cleanly. A request may be cancelled after it has performed an external effect. Shutdown may race with a callback or a database response.

### Current source

The [backend battery](../../../examples/features/net/backend_battery/run.jet) builds router/OpenAPI work, then separately composes mux, request identity, binding, deadline, load, trace, and shutdown behavior. The current Core export includes `db.pool` and `process.on_signal`; this is not a claim that those symbols are absent.

F01 removes duplicate endpoint descriptions. F05 makes deadline/cancellation behavior controllable. F10 searches the histories that hand-written happy-path tests miss. The existing task, effect, and bounded-buffering contracts remain the semantic owners.

### A cancellation does not undo a charge

```text
request admitted
payment provider accepts charge
client disconnects
service receives cancellation
```

The service must not report “nothing happened” or automatically retry the charge. The result records the completed external effect and the remaining uncertainty. An idempotency key is an application protocol, not a generic retry flag supplied by the language.

This is why F05 controls the timing of an external response but does not invent rollback. It can place cancellation before acceptance, after acceptance, or before response delivery and check the service's declared behavior in each case.

### Proposed testing composition

```jet
// Proposed: the same request protocol runs in a controlled world.
testing.world(world -> {
    world.transport.install(recorded_payment_provider)
    testing.histories(
        model: PaymentSessionModel.new,
        system: start_service,
        actions: payment_histories,
        invariant: payment_observations_agree
    ) ?? panic("payment history failed")
})
```

The model distinguishes “not sent,” “accepted,” “completed,” and “unknown.” The production service is real inside the controlled environment; the oracle is independent. A transport fixture supplies external events, not the expected business result.

### Required failure states

| State | Required response |
|---|---|
| Admission limit reached | Bounded refusal/backpressure under declared policy |
| Pool unavailable | Typed availability failure, not an unbounded wait |
| Deadline expires before an external effect | Cancel pending work and report the defined timeout |
| Deadline expires after an effect | Preserve the completed effect and report remaining uncertainty |
| Shutdown starts | Stop admitting new work, drain/cancel owned work under policy |
| Callback arrives after owner release | Checked rejection or retained quarantine according to the foreign contract |
| Trace contains a secret | Redact by the existing policy; inspection does not become a data leak |

### What still has to ship

A real network/database integration, graceful shutdown, client/server contract evolution, deployment artifact, operational limits, and failure behavior under load. A router benchmark alone does not prove a backend service is ready.

**Matched peers:** Go and Rust services with the same database, auth, deadlines, body limits, logging, and shutdown semantics. Include overload and cancellation, not only maximum successful requests per second.

## 6 · AI/ML applications: search with a model that matches its index

### What the application must do

Read documents, produce embeddings, build an index, encode a query, rank results, and optionally stream an answer. A model update must not silently reuse an incompatible index. The deployment must pin the actual model and backend, not merely the application source.

### Current source

The [ML battery](../../../examples/features/math/ml_battery/run.jet) combines manual compute/data and inference-adjacent operations. It establishes useful source parts, not a complete model-package or production retrieval interface. F08 fills that product boundary, using F07 for compatible data and F06's existing resource facts where device execution applies.

### The proposed pipeline

```text
pinned documents -> typed preprocessing -> model package
                                              |
                                   embeddings with model identity
                                              |
                               index with metric and dimension
                                              |
query text -> same encoder -> checked query -> ranked results
```

Changing the model invalidates the index relationship. Changing only a UI color does not. A content-derived identity explains the difference without a user-maintained “rebuild all embeddings” flag.

```jet
// Proposed model/index use:
encoder :: models.open(search_encoder) ?? panic("model load failed")
vector :: encoder.embed(query_text) ?? panic("encoding failed")
results :: index.nearest(vector, count: 5) ?? panic("index mismatch")
```

The typed result is not a promise that the search is good. Relevance evaluation remains a domain metric. Approximate search reports its algorithm and measured recall; exact search remains the comparison baseline.

### Failure and expert control

The beginner chooses a declared model package and supplies text. The expert inspects tensor shapes, numerical format, operator/provider selection, retained cache, device transfers, batch limits, and reproducibility scope. A requested GPU that is unavailable does not silently become CPU when the existing policy forbids fallback.

A tokenizer mismatch, wrong vector dimension, incompatible index metric, exhausted context, unsupported operator, and cancelled stream are distinct failures. A model cannot acquire tool authority by emitting a string that resembles a command.

### What still has to ship

A real supported inference backend, complete model artifact validation, tokenizer/preprocessing integration, streaming and cancellation, exact/approximate retrieval contracts, packaging, and native/web target proof. Training frameworks, distributed training, and every model architecture are not silently included in “load a model.” They need their own concrete supported-workload obligations.

**Matched peers:** the same ONNX Runtime/Python or other named engine path, model, inputs, numerical mode, device, batching, and output quality. Include model load, first result, sustained throughput, memory, and data transfers. Jet cannot claim an inference win by comparing different weights or lower-quality quantization.

## 7 · GUI: a document editor that survives close, undo, and resize

### What the application must do

Open a document, edit fields, search asynchronously, undo/redo, handle input methods, navigate by keyboard, support accessibility, and close while work is in flight. A native package must open on the declared host, not only in a headless tree renderer.

### Current source

The [GUI battery](../../../examples/features/ui/battery/run.jet) explicitly handles reactive nodes, focus, shortcuts, input methods, drag/drop, history, undo, theme, and host capability branches. The reference names null/TUI/browser/Linux GTK4 paths and unsupported native hosts. These are source/contract facts, not fresh native interaction proof.

The ratified typed-dot UI surface remains the node construction direction. This audit does not propose a second markup language. The important composition is shared revision identity, typed coordinates, controlled asynchronous work, and visible relationships.

### The late-search bug has one structural fix

```text
document A opens at revision 8
search starts for A / revision 8
A closes
B opens
old search returns
```

The result carries its document owner and revision. Publication checks that identity against the current target. It cannot publish into B merely because a widget occupies the same slot. This uses the same generation/revision authority as F03, not a separate GUI-only stale callback rule.

### A beginner-facing interaction

The ordinary UI binds a field to a typed value through the adopted reactive mechanism. A validation error appears at the field. A pending search is visibly pending. Undo applies a checked edit to the document state, not a blind restoration of stale background-task objects.

The workbench can show why a field updated, which dependency triggered it, and whether the displayed result is current. It teaches the relationship using the user's code, not a separate diagram language.

### What has to be checked with human interaction

| Interaction | Good behavior |
|---|---|
| Tab/Shift-Tab | Predictable focus order, visible focus, no trap |
| Input-method composition | No premature commit or lost composed text |
| Resize or display-scale change | Pointer conversions use the matching coordinate/frame context |
| Undo during pending work | New revision is coherent; old work cannot overwrite it |
| Close during search | Work is cancelled/accounted for and cannot reach a replacement document |
| Error state | Field association, readable text, and accessible announcement |
| No color or reduced motion | Meaning and focus remain available |

### What still has to ship

Native host controls, accessibility bridges, lifecycle and packaging on each claimed platform, plus actual keyboard/input-method/window interaction. A browser demo cannot prove a Windows native host. A source `supported` flag cannot replace physical or suitable automated host evidence.

**Matched peers:** an equivalent native toolkit application and a browser application where relevant. Measure startup, memory, input latency, edit feedback, package size, and accessibility/task completion. Native appearance remains an owner visual gate, not a test assertion about pixels alone.

## 8 · Embedded: a sensor logger with bounded interrupt and DMA lifetimes

### What the application must do

Configure a board, sample a sensor, transfer bytes, process interrupts, keep a bounded buffer, and recover from a device error. It must fit the declared memory/time limits and build a real artifact for the board.

A **hardware abstraction layer** is a library that turns device registers and pins into checked operations. **DMA** lets hardware transfer data without the CPU copying every byte. The hardware still needs exclusive access to a live buffer while the transfer is active.

### Current source

The [embedded battery](../../../examples/embedded/battery/run.jet) declares board/register, interrupt, DMA, ring, and slot policy manually and uses a host replay. The [hardware kernel](../../../crates/jet-codegen/src/Prelude/Core/EmbeddedHardware.rs) has typed widths/access, register blocks, interrupts, and DMA ownership. The reference explicitly does not claim a complete board-support package or HAL.

F05 supplies controlled event histories; F10 explores overflow/cancellation/order; F06's access and lifetime facts can support dependency derivation without imposing a renderer's scheduler on an interrupt handler. Existing units and resource ownership carry the ordinary safety.

### The intended buffer lifetime

```text
application owns sample buffer
              |
start DMA by transferring the owner
              |
hardware transfer owns buffer; application cannot mutate it
              |
completion or acknowledged stop
              |
application receives the buffer back
```

A proposed domain API can express that with ordinary ownership:

```jet
// Proposed HAL excerpt; no new ownership syntax.
transfer :: uart.send(^buffer) ?? panic("transfer not started")
buffer :: transfer.join() ?? panic("transfer failed")
```

The failure contract must distinguish “not started” from “hardware may still access memory.” A failed stop cannot free a buffer while DMA continues. Returning a buffer on every error would be a dangerous convenience, not simplification.

### Board facts should be declared once

A concrete board package should own pin multiplexing, clock relationships, register layout, interrupt identity, and DMA-channel compatibility. Generated initialization and inspection should project those facts. The application should not repeat the same peripheral choice in startup code, a linker fragment, and a driver configuration.

This is a completion direction for the board/battery layer, not a new universal hardware DSL. The proposed package must use the existing typed hardware and package mechanisms. Each supported board still needs actual startup/linker/flash/probe evidence and its real hardware limits.

### Host simulation cannot prove physical timing

| Evidence | What it can establish | What it cannot establish |
|---|---|---|
| Checked types/ownership | Invalid widths/access/lifetimes rejected within the declared model | Correctness of the chip's undocumented behavior |
| Controlled interrupt history | Queue overflow, ordering, cancellation, and state transitions | Electrical behavior or interrupt latency on the board |
| Emulator | Supported instruction/device behavior and artifact boot path | Every peripheral's timing and silicon erratum |
| Physical board | The exercised wiring, peripheral, timing, and reset behavior | All boards and all future environmental conditions |

The final release record names which evidence applies. “Host replay passed” never becomes “hardware verified.”

### What still has to ship

A real board-support package/HAL, buses and peripherals for the selected workload, startup/linker/flash/debug path, bounded-buffer behavior, target artifact, and physical or explicitly scoped emulator proof. Bare-metal support is not complete because the host can print a register transcript.

**Matched peers:** C/Rust/embedded HAL implementations of the same board workload, clock, optimization mode, buffer size, and interrupt conditions. Compare binary size, memory, latency/deadlines, and energy where the required workload measures it. A host simulation is not the peer for a physical board loop.

## The common pattern is smaller than the domain list

| Repeated problem | Shared language/library answer | Domain-specific part that stays |
|---|---|---|
| Two descriptions of one operation | Checked type/function descriptor | HTTP mapping, model graph, peripheral binding |
| Old work reaches a new owner | Shared owner/revision identity | Document, scene, request, or device lifetime |
| Same calculation maintained twice | Typed query plus checked incremental transformation | Query eligibility and source transaction rules |
| Raw numbers hide incompatible meanings | Units, nominal spaces, and shape knowledge | Camera, sensor, currency, or model semantics |
| Timing makes tests unreliable | Controlled world plus recorded events | Real device/driver evidence |
| A wrapper hides copies or transfers | Typed ownership boundary plus explanation | Layout, backend, and hardware cost |
| Test generation lacks valid lifetimes | Typed history generation and shrinking | Independent business oracle |

That is the proposed competitive advantage: not one grand framework for every domain, but ordinary Jet concepts that remain useful as an application crosses boundaries. The eight complete battery deliveries are the test of that claim.
