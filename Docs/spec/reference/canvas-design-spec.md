# Canvas node design

This document defines the visual and interaction contract for the Canvas graph:
node archetypes, type colors, pin anatomy, layout, palette behavior, and
source-backed editing. It is for Canvas UI contributors and clients that render
the graph protocol. Executable projection facts come from
[`graph_projection.rs`](../../../crates/jet-devserver/src/Canvas/graph_projection.rs),
node metadata from
[`node_catalog.rs`](../../../crates/jet-devserver/src/Canvas/node_catalog.rs),
and browser behavior from the Canvas JavaScript under
[`crates/jet-canvas/src`](../../../crates/jet-canvas/src). The wire fields and
transaction rules are in [`canvas-protocol.md`](canvas-protocol.md).

## Node archetypes

Canvas presents calls, methods, and functions as one function family. The
useful visual distinction is between value, executable, and pure-function
nodes:

1. **Value nodes** represent variables and literals.
   - A variable get is a compact pill without a header. Its 12%-opacity fill
     and 1px border use the value's type color; it has one output pin and the
     variable name as its label.
   - A literal is a compact chip with an inline editor and one output pin. It
     has no header.
   - A variable set or assignment is executable, with exec input/output pins,
     a slim dark header named for the variable, and a value input plus output
     passthrough in its body.
2. **Executable function nodes** have effects and participate in the execution
   rail. They use a steel-blue header, with exec input at top left, exec output
   at top right, and data pins below.
3. **Pure function nodes** have no exec pins. They are evaluated when a
   downstream input needs them and use a green header. Operators such as
   `+`, `-`, `*`, `/`, `==`, `!=`, `<`, and `>` are compact operator nodes with
   a centered glyph and side pins.

Control flow (branch, while/for/loop, switch, return, break, and continue) is a
supporting family. It uses a dark-charcoal header and a white glyph: `◇` for a
branch, `↻` for a loop, `⇉` for a switch, and `⏎` for return. Crimson is reserved
for entry and event surfaces, not control flow.

A function-definition entry has a crimson header, the function name as its
title, the subtitle `entry`, and one exec output.

Node identity comes from header color, glyph, and title. The default view does
not repeat `FN`, `CALL`, `SET`, `GET`, `RET`, or `IF` as header chips. Developer
mode may expose the raw kind as a small optional badge, but text must not repeat
information already conveyed by the header color.

## Type colors and wires

The JavaScript type-color map is the one source of truth for pins, wires,
variable-get tint, and type chips. The design values are:

| Type | Hex | Treatment |
|---|---|---|
| exec/control | `#f2f4f8` | White; pentagon-arrow pin shape. |
| Bool | `#c0392b` | Crimson. |
| Int / IntN | `#2ec4b6` | Teal. |
| Float / F32/F64 | `#9acd32` | Yellow-green. |
| String | `#c678dd` | Magenta-violet. |
| Char | `#e8a2c8` | Pink. |
| List `[T]` | element color | Square grid pin glyph. |
| Map | `#e5a03c` | Amber. |
| Option `T?` | base color | Hollow double-ring pin. |
| Result/fallible | `#fb7185` | Rose. |
| Struct/named | `#5b8dd9` | Steel blue. |
| Enum/variant | `#4f9e5a` | Forest. |
| Fn/lambda value | `#a78bfa` | Violet. |
| Void | `#6b7280` | Grey. |
| unknown/generic | `#8a8f98` | Grey. |

Wires inherit the source-pin color. Exec wires are 2.5px, data wires are
1.8px, and hover adds 0.7px. Use Bezier curves with horizontal tangents; do not
run straight segments through labels.

## Pin anatomy

- A data pin is an 11px circle with a 1.8px stroke in the type color. It is
  hollow when unconnected and filled when connected, including exec pins.
- An exec pin is a right-pointing pentagon arrow (`▷` outline or `▶` filled)
  in white.
- Pin labels use 11px UI sans at 70% white. Input labels are left-aligned
  after the pin; output labels are right-aligned before the pin.
- An unconnected input with an editable type gets an inline default editor:
  Bool checkbox, small Int/Float number field, small String field, or enum
  dropdown. The dark inset editor is at most 96px wide.
- Pin rows contain no decorative rail lines.

## Node chrome and graph field

Node bodies use `#1d2129` at 96% opacity, a 1px `#0b0d11` border, 8px
corners, and a `0 4px 12px rgba(0,0,0,.45)` shadow. Headers are 26px high with
a left-to-right archetype-color gradient (color to approximately 20% darker).
Titles use 13px semibold UI sans in white; optional subtitles use 10px at 65%
white for a module path such as `Core.List` or for `entry`. A 14px glyph slot
precedes the title; functions use `ƒ` and values use their type glyph.

Pin rows are 24px high with 8px vertical body padding. Nodes have a 150px
minimum width and grow to fit the title, widest pin pair, and editors. Selection
uses a 1.5px `#f5a623` outline with a soft glow; a multi-select marquee is
dashed amber. Hover lightens the border to `#3a4250`. Connected pins may emit a
subtle 25%-alpha 4px glow in their type color; this is the intentional dataflow
highlight.

