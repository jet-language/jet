# Mine for Jet: Jonathan Blow's code visualizer and a Jet code map (2026-09-30)

## Verdict

Jet cannot show any of the modes in Blow's visualizer today, and it has no visual code map. It does have most of the facts needed to build one. Sema already records allocation and reference-count events, dispatch sites with no known callee, and resolved call edges. Canvas (`jet dev --canvas`) is a browser IDE that re-checks on every save. Three pieces are missing: per-function metric rows, a command to print them, and a map view. They are tracked as #3867 and #3868, with three owner ballots.

The mining also found a defect in the one metric Jet already prints. `jet lint --complexity` documents a nesting term that it never applies, and the UI goldens pin the wrong scores. That is #3866, and it blocks the metric card because the map reuses the same nesting walker.

## Source and scope

| Field | Value |
|---|---|
| Source ID | `PbVHBToltXk` (checker status `new`; no rerun) |
| Kind | video (clip) |
| Canonical URL | https://www.youtube.com/watch?v=PbVHBToltXk |
| Title / channel | "Jonathan Blow on Visualizing Code Complexity" / Jonathan Blow Clips |
| Published / length | 2026-09-29 / 10:52 |
| Shared `source_identity` | `talk:jblow:IdpD5QIVOKQ` (the clip cuts from https://youtube.com/watch?v=IdpD5QIVOKQ) |
| Retrieved | 2026-09-30 |
| Jet subject | `jet inspect` and a visual tool for code hotspots |
| Requested outcome | support every mode shown in the video, plus a visual tool; Tower-enabled mining |

Lanes run: source capture (transcript and video frames), Jet cross-check, and a live probe. Audience review was not run; the request is about the tool's modes, and the clip has 8 comments. No other resource was in scope.

### Capture quality

- Captions are YouTube auto-captions (`en-orig`); there are no creator captions. The first subtitle request hit HTTP 429; a retry succeeded.
- The video downloaded only as 360p (format 18) through the `tv` client; other clients returned 403. Frames were sampled every 8 s and read at full size at 02:24, 04:56, 05:12, 07:12, and 07:20. Dropdown labels are legible; small hover-panel numbers are partly legible.
- The linked full talk was not captured.

## What the source shows

The tool reads a metrics file that the Jai compiler's metaprogram writes during compilation from the type-checked syntax tree. That file can be stored in source control and viewed later (01:54–02:17). Blow stresses that no compiler-author support was needed; the typed tree is exported at user level in a stable format (04:14–04:37, 07:41–08:03).

| Element | What is shown | Locator |
|---|---|---|
| Layout | Treemap; each leaf is one procedure | 02:17–02:41 |
| Size | Expression count (typed tree nodes), not lines | 02:41–03:04 |
| Colour | Blue (cold) to orange (hot), normalized within this program only; no absolute scale | 03:51–05:00 |
| Metric menu | 'if' Density, Maximum 'if' Depth, Maximum Loop Depth, Assignments, Global Reads, Global Writes, Heap Allocations, Heap Frees, Call Locality (module), Call Locality (file), Call Constancy, Blend | frames 05:12, 06:40 |
| Blend | Linear combination of all metrics with Blow's own weights | 03:04–03:51, 07:19–07:41 |
| Borders | "Modules and Files"; the standard library shows as its own region | 07:41–08:03 |
| Hover | Name, file:line, metric value and size ("if points: 46, size: 690"); in Blend, every metric | 02:24, 05:52 |
| Navigation | Zoom; click opens the procedure in Emacs | 03:04–03:51 |
| Uses shown | Uniform rectangles reveal polymorphic copies, showing what polymorphism costs in size (07:57–08:49). One unusually large procedure turned out to be repeated macro expansion (08:49–09:57). | |

**Evidence gap.** The View Mode dropdown only shows "Treemap". The Borders dropdown only shows "Modules and Files". The sidebar buttons (Settings, Info, Graphs, Allocs) are never opened. No card specifies those unseen modes.

## Jet today

| Capability | State | Evidence |
|---|---|---|
| Inspect views | Nine views: types, rights, claims, shapes, accel, decisions, structure, build, gates; none is a metric view | `crates/jet-cli/src/CLI.rs:371-437`, ratified D-CLI-ONE1 |
| Complexity | One cognitive score per function, one file at a time, from the parsed (not checked) file | `Source/CmdDevTools.rs:8545`, `crates/jet-sema/src/Sema/CognitiveComplexity.rs` |
| Allocation facts | Sema records `MemoryEvent` with kind `Allocation`, `RetainRelease`, or `ArenaBytes`, plus span | `crates/jet-sema/src/Sema/MemoryFacts.rs:30-46` |
| Dynamic dispatch | `open_dispatches` records dispatch sites with no known callee | `MemoryFacts.rs:61-72` |
| Call edges | Semantic-index call edges plus resolved references give caller and callee modules | `Source/CmdDevTools.rs:8793-8840` |
| Generic instances | `InstanceFact` covers parameterized code modules only, not generic function instantiation | `crates/jet-semindex/src/Build.rs:278-367` |
| Globals / frees | No mutable global item (only `Const`); shared mutation goes through `Shared<T>`; no manual free | `crates/jet-foundation/src/AST/items.rs:9-85`, `Examples/features/memory/shared_config.jet` |
| Visual hosts | Canvas browser IDE; devtools panel catalog (Structure, Profiler, …); `jet perf view` HTML flamegraph of runtime traces | `crates/jet-canvas`, `DevtoolsPanelCatalog.rs:8-28`, `Source/CmdPerf.rs:2437` |
| Treemap | None anywhere in `crates`, `Source`, `Core`, `Tools/editors` | case-sensitive search, 2026-09-30 |

The Rust `jet-semindex` and parts of sema are being ported to `Compiler/JetSema` (see the #3533 log). The metric card names the checked projection as its only input, so it applies wherever that projection lives when the work starts.

## Verified defect: complexity ignores nesting

Probe binary: `~/.cache/jet-test-scratch/jet-debug-snapshot27`, run through `~/.cache/jet-luna/safe-jet.sh`. The repository HEAD at the time was `39b713f09`.

```
fn two_loops(n: Int) -> Int   // loop inside loop
fn nested_if(n: Int) -> Int   // if inside if
fn if_in_loop(n: Int) -> Int  // if inside loop

$ jet lint --complexity nest.jet
nest.jet:1 fn two_loops — score 2
nest.jet:11 fn nested_if — score 2
nest.jet:21 fn if_in_loop — score 2
```

The scorer's header rule (`CognitiveComplexity.rs:3-7`) is `1 + enclosing-structure-depth` for each branch, loop, or dispatch, which gives 3 for each function. The committed goldens show the same drift. `tests/ui/lint_cognitive_complexity.stderr` pins `tangled` at 4; the rule gives 7. The refactored golden pins `add_if_match` at 3; the rule gives 4, which is above that fixture's pass budget. So fixing the rule also means redesigning the pass fixture, not just updating a number. The cause is not confirmed; the likely site is the strict span containment at `CognitiveComplexity.rs:110-112` or the spans stored for loop and `if` nodes. Carded as #3866.

## Mode-by-mode mapping

| Source mode | Jet meaning | Fact source today | Classification |
|---|---|---|---|
| Size (expression count) | Checked expression nodes per function | none | real gap (#3867) |
| Macro bloat | Checked nodes minus written nodes | none | real gap (#3867) |
| 'if' density | Branch count ÷ size | cognitive walker (broken nesting) | real gap (#3866, #3867) |
| Max 'if' depth, max loop depth | Deepest branch and loop nesting | same walker | real gap (#3866, #3867) |
| Assignments | Assignment and compound-assignment count | none | real gap (#3867) |
| Heap allocations | `Allocation` memory events | `MemoryFacts` | real gap: not projected per function (#3867) |
| Global reads / writes | No direct counterpart | — | owner gate (D-CODEMAP-MEANING1) |
| Heap frees | No direct counterpart | — | owner gate (D-CODEMAP-MEANING1) |
| Call locality (module / file) | Resolved calls leaving the module or file | semantic-index call edges | real gap (#3867) |
| Call constancy | Calls through a function value or trait object | `open_dispatches` | real gap (#3867) |
| Polymorphic copies | Distinct checked type-argument sets per generic function | none | real gap (#3867) |
| Blend | Weighted sum in the viewer, weights visible | none | real gap (#3868) |
| Saved metrics file | The metrics view's `--json` output | inspect `--json` convention | owner gate (D-CODEMAP-CLI1) |
| Treemap, borders, hover, zoom, click-to-source | Map view | Canvas has source navigation | owner gate (D-CODEMAP-VIEW1), #3868 |
| User-level typed-tree access | Project-defined checkers over typed code | #1974 done; #3534 ready | ratified / in progress |

## Where Jet can beat the source

- **Live refresh.** Blow's map needs a recompile and a file reload. The Canvas option redraws on every save, because the dev loop already re-checks the program (D-CODEMAP-VIEW1 option A).
- **Race view with a real meaning.** Jet has no mutable globals, so "global writes" is structurally zero. Counting `Shared<T>` writes points at the only places Jet tasks can race (D-CODEMAP-MEANING1 option A).
- **Honest gaps.** A metric that cannot be computed is shown as unavailable with a reason, never as zero (#3867 criterion 3).
- **Runtime colour later.** `jet perf` already produces source-attributed hotness (#2997). It can become one more colour source on the same map. That is not in scope here and is not carded.

## Avoid list

- **Recolouring over absolute numbers.** Blow normalizes within one program, so colours are not comparable across programs or over time. The map must say colours are relative and always show absolute values on hover (#3868 plan step 4).
- **A second analyzer.** Blow's tool computes metrics in a separate metaprogram. In Jet, the rows must come from the one checked projection, and `jet lint --complexity` must share their nesting walker (#3867 criterion 2, I8).
- **Cost on every build.** Metrics are computed only when asked for; `jet check` does no extra work (#3867 criterion 4).

## Owner gates raised

| Decision | Card | Question | Recommended |
|---|---|---|---|
| D-CODEMAP-CLI1 | #3867 | Which command prints the metrics? | A: new `jet inspect metrics` view (per D-CLI-ONE1) |
| D-CODEMAP-MEANING1 | #3867 | What do global reads/writes and heap frees mean in Jet? | A: shared-cell reads/writes and reference-count updates, under Jet names |
| D-CODEMAP-VIEW1 | #3868 | Where does the visual map live? | A: a Code Map view in Canvas |

All three are short ballots, non-draft and open. #3867 and #3868 compute to the owner's `decide` lane. #3868 also needs owner visual acceptance.

## Tower records created

| Record | Title | State after write |
|---|---|---|
| #3866 | Cognitive-complexity score ignores nesting depth, contradicting its documented rule | ready, agent `implement` lane, 3 criteria |
| #3867 | Per-function code metrics from the checked program, in one inspect view | owner `decide` (2 ballots), blocked by #3866, 5 criteria |
| #3868 | Visual code map: every function as a rectangle, coloured by a chosen metric | owner `decide` (1 ballot), blocked by #3867, 6 criteria, needs acceptance |

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 complexity score ignores nesting; goldens pin wrong scores | card | #3866 |
| F2 no per-function metric rows for any source mode | card | #3867 |
| F3 command for the metrics (D-CODEMAP-CLI1, open) | card | #3867 |
| F4 Jet meaning for global and free modes (D-CODEMAP-MEANING1, open) | card | #3867 |
| F5 no visual code map | card | #3868 |
| F6 host for the visual map (D-CODEMAP-VIEW1, open) | card | #3868 |
| F7 user-level access to the complete typed program | card | #3534 |
| F8 View Mode, Borders, and sidebar options never shown | no-action | evidence gap: the clip never opens them; the full talk IdpD5QIVOKQ is outside the declared source |
| F9 PbVHBToltXk is not yet listed in Docs/spec/reference/prior-art.md | no-action | repository edits were not authorized in this run; the source row is left for the owner |
<!-- /audit-dispositions -->

## Local evidence

- `crates/jet-sema/src/Sema/CognitiveComplexity.rs`
- `crates/jet-sema/src/Sema/MemoryFacts.rs`
- `crates/jet-semindex/src/Types.rs`, `crates/jet-semindex/src/Build.rs`
- `crates/jet-cli/src/CLI.rs`
- `Source/CmdInspect.rs`, `Source/CmdDevTools.rs`, `Source/CmdPerf.rs`
- `tests/ui/lint_cognitive_complexity.jet`, `tests/ui/lint_cognitive_complexity_refactored.jet`
- `crates/jet-codegen/src/Prelude/Core/DevtoolsPanelCatalog.rs`

Strongest unverified assumption: the checked projection exposes enough per-node structure to count written versus checked nodes. If generated code is not distinguishable from written code, the macro-bloat row waits on generated-declaration identity (#3533).
