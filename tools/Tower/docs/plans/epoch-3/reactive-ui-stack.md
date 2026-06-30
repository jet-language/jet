# Reactive UI stack — Tower #134

**Status:** deciding — phases 1+7 buildable now; phases 2-6 need 4 ballot cards
**All primary gates clear:** D-REACTCORE1=D, D-RENDERTGT2=A, D-SIGNAL1=A, D-TYPEDSTYLE1=A, D-MOTION1=A

## What exists already (do not rebuild)

- `JetSignal<T>`, `JetDerived<T>`, `jet_reactive_effect` in `Prelude/CoreLib.rs`
- `#Reactive fn` sema (E2914) + `#Reactive {}` block (TStmt::Reactive)
- `reactive.signal`, `.derived`, `.computed`, `.effect` in sema+codegen
- `JetBackend` trait + `JetNullBackend` + `JetTuiBackend` in `Prelude/Ui.rs`
- `jet_ui_reactive_render` in `Prelude/Ui.rs`
- Geometry: Point, Size, Rect, SizeConstraint, UiNode, JetAriaRole
- Events: JetInputEvent, JetEventResult; Paint: JetPaintCmd
- `core.ui` sema module (CheckerCoreLib.rs:3368) + codegen dispatch
- JS DOM runtime shim (`Prelude/DomRuntime.js`) — partial stubs
- Web codegen stubs (`Codegen/Web.rs`) — partial
- Examples: 161_reactive_scope, 162_ui_null_backend, 165_ui_tui_reactive
- Diagnostics E2910–E2914

**Minor gap:** `"computed"` missing from `KNOWN_CORE_MODULES["jet.reactive"]` export list at `CheckerCoreLib.rs:3360` — Phase 1 cleanup.

## Build order

### Phase 1 — computed alias fix (buildable now)
Add `"computed"` to the `jet.reactive` known-names list in `CheckerCoreLib.rs:3360`.

### Phase 2 — View model layer (BLOCKED: D-UITREE1)
Typed node tree over `core.reactive`. Composable, diff-based subtree rerenders.

### Phase 3 — Typed styles (BLOCKED: D-STYLESHAPE1, D-STYLEUNIT1)
`core.ui.style` module: typed `Style`, `Length`, `Color` value types. Replace raw String color in `JetPaintCmd::FillRect`. Diagnostic range E2920-E2924.

### Phase 4 — Ownable component kit
`jetpack add <component>` copies `.jet` source into user tree. Starter: Button, Label, Input, Container. Requires Phase 2 (View type) + Phase 3 (Style).

### Phase 5 — Motion (BLOCKED: D-MOTIONTIME1)
`core.motion`: spring/tween/keyframes as `Computed<T>` derived from reactive state + Clock. No imperative frame mutation.

### Phase 6 — A11y (BLOCKED: D-A11YGATE1)
`JetUiNode` gains `role: Option<JetAriaRole>` and accessible label. Keyboard focus routing. Release-gated lint diagnostics E2930-E2931.

### Phase 7 — Web/DOM backend (buildable now)
Complete `DomRuntime.js` DOM ops. Complete `Codegen/Web.rs` stubs for measure/layout/paint/on_event via `jetDom.*`. WASM loader. Example `168_ui_web_hello.jet`.

### Phase 8 — Native backends
AppKit/Win32/GTK via `#Extern` C FFI. Each implements `JetBackend`. Platform-gated with `#Target(os)`.

## Open ballots needed

- **D-UITREE1**: typed JetNodeKind enum vs tagged dynamic record (blocks Phase 2)
- **D-STYLESHAPE1**: flat Style record vs builder chain (blocks Phase 3)
- **D-MOTIONTIME1**: injectable Clock param vs global time signal (blocks Phase 5)
- **D-A11YGATE1**: --release flag vs jet lint --a11y profile vs #A11yStrict marker (blocks Phase 6)

Recommended: A for all four (enum, flat record, injectable clock, lint profile).

## Acceptance
Phases 1+7 can close today. Full card done when: view model example with diff rerenders; typed Style usable; component added via jetpack; motion spring with deterministic fake-clock tests; a11y example with keyboard nav golden; web DOM snapshot; one native backend behind #Target guard; no second reactive model anywhere.
