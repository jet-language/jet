# Blueprint-class visual editor for Jet (#182)

**Status:** durable plan. Proposal pitch lives at
[`../../proposals/blueprint-editor.md`](../../proposals/blueprint-editor.md).
This file is the implementation plan and ballot inventory.

**Build only after:** stable `D-SEMINDEX1` facts, formatter stability for every
projected construct, edit transactions/codemods (`D-CODEMOD1`), and the
ratified surface constraints below.

## Ratified Surface

- `D-BPE-NAME1=A`: product name is **Canopy**.
- `D-BPE-HOST1=B`: first host is a `jet dev` browser panel.
- `D-BPE-LAYOUT1=A`: v1 uses deterministic layout only.
- `D-BPE-ALTITUDE1=A`: structural nodes, pure leaves inline.
- `D-BPE-TAXONOMY1=A`: restrained semantic badges, typed pins, distinct rails.
- `D-BPE-EDITSCOPE1=A`: v1 write scope is structural essentials.
- `D-BPE-PROTOCOL1=C`: protocol is internal for Reader, public before write
  flows.

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

**Canvas clients.** `jet dev` browser panel is first. Editor webview,
standalone Studio, and future Zed pane can consume the same protocol later.

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
- `layout_hints`: deterministic, derivable hints only

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

Exit: an internal projection test helper emits stable JSON for fixture programs;
protocol remains internal until Reader proves it.

### BPE1 — Reader UI

Render one function graph read-only in the `jet dev` browser panel. Dragging,
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

Implement the structural essentials: insert call from drag-off, rewire pin,
inline literal edit, add fallback rail, collapse/extract to function, rename
binding, and create test. Every edit round-trips through formatter and produces
an ordinary diff. Public protocol/schema is required before this write flow.

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

## Ratified Implementation Constraints

Canopy starts as a `jet dev` browser panel because that host gives demos,
teaching, and local iteration without requiring an editor extension. Layout is
deterministic from source. The default graph expands structural code and keeps
pure arithmetic/comparison leaves inline. Nodes use one restrained shape family
with typed pins, semantic badges, and distinct control/error/proof rails.

The Reader protocol may remain internal while the projection proves out. Before
write-capable flows ship, the graph document and edit transaction schema become
public, versioned, and snapshot-tested.
