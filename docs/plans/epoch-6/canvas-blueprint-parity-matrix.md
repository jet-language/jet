# Canvas Blueprint Parity Matrix

Status vocabulary:

- `shipped`
- `claimed`
- `planned`
- `blocked-by-ballot`
- `rejected-as-Blueprint-semantic-debt`
- `not-yet-applicable`

Vocabulary: [Jet vocabulary](../../spec/vocabulary.md).

This matrix is the Epoch 6 ratchet. A row may move to `shipped` only when the
implementation is source-backed, verified by a real browser interaction
scenario, and does not add a hidden graph asset or semantic sidecar. `claimed`
means implementation exists but proof is protocol-, projection-, or grep-class.
Ratchets use class prefixes: `interaction:`, `protocol:`, `projection:`,
`grep:`.

Re-baselined 2026-07-10 against the live Chromium harness and code read. An
`interaction:` ratchet counts only when the scenario drives real pointer or
keyboard gestures; a scenario that posts transactions through
`window.__jetCanvasTest.postTransaction` or `fetch("/canvas/transaction")`
proves the protocol path, not the gesture, and holds a row at `claimed`.

Card #376 rechecked the remaining claimed in-graph rows on 2026-07-25. It
excluded the staged-node work in #388, menu and branch work in #389, data-wire
work in #488, selection and copy work in #489, and large-graph work in #382.
Rows that had only projection or grep proof moved back to `planned`.

