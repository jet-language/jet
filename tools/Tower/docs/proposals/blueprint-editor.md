# Blueprint-Class Visual Editor for Jet

> Proposal for owner review (card #182, 2026-07-02). Digestible report, not a
> plan. Companion: `ue-to-jet.md` (UE design lessons; its §3 covers the
> language-side type-directed authoring this editor sits on).
>
> Durable implementation plan: [`../plans/epoch-7/blueprint-editor.md`](../plans/epoch-7/blueprint-editor.md).

The pitch: a fully functional node-graph editor for Jet where the graph and
the text are the same program. Epic is removing Blueprint from UE6. Millions
of people learned to program in Blueprint and are about to lose it; no
general-purpose language has ever shipped a first-party visual surface that
isn't a toy. Jet is unusually positioned to be the first, because the
language decisions already ratified (expected-type elaboration, semantic
index, formatter stability, effect rows) are exactly the infrastructure a
lossless graph projection needs.

---

## Glossary (Blueprint terms used below)

| Term | Meaning |
|---|---|
| Node | A unit on the canvas: function call, control-flow construct, literal |
| Pin | A typed connection point on a node; inputs left, outputs right |
| Data wire | Connects an output pin to an input pin; carries a value |
| Exec wire | White wire carrying control flow (do this, then that) |
| Drag-off | Dragging from a pin into empty canvas — shows only nodes that fit that pin's type |
| Collapse | Folding a subgraph into a single named node |
| Latent node | A node whose execution spans time (delay, network call) |

Blueprint's core insight — the reason non-programmers can use it — is not
that it's visual. It's that **pins are typed**: connecting a `String` output
to an `Int` input is structurally impossible, and dragging off a pin shows
only what fits. Authoring errors become impossible actions instead of error
messages.

---

## Principles (the non-negotiables)

1. **Text is the single source of truth.** A `.jet` file is the program. The
   graph is a *projection* computed from the checked AST + semantic index —
   like the formatter's view, not a parallel format. No `.uasset` binary, no
   two dialects. Everything stays git-able and reviewable as text.
2. **Lossless round-trip.** Open any Jet file as a graph; edit; the written
   text is what `jet fmt` would emit, author intent preserved (same
   STABILITY bar as the formatter — this is a formatter client, not a code
   generator). Text edited in Cursor re-projects live.
3. **Full fidelity — no drop-to-code cliff.** Every language feature has a
   graph representation. Blueprint died in production because complex logic
   forced "rewrite it in C++"; the moment the editor says "this part you
   must edit as text," the product is a toy. Fidelity strategy below.
4. **The type system drives authoring.** Drag-off = expected-type query
   against the semantic index (D-SEMINDEX1). The graph editor and the LSP
   completion engine are the same query with two renderers.

Both facets served (philosophy): beginners get the magic Blueprint-style
surface; experts get the same graph as a *reading* tool over dense code —
dataflow visualization, effect badges, blast-radius coloring (D-IMPACT1).

---

## Projection model

One function = one graph. The module view (functions, types, imports) is the
dossier/outline surface (D-DOSSIER1), not a wire canvas.

| Jet construct | Graph form |
|---|---|
| `fn` signature | Entry node: one output pin per param; return type = exit node input pin |
| Call `f(a, b)` | Node `f`, input pins `a b`, output pin = return type |
| Method chain `x.lines().map(f)` | Left-to-right node chain on data wires |
| `:=` binding | Named reroute node (a labeled wire junction) |
| `if` / `if x == { }` | Branch node: one exec-in, one exec-out per arm; match arms show patterns on the pins |
| `for` loop | Loop node with body subgraph, item output pin |
| Closure / lambda | Collapsed subgraph node; expand in place |
| Literal, `.{ }` construction | Inline value editor on the input pin (no separate node unless shared) |
| Effect row `#(Net, Fs)` | Badges on the node header; function's own row on the entry node |
| `?` propagation | Error exec-wire exiting the node sideways (the try rail) |
| `#Test` / scope members (D-DOTSCOPE1) | Test graph; `.expect_fail { }` renders as a tinted region |
| `#Unsafe("…")` | Red-bordered region with the audit string as the region title |
| Generics | Pin templates; instantiation shown when wired (Blueprint wildcard pins, but sound) |
| Comments | Sticky notes anchored to nodes; doc comments on the entry node |

Worked example — the projection both ways:

```jet
fn top_scores(url: String) -> [Int] #(Net) {
    body := http.get(url)?
    body.lines().map(parse_score).take(10)
}
```

```
┌─ top_scores ────────── #(Net) ─┐
│ url: String ○──┐               │
└────────────────┼───────────────┘
                 │
        ┌─ http.get ─ #(Net) ─┐   error ▷──── (try rail → caller)
        │ url ○      ○ body   │
        └────────────┼────────┘
                     │
   ┌─ .lines ─┐  ┌─ .map ──────────┐  ┌─ .take ─┐  ┌─ return ─┐
   │ ○      ○─┼──│ ○ parse_score ○─┼──│ ○  10 ○─┼──│ ○ [Int]  │
   └──────────┘  └─────────────────┘  └─────────┘  └──────────┘
```

Dragging off `body`'s output pin (type `String`) offers `.lines()`,
`.split(…)`, `parse_score(…)` — the D-SEMINDEX1 expected-type query. Wiring
`body` into `.take`'s `Int` pin is impossible; the pin doesn't highlight.

Graph edits write text through the formatter: adding a `.filter(…)` node
between `.map` and `.take` produces exactly the diff a human would have
typed. `git diff` stays the review surface.

## Fidelity strategy (the hard 20%)

Expression-dense code (arithmetic, one-line lambdas) does not want one node
per `+`. The projection is **altitude-aware**: any pure subexpression can
render as an inline expression field on a pin (editable as text, still
type-checked live), and any subgraph can collapse to a named node. So "no
cliff" means: the *structural* skeleton (control flow, calls, bindings,
effects, error paths) is always graph; leaf expressions are inline text
*inside* the graph, expandable to full nodes on demand. Both renderings are
projections of the same AST — there is no boundary to fall off, only a zoom
level. This is the piece Blueprint never had (its math was 40-node spaghetti
or an opaque C++ node), and it is what makes full fidelity honest instead of
aspirational.

