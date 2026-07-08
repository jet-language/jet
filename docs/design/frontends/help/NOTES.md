# Help frontend archetypes — notes

Three surfaces for `jet ?`. Shared TUI palette + copy rules from
`../DESIGN-BRIEF.md`. Commands/flags/examples are real (`jet run/test/build/
fmt/repl`, `examples/features/`). Error-code pages render the verbatim
diagnostic from `tests/ui/arg_type_mismatch.stderr` (E0112, I4).

Core-loop test (one sentence each; none match):
- **palette** — summon, type three chars, Enter, back in the shell prefilled.
- **browser** — explore the whole tool like a reference book.
- **tasks** — say what you want to do, get the exact commands.

Axis = speed vs depth vs intent: fast recall / full reference / goal-first.

---

## 1. palette.html — instant overlay (fzf)

**Core loop:** summon → type three chars → Enter → back in the shell with the
command prefilled.

**Rationale.** `jet ?` drops a fuzzy finder over the shell — no alt-screen,
shell stays visible dimmed behind. Search spans commands, flags, and examples;
Tab peeks a command's flags inline; Enter prefills the shell line (never runs
it). For the user who knows roughly what they want and wants it in two seconds.

**Transplants.** fzf overlay + fuzzy match. navi prefill-the-shell-line (don't
execute). atuin drop-over-shell (no takeover).

**Risks.** Prefill-not-run must be unmistakable, or users fear it executed.
Overlay redraw must restore the shell exactly on Esc. Fuzzy ranking quality is
the whole UX. NO_COLOR: `>` selection + `[ ]` match brackets.

```
web-api ❯ jet ?
┌─ find a command ──────────────────────────────────────────┐
│ > run                                         5 shown │
├───────────────────────────────────────────────────────────┤
> jet [run] <file>      run a file, rebuilding if it changed   (selected)
│ jet [run] --watch     rerun on every save                 │
│ jet [run] --release   run an optimized build              │
├───────────────────────────────────────────────────────────┤
│ example  jet run examples/features/basics/hello.jet       │
│ ↑↓ move · ⏎ prefill shell · ⇥ flags · Esc close           │
└───────────────────────────────────────────────────────────┘
→ Enter closes overlay, shell line becomes:  web-api ❯ jet run _
```

---

## 2. browser.html — two-pane reference app (lazygit)

**Core loop:** explore the whole tool like a reference book, category → command
→ detail.

**Rationale.** Alt-screen. Left = category tree + commands. Right = full
man-depth page: usage, description, flags table, runnable examples, see-also.
Breadcrumbs, per-panel focus. Reference also covers error codes and syntax —
each code page renders the verbatim diagnostic. For learning the tool in depth
or looking something up precisely.

**Transplants.** lazygit fixed panels + focus + breadcrumbs. man/info depth.
tldr examples block at the bottom of each page.

**Risks.** Alt-screen takeover (unlike palette) — heavier to invoke, needs
clean restore + in-app scroll. Content is large to author/maintain. Enter on an
example still prefills the shell after exiting. NO_COLOR: `▾/▸` tree glyphs + `>`
selection.

```
jet ? reference              Build & run › run › usage
┌─ commands ──────────┬─ jet run ────────────────────────────┐
│ ▾ Build & run        │ Usage                                │
│ >  run              │   jet run <file> [flags]               │
│    build            │ Run a file, rebuilding first if its   │
│    test             │ sources changed.                      │
│ ▸ Project           │ Flags                                │
│ ▸ Reference         │   --watch     rebuild + rerun on save │
│    error codes      │   --release   optimized build         │
│    syntax           │   --quiet     hide the build ledger   │
└─────────────────────┴──────────────────────────────────────┘
↑↓ move · → into · ⏎ prefill shell · / search · q quit
```

---

## 3. tasks.html — goal-first explorer (navi/tealdeer)

**Core loop:** say what you want to do, get the exact commands to do it.

**Rationale.** Home lists outcomes ("run a file and rebuild on save", "add a
dependency", "understand an error message"), not command names. Pick one → a
short numbered recipe of real, runnable commands with one-line whys. A goal can
be "understand an error", whose recipe leads with the verbatim diagnostic then
tells you how to act on it. For beginners who don't yet know the command names.

**Transplants.** tealdeer/tldr task-first. navi recipes + placeholder prefill.
trogon goal→command building.

**Risks.** Goal list is curated content — must stay small and outcome-worded, or
it becomes a second command list (I8). Recipes must stay real/tested (golden
candidates). Numbered steps only where order truly matters (they do here).
NO_COLOR: step numbers + `>` selection.

```
jet ? what do you want to do?
┌───────────────────────────────────────────────────────────┐
│ > run a file and rebuild on save                          │
│   start a web app                                         │
│   add a dependency                                        │
│   understand an error message                             │
└───────────────────────────────────────────────────────────┘

open "run a file and rebuild on save":
│ 1  start it, watching for changes                         │
│ >    jet run --watch src/main.jet                         │
│ 2  in another shell, rerun tests on every save            │
│      jet test --watch                                     │
│ Every save rebuilds and reruns. Ctrl-C stops the watcher. │
```
