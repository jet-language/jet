# Adversarial Design Review: Unified Breadcrumb Frame Stack (`#Frame(text) { }`)

**Date:** 2026-11-26
**Scope:** Attack proposal item P11 (`#Frame("importing {file.name}") { … }` pushes one breadcrumb frame that errors, log lines, trace spans, and `jet perf` samples inside all carry), inventory every existing framing/span/zone/context mechanism in the checkout, specify the exact cutover to one shared runtime mechanism, and list every duplicate stack deleted.

---

## 1. Current Inventory: Four Disconnected Mechanisms

Before this change, a Jet program has to maintain four separate concepts to explain "what operation is currently running":

| Surface | Files | Storage / Stack Today | What Sees It Today | Gaps & Duplication |
|---|---|---|---|---|
| **Error context frames (`?("note")`)** | [`Compiler/JetRuntime/Source/Err.jet`](../../Compiler/JetRuntime/Source/Err.jet), [`Compiler/JetRuntime/Source/Journey.jet`](../../Compiler/JetRuntime/Source/Journey.jet), [`Compiler/JetRuntime/Source/JourneyTLS.jet`](../../Compiler/JetRuntime/Source/JourneyTLS.jet), [`Compiler/JetRuntime/Source/ErrReport.jet`](../../Compiler/JetRuntime/Source/ErrReport.jet) | `Err` inline list `ERR_CONTEXT` (`ERR_CONTEXT_BYTES = 24`: `{text, file, line}`) populated only at `?("note")` call sites via `jet_err_with_context_frame`. | `jet_render_err`, `jet_error_report_json` (`context_frames`). | Blind to active `log.span` or `perf.zone` scopes; every call site has to repeat `?("...")` manually even when an enclosing block already named the unit of work. |
| **Structured log spans (`core.log.span`)** | [`Core/Log/Log.jet`](../../Core/Log/Log.jet), [`Compiler/JetRuntime/Source/LogSpan.jet`](../../Compiler/JetRuntime/Source/LogSpan.jet), [`Compiler/JetRuntime/Source/RouteALog.jet`](../../Compiler/JetRuntime/Source/RouteALog.jet) | Separate thread-local dynamic array `LOG_STATE_SPANS` (`LOG_SPAN_SLOT = 24`: `{id, name, start}`) inside `LogState80` (`LOG_STATE_SPANS`, `LOG_STATE_SPANS_LEN`, `LOG_STATE_SPANS_CAP`). | `log_emit_json` (`"spans":[...]`), `log_span_row_write` (`PERF_ZONE_LOG` on close). | **Duplicate stack #1.** Thread-local rather than task-inherited (spawned tasks drop parent spans). `log_emit_text` completely omits active spans. `SourceSampling.jet` skips `JET_CLI_PERF_ZONE_LOG` during CPU-sample correlation. Invisible to `Err` and `jet debug`. |
| **Profiler zones (`core.perf.zone`)** | [`Core/Perf/Perf.jet`](../../Core/Perf/Perf.jet), [`Compiler/JetRuntime/Source/PerfSamples.jet`](../../Compiler/JetRuntime/Source/PerfSamples.jet), [`Compiler/JetCLI/Source/CLI/Perf/SourceSampling.jet`](../../Compiler/JetCLI/Source/CLI/Perf/SourceSampling.jet), [`Compiler/JetCLI/Source/CLI/Perf/Command.jet`](../../Compiler/JetCLI/Source/CLI/Perf/Command.jet) | Separate thread-local open-zone stack `PERF_ZONE_OPEN_AT` (`PERF_ZONE_OPEN_BYTES = 4096`) inside `PERF_ZONE_TCB_RING`. | `jet perf run` / `jet perf view` (`zone <name> count=... samples=...`). | **Duplicate stack #2.** Only active under `jet perf`; completely invisible to `Err`, `core.log`, and `jet debug`. Nested zones do not form a breadcrumb path on samples or errors. |
| **Debugger & crash-dump frames (`jet debug`, `context.trace`)** | [`Core/Debug/Debug.jet`](../../Core/Debug/Debug.jet), [`Compiler/JetDebug/Source/Native.jet`](../../Compiler/JetDebug/Source/Native.jet), [`Compiler/JetDebug/Source/NativeDump.jet`](../../Compiler/JetDebug/Source/NativeDump.jet) | Call-stack unwinder only (`NativeFrame` list from CFI/frame pointers). | `jet debug` (`Stopped`, `bt`, `frame N`), `context.trace`. | Shows only machine/Jet function frames; has no view of active high-level breadcrumb scopes (`#Frame`) on the stopped task's `Context`. |

---

## 2. Adversarial Critique of Proposal Item P11

