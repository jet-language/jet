# Unified Jet app framework: adversarial review

Date: 2026-11-08. Subject: #6701 c7, `$JET_SCRATCH_ROOT/WaylandLead/DESIGN-app.md`, lines 1–91 as read (snapshot 980F).
Author: WaylandLead, Claude Opus. Fresh adversarial reviewer: AppReview, GPT Sol.
Disposition: **not a passing design review**. Repaired proposal and owner ballots accompany this audit; they are not an implementation or proof of parity. No repository source, git state or Tower record was changed.

## Verdicts

| Claim / recommendation | Verdict | Agreement after repair |
|---|---|---|
| 1. Canvas plus hidden semantic DOM gives identical look/functionality | **Fails** as stated | Prefer shared raster for canonical pixels, conditionally. A transparent accessible projection is viable architecture, not a complete browser integration. Find, selection, autofill and host UI remain real limits. |
| 2. Invert `ui.run(state, update, view)` on both because wasm cannot block | **Holds within limits** for inversion; premise **fails** | Prefer an application-owned driver with expert stepping, not a free-standing state loop or native-only frames. Public API/lifetime timing needs an owner ballot. |
| 3. Bundle Atkinson on both to make text identical | **Holds within limits** | Agree with the owner's font choice. Same bytes alone do not supply shaping, fallback or multilingual functionality. Equal missing-glyph boxes are not parity success. |
| 4. Logical px, per-scale goldens guarantee parity | **Holds within limits** | Agree for deterministic pre-presentation raster. Disagree for physical display identity, arbitrary zoom or byte comparison across different alpha/channel encodings. |
| 5. Browser chrome defaults to Page | **Holds within limits** | Agree. Browser tabs are not app-owned native windows; close, minimize, move and secondary windows need explicit capability/lifetime semantics. |
| One implementation, only present and marshal per target | **Fails** as stated | Shared Core semantics are desirable; substantial browser/native adapters and capability contracts remain. Node byte digests cannot establish flawless user behavior. |

Every gap below is assigned to the repaired design's G1–G20 proof obligations under #6701 c7 and/or D-APP-WEB1/D-APP-RUN1. This audit creates no competing work queue. Main must record the obligations in Tower; this reviewer did not write it.

## Binding decisions checked directly

Read-only `node Tools/tower/tower.mjs decision show <ID>` returned:

- **D-UI-WINDOW-LIFE1=A**, ratified 2026-09-10: `UiApplication` owns windows; one application loop, not per-window loops; close can be cancelled; last-window completion; app-scoped work and teardown. The draft omits this ownership. No ballot has ratified `ui.run` or `frames()` specifically. Inversion still adds a public API; making `run` return a session changes the approved loop-entry/completion contract and must say so.
- **D-NATIVEUI-DEV1=C**, ratified 2026-07-13: one resident app/preview host and swap protocol, production renderer and compatible state identity. The draft's image-only test plan misses that integration.
- **D-PROGRAM-FORM1=A**, ratified 2026-10-02, is stale text/Canvas transaction replay. It does **not** approve an application entry or update/view API.
- **D-ROUTE-FILES2=A**, ratified 2026-10-06, derives page addresses from file-name words. A UI canvas must not replace the route model or discard history/deep links.
- **D-LEARN-WEB1=A**, ratified 2026-10-07, requires the real compiler/exercises in the page. It does not approve a second browser interpreter; I9's later shared-runtime rule still applies.
- **D-NATIVEUI3=A** originally approved mobile. **D-VERDICT-480-1**, ratified 2026-07-13, subsequently defers mobile-native work and focuses language/Linux. Citing D-NATIVEUI3=A itself as a deferral would be wrong. Phone browsers are not mobile-native targets.

AGENTS.md:93–103,133–149,204–212,284–309 were read: one greenfield cutover, I8/I9, owner gates and failed-review blocking rule. The supplied owner directions settle Jet CSD, the single bundled default font, safe Core/Ui, GTK deletion and TUI/null retention; these are not fresh optional recommendations.

## Executed evidence, not prose

