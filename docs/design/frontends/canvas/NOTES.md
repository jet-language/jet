# Jet Canvas — v2 archetype mockups

Four Blueprint-class visual editors for Jet. Same feature inventory (typed
pins, rails, node search, bidirectional graph⇄source, inspector, debugger,
replay, verbatim diagnostics) and one shared visual system from
`DESIGN-BRIEF.md`. The options differ by **UX archetype**, not paint.

Shared visual language (all four):
- Ground `#0B1119`, panels `#16202E`, accent `#3FC6FF`, ok/warn/error
  `#58D68D`/`#FFB454`/`#FF5C5C`, text `#D9E6F2`, secondary `#7E93A8`.
- One shared **pin/rail semantics** — colorblind-safe by shape, not only
  color: exec pin = triangle (control), data pin = circle, fallible pin =
  diamond. Rail colors fixed across all four: control slate, data cyan,
  fallible amber, effect violet, async green, proof grey, debug red.
- System font for chrome, ui-monospace for every type, value, and code
  fragment — the type/value duality is the personality.
- Signature element (shared): the **rail readout**. Jet's control/data/
  fallible/effect/async/proof/debug lanes are shown as a live gutter/strip
  that lights only the rails a graph actually uses; the debug rail pulses
  when a session is live. This is the memorable, Jet-specific thing; the
  four archetypes place it differently but never restyle it.

Feature truth read from `Source/Canvas/` (refactored out of `Canvas.rs`
into `schema_api.rs` + `graph_json.rs` + siblings): node kinds entry/
binding/call/branch/loop/return/dispatch/fallible/flow/yield; pins carry
name/direction/type/capability/fallible/effect_grant_need; wires control/
data/fallible; regions comment/collapse/grant/caps/taskgroup/unsafe/
comptime; affordances rename/edit-signature/break-link/move-link/promote-
inline/insert-visible-conversion/extract-fn/insert-call/insert-branch etc.
Diagnostic E0204 rendered verbatim from `tests/ui/borrow_conflict.stderr`
(I4). Graph content is real Jet from `examples/features/`.

---

## workbench.html
Core loop: **browse the palette catalog, drag a node onto the graph, wire
its pins, tune it in the docked inspector.**

Content: `io/files.jet` (fs.write/read, `??` panic, Fs+Io effects).
For people who live in the editor 8h/day. Everything is a fixed, resizable
dock: menubar, graph tabs, left palette+project+debugger, center stage with
the rail gutter down the left edge, right inspector. Nothing hides.

Transplants:
- Unreal Blueprint — white triangular exec pins vs colored round data pins,
  outline=unwired/solid=wired, exec wire highlights during play (our amber
  "hot" animated wire on the paused fallible edge). Its autocast-on-drop
  maps to our `insert_visible_conversion` inspector action.
- Blender node editor — socket color = data type; docked N-panel = our
  right inspector; category-grouped Add menu = the palette catalog.
- A traditional IDE (VS Code) — menubar, file tree, tabbed documents,
  docked debugger with call stack / locals / watches.

Debugger/replay/diagnostic: docked debugger pane (step/next/continue,
stack, locals, watches, replay scrubber); paused node glows amber, breakpoint
node ringed red; switching to the `borrow_conflict.jet` tab dims the graph
and pins the E0204 card to the offending call.

Beginner first node: drag a labelled, categorized node from the Palette.
Expert stays fast: Ctrl+K palette search, graph tabs, Align/Tidy, every
panel keyboard-reachable, docked debugger.

UX risks: densest of the four — palette+inspector always on screen costs
width (right dock drops below 1100px); most familiar, least novel.

---

## flow.html
Core loop: **type at the cursor to summon the node you're thinking of, and
keep flowing — never leave the canvas for a panel.**

Content: `effects/effect_grant.jet` (`#Grant(Fs, Io) { caps -> … }`).
Infinite dotted canvas, zero fixed panels. All chrome is floating and
transient: a top status pill, a rail-legend chip, a ⌘K/double-click
**summoner** that opens *at the cursor* and offers only type-compatible
nodes, a selection inspector that pops beside the picked node, a right-click
action menu, a floating debug/replay strip.

Transplants:
- tldraw / Figma — infinite canvas, chrome that floats over content, nothing
  docked; select-to-reveal contextual tools.
- Raycast / VS Code command palette — ⌘K summon; here it is spatial (opens
  where you are) and type-filtered to the pin you dragged from.
- Unreal's drag-from-pin context search — releasing a wire on empty space
  opens a search filtered to compatible nodes; we make that the primary verb.