1. **Why not another ad-hoc thread-local stack?**
   Adding a fourth thread-local array for `#Frame` would make the duplication worse and would immediately lose frames across `task { ... }` spawns and fiber migrations. Jet already has a task-owned, refcounted, copy-on-write context chain on every task's `Context` record (`TCB@48 -> CONTEXT_VALUES` in [`Compiler/JetRuntime/Source/Context.jet`](../../Compiler/JetRuntime/Source/Context.jet)), already shared across spawned tasks in O(1) via `context_inherit` (one `retain`) and released via `context_release`. Putting the **one** shared frame stack on `CONTEXT_VALUES` (key `CONTEXT_FRAME_KEY = 4` with `CONTEXT_IMPERATIVE_FLAG`) solves task inheritance, fiber migration, lifetime safety, and zero-copy sharing simultaneously.
2. **Lexical `#Frame` vs imperative `log.span` / `perf.zone` interleaving:**
   A user can open a `log.span("a")` inside `#Frame("outer") { ... }` and close it explicitly (`log.close(s)`), or hold a `LogSpan` past its enclosing block. Using `CONTEXT_IMPERATIVE_FLAG` (bit 62) ensures `#Context(key: val)` exits (`jet_ctx_value_pop`) never disturb frame nodes, and popping a frame by its monotonic `id` (`context_frame_pop_id`) handles both LIFO lexical `#Frame` exits and out-of-order `log.close(span)` safely via copy-on-write when shared with a child task.
3. **Zero-overhead fast path for errors and non-profiled runs:**
   When no `#Frame` is active, `context_current()` has no `CONTEXT_FRAME_KEY` node, so `Err` construction does a single pointer check (or fast head walk of short context chains) with zero allocations. Existing `perf.zone` patched-jump sites in `X64Select.jet` stay zero-overhead when `jet perf` is off; when zones are enabled, they push/pop through the shared `Context` frame stack instead of maintaining `PERF_ZONE_OPEN_AT`.
4. **Error context compatibility (`?("note")`):**
   `?("note")` remains the lightweight call-site annotation (`jet_err_with_context_frame`). Fresh `Err` creation (`jet_rt_record_new_jet_err`, `jet_err_from_message`) automatically snapshots active `#Frame` nodes into `Err`'s `ERR_CONTEXT` rows (outermost-to-innermost) — never inside `jet_rt_handle_clone_jet_err`, so cloning never duplicates frames. Existing `?("note")` UI/golden tests without active `#Frame` blocks remain byte-for-byte unchanged, while errors raised inside `#Frame("importing {file.name}") { ... }` automatically carry the breadcrumb with file and line.
5. **No marker/trait or closed-list churn:**
   Checking the entire checkout shows zero `Frame: trait` or `#Frame` collisions (plain `Frame: struct` types coexist cleanly like `State`, `Layout`, and `Region`). Registering `Frame: struct { text: String, impl Marker { sites: [.Block] } }` in [`Compiler/Prelude/Markers.jet`](../../Compiler/Prelude/Markers.jet) and routing `#Frame(text) { ... }` through `Stmt.ContextBlock` / `IRTxFrame` (like `#Deadline`) requires **zero** new `MarkerHook` enum variants, leaving `Tests/Guards/closed_lists/kernel.tsv` untouched.
6. **Canonical unified frame-stack rendering (`n4`):**
   Every surface renders a nonempty active breadcrumb stack identically, outermost to innermost joined by ` > `:
   - Canonical breadcrumb path: `outer > inner`
   - Canonical stack annotation: `frames=[outer > inner]`
   - **`core.log` text output (`RouteALog.jet log_emit_text`)**: appends ` frames=[outer > inner]` whenever frames are active; JSON output keeps `"spans":["outer","inner"]` (preserving existing JSON log contracts while now including `#Frame`, `log.span`, and `perf.zone` frames, inherited across spawned tasks).
   - **`jet perf view` (`Compiler/JetCLI/Source/CLI/Perf/Command.jet` + `SourceSampling.jet`)**: correlates CPU samples inside `#Frame`, `perf.zone`, and `log.span` intervals and renders breadcrumb stacks with the same `outer > inner` / `frames=[outer > inner]` presentation while preserving existing `zone <name> count=... samples=... total=...ns longest=...ns` totals for unnested zones.
   - **`jet debug` (`Compiler/JetDebug/Source/Native.jet` + `NativeDump.jet`)**: reads the stopped task's `TCB@48 -> Context@32` chain directly from target memory (both live sessions and `.jetdump` crash dumps) and renders `frames=[outer > inner]` in stopped-frame output and `bt`.

---

## 3. Duplicate Mechanisms Deleted / Consolidated

1. **`RouteALog.jet` `LOG_STATE_SPANS` duplicate stack — DELETED:**
   - Removed `LOG_STATE_SPANS`, `LOG_STATE_SPANS_LEN`, `LOG_STATE_SPANS_CAP`, and `LOG_SPAN_SLOT` from `LogState80` (shrinking `LogState` from 80 bytes to 56 bytes).
   - `jet_ring_log_enter`, `jet_ring_log_close`, `log_spans_report_open`, `log_spans_json_push`, and `log_emit_text` all read and mutate the single `Context` frame stack in `Context.jet`.
