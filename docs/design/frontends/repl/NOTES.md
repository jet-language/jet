# REPL frontend archetypes — notes

Three genuinely distinct core loops for `jet repl`, explored below as the
ballot options for D-FE-REPL1. The owner ratified **D — Hybrid** (2026-07-08);
its follow-up gates D-FE-REPL-DOCS1 (`?name` docs, =B) and D-FE-REPL-RERUN1
(replay semantics, =A) are ratified too. Feature truth is now
`Source/REPL/mod.rs` + its siblings (`Render.rs` turn-gutter/pin-rail/fold-
marker rendering, `RerunPlan.rs` replay-plan semantics, `Docs.rs` `?name`
lookup, `Terminal.rs` raw-mode + key decoding, `Interactive.rs` the TTY event
loop) — banner, `N user> ` prompt (interactive TTY) / `user> ` (non-TTY
floor), `... ` continuation, `value : Type` echo (D-REPL16=B),
`:help/:quit/:reset/:load/:type` plus the notebook/workspace layer commands
(`:turns/:pin/:fold/:unfold/:rerun`, `^P/^F/^R/^B`), NO_COLOR. This file stays
as the archetype-comparison record; it does not track hybrid's exact rendered
shape line-for-line post-implementation. Diagnostic verbatim from
`tests/ui/arg_type_mismatch.stderr` (E0112).

Core-loop test (one sentence each; none match):
- **line** — type an expression, read its value, keep typing.
- **notebook** — build a session of blocks you fold, rerun, edit, pin.
- **workspace** — evaluate on the left, watch bindings evolve on the right.

---

## 1. line.html — enhanced classic line REPL

**Core loop:** type expression → read value → continue, one line at a time.

**Rationale.** Lowest-ceremony surface; the default. Adds modern editing
(ghost suggest, completion menu, inline `?name` docs, live highlight) without
changing the mental model. Scrollback stays plain text — settled values never
re-render, so a pipe/log is clean.

**Transplants.** ptpython/IPython ghost autosuggestion from history + completion
menu + docstring. Python 3.14 REPL live highlight. `?name` docs = IPython `?`.

**Risks.** `?name` symbol-docs is new syntax (not in REPL.rs) — needs a ballot
row. Ghost suggest must never read as typed input (dim only) — NO_COLOR loses
the dim cue, so ghost is simply not shown when color is off (history still via
up-arrow). Completion menu redraw must not scroll settled output.

```
Jet 0.1.0 — interactive REPL  (type :quit to exit, :help for commands)

user> nums :: [1, 2, 3, 4, 5]
[1, 2, 3, 4, 5] : List
user> nums.map((n: Int) => (n * n))
[1, 4, 9, 16, 25] : List
user> nums.fil
      ┌──────────────────────────────────────────────┐
      > filter      (f: Fn) -> List   keep items where     <- selected
      │ filter_map  (f: Fn) -> List   map, drop None   │
      └──────────────────────────────────────────────┘
      Tab accept · up/down move · ? docs for selected
user> show("hi")
Error [E0112]: `show` wants Int (a whole number) for argument 1, but this is String (text)
  --> tests/ui/arg_type_mismatch.jet:5:10
    |
  5 |     show("hi")
    |          ^^^^
 Why: every argument must match its parameter's type
 Fix: use Int (a whole number) here
1 problem found
user> _
```

---

## 2. notebook.html — block / session REPL

**Core loop:** build up a session of living blocks you fold, rerun, edit in
place, and pin — revisit, don't scroll away.

**Rationale.** Every turn is command + output + status + timing in one
addressable block. Fold long blocks, pin a value to keep it on screen, edit +
rerun for a reproducible session instead of append-only history. Big outputs
page inside the block (browsable table) rather than flooding the screen.

**Transplants.** Warp blocks (bookmark→pin, rerun, copy-output, stable id,
status+duration header). jupyter console / nushell tabular structured output.

**Risks.** Edit-in-place reruns must recompute downstream bindings or mark them
stale (session is ordered) — real semantics work, ballot it. Block frames cost
vertical space; fold-by-default for old blocks. NO_COLOR relies on status words
(ok/error/edited/folded) — already textual, fine.

```
╭─ 1 · ok · 2ms ────────────────────────────── fold  rerun  pin
│ nums :: [1, 2, 3, 4, 5]
│ [1, 2, 3, 4, 5] : List
╰──────────────────────────────────────────────────────────
📌 ╭─ 1 · ok · pinned ───────────────────────────────────────
│ nums : List = [1, 2, 3, 4, 5]
╰──────────────────────────────────────────────────────────
╭─ 2 · ok · folded ── nums.map(…) → [1,4,9,16,25] ─ unfold
╭─ 4 · ok · 1ms ────────────────────────────── fold  rerun  pin
│ nums.map((n: Int) => (n * n))
│ idx  value        [U] · 5 items
│  0    1     <- selected row · j/k scroll · → expand · y copy
│  1    4
│  ...
╰──────────────────────────────────────────────────────────
╭─ 5 · error · E0112 ─────────────────────────── edit  rerun
│ Error [E0112]: `show` wants Int (a whole number) ...
│  Fix: use Int (a whole number) here
╰──────────────────────────────────────────────────────────
```

