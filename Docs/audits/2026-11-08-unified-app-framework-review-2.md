# Unified Jet applications — second adversarial review

2026-11-08. Author: WaylandLead (Claude Opus). First reviewer: AppReview (GPT Sol). Second, fresh reviewer: AppReview2 (OpenAI GPT).

Scope: `DESIGN-app.repaired.md`, `D-APP-WEB1.json`, `D-APP-RUN1.json`. Edited only those and this audit. Repository and Tower were read-only; no git operations. This is a direction review, **not a passing implementation/parity review**.

## What the first review missed, and what changed

1. **A mitigation was too easily read as a fix.** All mechanisms are now explicitly implementation/proof requirements. Retained DOM does not prove accessibility; matching null commands does not prove pixels. No browser, performance or superiority claim passes on these probes.
2. **First paint was underspecified.** Compressed/parallel assets do not remove transfer, decode, compile, initialization or first-visit latency. G5 now separates readable static content from a usable frame, specifies one read-only startup handoff, failed-load behavior and atomic hashed deployment. No second interactive HTML renderer is smuggled into A. B's absolute boxes do not automatically provide fluid document reflow or functional no-JS forms.
3. **Damage/caches could conceal power and memory costs.** Added G21/G22: demand/deadline scheduling, hidden-frame suspension, bounded catch-up, cache budgets/eviction, released surfaces and retained-memory measurements. Always-on searchable DOM/AX/text still scales with document size; wasm capacity can retain its high-water mark. Measure battery energy, not just CPU. No unmeasured benefit is claimed.
4. **Zoom and scroll feel were not solved.** Page zoom, pinch, text-only zoom and keyboard occlusion are different inputs. G7/G9 now distinguish layout/visual viewports, map offsets once, require shared text scaling, gesture arbitration and real-device scroll tests. DPR observation alone cannot make canvas respond to text-only zoom. Main-thread scrolling and unmatched momentum metadata remain costs.
5. **Bidi and user preferences were missing contracts.** G23/G24 specify pinned direction/segmentation data, visual/logical caret mapping, real script fonts, forced-colors palettes, shared focus/selection paint and reduced motion. A transparent mirror can become visible under forced colors; narrowly protecting duplicate paint is not permission to disable the user's palette. Latin-only default assets still cannot provide global script coverage.
6. **Navigation and focus were slogans.** G25/G26 reuse the existing router and one focus identity; define URL/history restoration, tab escape, modal background inertness and focus return. Back/popstate is not a cancellable Jet close event. No second UI router, positive-tabindex ordering or history trap.
7. **Mobile keyboard support was not implied by IME events.** G27 requires a real focused editor during trusted input, input hints and caret reveal under visual-viewport changes. Scripted focus, auto-zoom, keyboard UI and dictation remain browser-dependent. Phone browsers are not excluded by the mobile-native deferral.
8. **Printing and extensions were absent.** G28 derives a full, noninteractive print document from the same semantic content; screenshots of a virtual viewport are insufficient. G29 names translation/style-extension limits rather than assuming DOM metadata repaints Jet glyphs. B/D expose more browser-native document facilities but cannot promise arbitrary extensions or embedded password/print services.
9. **The examples undersold their own workload.** Three rows were called a long list; movable items lacked keys; close events used titles as identities; “Saved” only cleared a flag. All WEB specimens now contain the same 1,000 uniquely keyed rows and stable window IDs, label unsupported scripts and call the demonstration action “Mark clean”, not disk persistence. RUN=A is explicitly held constant for comparison, not selected by WEB. All RUN specimens have identical state/update/view/close behavior. All remain proposed notation, not executable evidence.
10. **Returning a native session was not enough.** G31 defines host-dispatcher retention after entry return, initial-ready registration ordering, one-use ordered batches, empty/hidden windows, typed failures, late-event generations and cleanup. Per-window veto preserves resources; explicit shutdown keeps the ratified close-all meaning. No new application-wide veto is introduced.
11. **Expert scheduling conflicted with gesture-time effects.** G32 now requires privileged batches to step in the trusted callback. Arbitrary deferral, a worker proxy or suspension cannot promise preserved activation. Cancellation is cooperative, not safe forced destruction of running code. All RUN alternatives name these costs.
12. **Find mitigation was misleading.** `::selection` is not browser Find. A retained transparent document does not prove visible browser-menu matches or correct reveal/scroll. This remains a parity failure, not a solved bridge.
13. **Native context was treated like an unlimited string.** The checked text-input-v3 protocol caps surrounding text at 4,000 UTF-8 bytes. G2 and the WEB ballot now require a tracked context origin, valid byte boundaries, directional selection and serial handling, and name long-selection/reconversion limits.

