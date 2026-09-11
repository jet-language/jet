# Probe area-gui — Desktop and mobile apps

Read `~/.cache/jet-luna/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: gui.

## Build this

A desktop note-taking app as one Jet package: a native window with a menu, a sidebar list, a text editor with proper Unicode text shaping and a system font, keyboard shortcuts, a file-open dialog, undo/redo, dark/light theme, a settings pane bound to reactive state, accessibility labels, and a packaged build for the host OS; then report what a mobile (Android/iOS) build would need. Start from examples/features/ui/** (native_linux, reactive, layout, a11y, component_kit, typed_style, view_tree), examples/features/io/app_config.jet.

## Answer these

1. Is text shaping and font discovery library code or a runtime bridge?
2. What does the reactive/ui layer lack for a real editor (selection, IME, clipboard, drag)?
3. What does a GUI battery need?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/desktop-apps/`, `~/.cache/jet-luna/dx2/typography-publishing/`, `~/.cache/jet-luna/dx2/education-teaching/`, `~/.cache/jet-luna/dx2/office-document-automation/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/area-gui/probe.md`, `gaps.json`, `batteries.json`, and the code under `~/.cache/jet-luna/dx3/area-gui/pkg/`. Gap ids start with `area-gui-G`.
