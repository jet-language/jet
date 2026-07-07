# Blueprint-class visual editor for Jet (#182)

**Status:** durable plan. Proposal pitch lives at
[`../../proposals/blueprint-editor.md`](../../proposals/blueprint-editor.md).
This file is the implementation plan and ballot inventory.

**Build only after:** stable `D-SEMINDEX1` facts, formatter stability for every
projected construct, edit transactions/codemods (`D-CODEMOD1`), and the owner
ballots at the end of this file.

## Goal

Open any Jet function as a typed node graph, edit the graph, and write normal
Jet source back through `jet fmt`. Text is the program. The graph is a checked
projection with no private dialect, binary asset, or semantic sidecar.

Beginner path: typed pins make wrong connections impossible, drag-off menus show
only fitting operations, and errors are prevented before diagnostics are needed.
Expert path: the same graph exposes exact types, effects, ownership, fallible
rails, generated facts, call impact, and debug/proof overlays.

Hybrid path: one source model, multiple renderings. Inline expression fields and
expanded node graphs are two altitudes over the same AST, not two mechanisms.

## Target architecture

```
.jet text
  -> lexer/parser/sema
  -> checked AST + semindex facts
  -> projection service
  -> graph document JSON
  -> canvas client
  -> edit transaction
  -> codemod + formatter
  -> .jet text
```

**Projection service.** Runs inside the compiler service/LSP process and exposes
read-only graph documents plus write-capable edit transactions. It does not
type-check independently.

**Canvas clients.** Editor webview, `jet dev` panel, standalone Studio, and
future Zed pane all consume the same protocol. First host is owner-gated.

**Edit transactions.** User actions are named source operations: insert call,
rewire argument, extract subgraph, inline node, add test region, wrap in
`#Unsafe`, add fallback rail. Each transaction either produces a formatted text
edit or fails with a Jet diagnostic.

**Layout.** Default layout is deterministic from source structure: data flows
left to right, control flows top to bottom, nested regions become framed groups.
Manual layout persistence is owner-gated because it can introduce non-source
state into review.

## Data model

`GraphDocument`:

- `schema_version`
- `source_id`, `revision`, `fmt_fingerprint`
- `graphs`: one `FunctionGraph` per function, test block, lambda, or generated
  body lens
- `diagnostics`: Jet diagnostics already emitted by parser/sema
- `facts`: semindex fact handles, not duplicated fact payloads

`FunctionGraph`:

- `graph_id`: stable semantic path plus source span
- `entry_node`, `exit_nodes`
- `nodes`, `pins`, `wires`, `regions`
- `inline_exprs`: AST subtrees rendered as editable expression text
- `layout_hints`: deterministic, derivable hints only unless a future ballot
  ratifies persistent manual positions

`Node`:

- `node_id`: stable from semantic path + AST role + ordinal in formatted source
- `kind`: call, method, binding, branch, dispatch, loop, closure, literal,
  construct, return, fallible, marker region, comment
- `source_span`
- `type_facts`, `effect_facts`, `ownership_facts`
- `edit_affordances`: transaction names valid at this node

`Pin`:

- direction, type, capability, optionality/fallibility, effect grant need
- expected-type query key for drag-off completion

`Wire`:

- data, control, fallible rail, ownership move, or proof/debug overlay

Comments and doc comments are nodes or annotations anchored by source span.
Unknown or unsupported constructs still project as source-backed nodes; the
canvas may reduce edit affordances but must not hide source.

## Synchronization rules

- Source edit wins. On every text revision, the graph is re-projected from the
  checked program.
- Graph edit is a transaction against the current source revision. If the text
  changed since the graph was drawn, the transaction rebases by semantic path or
  fails with a conflict diagnostic naming the stale node.
- Formatter is the only write path. The graph never writes ad hoc text.
- Round-trip proof: text -> graph -> no-op graph write -> text must be byte-stable
  after `jet fmt`.
- Comments stay source-owned. Sticky notes are comments unless a future ballot
  ratifies non-source notes.
- Inline expression fields are live Jet expressions. They parse and check through
  the normal front end before the graph accepts them.
- Generated code is read-only unless the generating source site owns an edit
  transaction. R11 still holds: generated fragments re-enter the front end.

## Implementation slices

### BPE0 — schema and read-only projection

Define graph JSON schema, projection query, and golden projection fixtures for
functions, calls, bindings, branches, loops, lambdas, `?`, `?? return`,
`#Test`, `#Unsafe`, effects, comments, generics, and method chains.