No new option was needed: the first review already added WEB=D and RUN=C. I strengthened B's document/scroll/tooling case and D's reuse case, while making D's conflict with safe-Jet-only rendering explicit. Both ballots retain AppReview followed by AppReview2 in `reviewPasses`.

## Authority checked directly

Read the repository contract first, then the supplied checkout's contract: I8/I9 and adversarial-review requirements agree. Read `Docs/spec/philosophy.md`. Read-only `node Tools/tower/tower.mjs decision show <ID>` established:

- **D-UI-WINDOW-LIFE1=A:** application-owned independent windows, cancellable window-close requests, one loop, last-window completion, explicit shutdown closing owned windows and releasing work. A/B must explicitly amend `run` timing; they do not amend these lifetime rules. Closed handles must return typed errors.
- **D-NATIVEUI-DEV1=C:** one resident app/preview host and checked swap protocol, production backends and identical state identity. Preservation remains a requirement, not a demonstrated cutover.
- **D-PROGRAM-FORM1=A:** replay stale text/Canvas edits; not application entry approval.
- **D-ROUTE-FILES2=A:** file-name words derive addresses; do not replace the route mechanism.
- **D-LEARN-WEB1=A:** same exercises and real compiler in the page. Its older interpreter alternative cannot override the supplied I9 contract.
- **D-VERDICT-480-1:** defers mobile-native work, not mobile browsers.

Owner instructions establish first-party drawing, Jet chrome, the single Latin-only default font and safe Core. B/C/D and host-capability/default-chrome boundaries require explicit amendments/acceptance, not an agent's quiet narrowing of “exactly the same”. Font/scale/chrome clauses remain proposed owner choices, not decisions already ratified by these two ballots.

## Independently exercised evidence

Paths below are relative to the supplied origin/dev checkout. `jet` is the installed toolchain, not a newly rebuilt checkout compiler. Only small probes and the requested payload validator ran; no suites or peer benchmarks.

### P1 — native/current UI

Compiled and ran through `printf ... | jet run -A -`:

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

Exit 0:

```text
text=5.0x1.0
text({x:0,y:0,w:5,h:1}, hello)
Auth(users=users, providers=[])
```

An initial malformed one-line import failed parsing and supplied no behavior evidence. A separate compiled probe, `use ui: core.ui` plus `run: fn() { ui.run() }`, exited 1 with `jet.UnknownCoreItem`, E1004. No proposed driver was thereby implemented or tested.

Source checks: `Core/Ui/Ui.jet:143–153` computes cell-based text/button dimensions; `159–161,195–208,216–219` contains unsafe wrappers. `Core/App/App.jet:1–24,48–79,101–126` contains process/auth/live/sync helpers, not the proposed UI bootstrap. `Compiler/JetRuntime/Source/UiBackends.jet:699–702` selects GTK headless/unavailable state; no graphical GTK run was claimed. `UiHostProvider.jet:18–25` defines declining native provider leaves. These inspections establish cutover obligations, not working new services.

### P2 — both JS paths, not browser proof

Imported the checkout's `Compiler/Prelude/Web/DomRuntime.js`, called `createBackend`, `makeText("hello")`, `mount`, `measure` and `commands` under Node. Output:

```text
legacy DOM {"width":5,"height":1} [ 'text({x:0,y:0,w:5,h:1}, hello)' ]
```

Separately extracted the `WEB_DOM_RUNTIME_JS` literal from `Compiler/JetWeb/Source/Web/Host.jet`, imported it in-memory with test exports, and called `Host.prototype.paintScene` with runtime scene bytes `1\t0\t0\t0\t5\t1\t\t5\thello\n` and a minimal DOM double. It created a `span`, absolute `left/top=0px`, `width=5px`, `height=1px`, text `hello`. `Host.jet:370–406` rebuilds scene leaves; `275–307` instantiates wasm without JSPI wrapping. `DomRuntime.js:1837–1871` supplies the other path. Both must be accounted for; neither proves an accessible browser app.

