# GUI foundations probe

## What I built
I built one real Jet package for a small note-taking app. It writes and reads a Unicode note, renders a reactive sidebar/editor/settings tree, exposes buttons and roles, moves focus with `Tab`, shows shortcut labels, and hand-codes theme, undo, redo, and save state. A separate GTK host fixture and focused missing-API and reactive-codegen fixtures test native windows, dialogs, fonts, input, accessibility, packaging, and mobile targets.

Files under `pkg/`: `package.jet`, `note_app.jet`, `native_window_attempt.jet`, `missing_*.jet`, and the `reactive_*_smoke.jet` fixtures.

## What worked

- **Package check:** works. `env JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-gui/store scripts/agent/jet-env jet check /home/nate/.cache/jet-luna/dx3/area-gui/pkg/note_app.jet` returned `check: passed` in 112 ms. It emitted one non-blocking `L0520` warning for `IOError` display.
- **Release host run:** works. `env JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-gui/store JET_UI_HEADLESS=1 scripts/agent/jet-env jet run --release /home/nate/.cache/jet-luna/dx3/area-gui/pkg/note_app.jet` printed `loaded bytes: 54`, `focus initial: Save`, `focus after Tab: Editor`, `initial renders: 1`, `edited renders: 5`, `undo restored: true`, `redo depth: 1`, and `saved: true` in 5.56 s.
- **Reactive UI tree:** works. The TUI frame contained the Unicode editor text, sidebar, four buttons, settings state, shortcut labels, and visible accessibility labels. `selected`, `draft`, `mode`, and `settings` updates caused five renders.
- **Typed roles and focus:** works. `node_role` accepts button, text-input, and label roles; `set_focus_group` and `key_event("Tab")` moved focus from `Save` to `Editor`.
- **Unicode file persistence:** works. `core.files` wrote and read `café`, `résumé`, `naïve`, and `中文`; the final save comparison printed `saved: true`.
- **Native artifact:** works for the headless/TUI path. `env JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-gui/store JET_UI_HEADLESS=1 scripts/agent/jet-env jet build --release /home/nate/.cache/jet-luna/dx3/area-gui/pkg/note_app.jet` ended `jet Built build/note_app in 8.5s ✓`.

Representative run output:

```text
loaded bytes: 54
focus initial: Save
focus after Tab: Editor
initial renders: 1
edited renders: 5
theme/settings: light / open
undo restored: true
redo depth: 1
saved: true
```

## Gaps

1. **area-gui-G1 — `impossible`:** Android and iOS GUI target artifacts are unavailable. `jet build --release --target android .../note_app.jet` and the corresponding `--target ios` command both returned `Error [E3302]: Target \`android\`/\`ios\` is not available` (3.97 s and 1.79 s). This blocks mobile delivery and is shared with every target-bound area. A library author must install target standard libraries and provide each platform host outside Jet; this probe did not do so.

2. **area-gui-G2 — `impossible`, `boilerplate`:** There is no typed native open/save file-dialog capability. `jet check .../missing_dialog_api.jet` returned `Error [E1004]: \`core.ui\` has no item \`file_open_dialog\`` and listed only the fixed UI items ending in `button` (2.94 s). This blocks ordinary desktop document workflows and is shared with office-document automation and education tools. The workaround is OS/GTK/Qt FFI plus hand-written cancellation, filter, path, and authority conversion.

3. **area-gui-G3 — `impossible`, `call-site`:** The UI event/state surface has no clipboard, text selection, IME, or drag-and-drop capability. `jet check .../missing_gui_api.jet` returned `Error [E1004]: \`core.ui\` has no item \`clipboard_read\`` and exposed only `key_event` and `resize_event` among input constructors (1.95 s). A TUI `TextInput` node is not a native editing protocol. The workaround is a host FFI event loop and manually maintained selection/IME/clipboard state. This is shared with data, education, and office applications.

4. **area-gui-G4 — `impossible`, `boilerplate`:** There is no font discovery, fallback, OpenType feature, glyph-run, or script/Bidi shaping capability. `jet check .../missing_font_api.jet` returned `Error [E1001]: There is no core module \`core.font\`` (2.03 s); the successful app only stores and displays Unicode scalar strings. Correct Arabic, Indic, combining-mark, emoji, and fallback rendering requires external fontconfig/FreeType/HarfBuzz FFI and a hand-written glyph bridge. This is shared with typography, publishing, office documents, and education.