2. **`PerfSamples.jet` `PERF_ZONE_OPEN_AT` duplicate open-zone stack — CONSOLIDATED:**
   - `jet_perf_zone_open_named`, `jet_perf_zone_close`, and `perf_zone_flush_open` push, pop, and flush open zones through the single `Context` frame stack in `Context.jet` rather than maintaining a second thread-local open-zone array.
3. **Sample correlation split between `PERF_ZONE_ZONE` and `PERF_ZONE_LOG` — CONSOLIDATED:**
   - `SourceSampling.jet` correlates timed CPU samples for closed `#Frame`, `perf.zone`, and `log.span` intervals through one unified path.

---

## 4. Exact Cutover List

### Stage 1 (`seed: s1` — Compiler, Runtime, CLI Perf/Debug, Editor Grammars)
1. `Docs/Audits/2026-11-26-frame-breadcrumb-review.md` — this design review (`n1`).
2. `Compiler/Prelude/Markers.jet` + regenerated `Compiler/JetFoundation/Source/Policy/PreludeMarkers.jet` + regenerated `Compiler/JetSema/Source/Sema/PreludeRows.jet` — declare `Frame: struct { text: String, impl Marker { sites: [.Block] } }` (`n2`).
3. `Compiler/JetFoundation/Data/Syntax/CoreSurface.jet` — register `FRAME_BLOCK: "Frame"` (`n2`).
4. `Compiler/JetParser/Source/Parser/Statements.jet` & `Compiler/JetParser/Source/Formatter/Statements.jet` — parse and losslessly format `#Frame(text) { … }` via `Stmt.ContextBlock` (`n2`).
5. `Compiler/JetSema/Source/Sema/ContextKeys.jet` & `Compiler/JetSema/Source/Sema/Statements/ContextBlocks.jet` — check `field.name == "frame"` against `Type.String` without altering allocator context (`n2`).
6. `Compiler/JetIR/Source/BuildTransact.jet` & `Compiler/JetIR/Source/RuntimeRoutes.jet` — lower `#Frame(text) { … }` onto `IRTxFrame` calling `jet_ctx_push_frame(text, file, line)` on entry and `jet_ctx_pop_frame(id)` on every exit path (`n2`).
7. `Compiler/JetRuntime/Source/Context.jet` — implement the one shared `CONTEXT_FRAME_KEY` frame stack, task inheritance, push/pop by id, error snapshotting, perf-span emission on pop, and canonical `frames=[outer > inner]` formatting (`n2`, `n3`).
8. `Compiler/JetRuntime/Source/Err.jet` — snapshot active context frames into fresh `Err` instances (`n2`, `n3`).
9. `Compiler/JetRuntime/Source/RouteALog.jet` — delete `LOG_STATE_SPANS` and route all log spans and text/JSON log rendering through `Context.jet` (`n3`, `n4`).
10. `Compiler/JetRuntime/Source/PerfSamples.jet` — route `jet_perf_zone_open_named` / `jet_perf_zone_close` / `perf_zone_flush_open` through `Context.jet` (`n3`).
11. `Compiler/JetCLI/Source/CLI/Perf/SourceSampling.jet` & `Compiler/JetCLI/Source/CLI/Perf/Command.jet` — correlate CPU samples across frames/zones/log spans and render frame stacks in `jet perf view` (`n3`, `n4`).
12. `Compiler/JetDebug/Source/Native.jet` & `Compiler/JetDebug/Source/NativeDump.jet` — walk target `TCB@48 -> Context@32` to render `frames=[outer > inner]` in `jet debug` live sessions and crash dumps (`n4`).
13. `Tools/Editors/` (`jet.tmLanguage.json`, `jet.tmGrammar`, `TreeSitter/grammar.js`, `Zed/languages/jet/highlights.scm`) — regenerated via `jet self devtools grammars` (`n5`).

### Stage 2 (Verified Live on Rebuilt S1 Compiler — Examples, Goldens, Live Cases, Docs)
1. `Examples/ErrorsAndEffects/Errors/FrameBreadcrumbs.{jet,out}` + `Examples/ErrorsAndEffects/README.md` — executable example verified on O0/O1/O2 (`n5`).
2. End-to-end live cases in `Tests/Cases/` proving `#Frame(text) { }` across errors + `?("note")` + spawned tasks, `core.log` text & JSON output, `jet perf run` / `jet perf view` sample attribution, and `jet debug` unified `frames=[outer > inner]` rendering (`n2`, `n3`, `n4`).
3. `Docs/Spec/spec.md` & `Docs/Spec/Reference/core-library.md` — document `#Frame(text) { }` and unified frame-stack rendering (`n5`).
