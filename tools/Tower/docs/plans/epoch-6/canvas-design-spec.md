# Canvas Node Design Spec — Blueprint-grade polish

Canvas is Jet's visual scripting surface (UE5 Blueprint equivalent). This spec
defines the node visual system. It is the contract for the rendering overhaul.
Bar: a screenshot of Canvas must read as polished as a UE5 Blueprint graph.

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
- NO decorative rail lines through the pin rows (current strikethrough
  artifact must die).

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
Zoom label bottom-left as now. Minimap keeps current placement but adopts node
archetype colors.

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

## Typography

UI sans stack (`Inter, "Segoe UI", Roboto, system-ui, sans-serif`) for node
titles, pin labels, menus. Monospace (`JetBrains Mono, ui-monospace`) ONLY for
type names, signatures, and source text. Current all-mono look is part of the
broken feel — replace it.

## Quality floor

- No overlapping nodes in default layout projection; tidy layout must respect
  measured node sizes.
- Entry node must never render an empty body band.
- All hit targets ≥ 12px; cursor: grab/grabbing on nodes, crosshair on pins.
- 60fps pan/zoom on the demo graph; batch canvas draws.
- Keyboard: Delete removes selection, arrows nudge, F fits selection.
- prefers-reduced-motion respected for any animation (exec pulse, glow).

---

# V2 — Blueprint EXPERIENCE spec (owner punch list 2026-07-09)

## Interaction model (non-negotiable)

- **Node placement is free.** The user's drop position is final: no collision
  resolution, no auto-layout override, no jumping. Auto-layout runs ONLY on
  first projection of a graph with no saved positions, or on explicit Tidy.
  User positions persist in editor view state and always win.
- **Dragging is smooth**: node follows cursor 1:1 at 60fps, no grid snap by
  default (snap optional later).
- **Wire drag, universal**: press on any pin → live wire preview follows
  cursor colored by pin type. Release on a compatible pin = connect. Release
  on empty canvas = context menu opens AT the release point, pre-filtered to
  type-compatible nodes (exec pin → executable nodes; String out → nodes with
  a String input), search box focused, **fuzzy matching** (subsequence, rank
  by match quality). Picking an entry creates the node there and auto-wires.
  Esc cancels.
- **Rewiring**: mouse-down on a wire near its endpoint detaches that end and
  re-enters wire-drag mode. Drop on another compatible pin re-targets (source
  transaction), drop on empty = filtered menu, Esc restores.
- **Fan-out**: one output pin connects to any number of input pins (a getter
  feeds many consumers). Variable-get projection dedupes: ONE value node per
  binding per graph, wires fan out to every use.
- **Multi-select**: marquee, shift-click additive, drag moves the whole
  selection. Copy/paste/duplicate for nodes and groups (paste = source
  transaction inserting clones, offset +24px).

## Pin labeling rules

- Exec pins are UNLABELED when a node has one exec-in and/or one exec-out —
  the arrow already says it. Label exec outs only when there are 2+ (then /
  else, match arms, loop body/done).
- No "exec", no "then" text on single-exec nodes. Data pins keep names.

## Node states (gated on D-CANVASSTATE1 — do not build until ratified)

Disabled (greyed, skipped) and debug-only (hazard badge, dropped in release).
UI affordances may be designed but no source encoding until the ballot lands.

## Staged nodes (canvas-only until wired)

Nodes placed from the palette without a wire target exist ONLY in editor view
state — rendered at 100% but with a dashed border "unsaved" ring, not written
to source, never dead code. First wire that connects a staged node to the
graph spine materializes it (insert transaction, wired). Deleting a staged
node touches nothing. Staged nodes persist per-graph in view state.

## Comment boxes

Blueprint comment groups: resizable rounded rect behind nodes, title bar
(editable), 6 muted tint choices, stored in editor view state, move-with-
contents when dragged by title. No source representation.

## Pattern-matching branch UI

Jet `if` pattern arms (`== Variant(binding)` PatternTest) render as rows on
the branch node: one exec-out per arm labeled with the pattern source chip
(mono, type-colored), plus else. Adding an arm = add-pin affordance (+) that
opens a pattern editor writing through the existing source transaction.

## Editor shell (the Blueprint editor window, not just the graph)

- **Left sidebar = My Canvas** (BP "My Blueprint"): FILES (each row clickable
  → opens that file's graphs; active row highlighted), FUNCTIONS (click →
  open graph; + New), VARIABLES (per open function: params + locals, each row
  = name + type chip in type color; click → properties panel; + Add).
  KILL: "TRUST"/"Source truth" block, sha256 hashes, "1 source-truth files"
  row — all developer-mode-only debris. Package/deps/diagnostics stay but
  collapse into a single quiet STATUS group.
- **Right panel = Details** (BP Details): selected variable → name, type,
  default value, all editable through existing transactions; selected
  function → name, signature, effects list, markers (test/bench), visibility;
  selected node → its pins/values, plain-language rows. NO raw kind strings,
  no "flow", no protocol jargon anywhere a user reads. Terms: Functions,
  Variables, Inputs, Outputs, Execution.
- **Toolbar**: icon buttons with tooltips, grouped: [view: fit, zoom] |
  [lens: Code / Split / Graph] | [edit: undo redo align tidy] | [run: run,
  debug controls collapsed into a Debug dropdown] | [right: search,
  developer toggle]. Text labels only where an icon would be ambiguous.
  One row, no wrapping; overflow menu for the rest. Debug rail buttons
  (Break/Watch/Step/Next/Continue/Stop) appear ONLY while debugging.

## Precision pass (bugs the polish missed)

- Nothing renders outside a node's rounded rect: editors, chips, badges all
  measured into the body width; node width = max(header, widest pin row,
  widest editor) + padding, measured with ctx.measureText, no clipping ever.
- Control nodes (if/return/loop/switch) have NO subtitle. Function nodes'
  subtitle = module path only. Entry subtitle = nothing (crimson header is
  the signal).
- Spacing: 8px grid inside nodes; consistent 10px header text baseline;
  pin rows exactly 24px; body padding 8px top/bottom.
