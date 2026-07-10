# Canvas Blueprint Parity Audit

Epoch 6 target: Canvas gives Jet a Blueprint-class visual editor while keeping ordinary Jet source as the only semantic truth. The UX copies Blueprint power. The semantics stay Jet: no binary graph asset, no semantic sidecar, no hidden source of truth.

Ratchet matrix: [`canvas-blueprint-parity-matrix.md`](canvas-blueprint-parity-matrix.md).

Sources audited: official Unreal Engine 5.8 docs for Blueprints Visual Scripting, Blueprint Editor Cheat Sheet, Nodes, Variables, Functions, Macros, Events, Flow Control, Math Expression Node, Blueprint Debugger, Comments, Collapsing Graphs, Blueprint Class Assets, Blueprint Communications, Blueprint Function Libraries, and Source Control.

## Best Hybrid

Canvas should be source-backed Blueprint parity.

Shared source truth:
- Every function, event handler, branch, loop, call, binding, conversion, comment, breakpoint target, and extracted subgraph maps to ordinary Jet source spans.
- Canvas writes through formatter/codemod, rechecks through the Jet front end, and reprojects from source after every transaction.
- The graph protocol is versioned JSON. Unknown future fields are ignored by old clients and preserved only when they are non-semantic.

Visual control:
- Deterministic layout is the base projection.
- Shared layout intent, comment regions, and extraction boundaries use source-anchored metadata comments only after ballot approval.
- Per-user viewport state, selection, scroll, zoom, temporary pin hover, palette MRU, and active debug session stay local and never become semantic truth.

Blueprint UX parity:
- Users can drag nodes, marquee select, wire pins, break links, comment regions, create nodes from compatible pin menus, promote pins to bindings, collapse/extract graphs, debug execution, search/refactor, and use Blueprint-grade hotkeys.
- Pure leaves render inline by default and expand to nodes on demand.
- Exec, data, fallible, effect, proof, and debug rails are separate visual affordances over Jet's existing semantics.

Safety/I8 line:
- Auto-conversion may only insert an explicit visible conversion node/call that writes source, or use an already ratified canonical coercion.
- Macro parity means extract/inline ordinary Jet functions or source blocks unless the owner ratifies a distinct visual macro surface.
- Event graph parity means ordinary framework callback functions unless the owner ratifies new event declaration syntax/API.
- Plugin/node extension starts as read-only palette/schema providers. Behavior-producing providers need an owner decision.

## UE Feature Inventory

Graph workbench:
- Right-click graph action menu.
- Drag-off-pin action menu filtered by compatible type.
- Selection, shift-add, ctrl-toggle, marquee replace/add/remove.
- Pan, zoom, zoom-to-fit, child/parent graph navigation.
- Drag nodes and groups; arrow-key nudge.
- Delete, cut, copy, paste, duplicate.
- Node, graph, and pin context menus.
- Inline rename and F2 rename.
- Breadcrumbs, graph picker, details panel, palette, My Blueprint-style tree.
- Minimap, viewport focus, reset view.

Hotkeys:
- Save, undo, redo, find in graph, project search.
- Compile/check, breakpoint toggle, comment.
- Creation chords for branch, delay/task, sequence, gate, loop, multi-route, do-once/do-N, begin/start callback.
- Link breaking and connection moving shortcuts.
- Keyboard navigation for selection, focus, command palette, inspector, and source jump.

Nodes:
- Events/callbacks.
- Function calls.
- Pure function calls.
- Variable get/set.
- Literals.
- Cast/conversion nodes.
- Flow control: branch, switch, sequence, loops, do-once, do-N, gates.
- Math expression node.
- Communication nodes: direct refs, spawned refs, casts, dispatchers, interfaces.
- Collapsed graphs, macros, function libraries.
- Comments and node bubbles.

Pins and wires:
- Exec pins and data pins, input left and output right.
- Directional exec wires with active execution pulse.
- Typed data wires colored by type.
- Compatible connect, incompatible refusal with diagnostic.
- Auto-cast node insertion for compatible mismatches.
- Promote input/output pin to variable/binding.
- Break all links, break one link, move all connections.
- Hover emphasis for connected wires.
- Tooltips for type, source span, docs, value, effects, and diagnostics.