Source root for the following relative references: the supplied origin/dev Main-dev-collectall checkout. Lane previews are identified separately. The installed `jet` was used for small probes, not a rebuilt checkout compiler. Thus native/web probe output establishes the exercised toolchain, and inspected files establish that checkout's structure; neither certifies an unbuilt proposal.

### P1: native and web/null

Compiled and ran this probe natively, then `jet build -A probe.jet --target=web --output-dir=<scratch output>` and imported the generated app with Node:

```jet
use {
    ui: core.ui
    app: app
}
run: fn() {
    n: ui.text("hello")
    print("text={n.width}x{n.height}")
    b: ui.null_backend()
    ui.mount(b, n)
    loop command in b.commands() -> print(command)
    print(app.auth_show(app.auth("users")))
}
```

Both executions printed, exit 0:

```text
text=5.0x1.0
text({x:0,y:0,w:5,h:1}, hello)
Auth(users=users, providers=[])
```

This proves a cell-sized text/null command path and process auth formatting, not graphical rasterization, native window behavior or route execution. It also corrects the import: the module is `app`, not `core.app`. Adding `ui.run()` produced `jet.UnknownCoreItem` / E1004. No existing `ui.run` implementation was established. Early inline probes failed because imports belong outside the entry body; these failed attempts supplied no UI evidence.

The small unoptimized web build contained these measured sizes (not a GUI-framework budget or peer comparison):

| File | Raw bytes | gzip | Brotli |
|---|---:|---:|---:|
| app.wasm | 19,437 | 8,187 | 6,792 |
| rt.wasm | 4,594,998 | 1,855,898 | 1,158,339 |
| jet_dom_runtime.js | 38,038 | 10,934 | 9,576 |

No browser first-paint/latency claim follows from these sizes. Rendering, font and semantic machinery would need their own measurements.

### P2: the two DOM paths

Imported the checkout's `Compiler/Prelude/Web/DomRuntime.js` with Node; `measure`, `layout`, `paint`, `commands` on a five-unit text node returned `{width:5,height:1}` and the same text command. This was headless: no accessibility, selection, IME or browser-layout behavior was exercised.

Separately extracted and evaluated the embedded JS from `Compiler/JetWeb/Source/Web/Host.jet` (WEB_DOM_RUNTIME_JS, lines 78–1060). Invoking its `Host.paintScene` with a minimal DOM double and runtime scene bytes produced a `span` with `left:0px`, `top:0px`, `width:5px`, `height:1px`, text `hello`. This exercises the current emitted scene adapter, not a canvas leaf. Source lines 275–307 instantiate the two wasm modules without JSPI wrappers; lines 370–406 remove/recreate DOM leaves. Updating only the older DomRuntime path misses this cutover.

`defaultUiProvider` (Host.jet:165–233), exercised with a clipboard promise that rejects, returned `true` immediately and retained the submitted text after rejection. Output: `clipboard immediate true secret`; `clipboard after denial secret`. It swallows the rejection at lines 223–224. That contradicts any claim that the current path reports clipboard denial correctly. Composition-end emitted `5:field14:compositionend2:é0:0:0:0:`: ranges absent, not a complete editing transaction.

### P3: wasm can suspend; Jet integration is not established

A small raw wasm module importing `h.f: () -> i32` and exporting a caller compiled with `WebAssembly.instantiate`. Wrapped the import with `new WebAssembly.Suspending`, resolved a delayed promise to 42, and wrapped the export with `WebAssembly.promising`. Output: **`JSPI result 42 eventLoopAdvanced true`**, exit 0. Node also reported both functions present and a one-millisecond `Atomics.wait` returned `timed-out`.

This disproves “wasm cannot block” as a universal statement. The browser main thread must yield; wasm may suspend, or a worker may wait. Searching the inspected compiler/Core for JSPI, promising, Suspending and Asyncify found no integration; the generated-host instantiation was inspected too. `Host.jet:135–141` calls Atomics.wait and catches its failure, returning immediately on a page main thread rather than suspending. A raw wasm success does not prove preservation of Jet's explicit ABI stack, ownership, task cancellation or reentrant host calls. JSPI browser support must be feature-detected and exercised, not asserted from Node.