| Area | UE 5.8 Blueprint capability | Canvas target | Status | Ratchet |
|---|---|---|---|---|
| Workbench | Right-click action menu | Context menu opens source-backed node actions from graph facts. | shipped | interaction:tests/canvas_scenarios.rs::palette_insert_flow_variable_project_core; interaction:tests/canvas_scenarios.rs::no_dead_end_ad_hoc_insert |
| Workbench | Drag-off-pin action menu | Compatible action menu filtered by sema expected-type facts. | shipped | interaction:tests/canvas_scenarios.rs::palette_insert_core_fn; interaction:tests/canvas_scenarios.rs::fallible_context; interaction:tests/canvas_scenarios.rs::no_dead_end_ad_hoc_insert |
| Workbench | Built-in method search | Palette/context search includes source-checked built-ins and ordinary Jet functions with pin metadata. | claimed | interaction:tests/canvas_scenarios.rs::palette_insert_catalog_sweep; downgraded 2026-07-10 — open P0 #389 (wrong-callee inserts, phantom `println` foreign leak, ranking) breaks the menu in real use |
| Workbench | Click-select node to details | Clicking a node selects it and populates the details panel. | shipped | interaction:tests/canvas_scenarios.rs::click_select_details |
| Workbench | Shift-add, ctrl-toggle, marquee select | Modifier clicks and marquee drag build multi-selection over projected nodes. | claimed | grep:crates/jet-canvas/src/js/input-events.js selection paths; no gesture scenario drives a marquee drag or modifier click |
| Workbench | Pan, zoom, zoom-to-fit | Browser panel supports pan, zoom, fit, and nonblank graph rendering. | shipped | interaction:tests/canvas_scenarios.rs::pan_zoom_fit |
| Workbench | Child/parent graph navigation | Breadcrumbs and graph picker navigate function/test/lambda graphs. | shipped | interaction:tests/canvas_scenarios.rs::workspace_keyboard_view_state |
| Workbench | Drag nodes and groups | Dragging stores local view state unless source-anchored hints apply. | shipped | interaction:tests/canvas_scenarios.rs::node_drag_persists_without_source_change |
| Workbench | Align, distribute, tidy | Graph organization commands persist only local view/editor state; source stays truth. | shipped | interaction:tests/canvas_scenarios.rs::workspace_keyboard_view_state |
| Workbench | Reroute nodes | Wire reroute pins let users shape long wires without semantic effect. | planned | not built; no reroute concept in graph JSON or JS |
| Workbench | Bookmarks and favorite actions | Graph bookmarks, palette pins, and recency/frequency ranking are local editor state. | shipped | interaction:tests/canvas_scenarios.rs::workspace_keyboard_view_state |
| Workbench | Cut/copy/paste/duplicate | Source transactions duplicate or move source-backed selections. | planned | #489 |
| Workbench | Inspector/details panel | Inspector shows node kind, pins, source span, and edit affordances. | shipped | interaction:tests/canvas_scenarios.rs::click_select_details; interaction:tests/canvas_scenarios.rs::rename_variable_sidebar |
| Hotkeys | Save, undo, redo, find, check | Blueprint-compatible command layer maps to source transactions and Jet checks. | planned | #271, #272, #282 |
| Hotkeys | Undo/redo | Undo/redo restore exact validated Jet source through the edit protocol. | shipped | interaction:tests/canvas_scenarios.rs::undo_restores_source; interaction:tests/canvas_scenarios.rs::undo_depth_20_mixed_run; interaction:tests/canvas_scenarios.rs::random_ops_source_sync |
| Hotkeys | Breakpoint and comment chords | Hotkeys target local debug anchors or source-backed comment regions. | planned | #273, #279 |
| Node model | Function calls | Calls project as typed nodes with input/output pins. | shipped | interaction:tests/canvas_scenarios.rs::palette_insert_catalog_sweep |
| Node model | Pure function calls | Pure leaves render inline by default and can expand. | planned | #274 |
| Node model | Variables get/set | Bindings, reads, and reassignments project as source nodes. | planned | projection proof only; needs a retained pointer or keyboard authoring scenario |
| Node model | Branch, switch, loops | Jet `if`, dispatch, and `loop` forms project and insert from palette without opaque fallbacks; pattern-arm rows are editable source transactions. | shipped | interaction:tests/canvas_scenarios.rs::palette_insert_flow_variable_project_core; interaction:tests/canvas_scenarios.rs::pattern_arm_add_edit_remove; interaction:tests/canvas_scenarios.rs::pattern_arm_invalid_refused; projection:#272/#274, tests/canvas.rs |
| Node model | Sequence, gate, do-once, do-N | Blueprint scheduler nodes are rejected unless represented by ordinary Jet control/callback code. | rejected-as-Blueprint-semantic-debt | #278 |
| Node model | Large graph virtualization and LOD | Canvas renders visible graph regions and low-zoom title-bar nodes for large projections. | planned | downgraded 2026-07-10 — no LOD scenario and no measured frame-time evidence; verify under #382's performance ratchet |
| Node model | Math Expression node | Expression text stays ordinary Jet expression source, not a separate formula language. | shipped | interaction:tests/canvas_scenarios.rs::math_expression_keyboard_edit |
| Pins and wires | Exec pins and data pins | Separate control/data rails over Jet semantics. | shipped | interaction:tests/canvas_scenarios.rs::palette_insert_catalog_sweep; interaction:tests/canvas_scenarios.rs::exec_rewire_reorders_statements |
| Pins and wires | Typed colored wires | Pin type, capability, fallibility, effect facts, and source spans render distinctly. | shipped | interaction:tests/canvas_scenarios.rs::exec_rewire_reorders_statements; projection:#274/#278, tests/canvas.rs |
| Pins and wires | Incompatible refusal (exec) | Wrong exec wires are impossible or fail with Jet diagnostics. | shipped | interaction:tests/canvas_scenarios.rs::exec_rewire_refuses_cross_block; interaction:tests/canvas_scenarios.rs::exec_rewire_binding_order_diagnostic |
| Pins and wires | Incompatible refusal (data) | An incompatible data-wire drop refuses in-UI before sema, with the reason shown. | planned | projection-only today; open bugs — data rewire to a fn symbol yields fn-value-where-Int (`crates/jet-canvas/src/js/inspector-connections.js::completeConnection`), inline editors accept wrong-type input then sema rejects (`crates/jet-canvas/src/js/inspector-connections.js::updateDetails`) |
| Pins and wires | Auto-cast insertion | Ratified visible conversion node/call writes source. | shipped | interaction:tests/canvas_scenarios.rs::conversion_keyboard_gesture; D-CANVAS-CONVERT1 |
| Pins and wires | Promote pin to variable | Promotion creates an ordinary Jet binding. | shipped | interaction:tests/canvas_scenarios.rs::promote_pin_keyboard_gesture |
| Pins and wires | Break/move links | Rewire transactions rewrite ordinary Jet call/argument source. | shipped | interaction:tests/canvas_scenarios.rs::exec_rewire_reorders_statements; projection:#277, tests/canvas.rs |
| Pins and wires | Drag-drop rewiring (exec) | Exec endpoint drag applies source-backed statement reorder transactions. | shipped | interaction:tests/canvas_scenarios.rs::exec_rewire_reorders_statements; interaction:tests/canvas_scenarios.rs::exec_rewire_refuses_cross_block |
| Pins and wires | Data-pin drag-to-wire | Real pointer drag from a data output pin to a compatible input pin writes the wired source transaction. | planned | the Blueprint-signature gesture has no gesture scenario; wire-data-and-exec posts transactions directly |
| Pins and wires | Multiple exec OUT (generalized) | Switch arms, loop body/done, and early returns project as N labeled exec outs. | planned | #391; branch/pattern arms exist, generalized outs do not |
| Pins and wires | Multiple exec IN (convergence) | N incoming exec wires converge on one node over source-backed statement spans. | planned | D-CANVAS-MULTIEXEC1=E on #391 is ratified: the second drop opens one no-write preview with extract selected by default, exact-body helper reuse when available, and explicit warned duplication |
| Types | Primitive and user types | Pins display Bool, numeric, String, structs, enums, collections, options/results. | planned | projection proof only; needs pointer or keyboard inspection across the type set |
| Types | Object/reference handles | Handles render as Jet library/type facts, not Blueprint object semantics. | planned | #274 |
| Types | Effect and unsafe markers | Async/effect/proof/unsafe rails are visual projections only. | planned | projection proof only; needs pointer or keyboard inspection |
| Patterns | Pattern arm authoring | Jet pattern arms are first-class rows: add, edit, remove, diagnose, undo, and reproject as ordinary source. | shipped | interaction:tests/canvas_scenarios.rs::pattern_arm_add_edit_remove; interaction:tests/canvas_scenarios.rs::pattern_arm_invalid_refused; protocol:tests/canvas.rs::canvas_pattern_arm_and_multi_input_transactions_write_source |
| Patterns | Pattern forms | Variant patterns, or-patterns, ranges, options/results, structs, and string-match patterns are source-backed Jet pattern text validated by the front end. | shipped | interaction:tests/canvas_scenarios.rs::pattern_arm_add_edit_remove; projection:tests/canvas.rs::canvas_projects_pattern_arm_and_multi_input_pin_metadata |
| Pins and wires | Multi-input pins | List literal nodes append and remove element pins through source transactions. | shipped | interaction:tests/canvas_scenarios.rs::multi_input_append_remove; protocol:tests/canvas.rs::canvas_pattern_arm_and_multi_input_transactions_write_source |
| Comments | Node bubbles | Source-backed comment bubbles use ratified source-anchored hints. | shipped | interaction:tests/canvas_scenarios.rs::source_comment_keyboard_reload; D-CANVAS-LAYOUT1 |
| Comments | Comment boxes | Region boxes persist through ordinary Jet comments when shared; viewport state stays local. | shipped | interaction:tests/canvas_scenarios.rs::source_comment_keyboard_reload; D-CANVAS-LAYOUT1 |
| Comments | Free-floating editor notes | Notes with no source anchor stay local. | rejected-as-Blueprint-semantic-debt | #279 |
| Functions | Create function graph | Every Jet function body opens as a graph; Canvas can create an ordinary helper function transaction. | planned | protocol proof only; needs a pointer or keyboard creation scenario |
| Functions | Edit signature | Signature edits write ordinary Jet function source, sema-check, and reproject. | planned | protocol proof only; needs a pointer or keyboard signature scenario |
| Functions | Add/remove/modify input and output pins | Function pin controls edit ordinary Jet parameter lists and return type, then reproject call nodes from source. | planned | grep proof only; needs a pointer or keyboard pin scenario |
| Functions | Extract selection to function | Extract writes an ordinary helper function after sema proves captures/returns. | planned | protocol proof only; needs a pointer or keyboard extraction scenario |
| Macros/collapse | Collapse graph | Collapse is a view over a source span, expandable without semantic drift. | shipped | interaction:tests/canvas_scenarios.rs::collapse_expand_keyboard_gesture; D-CANVAS-COLLAPSE1 |
| Macros/collapse | Blueprint macros | Separate visual macro semantics are not part of Canvas v1. | rejected-as-Blueprint-semantic-debt | #280 |
| Events | Event graph entry nodes | Framework callback views project from ordinary `on_*` Jet functions. | planned | projection proof only; needs pointer or keyboard graph-entry proof |
| Events | Event dispatchers/interfaces | `core.event` Event/Hook calls project dispatcher emit/subscription/lifetime/EventTrace facts from source. | planned | projection proof only; needs pointer or keyboard dispatcher proof |
| Interfaces | Trait/impl authoring | Jet traits project Blueprint-interface parity facts and create ordinary checked impl stubs. | planned | protocol proof only; needs pointer or keyboard impl authoring proof |
| Tasks | Latent action parity | `task`/`task.group`/`join` and `core.tasks` channel forms project async rails and task-flow facts. | planned | projection proof only; needs pointer or keyboard task-flow proof |
| Debugger | Debug session selector | Canvas selects the local source-span debug session. | planned | projection proof only; needs live pointer or keyboard debug-session proof |
| Debugger | Step, next, continue, stop | Debug rail buttons drive a live session end to end under gesture coverage. | planned | debug-break/watch/step/next/continue/stop buttons exist in html.rs with zero behavior tests; largest untested surface |
| Debugger | Breakpoints/watches | Local source-span debug state drives breakpoints and watches without editing source. | planned | projection proof only; needs live pointer or keyboard debug-state proof |
| Debugger | Active node/wire pulse | Runtime overlays map active node/wire pulses back to source spans. | planned | projection proof only; needs live interaction proof |
| Debugger | Call stack and trace | Trace anchors map to graph/source spans. | planned | projection proof only; needs live interaction proof |
| Search/refactor | Find in graph/project | Shared LSP/semindex query engine drives graph search. | planned | protocol proof only; needs keyboard search proof |
| Search/refactor | Find references/rename | Refactors preview and write source through existing codemod paths. | planned | protocol proof only; needs pointer or keyboard refactor proof |
| Search/refactor | Source-to-graph jump | Source span selects matching graph node/pin/inline expression. | planned | protocol proof only; needs source-to-graph gesture proof |
| Search/refactor | Toggle graph/source view | Canvas and source code toggle over the same file, preserving source as truth. | shipped | interaction:tests/canvas_scenarios.rs::graph_source_toggle_preserves_selection |
| Accessibility | Keyboard-only authoring | Keyboard commands cover search, action palette, alignment/tidy, bookmarks, run, undo/redo, graph/source toggle, and node nudge with focus-visible/reduced-motion support. | shipped | interaction:tests/canvas_scenarios.rs::workspace_keyboard_view_state; interaction:tests/canvas_scenarios.rs::promote_pin_keyboard_gesture; interaction:tests/canvas_scenarios.rs::conversion_keyboard_gesture; interaction:tests/canvas_scenarios.rs::math_expression_keyboard_edit |
| Learning | Node docs and first-run overlay | Source doc comments, type explanations, pin hover text, and a dismissible local first-run overlay guide Canvas without changing source. | claimed | interaction:tests/canvas_scenarios.rs::node_docs_pointer_hover; first-run dismissal still needs retained interaction proof |
| Runtime | Live-run loop | Canvas exposes a run HUD and one-key run/re-run path through the debug overlay and local watches. | shipped | interaction:tests/canvas_scenarios.rs::run_button_output_visible |
| Source control | Dirty/stale/conflict markers | Graph transactions guard against stale source revisions; diagnostics are tagged to the checked revision so stale problem overlays clear on reproject. | shipped | interaction:tests/canvas_scenarios.rs::bubble_appears_and_clears; projection:#265, tests/canvas.rs |
| Source control | Transaction diff preview | Canvas shows exact Jet text diffs and Git dirty state before/after source-backed writes. | planned | grep proof only; needs pointer or keyboard preview proof |
| Source control | Asset checkout/lock model | Asset-style graph checkout is not Canvas truth. | rejected-as-Blueprint-semantic-debt | #283, D-CANVAS-SCM1 |
| Public protocol | Graph JSON schema | `jet.canvas.graph` v1 exposes source-backed graph facts. | claimed | protocol:#265, tests/canvas.rs |
| Public protocol | Edit transaction schema | `jet.canvas.edit` v1 supports initial source transactions. | claimed | protocol:#265, tests/canvas.rs |
| Public protocol | Forward compatibility | Unknown non-semantic fields are ignored by old clients and never carry semantics. | planned | #276 |
| Extensibility | Function library projection | Packages expose ordinary Jet functions/types/docs as source-backed palette entries. | shipped | interaction:tests/canvas_scenarios.rs::palette_insert_catalog_sweep; interaction:tests/canvas_scenarios.rs::palette_insert_core_fn |
| Extensibility | Behavior-producing third-party nodes | Ratified Canvas actions use checked TIR/JIT preview and return source transactions only. | planned | protocol proof only; needs pointer or keyboard preview-and-apply proof |
| Validation | Check/compile button | Canvas invokes Jet front-end diagnostics, never raw rustc output. | shipped | interaction:tests/canvas_scenarios.rs::check_button_populates_panel |
| Validation | Formatter stability | Every write runs through formatter and reprojects from source. | shipped | interaction:tests/canvas_scenarios.rs::random_ops_source_sync; projection:#265, tests/canvas.rs |
| Tests | Projection JSON snapshots | Every supported Jet construct needs deterministic graph coverage. | claimed | projection:#285, tests/canvas.rs::canvas_hardening_projection_suite_covers_blueprint_backlog_constructs |
| Tests | UI nonblank and interactions | Browser panel has nonblank, keyboard, pointer, search, palette, undo, diff, and debug ratchets. | shipped | interaction:tests/canvas_scenarios.rs::open_and_render |
| Tests | Unsupported-feature diagnostics | Unsupported and invalid Canvas actions fail through Canvas/Jet diagnostics, never raw rustc. | claimed | projection:#285, tests/canvas.rs::canvas_unsupported_and_invalid_actions_return_canvas_errors_without_rustc |
| Tests | Rows cannot ship without tests | This matrix is scanned by tests and shipped rows must name ratchets. | claimed | projection:#275, tests/canvas.rs |
| Future | Multi-user graph collaboration | Multiplayer editing is outside the current source transaction layer. | not-yet-applicable | post-Epoch 6 |
| Future | Binary graph assets | Canvas will not adopt Blueprint asset storage. | rejected-as-Blueprint-semantic-debt | Epoch 6 invariant |
