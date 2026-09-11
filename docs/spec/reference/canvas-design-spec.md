# Canvas Node Design Spec — Blueprint-grade polish

Canvas is Jet's visual scripting surface (UE5 Blueprint equivalent). This spec
defines the node visual system. It is the contract for the rendering overhaul.
Bar: a screenshot of Canvas must read as polished as a UE5 Blueprint graph.

Vocabulary: [Jet vocabulary](../../spec/vocabulary.md).

## Node taxonomy — three primary archetypes (owner directive)

There is NO user-visible distinction between "call", "method", and "function".
They are all **functions**. The distinctions that matter:

1. **Value nodes** — variables and literals.
   - *Variable get*: compact pill (capsule), no header, tinted by the value's
     type color (12% opacity fill + 1px type-color border), single output pin.
     Label = variable name only.
   - *Literal/constant*: compact chip with inline editor (see pin editors) and
     one output pin. No header.
   - *Variable set / assign*: executable node (exec in/out), slim dark header
     with the variable name; body row = value input pin + output passthrough.
2. **Executable function nodes** — functions with effects; participate in the
   execution rail. Steel-blue header. Exec pins: input top-left, output
   top-right. Data pins below.
3. **Pure function nodes** — no exec pins; evaluated on demand when a
   downstream input needs them. Green header. Operators (`+ - * / == != < >`
   etc.) render as **compact operator nodes**: no header, large centered glyph,
   pins on the sides.

Control flow (if/branch, while/for/loop, switch, return, break/continue) is a
supporting family: dark charcoal header with a white glyph (◇ branch, ↻ loop,
⇉ switch, ⏎ return). Never red — red is reserved for entry/event.

Entry node (function definition entry): crimson header, title = function name,
subtitle `entry`. Single exec output.

## Kill the chips

Delete the FN / CALL / SET / GET / RET / IF header chips. Node identity is
carried by header color + glyph + title. Developer mode may show the raw kind
as a small badge, hidden by default. No text on a node may duplicate what the
header color already says.

## Type → color (pins, wires, get-pill tint, type chips)

One map, one source of truth in JS; every colored element derives from it.

| Type            | Hex       | Note |
|-----------------|-----------|------|
| exec/control    | `#f2f4f8` | white; pentagon-arrow pin shape |
| Bool            | `#c0392b` | crimson |
| Int / IntN      | `#2ec4b6` | teal |
| Float / F32/F64 | `#9acd32` | yellow-green |
| String          | `#c678dd` | magenta-violet |
| Char            | `#e8a2c8` | pink |
| List `[T]`      | element color, square "grid" pin glyph |
| Map             | `#e5a03c` | amber |
| Option `T?`     | base color, hollow double-ring pin |
| Result/fallible | `#fb7185` | rose |
| Struct/named    | `#5b8dd9` | steel blue |
| Enum/variant    | `#4f9e5a` | forest |
| Fn/lambda value | `#a78bfa` | violet |
| Void            | `#6b7280` | grey |
| unknown/generic | `#8a8f98` | grey |

Wires inherit the source pin color. Exec wires 2.5px, data wires 1.8px,
hover +0.7px. Bezier with horizontal tangents (BP-style), never straight
segments through labels.

## Pin anatomy

- Data pin: 11px circle, 1.8px stroke in type color. **Hollow when
  unconnected, filled when connected** (BP rule, both pins and exec).
- Exec pin: pentagon arrow pointing right (▷ outline / ▶ filled), white.
- Label: 11px UI sans, 70% white, inside the node beside the pin. Input labels
  left-aligned after the pin; output labels right-aligned before the pin.
- Unconnected input pins of editable types show an **inline default editor**:
  Bool checkbox, Int/Float small number field, String small text field, enum
  dropdown. Editor width ≤ 96px, dark inset field.
- No decorative rail lines through the pin rows.

## Node chrome

- Body: `#1d2129` at 96% opacity, 1px border `#0b0d11`, corner radius 8px,
  drop shadow 0 4px 12px rgba(0,0,0,.45).
- Header: 26px tall, archetype color as a left-to-right gradient (color →
  ~20% darker), title 13px semibold UI sans white, optional subtitle 10px
  65% white (module path e.g. `Core.List`, or `entry`). Small glyph slot
  (14px) before the title — ƒ for functions, type glyphs for values.
- Pin rows: 24px height, 8px vertical padding top/bottom of body.
- Min width 150px; width fits content (title + widest pin pair + editors).
- Selection: 1.5px amber `#f5a623` outline + soft outer glow; multi-select
  marquee dashed amber.
- Hover: border lightens to `#3a4250`.
- **Signature detail**: connected pins emit a 4px soft glow in their type
  color — live dataflow readable at a glance. Subtle (25% alpha), the one
  intentional flourish; everything else stays quiet.

## Graph field