Debugger/replay/diagnostic: floating debug strip (step/next, replay slider,
live local value, continue); paused node glows amber, its `??` fallible wire
runs hot; the E0204 card floats and sticks under the failing call.

Beginner first node: double-click empty canvas → summoner → type "print" →
Enter. The bottom hint bar spells this out.
Expert stays fast: ⌘K anywhere, right-click actions, keyboard through the
summoner, no pointer trips to a side panel — the whole loop is at the cursor.

UX risks: discoverability — power lives behind keys/right-click; the hint
bar and first-run cue must carry new users. Floating inspector can occlude
the node it describes on small screens.

---

## duallens.html
Core loop: **read or write in whichever lens fits the thought — code editor
or graph — and the other lens follows your cursor live.**

Content: `basics/fizzbuzz.jet` `label()` (nested if/else-if → returns).
Two equal panes over the *same* source (draggable split). Code lens is a
real line-numbered editor with a breakpoint gutter; graph lens is the node
projection with the rail strip on its edge. Click a line → its node lights;
click a node → its line scrolls into view and highlights. A breakpoint set
in the code gutter mirrors a red halo on the node.

Transplants:
- Enso — code and graph as two faithful views of one program, edit either.
- Observable / literate notebooks — code-forward, the visual is a live twin.
- JetBrains gutter breakpoints + editor↔structure highlight sync.
- Unreal "jump to node from source" made bidirectional and always-on.

Debugger/replay/diagnostic: a shared replay timeline in the status bar
scrubs execution; the current step highlights the same statement in *both*
lenses at once. The `borrow_conflict.jet` tab shows E0204 as one truth in
both lenses — a wavy red underline under `x` in code, a red node in the
graph, and the verbatim card at the bottom.

Beginner first node: type ordinary Jet in the familiar code lens; the graph
draws itself. No new gesture to learn.
Expert stays fast: full-speed text editing, gutter breakpoints, and the
graph is a free live map — never a thing you must hand-maintain.

UX risks: two panes halve horizontal room (graph collapses under 900px);
keeping selection/scroll/breakpoint/edit sync truly instant is the whole
value — any lag breaks the promise.

---

## guided.html
Core loop: **pick a seed, then choose from only the type-compatible offers;
the program grows correct-by-construction, one step at a time.**

Content: builds `errors/fallible_run.jet` (`load_port() -> Int ?` → `?` →
`print`) live. A vertical **spine** of built step-cards runs down the
center; the value's type rides the connector between steps. The right
**offer tray** lists only next steps whose input matches the value in hand,
Hazel-ordered (handle fallibility → name it → transform → call → control →
finish). Picking one appends a card and re-types the tray. You never drag a
wire; an invalid wire is unreachable.

Transplants:
- Hazel / Hazelnut — typed holes + type-directed, ordered next-action
  offers; incomplete-but-valid program at every step.
- Scratch — palette blocks that only snap where they legally fit; here the
  type system, not block shape, is the snap constraint.
- Type-directed IntelliSense / structured search (Unreal drag-from-pin
  again) turned into the *only* way to author, so beginners can't mis-wire.
- Blueprint exec/data pin shapes reused so a graduate reads the other three
  archetypes for free.

Debugger/replay/diagnostic: "Preview values" pins each step's runtime value
inline; a preview-run scrubber in the status bar walks the seed forward.
Diagnostics appear as **refused offers** — E0204 is shown as a disabled
offer with the verbatim why/fix, so the tool teaches the rule at the exact
moment the move would break it, instead of after.

Beginner first node: the strongest story — pick any offered step; every
option is guaranteed to type-check, so the first node can't be wrong.
Expert stays fast: type-to-filter the tray, ↑↓ + ↵ to append; the offer set
is a keyboard menu, not a drag.

UX risks: linear spine suits growing a function, not reshaping a large
existing graph — needs an escape to free-edit (hand off to workbench/flow).
Offer ranking quality is load-bearing; a bad first offer misleads novices.

---

## Self-critique
- Four core-loop verbs are distinct: drag-from-catalog · summon-by-typing ·
  edit-either-synced-lens · pick-from-typed-offers. No two match.
- No invented jargon or branding in UI copy (brief + v1 rejection): rails
  named by Jet's real facts (control/data/fallible/effect/async/proof/
  debug), no "fuel"/metaphors. Buttons name actions.
- Diagnostic text is verbatim E0204 (I4), frame/color-free words.
- Each answers beginner-first-node and expert-stays-fast above.
- All self-contained, system fonts, offline `file://`, reduced-motion
  respected, visible focus, laptop-responsive, each <35KB (< 300KB cap).