## Live execution (the demo that sells it)

The graph is also the debug surface, reusing shipped/ratified machinery:

- **Values on wires**: run under the interpreter/JIT dev tiers; each wire
  shows the last value that crossed it (D-TIMETRAVEL1 value history gives
  scrubbing through past values).
- **Live editing**: `jet dev` hot-swap (D-HOTSWAP1) — rewire while it runs;
  type-stable edits swap in place.
- **Breakpoints on nodes**: `jet debug` (DAP) drives node highlighting;
  stepping = watching execution walk the exec wires.
- **Impact preview**: touching a node tints every downstream caller
  (D-IMPACT1 blast radius).

## Architecture sketch

```
.jet text ──parse/sema──▶ checked AST + semindex ──projection──▶ graph JSON
    ▲                                                            │
    └────────── formatter (STABILITY law) ◀──── graph edit ops ◀─┘
```

- **Projection service**: new query surface on the existing LSP server
  (`jet lsp` already hosts features; semindex is ratified as its data
  layer). Speaks a graph document + edit-op protocol to any client.
- **Canvas client**: one renderer, embeddable in the places users already
  are (Cursor/VS Code webview, Zed, browser via `jet dev`) — host question
  balloted below, not guessed.
- **Edit ops are AST transactions**: "insert call node," "rewire pin,"
  "extract subgraph to fn" — each maps to a codemod (D-CODEMOD1) and emits
  text through the formatter. No textual patching from the canvas.
- **Layout**: recommend fully deterministic auto-layout derived from code
  structure (dataflow left-to-right, exec top-to-bottom). No node-position
  sidecar file → nothing extra in git, no layout merge conflicts, identical
  view for every reader. Manual-position lovers are a ballot option, not the
  default.

## Phasing (each phase independently shippable)