Exit: an internal projection test helper emits stable JSON for fixture programs.
No public CLI until `D-BPE-PROTOCOL1` is decided.

### BPE1 — Reader UI

Render one function graph read-only in the first ratified host. Dragging,
zooming, search, node focus, and source-span jump work. Pin hover shows type,
capability, effect, and source snippet.

Exit: UI snapshot tests cover desktop and narrow viewport; canvas is nonblank;
all nodes link back to source spans.

### BPE2 — dev/debug overlays

Attach `jet dev`, `jet debug`, and later `jet prove` facts as overlays:
last value on wire, active execution node, fallible rail taken, effect/proof
status. Overlay absence is explicit, not silent.

Exit: interpreter-run fixture shows wire values; unsupported native feature
shows the existing Jet unsupported diagnostic, not raw backend output.

### BPE3 — structural editing

Implement insert call from drag-off, rewire pin, inline literal edit, add
fallback rail, collapse/extract to function, and rename binding. Every edit
round-trips through formatter and produces an ordinary diff.

Exit: paired text/graph tests prove side-by-side editing stays synchronized.

### BPE4 — authoring flows

Create new function, type, test, `#Test` scope member, lambda body, and marker
region from canvas gestures. D-DOTSCOPE1 member menus use the same vocabulary as
LSP completion.

Exit: new-code flows produce source that passes parser, sema, formatter
stability, and focused golden tests.

### BPE5 — production cockpit

Add impact coloring, proof badges, profiling/budget facts, multiplayer-safe
source conflict handling, and polished Blueprint-class interactions.

Exit: flagship example demonstrates a non-trivial Jet program edited in graph
and text with identical build/test/proof outcomes.

## Test plan

- Projection golden JSON for every AST family and marker family.
- No-op round-trip byte stability after `jet fmt`.
- Transaction tests: insert, rewire, extract, inline, rename, fallback rail.
- Conflict tests: stale graph revision, deleted node, moved source span.
- LSP/schema version tests with unknown-field forward compatibility.
- Visual tests for first host: screenshot, canvas-pixel nonblank, keyboard
  navigation, drag-off menu filtering.
- Dev/debug overlay tests over interpreter fixtures.
- Invariant checks: no second parser/checker, no generated semantic sidecar, no
  graph-owned source truth.

## Ballots to queue

### D-BPE-NAME1

**group:** tooling

**gist:** Choose the product name for Jet's visual code editor.

**story:** Maya teaches game scripting to artists. She wants to say "open this
in ___" and have the name feel like a first-party Jet surface, not a plugin.

**inWild:**

```text
jet <name> open src/game.jet
```

**options:**

- A / Canopy: The graph sits over the code like glass over the cockpit. Strong
  "projection over source" meaning.
  `jet canopy src/game.jet`
- B / Flightdeck: Emphasizes full operational control. Strong for expert
  cockpit, heavier for beginners.
  `jet flightdeck src/game.jet`
- C / HUD: Short and literal: information projected over the real program.
  Risks sounding like an overlay, not an editor.
  `jet hud src/game.jet`
- D / Blueprint: Familiar to target users but owned by Unreal and semantically
  misleading because Jet is source-first.
  `jet blueprint src/game.jet`

**comparisons:** Unreal uses Blueprint for binary graph assets; Jet should avoid
that ownership model. Xcode Instruments and VS Code names are tool nouns, not
language syntax.

**rec:** A. "Canopy" best communicates source-backed visibility without copying
Blueprint's asset model.

### D-BPE-HOST1

**group:** tooling

**gist:** Choose the first host for the visual editor.

**story:** Devon is reviewing a Jet PR. They want to open one function as a
graph from the editor they already use, then jump back to the diff.

**inWild:**

```text
Open command palette -> Jet: Open Function Graph
```

**options:**

- A / Editor webview first: VS Code/Cursor extension over LSP protocol.
  Best where code review and source editing already happen.
- B / `jet dev` browser panel first: no editor dependency, good demos and
  teaching, weaker daily-edit fit.
- C / Standalone app first: best canvas control, highest risk of feeling like a
  separate product.
- D / Zed pane first: aligned with a modern editor, narrower audience.

**comparisons:** Blueprint succeeds as an integrated editor surface; Enso's
standalone experience made adoption depend on switching tools.