5. **area-gui-G5 — `impossible`, `call-site`:** There is no typed shortcut registration and action-dispatch capability. `jet check .../missing_shortcut_api.jet` returned `Error [E1004]: \`core.ui\` has no item \`shortcut\`` and the same fixed public-item list (1.89 s). The app can label `Ctrl+S` and map strings in ordinary library code, but cannot bind that shortcut to a host action through `core.ui`. A library author must decode host events and maintain a manual action table. This is shared with education and office applications.

6. **area-gui-G6 — `impossible`, `call-site`:** There is no accessible-name/description/state capability beyond a small role enum. `jet check .../missing_a11y_api.jet` returned `Error [E1004]: \`core.ui\` has no item \`aria_label\`` and listed role constructors but no naming API (1.95 s). The app therefore prints an accessibility-label line as visible text rather than attaching names to controls. External native accessibility bindings are the workaround. This is shared with education and office applications.

7. **area-gui-G7 — `impossible`, `boilerplate`:** There is no Jet desktop bundle/installer command. `jet package --help` returned `Error [E2101]: \`package\` isn't a jet command` (1.84 s). `jet build` emits a native executable, but a library author must invoke platform bundlers, signing, metadata, and updater tooling separately. This blocks the requested packaged host build.

8. **area-gui-G8 — `defect`:** A conditional inside a reactive render closure moves the signal into the generated closure, then rejects a later update. `jet run --release .../reactive_conditional_smoke.jet` returned the Jet ICE in 9.65 s; compiling its generated Rust exposed `error[E0382]: borrow of moved value: __jet_selected` at the later `.set(1)`. This affects any reactive UI with conditional view logic. The current workaround is to avoid the conditional in the closure or precompute the marker.

9. **area-gui-G9 — `defect`:** Calling a user view helper from a reactive render closure emits invalid Rust. `jet run --release .../reactive_view_smoke.jet` returned the Jet ICE in 9.03 s; generated-Rust compilation exposed `error[E0277]` (`?` in a closure returning `()`) and `error[E0308]` (`expected ()`, found `Result`). The workaround is to inline the tree in the closure, as this app does.

## Friction

- The default evaluator cannot execute the reactive program: `env JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/area-gui/store JET_UI_HEADLESS=1 scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/area-gui/pkg/note_app.jet` returned `Error [E0956]: core.reactive.signal() isn't supported by the current evaluator yet` in 1.98 s. `jet run --release` is the working path.
- The GTK host fixture reached `Error [E3201]: C library \`gtk4\` was not found` in 2.12 s. This is external host setup, not counted as a language gap; the manifest needs the C dependency and a display-capable host.
- The author hand-wrote four button callbacks, a focus-group list, shortcut mapping, visible accessibility text, and two undo/redo stacks. These are repeated library code, not a new type-system need.
- The TUI backend gives deterministic frame lines and focus behavior, but no real display, native IME, system font selection, or OS dialog was available in this probe.

## Defects

- `pkg/reactive_conditional_smoke.jet:8-13`: release run ICE; generated Rust has `E0382` because `selected` is moved into the render closure before `selected.set(1)`.
- `pkg/reactive_view_smoke.jet:14-16`: release run ICE; generated Rust uses `?` in a `()` closure and returns `Ok(...)`, producing `E0277` and `E0308`.

## Battery notes

- **Headless app shell:** mount one tree through null, TUI, and GTK backends; prove deterministic frame output and native host startup. **Needs runtime/host support.**
- **Reactive editor state:** signal updates, conditional branches, helper views, stable identity, and render counts. **Pure library-shaped code, but compiler regression coverage is required.**
- **Text editing input:** cursor, selection, IME composition, clipboard copy/paste, and drag/drop. **Needs runtime/host support.**
- **Typography:** system-font discovery, fallback, Arabic/Indic shaping, combining marks, emoji clusters, and cursor hit testing. **Needs font/shaping support.**
- **Dialogs and commands:** open/save cancellation, filters, menus, shortcuts, window lifecycle, and tray fallback. **Needs native host support.**
- **Accessibility:** attached names, roles, states, focus order, and keyboard traversal on every backend. **Needs native accessibility support.**
- **Persistence policy:** Unicode file save, theme/settings state, undo/redo, and recovery after reload. **Pure library code.**

## Verdict

The note app is buildable today as a checked, release-built, headless/TUI package.

The complete native desktop/mobile app is not buildable with today's surfaced capabilities: input services, dialogs, typography, semantic names, packaging, and mobile targets are missing.

Two ordinary reactive compositions also ICE in native code generation; inlining and avoiding conditional render logic were required.

The probe did not run a real GTK display, Android/iOS artifact, native IME, system font shaper, or installer.