### P4: font coverage

Fetched Google Fonts' AtkinsonHyperlegibleNext[wght].ttf, 114,552 bytes, SHA-256 `5a455d1cfa099b601ab70751bb9673e8fe1854dc4500c80e1a220d0d75e31745`. A Node cmap-format-4 lookup returned glyph 1 for `A`, 134 for `é`, and 0 for `Ж`, `α`, `中`, `ع`. Google Fonts metadata lists latin and latin-ext. This checks that upstream asset, not an unidentified future Jet-bundled asset. Pin and inspect the actual bundled asset before claiming its coverage. “Latin-only” is script coverage, not “English-only”: Braille Institute advertises over 150 languages.

No browser executable was found by `command -v chromium chromium-browser google-chrome firefox`. No real browser, screen reader, native IME, compositor, password manager or colorimeter was exercised. Their gaps remain open; a DOM double never substitutes for those proofs. No suites ran.

## Source findings and exact draft repairs

| Draft lines | Error, unfair framing or missing requirement | Repair / gap |
|---|---|---|
| 3,24,71 | “Only pixels/raw input differ” and “pixel-identical page” confuse canonical raster with browser/user behavior. Even draft layers 6–8 include large host services. | State shared semantics and explicit adapter/capability limits; G1–G12. Do not promise one tiny leaf. |
| 6 | Cell-sized nodes are supported by Ui.jet:143–153 and P1. “Three backends” names exported paths, not working GTK pixels. | UiBackends.jet:13–16,699–702 has an unavailable/headless GTK path. Do not imply a functioning graphical GTK baseline. P1 does not test GTK. |
| 7 | Native provider absence is source-backed (UiHostProvider.jet:18–25); browser provider presence is not capability success. | Preserve typed capability semantics; test permissions and actual results, G11. |
| 8 | Correct for the directly imported older JS path, incomplete for the emitted web target. | Account for Host.jet/UiBackends scene path too (P2). DomRuntime.js:2014–2038 has composition/input handlers, but no demonstrated exactly-once edit semantics. G2/G20. |
| 9 | App.jet is process live/sync/auth code (1–24,48–79,101–126), not the entire route graph. “Unaffected” is unproved integration. | Preserve app/web auth, routes/forms/history and reactive state; don't rename/replace App as UI bootstrap. |
| 10 | Lane preview is not origin/dev. Its UIEvent pointer coordinates are physical Int pixels, not proposed logical floats. | Identify preview provenance; Ui preview:17–20,104–119. Do not assert it already implements the proposed normalized model. |
| 15–19 | Stable identity, layout, font, display list, raster reuse are proposed, not present/proven. Same font alone cannot fix bidi, shaping, glyph coverage or rounding. | Shared pipeline, explicit keys, safe resource ownership; G2/G8/G15/G20. No benchmark claim for Raylib reuse. |
| 19–24,81–86 | Native-endian premultiplied ARGB/BGRA and browser straight RGBA digests cannot simply be equal. Premultiplication loses information at low/zero alpha. | Compare one canonical encoding before host projection; separately verify conversion and compositing; G8. |
| 27–34 | Browser `event.key` is the produced key, not the native UIKey's unshifted level-1 symbol. A code table cannot recover missing physical/layout info. “Reserved chords never arrive” is too absolute across hosts. | Retain unknown raw facts, normalize actions/text, test non-US/AltGr/Shift/blur/repeat; no fabricated equality. G2/G20. |
| 35–38 | Preedit + Commit lacks deletion/replacement ranges, surrounding text, cancel/serial, reconversion, dictation and browser mutation ordering. | Single transaction/undo model, unit conversions and retained transport control; G2. |
| 39–42 | Wheel deltaMode normalization does not supply source/phase/momentum parity. Touch “later” excludes phone browser use, not merely deferred mobile-native. | Shared scrolling state, precise host deltas, no double inertia; pointercancel/capture/touch contract; G9. |
| 43–44 | Shared focus/shortcuts need screen-reader actions, browser focus, activation duplication, reserved commands and gesture-time permission calls. | One semantic action dispatcher, focus restoration, browser escape paths; G1/G10/G11. |
| 45–47 | “Same async result” is not the current API: Clipboard.jet:10–23 is synchronous. Generated provider swallows rejection (P2). | Async completion/denial must be a public cutover, not false current behavior; G11 and run ballot. |
| 48–50 | A role/name snapshot is not an accessible app. Deferring native AT-SPI while claiming full functionality is a dropped hard case. | Real assistive tests and full actions/relations/live regions/virtualization before parity sign-off; G1. |
| 51–58 | Tab title/favicon are not window controls. Page Close cannot be a reliable cancellable tab-close event. InPage maximize is not browser maximize/fullscreen. SSD may be unavailable. | Explicit document-window operations/capabilities; G12/G16; owner chrome clause. |
| 62–68 | “Hidden” may remove semantics entirely. Text spans do not automatically provide selection, visible browser Find or password-manager integration. Ctrl+F/zoom hijacking removes native browser behavior. | Transparent accessible retained nodes, real controls/anchors; preserve browser Find/zoom. State irreducible gaps honestly, G1–G7. |
| 68 | Damage rects do not remove full-window animation cost, wasm startup, upload copies or semantic DOM work. GPU later is not a present solution. | Retained tiles and semantic diffs; measure cold start, bandwidth, AX cost, p95 latency; G5/G6. |
| 69 | DOM need not have two hand-authored layouts/themes: project shared geometry/styles into CSS. Browser text/edit differences still remain. | Strengthen B fairly; G17. React Native for Web is a serious existing counterexample to the “written twice” framing. |
| 70–71 | Hybrid is under-specified, not merely a bad-looking text field. Canvas+semantic controls already shares part of this complexity. A is not uniquely capable of one authored UI. | Specify overlay/transaction ownership, G18. Add browser-engine-on-both-targets alternative D and its dependency/cost gate. |
| 72–75 | False wasm premise; omits explicit cooperative stepping/continuations/worker path; standalone run/native-only frames can break I8/I9 and application ownership. | D-APP-RUN1, G13/G14; one app driver and one semantic step, expert scheduling available on both. |
| 76 | Correct font direction, incomplete choice set: shared expert font assets and shared fallback packs are missing. Global text cannot wait behind identical tofu. | Font owner clauses and coverage tests, G15; no unapproved default fonts. |
| 77 | CSS×DPR may be fractional; pinch zoom still resamples; page zoom changes viewport/DPR. Four goldens cannot prove arbitrary scales or physical color. | Actual backing dimensions/effective ratios, resize/DPR/visualViewport handling, browser zoom preserved; G7/G8. |
| 78 | Page is sensible, not literal chrome parity. | Owner-gated exact behaviors and expert chrome override; G12/G16. |
| 80–91 | Node stub, fake compositor bytes and semantic snapshots miss actual browser/AT/IME/permissions/latency/hot reload. Hostile native keymap input has no matching browser raw input. | Separate canonical proofs from host conformance and real integration. Translate logically equivalent actions, not impossible raw transcript equality. G1–G20. |