**rec:** A. Start where source lives; keep the protocol host-neutral.

### D-BPE-LAYOUT1

**group:** tooling

**gist:** Decide whether graph node positions are derived or saved.

**story:** Priya reviews a refactor. She wants a stable graph view without
committing noisy layout changes.

**inWild:**

```text
git diff
# only .jet source changed; no graph-position file appears
```

**options:**

- A / deterministic layout only: positions derive from formatted source and
  semantic structure. No layout merge conflicts.
- B / generated local cache: manual positions saved under `.jet/`, never
  committed, resettable.
- C / committed sidecar: manual positions in a reviewable file beside source.
  Better hand-arranged diagrams, but introduces a second artifact.

**comparisons:** Blueprint stores layout in opaque assets; code review suffers.
Graphviz derives layout; review stays text-first.

**rec:** A for v1. Add B later only for local comfort if users demand it.

### D-BPE-ALTITUDE1

**group:** tooling

**gist:** Choose which expressions start inline versus expanded as nodes.

**story:** Nora opens math-heavy gameplay code. She needs the flow readable
without turning every `+` and comparison into node clutter.

**inWild:**

```jet
scene.on_frame(frame => {
    player.velocity = player.velocity + gravity * frame.dt
})
```

**options:**

- A / structural nodes, pure leaves inline: calls, bindings, branches, loops,
  effects, and fallible paths are nodes; pure arithmetic/comparisons stay inline
  unless expanded.
- B / all expressions as nodes: maximum visual uniformity; math becomes noisy.
- C / all expression bodies inline until manually expanded: compact, but hides
  important call/effect structure from beginners.

**comparisons:** Blueprint's math graphs become spaghetti; spreadsheets keep
small expressions inline and expand structure around them.

**rec:** A. It preserves full fidelity while making the default graph readable.

### D-BPE-TAXONOMY1

**group:** tooling

**gist:** Choose the visual vocabulary for node kinds and rails.

**story:** Sam scans a graph and needs to distinguish data, control, error,
unsafe, effectful, and proof-failed regions before reading labels.

**inWild:**

```text
http.get #(Net) node: effect badge
body? rail: fallible side exit
#Unsafe region: audit border + reason
```

**options:**

- A / restrained semantic badges: one node shape, typed pins, small badges for
  effects/capabilities, distinct rails for control/error/proof.
- B / many shapes/colors by construct: fast visual scanning, higher learning
  load and accessibility risk.
- C / text-first monochrome: easiest to theme, weaker at impossible-action UX.

**comparisons:** Blueprint uses strong color categories; accessibility and
large-graph noise are common pain points. Modern IDEs favor semantic badges.

**rec:** A. It keeps the beginner surface learnable and leaves expert facts
visible without color-only meaning.

### D-BPE-EDITSCOPE1

**group:** tooling

**gist:** Choose the v1 write-capable graph edit vocabulary.

**story:** Lee wants to patch a bug from the graph without learning every
refactor gesture on day one.

**inWild:**

```text
drag from String pin -> choose .lines()
select nodes -> Extract Function
click fallible rail -> Add ?? return
```

**options:**

- A / structural essentials: insert call, rewire, edit inline expr, add
  fallback rail, extract/collapse, rename binding, create test.
- B / read-only plus inline edits only: safest first write path, too weak to
  prove the product.
- C / full refactor suite v1: powerful, but makes first release harder to
  explain and test as a coherent contract.

**comparisons:** IDE refactors succeed when each operation maps to a clear
source edit. Visual tools fail when gestures are magic transformations.

**rec:** A. It is enough to author real code while staying bounded and testable.

### D-BPE-PROTOCOL1

**group:** tooling

**gist:** Decide whether the graph protocol is public in v1.

**story:** Imani builds a review bot that wants to render graph diffs in CI.
They need a stable schema or a clear "internal only" boundary.

**inWild:**

```text
jet graph src/payments.jet --json > graph.json
```

**options:**

- A / internal LSP protocol first: faster iteration; no compatibility promise.
- B / public `jet graph --json` schema v1: CLI and tools can consume it; schema
  versioning required immediately.
- C / public only after Reader ships: internal during first UI, then stabilize
  before write flows.

**comparisons:** LSP stabilized editor integration by making protocol explicit;
early unstable compiler JSON often becomes accidental API.

**rec:** C. Keep Reader moving, then freeze before external write tooling grows.
