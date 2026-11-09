# Apps

Interfaces, web apps, games and larger programs.

Run a lesson from the repository root, for example `jet run Examples/Apps/<Lesson>.jet`. Each lesson's expected output sits beside it as `<Lesson>.out`.

## Game

- [CoreGameHeadless](Game/CoreGameHeadless.jet)
- [DrawCanvas](Game/DrawCanvas.jet)
- [DrawImages](Game/DrawImages.jet)
- [DrawShapes](Game/DrawShapes.jet)
- [DrawSprites](Game/DrawSprites/)
- [ResourceSchedule](Game/ResourceSchedule/)

## Programs

- [Dispatch](Programs/Dispatch.jet) — User story: a parser routes string keys without a chain of guard branches.
- [ExpenseReport](Programs/ExpenseReport.jet) — User story: a shop owner wants a small daily expense report.
- [Failure](Programs/Failure.jet) — User story: a command keeps ordinary failure flow short and typed.
- [FiniteState](Programs/FiniteState.jet) — User story: a build job exposes legal states and legal transitions.
- [InventoryReorder](Programs/InventoryReorder.jet) — User story: a stock clerk wants a quick reorder list.
- [Ownership](Programs/Ownership.jet) — User story: a report reuses read views and makes an owning copy only at a deliberate boundary.
- [SupportQueue](Programs/SupportQueue.jet) — User story: a support lead wants the open tickets that need attention.
- [WireOutput](Programs/WireOutput.jet) — User story: a command emits stable JSON and reads the typed record back.

## UI

- [CoordinateSpaces](UI/CoordinateSpaces.jet) — F04: nominal spaces reject accidental mixing while frame ids keep dynamic scene identity checks at runtime.
- [Events](UI/Events.jet)
- [LayoutBasic](UI/LayoutBasic.jet) — Constraint layout — a Cassowary-style solver.
- [Loadable](UI/Loadable.jet) — Loadable<T,E> — async UI state machine.
- [NodeSlots](UI/NodeSlots.jet)
- [PreviewViewport](UI/PreviewViewport.jet)
- [Reactive](UI/Reactive.jet) — Reactivity is an opt-in LIBRARY, not core semantics.
- [ReactiveScope](UI/ReactiveScope.jet) — `#Reactive { … }` is an explicit reactive effect scope; `reactive.computed` is the canonical name for a derived value.
- [Surface](UI/Surface.jet)
- [TUIPolicy](UI/TUIPolicy.jet)
- [UIA11y](UI/UIA11y.jet)
- [UIComponentKit](UI/UIComponentKit.jet)
- [UIMotion](UI/UIMotion.jet)
- [UINullBackend](UI/UINullBackend.jet)
- [UITUIReactive](UI/UITUIReactive.jet)
- [UITypedStyle](UI/UITypedStyle.jet)
- [UIViewTree](UI/UIViewTree.jet)
- [Window](UI/Window.jet) — A native window drawn by Jet alone, paced by the display; headless without one.

## Web

- [AppHello](Web/AppHello.jet) — Minimal full-stack application builder smoke example.
- [AppTypedArgs](Web/AppTypedArgs.jet) — The checked CLI schema reaches the entry; the app serves on the parsed port.
- [BrowserSurface](Web/BrowserSurface.jet) — A real browser target: the companion HTML is the page under test.
- [FormsTable](Web/FormsTable.jet) — A typed Person form renders its canonical progressive endpoint and feeds the matching App action with the same checked model contract.
- [JetWeb](Web/JetWeb/) — Jet Web reference loop: one typed application graph with one first-party Router/Query/Forms/Table/Virtual/Store state projection.
- [QueryLifecycle](Web/QueryLifecycle.jet)
- [StateSurface](Web/StateSurface.jet) — Headless web state surface.
- [StorageLocalSession](Web/StorageLocalSession.jet) — Local and session storage outside a browser: each keeps one process-wide store, so set, get, has, remove and clear behave as the browser API does.
- [StoreHistory](Web/StoreHistory.jet)
- [UIShowcase](Web/UIShowcase.jet) — Reactive UI flagship: canonical UiNode composition, typed color, motion, signals, DOM rendering, and native/null rendering share one tree mechanism.
- [UIWebClick](Web/UIWebClick.jet) — Jet owns the click listener, counter signal, and render.
- [UIWebMount](Web/UIWebMount.jet) — A reactive core.ui counter built for the web.
- [UIWebReactive](Web/UIWebReactive.jet)
- [WebApp](Web/WebApp.jet) — One statically known full-stack application graph.
- [WebCompute](Web/WebCompute.jet)
- [WebComputeWebGPU](Web/WebComputeWebGPU.jet) — Browser-owned WebGPU compute stays on the shared compute surface while the JS adapter awaits queue submission and readback.
- [WebHello](Web/WebHello.jet)
- [WebWASMCallback](Web/WebWASMCallback.jet) — JS event callback invokes a #WasmExport bridge (JS -> Wasm inside a browser-style handler), proving callback + compute round-trip.
- [WebWASMIntExport](Web/WebWASMIntExport.jet) — Scalar Int Wasm export with a loop-shaped body.