Background `#101318`; minor grid 16px `#161a21`, major grid 128px `#20262f`.
Zoom label sits at bottom-left. The minimap uses node archetype colors.

## Palette / node insertion (Blueprint context menu parity)

- Right-click on empty canvas → searchable context menu: fuzzy search field
  focused on open, tree of categories: **Flow**, **Variables**, **Project**
  (every function in the user's project), **Core** (the full core-library
  catalog from the `core_catalog` query — every module, every function).
- Drag from a pin and release on empty canvas → same menu, **filtered to
  type-compatible nodes**, and the chosen node auto-wires to the source pin.
- Menu rows: name + type-colored signature summary; pure functions get the
  green ƒ, executable the blue ƒ.
- Placing a Core/Project function creates a real function node (source-backed
  edit through the existing transaction path).

## Component tree

The left `My Canvas` rail is one source-backed tree over the current project:

- `Files` selects a source file and keeps its file revision visible to Canvas.
- `Functions` selects a projected function graph; `New` uses the existing
  checked function transaction.
- `Callback` creates an ordinary `fn on_<name>()` handler through that same
  checked function transaction. A projected `on_*` function is marked as a
  framework callback view; selecting its `handler` row opens the source-backed
  function graph.
- `Variables` lists the open function's parameters and local bindings with
  their types; `Add` promotes a selected value expression through the
  existing checked binding transaction.

Tree navigation changes the selected projection only. It never creates a
parallel graph or variable model; every item carries the current source ID and
revision and is re-rendered after a source transaction. Callback labels and
handler navigation are derived from `event_views`; they do not create an event
sidecar or alter saved Jet text.

## Typography

UI sans stack (`Inter, "Segoe UI", Roboto, system-ui, sans-serif`) for node
titles, pin labels, and menus. Monospace (`JetBrains Mono, ui-monospace`) is
reserved for type names, signatures, and source text.

## Quality floor

- No overlapping nodes in default layout projection; tidy layout must respect
  measured node sizes.
- Entry node must never render an empty body band.
- All hit targets ≥ 12px; cursor: grab/grabbing on nodes, crosshair on pins.
- 60fps pan/zoom on the demo graph; batch canvas draws.
- Keyboard: Delete removes selection, arrows nudge, F fits selection.
- prefers-reduced-motion respected for any animation (exec pulse, glow).


## Interaction behavior

- Node placement is free. A user's drop position is final; automatic layout runs
  only for a graph with no saved positions or when the user invokes Tidy.
  Positions persist in editor view state and always win.
- Dragging follows the pointer at 60fps with no default grid snap. Any pin can
  start a typed wire preview. A compatible drop connects; an empty-canvas drop
  opens a fuzzy, type-filtered insertion menu at the release point, and `Esc`
  cancels.
- Dragging a wire endpoint re-enters wire mode. Compatible drops retarget through
  a source transaction; empty drops use the same filtered menu and `Esc` restores.
  One output may fan out to many inputs, while a binding has one projected getter
  per graph. Marquee, additive selection, group movement, copy, paste, and
  duplicate use source transactions; pasted groups offset by 24px.

## Source-backed editor behavior

- Execution outputs use `role:"loop_body"`, `role:"loop_done"`, and
  `role:"early_return"` where applicable. A second compatible execution drop
  opens a no-write convergence preview. Extraction selects the exact-body helper
  when one exists; stale, ill-typed, or out-of-scope previews leave source and
  preview unchanged.
- Disabled and debug-only node states use the source-backed `D-CANVASSTATE1`
  contract; no source encoding is invented before that decision permits it.
- Palette nodes without a wire target are view-state-only staged nodes. They show
  a dashed unsaved ring, do not become dead source code, and materialize only
  when a compatible wire reaches the graph. Deleting one writes nothing.
- Pattern-test branches show one execution output per arm plus `else`; adding an
  arm opens a source-backed pattern editor. Comment groups are resizable,
  titled, movable view-state objects with no source representation.
- The `My Canvas` tree projects files, functions, and variables from source
  revisions. It never creates a parallel graph or variable model. Developer-only
  trust/hash details stay out of the beginner view; package, dependency, and
  diagnostic facts remain available in one quiet detail group.
- Details controls exist only when a live source transaction exists. Typed
  controls edit scalar, collection, tuple, and struct values through descriptors
  that retain the source expression and span. Invalid, incomplete, ambiguous,
  or stale input writes zero source bytes and shows the normal Canvas diagnostic.
  User-facing terms are Functions, Variables, Inputs, Outputs, and Execution;
  raw kind strings and protocol jargon are hidden.
- The toolbar groups view, lens, edit, run, and search/developer controls in one
  non-wrapping row. Debug controls appear only while debugging. Node width
  measures headers, pins, editors, and badges before rendering; control nodes
  have no subtitle, function nodes show only their module path, and entry nodes
  use the crimson header without a subtitle.