Types:
- Bool, byte/integer widths, float, name/symbol, string/text, vector-like records, object/reference-like handles, arrays, sets, maps, option/result, tasks, effects, unsafe capability markers, user structs/enums.
- Pin color/shape must distinguish data, control, fallible, async/task, effect, proof, and unsafe rails without creating new semantics.

Variables and bindings:
- Create binding from palette or promoted pin.
- Get/set/read/reassign nodes.
- Rename binding.
- Category/grouping, docs tooltip, visibility/export surface where Jet has a matching construct.
- Defaults, mutability, type annotation, inferred type display.
- Source jump and refactor preview.

Functions:
- Create function graph from source.
- Edit signature, params, return, defaults, docs, pure marker, visibility where Jet supports it.
- Reorder pins when source order changes.
- Call node updates after signature edits.
- Extract selection to function and inline function when sema proves it safe.

Macros/collapse:
- Collapse selection to a visual node.
- Expand collapsed node.
- Extract to function.
- Tunnel nodes for collapsed input/output projection.
- No separate semantic macro mechanism unless ratified.

Comments and organization:
- Node comment bubble.
- Comment boxes around selection or empty canvas region.
- Header edit, color, alpha, resize, move with contained nodes, zoom-scaled text.
- Section/category grouping in graph tree and palette.
- Comment/source round-trip test for every supported shape.

Debugger:
- Debug session selector.
- Breakpoints add/remove/disable/enable, invalid breakpoint state.
- Step, resume, stop.
- Active node highlight and execution arrow.
- Active wire pulse/fade.
- Watch expressions and pin values.
- Hover debug values; unexecuted value state.
- Data flow view with object/session filtering and collections expansion.
- Call stack and execution trace.
- Diagnostic overlays on nodes, pins, and wires.

Validation:
- Check/compile button runs Jet front-end checks.
- Diagnostics are Jet diagnostics, never raw rustc output.
- Type-compatible wiring enforced by sema-backed query.
- Stale graph transactions fail cleanly with source diff/reload.
- Formatter stability and no-op reprojection ratchets.

Search/refactor:
- Find in graph.
- Find in project.
- Find references.
- Rename.
- Jump to source.
- Jump from source span to graph.
- Promote to binding.
- Extract/inline.
- Diff preview before write.

Source control:
- Git text diff first.
- Dirty/stale/conflict markers.
- History hooks and revision compare.
- Optional lock/check-out abstraction only as UI over VCS, not as Canvas storage.

Extensibility:
- Public graph/edit schema.
- Palette/schema providers.
- Function library projection from Jet packages.
- Third-party behavior nodes only after a ratified API/protocol/safety boundary.

## Required Cards

The Tower card set for `plan=canvas-blueprint-parity` must cover:
- UE 5.8 parity matrix and ratchet.
- Graph UX shell.
- Typed pin authoring and conversions.
- Rails model.
- Comments and regions.
- Collapse/extract.
- Debugger parity.
- Search/refactor.
- Variables, functions, events.
- Source control and diff.
- Public protocol.
- Extensibility API.
- Test/hardening ratchets.

## Ballot Gates

Owner decisions required before implementation of gated surfaces:
- Source-backed layout/comment persistence.
- Auto-conversion policy.
- Collapse/macro parity model.
- Event/callback graph model.
- Breakpoint/watch persistence.
- Source-control UX scope.
- Plugin/node provider API.

## Devil's Advocate

Blueprint parity can import Blueprint's debt: graph sprawl, invisible casts, asset lock-in, duplicated macro/function mechanisms, and implicit scheduler behavior. Canvas should copy the authoring power, not the semantic debt.

Manual node positions fight deterministic source projection. The only durable compromise is deterministic layout plus ratified source-anchored layout/comment hints; everything else stays local view state.

Comments are the hardest source-backed feature because free-floating boxes do not map naturally to text. Unsupported comment geometry must be rejected or kept local until a source convention is ratified.

Auto-conversion is dangerous. The graph may help users create conversions, but the resulting source must make the conversion visible unless Jet already has one canonical coercion.

Debugger trust depends on stable source spans. If value bubbles, active wires, and breakpoints drift after formatting, users will stop trusting Canvas immediately.

Extensibility can become a second language. The first public API should expose facts, palette entries, docs, and schemas; behavior-producing node providers need a separate safety and protocol decision.
