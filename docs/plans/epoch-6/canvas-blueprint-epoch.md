# Epoch 6 — Canvas: the Blueprint-class editor, for real

Companion to [canvas-blueprint-parity-audit.md](canvas-blueprint-parity-audit.md)
(feature inventory, hybrid model, devil's advocate — still accurate) and
[canvas-blueprint-parity-matrix.md](canvas-blueprint-parity-matrix.md) (the
ratchet). This document adds what four failed attempts were missing: an honest
definition of "shipped", the ecosystem scope beyond the single graph, and a
milestone plan whose exit criteria a human can watch happen on screen.

## Why attempts 1–4 failed (post-mortem, 2026-07-09)

The matrix shows 57 rows "shipped". Zero are verified by driving the real UI.
~33 rows cite in-process JSON assertions (projection/transaction correctness,
never the rendered editor); ~23 cite one 630-line test that greps served
HTML/JS for id strings and function names. A button with a dead click handler
passes every existing test. Card #265's acceptance originally required
browser-driven interaction tests; that bar was silently dropped and never
balloted. Each successive rewrite went "green" by adding new grep lines.

Owner-observed result: drag-off-exec-pin node creation fails for every
function with an unreadable 2-second error toast — while the matrix rows
"Drag-off-pin action menu" and "Drag-drop rewiring" say shipped.

**Epoch 6 rule: a parity row is `shipped` only when a scenario test drives the
real editor in a real browser — mouse, keyboard, screenshot — and asserts both
the on-screen result and the resulting Jet source.** Everything else is
`claimed` at best. No exceptions, no downgrades without a ballot.

## Scope: the ecosystem, not the graph

The graph canvas is one panel. Blueprint earns production use from the
machinery around it. Inventory research (2026-07-09, 14 sources) ranked the
ecosystem features by production impact; the table maps each to its Jet
answer. "Free" means source-backing already solves it better than BP's binary
assets ever could — the work is surfacing it in the editor, not inventing
machinery.

| BP ecosystem feature | Jet/Canvas answer | Status |
|---|---|---|
| Find in Blueprints (project-wide indexed search) | semindex/LSP query engine, already shipped for graph search | surface project-wide UI (M3) |
| Blueprint Diff tool (binary assets need custom UI) | plain git text diff — structurally free | review view polish (M3) |
| Compiler Results panel + node error bubbles | Jet diagnostics (I4 quality) | build the panel + bubbles (M1) |
| Function/Macro libraries | ordinary Jet modules + jetpack packages | palette already lists; browsing UX (M3) |
| Blueprint namespaces (editor perf + visibility) | Jet modules + import surface | palette scoping (M4) |
| Interfaces panel | Jet traits (projection shipped #316) | surface panel UI (M3) |
| Event dispatchers | core.event (projection shipped #311) | surface panel UI (M3) |
| Call In Editor | `jet run` / `#Test` + Canvas run HUD (#317) | verify interactively (M1) |
| Variable metadata (instance-editable, tooltips, categories) | doc comments + attributes — partially exists, partially **gated** | ballot D-CANVASMETA1 (M3) |
| Reparenting | traits/composition; no BP-style inheritance to repair | not applicable |
| 3-way merge tool | plain text merge — free | document as advantage (M3) |
| My Blueprint categories | My Canvas sidebar groups | maturity pass (M3) |
| BP→C++ hot-path migration | none needed: Canvas IS the language | not applicable — advantage |
| Palette favorites + class filter | favorites shipped (#313) | verify interactively (M0 sweep) |
| Templates + Content Examples first-run | Jet examples are the executable spec (I5) | onboarding project + tour (M5) |

BP's production failures Canvas must not import: binary-asset merge pain
(free), stale-compile bugs (Canvas rechecks on every transaction), VM overhead
(compiled Jet), spaghetti-by-default (extract-to-function is first-class).

## Milestones

Ordering is strict: parity before improvement (M6 last), truth bar before
features (M0 first). Each milestone's exit criteria are scenario tests plus an
owner acceptance script — the exact clicks the owner performs to accept.

### M0 — Truth bar: interaction test harness

The epoch's foundation. A headless-browser scenario harness in the dev shell
that drives the served Canvas with real mouse/keyboard events, reads pixels
and DOM, asserts resulting source text, and runs in CI via
`scripts/agent/verify-full.sh`.

- Browser strategy needs one decision (ballot D-CANVASTEST1): dev-shell
  chromium + zero-npm-dep CDP pipe driver (rec), vs playwright-core dev-dep,
  vs WebDriver BiDi/firefox.
- Editor exposes dev-mode test hooks (pin screen positions, node bounds,
  staged-node registry) — hooks are dev-only, never product surface.
- Matrix upgrade: ratchet-class column (interaction / protocol / projection /
  grep); enforcement test requires `interaction:` ratchets for `shipped`;
  all 57 current rows reclassify to `claimed` until their scenario lands.
- Exit: harness runs 5 seed scenarios (open, pan/zoom, click-select, palette
  insert, undo) headless in CI; matrix enforcement live; a deliberately broken
  click handler fails the suite (anti-regression proof).

### M1 — The core loop works, visibly

The owner's demo path, bulletproof: open project → read graph → insert any
node from any category → wire it → edit values → undo → run → see output.

- Every core-catalog and project-function palette entry inserts valid source
  or is excluded from the palette with a stated reason. Property-style
  scenario: iterate the entire catalog, insert each into a scratch function,
  assert sema-clean or excluded. (Root-cause fix for the exec-pin drag
  failures lands here; audit of 2026-07-09 carries the defect list.)
- Error surfacing: failed transactions show the full Jet diagnostic in a
  persistent panel (dismiss or 10s minimum, never 2s); Check button runs
  front-end diagnostics into a Compiler-Results-style panel with
  jump-to-node; nodes with diagnostics get error/warning bubbles.
- Undo/redo to depth 20 across mixed operations, graph/source views never
  desync (scenario: 30 random ops, compare projection to source each step).
- Exit: ~20-scenario suite green + owner acceptance script #1.

### M2 — Complete in-graph authoring parity

The known unfinished graph work, now interaction-verified:

- Exec-wire endpoint rewiring = statement reorder (control wires gain source
  spans; drag exec wire re-orders statements with diff preview).
- Pattern-matching arms: add/edit/remove arm transactions (Jet `== Variant(x)`
  arms as first-class rows — this is a Jet advantage over BP, treat it as such).
- Multi-input pins: append-element transactions for list literals and fan-out
  `f.[…]` calls.
- Math-expression chip: inline Jet expression editing on any input pin
  (BP Math Expression node parity, but it is just Jet source).
- Promote-pin-to-variable, cast/conversion insertion, collapse/expand — all
  already claim shipped; each gets its scenario and whatever fixes that
  reveals.
- Exit: in-graph matrix rows all `shipped` under the new bar + owner script #2.

### M3 — Editor ecosystem parity

- Details panel maturity: per-selection property editing (variable
  name/type/default/docs; function signature/pure/visibility; node values)
  with every field either live or absent — no dead controls.
- Variable/function metadata surface (categories, tooltips from doc comments,
  editor-exposure flags): needs ballot **D-CANVASMETA1** for any new
  attribute syntax; doc-comment-derived parts are ungated.
- Project-wide find/references/rename UI (semindex exists; make the UX real).
- Review view: git diff panel with per-hunk graph highlight (our answer to
  the BP diff tool — text diff plus graph overlay).
- Traits panel (implemented/required methods, impl stub creation — #316
  machinery) and Events panel (core.event dispatchers — #311 machinery).
- Module/package browser: jetpack deps + core modules as a navigable library
  view with docs.
- Exit: ecosystem scenario suite + owner script #3.

### M4 — Project scale

- Multi-file projects: file switching, cross-file function graphs, cross-file
  insert (import synthesis already exists), workspace-wide search scoping.
- Performance ratchet, interactively measured: open a generated 300-function
  project; pan/zoom stays smooth (frame-time budget asserted via the harness);
  virtualization/LOD rows re-verified under the new bar.
- Palette scoping by module (BP-namespace equivalent) + favorites polish.
- Exit: scale scenario suite on the generated big project + owner script #4.

### M5 — Onboarding

- First-run: a `canvas tour` example project (I5: examples are the spec) that
  opens with a guided overlay — the Content-Examples pattern, source-backed.
- Node/docs hovers everywhere (doc comments → tooltips; module docs in
  palette rows), keyboard cheat-sheet overlay, empty-states that teach.
- `jet dev --canvas-demo` (or equivalent) one-command demo entry.
- Exit: a new user (owner proxy) goes from `jet new` to a working two-function
  graph program without touching a text editor or any doc outside the tool.

### M6 — Beyond parity (only after M1–M5)

Jet-native advantages, explicitly deferred until parity is owner-accepted:
pattern-matching authoring UX beyond BP, fallible/effect rails polish,
fan-out operator visualization, live-value overlays on wires during runs,
text-review-first collaboration flows. Card them, freeze them until M5 exits.

## Ballot gates (queue before their milestone starts)

| Ballot | Decides | Blocks |
|---|---|---|
| D-CANVASSTATE1 (queued, on #368) | disabled / debug-only node source spelling | node-state UI (M2) |
| D-CANVASTEST1 (new) | interaction-harness browser + driver strategy | M0 |
| D-CANVASMETA1 (new) | variable/function metadata attribute surface | metadata parts of M3 |

Prior ratified decisions stand: D-CANVAS-LAYOUT1 (layout/comment hints),
D-CANVAS-CONVERT1, D-CANVAS-COLLAPSE1, D-CANVAS-EVENT1, D-CANVAS-DEBUGSTATE1,
D-CANVAS-SCM1, D-CANVAS-EXT1/2.

## Process rules for this epoch

- Every card names its scenarios in exit criteria; a card is `verify`-ready
  only with scenarios green locally, `done` only after the parent re-runs them
  plus the full suite.
- No matrix row moves to `shipped` without an `interaction:` ratchet.
- Owner acceptance scripts are part of milestone exit — a milestone is not
  done because tests pass; it is done when the owner ratifies that milestone's
  acceptance **ballot** (options: accept / reject with punch list). The owner
  never answers through card logs or messages; every owner input is a decision
  ballot.
- Defects found by any audit become cards immediately (no fix-in-place during
  planning) so nothing lives only in a report.
