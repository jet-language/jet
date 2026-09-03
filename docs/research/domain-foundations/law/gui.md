# gui

## Ratified

- **D-UITREE1** — the same sigil constructs named-payload UI variants as `.Variant{…}` / `Type.Variant{…}` with no new token. The spelling is explicitly ratified-but-unbuilt. — `crates/jet-foundation/src/Syntax/package_files.rs:69-80`; `docs/spec/syntax-decisions.md:4176-4193`
- **D-ONCE-UITREE1=C** — the UI-tree spelling is marked “ratified and unbuilt” until the architecture result; the shipped callable surface remains `ui.*`. — `tower` (`D-ONCE-UITREE1` outcome C)
- **D-UI-MOUNT1=A** — `ui.mount(backend, tree)` runs “measure → layout → paint → present?” while the three manual stages stay public. — `tower` (`D-UI-MOUNT1` outcome A)
- **D-UI-NODE-ID1=C / D-UI-EVT-DISP1=E** — identity defaults to render path with optional author key; dispatch uses an O(1) stable node-slot table, clears on unmount, and never uses CSS selectors. — `docs/spec/syntax-decisions.md:6664-6669`; `tower` (outcomes C/E)
- **D-UI-EVT-SET1=D / D-WEB-CLICK-PORT1=D** — click/activate is the portable core event; richer events require an explicit capability module, and unsupported backends may register without firing. — `docs/spec/syntax-decisions.md:6664-6672`; `tower` (outcomes D)
- **D-EVENT1 / D-EVENT2=A** — one typed `core.event` family provides `Event`, `Hook`, `Subscription`, `EventScope`, `EventPolicy`, `EventTrace`, and bounded asynchronous events; source sugar is reserved. — `docs/spec/syntax-decisions.md:3404-3425`
- **D-DATARACE1=C** — reactive boxes use lock-ordered `Arc` storage; `#Local` rejects task/channel/parallel crossings and `#Shared` records synchronized upgrades. — `docs/spec/syntax-decisions.md:4179-4186`
- **D-LAYOUT-FACTS1=B** — focused layout inspection is a compiler fact (`T.$layout`); full reflection is `T.reflect()`. — `tower` (`D-LAYOUT-FACTS1` outcome B)
- **D-CASE-CHROME1=C / D-CASE-PROSE1=A** — UI labels, headers, and buttons use Title Case; summaries and explanatory prose use sentence case. — `docs/spec/ui-case-law.md:1-60`; `tower` (outcomes C/A)
- **D-NATIVEUI3=A** — `jet build --target=ios|android` is intended to compile the same reactive app natively, with first-party signing/upload. — `tower` (`D-NATIVEUI3` outcome A)
- **D-NATIVEUI-ANDROID1=C** — Android has one backend protocol, direct JNI Views by default, and a Compose adapter over the identical tree/events/accessibility semantics. — `tower` (`D-NATIVEUI-ANDROID1` outcome C)
- **D-NATIVEUI-DEV1=C** — one resident `NativeDevHost` serves full-app and preview roots; both use production backends, state identity, and diagnostics. — `tower` (`D-NATIVEUI-DEV1` outcome C)
- **D-SIGNAL1 / D-RENDERTGT1/2 / D-STYLESHAPE1 / D-MOTIONTIME1 / D-LAYOUT1 / D-A11Y1** — the operative stack text fixes `Signal.get/set`, `Computed`, `Effect`, pure std runtime, measure/layout/paint backends, flat `Style`, injectable `Clock` motion, constraint `Layout`, and native-widget FFI. — `docs/spec/syntax-decisions.md:4176-4199`
- **D-A11YGATE1** — accessibility issues are opt-in `jet lint --a11y` lints, E2930/E2931. — `docs/spec/syntax-decisions.md:3039-3041`

## Shipped