Core/Ui/Ui.jet:159–161,195–208,216–219 contain #Unsafe wrappers. Their removal and the runtime-backed null/TUI conversion are explicit scope, not something already made safe by a backend-neutral tree.

## Strongest existing solutions, fairly compared

These are vendor documentation/source findings, not independently benchmarked or compiled competitor applications. Sources were opened; no snippet below is offered as measured peer superiority. The repaired proposal must earn its claims against matched workloads.

| Solution and source | Best case / what Jet's draft lacks | Relevance to claims |
|---|---|---|
| **Flutter CanvasKit/skwasm**: [web architecture](https://docs.flutter.dev/platform-integration/web), [semantics](https://docs.flutter.dev/ui/accessibility/web-accessibility), [HTML removal rationale](https://github.com/flutter/flutter/issues/145954), [web FAQ](https://docs.flutter.dev/platform-integration/web/faq) | Shared widget/paint model with GPU browser rendering and a semantic DOM projection is not novel. Flutter's documented semantics enablement reflects real cost, unlike an assumed free mirror. Its rendering ecosystem, text/edit integrations and explicit app-vs-document/SEO limits are stronger than Jet's proposed rect/text projection. Its HTML renderer was considered complex, underperforming, graphically limited and costly to maintain alongside matured WebGL renderers—not removed because HTML cannot be accessible or cannot support shared app code. Feedback includes startup, CORS and platform-view costs; do not use removal as proof all DOM UIs are bad. | 1: closest A precedent; 2: framework-owned callback tree; 3: shared fonts still need fallback; 4: portable rendering not physical-screen identity; 5: browser remains host. |
| **Compose Multiplatform web**: [shared Compose vs Compose HTML](https://raw.githubusercontent.com/JetBrains/compose-multiplatform/master/README.md), [web release](https://blog.jetbrains.com/kotlin/2025/09/compose-multiplatform-1-9-0-compose-for-web-beta/), [platform requirements](https://kotlinlang.org/docs/multiplatform/compose-compatibility-and-versioning.html) | Common declarative UI on Kotlin/Wasm, Material/adaptive components, animations, HTML interop, history/deep links, browser preferences and accessibility support are documented. Jet's draft omits these integration layers. Compose HTML is Kotlin/JS and web-only, not an interchangeable multiplatform renderer. Vendor docs have different moving-version labels; no current stability/version claim is needed here. | 1/2: strong same-source alternative; 3/4: resource/adaptive requirements beyond a font/DPR; 5: browser navigation not desktop decorations. |
| **Slint**: [API/event-loop source](https://raw.githubusercontent.com/slint-ui/slint/master/api/rs/slint/lib.rs), [winit renderers](https://raw.githubusercontent.com/slint-ui/slint/master/internal/backends/winit/lib.rs), [wasm input bridge](https://raw.githubusercontent.com/slint-ui/slint/master/internal/backends/winit/wasm_input_helper.rs) | Compiled retained components, reactive properties/models, selectable software/GPU renderers and explicit event-loop integration are strong comparators. The input source has a real hidden-input/composition/clipboard bridge; Jet's two-event IME sketch is not equivalent engineering. This is not proof Slint guarantees all browser accessibility/IME behavior. | 1–4: shared scene/resources with concrete host complexity; 2: app event-loop and async integration; 5: platform adapter remains. |
| **egui/eframe**: [API and feature documentation](https://docs.rs/eframe/latest/eframe/) | Same App UI callback across native/web, bundled fonts and custom graphics, native loop embedding via create_native, and explicit web runner exist. It documents native AccessKit vs web screen-reader support and native-only system fallback; do not pretend its support is uniform. Jet's draft lacks comparable expert loop embedding and graphics integration. | 1/2: shared rendering does not require one entry implementation; 3: bundled fonts/fallback distinction; 4: device integration; 5: native/browser runners differ. |
| **Avalonia browser**: [browser platform options](https://docs.avaloniaui.net/docs/platform-specific-guides/webassembly) | Existing retained UI and .NET ecosystem in wasm, GPU WebGL with Software2D alternatives, worker-dispatch configuration, JS interop and file-dialog choices. Jet's proposal supplies none of these as exercised features. Runtime download/interop cost is real, not grounds to dismiss the option. | 1/2: substantial browser host and scheduler, not merely present; 3/4: common UI still needs resource/renderer controls; 5: browser platform options. |
| **Qt for WebAssembly**: [rendering, fonts, accessibility, loops, Asyncify/JSPI](https://doc.qt.io/qt-6/wasm.html) | Qt Widgets/Quick, window hosting inside container screens, font APIs, clipboard/files, shadow HTML accessibility and JSPI/Asyncify approaches are documented together with limits. This is the strongest direct rebuttal to the forced inversion argument. Its default stack/exit workaround is not a safe-Jet lifetime model to copy: the docs explicitly describe skipped destructors unless using a clean async structure/suspension. | All five: mature common UI with explicit host limitations and in-page windows; font packs and zoom tests still needed. |
| **React Native for Web**: [architecture](https://necolas.github.io/react-native-web/docs/) | React DOM presents shared View/Text/TextInput/ScrollView with generated CSS, localization, gesture abstractions and selective modules. Browser-native controls/document semantics are B's best case. One app source need not hand-write every layout/style twice; native and DOM behavior can still differ. | 1: stronger B; 2: declarative state callbacks; 3/4: generated CSS/font/layout not pixel guarantee; 5: browser document/chrome integration. |
| **Tauri**: [webview engines](https://v2.tauri.app/reference/webview-versions/) | Reuse a web UI in desktop system WebViews with host integration rather than inventing a raster/semantics bridge. It avoids bundling an entire engine but accepts WebView2/WebKit/version differences. Jet's D alternative gains browser editing/document facilities but needs a dependency/desktop architecture ballot; it is not safe-Jet-only rendering. | 1–4: web-engine alternative missing from draft; 2: host loop/lifetime; 5: native wrapper vs tab remains distinct. |
| **Electron**: [Chromium process/window model](https://www.electronjs.org/docs/latest/tutorial/process-model) | A controlled Chromium desktop runtime reuses DOM/browser text, rendering and tooling; BrowserWindow and main-process lifecycle/native integration are explicit. Package/process/security/update costs remain, and the visiting browser is still not pinned. Jet's draft has neither this breadth nor evidence for lower matched cost. | All five: D's strongest fidelity/tooling case, not perfect web/native host parity. |

For each recommendation, use the relevant row's full best case rather than an easy “system fonts” or “write it twice” strawman. None proves flawless parity. A pixel framework, native-widget framework and browser-document framework make different valuable trade-offs.

## Owner choices and posting limits

1. **D-APP-WEB1**: A shared raster + always-on semantic DOM (conditional recommendation); B real semantic DOM presentation; C canvas with visible DOM editors; added D browser document engine on both targets. Same complete notes program, beginner/expert paths, costs after mitigation and real-browser criteria are in the payload/design. B/C require explicit visual/editing parity relaxation; D requires dependency/desktop scope changes. A's browser Find/autofill/host integration must not be called solved by the ballot.
2. **D-APP-RUN1**: A managed application driver over shared step (recommend); B explicit step-only program; added C sequential application loop with suspension on both targets. All preserve application-owned windows and close veto. A/B explicitly amend `run` completion timing; C must prove stack/cleanup/browser support. No native-only per-window frames escape hatch.
3. The repaired proposal also supplies ballot-ready **font coverage**, **canonical-vs-physical scale parity**, and **Page-vs-InPage chrome** clauses. Font embedding itself is already directed; extra default packs/host fallback are owner changes, not unapproved implementation options. Physical/browser capability limits also need consent, not scope reduction by an agent.

Do not post the original design, claim “pixel-identical page”, “only present/marshal”, “wasm cannot block”, “native AT-SPI later” as full parity, or claim a passing adversarial review. Do not present proposed API notation as compiling code. The two JSON files are separate decisions because the current validator has one options/rec set per payload, not independently votable nested questions.

The repaired proposal concretely mitigates every identified downside with mechanisms, residual costs and tests (G1–G20); those tests are obligations, not claimed results. Real browser/assistive evidence and unresolved owner choices prevent sign-off. Schema validation does not establish technical correctness or owner approval.

Payload validation was executed from the repository checkout, exit 0:

```text
jet Tools/agent/ballots/Validate.jet $JET_SCRATCH_ROOT/Main/review-app/D-APP-WEB1.json $JET_SCRATCH_ROOT/Main/review-app/D-APP-RUN1.json
ok $JET_SCRATCH_ROOT/Main/review-app/D-APP-WEB1.json (D-APP-WEB1)
ok $JET_SCRATCH_ROOT/Main/review-app/D-APP-RUN1.json (D-APP-RUN1)
```

Paths above are normalized to the required scratch variable. The first validation rejected raw newlines in JSON strings; those strings were escaped before the successful run. Probe source/output was removed after its observations were recorded here. Deliverables are documentation and ballot payloads only.