1. **Reader**: any function renders as a live graph (LSP webview command +
   `jet dev` browser panel). Read-only; values-on-wires in dev mode. This
   alone is a differentiating docs/teaching/review tool.
2. **Wirer**: structural edits — rewire, insert node from drag-off, inline
   value edits, collapse/extract. Round-trip through formatter; text and
   canvas open side-by-side stay in sync.
3. **Author**: full creation flow — new fn/type/test from the canvas, the
   D-DOTSCOPE1 `.` menu as a canvas gesture, debugger integration, refactors
   as graph gestures.
4. **Cockpit**: multiplayer-grade polish — the "this replaces Blueprint"
   release aimed at the UE6 gap; marketing surface for the language itself.

## Prior art (what to steal, what to avoid)

- **Blueprint**: steal typed pins, drag-off, collapse, live debug wires.
  Avoid: binary assets (unreviewable diffs), interpreter-only tier (perf
  cliff → C++ rewrite culture), no expression altitude (math spaghetti).
- **Scratch**: steal the impossible-to-misassemble feel; avoid the toy
  ceiling (no types, no real programs).
- **LabVIEW**: proof pros will ship in dataflow graphs for decades; avoid
  its proprietary opaque format lock-in.
- **Enso / Luna**: closest prior attempt at graph⇄text isomorphism; validates
  the projection idea, died on a niche language nobody had other reasons to
  use. Jet inverts that: the language is the draw, the canvas is leverage.
- **Simulink / Houdini / Max**: domain graphs with strong live-data UX —
  steal values-on-wires.

## Sequencing & dependencies

Runs on: D-SEMINDEX1 (ratified) as data layer; formatter STABILITY law
(shipped) as write path; D-CODEMOD1 / D-DOSSIER1 / D-IMPACT1 (ratified,
gated on semindex) as edit/nav services; dev tiers + D-HOTSWAP1 +
D-TIMETRAVEL1 for live mode. Epoch placement: e6 alongside the tooling
stack; Phase-1 Reader is buildable as soon as semindex + LSP webview exist.
Feeds and is fed by `ue-to-jet.md` §3 (type-directed authoring — same
queries, text renderer).

## Open decisions (future ballot rows — nothing here is decided)

| ID | Question | Notes |
|---|---|---|
| D-BPE-NAME1 | Product name | Menu below |
| D-BPE-HOST1 | Where the canvas lives first: editor webview (Cursor/VS Code), `jet dev` browser panel, standalone app, Zed pane | One first-class host first; protocol keeps the rest open |
| D-BPE-LAYOUT1 | Deterministic auto-layout only vs optional manual positions (and if manual: where positions persist) | Recommendation: auto-only; positions are the one thing that would reintroduce a non-text artifact |
| D-BPE-ALTITUDE1 | Default expression altitude: which subexpressions start inline vs as nodes | Pure UX, but owner-facing defaults |
| D-BPE-TAXONOMY1 | Node visual language: shapes/colors per construct class, effect badge design, error-rail styling | Decide from mockups, not prose |
| D-BPE-EDITSCOPE1 | Phase-2/3 edit-op vocabulary: which refactors are canvas gestures v1 | Bounded by D-CODEMOD1 |

**Naming menu (D-BPE-NAME1)** — jet/aviation, original:

| Candidate | Read |
|---|---|
| **Canopy** | the glass you see the whole flight through; over the code, not instead of it |
| **Flightdeck** | where the whole aircraft is visible and operable |
| **HUD** | heads-up display: data projected over the real thing — exactly the projection model |
| **Contrail** | the visible line a jet draws; wires as vapor trails |
| **Slipstream** | flow you can ride; dataflow canvas |
| **Vector** | direction + magnitude; also the drawing sense |
| **Glasswork** | glass cockpit + craft |
| **Skein** | threads/wires; the term for geese in flight formation |

---

*Verdict sought from owner: (a) green-light Phase 1 (Reader) as an e6 card
with a real plan, (b) reactions to the six ballot rows so they can be minted
as decisions, (c) a name shortlist.*