- The callable `core.ui` surface has `ui.text`, `ui.button`, `ui.box`, `ui.mount`, plus null/TUI backends and typed node/geometry/role/event constructors; the dot-construction spelling is not shipped. — `docs/spec/syntax-decisions.md:4188-4193`; `crates/jet-foundation/src/Syntax/core_calls.rs:4168-4266`
- `#Reactive` is an explicit marker; `Signal<T>.get()` registers an observer and the runtime lowers reactive updates. — `crates/jet-foundation/src/Syntax/core_surface.rs:426-430`
- Core event, mount, backend, node, text/button, key/resize event, role, color, and ARIA-role constructor rows exist in the syntax/call registry. — `crates/jet-foundation/src/Syntax/core_calls.rs:4168-4266`
- The GUI probe built and release-ran a Unicode note app through the headless/TUI path: reactive renders, focus movement, roles, buttons, undo/redo, and Unicode persistence all worked; native artifact generation also passed. — `~/.cache/jet-luna/dx3/area-gui/probe.md:8-29`
- Portable click identity/dispatch is the shipped direction: render path by default, optional key, O(1) slot dispatch, click-only portable core, richer capability modules. — `docs/spec/syntax-decisions.md:6664-6672`
- `#Layout(c)` maps to `#[repr(C)]`, preserves field order, and rejects growable fields; this is available for native interop and register models. — `crates/jet-foundation/src/Syntax/package_files.rs:349-356`
- The web/native entry law returns an `App`; `jet dev` serves that value with watch/reload/hot swap, while `fn dev()` remains the expert override. — `docs/spec/syntax-decisions.md:5848-5861`

## Undecided

- Should `core.ui` add typed file-open/save dialogs with cancellation, filters, path authority, and result/error semantics?
- Should the UI capability family add typed clipboard, selection, IME composition, and drag-and-drop protocols, rather than requiring host FFI and manual state?
- Should `core.font` provide font discovery, fallback, OpenType features, glyph runs, and script/Bidi shaping with a stable cross-backend contract?
- Should `core.ui` add typed shortcut registration and action dispatch, and how should conflicts and platform key maps be represented?
- Should accessible names, descriptions, states, and relationships become typed node properties rather than only role constructors and visible text?
- Should Jet ratify one desktop bundle/installer surface (`jet package`) for metadata, signing, and updates?
- Should conditional reactive render closures and user view-helper calls be repaired so ordinary compositions compile without ownership or closure-result ICEs?
- Should the open/spec-only layout, ownership, and reactive stack records receive a complete implementation contract, while preserving the already stated `Signal`, `Style`, `Layout`, component, and backend directions?
- Should Android/iOS target delivery implement the already ratified native target and host laws, or is a new target boundary required for a missing toolchain?

## Conflicts

- `D-ONCE-UITREE1=C` says dot-construction is ratified-but-unbuilt and the callable `ui.*` API is the current surface. A ballot must not present `.Button{…}` as shipped or silently withdraw it.
- `D-NATIVEUI3=A` and `D-NATIVEUI-ANDROID1=C` already choose same-source mobile native UI, JNI View default, Compose adapter, and first-party shipping. The probe's E3302 is an implementation/toolchain blocker, not permission to choose a second UI model. — `~/.cache/jet-luna/dx3/area-gui/probe.md:31-35`
- The portable event rulings deliberately limit core to click/activate and require explicit capability modules for richer events. Clipboard, IME, drag, hover, and platform-specific input must not be added as silent core aliases. — `docs/spec/syntax-decisions.md:6664-6672`
- D-UI-NODE-ID1/D-UI-EVT-DISP1 forbid CSS-selector identity and require stable O(1) slot dispatch; a proposed event API using selectors or a second dispatch table conflicts with them.
- The source stack already names `Style` as one flat record, motion as injectable `Clock`, `Layout` as a constraint value with a first-party simplex solver, and components as copy-in-and-own. A ballot proposing another style, time, layout, or ownership model would reopen those directions. — `docs/spec/syntax-decisions.md:4193-4199`
- `D-REACT1`, `D-LAYOUT-CTOR1`, and `D-OWNCOMP1` are open/spec-only records in Tower despite appearing in the source stack heading. Keep their stated direction visible, but do not present them as selected ballot outcomes. — `tower` (IDs open/spec-only); `docs/spec/syntax-decisions.md:4176-4199`
- The GUI probe's G8/G9 are compiler defects (E0382/E0277/E0308), not evidence for inlining as a permanent UI rule. — `~/.cache/jet-luna/dx3/area-gui/probe.md:47-49,58-61`
- `jet package` is absent (E2101), while `jet build` already produces a native executable; packaging/installer behavior remains a real question and must not be claimed by the native build law. — `~/.cache/jet-luna/dx3/area-gui/probe.md:45-46`
- `D-A11YGATE1` provides a lint gate, not typed accessible names; the missing `aria_label` API remains a genuine gap. — `docs/spec/syntax-decisions.md:3039-3041`; probe `~/.cache/jet-luna/dx3/area-gui/probe.md:41-43`