---

## 3. workspace.html — pane workspace

**Core loop:** evaluate on the left; watch state evolve in a live bindings
inspector on the right, docs on demand.

**Rationale.** Alt-screen split. Left = session scroll. Right = every binding
as `name : Type = value`, updated the instant a step lands, with the changed
row marked. Kills `:type` round-trips; surfaces the memory model (moved/owned)
live. `?` splits the right column for docs without covering the session.

**Transplants.** lazygit fixed panes + focus keys. Debugger "variables" pane
(watch state evolve). Alt-screen from any full-TUI.

**Risks.** Alt-screen means scrollback isn't native — needs an in-app pager and
a clean exit restore. Inspector value formatting must truncate big values
(shown wrapped). Non-TTY can't alt-screen → falls back to the line archetype's
plain transcript. NO_COLOR keeps panes (box-drawing) + `◂ new this step` glyph.

```
jet repl · workspace ───────────────────────── ? docs   :help   :quit
┌─ session ───────────────────────────┬─ bindings ────────────────┐
│ user> doubled := nums.map(          │ nums    : List = [1,2,3,  │
│   (n: Int) => (n * 2))              │                    4,5]   │
│ [2, 4, 6, 8, 10] : List             │ total   : Int  = 15       │
│ user> _                             │ doubled : List = [2,4,6,  │
│                                     │                    8,10]  │
│                                     │ ◂ new this step           │
│                                     │ 3 bindings · 0 moved      │
└─────────────────────────────────────┴────────────────────────────┘
⏎ eval   ⇥ complete   ^L clear session   ^B focus bindings
```

---

## hybrid.html — curated line REPL with layered depth

**Core loop:** type an expression, read its value, keep typing — and every turn
stays quietly addressable, so you can pin, fold, or rerun any of them, or press
`^B` for live bindings, without leaving the line.

The line REPL is the ground floor; nothing above changes the mental model of
"type, read, continue." Each layer is reachable by one key and invisible until
asked for.

| Source option | Transplanted aspect |
|---------------|--------------------|
| line | Foundation: plain-text scrollback, `user>`, ghost autosuggest, completion menu, inline `?name` docs, live highlight on the editing line. |
| notebook | The addressable turn — a dim turn-number gutter, `^P` pin / `^F` fold / `^R` rerun, and big outputs paging in place as a browsable table. |
| workspace | The live bindings inspector (`name : Type = value`, changed-row marker), toggled on demand with `^B`. |

**Deliberately left out**
- notebook's heavy per-turn block frames (`╭─ 1 · ok · 2ms ─╮`) — they cost
  vertical space and fight the flat scroll that is the whole point of a line
  REPL. Replaced by a one-character dim gutter number.
- notebook's structured table as the default output — paging engages only for
  big collections; a scalar or short list still prints as one plain value line.
- workspace's always-docked split — the bindings pane is opt-in (`^B`), not the
  resting layout, so the default surface is a single clean column.
- Pinned block showing full output — a pin is one line (`name : Type = value`)
  above the prompt, not a re-hosted frame.

**Risks**
- `?name` symbol-docs is new syntax (not in REPL.rs) — carries an owner ballot
  row.
- notebook edit-in-place rerun must recompute or stale downstream bindings —
  semantics need a ballot before `^R` on a past turn ships.
- `^B` alt-screen must restore the scrollback cleanly on toggle-off; `^P/^F/^R`
  must not collide with readline bindings.
- Turn-gutter width must stay stable as indices grow (align output under it).

```
Jet 0.1.0 — interactive REPL  (:quit, :help, ^B bindings)

1 user> nums :: [1, 2, 3, 4, 5]
  [1, 2, 3, 4, 5] : List
2 user> nums.map((n: Int) => (n * n))
  [1, 4, 9, 16, 25] : List
📌 total : Int = 15                          turn 3 · unpin ^P
────────────────────────────────────────────────────────────
4 user> data.rows()
  ⋯ 42 rows folded · [Row] · unfold ⏎
5 user> _

^B opens the bindings pane on the right:
┌─ session ──────────────────┬─ bindings ───────────────┐
│ 5 user> _                  │ nums    : List = [1,2,3…] │
│                            │ total   : Int  = 15       │
│                            │ ◂ new this step           │
└────────────────────────────┴───────────────────────────┘
NO_COLOR / non-TTY → pane folds away, plain line transcript, turn digits stay.
```
