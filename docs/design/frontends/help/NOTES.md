# Jet help TUI — design options

Surface: `jet ?` elevated from flat `--help` (card #159 aliased `jet ?` to
help) into a full-screen, keyboard-first help app. Owner goal: clean,
beautiful, navigable; helps people find what they want faster; makes
exploration enjoyable.

Flows shown per option: home · fuzzy search · command detail with runnable
examples · category browse · exit-to-shell with command prefilled.

Shared transplants: **lazygit** two-pane list/detail; **fzf** fuzzy
subsequence match + prefill-on-enter; **atuin** exit dropping the choice onto
the shell line (not executed); **glow** man-page rendering in the detail pane;
**gh** command groups.

Alt-screen app, closes clean. 80-col safe: detail stacks under list below 90
cols.

---

## A · Carbon — dense two-pane

Signature: status band up top (match count, active command), engineered
list/detail split. Maximum info per screen; examples inline with a green ❯.
Best for power users who already know the tool and want speed.

```
 jet ?  help browser              ↑↓ / search  ? keys  q quit
 ⌕ _                                    filter 26 commands
 ┌ BUILD & RUN ────────┬ jet run ─────────────────────────┐
 │▸run    build & run  │ build the current program, run it│
 │ build  compile bin  │ USAGE  jet run [FILE] [--watch]   │
 │ test   run tests    │ EXAMPLES                          │
 │ CODE QUALITY        │  ❯ jet run            enter→shell │
 │ check  type-check   │  ❯ jet run app.jet --watch        │
 └─────────────────────┴───────────────────────────────────┘
 enter use · tab pane · / search · c categories · q quit
```

Exit: `webapp ▸ jet build --release_`  (prefilled, not run.)
NO_COLOR: selection = ▸ + reverse video; fuzzy matches underlined.

---

## B · Paper — editorial reading view

Signature: fading hairline under every section label; real heading font in
the detail pane; whitespace does the work. Reads like Stripe docs. One flare
accent = fuzzy match chars. Best for newcomers and discovery.

```
 jet ?  help                      ↑↓ / search  ? keys  q quit
 ⌕ _                                    filter 26 commands
 BUILD & RUN            │ jet run
  run   build & run     │ Build the current program and run
  build compile binary  │ it — fastest path from code to out.
  test  run tests       │ USAGE ─────────────
 CODE QUALITY           │  jet run [FILE] [--watch]
  check type-check      │ EXAMPLES
  fmt   format source   │  [ ❯ jet run ]        enter→shell
                        │  [ ❯ jet run app.jet --watch ]
 enter use · / search · c categories · q quit
```

Exit: `webapp › jet build --release_`  (dark shell = user's terminal.)
NO_COLOR: matches underline, selection = left bar + bold. `--dark` variant.

---

## C · Pulse — one glow, follows focus

Signature: exactly one hot→hot2 glow — the selected command, then the chosen
example, then (on exit) the command riding out to the shell. Fuzzy matches
glow inside the word. Everything else cool. Best as a showpiece help.

```
 jet ?  help                      ↑↓ / search  ? keys  q quit
 ⌕ biuld                                3 of 26 match
 MATCHES               │ jet build
 ▸build  (b·ild glow)  │ compile the current program to a bin
  bundle assets        │ USAGE jet build [--release] [--target]
  rebuild clean        │ EXAMPLES
                       │  ❯ jet build
                       │ [❯ jet build --release]  ← glows
 enter use · esc clear · ↑↓ move · q quit
```

Exit: `webapp ▸ jet build --release_`  (command keeps its glow out.)
Truecolor gradient; 16-color = solid bright-red on the one element.
NO_COLOR: selection reverse-video, matches underlined.
```
