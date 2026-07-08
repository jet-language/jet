# Canvas mockups — design notes

Three single-file mockups, one per family (docs/design/frontends/DESIGN-FAMILIES.md).
Same feature inventory in all three; visuals differ per family. No theming.

Shared facts:

- Graph content is real Jet: `examples/features/tooling/canvas_blueprint_demo.jet`
  (helper/square/summarize/run) plus a fallible/effectful strip from
  `examples/features/net/http_client.jet` (tcp_listen, `?? panic`).
- Diagnostic text is verbatim from `tests/ui/borrow_conflict.stderr` (E0204) — I4:
  frame styled per family, words untouched.
- Pin colors by rail/type in every file: Control, Data (Int), Effects, Fallibility,
  Bool. Pin shapes: triangle = exec, circle = value, diamond = fallible/field-like.
- Feature parity per file: typed pins + bezier wires, pin conversion widget,
  rails legend, right-click context search palette (type-filtered), graph⇄code
  view of the same source, inspector, project/function selector, breakpoint +
  paused-at-breakpoint state, step/continue/stop, replay scrubber, active-wire
  pulse, minimap, status readouts, live focus styles, reduced-motion fallback.

## carbon.html — Family A

Rationale: the eight-hours-a-day instrument. Dense 3-column workbench, everything
monospaced and scannable; node headers tinted by kind; wires bright on near-black.
Closest in spirit to shipped Canvas.rs UI, upgraded to family tokens.

Signature: bottom status band of labeled lamps — BUILD ● OK · WATCH ● 3 files ·
DIAG ● E0204 ×1 · PORT ● 8080. Whole-system state in one glance; error lamp blinks
(disabled under reduced-motion).

Transplants:
- Unreal Blueprint → white/neutral exec triangles vs colored data pins; exec wire
  thicker; live execution pulse traveling the active wire while paused.
- Unreal Blueprint → right-click contextual node search, pre-filtered by the
  dragged pin's type ("compatible with Int").
- Blender nodes → kind-colored node headers (entry green, call cyan, branch amber,
  io/effect magenta-violet).
- Node-RED → left palette rows with type-colored left edge.

UX risks: density punishes newcomers — first-run needs the beginner path (tour,
big empty-state) that the shipped Canvas already has; lamp blink must stay the only
animation or the band turns into noise; dark-only.

## paper.html — Family B

Rationale: the counter-position — Canvas as a calm, editorial document. Light
ground, white node cards with a thin type-colored top rule, hairline structure,
type does hierarchy. Aims at the beginner facet: reads like docs, not a cockpit.

Signature: the fading hairline — every label→value row in the inspector and every
section head leads the eye with a rule that starts solid and fades to nothing.

Transplants:
- Unreal Blueprint → exec triangles / data circles, contextual right-click search.
- Blender nodes → node kind color moved to a restrained 3px top rule instead of a
  filled header (keeps the page light).
- Figma → quiet left nav with inline signatures; selection = soft blue fill +
  2px accent ring, not glow.
- Stripe-docs class layout → breadcrumb project/function selector in the header.

UX risks: pin/wire colors need dark, saturated tuning on light ground (done:
slate/blue/violet/amber/crimson) — any lighter and rails become illegible;
long debug sessions on white may fatigue; wire pulse is subtler than on dark.

## pulse.html — Family C

Rationale: minimal chrome, maximal canvas. No fixed panels — floating command
pill, drawer, inspector, transport bar over a full-bleed graph. Bold weight-800
titles, cold violet structure. Built for demo-day energy without losing the
inventory.

Signature: the single hot→hot2 gradient — exactly one hot thing on screen: the
paused Branch node (gradient border + gradient title) and the execution pulse
flowing into it. Everything else stays cold; when nothing runs, nothing is hot.

Transplants:
- tldraw → floating pill toolbar + floating panels over an edge-to-edge canvas.
- Rive editor → bottom-center transport bar (debug step/continue + replay scrub
  as one media-style strip).
- Unreal Blueprint → exec/data pin split, contextual search, wire pulse (recolored
  to the hot gradient — it IS the one hot element).
- Blender nodes → kind-colored header badges (cold hues only).

UX risks: floating panels can occlude graph on small laptops (drawer/inspector
collapse under 1100/900px, but overlap between those widths needs real layout
work); one-hot-thing discipline is fragile — any second warm accent (e.g. amber
fallibility pins) must stay visibly muted; backdrop-filter cost on weak GPUs.

## Research sources

- [Nodes in Unreal Engine — pin/wire semantics](https://dev.epicgames.com/documentation/unreal-engine/nodes-in-unreal-engine)
- [Blueprint execution path — exec wire highlight during play](https://www.oreilly.com/library/view/blueprints-visual-scripting/9781789347067/2757c489-4be0-4b70-afdd-3b5b0e4d74d4.xhtml)
- [Blender node parts — socket color = data type](https://docs.blender.org/manual/en/latest/interface/controls/nodes/parts.html)
- [Blender socket shapes redesign](https://code.blender.org/2025/08/new-socket-shapes/)
- [tldraw UI components — floating minimal chrome](https://tldraw.dev/docs/user-interface)
- [Node-RED palette/sidebar layout](https://nodered.org/docs/user-guide/editor/palette/)