The graph field uses `#101318`, a 16px minor grid in `#161a21`, and a 128px
major grid in `#20262f`. The zoom label is bottom-left, and the minimap uses
node-archetype colors.

## Palette and insertion

Right-clicking empty canvas opens a searchable context menu with a focused fuzzy
search field and the categories **Flow**, **Variables**, **Project**, and
**Core**. Project contains functions from the project; Core is the complete
read-only catalog returned by the `core_catalog` query.

Dragging from a pin to empty canvas opens the same menu filtered to type-
compatible nodes. The selected node is placed at the release point and
auto-wired to the source pin. Rows show a name and type-colored signature
summary; pure functions use the green `ƒ`, executable functions the blue `ƒ`.
Placing a Core or Project function creates a real function node through the
source transaction path.

## Component tree

The left `My Canvas` rail is one source-backed tree over the project:

- `Files` selects a source file and keeps its file revision visible.
- `Functions` selects a projected function graph; `New` uses the checked
  function transaction.
- `Callback` creates an ordinary handler such as `fn on_start() -> Void { }`
  through that same transaction. A projected `on_*` function is marked as a
  framework-callback view; selecting its `handler` row opens its source-backed
  graph.
- `Variables` lists parameters and local bindings for the open function with
  their types; `Add` promotes a selected expression through the checked binding
  transaction.

Navigation changes the selected projection only. It never creates a parallel
graph or variable model. Every item carries the current source ID and revision
and is re-rendered after a source transaction. Callback labels and handler
navigation come from `event_views`; they do not create an event sidecar or
alter saved Jet text.

## Typography

Use the UI sans stack `Inter, "Segoe UI", Roboto, system-ui, sans-serif` for
node titles, pin labels, and menus. Reserve the monospace stack
`JetBrains Mono, ui-monospace` for type names, signatures, and source text.

## Quality floor

- The default projection has no overlapping nodes; Tidy respects measured node
  sizes.
- An entry node never renders an empty body band.
- Hit targets are at least 12px. Use `grab`/`grabbing` on nodes and a
  crosshair on pins.
- Pan and zoom target 60fps on the demo graph; batch canvas draws.
- Delete removes the selection, arrow keys nudge it, and `F` fits the selection.
- Respect `prefers-reduced-motion` for exec pulses and glows.

## Interaction

Node placement is free. A drop position is final; automatic layout runs only
when no saved position exists or when the user invokes Tidy. Positions persist
as editor view state and take precedence over automatic layout.

Dragging follows the pointer at 60fps without default grid snap. Any pin can
start a typed-wire preview. A compatible drop connects; an empty-canvas drop
opens the fuzzy, type-filtered menu at the release point, and `Esc` cancels.
Dragging a wire endpoint re-enters wire mode. Compatible drops retarget through
a source transaction; empty drops use the same filtered menu and `Esc` restores
the prior state. One output may fan out to many inputs, while a binding has one
projected getter per graph. Marquee, additive selection, group movement, copy,
paste, and duplicate use source transactions; pasted groups offset by 24px.

## Source-backed editing

- Execution outputs use `role:"loop_body"`, `role:"loop_done"`, and
  `role:"early_return"` where applicable. A second compatible execution drop
  opens a no-write convergence preview. Extraction selects the exact-body
  helper when one exists; stale, ill-typed, or out-of-scope previews leave the
  source and preview unchanged.
- Disabled and debug-only states use the source-backed `D-CANVASSTATE1`
  contract. Canvas does not invent another source encoding.
- Palette nodes without a wire target are view-state-only staged nodes. They
  show a dashed unsaved ring, do not become dead source code, and materialize
  only when a compatible wire reaches the graph. Deleting one writes nothing.
- Pattern-test branches show one execution output per arm plus `else`; adding
  an arm opens a source-backed pattern editor. Comment groups are resizable,
  titled, movable view-state objects with no source representation.
- The `My Canvas` tree projects files, functions, and variables from source
  revisions; it never creates a parallel graph or variable model. Developer-
  only trust and hash details stay out of the beginner view, while package,
  dependency, and diagnostic facts remain in one quiet detail group.
- Details controls exist only when a live source transaction exists. Typed
  controls edit scalar, collection, tuple, and struct values through
  descriptors that retain the source expression and span. Invalid, incomplete,
  ambiguous, or stale input writes zero source bytes and shows the normal
  Canvas diagnostic.
- User-facing terms are Functions, Variables, Inputs, Outputs, and Execution.
  Raw kind strings and protocol jargon stay hidden.
- The toolbar groups view, lens, edit, run, and search/developer controls in
  one non-wrapping row. Debug controls appear only while debugging. Node width
  measures headers, pins, editors, and badges before rendering; control nodes
  have no subtitle, function nodes show only their module path, and entry nodes
  use the crimson header without a subtitle.