Exercised extracted `defaultUiProvider` with a rejecting `navigator.clipboard.writeText`, then emitted compositionend. Output:

```text
clipboard immediate true secret
clipboard after denial secret
composition 5:field14:compositionend2:é0:0:0:0:
```

`Host.jet:165–233` confirms the swallowed rejection and missing composition ranges. A typed asynchronous permission/editing cutover is necessary; “same outcome today” would fail.

### P3 — JSPI, memory and raster arithmetic

Compiled/instantiated a minimal raw wasm module under Node:

```js
const bytes = Uint8Array.from([0,97,115,109,1,0,0,0,1,5,1,96,0,1,127,
  2,7,1,1,104,1,102,0,0,3,2,1,0,7,7,1,3,114,117,110,0,1,
  10,6,1,4,0,16,0,11]);
let tick = false;
setTimeout(() => tick = true, 0);
const {instance} = await WebAssembly.instantiate(bytes, {h: {
  f: new WebAssembly.Suspending(() => new Promise(r => setTimeout(() => r(42), 5)))
}});
console.log(await WebAssembly.promising(instance.exports.run)(), tick);
```

Observed `JSPI result 42 eventLoopAdvanced true`, exit 0. This is not Jet ABI/ownership integration or browser-support evidence.

A Node `WebAssembly.Memory({initial:1, maximum:4})` grew from 65,536 to 196,608 bytes; `grow(-1)` threw TypeError. Cache eviction therefore cannot be presented as proof that linear-memory capacity shrinks.

Independent arithmetic: `3840*2160*4 = 33,177,600` bytes; at 60 full uploads/s, `1,990,656,000` bytes/s before extra copies. This is bandwidth arithmetic, not observed Jet traffic. Integer alpha roundtrip with channel 200, nearest premultiplication/unpremultiplication, produced:

| Alpha | Premultiplied | Recovered |
|---|---:|---:|
| 0 | 0 | 0 |
| 1 | 1 | 255 |
| 127 | 100 | 201 |
| 255 | 200 | 200 |

Thus canonical comparison must define its encoding before host conversion. No physical-display equality follows.

No browser or assistive/IME device was exercised. `command -v chromium chromium-browser google-chrome firefox` found none on PATH. Browser-specific behavior below is a documented constraint or proposed test, **not an observed Jet pass/failure**. No new wasm bundle was built or timed; first-review sizes were not reused as independent measurements. The future bundled font was not available/pinned: exact glyph coverage is deliberately not certified.

## Strong alternatives and external limits checked

Opened these sources directly. These are vendor/platform documentation findings, not executed competitor comparisons:

- [Flutter semantics](https://docs.flutter.dev/ui/accessibility/web-accessibility): shared drawing plus accessible HTML is established architecture; its documented opt-in performance cost rebuts a free mirror. [Web FAQ](https://docs.flutter.dev/platform-integration/web/faq): app-centric strengths and document/SEO limits. A must earn comparable integration, not dismiss it as a small adapter.
- [React Native for Web](https://necolas.github.io/react-native-web/docs/): shared View/Text/TextInput/ScrollView, localization and generated CSS. This is B's strong case: one authored program can use DOM without hand-writing two layouts/themes. It does not prove Jet's proposed absolute boxes inherit all document behavior.
- [Qt WebAssembly](https://doc.qt.io/qt-6/wasm.html): in-page windows, host services, accessibility and sequential suspension with costs. This is the strongest direct comparator for RUN=C and browser lifetime boundaries.
- [Electron process model](https://www.electronjs.org/docs/latest/tutorial/process-model) and [Tauri engine policy](https://v2.tauri.app/reference/webview-versions/): browser engine reuse is a serious D alternative. Process/security/deployment work and system-version variation remain. A smaller Jet package or better battery result is unmeasured.
- MDN [forced colors](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/forced-colors), [reduced motion](https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-reduced-motion), [VisualViewport](https://developer.mozilla.org/en-US/docs/Web/API/VisualViewport), [Page Visibility](https://developer.mozilla.org/en-US/docs/Web/API/Page_Visibility_API): inputs require actual adaptation; palette CSS is not a canvas renderer. Pinch/keyboard changes can leave layout viewport unchanged; background scheduling is host-controlled.
- MDN [popstate](https://developer.mozilla.org/en-US/docs/Web/API/Window/popstate_event), [beforeunload](https://developer.mozilla.org/en-US/docs/Web/API/Window/beforeunload_event), [printing](https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_media_queries/Printing), [selection](https://developer.mozilla.org/en-US/docs/Web/CSS/::selection): history notification, unreliable generic unload warning, print projection and selected-text styling do not supply Jet close veto or browser Find control.
- [Wayland text-input-v3](https://wayland.app/protocols/text-input-unstable-v3): surrounding text, byte offsets, purpose/hints, deletion and commit/done semantics are protocol facilities, not evidence Jet already integrates them. Byte-offset/window limits require mapping, not a whole-string transport assumption.

## Recommendation, posting and validator result

I agree with **RUN=A**, conditional on the explicit timing amendment and G31/G32 contract. B has a genuinely smaller surface; C has genuinely better sequential control flow. Neither is forbidden or inherently slower. A's wrapper is not assumed free.

I agree with **WEB=A only as the closest direction to the owner's first-party drawing constraint**, not as a demonstrated route to unrestricted browser parity. If native browser document behavior is non-negotiable, B/D deserve reconsideration; A cannot bury its Find/extension limits. C improves text controls but does not solve the rest of the document. No option satisfies every original constraint unchanged.

Font A follows the owner's instruction with honest coverage limits; scale A is defensible only for canonical pixels; Page chrome A is practical but requires acceptance of its different default look/capabilities. No vote on these is implied by WEB/RUN.

Ran from the checkout, exit 0 for both payloads:

```text
jet Tools/agent/ballots/Validate.jet $JET_SCRATCH_ROOT/lanes/Main/review-app/D-APP-WEB1.json $JET_SCRATCH_ROOT/lanes/Main/review-app/D-APP-RUN1.json
ok .../D-APP-WEB1.json (D-APP-WEB1)
ok .../D-APP-RUN1.json (D-APP-RUN1)
```

Do not post the superseded design, a passing implementation review, a benchmark win, “flawless parity”, “Find solved”, “free semantic mirror”, “safe cutover complete”, or proposed APIs as compiling Jet. Do not post a claim that schema validation approves the technical design. G1-G32 remain proof obligations attached to these direction ballots, not a separate implementation approval or work ledger.

## Final verdict per claim

“Holds within stated limits” below means a defensible proposed mechanism with named residual cost, **not** an exercised implementation. Missing integration/measurements do not receive a runtime pass.

| Claim / mechanism | Verdict | Limit or decisive evidence |
|---|---|---|
| Present unrestricted flawless parity as achieved/guaranteed | **fails** | No option controls all browser/OS behavior; real integration evidence absent. |
| Current null text dimensions/commands and missing ui.run | **holds** | Independently compiled P1; not graphical framework evidence. |
| Both web presentation paths need cutover | **holds** | P2 exercised both, source owners inspected. |
| One safe shared semantic/raster implementation already exists | **fails** | Proposed only; current unsafe wrappers remain. |
| Shared Core pipeline, keys and one semantic authority | **holds within stated limits** | I8/I9-compatible requirement; no new raster/shaping proof. |
| Canonical sRGB/alpha comparisons imply physical screen equality | **fails** | P3 lossy roundtrip; host color/output constraints. |
| Owned windows and explicit run-timing amendment | **holds within stated limits** | Ratified life law preserved; dispatcher/session integration unproved. |
| WEB B/C/D are fair same-program alternatives | **holds within stated limits** | Written out; explicit parity/dependency amendments, no false equal-compliance claim. |
| RUN B/C are serious alternatives | **holds within stated limits** | Same reducer/program; C's Node JSPI proof is not Jet/browser integration. |
| G1 retained semantic controls and shared accessible actions | **holds within stated limits** | Real AT/focus actions still required; O(N) document/AX cost. |
| G2 transaction/range bridge with bounded native context | **holds within stated limits** | P2 lacks current ranges; 4,000-byte native context and noncancellable browser edits retain limits. |
| G3 transparent text plus selection styling solves browser Find | **fails** | No demonstrated visible matches/reveal; repaired text names this blocker. |
| G4 real password controls improve autofill integration | **holds within stated limits** | Metadata helps; named managers, secrecy and browser policies need testing. |
| G5 strip/compress/stream/static HTML mitigates startup/SEO costs | **holds within stated limits** | First usable paint differs from readable HTML; transfer/compile/init and indexing remain unmeasured. |
| G6 damage/caches reduce unchanged-region raster/upload work | **holds within stated limits** | Only changed regions benefit; P3 full-frame arithmetic remains. |
| G7 separate DPR, layout, visual viewport and text-scale handling | **holds within stated limits** | Separate text scale and pinch mapping required; unsupported host preferences named. |
| G8 canonical format solves channel/alpha ambiguity | **holds within stated limits** | Must specify exact integer rules and test host projections; P3 rejects lossless roundtrip assumptions. |
| G9 shared scroll state with honest host gesture limits | **holds within stated limits** | Host momentum/source facts and scheduling differ; real-device comparisons required. |
| G10 real anchors preserve ordinary link behavior | **holds within stated limits** | Must test modifiers/context menus; host chrome remains external. |
| G11 existing clipboard reports actual success | **fails** | P2 returns true then swallows denial. Proposed async cutover is not current behavior. |
| G12 incremental draft persistence mitigates abrupt loss | **holds within stated limits** | Storage failure/eviction and abrupt loss remain; no reliable veto/cleanup promise. |
| G13 shared step supports beginner/expert control | **holds within stated limits** | One mechanism; callbacks/tasks and cooperative cancellation still cost control flow. |
| G14 JSPI makes sequential Jet UI proven | **fails** | P3 only proves a raw wasm call; ABI, support, cleanup and activation remain unproved. |
| G15 shared font bytes/resolver with explicit missing coverage | **holds within stated limits** | Owner's Latin-only default; actual bytes/coverage must be pinned, extra assets explicit. |
| G16 shared frame actions with typed host capability limits | **holds within stated limits** | In-page operations only; unsupported native/host capabilities explicit. |
| G17 shared CSS boxes remove B's second author layout | **holds within stated limits** | Browser internal text/control layout, styles and reflow remain different. |
| G18 retained overlays remove C's double paint/teardown | **holds within stated limits** | Requirements, not tested fixes; clipping/composition/editor differences remain. |
| G19 explicit engine/security policy prices D's dependency | **holds within stated limits** | Names cost and approval; neither pinned nor system engine controls visiting browsers. |
| G20 shared inspection model and complete caller cutover | **holds within stated limits** | Jet inspection does not become DOM/CSS visual tooling; all callers still need migration. |
| G21 demand scheduling avoids unnecessary frames | **holds within stated limits** | Joules/task and thermal behavior unmeasured; necessary work still consumes energy. |
| G22 cache budgets/eviction mitigate retained memory | **holds within stated limits** | Do not bound document/AX size or shrink wasm capacity; explicit resource failure required. |
| G23 shared bidi/direction/caret contract, not shaping alone | **holds within stated limits** | Script fonts and real host tests required. |
| G24 explicit canvas adaptation to contrast/motion preferences | **holds within stated limits** | Canvas must repaint; forced-visible duplicates require tests. |
| G25 existing router plus history solves Back/deep links | **holds within stated limits** | One routing mechanism; Back cannot inherit Jet close veto or guaranteed persistence. |
| G26 shared focus identity solves tab order | **holds within stated limits** | DOM order, modal behavior, escape and virtual materialization need real AT/keyboard tests. |
| G27 real editor plus viewport mapping solves mobile keyboard | **holds within stated limits** | Trusted focus, occlusion and selection required; browser keyboard/auto-zoom remain external. |
| G28 semantic print projection solves printing | **holds within stated limits** | Full content and no secrets specified; pagination, services and large-document cost unproved. |
| G29 semantic metadata supports named extensions only | **holds within stated limits** | No generic canvas repaint compatibility or translation guarantee. |
| G30 specimens prove the proposed framework works | **fails** | Equal 1,000-row/state specimens are comparison notation, not an implementation or benchmark. |
| G31 dispatcher retention and ordered session lifecycle | **holds within stated limits** | Startup/failure/cleanup specified, not implemented proof. A local handle alone is insufficient. |
| G32 timely privileged dispatch with explicit deferred denial | **holds within stated limits** | Only trusted-callback requests are promised; arbitrary deferral cannot preserve activation. |
| Both ballot payloads satisfy the exercised schema | **holds** | Requested validator exited 0 for both. |

**Both ballots are fit to present as candid direction choices with explicit owner amendments and residual costs. They are not fit to present as a passing design, a completed fix, or proof that unrestricted parity can be delivered.**
